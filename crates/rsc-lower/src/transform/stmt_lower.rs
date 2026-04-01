//! Statement lowering.
//!
//! Transforms `RustScript` AST statements into Rust IR statements. Handles
//! blocks, variable declarations, return statements, if/else (including null
//! check narrowing), while loops, for-of loops, destructuring, try/catch, and
//! control flow (break/continue).

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{
    IteratorTerminal, RustBlock, RustCompoundAssignOp, RustDestructureDefaultField,
    RustDestructureDefaultsStmt, RustDestructureField, RustDestructureStmt, RustExpr, RustExprKind,
    RustForInStmt, RustIfLetStmt, RustIfStmt, RustLetElseStmt, RustLetStmt, RustLoopStmt,
    RustMatchResultStmt, RustReturnStmt, RustStmt, RustTryFinallyStmt, RustTupleDestructureStmt,
    RustType, RustUnaryOp, RustWhileLetStmt, RustWhileStmt,
};

use crate::context::LoweringContext;
use crate::ownership::UseMap;

use super::async_lower::block_contains_await;
use super::expr_lower::is_hashmap_type;
use super::{
    Transform, capitalize_first, element_type_is_copy, extract_named_type, is_default_literal_type,
};

/// Check whether a lowered expression already returns `Option<T>`.
///
/// Detects expressions that already produce `Option<T>`, preventing
/// double-wrapping in `Some()`. Covers:
/// - `IteratorChain` with `Find` terminal (`.find().cloned()`)
/// - Variables whose type in context is `Option<T>` (e.g., result of `.find()`)
fn returns_option(expr: &RustExpr, ctx: &LoweringContext) -> bool {
    match &expr.kind {
        RustExprKind::IteratorChain {
            terminal: IteratorTerminal::Find(..),
            ..
        } => true,
        RustExprKind::Ident(name) => ctx
            .lookup_variable(name)
            .is_some_and(|info| matches!(info.ty, RustType::Option(_))),
        _ => false,
    }
}

