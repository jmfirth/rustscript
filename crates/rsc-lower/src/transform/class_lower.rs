//! Class definition lowering.
//!
//! Handles registration of class types, lowering of class definitions to
//! struct + impl blocks, constructors, methods, getters, setters, static
//! fields (associated constants), and constructor parameter properties.

use rsc_syntax::ast;
use rsc_syntax::rust_ir::{
    ParamMode, RustBlock, RustConstItem, RustExpr, RustExprKind, RustFieldDef, RustImplBlock,
    RustItem, RustMethod, RustParam, RustSelfParam, RustStructDef, RustTraitDef,
    RustTraitImplBlock, RustTraitMethod, RustType,
};

use crate::context::LoweringContext;
use crate::derive_inference;
use crate::ownership::{self, UseMap};

use super::{Transform, collect_generic_param_names, lower_type_params};

impl Transform {
    /// Register a class definition in the type registry during the pre-pass.
    ///
    /// Includes fields from both explicit declarations and constructor parameter
    /// properties. Registers getter/setter names for call-site transformation.
    pub(super) fn register_class_def(&mut self, cls: &ast::ClassDef, ctx: &mut LoweringContext) {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(cls.type_params.as_ref());

        // Collect explicit (non-static) fields
        let mut fields: Vec<(String, rsc_typeck::types::Type)> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ast::ClassMember::Field(f) if !f.is_static => {
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

        // Collect fields from constructor parameter properties
        for member in &cls.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                for p in &ctor.params {
                    if p.property_visibility.is_some() {
                        let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                            &p.type_ann,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        // Only add if not already declared as an explicit field
                        if !fields.iter().any(|(name, _)| name == &p.name.name) {
                            fields.push((p.name.name.clone(), ty));
                        }
                    }
                }
            }
        }

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        // Collect getter and setter names for call-site transformation
        let mut getters = Vec::new();
        let mut setters = Vec::new();
        let mut static_methods = Vec::new();
        for member in &cls.members {
            match member {
                ast::ClassMember::Getter(g) => {
                    getters.push(g.name.name.clone());
                }
                ast::ClassMember::Setter(s) => {
                    setters.push(s.name.name.clone());
                }
                ast::ClassMember::Method(m) if m.is_static => {
                    static_methods.push(m.name.name.clone());
                }
                _ => {}
            }
        }

