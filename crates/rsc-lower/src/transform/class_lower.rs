//! Class definition lowering.
//!
//! Handles registration of class types, lowering of class definitions to
//! struct + impl blocks, constructors, and methods.

use rsc_syntax::ast;
use rsc_syntax::rust_ir::{
    ParamMode, RustBlock, RustExpr, RustExprKind, RustFieldDef, RustImplBlock, RustItem,
    RustMethod, RustParam, RustSelfParam, RustStructDef, RustTraitImplBlock, RustType,
};

use crate::context::LoweringContext;
use crate::ownership::{self, UseMap};

use super::{Transform, collect_generic_param_names, lower_type_params};

impl Transform {
    /// Register a class definition in the type registry during the pre-pass.
    pub(super) fn register_class_def(&mut self, cls: &ast::ClassDef, ctx: &mut LoweringContext) {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(cls.type_params.as_ref());
        let fields: Vec<(String, rsc_typeck::types::Type)> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ast::ClassMember::Field(f) => {
                    let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                        &f.type_ann,
                        &self.type_registry,
                        &generic_names,
                        &mut diags,
                    );
                    Some((f.name.name.clone(), ty))
                }
                _ => None,
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        self.type_registry.register(cls.name.name.clone(), fields);
    }

    /// Lower a class definition to a struct + impl block(s).
    ///
    /// Returns multiple `RustItem`s: one struct, one inherent impl, and
    /// optionally trait impl blocks for each interface the class implements.
    #[allow(clippy::too_many_lines)]
    // Class lowering coordinates struct, constructor, methods, and trait impls;
    // splitting would fragment the coherent pipeline.
    pub(super) fn lower_class_def(
        &self,
        cls: &ast::ClassDef,
        exported: bool,
        ctx: &mut LoweringContext,
    ) -> Vec<RustItem> {
        let mut items = Vec::new();
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(cls.type_params.as_ref());
        let type_params = lower_type_params(cls.type_params.as_ref());

        // 1. Build the struct definition from class fields
        let fields: Vec<RustFieldDef> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ast::ClassMember::Field(f) => {
                    let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                        &f.type_ann,
                        &self.type_registry,
                        &generic_names,
                        &mut diags,
                    );
                    let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                    Some(RustFieldDef {
                        public: f.visibility == ast::Visibility::Public,
                        name: f.name.name.clone(),
                        ty: rust_ty,
                        span: Some(f.span),
                    })
                }
                _ => None,
            })
            .collect();

        items.push(RustItem::Struct(RustStructDef {
            public: exported,
            name: cls.name.name.clone(),
            type_params: type_params.clone(),
            fields,
            span: Some(cls.span),
        }));

        // Collect field names for the constructor's Self { } literal
        let field_names: Vec<String> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ast::ClassMember::Field(f) => Some(f.name.name.clone()),
                _ => None,
            })
            .collect();

        // Collect interface method names for trait impl separation
        let trait_method_names: std::collections::HashSet<String> = cls
            .implements
            .iter()
            .filter_map(|iface_name| self.type_registry.get_interface_methods(&iface_name.name))
            .flatten()
            .map(|sig| sig.name.clone())
            .collect();

        // 2. Build methods
        let mut inherent_methods: Vec<RustMethod> = Vec::new();
        let mut trait_methods: std::collections::HashMap<String, Vec<RustMethod>> =
            std::collections::HashMap::new();

        // Initialize trait method buckets
        for iface in &cls.implements {
            trait_methods.entry(iface.name.clone()).or_default();
        }

        // Lower the constructor
        for member in &cls.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                let method = self.lower_class_constructor(
                    ctor,
                    &field_names,
                    &generic_names,
                    ctx,
                    &mut diags,
                );
                inherent_methods.push(method);
            }
        }

        // Lower methods
        for member in &cls.members {
            if let ast::ClassMember::Method(method) = member {
                let lowered = self.lower_class_method(method, &generic_names, ctx, &mut diags);

                // Check if this method belongs to a trait impl
                if trait_method_names.contains(&method.name.name) {
                    // Find which interface this method belongs to
                    for iface in &cls.implements {
                        if let Some(iface_methods) =
                            self.type_registry.get_interface_methods(&iface.name)
                            && iface_methods.iter().any(|sig| sig.name == method.name.name)
                        {
                            trait_methods
                                .entry(iface.name.clone())
                                .or_default()
                                .push(lowered.clone());
                            break;
                        }
                    }
                } else {
                    inherent_methods.push(lowered);
                }
            }
        }

        // 3. Emit the inherent impl block
        items.push(RustItem::Impl(RustImplBlock {
            type_name: cls.name.name.clone(),
            type_params: type_params.clone(),
            methods: inherent_methods,
            span: Some(cls.span),
        }));

        // 4. Emit trait impl blocks
        for iface in &cls.implements {
            if let Some(methods) = trait_methods.remove(&iface.name) {
                items.push(RustItem::TraitImpl(RustTraitImplBlock {
                    trait_name: iface.name.clone(),
                    type_name: cls.name.name.clone(),
                    type_params: type_params.clone(),
                    methods,
                    span: Some(cls.span),
                }));
            }
        }

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        items
    }

    /// Lower a class constructor to a `fn new(params) -> Self { Self { fields } }`.
    fn lower_class_constructor(
        &self,
        ctor: &ast::ClassConstructor,
        field_names: &[String],
        generic_names: &[String],
        ctx: &mut LoweringContext,
        diags: &mut Vec<rsc_syntax::diagnostic::Diagnostic>,
    ) -> RustMethod {
        ctx.push_scope();

        let params: Vec<RustParam> = ctor
            .params
            .iter()
            .map(|p| {
                let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                    &p.type_ann,
                    &self.type_registry,
                    generic_names,
                    diags,
                );
                let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                ctx.declare_variable(p.name.name.clone(), rust_ty.clone());
                RustParam {
                    name: p.name.name.clone(),
                    ty: rust_ty,
                    mode: ParamMode::Owned,
                    span: Some(p.span),
                }
            })
            .collect();

        // Analyze constructor body for field assignments: `this.field = value`
        // Collect field name → initializer expression
        let mut field_inits: Vec<(String, RustExpr)> = Vec::new();
        let mut other_stmts: Vec<rsc_syntax::rust_ir::RustStmt> = Vec::new();

        // Build use map for the constructor body
        let empty_reassigned = std::collections::HashSet::new();
        // Class methods stay Tier 1 — no callee param mode lookup
        let use_map = UseMap::analyze(
            &ctor.body,
            |obj, method| self.builtins.is_ref_args(obj, method),
            |_| None,
        );

        for (i, stmt) in ctor.body.stmts.iter().enumerate() {
            match stmt {
                ast::Stmt::Expr(expr) if matches!(expr.kind, ast::ExprKind::FieldAssign(_)) => {
                    if let ast::ExprKind::FieldAssign(fa) = &expr.kind
                        && matches!(fa.object.kind, ast::ExprKind::This)
                    {
                        let value = self.lower_expr(&fa.value, ctx, &use_map, i);
                        field_inits.push((fa.field.name.clone(), value));
                        continue;
                    }
                    let lowered = self.lower_stmt(stmt, ctx, &use_map, i, &empty_reassigned);
                    other_stmts.push(lowered);
                }
                _ => {
                    let lowered = self.lower_stmt(stmt, ctx, &use_map, i, &empty_reassigned);
                    other_stmts.push(lowered);
                }
            }
        }

        // Build the return expression: `Self { field1: value1, field2: value2, ... }`
        // Use the field names in declaration order, matching with the collected inits
        let self_fields: Vec<(String, RustExpr)> = field_names
            .iter()
            .map(|name| {
                let value = field_inits.iter().find(|(n, _)| n == name).map_or_else(
                    || RustExpr::synthetic(RustExprKind::Ident(name.clone())),
                    |(_, v)| v.clone(),
                );
                (name.clone(), value)
            })
            .collect();

        // Build the body block
        other_stmts.push(rsc_syntax::rust_ir::RustStmt::Expr(RustExpr::synthetic(
            RustExprKind::SelfStructLit {
                fields: self_fields,
            },
        )));

        let body = RustBlock {
            stmts: other_stmts,
            expr: None,
        };

        ctx.pop_scope();

        RustMethod {
            is_async: false,
            name: "new".to_owned(),
            self_param: None,
            params,
            return_type: Some(RustType::SelfType),
            body,
            span: Some(ctor.span),
        }
    }

    /// Lower a class method to a `RustMethod`.
    ///
    /// Determines `&self` or `&mut self` by analyzing whether the method
    /// writes to `this.field`.
    fn lower_class_method(
        &self,
        method: &ast::ClassMethod,
        generic_names: &[String],
        ctx: &mut LoweringContext,
        diags: &mut Vec<rsc_syntax::diagnostic::Diagnostic>,
    ) -> RustMethod {
        ctx.push_scope();

        let params: Vec<RustParam> = method
            .params
            .iter()
            .map(|p| {
                let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                    &p.type_ann,
                    &self.type_registry,
                    generic_names,
                    diags,
                );
                let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                ctx.declare_variable(p.name.name.clone(), rust_ty.clone());
                RustParam {
                    name: p.name.name.clone(),
                    ty: rust_ty,
                    mode: ParamMode::Owned,
                    span: Some(p.span),
                }
            })
            .collect();

        // Determine return type
        let return_type = method.return_type.as_ref().and_then(|rt| {
            rt.type_ann.as_ref().and_then(|ann| {
                let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                    ann,
                    &self.type_registry,
                    generic_names,
                    diags,
                );
                let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                if rust_ty == RustType::Unit {
                    return None;
                }
                Some(rust_ty)
            })
        });

        // Determine self_param: check if any statement writes to this.field
        let mutates_self = method_mutates_self(&method.body);
        let self_param = if mutates_self {
            Some(RustSelfParam::RefMut)
        } else {
            Some(RustSelfParam::Ref)
        };

        // Build use map and lower the body
        let reassigned = ownership::find_reassigned_variables(&method.body);
        // Class methods stay Tier 1 — no callee param mode lookup
        let use_map = UseMap::analyze(
            &method.body,
            |obj, method_name| self.builtins.is_ref_args(obj, method_name),
            |_| None,
        );

        let body = self.lower_block(&method.body, ctx, &use_map, 0, &reassigned);

        ctx.pop_scope();

        RustMethod {
            is_async: method.is_async,
            name: method.name.name.clone(),
            self_param,
            params,
            return_type,
            body,
            span: Some(method.span),
        }
    }
}