/// If the expression is an identifier that is a reference variable
/// (e.g., a for-of loop variable), wrap it in `.clone()` to convert
/// from `&T` to `T`. Returns the expression unchanged otherwise.
fn clone_if_reference(expr: &RustExpr, ctx: &LoweringContext) -> RustExpr {
    if let RustExprKind::Ident(name) = &expr.kind
        && ctx.is_reference_variable(name)
    {
        return RustExpr::synthetic(RustExprKind::Clone(Box::new(expr.clone())));
    }
    expr.clone()
}

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
        let mut stmts = Vec::new();
        for (i, stmt) in block.stmts.iter().enumerate() {
            if let ast::Stmt::ForClassic(fc) = stmt {
                stmts.extend(self.lower_for_classic(
                    fc,
                    ctx,
                    use_map,
                    current_base + i,
                    reassigned,
                ));
            } else {
                stmts.push(self.lower_stmt(stmt, ctx, use_map, current_base + i, reassigned));
            }
        }

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
            ast::Stmt::DoWhile(dw) => {
                RustStmt::Loop(self.lower_do_while(dw, ctx, use_map, stmt_index, reassigned))
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
                if for_of.is_await {
                    RustStmt::WhileLet(
                        self.lower_for_await(for_of, ctx, use_map, stmt_index, reassigned),
                    )
                } else {
                    RustStmt::ForIn(self.lower_for_of(for_of, ctx, use_map, stmt_index, reassigned))
                }
            }
            ast::Stmt::ForIn(for_in) => {
                RustStmt::ForIn(self.lower_for_in(for_in, ctx, use_map, stmt_index, reassigned))
            }
            ast::Stmt::ArrayDestructure(adestr) => {
                self.lower_array_destructure(adestr, ctx, use_map, stmt_index)
            }
            ast::Stmt::Break(brk) => RustStmt::Break {
                label: brk.label.clone(),
                span: Some(brk.span),
            },
            ast::Stmt::Continue(cont) => RustStmt::Continue {
                label: cont.label.clone(),
                span: Some(cont.span),
            },
            ast::Stmt::RustBlock(rb) => RustStmt::RawRust(rb.code.clone()),
            // `using`/`await using` → normal `let` binding (Rust RAII handles Drop)
            ast::Stmt::Using(decl) => {
                let equiv = ast::VarDecl {
                    binding: ast::VarBinding::Const,
                    name: decl.name.clone(),
                    type_ann: decl.type_ann.clone(),
                    init: decl.init.clone(),
                    span: decl.span,
                };
                self.lower_var_decl(&equiv, ctx, use_map, stmt_index, reassigned)
            }
            // ForClassic is normally handled by lower_block expansion;
            // if reached here, lower as general while pattern.
            ast::Stmt::ForClassic(fc) => {
                let stmts = self.lower_for_classic(fc, ctx, use_map, stmt_index, reassigned);
                // Wrap in a while if there's only one, or take the last
                // (This fallback shouldn't normally be reached)
                if stmts.len() == 1 {
                    stmts.into_iter().next().unwrap_or(RustStmt::Semi(RustExpr {
                        kind: RustExprKind::Ident("()".to_string()),
                        span: None,
                    }))
                } else {
                    // Return the while/loop/for statement (last one)
                    stmts.into_iter().last().unwrap_or(RustStmt::Semi(RustExpr {
                        kind: RustExprKind::Ident("()".to_string()),
                        span: None,
                    }))
                }
            }
        }
    }

    /// Lower a variable declaration.
    #[allow(clippy::too_many_lines)]
    // Variable declaration lowering handles HashMap init, tuple construction, enum construction,
    // union wrapping, type inference, and mutability — splitting would fragment the logic.
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

        // Check for HashMap initialization: `const config: { [key: string]: string } = {}`
        // When the type is a HashMap and the init is an empty struct literal, emit `HashMap::new()`.
        let init = if is_hashmap_type(&ty)
            && matches!(&decl.init.kind, ast::ExprKind::StructLit(slit) if slit.fields.is_empty() && slit.spread.is_none())
        {
            RustExpr::new(
                RustExprKind::StaticCall {
                    type_name: "HashMap".to_owned(),
                    method: "new".to_owned(),
                    args: vec![],
                },
                decl.init.span,
            )
        }
        // Check for tuple construction: `const pair: [string, i32] = ["hello", 42]`
        // When the type is a tuple and the init is an array literal, lower as a tuple expression.
        else if let (RustType::Tuple(_), ast::ExprKind::ArrayLit(elements)) =
            (&ty, &decl.init.kind)
        {
            let lowered: Vec<RustExpr> = elements
                .iter()
                .map(|e| match e {
                    ast::ArrayElement::Expr(expr) | ast::ArrayElement::Spread(expr) => {
                        self.lower_expr(expr, ctx, use_map, stmt_index)
                    }
                })
                .collect();
            RustExpr::new(RustExprKind::Tuple(lowered), decl.init.span)
        }
        // Check for enum construction: `const dir: Direction = "north"` → `Direction::North`
        else if let (RustType::Named(type_name), ast::ExprKind::StringLit(s)) =
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

        // If the init expression is an iterator .find() chain, the result type is
        // Option<T>. Override the variable's type so that returning this variable
        // from an Option-returning function doesn't double-wrap in Some().
        let ty = if returns_option(&init, ctx) {
            let opt_ty = RustType::Option(Box::new(ty));
            // Re-declare the variable with the corrected Option type
            ctx.declare_variable(decl.name.name.clone(), opt_ty.clone());
            opt_ty
        } else {
            ty
        };

        // When the target type is a generated union, wrap the init with `.into()`
        // so the From impl converts the concrete type to the union enum.
        let init = if matches!(&ty, RustType::GeneratedUnion { .. }) {
            RustExpr::synthetic(RustExprKind::MethodCall {
                receiver: Box::new(init),
                method: "into".to_owned(),
                type_args: vec![],
                args: vec![],
            })
        } else {
            init
        };

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
        } else if is_default_literal_type(&decl.init, &ty)
            && !matches!(ty, RustType::F32 | RustType::F64)
        {
            // Type matches the literal's default — omit for cleaner output.
            // Exception: float types must keep the annotation because Rust's
            // `{float}` is ambiguous and can't call methods like .floor().
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

        let is_tuple_return = ctx
            .current_return_type()
            .is_some_and(|ty| matches!(ty, RustType::Tuple(_)));

        let is_union_return = ctx
            .current_return_type()
            .is_some_and(|ty| matches!(ty, RustType::GeneratedUnion { .. }));

        let value = ret.value.as_ref().map(|v| {
            if is_throws {
                let lowered = self.lower_return_value(v, ctx, use_map, stmt_index, is_tuple_return);
                return RustExpr::synthetic(RustExprKind::Ok(Box::new(lowered)));
            }
            if is_option_return {
                // Check for `return null;`
                if matches!(v.kind, ast::ExprKind::NullLit) {
                    return RustExpr::new(RustExprKind::None, v.span);
                }
                // Non-null return in Option context → wrap in Some(...)
                let lowered = self.lower_return_value(v, ctx, use_map, stmt_index, is_tuple_return);

                // Bug 3: If the lowered expression is an IteratorChain with a
                // Find terminal, it already returns Option<T> (via .find().cloned()).
                // Don't double-wrap in Some().
                if returns_option(&lowered, ctx) {
                    return lowered;
                }

                // Bug 2: If the return value is a reference variable (e.g.,
                // a for-of loop variable), clone it before wrapping in Some().
                let lowered = clone_if_reference(&lowered, ctx);
                RustExpr::synthetic(RustExprKind::Some(Box::new(lowered)))
            } else if is_union_return {
                let lowered = self.lower_return_value(v, ctx, use_map, stmt_index, is_tuple_return);
                RustExpr::synthetic(RustExprKind::MethodCall {
                    receiver: Box::new(lowered),
                    method: "into".to_owned(),
                    type_args: vec![],
                    args: vec![],
                })
            } else {
                self.lower_return_value(v, ctx, use_map, stmt_index, is_tuple_return)
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
            label: while_stmt.label.clone(),
            condition,
            body,
            span: Some(while_stmt.span),
        }
    }

    /// Lower a do-while statement to a Rust `loop` with a trailing break.
    ///
    /// `do { body } while (cond)` → `loop { body; if !cond { break; } }`.
    fn lower_do_while(
        &self,
        dw: &ast::DoWhileStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustLoopStmt {
        let mut body = self.lower_block(&dw.body, ctx, use_map, stmt_index, reassigned);
        let condition = self.lower_expr(&dw.condition, ctx, use_map, stmt_index);

        // Append `if !(condition) { break; }` to the end of the body.
        // Wrap the condition in parentheses to ensure correct precedence
        // when the `!` operator is applied (e.g., `!(x < 10)` not `!x < 10`).
        let parens = RustExpr {
            kind: RustExprKind::Paren(Box::new(condition)),
            span: None,
        };
        let negated_condition = RustExpr {
            kind: RustExprKind::Unary {
                op: RustUnaryOp::Not,
                operand: Box::new(parens),
            },
            span: None,
        };

        let break_if = RustStmt::If(RustIfStmt {
            condition: negated_condition,
            then_block: RustBlock {
                stmts: vec![RustStmt::Break {
                    label: None,
                    span: None,
                }],
                expr: None,
            },
            else_clause: None,
            span: None,
        });

        body.stmts.push(break_if);

        RustLoopStmt {
            label: dw.label.clone(),
            body,
            span: Some(dw.span),
        }
    }

    /// Lower a classic C-style for loop.
    ///
    /// Returns a `Vec<RustStmt>` because the general case emits an init
    /// statement followed by a while loop.
    ///
    /// **Range optimization:** `for (let i = START; i < END; i++)` →
    ///   `for i in START..END { body }` (single `RustStmt::ForIn`).
    ///
    /// **General case:** `for (let i = INIT; COND; UPDATE) { body }` →
    ///   `let mut i = INIT; while COND { body; UPDATE; }`.
    ///
    /// **Infinite loop:** `for (;;) { body }` → `loop { body }`.
    fn lower_for_classic(
        &self,
        fc: &ast::ForClassicStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> Vec<RustStmt> {
        // Check for infinite loop: `for (;;) { body }`
        if fc.init.is_none() && fc.condition.is_none() && fc.update.is_none() {
            let body = self.lower_block(&fc.body, ctx, use_map, stmt_index, reassigned);
            return vec![RustStmt::Loop(RustLoopStmt {
                label: fc.label.clone(),
                body,
                span: Some(fc.span),
            })];
        }

        // Try range optimization: `for (let i = START; i < END; i++)` or `i++` or `++i`
        if let Some(range_stmt) =
            self.try_range_optimization(fc, ctx, use_map, stmt_index, reassigned)
        {
            return vec![range_stmt];
        }

        // General case: emit init + while loop
        let mut result = Vec::new();

        // Emit init statement
        if let Some(init) = &fc.init {
            match init {
                ast::ForInit::VarDecl(decl) => {
                    result.push(RustStmt::Let(RustLetStmt {
                        mutable: true,
                        name: decl.name.name.clone(),
                        ty: None,
                        init: self.lower_expr(&decl.init, ctx, use_map, stmt_index),
                        span: Some(decl.span),
                    }));
                }
                ast::ForInit::Expr(expr) => {
                    let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
                    result.push(RustStmt::Semi(lowered));
                }
            }
        }

        // Build while body: original body + update
        let mut body = self.lower_block(&fc.body, ctx, use_map, stmt_index, reassigned);
        if let Some(update) = &fc.update {
            let update_stmt = self.lower_for_update(update, ctx, use_map, stmt_index);
            body.stmts.push(update_stmt);
        }

        // Build while condition (or loop if no condition)
        if let Some(condition) = &fc.condition {
            let cond = self.lower_expr(condition, ctx, use_map, stmt_index);
            result.push(RustStmt::While(RustWhileStmt {
                label: fc.label.clone(),
                condition: cond,
                body,
                span: Some(fc.span),
            }));
        } else {
            result.push(RustStmt::Loop(RustLoopStmt {
                label: fc.label.clone(),
                body,
                span: Some(fc.span),
            }));
        }

        result
    }

    /// Try to optimize a classic for loop to a Rust `for i in START..END` range loop.
    ///
    /// Succeeds when the pattern is:
    /// - Init: `let i = START`
    /// - Condition: `i < END`
    /// - Update: `i++` or `++i` or `i += 1`
    fn try_range_optimization(
        &self,
        fc: &ast::ForClassicStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> Option<RustStmt> {
        // Must have init, condition, and update
        let init = fc.init.as_ref()?;
        let condition = fc.condition.as_ref()?;
        let update = fc.update.as_ref()?;

        // Init must be a var decl: `let i = START`
        let decl = match init {
            ast::ForInit::VarDecl(d) => d,
            ast::ForInit::Expr(_) => return None,
        };
        let var_name = &decl.name.name;

        // Condition must be `i < END`
        let end_expr = match &condition.kind {
            ast::ExprKind::Binary(bin) if bin.op == ast::BinaryOp::Lt => {
                if let ast::ExprKind::Ident(ident) = &bin.left.kind {
                    if ident.name == *var_name {
                        Some(&bin.right)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }?;

        // Update must be `i++`, `++i`, or `i += 1`
        let is_simple_increment = match &update.kind {
            ast::ExprKind::PostfixIncrement(operand) | ast::ExprKind::PrefixIncrement(operand) => {
                matches!(&operand.kind, ast::ExprKind::Ident(ident) if ident.name == *var_name)
            }
            ast::ExprKind::Assign(assign) => {
                // Check for `i = i + 1` (which is what `i += 1` desugars to in the parser)
                if assign.target.name != *var_name {
                    return None;
                }
                if let ast::ExprKind::Binary(bin) = &assign.value.kind {
                    if bin.op == ast::BinaryOp::Add {
                        if let ast::ExprKind::Ident(left_ident) = &bin.left.kind {
                            if left_ident.name == *var_name {
                                matches!(&bin.right.kind, ast::ExprKind::IntLit(1))
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };

        if !is_simple_increment {
            return None;
        }

        // Emit `for i in START..END { body }`
        let start = self.lower_expr(&decl.init, ctx, use_map, stmt_index);
        let end = self.lower_expr(end_expr, ctx, use_map, stmt_index);

        let range = RustExpr {
            kind: RustExprKind::Range {
                start: Box::new(start),
                end: Box::new(end),
            },
            span: None,
        };

        let body = self.lower_block(&fc.body, ctx, use_map, stmt_index, reassigned);

        Some(RustStmt::ForIn(RustForInStmt {
            label: fc.label.clone(),
            variable: var_name.clone(),
            iterable: range,
            body,
            deref_pattern: false,
            iterable_is_borrowed: true,
            span: Some(fc.span),
        }))
    }

    /// Lower a for-loop update expression to a `RustStmt`.
    ///
    /// Handles `i++`, `i--`, `++i`, `--i`, and general expressions.
    fn lower_for_update(
        &self,
        update: &ast::Expr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustStmt {
        match &update.kind {
            ast::ExprKind::PostfixIncrement(operand) | ast::ExprKind::PrefixIncrement(operand) => {
                if let ast::ExprKind::Ident(ident) = &operand.kind {
                    RustStmt::Semi(RustExpr {
                        kind: RustExprKind::CompoundAssign {
                            target: ident.name.clone(),
                            op: RustCompoundAssignOp::AddAssign,
                            value: Box::new(RustExpr {
                                kind: RustExprKind::IntLit(1),
                                span: None,
                            }),
                        },
                        span: None,
                    })
                } else {
                    let lowered = self.lower_expr(update, ctx, use_map, stmt_index);
                    RustStmt::Semi(lowered)
                }
            }
            ast::ExprKind::PostfixDecrement(operand) | ast::ExprKind::PrefixDecrement(operand) => {
                if let ast::ExprKind::Ident(ident) = &operand.kind {
                    RustStmt::Semi(RustExpr {
                        kind: RustExprKind::CompoundAssign {
                            target: ident.name.clone(),
                            op: RustCompoundAssignOp::SubAssign,
                            value: Box::new(RustExpr {
                                kind: RustExprKind::IntLit(1),
                                span: None,
                            }),
                        },
                        span: None,
                    })
                } else {
                    let lowered = self.lower_expr(update, ctx, use_map, stmt_index);
                    RustStmt::Semi(lowered)
                }
            }
            _ => {
                let lowered = self.lower_expr(update, ctx, use_map, stmt_index);
                RustStmt::Semi(lowered)
            }
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

        // Check if the iterable is already a reference (e.g., a borrowed parameter)
        // or a generator call (iterator owns its values, no `&` needed).
        // If so, the emitter must skip adding `&` to avoid `&&Vec<T>`.
        let is_generator_call = if let ast::ExprKind::Call(ref call) = for_of.iterable.kind {
            self.generator_structs.contains_key(&call.callee.name)
        } else {
            false
        };
        let iterable_is_borrowed = is_generator_call
            || if let ast::ExprKind::Ident(ref ident) = for_of.iterable.kind {
                ctx.is_reference_variable(&ident.name)
            } else {
                false
            };

        // Determine if the element type is Copy to use a deref pattern.
        // For `for n in &items` where items: Vec<i32>, we emit `for &n in &items`
        // so that n has type i32 instead of &i32.
        let deref_pattern = if let ast::ExprKind::Ident(ident) = &for_of.iterable.kind {
            ctx.lookup_variable(&ident.name)
                .is_some_and(|info| element_type_is_copy(&info.ty))
        } else {
            false
        };

        // Mark the loop variable as a reference when iterating non-Copy types.
        // In `for x in &items`, x is `&T`. Any use that needs ownership (return,
        // passing to a function with owned param) should auto-clone.
        let var_name = for_of.variable.name.clone();
        if !deref_pattern {
            ctx.mark_as_reference(var_name.clone());
        }

        let body = self.lower_block(&for_of.body, ctx, use_map, stmt_index, reassigned);

        // Unmark the loop variable after leaving the for-of body.
        if !deref_pattern {
            ctx.unmark_reference(&var_name);
        }

        RustForInStmt {
            label: for_of.label.clone(),
            variable: for_of.variable.name.clone(),
            iterable,
            body,
            deref_pattern,
            iterable_is_borrowed,
            span: Some(for_of.span),
        }
    }

    /// Lower a for-in statement to a Rust for loop over keys.
    ///
    /// `for (const key in map) { body }` → `for key in map.keys() { body }`.
    /// The iterable is wrapped in a `.keys()` method call.
    fn lower_for_in(
        &self,
        for_in: &ast::ForInStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustForInStmt {
        let iterable = self.lower_expr(&for_in.iterable, ctx, use_map, stmt_index);

        // Wrap the iterable in a `.keys()` call: `expr.keys()`
        let keys_call = RustExpr {
            kind: RustExprKind::MethodCall {
                receiver: Box::new(iterable),
                method: "keys".to_owned(),
                type_args: vec![],
                args: vec![],
            },
            span: None,
        };

        let body = self.lower_block(&for_in.body, ctx, use_map, stmt_index, reassigned);

        RustForInStmt {
            label: for_in.label.clone(),
            variable: for_in.variable.name.clone(),
            iterable: keys_call,
            body,
            deref_pattern: false,
            // The `.keys()` call returns an iterator that owns its values,
            // so the emitter should not add `&` prefix.
            iterable_is_borrowed: true,
            span: Some(for_in.span),
        }
    }

    /// Lower a `for await` statement to a `while let Some(item) = stream.next().await`.
    ///
    /// `for await (const item of stream) { body }` →
    /// `while let Some(item) = stream.next().await { body }`
    fn lower_for_await(
        &self,
        for_of: &ast::ForOfStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustWhileLetStmt {
        let stream = self.lower_expr(&for_of.iterable, ctx, use_map, stmt_index);
        let body = self.lower_block(&for_of.body, ctx, use_map, stmt_index, reassigned);

        RustWhileLetStmt {
            label: for_of.label.clone(),
            binding: for_of.variable.name.clone(),
            stream,
            body,
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

    /// Lower a return value, converting array literals to tuples when the return type is a tuple.
    fn lower_return_value(
        &self,
        v: &ast::Expr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        is_tuple_return: bool,
    ) -> RustExpr {
        if is_tuple_return && let ast::ExprKind::ArrayLit(elements) = &v.kind {
            let lowered: Vec<RustExpr> = elements
                .iter()
                .map(|e| match e {
                    ast::ArrayElement::Expr(expr) | ast::ArrayElement::Spread(expr) => {
                        self.lower_expr(expr, ctx, use_map, stmt_index)
                    }
                })
                .collect();
            return RustExpr::new(RustExprKind::Tuple(lowered), v.span);
        }
        self.lower_expr(v, ctx, use_map, stmt_index)
    }

    /// Lower an array destructuring statement.
    ///
    /// `const [a, b] = expr;` → `let (a, b) = expr;`
    /// `const [first, ...rest] = arr;` → indexed access with rest slice.
    /// Typically used with `Promise.all` results.
    /// `const [a, b]: [string, i32] = expr;` → `let (a, b): (String, i32) = expr;`
    fn lower_array_destructure(
        &self,
        adestr: &ast::ArrayDestructureStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustStmt {
        let init = self.lower_expr(&adestr.init, ctx, use_map, stmt_index);

        // Resolve the tuple type annotation if present, to declare typed variables.
        let tuple_element_types = adestr.type_ann.as_ref().and_then(|ann| {
            if let ast::TypeKind::Tuple(types) = &ann.kind {
                let mut diags = Vec::new();
                let resolved: Vec<RustType> = types
                    .iter()
                    .map(|t| {
                        let ty = rsc_typeck::resolve::resolve_type_annotation_with_registry(
                            t,
                            &self.type_registry,
                            &mut diags,
                        );
                        rsc_typeck::bridge::type_to_rust_type(&ty)
                    })
                    .collect();
                for d in diags {
                    ctx.emit_diagnostic(d);
                }
                Some(resolved)
            } else {
                None
            }
        });

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
                        let ty = tuple_element_types
                            .as_ref()
                            .and_then(|types| types.get(i).cloned())
                            .unwrap_or(RustType::Unit);
                        ctx.declare_variable(ident.name.clone(), ty);
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

            // Declare the extracted variables in the current scope with resolved or inferred types.
            for (i, elem) in adestr.elements.iter().enumerate() {
                let name = match elem {
                    ast::ArrayDestructureElement::Single(ident)
                    | ast::ArrayDestructureElement::Rest(ident) => &ident.name,
                };
                let ty = tuple_element_types
                    .as_ref()
                    .and_then(|types| types.get(i).cloned())
                    .unwrap_or(RustType::Unit);
                ctx.declare_variable(name.clone(), ty);
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

        // Mark the catch binding as a catch variable so `.message`/`.name` are
        // handled specially during lowering of the catch body.
        let catch_var_name = tc.catch_binding.as_ref().map(|b| b.name.clone());
        if let Some(ref name) = catch_var_name {
            ctx.mark_as_catch_variable(name.clone());
        }
        let catch_body = self.lower_block(catch_block, ctx, use_map, stmt_index, reassigned);
        if let Some(ref name) = catch_var_name {
            ctx.unmark_catch_variable(name);
        }

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

        // Lower the call expression WITHOUT the ? operator.
        // Use lower_call_expr (not raw lower_expr on each arg) so that
        // Tier 2 borrow transforms and reference-variable clone insertion apply.
        let lowered_call = self.lower_expr(call_expr, ctx, use_map, stmt_index);
        // lower_expr on a Call produces a RustExprKind::Call (or QuestionMark-wrapped
        // for throws functions). For simple try/catch we need the unwrapped call,
        // so strip the QuestionMark wrapper if present.
        let lowered_call = match lowered_call.kind {
            RustExprKind::QuestionMark(inner) => *inner,
            _ => lowered_call,
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

        // Mark the catch binding so `.message`/`.name` are handled specially.
        let catch_var_name = tc.catch_binding.as_ref().map(|b| b.name.clone());
        if let Some(ref name) = catch_var_name {
            ctx.mark_as_catch_variable(name.clone());
        }
        let catch_body = self.lower_block(catch_block, ctx, use_map, stmt_index, reassigned);
        if let Some(ref name) = catch_var_name {
            ctx.unmark_catch_variable(name);
        }

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