        self.type_registry.register_class(
            cls.name.name.clone(),
            fields,
            getters,
            setters,
            static_methods,
        );
    }

    /// Lower a class definition to a struct + impl block(s).
    ///
    /// Returns multiple `RustItem`s: one struct, one inherent impl, and
    /// optionally trait impl blocks for each interface the class implements.
    /// Abstract classes lower to trait definitions instead.
    #[allow(clippy::too_many_lines)]
    // Class lowering coordinates struct, constructor, methods, getters, setters,
    // static fields, and trait impls; splitting would fragment the coherent pipeline.
    pub(super) fn lower_class_def(
        &self,
        cls: &ast::ClassDef,
        exported: bool,
        ctx: &mut LoweringContext,
    ) -> Vec<RustItem> {
        // Abstract classes lower to trait definitions
        if cls.is_abstract {
            return self.lower_abstract_class_def(cls, exported, ctx);
        }

        let mut items = Vec::new();
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(cls.type_params.as_ref());
        let type_params = lower_type_params(cls.type_params.as_ref());

        // 1. Collect fields generated from constructor parameter properties
        let mut param_property_fields: Vec<RustFieldDef> = Vec::new();
        for member in &cls.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                for p in &ctor.params {
                    if let Some(vis) = p.property_visibility {
                        let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                            &p.type_ann,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                        param_property_fields.push(RustFieldDef {
                            public: vis == ast::Visibility::Public,
                            name: p.name.name.clone(),
                            ty: rust_ty,
                            doc_comment: None,
                            span: Some(p.span),
                        });
                    }
                }
            }
        }

        // 2. Build the struct definition from non-static class fields + param properties
        let mut fields: Vec<RustFieldDef> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ast::ClassMember::Field(f) if !f.is_static => {
                    let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                        &f.type_ann,
                        &self.type_registry,
                        &generic_names,
                        &mut diags,
                    );
                    let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                    // Hash-private fields are always private (no pub)
                    let public = !f.is_hash_private && f.visibility == ast::Visibility::Public;
                    Some(RustFieldDef {
                        public,
                        name: f.name.name.clone(),
                        ty: rust_ty,
                        doc_comment: f.doc_comment.clone(),
                        span: Some(f.span),
                    })
                }
                _ => None,
            })
            .collect();

        // Add parameter property fields that aren't already declared
        for ppf in param_property_fields {
            if !fields.iter().any(|f| f.name == ppf.name) {
                fields.push(ppf);
            }
        }

        let field_types: Vec<&RustType> = fields.iter().map(|f| &f.ty).collect();
        let has_type_params = !type_params.is_empty();
        let derives = derive_inference::infer_struct_derives(&field_types, has_type_params);
        items.push(RustItem::Struct(RustStructDef {
            public: exported,
            name: cls.name.name.clone(),
            type_params: type_params.clone(),
            fields,
            derives,
            doc_comment: cls.doc_comment.clone(),
            span: Some(cls.span),
        }));

        // Collect field names for the constructor's Self { } literal
        // This includes both explicit fields and parameter property fields
        let field_names: Vec<String> = items
            .iter()
            .filter_map(|item| {
                if let RustItem::Struct(s) = item {
                    Some(s.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>())
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        // Collect field initializers (for fields with default values)
        let field_initializers: Vec<(String, &ast::Expr)> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ast::ClassMember::Field(f) if !f.is_static => f
                    .initializer
                    .as_ref()
                    .map(|init| (f.name.name.clone(), init)),
                _ => None,
            })
            .collect();

        // Collect interface and extends method names for trait impl separation
        let mut all_trait_sources: Vec<&ast::Ident> = cls.implements.iter().collect();
        if let Some(ref base) = cls.extends {
            all_trait_sources.push(base);
        }
        let trait_method_names: std::collections::HashSet<String> = all_trait_sources
            .iter()
            .filter_map(|iface_name| self.type_registry.get_interface_methods(&iface_name.name))
            .flatten()
            .map(|sig| sig.name.clone())
            .collect();

        // 3. Build methods
        let mut inherent_methods: Vec<RustMethod> = Vec::new();
        let mut trait_methods: std::collections::HashMap<String, Vec<RustMethod>> =
            std::collections::HashMap::new();

        // Initialize trait method buckets
        for iface in &cls.implements {
            trait_methods.entry(iface.name.clone()).or_default();
        }
        if let Some(ref base) = cls.extends {
            trait_methods.entry(base.name.clone()).or_default();
        }

        // Lower the constructor
        for member in &cls.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                let method = self.lower_class_constructor(
                    ctor,
                    &field_names,
                    &field_initializers,
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
                let is_trait_impl = trait_method_names.contains(&method.name.name);
                let lowered =
                    self.lower_class_method(method, &generic_names, is_trait_impl, ctx, &mut diags);

                // Check if this method belongs to a trait impl
                if is_trait_impl {
                    // Find which interface/base this method belongs to
                    for iface in &all_trait_sources {
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

        // Lower getters
        for member in &cls.members {
            if let ast::ClassMember::Getter(getter) = member {
                let lowered = self.lower_class_getter(getter, &generic_names, ctx, &mut diags);
                inherent_methods.push(lowered);
            }
        }

        // Lower setters
        for member in &cls.members {
            if let ast::ClassMember::Setter(setter) = member {
                let lowered = self.lower_class_setter(setter, &generic_names, ctx, &mut diags);
                inherent_methods.push(lowered);
            }
        }

        // 4. Lower static fields to associated constants
        let mut associated_consts: Vec<RustConstItem> = Vec::new();
        for member in &cls.members {
            if let ast::ClassMember::Field(f) = member
                && f.is_static
                && let Some(init_expr) = &f.initializer
            {
                let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                    &f.type_ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                let use_map = UseMap::analyze(
                    &ast::Block {
                        stmts: vec![],
                        span: f.span,
                    },
                    |obj, method| self.builtins.is_ref_args(obj, method),
                    |_| None,
                );
                let init = self.lower_expr(init_expr, ctx, &use_map, 0);
                associated_consts.push(RustConstItem {
                    public: f.visibility == ast::Visibility::Public,
                    name: f.name.name.clone(),
                    ty: rust_ty,
                    init,
                    span: Some(f.span),
                });
            }
        }

        // 5. Emit the inherent impl block
        items.push(RustItem::Impl(RustImplBlock {
            type_name: cls.name.name.clone(),
            type_params: type_params.clone(),
            associated_consts,
            methods: inherent_methods,
            span: Some(cls.span),
        }));

        // 6. Emit trait impl blocks (for both implements and extends)
        for iface in &all_trait_sources {
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

    /// Lower an abstract class definition to a trait.
    ///
    /// Abstract methods become required trait methods (no body).
    /// Concrete methods become default trait methods (with body).
    fn lower_abstract_class_def(
        &self,
        cls: &ast::ClassDef,
        exported: bool,
        ctx: &mut LoweringContext,
    ) -> Vec<RustItem> {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(cls.type_params.as_ref());

        let mut methods = Vec::new();

        for member in &cls.members {
            if let ast::ClassMember::Method(method) = member {
                let mut params = Vec::new();
                for p in &method.params {
                    let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                        &p.type_ann,
                        &self.type_registry,
                        &generic_names,
                        &mut diags,
                    );
                    let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                    params.push(RustParam {
                        name: p.name.name.clone(),
                        ty: rust_ty,
                        mode: ParamMode::Owned,
                        span: Some(p.span),
                    });
                }

                let return_type = method.return_type.as_ref().and_then(|rt| {
                    rt.type_ann.as_ref().map(|t| {
                        let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                            t,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        rsc_typeck::bridge::type_to_rust_type(&ty)
                    })
                });

                if method.is_abstract {
                    // Abstract method → required trait method (no default body)
                    methods.push(RustTraitMethod {
                        name: method.name.name.clone(),
                        params,
                        return_type,
                        has_self: !method.is_static,
                        default_body: None,
                        doc_comment: method.doc_comment.clone(),
                        span: Some(method.span),
                    });
                } else {
                    // Concrete method → default trait method (with body)
                    ctx.push_scope();
                    for p in &params {
                        ctx.declare_variable(p.name.clone(), p.ty.clone());
                    }
                    let use_map = UseMap::analyze(
                        &method.body,
                        |obj, m| self.builtins.is_ref_args(obj, m),
                        |name| {
                            self.fn_signatures
                                .get(name)
                                .and_then(|sig| sig.param_modes.as_deref())
                        },
                    );
                    let body = self.lower_block(
                        &method.body,
                        ctx,
                        &use_map,
                        0,
                        &std::collections::HashSet::new(),
                    );
                    ctx.pop_scope();

                    methods.push(RustTraitMethod {
                        name: method.name.name.clone(),
                        params,
                        return_type,
                        has_self: !method.is_static,
                        default_body: Some(body),
                        doc_comment: method.doc_comment.clone(),
                        span: Some(method.span),
                    });
                }
            }
        }

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        vec![RustItem::Trait(RustTraitDef {
            public: exported,
            name: cls.name.name.clone(),
            type_params: vec![],
            methods,
            doc_comment: cls.doc_comment.clone(),
            span: Some(cls.span),
        })]
    }

    /// Lower a class constructor to a `fn new(params) -> Self { Self { fields } }`.
    ///
    /// Handles constructor parameter properties (auto-generated fields) and
    /// field initializers (default values for fields not explicitly assigned).
    #[allow(clippy::too_many_lines)]
    // Constructor lowering coordinates param properties, field initializers,
    // body analysis, and Self literal construction; splitting would fragment
    // the coherent pipeline.
    fn lower_class_constructor(
        &self,
        ctor: &ast::ClassConstructor,
        field_names: &[String],
        field_initializers: &[(String, &ast::Expr)],
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

        // Collect parameter property names for auto-assignment
        let param_property_names: Vec<String> = ctor
            .params
            .iter()
            .filter(|p| p.property_visibility.is_some())
            .map(|p| p.name.name.clone())
            .collect();

        // Analyze constructor body for field assignments: `this.field = value`
        // Collect field name → initializer expression
        let mut field_inits: Vec<(String, RustExpr)> = Vec::new();
        let mut other_stmts: Vec<rsc_syntax::rust_ir::RustStmt> = Vec::new();

        // Add parameter property auto-assignments
        for name in &param_property_names {
            field_inits.push((
                name.clone(),
                RustExpr::synthetic(RustExprKind::Ident(name.clone())),
            ));
        }

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
                        // Override any param property auto-assignment
                        if let Some(existing) =
                            field_inits.iter_mut().find(|(n, _)| *n == fa.field.name)
                        {
                            existing.1 = value;
                        } else {
                            field_inits.push((fa.field.name.clone(), value));
                        }
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
        // Use the field names in declaration order, matching with the collected inits.
        // For fields not assigned in the constructor body or via param properties,
        // check for field initializers (default values).
        let init_use_map = UseMap::analyze(
            &ast::Block {
                stmts: vec![],
                span: ctor.span,
            },
            |obj, method| self.builtins.is_ref_args(obj, method),
            |_| None,
        );
        let self_fields: Vec<(String, RustExpr)> = field_names
            .iter()
            .map(|name| {
                let value = field_inits.iter().find(|(n, _)| n == name).map_or_else(
                    || {
                        // Check for field initializer (default value)
                        if let Some((_, init_expr)) =
                            field_initializers.iter().find(|(n, _)| n == name)
                        {
                            self.lower_expr(init_expr, ctx, &init_use_map, 0)
                        } else {
                            RustExpr::synthetic(RustExprKind::Ident(name.clone()))
                        }
                    },
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
            doc_comment: ctor.doc_comment.clone(),
            span: Some(ctor.span),
        }
    }

    /// Lower a class method to a `RustMethod`.
    ///
    /// Determines `&self` or `&mut self` by analyzing whether the method
    /// writes to `this.field`. Static methods have no self parameter.
    /// When `is_trait_impl` is false and borrow inference is enabled,
    /// non-self parameters are analyzed for borrowing.
    /// Trait impl methods always keep `Owned` params to match the trait contract.
    fn lower_class_method(
        &self,
        method: &ast::ClassMethod,
        generic_names: &[String],
        is_trait_impl: bool,
        ctx: &mut LoweringContext,
        diags: &mut Vec<rsc_syntax::diagnostic::Diagnostic>,
    ) -> RustMethod {
        ctx.push_scope();

        // Analyze param usage for borrow inference (inherent methods only)
        // Trait impl methods must match the trait signature → always Owned.
        // Conditional move analysis is not attempted in Phase 4 — any branch
        // that moves a parameter taints the entire parameter as Moved.
        let param_modes = if !is_trait_impl && !self.no_borrow_inference {
            let param_names: Vec<String> =
                method.params.iter().map(|p| p.name.name.clone()).collect();
            let builtins = &self.builtins;
            let usage_map = ownership::analyze_param_usage(&method.body, &param_names, |obj, m| {
                builtins.is_ref_args(obj, m)
            });

            let mut temp_diags = Vec::new();
            let modes: Vec<ParamMode> = method
                .params
                .iter()
                .map(|p| {
                    let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                        &p.type_ann,
                        &self.type_registry,
                        generic_names,
                        &mut temp_diags,
                    );
                    let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                    let usage = usage_map
                        .get(p.name.name.as_str())
                        .copied()
                        .unwrap_or(ownership::ParamUsage::ReadOnly);
                    ownership::usage_to_mode(usage, &rust_ty, Some(&self.type_registry))
                })
                .collect();
            Some(modes)
        } else {
            None
        };

        let params: Vec<RustParam> = method
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                    &p.type_ann,
                    &self.type_registry,
                    generic_names,
                    diags,
                );
                let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                ctx.declare_variable(p.name.name.clone(), rust_ty.clone());

                let mode = param_modes
                    .as_ref()
                    .and_then(|modes| modes.get(i))
                    .copied()
                    .unwrap_or(ParamMode::Owned);

                RustParam {
                    name: p.name.name.clone(),
                    ty: rust_ty,
                    mode,
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

        // Determine self_param: static methods have none, instance methods check mutation
        let self_param = if method.is_static {
            None
        } else {
            let mutates_self = method_mutates_self(&method.body);
            if mutates_self {
                Some(RustSelfParam::RefMut)
            } else {
                Some(RustSelfParam::Ref)
            }
        };

        // Build use map and lower the body
        let reassigned = ownership::find_reassigned_variables(&method.body);
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
            doc_comment: method.doc_comment.clone(),
            span: Some(method.span),
        }
    }

    /// Lower a getter accessor to a `fn name(&self) -> Type { body }`.
    fn lower_class_getter(
        &self,
        getter: &ast::ClassGetter,
        generic_names: &[String],
        ctx: &mut LoweringContext,
        diags: &mut Vec<rsc_syntax::diagnostic::Diagnostic>,
    ) -> RustMethod {
        ctx.push_scope();

        let return_type = getter.return_type.as_ref().and_then(|rt| {
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

        let reassigned = ownership::find_reassigned_variables(&getter.body);
        let use_map = UseMap::analyze(
            &getter.body,
            |obj, method_name| self.builtins.is_ref_args(obj, method_name),
            |_| None,
        );

        let body = self.lower_block(&getter.body, ctx, &use_map, 0, &reassigned);

        ctx.pop_scope();

        RustMethod {
            is_async: false,
            name: getter.name.name.clone(),
            self_param: Some(RustSelfParam::Ref),
            params: vec![],
            return_type,
            body,
            doc_comment: None,
            span: Some(getter.span),
        }
    }

    /// Lower a setter accessor to a `fn set_name(&mut self, value: Type) { body }`.
    fn lower_class_setter(
        &self,
        setter: &ast::ClassSetter,
        generic_names: &[String],
        ctx: &mut LoweringContext,
        diags: &mut Vec<rsc_syntax::diagnostic::Diagnostic>,
    ) -> RustMethod {
        ctx.push_scope();

        let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
            &setter.param.type_ann,
            &self.type_registry,
            generic_names,
            diags,
        );
        let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
        ctx.declare_variable(setter.param.name.name.clone(), rust_ty.clone());

        let params = vec![RustParam {
            name: setter.param.name.name.clone(),
            ty: rust_ty,
            mode: ParamMode::Owned,
            span: Some(setter.param.span),
        }];

        let reassigned = ownership::find_reassigned_variables(&setter.body);
        let use_map = UseMap::analyze(
            &setter.body,
            |obj, method_name| self.builtins.is_ref_args(obj, method_name),
            |_| None,
        );

        let body = self.lower_block(&setter.body, ctx, &use_map, 0, &reassigned);

        ctx.pop_scope();

        RustMethod {
            is_async: false,
            name: format!("set_{}", setter.name.name),
            self_param: Some(RustSelfParam::RefMut),
            params,
            return_type: None,
            body,
            doc_comment: None,
            span: Some(setter.span),
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