/// Check whether a class method mutates `self` (writes to `this.field`).
pub(super) fn method_mutates_self(body: &ast::Block) -> bool {
    for stmt in &body.stmts {
        if stmt_mutates_self(stmt) {
            return true;
        }
    }
    false
}

/// Check if a statement contains a `this.field = value` assignment.
fn stmt_mutates_self(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Expr(expr) => expr_mutates_self(expr),
        ast::Stmt::If(if_stmt) => {
            for s in &if_stmt.then_block.stmts {
                if stmt_mutates_self(s) {
                    return true;
                }
            }
            if let Some(else_clause) = &if_stmt.else_clause {
                match else_clause {
                    ast::ElseClause::Block(block) => {
                        for s in &block.stmts {
                            if stmt_mutates_self(s) {
                                return true;
                            }
                        }
                    }
                    ast::ElseClause::ElseIf(nested) => {
                        let nested_block = ast::Block {
                            stmts: vec![ast::Stmt::If(nested.as_ref().clone())],
                            span: nested.span,
                        };
                        return method_mutates_self(&nested_block);
                    }
                }
            }
            false
        }
        ast::Stmt::While(w) => {
            for s in &w.body.stmts {
                if stmt_mutates_self(s) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

/// Check if an expression contains a `this.field = value` pattern.
fn expr_mutates_self(expr: &ast::Expr) -> bool {
    matches!(expr.kind, ast::ExprKind::FieldAssign(ref fa) if matches!(fa.object.kind, ast::ExprKind::This))
}
