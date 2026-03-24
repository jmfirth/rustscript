//! AST-to-IR transformation.
//!
//! Consumes the `RustScript` AST and produces Rust IR, using the types,
//! ownership, and builtins modules for type resolution, clone insertion,
//! and builtin method lowering respectively.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{
    RustBinaryOp, RustBlock, RustCompoundAssignOp, RustDestructureStmt, RustElse, RustEnumDef,
    RustEnumVariant, RustExpr, RustExprKind, RustFieldDef, RustFile, RustFnDecl, RustIfStmt,
    RustItem, RustLetStmt, RustMatchArm, RustMatchStmt, RustParam, RustPattern, RustReturnStmt,
    RustStmt, RustStructDef, RustType, RustTypeParam, RustUnaryOp, RustUseDecl, RustWhileStmt,
};

use crate::builtins::BuiltinRegistry;
use crate::context::LoweringContext;
use crate::ownership::{self, UseMap};
use rsc_typeck::registry::TypeRegistry;
use rsc_typeck::resolve;
use rsc_typeck::types::Type;

/// The AST-to-IR transformer.
///
/// Holds the builtin registry and type registry, and drives the lowering of
/// an entire module.
pub(crate) struct Transform {
    builtins: BuiltinRegistry,
    type_registry: TypeRegistry,
}

