//! Statement lowering.
//!
//! Transforms `RustScript` AST statements into Rust IR statements. Handles
//! blocks, variable declarations, return statements, if/else (including null
//! check narrowing), while loops, for-of loops, destructuring, try/catch, and
//! control flow (break/continue).

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{
    RustBlock, RustDestructureDefaultField, RustDestructureDefaultsStmt, RustDestructureField,
    RustDestructureStmt, RustExpr, RustExprKind, RustForInStmt, RustIfLetStmt, RustIfStmt,
    RustLetElseStmt, RustLetStmt, RustMatchResultStmt, RustReturnStmt, RustStmt,
    RustTryFinallyStmt, RustTupleDestructureStmt, RustType, RustWhileStmt,
};

use crate::context::LoweringContext;
use crate::ownership::UseMap;

use super::async_lower::block_contains_await;
use super::{
    Transform, capitalize_first, element_type_is_copy, extract_named_type, is_default_literal_type,
};

impl Transform {
    /// Lower a block of statements.
    #[allow(clippy::only_used_in_recursion)]
    pub(super) fn lower_block(
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
    pub(super) fn lower_stmt(
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
                // `throw expr;` → `return Err(expr);`
                if let ast::ExprKind::Throw(_) = &expr.kind {
                    let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
                    return RustStmt::Return(RustReturnStmt {
                        value: Some(lowered),
                        span: Some(expr.span),
                    });
                }
                // Logical assignment: `x ??= val`, `x ||= val`, `x &&= val`
                if let ast::ExprKind::LogicalAssign(la) = &expr.kind {
                    return self.lower_logical_assign(la, ctx, use_map, stmt_index);
                }
                let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
                RustStmt::Semi(lowered)
            }
            ast::Stmt::Return(ret) => self.lower_return(ret, ctx, use_map, stmt_index),
            ast::Stmt::If(if_stmt) => {
                self.lower_if_as_stmt(if_stmt, ctx, use_map, stmt_index, reassigned)
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
            ast::Stmt::TryCatch(tc) => {
                self.lower_try_catch(tc, ctx, use_map, stmt_index, reassigned)
            }
            ast::Stmt::For(for_of) => {
                RustStmt::ForIn(self.lower_for_of(for_of, ctx, use_map, stmt_index, reassigned))
            }
            ast::Stmt::ArrayDestructure(adestr) => {
                self.lower_array_destructure(adestr, ctx, use_map, stmt_index)
            }
            ast::Stmt::Break(brk) => RustStmt::Break(Some(brk.span)),
            ast::Stmt::Continue(cont) => RustStmt::Continue(Some(cont.span)),
            ast::Stmt::RustBlock(rb) => RustStmt::RawRust(rb.code.clone()),
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
        let has_explicit_annotation = decl.type_ann.is_some();
        let ty = if let Some(ann) = &decl.type_ann {
            let ty_inner = rsc_typeck::resolve::resolve_type_annotation_with_registry(
                ann,
                &self.type_registry,
                &mut diags,
            );
            rsc_typeck::bridge::type_to_rust_type(&ty_inner)
        } else {
            rsc_typeck::resolve::infer_literal_rust_type(&decl.init).unwrap_or(RustType::I64)
        };
        // Track whether the type was actually inferred from the init expression.
        // If no annotation and the init is not a literal, we should let Rust infer.
        let type_inferred_from_literal = !has_explicit_annotation
            && rsc_typeck::resolve::infer_literal_rust_type(&decl.init).is_some();

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
        let struct_type_name = extract_named_type(&ty);
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
        } else if has_explicit_annotation {
            // User wrote an explicit type annotation — always include it
            Some(ty)
        } else if is_default_literal_type(&decl.init, &ty) {
            // Type matches the literal's default — omit for cleaner output
            None
        } else if !type_inferred_from_literal {
            // Init is not a literal (e.g., a function call) — let Rust infer the type
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

    /// Lower a return statement, wrapping in `Some()` or `Ok()` as needed.
    ///
    /// - `return null;` in an `Option` function → `return None;`
    /// - `return value;` in an `Option` function → `return Some(value);`
    /// - `return value;` in a `throws` function → `return Ok(value);`
    /// - Other returns pass through unchanged.
    fn lower_return(
        &self,
        ret: &ast::ReturnStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustStmt {
        let is_option_return = ctx
            .current_return_type()
            .is_some_and(|ty| matches!(ty, RustType::Option(_)));
        let is_throws = ctx.is_fn_throws();

        // Set struct type context from return type for struct literal inference.
        // This allows `return { name: "Alice" }` to resolve the struct name from
        // the function's return type (possibly wrapped in Option or Result).
        let return_struct_name = ctx.current_return_type().and_then(extract_named_type);
        let prev_struct_name = ctx.current_struct_type_name().map(String::from);
        if let Some(ref name) = return_struct_name {
            ctx.set_struct_type_name(Some(name.clone()));
        }

        let value = ret.value.as_ref().map(|v| {
            if is_throws {
                let lowered = self.lower_expr(v, ctx, use_map, stmt_index);
                return RustExpr::synthetic(RustExprKind::Ok(Box::new(lowered)));
            }
            if is_option_return {
                // Check for `return null;`
                if matches!(v.kind, ast::ExprKind::NullLit) {
                    return RustExpr::new(RustExprKind::None, v.span);
                }
                // Non-null return in Option context → wrap in Some(...)
                let lowered = self.lower_expr(v, ctx, use_map, stmt_index);
                RustExpr::synthetic(RustExprKind::Some(Box::new(lowered)))
            } else {
                self.lower_expr(v, ctx, use_map, stmt_index)
            }
        });

        // Restore previous struct type context.
        ctx.set_struct_type_name(prev_struct_name);

        // Bare `return;` in throws context → `return Ok(());`
        // Bare `return;` in Option context → `return None;`
        let value = if value.is_none() && is_throws {
            Some(RustExpr::synthetic(RustExprKind::Ok(Box::new(
                RustExpr::synthetic(RustExprKind::Ident("()".to_owned())),
            ))))
        } else if value.is_none() && is_option_return {
            Some(RustExpr::new(RustExprKind::None, ret.span))
        } else {
            value
        };

        RustStmt::Return(RustReturnStmt {
            value,
            span: Some(ret.span),
        })
    }

    /// Lower an if statement, detecting null check narrowing patterns.
    ///
    /// When the condition is `x !== null`, lowers to `if let Some(x) = x { ... }`.
    /// When the condition is `x === null`, lowers to `if let Some(x) = x { else } else { then }`.
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
            ast::ElseClause::Block(block) => rsc_syntax::rust_ir::RustElse::Block(
                self.lower_block(block, ctx, use_map, stmt_index, reassigned),
            ),
            ast::ElseClause::ElseIf(nested_if) => rsc_syntax::rust_ir::RustElse::ElseIf(Box::new(
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

    /// Check whether a block always diverges (returns, throws, breaks, or continues).
    ///
    /// Returns true if the last statement in the block is guaranteed to prevent
    /// reaching the end of the block.
    fn block_always_diverges(block: &ast::Block) -> bool {
        block.stmts.last().is_some_and(|stmt| match stmt {
            ast::Stmt::Return(_) | ast::Stmt::Break(_) | ast::Stmt::Continue(_) => true,
            ast::Stmt::Expr(expr) => matches!(expr.kind, ast::ExprKind::Throw(_)),
            _ => false,
        })
    }

    /// Detect a null check pattern in an if condition.
    ///
    /// Returns `Some((var_name, is_not_null))` if the condition is `x !== null`
    /// or `x === null`.
    fn detect_null_check(condition: &ast::Expr) -> Option<(String, bool)> {
        if let ast::ExprKind::Binary(bin) = &condition.kind {
            let var_name = match (&bin.left.kind, &bin.right.kind) {
                (ast::ExprKind::Ident(ident), ast::ExprKind::NullLit)
                | (ast::ExprKind::NullLit, ast::ExprKind::Ident(ident)) => Some(ident.name.clone()),
                _ => None,
            };

            if let Some(var_name) = var_name {
                return match bin.op {
                    ast::BinaryOp::Ne => Some((var_name, true)), // !== null → not null
                    ast::BinaryOp::Eq => Some((var_name, false)), // === null → is null
                    _ => None,
                };
            }
        }
        None
    }

    /// Lower an if statement to an `IfLet` when a null check pattern is detected.
    fn lower_if_as_stmt(
        &self,
        if_stmt: &ast::IfStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        if let Some((var_name, is_not_null)) = Self::detect_null_check(&if_stmt.condition) {
            let expr = self.lower_expr(
                &ast::Expr {
                    kind: ast::ExprKind::Ident(ast::Ident {
                        name: var_name.clone(),
                        span: if_stmt.condition.span,
                    }),
                    span: if_stmt.condition.span,
                },
                ctx,
                use_map,
                stmt_index,
            );

            if is_not_null {
                // `if (x !== null)` → `if let Some(x) = x { then } else { else }`
                let then_block =
                    self.lower_block(&if_stmt.then_block, ctx, use_map, stmt_index, reassigned);
                let else_block = if_stmt.else_clause.as_ref().map(|ec| match ec {
                    ast::ElseClause::Block(block) => {
                        self.lower_block(block, ctx, use_map, stmt_index, reassigned)
                    }
                    ast::ElseClause::ElseIf(_) => {
                        // For else-if chains after null check, fall back to normal if lowering
                        // within an else block
                        RustBlock {
                            stmts: vec![],
                            expr: None,
                        }
                    }
                });

                return RustStmt::IfLet(RustIfLetStmt {
                    binding: var_name,
                    expr,
                    then_block,
                    else_block,
                    span: Some(if_stmt.span),
                });
            }
            // `if (x === null) { throw/return; }` with no else → `let Some(x) = x else { ... };`
            // This narrows x to non-null in the continuation scope.
            if if_stmt.else_clause.is_none() && Self::block_always_diverges(&if_stmt.then_block) {
                let else_block =
                    self.lower_block(&if_stmt.then_block, ctx, use_map, stmt_index, reassigned);
                return RustStmt::LetElse(RustLetElseStmt {
                    binding: var_name,
                    expr,
                    else_block,
                    span: Some(if_stmt.span),
                });
            }

            // `if (x === null)` → `if let Some(x) = x { else_block } else { then_block }`
            // We swap the branches: the then block is the "is None" case
            let then_of_some = if_stmt.else_clause.as_ref().map(|ec| match ec {
                ast::ElseClause::Block(block) => {
                    self.lower_block(block, ctx, use_map, stmt_index, reassigned)
                }
                ast::ElseClause::ElseIf(_) => RustBlock {
                    stmts: vec![],
                    expr: None,
                },
            });
            let else_of_some =
                Some(self.lower_block(&if_stmt.then_block, ctx, use_map, stmt_index, reassigned));

            return RustStmt::IfLet(RustIfLetStmt {
                binding: var_name,
                expr,
                then_block: then_of_some.unwrap_or(RustBlock {
                    stmts: vec![],
                    expr: None,
                }),
                else_block: else_of_some,
                span: Some(if_stmt.span),
            });
        }

        // Not a null check — lower as normal if
        RustStmt::If(self.lower_if(if_stmt, ctx, use_map, stmt_index, reassigned))
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

    /// Lower a for-of statement to a Rust for-in loop.
    ///
    /// `for (const x of items) { body }` → `for x in &items { body }`.
    /// The iterable is always borrowed (`&items`) in Tier 1.
    fn lower_for_of(
        &self,
        for_of: &ast::ForOfStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustForInStmt {
        let iterable = self.lower_expr(&for_of.iterable, ctx, use_map, stmt_index);

        // Determine if the element type is Copy to use a deref pattern.
        // For `for n in &items` where items: Vec<i32>, we emit `for &n in &items`
        // so that n has type i32 instead of &i32.
        let deref_pattern = if let ast::ExprKind::Ident(ident) = &for_of.iterable.kind {
            ctx.lookup_variable(&ident.name)
                .is_some_and(|info| element_type_is_copy(&info.ty))
        } else {
            false
        };

        let body = self.lower_block(&for_of.body, ctx, use_map, stmt_index, reassigned);

        RustForInStmt {
            variable: for_of.variable.name.clone(),
            iterable,
            body,
            deref_pattern,
            span: Some(for_of.span),
        }
    }

    /// Lower a destructuring statement.
    ///
    /// When no defaults are present, produces a `RustDestructureStmt` with optional
    /// field renames. When defaults are present, produces a `RustDestructureDefaultsStmt`
    /// with individual field access expressions and `unwrap_or_else` for Option fields.
    fn lower_destructure(
        &self,
        destr: &ast::DestructureStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustStmt {
        let init = self.lower_expr(&destr.init, ctx, use_map, stmt_index);

        // Check if any field has a default value — that requires individual statements.
        let has_defaults = destr.fields.iter().any(|f| f.default_value.is_some());

        // Infer the type name from the init expression.
        let type_name = match &destr.init.kind {
            ast::ExprKind::Ident(ident) => ctx
                .lookup_variable(&ident.name)
                .and_then(|info| extract_named_type(&info.ty)),
            _ => None,
        };
        let Some(type_name) = type_name else {
            ctx.emit_diagnostic(Diagnostic::error("cannot infer type for destructuring"));
            return RustStmt::Semi(init);
        };

        // Declare the extracted fields as variables in the current scope.
        // Look up their types from the type registry.
        if let Some(td) = self.type_registry.lookup(&type_name)
            && let Some(struct_fields) = td.struct_fields()
        {
            for field in &destr.fields {
                let binding_name = field
                    .local_name
                    .as_ref()
                    .map_or(&field.field_name.name, |l| &l.name);
                let field_ty = struct_fields
                    .iter()
                    .find(|(name, _)| name == &field.field_name.name)
                    .map_or(RustType::Unit, |(_, ty)| {
                        rsc_typeck::bridge::type_to_rust_type(ty)
                    });
                ctx.declare_variable(binding_name.clone(), field_ty);
            }
        }

        if has_defaults {
            // Emit individual `let` statements with field access and defaults.
            let init_name = match &destr.init.kind {
                ast::ExprKind::Ident(ident) => ident.name.clone(),
                _ => "source".to_owned(),
            };

            let fields = destr
                .fields
                .iter()
                .map(|f| {
                    let local_name = f
                        .local_name
                        .as_ref()
                        .map_or(f.field_name.name.clone(), |l| l.name.clone());
                    let access_expr = RustExpr {
                        kind: RustExprKind::FieldAccess {
                            object: Box::new(RustExpr {
                                kind: RustExprKind::Ident(init_name.clone()),
                                span: None,
                            }),
                            field: f.field_name.name.clone(),
                        },
                        span: None,
                    };
                    let default_value = f
                        .default_value
                        .as_ref()
                        .map(|dv| self.lower_expr(dv, ctx, use_map, stmt_index));
                    RustDestructureDefaultField {
                        local_name,
                        access_expr,
                        default_value,
                        mutable: destr.binding == ast::VarBinding::Let,
                    }
                })
                .collect();

            RustStmt::DestructureDefaults(RustDestructureDefaultsStmt {
                fields,
                span: Some(destr.span),
            })
        } else {
            // Simple struct pattern destructuring, with optional renames.
            let fields = destr
                .fields
                .iter()
                .map(|f| RustDestructureField {
                    field_name: f.field_name.name.clone(),
                    local_name: f.local_name.as_ref().map(|l| l.name.clone()),
                })
                .collect();

            RustStmt::Destructure(RustDestructureStmt {
                type_name,
                fields,
                init,
                mutable: destr.binding == ast::VarBinding::Let,
                span: Some(destr.span),
            })
        }
    }

    /// Lower an array destructuring statement.
    ///
    /// `const [a, b] = expr;` → `let (a, b) = expr;`
    /// `const [first, ...rest] = arr;` → indexed access with rest slice.
    /// Typically used with `Promise.all` results.
    fn lower_array_destructure(
        &self,
        adestr: &ast::ArrayDestructureStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustStmt {
        let init = self.lower_expr(&adestr.init, ctx, use_map, stmt_index);

        // Check if any element is a rest element.
        let has_rest = adestr
            .elements
            .iter()
            .any(|e| matches!(e, ast::ArrayDestructureElement::Rest(_)));

        if has_rest {
            // Emit individual indexed access statements with rest slice.
            // Uses `arr[N].clone()` for single elements and `arr[N..].to_vec()` for rest.
            // Since the IR doesn't have range expressions, emit as raw Rust.
            let init_name = match &adestr.init.kind {
                ast::ExprKind::Ident(ident) => ident.name.clone(),
                _ => "source".to_owned(),
            };

            let mutable = adestr.binding == ast::VarBinding::Let;
            let let_kw = if mutable { "let mut" } else { "let" };
            let mut raw_lines = Vec::new();
            for (i, elem) in adestr.elements.iter().enumerate() {
                match elem {
                    ast::ArrayDestructureElement::Single(ident) => {
                        ctx.declare_variable(ident.name.clone(), RustType::Unit);
                        raw_lines.push(format!(
                            "{let_kw} {} = {init_name}[{i}].clone();",
                            ident.name
                        ));
                    }
                    ast::ArrayDestructureElement::Rest(ident) => {
                        ctx.declare_variable(ident.name.clone(), RustType::Unit);
                        raw_lines.push(format!(
                            "{let_kw} {} = {init_name}[{i}..].to_vec();",
                            ident.name
                        ));
                    }
                }
            }

            RustStmt::RawRust(raw_lines.join("\n"))
        } else {
            // Simple tuple destructuring (no rest element).
            let bindings = adestr
                .elements
                .iter()
                .map(|e| match e {
                    ast::ArrayDestructureElement::Single(ident)
                    | ast::ArrayDestructureElement::Rest(ident) => ident.name.clone(),
                })
                .collect();

            // Declare the extracted variables in the current scope with inferred types.
            for elem in &adestr.elements {
                let name = match elem {
                    ast::ArrayDestructureElement::Single(ident)
                    | ast::ArrayDestructureElement::Rest(ident) => &ident.name,
                };
                ctx.declare_variable(name.clone(), RustType::Unit);
            }

            RustStmt::TupleDestructure(RustTupleDestructureStmt {
                bindings,
                init,
                mutable: adestr.binding == ast::VarBinding::Let,
                span: Some(adestr.span),
            })
        }
    }

    /// Lower a `try/catch` or `try/catch/finally` or `try/finally` statement.
    ///
    /// For `try/finally` (no catch), emits the try body followed by finally in a block.
    /// For `try/catch[/finally]`, lowers to a `match` on `Result` with optional finally cleanup.
    fn lower_try_catch(
        &self,
        tc: &ast::TryCatchStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        // Lower finally statements if present
        let finally_stmts = tc.finally_block.as_ref().map_or_else(Vec::new, |block| {
            self.lower_block(block, ctx, use_map, stmt_index, reassigned)
                .stmts
        });

        // Handle try {} finally {} (no catch)
        if tc.catch_block.is_none() {
            let try_body = self.lower_block(&tc.try_block, ctx, use_map, stmt_index, reassigned);
            return RustStmt::TryFinally(RustTryFinallyStmt {
                try_block: try_body,
                finally_stmts,
                span: Some(tc.span),
            });
        }

        // Detect single-statement try block with a var decl calling a throws function.
        // This enables the simpler direct match pattern.
        if let Some(simple_match) = self.try_lower_simple_try_catch(
            tc,
            ctx,
            use_map,
            stmt_index,
            reassigned,
            &finally_stmts,
        ) {
            return simple_match;
        }

        // General case: immediately-invoked closure
        // match (|| -> Result<(), E> { body; Ok(()) })() { Ok(_) => {}, Err(e) => { catch } }
        // If the try block contains await expressions, use an async closure:
        // match (async || -> Result<(), E> { body; Ok(()) })().await { ... }
        let needs_async = block_contains_await(&tc.try_block);
        let try_body = self.lower_block(&tc.try_block, ctx, use_map, stmt_index, reassigned);
        let catch_block = tc.catch_block.as_ref().expect("catch block checked above");
        let catch_body = self.lower_block(catch_block, ctx, use_map, stmt_index, reassigned);

        // Determine the error type from the catch annotation or default to String
        let err_ty = tc.catch_type.as_ref().map_or(RustType::String, |ann| {
            let mut diags = Vec::new();
            let ty = rsc_typeck::resolve::resolve_type_annotation_with_registry(
                ann,
                &self.type_registry,
                &mut diags,
            );
            for d in diags {
                ctx.emit_diagnostic(d);
            }
            rsc_typeck::bridge::type_to_rust_type(&ty)
        });

        // Build: match (|| -> Result<(), ErrType> { <try_body>; Ok(()) })()
        // For the try body, we need to wrap it in a closure that returns Ok(())
        let mut closure_stmts = try_body.stmts;
        closure_stmts.push(RustStmt::Expr(RustExpr::synthetic(RustExprKind::Ok(
            Box::new(RustExpr::synthetic(RustExprKind::Ident("()".to_owned()))),
        ))));

        // The closure call expression will be emitted by the MatchResult handler
        let closure_body = RustBlock {
            stmts: closure_stmts,
            expr: None,
        };

        let catch_binding_name = tc
            .catch_binding
            .as_ref()
            .map_or_else(|| "_".to_owned(), |b| b.name.clone());

        RustStmt::MatchResult(RustMatchResultStmt {
            expr: RustExpr::synthetic(RustExprKind::ClosureCall {
                is_async: needs_async,
                body: closure_body,
                return_type: RustType::Result(Box::new(RustType::Unit), Box::new(err_ty)),
            }),
            ok_binding: "_".to_owned(),
            ok_block: RustBlock {
                stmts: vec![],
                expr: None,
            },
            err_binding: catch_binding_name,
            err_block: catch_body,
            finally_stmts,
            span: Some(tc.span),
        })
    }

    /// Try to lower a try/catch as a simple direct match when the try block
    /// has a single var decl calling a throws function followed by uses of that binding.
    fn try_lower_simple_try_catch(
        &self,
        tc: &ast::TryCatchStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
        finally_stmts: &[RustStmt],
    ) -> Option<RustStmt> {
        // Check if the first statement is a var decl with a call to a throws function
        if tc.try_block.stmts.is_empty() {
            return None;
        }

        let first = &tc.try_block.stmts[0];
        let (binding_name, call_expr) = match first {
            ast::Stmt::VarDecl(decl) => {
                if let ast::ExprKind::Call(call) = &decl.init.kind {
                    let callee_throws = self
                        .fn_signatures
                        .get(&call.callee.name)
                        .is_some_and(|sig| sig.throws);
                    if callee_throws {
                        Some((decl.name.name.clone(), &decl.init))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }?;

        // Lower the call expression WITHOUT the ? operator
        let lowered_call = match &call_expr.kind {
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
                    call_expr.span,
                )
            }
            _ => return None,
        };

        // Build the Ok arm body: the remaining statements after the var decl
        let mut ok_stmts: Vec<RustStmt> = Vec::new();
        for s in tc.try_block.stmts.iter().skip(1) {
            ok_stmts.push(self.lower_stmt(s, ctx, use_map, stmt_index, reassigned));
        }

        let ok_block = RustBlock {
            stmts: ok_stmts,
            expr: None,
        };

        let catch_block = tc.catch_block.as_ref()?;
        let catch_body = self.lower_block(catch_block, ctx, use_map, stmt_index, reassigned);

        let catch_binding_name = tc
            .catch_binding
            .as_ref()
            .map_or_else(|| "_".to_owned(), |b| b.name.clone());

        Some(RustStmt::MatchResult(RustMatchResultStmt {
            expr: lowered_call,
            ok_binding: binding_name,
            ok_block,
            err_binding: catch_binding_name,
            err_block: catch_body,
            finally_stmts: finally_stmts.to_vec(),
            span: Some(tc.span),
        }))
    }

    /// Lower a logical assignment expression to an if-statement.
    ///
    /// - `x ??= val` → `if x.is_none() { x = Some(val); }`
    /// - `x ||= val` → `if !x { x = val; }`
    /// - `x &&= val` → `if x { x = val; }`
    fn lower_logical_assign(
        &self,
        la: &ast::LogicalAssignExpr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustStmt {
        let target_name = la.target.name.clone();
        let lowered_value = self.lower_expr(&la.value, ctx, use_map, stmt_index);

        match la.op {
            ast::LogicalAssignOp::NullishAssign => {
                // `x ??= val` → `if x.is_none() { x = Some(val); }`
                let condition = RustExpr::synthetic(RustExprKind::MethodCall {
                    receiver: Box::new(RustExpr::synthetic(RustExprKind::Ident(
                        target_name.clone(),
                    ))),
                    method: "is_none".to_owned(),
                    type_args: vec![],
                    args: vec![],
                });
                let assign = RustStmt::Semi(RustExpr::synthetic(RustExprKind::Assign {
                    target: target_name,
                    value: Box::new(RustExpr::synthetic(RustExprKind::Some(Box::new(
                        lowered_value,
                    )))),
                }));
                RustStmt::If(RustIfStmt {
                    condition,
                    then_block: RustBlock {
                        stmts: vec![assign],
                        expr: None,
                    },
                    else_clause: None,
                    span: Some(la.target.span),
                })
            }
            ast::LogicalAssignOp::OrAssign => {
                // `x ||= val` → `if !x { x = val; }`
                let condition = RustExpr::synthetic(RustExprKind::Unary {
                    op: rsc_syntax::rust_ir::RustUnaryOp::Not,
                    operand: Box::new(RustExpr::synthetic(RustExprKind::Ident(
                        target_name.clone(),
                    ))),
                });
                let assign = RustStmt::Semi(RustExpr::synthetic(RustExprKind::Assign {
                    target: target_name,
                    value: Box::new(lowered_value),
                }));
                RustStmt::If(RustIfStmt {
                    condition,
                    then_block: RustBlock {
                        stmts: vec![assign],
                        expr: None,
                    },
                    else_clause: None,
                    span: Some(la.target.span),
                })
            }
            ast::LogicalAssignOp::AndAssign => {
                // `x &&= val` → `if x { x = val; }`
                let condition = RustExpr::synthetic(RustExprKind::Ident(target_name.clone()));
                let assign = RustStmt::Semi(RustExpr::synthetic(RustExprKind::Assign {
                    target: target_name,
                    value: Box::new(lowered_value),
                }));
                RustStmt::If(RustIfStmt {
                    condition,
                    then_block: RustBlock {
                        stmts: vec![assign],
                        expr: None,
                    },
                    else_clause: None,
                    span: Some(la.target.span),
                })
            }
        }
    }
}
