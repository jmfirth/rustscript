//! Switch/match statement lowering.
//!
//! Handles lowering of `switch` statements to Rust `match` statements,
//! including enum variant pattern matching and field destructuring rewrites.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{
    RustBlock, RustExpr, RustExprKind, RustMatchArm, RustMatchStmt, RustPattern, RustReturnStmt,
    RustStmt,
};

use crate::context::LoweringContext;
use crate::ownership::UseMap;

use super::{Transform, capitalize_first, extract_named_type, lower_binary_op};

/// Classification of a switch statement based on its case patterns.
enum SwitchKind {
    /// Integer literal patterns: `case 1:`, `case 2:`.
    Integer,
    /// Enum member access patterns: `case Color.Red:`.
    EnumMember,
    /// String literal patterns (original behavior): `case "north":`.
    StringEnum,
}

impl Transform {
    /// Lower a switch statement to a Rust match statement.
    ///
    /// Handles three kinds of switch patterns:
    /// - **String literals** (original behavior): resolves the scrutinee type to determine
    ///   the enum being matched and generates `EnumVariant` or `EnumVariantFields` patterns.
    /// - **Integer literals**: generates `IntLiteral` patterns for numeric matching.
    /// - **Enum member access** (`Color.Red`): generates `EnumVariant` patterns directly.
    /// - **Default**: generates `Wildcard` (`_`) patterns.
    ///
    /// For data enums with string-based patterns, rewrites `scrutinee.field` to just `field`
    /// inside case bodies.
    pub(super) fn lower_switch(
        &self,
        switch: &ast::SwitchStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        let scrutinee = self.lower_expr(&switch.scrutinee, ctx, use_map, stmt_index);

        let scrutinee_var_name = match &switch.scrutinee.kind {
            ast::ExprKind::Ident(ident) => Some(ident.name.clone()),
            _ => None,
        };

        // Determine switch kind from the patterns present
        let switch_kind = Self::classify_switch(&switch.cases);

        match switch_kind {
            SwitchKind::Integer => self.lower_integer_switch(
                switch, scrutinee, ctx, use_map, stmt_index, reassigned,
            ),
            SwitchKind::EnumMember => self.lower_enum_member_switch(
                switch, scrutinee, ctx, use_map, stmt_index, reassigned,
            ),
            SwitchKind::StringEnum => self.lower_string_enum_switch(
                switch,
                scrutinee,
                ctx,
                use_map,
                stmt_index,
                reassigned,
                scrutinee_var_name,
            ),
        }
    }

    /// Classify a switch statement based on its case patterns.
    fn classify_switch(cases: &[ast::SwitchCase]) -> SwitchKind {
        for case in cases {
            match &case.pattern {
                ast::SwitchPattern::IntLit(_) => return SwitchKind::Integer,
                ast::SwitchPattern::EnumMember(_, _) => return SwitchKind::EnumMember,
                ast::SwitchPattern::StringLit(_) => return SwitchKind::StringEnum,
                ast::SwitchPattern::Default => continue,
            }
        }
        // All-default or empty: default to string enum for backwards compat
        SwitchKind::StringEnum
    }

    /// Lower a switch on integer values.
    fn lower_integer_switch(
        &self,
        switch: &ast::SwitchStmt,
        scrutinee: RustExpr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        let arms: Vec<RustMatchArm> = switch
            .cases
            .iter()
            .map(|case| {
                let pattern = match &case.pattern {
                    ast::SwitchPattern::IntLit(v) => RustPattern::IntLiteral(*v),
                    ast::SwitchPattern::Default => RustPattern::Wildcard,
                    _ => RustPattern::Wildcard,
                };
                let body = self.lower_switch_case_body(
                    &case.body, ctx, use_map, stmt_index, reassigned, None, &[], "",
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

    /// Lower a switch with enum member access patterns (`Color.Red`).
    fn lower_enum_member_switch(
        &self,
        switch: &ast::SwitchStmt,
        scrutinee: RustExpr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        let arms: Vec<RustMatchArm> = switch
            .cases
            .iter()
            .map(|case| {
                let pattern = match &case.pattern {
                    ast::SwitchPattern::EnumMember(enum_name, variant) => {
                        RustPattern::EnumVariant(enum_name.clone(), variant.clone())
                    }
                    ast::SwitchPattern::Default => RustPattern::Wildcard,
                    _ => RustPattern::Wildcard,
                };
                let body = self.lower_switch_case_body(
                    &case.body, ctx, use_map, stmt_index, reassigned, None, &[], "",
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

    /// Lower a switch with string literal patterns (original enum-based behavior).
    #[allow(clippy::too_many_arguments)]
    fn lower_string_enum_switch(
        &self,
        switch: &ast::SwitchStmt,
        scrutinee: RustExpr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
        scrutinee_var_name: Option<String>,
    ) -> RustStmt {
        let enum_name = scrutinee_var_name
            .as_ref()
            .and_then(|name| ctx.lookup_variable(name))
            .and_then(|info| extract_named_type(&info.ty))
            .unwrap_or_else(|| {
                ctx.emit_diagnostic(Diagnostic::error(
                    "cannot infer enum type for switch expression; add a type annotation to the switch variable",
                ));
                "_UnknownEnum".to_owned()
            });

        let td = self.type_registry.lookup(&enum_name);

        let arms: Vec<RustMatchArm> = switch
            .cases
            .iter()
            .map(|case| {
                match &case.pattern {
                    ast::SwitchPattern::Default => {
                        let body = self.lower_switch_case_body(
                            &case.body,
                            ctx,
                            use_map,
                            stmt_index,
                            reassigned,
                            scrutinee_var_name.as_deref(),
                            &[],
                            &enum_name,
                        );
                        RustMatchArm {
                            pattern: RustPattern::Wildcard,
                            body,
                        }
                    }
                    ast::SwitchPattern::StringLit(pat) => {
                        let variant_name = capitalize_first(pat);

                        let (pattern, bound_fields) = match td.map(|t| &t.kind) {
                            Some(rsc_typeck::registry::TypeDefKind::DataEnum(variants)) => {
                                let field_names: Vec<String> = variants
                                    .iter()
                                    .find(|(vn, _)| *vn == variant_name)
                                    .map(|(_, fields)| {
                                        fields.iter().map(|(n, _)| n.clone()).collect()
                                    })
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
                    }
                    // Non-string patterns in a string-enum switch are unexpected;
                    // treat as wildcard to avoid panics.
                    _ => {
                        let body = self.lower_switch_case_body(
                            &case.body,
                            ctx,
                            use_map,
                            stmt_index,
                            reassigned,
                            scrutinee_var_name.as_deref(),
                            &[],
                            &enum_name,
                        );
                        RustMatchArm {
                            pattern: RustPattern::Wildcard,
                            body,
                        }
                    }
                }
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
                        rsc_typeck::registry::TypeDefKind::Struct(_)
                        | rsc_typeck::registry::TypeDefKind::Interface(_)
                        | rsc_typeck::registry::TypeDefKind::Class { .. } => false,
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
}