impl Transform {
    /// Create a new transformer with the default builtin registry and an empty
    /// type registry.
    pub fn new() -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
            type_registry: TypeRegistry::new(),
        }
    }

    /// Lower a complete `RustScript` module to a Rust file.
    ///
    /// Performs a pre-pass to register all type definitions, then lowers
    /// each item.
    pub fn lower_module(&mut self, module: &ast::Module) -> (RustFile, Vec<Diagnostic>) {
        let mut ctx = LoweringContext::new();

        // Pre-pass: register all type definitions so they can be resolved
        // during function lowering.
        for item in &module.items {
            match &item.kind {
                ast::ItemKind::TypeDef(td) => self.register_type_def(td, &mut ctx),
                ast::ItemKind::EnumDef(ed) => self.register_enum_def(ed, &mut ctx),
                ast::ItemKind::Function(_) => {}
            }
        }

        let items: Vec<RustItem> = module
            .items
            .iter()
            .map(|item| match &item.kind {
                ast::ItemKind::Function(f) => RustItem::Function(self.lower_fn(f, &mut ctx)),
                ast::ItemKind::TypeDef(td) => RustItem::Struct(self.lower_type_def(td, &mut ctx)),
                ast::ItemKind::EnumDef(ed) => RustItem::Enum(self.lower_enum_def(ed, &mut ctx)),
            })
            .collect();

        // Collect use declarations by scanning generated items for HashMap/HashSet usage
        let uses = collect_use_declarations(&items);

        let diagnostics = ctx.into_diagnostics();
        (
            RustFile {
                uses,
                mod_decls: Vec::new(),
                items,
            },
            diagnostics,
        )
    }

    /// Register a type definition in the type registry during the pre-pass.
    fn register_type_def(&mut self, td: &ast::TypeDef, ctx: &mut LoweringContext) {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(td.type_params.as_ref());
        let fields: Vec<(String, Type)> = td
            .fields
            .iter()
            .map(|f| {
                let ty = resolve::resolve_type_annotation_with_generics(
                    &f.type_ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                (f.name.name.clone(), ty)
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        self.type_registry.register(td.name.name.clone(), fields);
    }

    /// Lower a type definition to a Rust struct.
    fn lower_type_def(&self, td: &ast::TypeDef, ctx: &mut LoweringContext) -> RustStructDef {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(td.type_params.as_ref());
        let type_params = lower_type_params(td.type_params.as_ref());
        let fields = td
            .fields
            .iter()
            .map(|f| {
                let ty = resolve::resolve_type_annotation_with_generics(
                    &f.type_ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                RustFieldDef {
                    public: true,
                    name: f.name.name.clone(),
                    ty: rust_ty,
                    span: Some(f.span),
                }
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        RustStructDef {
            name: td.name.name.clone(),
            type_params,
            fields,
            span: Some(td.span),
        }
    }

    /// Register an enum definition in the type registry during the pre-pass.
    fn register_enum_def(&mut self, ed: &ast::EnumDef, ctx: &mut LoweringContext) {
        // Determine if simple or data enum
        let is_data = ed
            .variants
            .iter()
            .any(|v| matches!(v, ast::EnumVariant::Data { .. }));

        if is_data {
            let mut diags = Vec::new();
            let variants: Vec<(String, Vec<(String, rsc_typeck::types::Type)>)> = ed
                .variants
                .iter()
                .filter_map(|v| match v {
                    ast::EnumVariant::Data { name, fields, .. } => {
                        let field_types: Vec<(String, rsc_typeck::types::Type)> = fields
                            .iter()
                            .map(|f| {
                                let ty = resolve::resolve_type_annotation_with_generics(
                                    &f.type_ann,
                                    &self.type_registry,
                                    &[],
                                    &mut diags,
                                );
                                (f.name.name.clone(), ty)
                            })
                            .collect();
                        Some((name.name.clone(), field_types))
                    }
                    ast::EnumVariant::Simple(..) => None,
                })
                .collect();
            for d in diags {
                ctx.emit_diagnostic(d);
            }
            self.type_registry
                .register_data_enum(ed.name.name.clone(), variants);
        } else {
            let variants: Vec<String> = ed
                .variants
                .iter()
                .filter_map(|v| match v {
                    ast::EnumVariant::Simple(ident, _) => Some(ident.name.clone()),
                    ast::EnumVariant::Data { .. } => None,
                })
                .collect();
            self.type_registry
                .register_simple_enum(ed.name.name.clone(), variants);
        }
    }

    /// Lower an enum definition to a Rust enum.
    fn lower_enum_def(&self, ed: &ast::EnumDef, ctx: &mut LoweringContext) -> RustEnumDef {
        let mut diags = Vec::new();
        let variants = ed
            .variants
            .iter()
            .map(|v| match v {
                ast::EnumVariant::Simple(ident, span) => RustEnumVariant {
                    name: ident.name.clone(),
                    fields: vec![],
                    span: Some(*span),
                },
                ast::EnumVariant::Data {
                    name, fields, span, ..
                } => {
                    let rust_fields = fields
                        .iter()
                        .map(|f| {
                            let ty = resolve::resolve_type_annotation_with_generics(
                                &f.type_ann,
                                &self.type_registry,
                                &[],
                                &mut diags,
                            );
                            let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                            RustFieldDef {
                                public: true,
                                name: f.name.name.clone(),
                                ty: rust_ty,
                                span: Some(f.span),
                            }
                        })
                        .collect();
                    RustEnumVariant {
                        name: name.name.clone(),
                        fields: rust_fields,
                        span: Some(*span),
                    }
                }
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        RustEnumDef {
            name: ed.name.name.clone(),
            variants,
            span: Some(ed.span),
        }
    }

    /// Lower a function declaration.
    ///
    /// Performs two-pass analysis: first finds reassigned variables and builds
    /// a use map, then lowers the body with that context.
    pub fn lower_fn(&self, f: &ast::FnDecl, ctx: &mut LoweringContext) -> RustFnDecl {
        ctx.push_scope();

        let generic_names = collect_generic_param_names(f.type_params.as_ref());
        let type_params = lower_type_params(f.type_params.as_ref());

        // Phase 1: find reassigned variables for mutability analysis
        let reassigned = ownership::find_reassigned_variables(&f.body);

        // Phase 2: build use map for ownership analysis
        let use_map = UseMap::analyze(&f.body, |obj, method| {
            self.builtins.is_ref_args(obj, method)
        });

        // Declare parameters in scope
        let params: Vec<RustParam> = f
            .params
            .iter()
            .map(|p| {
                let mut diags = Vec::new();
                let ty_inner = resolve::resolve_type_annotation_with_generics(
                    &p.type_ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                let ty = rsc_typeck::bridge::type_to_rust_type(&ty_inner);
                for d in diags {
                    ctx.emit_diagnostic(d);
                }
                ctx.declare_variable(p.name.name.clone(), ty.clone());
                RustParam {
                    name: p.name.name.clone(),
                    ty,
                    span: Some(p.span),
                }
            })
            .collect();

        let return_type = f.return_type.as_ref().and_then(|ann| {
            let mut diags = Vec::new();
            let ty_inner = resolve::resolve_type_annotation_with_generics(
                ann,
                &self.type_registry,
                &generic_names,
                &mut diags,
            );
            let ty = rsc_typeck::bridge::type_to_rust_type(&ty_inner);
            for d in diags {
                ctx.emit_diagnostic(d);
            }
            if ty == RustType::Unit {
                return None;
            }
            Some(ty)
        });

        // Lower the body
        let body = self.lower_block(&f.body, ctx, &use_map, 0, &reassigned);

        ctx.pop_scope();

        RustFnDecl {
            name: f.name.name.clone(),
            type_params,
            params,
            return_type,
            body,
            span: Some(f.span),
        }
    }

    /// Lower a block of statements.
    #[allow(clippy::only_used_in_recursion)]
    fn lower_block(
        &self,
        block: &ast::Block,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        current_base: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustBlock {
        let stmts = block
            .stmts
            .iter()
            .enumerate()
            .map(|(i, stmt)| self.lower_stmt(stmt, ctx, use_map, current_base + i, reassigned))
            .collect();

        RustBlock { stmts, expr: None }
    }

    /// Lower a single statement.
    fn lower_stmt(
        &self,
        stmt: &ast::Stmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        match stmt {
            ast::Stmt::VarDecl(decl) => {
                self.lower_var_decl(decl, ctx, use_map, stmt_index, reassigned)
            }
            ast::Stmt::Expr(expr) => {
                let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
                RustStmt::Semi(lowered)
            }
            ast::Stmt::Return(ret) => {
                let value = ret
                    .value
                    .as_ref()
                    .map(|v| self.lower_expr(v, ctx, use_map, stmt_index));
                RustStmt::Return(RustReturnStmt {
                    value,
                    span: Some(ret.span),
                })
            }
            ast::Stmt::If(if_stmt) => {
                RustStmt::If(self.lower_if(if_stmt, ctx, use_map, stmt_index, reassigned))
            }
            ast::Stmt::While(while_stmt) => {
                RustStmt::While(self.lower_while(while_stmt, ctx, use_map, stmt_index, reassigned))
            }
            ast::Stmt::Destructure(destr) => {
                self.lower_destructure(destr, ctx, use_map, stmt_index)
            }
            ast::Stmt::Switch(switch) => {
                self.lower_switch(switch, ctx, use_map, stmt_index, reassigned)
            }
        }
    }

    /// Lower a variable declaration.
    fn lower_var_decl(
        &self,
        decl: &ast::VarDecl,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        // Resolve the type from annotation or infer from literal
        let mut diags = Vec::new();
        let ty = if let Some(ann) = &decl.type_ann {
            let ty_inner = resolve::resolve_type_annotation_with_registry(
                ann,
                &self.type_registry,
                &mut diags,
            );
            rsc_typeck::bridge::type_to_rust_type(&ty_inner)
        } else {
            resolve::infer_literal_rust_type(&decl.init).unwrap_or(RustType::I64)
        };

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        // Determine mutability:
        // - `const` declarations are never mutable
        // - `let` declarations are mutable only if the variable is reassigned
        let mutable = decl.binding == ast::VarBinding::Let && reassigned.contains(&decl.name.name);

        ctx.declare_variable(decl.name.name.clone(), ty.clone());

        // Set the struct type context so struct literal lowering can resolve the
        // type name from the variable's annotation.
        let struct_type_name = match &ty {
            RustType::Named(name) => Some(name.clone()),
            _ => None,
        };
        ctx.set_struct_type_name(struct_type_name);

        // Check for enum construction: `const dir: Direction = "north"` → `Direction::North`
        let init = if let (RustType::Named(type_name), ast::ExprKind::StringLit(s)) =
            (&ty, &decl.init.kind)
        {
            if let Some(td) = self.type_registry.lookup(type_name) {
                let variant_name = capitalize_first(s);
                let is_enum = matches!(
                    &td.kind,
                    rsc_typeck::registry::TypeDefKind::SimpleEnum(_)
                        | rsc_typeck::registry::TypeDefKind::DataEnum(_)
                );
                if is_enum {
                    RustExpr::new(
                        RustExprKind::EnumVariant {
                            enum_name: type_name.clone(),
                            variant_name,
                        },
                        decl.init.span,
                    )
                } else {
                    self.lower_expr(&decl.init, ctx, use_map, stmt_index)
                }
            } else {
                self.lower_expr(&decl.init, ctx, use_map, stmt_index)
            }
        } else {
            self.lower_expr(&decl.init, ctx, use_map, stmt_index)
        };

        ctx.set_struct_type_name(None);

        // Omit the type annotation when it's inferable from the literal initializer
        // and the user didn't write an explicit annotation.
        // Named types in struct construction don't need the type annotation since
        // the struct literal provides the type.
        let emit_ty = if matches!(ty, RustType::Named(_)) {
            // Struct types: the struct literal provides the type, so omit annotation
            None
        } else if decl.type_ann.is_some() {
            // User wrote an explicit type annotation — always include it
            Some(ty)
        } else if is_default_literal_type(&decl.init, &ty) {
            // Type matches the literal's default — omit for cleaner output
            None
        } else {
            Some(ty)
        };

        RustStmt::Let(RustLetStmt {
            mutable,
            name: decl.name.name.clone(),
            ty: emit_ty,
            init,
            span: Some(decl.span),
        })
    }

    /// Lower an if statement.
    fn lower_if(
        &self,
        if_stmt: &ast::IfStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustIfStmt {
        let condition = self.lower_expr(&if_stmt.condition, ctx, use_map, stmt_index);
        let then_block =
            self.lower_block(&if_stmt.then_block, ctx, use_map, stmt_index, reassigned);
        let else_clause = if_stmt.else_clause.as_ref().map(|ec| match ec {
            ast::ElseClause::Block(block) => {
                RustElse::Block(self.lower_block(block, ctx, use_map, stmt_index, reassigned))
            }
            ast::ElseClause::ElseIf(nested_if) => RustElse::ElseIf(Box::new(
                self.lower_if(nested_if, ctx, use_map, stmt_index, reassigned),
            )),
        });

        RustIfStmt {
            condition,
            then_block,
            else_clause,
            span: Some(if_stmt.span),
        }
    }

    /// Lower a while statement.
    fn lower_while(
        &self,
        while_stmt: &ast::WhileStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustWhileStmt {
        let condition = self.lower_expr(&while_stmt.condition, ctx, use_map, stmt_index);
        let body = self.lower_block(&while_stmt.body, ctx, use_map, stmt_index, reassigned);

        RustWhileStmt {
            condition,
            body,
            span: Some(while_stmt.span),
        }
    }

    /// Lower a destructuring statement.
    ///
    /// Resolves the type name from the init expression's type in the context,
    /// then produces a `RustDestructureStmt`.
    fn lower_destructure(
        &self,
        destr: &ast::DestructureStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustStmt {
        let init = self.lower_expr(&destr.init, ctx, use_map, stmt_index);

        // Try to infer the type name from the init expression. If the init
        // is an identifier, look up its type in the context.
        let type_name = match &destr.init.kind {
            ast::ExprKind::Ident(ident) => ctx
                .lookup_variable(&ident.name)
                .and_then(|info| match &info.ty {
                    RustType::Named(name) => Some(name.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Unknown".to_owned()),
            _ => "Unknown".to_owned(),
        };

        let fields = destr.fields.iter().map(|f| f.name.clone()).collect();

        // Declare the extracted fields as variables in the current scope.
        // Look up their types from the type registry.
        if let Some(td) = self.type_registry.lookup(&type_name)
            && let Some(fields) = td.struct_fields()
        {
            for field_ident in &destr.fields {
                let field_ty = fields
                    .iter()
                    .find(|(name, _)| name == &field_ident.name)
                    .map_or(RustType::Unit, |(_, ty)| {
                        rsc_typeck::bridge::type_to_rust_type(ty)
                    });
                ctx.declare_variable(field_ident.name.clone(), field_ty);
            }
        }

        RustStmt::Destructure(RustDestructureStmt {
            type_name,
            fields,
            init,
            mutable: destr.binding == ast::VarBinding::Let,
            span: Some(destr.span),
        })
    }

    /// Lower a switch statement to a Rust match statement.
    ///
    /// Resolves the scrutinee type to determine the enum being matched.
    /// For simple enums, generates `EnumVariant` patterns.
    /// For data enums, generates `EnumVariantFields` patterns with field bindings.
    /// Inside case bodies, rewrites `scrutinee.field` to just `field` (the
    /// destructured binding from the match arm).
    fn lower_switch(
        &self,
        switch: &ast::SwitchStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        let scrutinee = self.lower_expr(&switch.scrutinee, ctx, use_map, stmt_index);

        // Determine the enum name from the scrutinee's type
        let scrutinee_var_name = match &switch.scrutinee.kind {
            ast::ExprKind::Ident(ident) => Some(ident.name.clone()),
            _ => None,
        };

        let enum_name = scrutinee_var_name
            .as_ref()
            .and_then(|name| ctx.lookup_variable(name))
            .and_then(|info| match &info.ty {
                RustType::Named(n) => Some(n.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "Unknown".to_owned());

        let td = self.type_registry.lookup(&enum_name);

        let arms: Vec<RustMatchArm> = switch
            .cases
            .iter()
            .map(|case| {
                let variant_name = capitalize_first(&case.pattern);

                let (pattern, bound_fields) = match td.map(|t| &t.kind) {
                    Some(rsc_typeck::registry::TypeDefKind::DataEnum(variants)) => {
                        // Find the variant's fields
                        let field_names: Vec<String> = variants
                            .iter()
                            .find(|(vn, _)| *vn == variant_name)
                            .map(|(_, fields)| fields.iter().map(|(n, _)| n.clone()).collect())
                            .unwrap_or_default();
                        (
                            RustPattern::EnumVariantFields(
                                enum_name.clone(),
                                variant_name.clone(),
                                field_names.clone(),
                            ),
                            field_names,
                        )
                    }
                    _ => (
                        RustPattern::EnumVariant(enum_name.clone(), variant_name),
                        Vec::new(),
                    ),
                };

                // Lower case body with field binding context
                let body = self.lower_switch_case_body(
                    &case.body,
                    ctx,
                    use_map,
                    stmt_index,
                    reassigned,
                    scrutinee_var_name.as_deref(),
                    &bound_fields,
                    &enum_name,
                );

                RustMatchArm { pattern, body }
            })
            .collect();

        RustStmt::Match(RustMatchStmt {
            scrutinee,
            arms,
            span: Some(switch.span),
        })
    }

    /// Lower switch case body statements, rewriting field accesses on the
    /// scrutinee variable to direct identifier references (the destructured
    /// bindings from the match arm pattern).
    ///
    /// Also rewrites string literals in return position that match enum variant
    /// names to `EnumName::VariantName` expressions.
    #[allow(clippy::too_many_arguments)]
    fn lower_switch_case_body(
        &self,
        stmts: &[ast::Stmt],
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
        scrutinee_var: Option<&str>,
        bound_fields: &[String],
        enum_name: &str,
    ) -> RustBlock {
        let rust_stmts: Vec<RustStmt> = stmts
            .iter()
            .enumerate()
            .map(|(i, stmt)| {
                self.lower_switch_case_stmt(
                    stmt,
                    ctx,
                    use_map,
                    stmt_index + i,
                    reassigned,
                    scrutinee_var,
                    bound_fields,
                    enum_name,
                )
            })
            .collect();

        RustBlock {
            stmts: rust_stmts,
            expr: None,
        }
    }

    /// Lower a single statement within a switch case body.
    #[allow(clippy::too_many_arguments)]
    fn lower_switch_case_stmt(
        &self,
        stmt: &ast::Stmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
        scrutinee_var: Option<&str>,
        bound_fields: &[String],
        enum_name: &str,
    ) -> RustStmt {
        match stmt {
            ast::Stmt::Return(ret) => {
                let value = ret.value.as_ref().map(|v| {
                    self.lower_switch_case_expr(
                        v,
                        ctx,
                        use_map,
                        stmt_index,
                        scrutinee_var,
                        bound_fields,
                        enum_name,
                    )
                });
                RustStmt::Return(RustReturnStmt {
                    value,
                    span: Some(ret.span),
                })
            }
            ast::Stmt::Expr(expr) => {
                let lowered = self.lower_switch_case_expr(
                    expr,
                    ctx,
                    use_map,
                    stmt_index,
                    scrutinee_var,
                    bound_fields,
                    enum_name,
                );
                RustStmt::Semi(lowered)
            }
            // For other statement types, fall back to the normal lowering
            _ => self.lower_stmt(stmt, ctx, use_map, stmt_index, reassigned),
        }
    }

    /// Lower an expression within a switch case body.
    ///
    /// This handles two key rewrites:
    /// 1. `scrutinee.field` → `field` when `field` is a bound destructured binding
    /// 2. String literals that match enum variant names → `EnumName::VariantName`
    #[allow(clippy::too_many_arguments)]
    fn lower_switch_case_expr(
        &self,
        expr: &ast::Expr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        scrutinee_var: Option<&str>,
        bound_fields: &[String],
        enum_name: &str,
    ) -> RustExpr {
        match &expr.kind {
            // Rewrite: scrutinee.field → field (destructured binding)
            ast::ExprKind::FieldAccess(fa) => {
                if let ast::ExprKind::Ident(obj_ident) = &fa.object.kind
                    && scrutinee_var == Some(obj_ident.name.as_str())
                    && bound_fields.contains(&fa.field.name)
                {
                    return RustExpr::new(RustExprKind::Ident(fa.field.name.clone()), expr.span);
                }
                // Not a match binding — lower normally
                let object = self.lower_switch_case_expr(
                    &fa.object,
                    ctx,
                    use_map,
                    stmt_index,
                    scrutinee_var,
                    bound_fields,
                    enum_name,
                );
                RustExpr::new(
                    RustExprKind::FieldAccess {
                        object: Box::new(object),
                        field: fa.field.name.clone(),
                    },
                    expr.span,
                )
            }
            // Rewrite: string literal → enum variant when return type is an enum
            ast::ExprKind::StringLit(s) => {
                // Check if this string matches an enum variant
                if let Some(td) = self.type_registry.lookup(enum_name) {
                    let variant_name = capitalize_first(s);
                    let is_variant = match &td.kind {
                        rsc_typeck::registry::TypeDefKind::SimpleEnum(variants) => {
                            variants.contains(&variant_name)
                        }
                        rsc_typeck::registry::TypeDefKind::DataEnum(variants) => {
                            variants.iter().any(|(vn, _)| *vn == variant_name)
                        }
                        rsc_typeck::registry::TypeDefKind::Struct(_) => false,
                    };
                    if is_variant {
                        return RustExpr::new(
                            RustExprKind::EnumVariant {
                                enum_name: enum_name.to_owned(),
                                variant_name,
                            },
                            expr.span,
                        );
                    }
                }
                // Not an enum variant — lower as normal string
                self.lower_expr(expr, ctx, use_map, stmt_index)
            }
            // Binary expressions: recurse into operands
            ast::ExprKind::Binary(bin) => {
                let left = self.lower_switch_case_expr(
                    &bin.left,
                    ctx,
                    use_map,
                    stmt_index,
                    scrutinee_var,
                    bound_fields,
                    enum_name,
                );
                let right = self.lower_switch_case_expr(
                    &bin.right,
                    ctx,
                    use_map,
                    stmt_index,
                    scrutinee_var,
                    bound_fields,
                    enum_name,
                );
                let op = lower_binary_op(bin.op);
                RustExpr::new(
                    RustExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    expr.span,
                )
            }
            // Paren: recurse
            ast::ExprKind::Paren(inner) => {
                let lowered = self.lower_switch_case_expr(
                    inner,
                    ctx,
                    use_map,
                    stmt_index,
                    scrutinee_var,
                    bound_fields,
                    enum_name,
                );
                RustExpr::new(RustExprKind::Paren(Box::new(lowered)), expr.span)
            }
            // Everything else: use normal lowering
            _ => self.lower_expr(expr, ctx, use_map, stmt_index),
        }
    }

    /// Lower an expression.
    #[allow(clippy::too_many_lines)]
    // Expression lowering covers all AST expression kinds; splitting would obscure the match
    fn lower_expr(
        &self,
        expr: &ast::Expr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        match &expr.kind {
            ast::ExprKind::IntLit(v) => RustExpr::new(RustExprKind::IntLit(*v), expr.span),
            ast::ExprKind::FloatLit(v) => RustExpr::new(RustExprKind::FloatLit(*v), expr.span),
            ast::ExprKind::StringLit(s) => {
                // In Rust, string literals are &str. RustScript's `string` type is
                // String (owned). Wrap in .to_string() so the expression produces
                // an owned String. The exception is when this literal ends up inside
                // a println! format position — but that's handled by the builtin
                // registry which constructs its own StringLit for the format string.
                let lit = RustExpr::new(RustExprKind::StringLit(s.clone()), expr.span);
                RustExpr::synthetic(RustExprKind::ToString(Box::new(lit)))
            }
            ast::ExprKind::BoolLit(v) => RustExpr::new(RustExprKind::BoolLit(*v), expr.span),
            ast::ExprKind::Ident(ident) => {
                self.lower_ident_ref(ident, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Binary(bin) => {
                let left = self.lower_expr(&bin.left, ctx, use_map, stmt_index);
                let right = self.lower_expr(&bin.right, ctx, use_map, stmt_index);
                let op = lower_binary_op(bin.op);
                RustExpr::new(
                    RustExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::Unary(un) => {
                let operand = self.lower_expr(&un.operand, ctx, use_map, stmt_index);
                let op = lower_unary_op(un.op);
                RustExpr::new(
                    RustExprKind::Unary {
                        op,
                        operand: Box::new(operand),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::Call(call) => {
                let args: Vec<RustExpr> = call
                    .args
                    .iter()
                    .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                    .collect();
                RustExpr::new(
                    RustExprKind::Call {
                        func: call.callee.name.clone(),
                        args,
                    },
                    expr.span,
                )
            }
            ast::ExprKind::MethodCall(mc) => {
                self.lower_method_call(mc, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Paren(inner) => {
                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                RustExpr::new(RustExprKind::Paren(Box::new(lowered)), expr.span)
            }
            ast::ExprKind::Assign(assign) => {
                // Detect compound assignment pattern: x = x op rhs
                if let Some((compound_op, rhs)) =
                    detect_compound_assign(&assign.target.name, &assign.value)
                {
                    let lowered_rhs = self.lower_expr(rhs, ctx, use_map, stmt_index);
                    return RustExpr::new(
                        RustExprKind::CompoundAssign {
                            target: assign.target.name.clone(),
                            op: compound_op,
                            value: Box::new(lowered_rhs),
                        },
                        expr.span,
                    );
                }
                let value = self.lower_expr(&assign.value, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::Assign {
                        target: assign.target.name.clone(),
                        value: Box::new(value),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::StructLit(slit) => {
                self.lower_struct_lit(slit, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::FieldAccess(fa) => {
                let object = self.lower_expr(&fa.object, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::FieldAccess {
                        object: Box::new(object),
                        field: fa.field.name.clone(),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::TemplateLit(tpl) => {
                self.lower_template_lit(tpl, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::ArrayLit(elements) => {
                let lowered: Vec<RustExpr> = elements
                    .iter()
                    .map(|e| self.lower_expr(e, ctx, use_map, stmt_index))
                    .collect();
                RustExpr::new(RustExprKind::VecLit(lowered), expr.span)
            }
            ast::ExprKind::New(new_expr) => {
                self.lower_new_expr(new_expr, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Index(index_expr) => {
                let object = self.lower_expr(&index_expr.object, ctx, use_map, stmt_index);
                let index = self.lower_expr(&index_expr.index, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::Index {
                        object: Box::new(object),
                        index: Box::new(index),
                    },
                    expr.span,
                )
            }
        }
    }

    /// Lower an identifier reference, inserting a clone if needed.
    #[allow(clippy::unused_self)] // Method for consistency with other lower_* methods
    fn lower_ident_ref(
        &self,
        ident: &ast::Ident,
        span: rsc_syntax::span::Span,
        ctx: &LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let base = RustExpr::new(RustExprKind::Ident(ident.name.clone()), span);

        // Check if clone is needed
        let var_type = ctx.lookup_variable(&ident.name).map(|info| info.ty.clone());

        if let Some(ty) = var_type
            && ownership::needs_clone(&ident.name, stmt_index, use_map, &ty)
        {
            return RustExpr::synthetic(RustExprKind::Clone(Box::new(base)));
        }

        base
    }

    /// Lower a method call expression.
    ///
    /// First checks if the method call matches a builtin. If so, lowers
    /// the arguments first then delegates to the builtin lowering function.
    fn lower_method_call(
        &self,
        mc: &ast::MethodCallExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Try to match as a builtin: extract object name from Ident
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && let Some(lowering_fn) = self
                .builtins
                .lookup_method(&obj_ident.name, &mc.method.name)
        {
            // Lower arguments first, then pass to builtin
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return lowering_fn(lowered_args, span);
        }

        // Not a builtin — lower as a regular method call
        let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
        let args: Vec<RustExpr> = mc
            .args
            .iter()
            .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
            .collect();

        RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(receiver),
                method: mc.method.name.clone(),
                args,
            },
            span,
        )
    }

    /// Lower a struct literal expression.
    ///
    /// If the struct literal has no explicit type name, attempts to resolve it
    /// from the surrounding variable declaration context. The lowering pass
    /// stores the current expected type when processing `VarDecl` with a
    /// struct literal initializer.
    fn lower_struct_lit(
        &self,
        slit: &ast::StructLitExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let type_name = slit
            .type_name
            .as_ref()
            .map(|n| n.name.clone())
            .or_else(|| ctx.current_struct_type_name().map(String::from))
            .unwrap_or_else(|| "Unknown".to_owned());

        let fields = slit
            .fields
            .iter()
            .map(|f| {
                let value = self.lower_expr(&f.value, ctx, use_map, stmt_index);
                (f.name.name.clone(), value)
            })
            .collect();

        RustExpr::new(RustExprKind::StructLit { type_name, fields }, span)
    }

    /// Lower a template literal expression.
    ///
    /// - No interpolation: lowers to `"text".to_string()`
    /// - With interpolation: lowers to `format!("text{}text", expr, ...)`
    fn lower_template_lit(
        &self,
        tpl: &ast::TemplateLitExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Separate string parts and expression parts
        let mut strings: Vec<&str> = Vec::new();
        let mut exprs: Vec<&ast::Expr> = Vec::new();

        for part in &tpl.parts {
            match part {
                ast::TemplatePart::String(s, _) => strings.push(s),
                ast::TemplatePart::Expr(e) => exprs.push(e),
            }
        }

        // No interpolation: just a plain string
        if exprs.is_empty() {
            let text = strings.join("");
            let lit = RustExpr::new(RustExprKind::StringLit(text), span);
            return RustExpr::synthetic(RustExprKind::ToString(Box::new(lit)));
        }

        // Build the format string by joining string segments with `{}`
        let mut format_str = String::new();
        for (i, s) in strings.iter().enumerate() {
            format_str.push_str(s);
            if i < exprs.len() {
                format_str.push_str("{}");
            }
        }

        // Build the format! arguments: format string + lowered expressions
        let mut args = vec![RustExpr::synthetic(RustExprKind::StringLit(format_str))];
        for expr in &exprs {
            args.push(self.lower_expr(expr, ctx, use_map, stmt_index));
        }

        RustExpr::new(
            RustExprKind::Macro {
                name: "format".to_owned(),
                args,
            },
            span,
        )
    }

    /// Lower a `new` expression to a Rust static method call or vec literal.
    ///
    /// `new Map()` → `HashMap::new()`, `new Set()` → `HashSet::new()`,
    /// `new Array()` → `vec![]` (empty vec).
    fn lower_new_expr(
        &self,
        new_expr: &ast::NewExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let rust_type_name =
            rsc_typeck::resolve::map_collection_type_name(&new_expr.type_name.name);

        let args: Vec<RustExpr> = new_expr
            .args
            .iter()
            .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
            .collect();

        match rust_type_name.as_str() {
            "Vec" => {
                // `new Array()` → `vec![]` (empty vec literal)
                RustExpr::new(RustExprKind::VecLit(args), span)
            }
            _ => {
                // `new Map()` → `HashMap::new()`, `new Set()` → `HashSet::new()`
                RustExpr::new(
                    RustExprKind::StaticCall {
                        type_name: rust_type_name,
                        method: "new".to_owned(),
                        args,
                    },
                    span,
                )
            }
        }
    }
}

/// Scan generated items for usage of `HashMap` or `HashSet` types and produce
/// the corresponding `use std::collections::...` declarations.
fn collect_use_declarations(items: &[RustItem]) -> Vec<RustUseDecl> {
    let mut needs_hashmap = false;
    let mut needs_hashset = false;

    for item in items {
        scan_item_for_collections(item, &mut needs_hashmap, &mut needs_hashset);
    }

    let mut uses = Vec::new();
    if needs_hashmap {
        uses.push(RustUseDecl {
            path: "std::collections::HashMap".to_owned(),
            span: None,
        });
    }
    if needs_hashset {
        uses.push(RustUseDecl {
            path: "std::collections::HashSet".to_owned(),
            span: None,
        });
    }
    uses
}

/// Scan a single item for references to `HashMap` or `HashSet`.
fn scan_item_for_collections(item: &RustItem, needs_hashmap: &mut bool, needs_hashset: &mut bool) {
    match item {
        RustItem::Function(f) => {
            for p in &f.params {
                scan_type_for_collections(&p.ty, needs_hashmap, needs_hashset);
            }
            if let Some(ret) = &f.return_type {
                scan_type_for_collections(ret, needs_hashmap, needs_hashset);
            }
            scan_block_for_collections(&f.body, needs_hashmap, needs_hashset);
        }
        RustItem::Struct(s) => {
            for field in &s.fields {
                scan_type_for_collections(&field.ty, needs_hashmap, needs_hashset);
            }
        }
        RustItem::Enum(e) => {
            for variant in &e.variants {
                for field in &variant.fields {
                    scan_type_for_collections(&field.ty, needs_hashmap, needs_hashset);
                }
            }
        }
    }
}

/// Scan a type for `HashMap` or `HashSet` references.
fn scan_type_for_collections(ty: &RustType, needs_hashmap: &mut bool, needs_hashset: &mut bool) {
    match ty {
        RustType::Named(name) => {
            if name == "HashMap" {
                *needs_hashmap = true;
            } else if name == "HashSet" {
                *needs_hashset = true;
            }
        }
        RustType::Generic(base, args) => {
            scan_type_for_collections(base, needs_hashmap, needs_hashset);
            for arg in args {
                scan_type_for_collections(arg, needs_hashmap, needs_hashset);
            }
        }
        _ => {}
    }
}

/// Scan a block for `HashMap` or `HashSet` usage in expressions and statements.
fn scan_block_for_collections(
    block: &RustBlock,
    needs_hashmap: &mut bool,
    needs_hashset: &mut bool,
) {
    for stmt in &block.stmts {
        scan_stmt_for_collections(stmt, needs_hashmap, needs_hashset);
    }
    if let Some(expr) = &block.expr {
        scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
    }
}

/// Scan a statement for `HashMap` or `HashSet` usage.
fn scan_stmt_for_collections(stmt: &RustStmt, needs_hashmap: &mut bool, needs_hashset: &mut bool) {
    match stmt {
        RustStmt::Let(let_stmt) => {
            if let Some(ty) = &let_stmt.ty {
                scan_type_for_collections(ty, needs_hashmap, needs_hashset);
            }
            scan_expr_for_collections(&let_stmt.init, needs_hashmap, needs_hashset);
        }
        RustStmt::Expr(expr) | RustStmt::Semi(expr) => {
            scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
        }
        RustStmt::Return(ret) => {
            if let Some(val) = &ret.value {
                scan_expr_for_collections(val, needs_hashmap, needs_hashset);
            }
        }
        RustStmt::If(if_stmt) => {
            scan_expr_for_collections(&if_stmt.condition, needs_hashmap, needs_hashset);
            scan_block_for_collections(&if_stmt.then_block, needs_hashmap, needs_hashset);
            if let Some(else_clause) = &if_stmt.else_clause {
                match else_clause {
                    RustElse::Block(block) => {
                        scan_block_for_collections(block, needs_hashmap, needs_hashset);
                    }
                    RustElse::ElseIf(nested_if) => {
                        scan_expr_for_collections(
                            &nested_if.condition,
                            needs_hashmap,
                            needs_hashset,
                        );
                        scan_block_for_collections(
                            &nested_if.then_block,
                            needs_hashmap,
                            needs_hashset,
                        );
                    }
                }
            }
        }
        RustStmt::While(while_stmt) => {
            scan_expr_for_collections(&while_stmt.condition, needs_hashmap, needs_hashset);
            scan_block_for_collections(&while_stmt.body, needs_hashmap, needs_hashset);
        }
        RustStmt::Destructure(destr) => {
            scan_expr_for_collections(&destr.init, needs_hashmap, needs_hashset);
        }
        RustStmt::Match(match_stmt) => {
            scan_expr_for_collections(&match_stmt.scrutinee, needs_hashmap, needs_hashset);
            for arm in &match_stmt.arms {
                scan_block_for_collections(&arm.body, needs_hashmap, needs_hashset);
            }
        }
    }
}

/// Scan an expression for `HashMap` or `HashSet` usage.
fn scan_expr_for_collections(expr: &RustExpr, needs_hashmap: &mut bool, needs_hashset: &mut bool) {
    match &expr.kind {
        RustExprKind::StaticCall {
            type_name, args, ..
        } => {
            if type_name == "HashMap" {
                *needs_hashmap = true;
            } else if type_name == "HashSet" {
                *needs_hashset = true;
            }
            for arg in args {
                scan_expr_for_collections(arg, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::VecLit(elems) => {
            for elem in elems {
                scan_expr_for_collections(elem, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::Index { object, index } => {
            scan_expr_for_collections(object, needs_hashmap, needs_hashset);
            scan_expr_for_collections(index, needs_hashmap, needs_hashset);
        }
        RustExprKind::Binary { left, right, .. } => {
            scan_expr_for_collections(left, needs_hashmap, needs_hashset);
            scan_expr_for_collections(right, needs_hashmap, needs_hashset);
        }
        RustExprKind::Unary { operand, .. } => {
            scan_expr_for_collections(operand, needs_hashmap, needs_hashset);
        }
        RustExprKind::Call { args, .. } | RustExprKind::Macro { args, .. } => {
            for arg in args {
                scan_expr_for_collections(arg, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::MethodCall { receiver, args, .. } => {
            scan_expr_for_collections(receiver, needs_hashmap, needs_hashset);
            for arg in args {
                scan_expr_for_collections(arg, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::Paren(inner) | RustExprKind::Clone(inner) | RustExprKind::ToString(inner) => {
            scan_expr_for_collections(inner, needs_hashmap, needs_hashset);
        }
        RustExprKind::Assign { value, .. } | RustExprKind::CompoundAssign { value, .. } => {
            scan_expr_for_collections(value, needs_hashmap, needs_hashset);
        }
        RustExprKind::StructLit { fields, .. } => {
            for (_, val) in fields {
                scan_expr_for_collections(val, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::FieldAccess { object, .. } => {
            scan_expr_for_collections(object, needs_hashmap, needs_hashset);
        }
        RustExprKind::IntLit(_)
        | RustExprKind::FloatLit(_)
        | RustExprKind::StringLit(_)
        | RustExprKind::BoolLit(_)
        | RustExprKind::Ident(_)
        | RustExprKind::EnumVariant { .. } => {}
    }
}

/// Map a `RustScript` binary operator to a Rust binary operator.
fn lower_binary_op(op: ast::BinaryOp) -> RustBinaryOp {
    match op {
        ast::BinaryOp::Add => RustBinaryOp::Add,
        ast::BinaryOp::Sub => RustBinaryOp::Sub,
        ast::BinaryOp::Mul => RustBinaryOp::Mul,
        ast::BinaryOp::Div => RustBinaryOp::Div,
        ast::BinaryOp::Mod => RustBinaryOp::Rem,
        ast::BinaryOp::Eq => RustBinaryOp::Eq,
        ast::BinaryOp::Ne => RustBinaryOp::Ne,
        ast::BinaryOp::Lt => RustBinaryOp::Lt,
        ast::BinaryOp::Gt => RustBinaryOp::Gt,
        ast::BinaryOp::Le => RustBinaryOp::Le,
        ast::BinaryOp::Ge => RustBinaryOp::Ge,
        ast::BinaryOp::And => RustBinaryOp::And,
        ast::BinaryOp::Or => RustBinaryOp::Or,
    }
}

/// Map a `RustScript` unary operator to a Rust unary operator.
fn lower_unary_op(op: ast::UnaryOp) -> RustUnaryOp {
    match op {
        ast::UnaryOp::Neg => RustUnaryOp::Neg,
        ast::UnaryOp::Not => RustUnaryOp::Not,
    }
}

/// Collect generic parameter names from an optional `TypeParams`.
///
/// Returns a `Vec<String>` of type parameter names (e.g., `["T", "U"]`).
/// Used to set up the generic scope during lowering.
fn collect_generic_param_names(type_params: Option<&ast::TypeParams>) -> Vec<String> {
    match type_params {
        Some(tp) => tp.params.iter().map(|p| p.name.name.clone()).collect(),
        None => Vec::new(),
    }
}

/// Lower AST type parameters to Rust IR type parameters.
///
/// Maps `T extends Bound` to `RustTypeParam { name: "T", bounds: vec!["Bound"] }`.
fn lower_type_params(type_params: Option<&ast::TypeParams>) -> Vec<RustTypeParam> {
    match type_params {
        Some(tp) => tp
            .params
            .iter()
            .map(|p| {
                let bounds = p
                    .constraint
                    .as_ref()
                    .map(|c| match &c.kind {
                        ast::TypeKind::Named(ident) | ast::TypeKind::Generic(ident, _) => {
                            vec![ident.name.clone()]
                        }
                        ast::TypeKind::Void => vec![],
                    })
                    .unwrap_or_default();
                RustTypeParam {
                    name: p.name.name.clone(),
                    bounds,
                }
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Check if the expression is a literal whose default inferred type matches `ty`.
///
/// When this returns `true`, the type annotation can be omitted on the `let`
/// binding because Rust will infer the same type.
/// Capitalize the first letter of a string.
///
/// Used to derive Rust enum variant names from `RustScript` string literals.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let mut result = c.to_uppercase().to_string();
            result.push_str(chars.as_str());
            result
        }
        None => String::new(),
    }
}

fn is_default_literal_type(expr: &ast::Expr, ty: &RustType) -> bool {
    match &expr.kind {
        ast::ExprKind::IntLit(_) => *ty == RustType::I64,
        ast::ExprKind::FloatLit(_) => *ty == RustType::F64,
        ast::ExprKind::BoolLit(_) => *ty == RustType::Bool,
        ast::ExprKind::StringLit(_) | ast::ExprKind::TemplateLit(_) => *ty == RustType::String,
        _ => false,
    }
}

/// Detect the compound assignment pattern `x = x op rhs`.
///
/// If the value expression is `Binary(op, Ident(name), rhs)` where `name`
/// matches the assignment target, returns the compound operator and the rhs.
fn detect_compound_assign<'a>(
    target: &str,
    value: &'a ast::Expr,
) -> Option<(RustCompoundAssignOp, &'a ast::Expr)> {
    if let ast::ExprKind::Binary(bin) = &value.kind
        && let ast::ExprKind::Ident(ident) = &bin.left.kind
        && ident.name == target
    {
        let compound_op = match bin.op {
            ast::BinaryOp::Add => RustCompoundAssignOp::AddAssign,
            ast::BinaryOp::Sub => RustCompoundAssignOp::SubAssign,
            ast::BinaryOp::Mul => RustCompoundAssignOp::MulAssign,
            ast::BinaryOp::Div => RustCompoundAssignOp::DivAssign,
            ast::BinaryOp::Mod => RustCompoundAssignOp::RemAssign,
            _ => return None,
        };
        return Some((compound_op, &bin.right));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::ast::*;
    use rsc_syntax::span::Span;

    fn span(start: u32, end: u32) -> Span {
        Span::new(start, end)
    }

    fn ident(name: &str, start: u32, end: u32) -> Ident {
        Ident {
            name: name.to_owned(),
            span: span(start, end),
        }
    }

    fn int_expr(value: i64, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::IntLit(value),
            span: span(start, end),
        }
    }

    fn ident_expr(name: &str, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::Ident(ident(name, start, end)),
            span: span(start, end),
        }
    }

    fn string_expr(s: &str, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::StringLit(s.to_owned()),
            span: span(start, end),
        }
    }

    fn fn_item(f: FnDecl) -> Item {
        let item_span = f.span;
        Item {
            kind: ItemKind::Function(f),
            exported: false,
            span: item_span,
        }
    }

    fn make_module(items: Vec<Item>) -> Module {
        Module {
            items,
            span: span(0, 100),
        }
    }

    fn make_fn(
        name: &str,
        params: Vec<Param>,
        return_type: Option<TypeAnnotation>,
        body: Vec<Stmt>,
    ) -> FnDecl {
        FnDecl {
            name: ident(name, 0, name.len() as u32),
            type_params: None,
            params,
            return_type,
            body: Block {
                stmts: body,
                span: span(0, 100),
            },
            span: span(0, 100),
        }
    }

    fn make_param(name: &str, type_name: &str) -> Param {
        Param {
            name: ident(name, 0, name.len() as u32),
            type_ann: TypeAnnotation {
                kind: TypeKind::Named(ident(type_name, 0, type_name.len() as u32)),
                span: span(0, type_name.len() as u32),
            },
            span: span(0, 10),
        }
    }

    // Test 15: Lower empty function main()
    #[test]
    fn test_lower_empty_main_function() {
        let f = make_fn("main", vec![], None, vec![]);
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        assert_eq!(file.items.len(), 1);
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.name, "main");
        assert!(func.params.is_empty());
        assert!(func.return_type.is_none());
        assert!(func.body.stmts.is_empty());
        assert!(func.span.is_some());
    }

    // Test 16: Lower function params (a: i32, b: string): bool
    #[test]
    fn test_lower_function_params_and_return_type() {
        let f = make_fn(
            "test",
            vec![make_param("a", "i32"), make_param("b", "string")],
            Some(TypeAnnotation {
                kind: TypeKind::Named(ident("bool", 0, 4)),
                span: span(0, 4),
            }),
            vec![],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.params[0].name, "a");
        assert_eq!(func.params[0].ty, RustType::I32);
        assert_eq!(func.params[1].name, "b");
        assert_eq!(func.params[1].ty, RustType::String);
        assert_eq!(func.return_type, Some(RustType::Bool));
    }

    // Test 17: Lower const x: i32 = 42
    #[test]
    fn test_lower_const_with_type_annotation() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 1);
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(!let_stmt.mutable);
                assert_eq!(let_stmt.ty, Some(RustType::I32));
                assert_eq!(let_stmt.name, "x");
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test 18: Lower let x = 42; x = 10; → let mut
    #[test]
    fn test_lower_let_with_reassignment_becomes_mut() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![
                Stmt::VarDecl(VarDecl {
                    binding: VarBinding::Let,
                    name: ident("x", 4, 5),
                    type_ann: None,
                    init: int_expr(42, 8, 10),
                    span: span(0, 11),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident("x", 12, 13),
                        value: Box::new(int_expr(10, 16, 18)),
                    }),
                    span: span(12, 19),
                }),
            ],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 2);
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(
                    let_stmt.mutable,
                    "x should be mutable since it's reassigned"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
        // Second stmt should be Semi(Assign)
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Assign { target, .. } => assert_eq!(target, "x"),
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 19: Lower const x = 42; (no type ann) → omit type (inferable)
    #[test]
    fn test_lower_const_no_type_annotation_omits_type() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: None,
                init: int_expr(42, 10, 12),
                span: span(0, 13),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(
                    let_stmt.ty, None,
                    "type should be omitted when inferable from literal"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test 20: Lower console.log("hello") → println! via builtin registry
    #[test]
    fn test_lower_console_log_single_arg_produces_println_macro() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![string_expr("hello", 12, 19)],
                }),
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    assert_eq!(args.len(), 2);
                    match &args[0].kind {
                        RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{}"),
                        other => panic!("expected StringLit format, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 21: Lower console.log(x, y) → println! with format string "{} {}"
    #[test]
    fn test_lower_console_log_two_args_format_string() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![ident_expr("x", 12, 13), ident_expr("y", 15, 16)],
                }),
                span: span(0, 17),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    assert_eq!(args.len(), 3); // format string + 2 args
                    match &args[0].kind {
                        RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{} {}"),
                        other => panic!("expected StringLit format, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 22: Lower if (x > 0) { ... } → RustIfStmt sans parens
    #[test]
    fn test_lower_if_statement_with_condition() {
        let f = make_fn(
            "main",
            vec![make_param("x", "i32")],
            None,
            vec![Stmt::If(IfStmt {
                condition: Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Gt,
                        left: Box::new(ident_expr("x", 4, 5)),
                        right: Box::new(int_expr(0, 8, 9)),
                    }),
                    span: span(4, 9),
                },
                then_block: Block {
                    stmts: vec![],
                    span: span(11, 13),
                },
                else_clause: None,
                span: span(0, 13),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::If(if_stmt) => {
                assert!(if_stmt.span.is_some());
                match &if_stmt.condition.kind {
                    RustExprKind::Binary { op, .. } => {
                        assert_eq!(*op, RustBinaryOp::Gt);
                    }
                    other => panic!("expected Binary, got {other:?}"),
                }
                assert!(if_stmt.else_clause.is_none());
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    // Test 23: Lower binary % → RustBinaryOp::Rem
    #[test]
    fn test_lower_binary_mod_to_rem() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Mod,
                    left: Box::new(int_expr(10, 0, 2)),
                    right: Box::new(int_expr(3, 5, 6)),
                }),
                span: span(0, 6),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Binary { op, .. } => assert_eq!(*op, RustBinaryOp::Rem),
                other => panic!("expected Binary, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 24: Lower return; → RustStmt::Return with value: None
    #[test]
    fn test_lower_bare_return() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Return(ReturnStmt {
                value: None,
                span: span(0, 7),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Return(ret) => {
                assert!(ret.value.is_none());
                assert!(ret.span.is_some());
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // Test 25: Unknown type name → diagnostic emitted
    #[test]
    fn test_lower_unknown_type_emits_diagnostic() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("Foo", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (_, diags) = transform.lower_module(&module);

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown type"));
        assert!(diags[0].message.contains("Foo"));
    }

    // Correctness Scenario 1: Fibonacci lowering
    #[test]
    fn test_correctness_fibonacci_lowering() {
        // function fib(n: i32): i32 {
        //   if (n <= 1) { return n; }
        //   return fib(n - 1) + fib(n - 2);
        // }
        let f = FnDecl {
            name: ident("fib", 0, 3),
            type_params: None,
            params: vec![make_param("n", "i32")],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("i32", 0, 3)),
                span: span(0, 3),
            }),
            body: Block {
                stmts: vec![
                    Stmt::If(IfStmt {
                        condition: Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Le,
                                left: Box::new(ident_expr("n", 10, 11)),
                                right: Box::new(int_expr(1, 15, 16)),
                            }),
                            span: span(10, 16),
                        },
                        then_block: Block {
                            stmts: vec![Stmt::Return(ReturnStmt {
                                value: Some(ident_expr("n", 20, 21)),
                                span: span(18, 22),
                            })],
                            span: span(17, 23),
                        },
                        else_clause: None,
                        span: span(7, 23),
                    }),
                    Stmt::Return(ReturnStmt {
                        value: Some(Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Add,
                                left: Box::new(Expr {
                                    kind: ExprKind::Call(CallExpr {
                                        callee: ident("fib", 30, 33),
                                        args: vec![Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: BinaryOp::Sub,
                                                left: Box::new(ident_expr("n", 34, 35)),
                                                right: Box::new(int_expr(1, 38, 39)),
                                            }),
                                            span: span(34, 39),
                                        }],
                                    }),
                                    span: span(30, 40),
                                }),
                                right: Box::new(Expr {
                                    kind: ExprKind::Call(CallExpr {
                                        callee: ident("fib", 43, 46),
                                        args: vec![Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: BinaryOp::Sub,
                                                left: Box::new(ident_expr("n", 47, 48)),
                                                right: Box::new(int_expr(2, 51, 52)),
                                            }),
                                            span: span(47, 52),
                                        }],
                                    }),
                                    span: span(43, 53),
                                }),
                            }),
                            span: span(30, 53),
                        }),
                        span: span(24, 54),
                    }),
                ],
                span: span(5, 55),
            },
            span: span(0, 55),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.name, "fib");
        assert_eq!(func.params.len(), 1);
        assert_eq!(func.params[0].name, "n");
        assert_eq!(func.params[0].ty, RustType::I32);
        assert_eq!(func.return_type, Some(RustType::I32));
        assert_eq!(func.body.stmts.len(), 2);
        assert!(func.span.is_some());

        // Verify all spans are Some
        assert!(func.params[0].span.is_some());
        match &func.body.stmts[0] {
            RustStmt::If(if_stmt) => {
                assert!(if_stmt.span.is_some());
                assert!(if_stmt.condition.span.is_some());
            }
            other => panic!("expected If, got {other:?}"),
        }
        match &func.body.stmts[1] {
            RustStmt::Return(ret) => {
                assert!(ret.span.is_some());
                assert!(ret.value.as_ref().unwrap().span.is_some());
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // Correctness Scenario 2: String - no clones for println! args
    #[test]
    fn test_correctness_no_clones_for_println_args() {
        // function example(name: string): void {
        //   console.log(name);   // stmt 0: NOT a move position
        //   console.log(name);   // stmt 1: NOT a move position
        // }
        let f = FnDecl {
            name: ident("example", 0, 7),
            type_params: None,
            params: vec![make_param("name", "string")],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            }),
            body: Block {
                stmts: vec![
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 30, 37)),
                            method: ident("log", 38, 41),
                            args: vec![ident_expr("name", 42, 46)],
                        }),
                        span: span(30, 47),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 50, 57)),
                            method: ident("log", 58, 61),
                            args: vec![ident_expr("name", 62, 66)],
                        }),
                        span: span(50, 67),
                    }),
                ],
                span: span(28, 68),
            },
            span: span(0, 68),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        // Both statements should be println! macros with NO clones
        for (i, stmt) in func.body.stmts.iter().enumerate() {
            match stmt {
                RustStmt::Semi(expr) => match &expr.kind {
                    RustExprKind::Macro { name, args } => {
                        assert_eq!(name, "println");
                        // The second arg should be an Ident, not a Clone
                        assert!(
                            args.len() >= 2,
                            "stmt {i}: expected at least 2 args in println!"
                        );
                        match &args[1].kind {
                            RustExprKind::Ident(n) => assert_eq!(n, "name"),
                            RustExprKind::Clone(_) => {
                                panic!("stmt {i}: name should NOT be cloned for println!")
                            }
                            other => panic!("stmt {i}: expected Ident, got {other:?}"),
                        }
                    }
                    other => panic!("stmt {i}: expected Macro, got {other:?}"),
                },
                other => panic!("stmt {i}: expected Semi, got {other:?}"),
            }
        }
    }

    // Correctness Scenario 3: String clone when actually needed
    #[test]
    fn test_correctness_string_clone_at_move_point() {
        // function example(name: string): void {
        //   greet(name);          // stmt 0: move position, name used later → clone
        //   console.log(name);    // stmt 1: not a move position, no clone
        // }
        let f = FnDecl {
            name: ident("example", 0, 7),
            type_params: None,
            params: vec![make_param("name", "string")],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            }),
            body: Block {
                stmts: vec![
                    Stmt::Expr(Expr {
                        kind: ExprKind::Call(CallExpr {
                            callee: ident("greet", 30, 35),
                            args: vec![ident_expr("name", 36, 40)],
                        }),
                        span: span(30, 41),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 45, 52)),
                            method: ident("log", 53, 56),
                            args: vec![ident_expr("name", 57, 61)],
                        }),
                        span: span(45, 62),
                    }),
                ],
                span: span(28, 63),
            },
            span: span(0, 63),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 2);

        // stmt 0: greet(name.clone()) — name is in move position and used later
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Call { func: f, args } => {
                    assert_eq!(f, "greet");
                    assert_eq!(args.len(), 1);
                    match &args[0].kind {
                        RustExprKind::Clone(inner) => match &inner.kind {
                            RustExprKind::Ident(n) => assert_eq!(n, "name"),
                            other => panic!("expected Ident inside Clone, got {other:?}"),
                        },
                        other => panic!("expected Clone, got {other:?}"),
                    }
                }
                other => panic!("expected Call, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }

        // stmt 1: println! — name is NOT cloned
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    match &args[1].kind {
                        RustExprKind::Ident(n) => assert_eq!(n, "name"),
                        RustExprKind::Clone(_) => panic!("name should NOT be cloned in println!"),
                        other => panic!("expected Ident, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Correctness Scenario 4: Mutability detection
    #[test]
    fn test_correctness_mutability_detection() {
        // function counter(): void {
        //   let x = 0;
        //   const y = 10;
        //   x = x + 1;
        // }
        let f = FnDecl {
            name: ident("counter", 0, 7),
            type_params: None,
            params: vec![],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            }),
            body: Block {
                stmts: vec![
                    Stmt::VarDecl(VarDecl {
                        binding: VarBinding::Let,
                        name: ident("x", 20, 21),
                        type_ann: None,
                        init: int_expr(0, 24, 25),
                        span: span(16, 26),
                    }),
                    Stmt::VarDecl(VarDecl {
                        binding: VarBinding::Const,
                        name: ident("y", 33, 34),
                        type_ann: None,
                        init: int_expr(10, 37, 39),
                        span: span(27, 40),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::Assign(AssignExpr {
                            target: ident("x", 41, 42),
                            value: Box::new(Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Add,
                                    left: Box::new(ident_expr("x", 45, 46)),
                                    right: Box::new(int_expr(1, 49, 50)),
                                }),
                                span: span(45, 50),
                            }),
                        }),
                        span: span(41, 51),
                    }),
                ],
                span: span(14, 52),
            },
            span: span(0, 52),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 3);

        // x is let mut (reassigned)
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(let_stmt.name, "x");
                assert!(let_stmt.mutable, "x should be let mut (reassigned)");
            }
            other => panic!("expected Let for x, got {other:?}"),
        }

        // y is let (const, not reassigned)
        match &func.body.stmts[1] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(let_stmt.name, "y");
                assert!(!let_stmt.mutable, "y should be let (const)");
            }
            other => panic!("expected Let for y, got {other:?}"),
        }
    }

    // Test: Compound assignment lowering — x = x + 1 becomes CompoundAssign
    #[test]
    fn test_lower_compound_assign_add() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![
                Stmt::VarDecl(VarDecl {
                    binding: VarBinding::Let,
                    name: ident("x", 4, 5),
                    type_ann: None,
                    init: int_expr(0, 8, 9),
                    span: span(0, 10),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident("x", 11, 12),
                        value: Box::new(Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Add,
                                left: Box::new(ident_expr("x", 15, 16)),
                                right: Box::new(int_expr(1, 19, 20)),
                            }),
                            span: span(15, 20),
                        }),
                    }),
                    span: span(11, 21),
                }),
            ],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::CompoundAssign { target, op, .. } => {
                    assert_eq!(target, "x");
                    assert_eq!(*op, RustCompoundAssignOp::AddAssign);
                }
                other => panic!("expected CompoundAssign, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test: x = y + 1 does NOT become compound assign (target != lhs)
    #[test]
    fn test_lower_non_compound_assign_different_ident() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Assign(AssignExpr {
                    target: ident("x", 0, 1),
                    value: Box::new(Expr {
                        kind: ExprKind::Binary(BinaryExpr {
                            op: BinaryOp::Add,
                            left: Box::new(ident_expr("y", 4, 5)),
                            right: Box::new(int_expr(1, 8, 9)),
                        }),
                        span: span(4, 9),
                    }),
                }),
                span: span(0, 10),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Assign { target, .. } => assert_eq!(target, "x"),
                other => panic!("expected Assign (not CompoundAssign), got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test: console.log("hello") strips .to_string() from string arg
    #[test]
    fn test_lower_console_log_string_arg_no_to_string() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![string_expr("hello", 12, 19)],
                }),
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { args, .. } => {
                    // args[1] should be StringLit, NOT ToString(StringLit)
                    match &args[1].kind {
                        RustExprKind::StringLit(s) => assert_eq!(s, "hello"),
                        RustExprKind::ToString(_) => {
                            panic!("string arg in println! should NOT be wrapped in .to_string()")
                        }
                        other => panic!("expected StringLit, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test: console.log(name) where name is a variable — still works
    #[test]
    fn test_lower_console_log_ident_arg_not_stripped() {
        let f = make_fn(
            "main",
            vec![make_param("name", "string")],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![ident_expr("name", 12, 16)],
                }),
                span: span(0, 17),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { args, .. } => {
                    // args[1] should be Ident, not ToString-wrapped
                    match &args[1].kind {
                        RustExprKind::Ident(n) => assert_eq!(n, "name"),
                        other => panic!("expected Ident(name), got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test: Explicit type annotation is preserved even when it matches default
    #[test]
    fn test_lower_explicit_type_annotation_preserved() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i64", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(
                    let_stmt.ty,
                    Some(RustType::I64),
                    "explicit i64 annotation should be preserved"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test: Explicit non-default type annotation is preserved
    #[test]
    fn test_lower_explicit_i32_annotation_preserved() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(
                    let_stmt.ty,
                    Some(RustType::I32),
                    "explicit i32 annotation should be preserved"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 014: Type definitions and struct sugar lowering
    // ---------------------------------------------------------------

    // Test T14-6: Lower TypeDef -> RustStructDef with pub fields
    #[test]
    fn test_lower_type_def_produces_struct_with_pub_fields() {
        let td = ast::TypeDef {
            name: ident("User", 0, 4),
            type_params: None,
            fields: vec![
                ast::FieldDef {
                    name: ident("name", 0, 4),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("string", 0, 6)),
                        span: span(0, 6),
                    },
                    span: span(0, 10),
                },
                ast::FieldDef {
                    name: ident("age", 0, 3),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("u32", 0, 3)),
                        span: span(0, 3),
                    },
                    span: span(0, 6),
                },
            ],
            span: span(0, 50),
        };
        let module = Module {
            items: vec![Item {
                kind: ItemKind::TypeDef(td),
                exported: false,
                span: span(0, 50),
            }],
            span: span(0, 50),
        };
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);
        assert!(diags.is_empty());
        assert_eq!(file.items.len(), 1);
        let RustItem::Struct(s) = &file.items[0] else {
            panic!("expected Struct item");
        };
        assert_eq!(s.name, "User");
        assert_eq!(s.fields.len(), 2);
        assert!(s.fields[0].public);
        assert_eq!(s.fields[0].name, "name");
        assert_eq!(s.fields[0].ty, RustType::String);
        assert!(s.fields[1].public);
        assert_eq!(s.fields[1].name, "age");
        assert_eq!(s.fields[1].ty, RustType::U32);
    }

    // Test T14-7: Lower struct literal -> RustExprKind::StructLit
    #[test]
    fn test_lower_struct_literal_produces_struct_lit_expr() {
        let td = ast::TypeDef {
            name: ident("Point", 0, 5),
            type_params: None,
            fields: vec![
                ast::FieldDef {
                    name: ident("x", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("f64", 0, 3)),
                        span: span(0, 3),
                    },
                    span: span(0, 4),
                },
                ast::FieldDef {
                    name: ident("y", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("f64", 0, 3)),
                        span: span(0, 3),
                    },
                    span: span(0, 4),
                },
            ],
            span: span(0, 30),
        };
        let body = vec![Stmt::VarDecl(VarDecl {
            binding: VarBinding::Const,
            name: ident("p", 0, 1),
            type_ann: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("Point", 0, 5)),
                span: span(0, 5),
            }),
            init: Expr {
                kind: ExprKind::StructLit(ast::StructLitExpr {
                    type_name: None,
                    fields: vec![
                        ast::FieldInit {
                            name: ident("x", 0, 1),
                            value: Expr {
                                kind: ExprKind::FloatLit(1.0),
                                span: span(0, 3),
                            },
                            span: span(0, 4),
                        },
                        ast::FieldInit {
                            name: ident("y", 0, 1),
                            value: Expr {
                                kind: ExprKind::FloatLit(2.0),
                                span: span(0, 3),
                            },
                            span: span(0, 4),
                        },
                    ],
                }),
                span: span(0, 20),
            },
            span: span(0, 25),
        })];
        let module = Module {
            items: vec![
                Item {
                    kind: ItemKind::TypeDef(td),
                    exported: false,
                    span: span(0, 30),
                },
                fn_item(make_fn("main", vec![], None, body)),
            ],
            span: span(0, 100),
        };
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);
        assert!(diags.is_empty());
        // The function is the second item
        let RustItem::Function(func) = &file.items[1] else {
            panic!("expected Function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                RustExprKind::StructLit { type_name, fields } => {
                    assert_eq!(type_name, "Point");
                    assert_eq!(fields.len(), 2);
                    assert_eq!(fields[0].0, "x");
                    assert_eq!(fields[1].0, "y");
                }
                other => panic!("expected StructLit, got {other:?}"),
            },
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test T14-8: Lower field access -> RustExprKind::FieldAccess
    #[test]
    fn test_lower_field_access_produces_field_access_expr() {
        let body = vec![
            Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 0, 3)),
                    span: span(0, 3),
                }),
                init: int_expr(42, 0, 2),
                span: span(0, 10),
            }),
            Stmt::Expr(Expr {
                kind: ExprKind::FieldAccess(ast::FieldAccessExpr {
                    object: Box::new(ident_expr("obj", 0, 3)),
                    field: ident("name", 4, 8),
                }),
                span: span(0, 8),
            }),
        ];
        let module = make_module(vec![fn_item(make_fn("main", vec![], None, body))]);
        let mut transform = Transform::new();
        let (file, _diags) = transform.lower_module(&module);
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected Function item");
        };
        // Second stmt should be the field access
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::FieldAccess { object: _, field } => {
                    assert_eq!(field, "name");
                }
                other => panic!("expected FieldAccess, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // ---- Task 016: Generics lowering ----

    // Test T16-6: Lower generic function → RustFnDecl with type_params
    #[test]
    fn test_lower_generic_fn_produces_type_params() {
        let f = FnDecl {
            name: ident("id", 0, 2),
            type_params: Some(ast::TypeParams {
                params: vec![ast::TypeParam {
                    name: ident("T", 0, 1),
                    constraint: None,
                    span: span(0, 1),
                }],
                span: span(0, 3),
            }),
            params: vec![Param {
                name: ident("x", 0, 1),
                type_ann: TypeAnnotation {
                    kind: TypeKind::Named(ident("T", 0, 1)),
                    span: span(0, 1),
                },
                span: span(0, 3),
            }],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("T", 0, 1)),
                span: span(0, 1),
            }),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(ident_expr("x", 0, 1)),
                    span: span(0, 10),
                })],
                span: span(0, 20),
            },
            span: span(0, 30),
        };
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected Function");
        };
        assert_eq!(func.type_params.len(), 1);
        assert_eq!(func.type_params[0].name, "T");
        assert!(func.type_params[0].bounds.is_empty());
        assert_eq!(func.params[0].ty, RustType::TypeParam("T".to_owned()));
        assert_eq!(func.return_type, Some(RustType::TypeParam("T".to_owned())));
    }

    // Test T16-7: Lower constrained generic → RustTypeParam with bounds
    #[test]
    fn test_lower_constrained_generic_produces_bounds() {
        let f = FnDecl {
            name: ident("merge", 0, 5),
            type_params: Some(ast::TypeParams {
                params: vec![ast::TypeParam {
                    name: ident("T", 0, 1),
                    constraint: Some(TypeAnnotation {
                        kind: TypeKind::Named(ident("Comparable", 0, 10)),
                        span: span(0, 10),
                    }),
                    span: span(0, 20),
                }],
                span: span(0, 22),
            }),
            params: vec![
                Param {
                    name: ident("a", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("T", 0, 1)),
                        span: span(0, 1),
                    },
                    span: span(0, 3),
                },
                Param {
                    name: ident("b", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("T", 0, 1)),
                        span: span(0, 1),
                    },
                    span: span(0, 3),
                },
            ],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("T", 0, 1)),
                span: span(0, 1),
            }),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(ident_expr("a", 0, 1)),
                    span: span(0, 10),
                })],
                span: span(0, 20),
            },
            span: span(0, 50),
        };
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected Function");
        };
        assert_eq!(func.type_params.len(), 1);
        assert_eq!(func.type_params[0].name, "T");
        assert_eq!(func.type_params[0].bounds, vec!["Comparable".to_owned()]);
    }

    // Test T16-8: Lower generic struct → RustStructDef with type_params
    #[test]
    fn test_lower_generic_struct_produces_type_params() {
        let td = ast::TypeDef {
            name: ident("Container", 0, 9),
            type_params: Some(ast::TypeParams {
                params: vec![ast::TypeParam {
                    name: ident("T", 0, 1),
                    constraint: None,
                    span: span(0, 1),
                }],
                span: span(0, 3),
            }),
            fields: vec![ast::FieldDef {
                name: ident("value", 0, 5),
                type_ann: TypeAnnotation {
                    kind: TypeKind::Named(ident("T", 0, 1)),
                    span: span(0, 1),
                },
                span: span(0, 8),
            }],
            span: span(0, 30),
        };
        let module = make_module(vec![Item {
            kind: ItemKind::TypeDef(td),
            exported: false,
            span: span(0, 30),
        }]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[0] else {
            panic!("expected Struct");
        };
        assert_eq!(s.name, "Container");
        assert_eq!(s.type_params.len(), 1);
        assert_eq!(s.type_params[0].name, "T");
        assert_eq!(s.fields[0].ty, RustType::TypeParam("T".to_owned()));
    }

    // ---------------------------------------------------------------
    // Template literal lowering tests
    // ---------------------------------------------------------------

    // Test: Lower no-interpolation template → .to_string()
    #[test]
    fn test_lower_template_no_interpolation_produces_to_string() {
        let template_expr = Expr {
            kind: ExprKind::TemplateLit(ast::TemplateLitExpr {
                parts: vec![ast::TemplatePart::String(
                    "hello world".to_owned(),
                    span(0, 11),
                )],
            }),
            span: span(0, 14),
        };
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("msg", 0, 3),
                type_ann: None,
                init: template_expr,
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let stmt");
        };
        // Should be .to_string() wrapping a string literal
        assert!(
            matches!(&let_stmt.init.kind, RustExprKind::ToString(inner) if
                matches!(&inner.kind, RustExprKind::StringLit(s) if s == "hello world")
            ),
            "expected ToString(StringLit(\"hello world\")), got {:?}",
            let_stmt.init.kind
        );
    }

    // Test: Lower single interpolation → format!("Hello, {}!", name)
    #[test]
    fn test_lower_template_single_interpolation_produces_format_macro() {
        let template_expr = Expr {
            kind: ExprKind::TemplateLit(ast::TemplateLitExpr {
                parts: vec![
                    ast::TemplatePart::String("Hello, ".to_owned(), span(0, 7)),
                    ast::TemplatePart::Expr(ident_expr("name", 10, 14)),
                    ast::TemplatePart::String("!".to_owned(), span(15, 16)),
                ],
            }),
            span: span(0, 18),
        };
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("greeting", 0, 8),
                type_ann: None,
                init: template_expr,
                span: span(0, 25),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let stmt");
        };
        let RustExprKind::Macro { name, args } = &let_stmt.init.kind else {
            panic!("expected Macro, got {:?}", let_stmt.init.kind);
        };
        assert_eq!(name, "format");
        assert_eq!(args.len(), 2);
        assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "Hello, {}!"));
        assert!(matches!(&args[1].kind, RustExprKind::Ident(n) if n == "name"));
    }

    // Test: Lower multiple interpolations → format! with multiple args
    #[test]
    fn test_lower_template_multiple_interpolations_produces_format_with_multiple_args() {
        let template_expr = Expr {
            kind: ExprKind::TemplateLit(ast::TemplateLitExpr {
                parts: vec![
                    ast::TemplatePart::String(String::new(), span(0, 0)),
                    ast::TemplatePart::Expr(ident_expr("a", 2, 3)),
                    ast::TemplatePart::String(" + ".to_owned(), span(4, 7)),
                    ast::TemplatePart::Expr(ident_expr("b", 9, 10)),
                    ast::TemplatePart::String(" = ".to_owned(), span(11, 14)),
                    ast::TemplatePart::Expr(ident_expr("c", 16, 17)),
                    ast::TemplatePart::String(String::new(), span(18, 18)),
                ],
            }),
            span: span(0, 20),
        };
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: None,
                init: template_expr,
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let stmt");
        };
        let RustExprKind::Macro { name, args } = &let_stmt.init.kind else {
            panic!("expected Macro, got {:?}", let_stmt.init.kind);
        };
        assert_eq!(name, "format");
        assert_eq!(args.len(), 4); // format string + 3 exprs
        assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "{} + {} = {}"));
    }

    // Test: Lower expression interpolation → format!("Result: {}", x + y)
    #[test]
    fn test_lower_template_expression_interpolation_produces_format_with_binary() {
        let binary_expr = Expr {
            kind: ExprKind::Binary(BinaryExpr {
                op: BinaryOp::Add,
                left: Box::new(ident_expr("x", 10, 11)),
                right: Box::new(ident_expr("y", 14, 15)),
            }),
            span: span(10, 15),
        };
        let template_expr = Expr {
            kind: ExprKind::TemplateLit(ast::TemplateLitExpr {
                parts: vec![
                    ast::TemplatePart::String("Result: ".to_owned(), span(0, 8)),
                    ast::TemplatePart::Expr(binary_expr),
                    ast::TemplatePart::String(String::new(), span(16, 16)),
                ],
            }),
            span: span(0, 18),
        };
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: None,
                init: template_expr,
                span: span(0, 25),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let stmt");
        };
        let RustExprKind::Macro { name, args } = &let_stmt.init.kind else {
            panic!("expected Macro, got {:?}", let_stmt.init.kind);
        };
        assert_eq!(name, "format");
        assert_eq!(args.len(), 2);
        assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "Result: {}"));
        assert!(matches!(&args[1].kind, RustExprKind::Binary { .. }));
    }

    // ---- Task 015: Enum lowering tests ----

    // Test T015-4: Lower simple enum → RustEnumDef with fieldless variants, names capitalized
    #[test]
    fn test_lower_simple_enum_capitalized_variants() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::EnumDef(EnumDef {
                    name: ident("Direction", 0, 9),
                    variants: vec![
                        EnumVariant::Simple(ident("North", 0, 5), span(0, 5)),
                        EnumVariant::Simple(ident("South", 0, 5), span(0, 5)),
                        EnumVariant::Simple(ident("East", 0, 4), span(0, 4)),
                        EnumVariant::Simple(ident("West", 0, 4), span(0, 4)),
                    ],
                    span: span(0, 50),
                }),
                exported: false,
                span: span(0, 50),
            }],
            span: span(0, 50),
        };

        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);
        assert_eq!(file.items.len(), 1);
        match &file.items[0] {
            RustItem::Enum(e) => {
                assert_eq!(e.name, "Direction");
                assert_eq!(e.variants.len(), 4);
                assert_eq!(e.variants[0].name, "North");
                assert!(e.variants[0].fields.is_empty());
                assert_eq!(e.variants[3].name, "West");
            }
            _ => panic!("expected Enum item"),
        }
    }

    // Test T015-5: Lower data enum → RustEnumDef with struct variants, kind field removed
    #[test]
    fn test_lower_data_enum_struct_variants() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::EnumDef(EnumDef {
                    name: ident("Shape", 0, 5),
                    variants: vec![
                        EnumVariant::Data {
                            discriminant_value: "circle".to_owned(),
                            name: ident("Circle", 0, 6),
                            fields: vec![FieldDef {
                                name: ident("radius", 0, 6),
                                type_ann: TypeAnnotation {
                                    kind: TypeKind::Named(ident("f64", 0, 3)),
                                    span: span(0, 3),
                                },
                                span: span(0, 10),
                            }],
                            span: span(0, 30),
                        },
                        EnumVariant::Data {
                            discriminant_value: "rect".to_owned(),
                            name: ident("Rect", 0, 4),
                            fields: vec![
                                FieldDef {
                                    name: ident("width", 0, 5),
                                    type_ann: TypeAnnotation {
                                        kind: TypeKind::Named(ident("f64", 0, 3)),
                                        span: span(0, 3),
                                    },
                                    span: span(0, 10),
                                },
                                FieldDef {
                                    name: ident("height", 0, 6),
                                    type_ann: TypeAnnotation {
                                        kind: TypeKind::Named(ident("f64", 0, 3)),
                                        span: span(0, 3),
                                    },
                                    span: span(0, 10),
                                },
                            ],
                            span: span(0, 50),
                        },
                    ],
                    span: span(0, 80),
                }),
                exported: false,
                span: span(0, 80),
            }],
            span: span(0, 80),
        };

        let mut transform = Transform::new();
        let (file, _) = transform.lower_module(&module);
        match &file.items[0] {
            RustItem::Enum(e) => {
                assert_eq!(e.name, "Shape");
                assert_eq!(e.variants.len(), 2);
                assert_eq!(e.variants[0].name, "Circle");
                assert_eq!(e.variants[0].fields.len(), 1);
                assert_eq!(e.variants[0].fields[0].name, "radius");
                assert_eq!(e.variants[1].name, "Rect");
                assert_eq!(e.variants[1].fields.len(), 2);
            }
            _ => panic!("expected Enum item"),
        }
    }

    // ---------------------------------------------------------------
    // Task 017: Collection lowering
    // ---------------------------------------------------------------

    // Test T17-7: Lower array literal → RustExprKind::VecLit
    #[test]
    fn test_lower_array_literal_produces_vec_lit() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("nums", 0, 4),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::ArrayLit(vec![
                        int_expr(1, 0, 1),
                        int_expr(2, 3, 4),
                        int_expr(3, 6, 7),
                    ]),
                    span: span(0, 8),
                },
                span: span(0, 10),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);
        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        assert!(matches!(let_stmt.init.kind, RustExprKind::VecLit(_)));
        if let RustExprKind::VecLit(elems) = &let_stmt.init.kind {
            assert_eq!(elems.len(), 3);
        }
    }

    // Test T17-8: Lower `new Map()` → StaticCall { type_name: "HashMap", method: "new" }
    #[test]
    fn test_lower_new_map_produces_static_call_hashmap() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("lookup", 0, 6),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Generic(
                        ident("Map", 0, 3),
                        vec![
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("string", 0, 6)),
                                span: span(0, 6),
                            },
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("i32", 0, 3)),
                                span: span(0, 3),
                            },
                        ],
                    ),
                    span: span(0, 20),
                }),
                init: Expr {
                    kind: ExprKind::New(ast::NewExpr {
                        type_name: ident("Map", 0, 3),
                        type_args: vec![],
                        args: vec![],
                    }),
                    span: span(0, 10),
                },
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);
        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        match &let_stmt.init.kind {
            RustExprKind::StaticCall {
                type_name, method, ..
            } => {
                assert_eq!(type_name, "HashMap");
                assert_eq!(method, "new");
            }
            _ => panic!("expected StaticCall, got {:?}", let_stmt.init.kind),
        }
    }

    // Test T17-9: Lower index access → RustExprKind::Index
    #[test]
    fn test_lower_index_access_produces_index() {
        let f = make_fn(
            "main",
            vec![make_param("arr", "i32")],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Index(ast::IndexExpr {
                    object: Box::new(ident_expr("arr", 0, 3)),
                    index: Box::new(int_expr(0, 4, 5)),
                }),
                span: span(0, 6),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);
        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Semi(expr) = &func.body.stmts[0] else {
            panic!("expected Semi");
        };
        assert!(matches!(expr.kind, RustExprKind::Index { .. }));
    }

    // Test T17-10: Lower `Array<string>` type → Vec<String> in Rust type
    #[test]
    fn test_lower_array_string_type_to_vec_string() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("names", 0, 5),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Generic(
                        ident("Array", 0, 5),
                        vec![TypeAnnotation {
                            kind: TypeKind::Named(ident("string", 0, 6)),
                            span: span(0, 6),
                        }],
                    ),
                    span: span(0, 13),
                }),
                init: Expr {
                    kind: ExprKind::ArrayLit(vec![]),
                    span: span(0, 2),
                },
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);
        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        // Type should be Vec<String>
        let expected_ty = RustType::Generic(
            Box::new(RustType::Named("Vec".to_owned())),
            vec![RustType::String],
        );
        assert_eq!(let_stmt.ty, Some(expected_ty));
    }

    // Test T17-14: use declarations generated for HashMap
    #[test]
    fn test_lower_map_generates_use_hashmap() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("lookup", 0, 6),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Generic(
                        ident("Map", 0, 3),
                        vec![
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("string", 0, 6)),
                                span: span(0, 6),
                            },
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("i32", 0, 3)),
                                span: span(0, 3),
                            },
                        ],
                    ),
                    span: span(0, 20),
                }),
                init: Expr {
                    kind: ExprKind::New(ast::NewExpr {
                        type_name: ident("Map", 0, 3),
                        type_args: vec![],
                        args: vec![],
                    }),
                    span: span(0, 10),
                },
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _diags) = transform.lower_module(&module);

        assert!(!file.uses.is_empty(), "expected use declarations");
        assert!(
            file.uses
                .iter()
                .any(|u| u.path == "std::collections::HashMap"),
            "expected use std::collections::HashMap"
        );
    }
}
