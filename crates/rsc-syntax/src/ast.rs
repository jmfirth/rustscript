//! `RustScript` AST type definitions for the Phase 0 syntax subset.
//!
//! The parser produces this AST from `.rts` source. The lowering pass consumes
//! it and produces Rust IR. Each node carries a [`Span`] for diagnostic reporting.

use crate::span::Span;

/// A complete `RustScript` module (one `.rts` file).
///
/// This is the root AST node. Named `Module` rather than `SourceFile` to avoid
/// collision with [`crate::source::SourceFile`] and to correctly reflect that
/// each `.rts` file maps to a Rust module.
#[derive(Debug, Clone)]
pub struct Module {
    /// The top-level items in this module.
    pub items: Vec<Item>,
    /// The span covering the entire module.
    pub span: Span,
}

/// A top-level item in a `RustScript` module.
///
/// Wraps an [`ItemKind`] with metadata common to all items (export status,
/// source span). Phase 0 supports only function declarations; Phase 1 will
/// add type definitions, enums, interfaces, classes, and imports.
#[derive(Debug, Clone)]
pub struct Item {
    /// The kind of item.
    pub kind: ItemKind,
    /// Whether this item is exported (`export` keyword). Defaults to `false`
    /// until the module system is implemented (Task 024).
    pub exported: bool,
    /// The span covering the entire item.
    pub span: Span,
}

/// The kinds of top-level items in a `RustScript` module.
///
/// Phase 0 supports function declarations; Phase 1 adds type definitions.
#[derive(Debug, Clone)]
pub enum ItemKind {
    /// A function declaration (`function name(...) { ... }`).
    Function(FnDecl),
    /// A type definition (`type Name = { field: Type, ... }`).
    /// Lowers to a Rust `struct`.
    TypeDef(TypeDef),
}

/// A function declaration.
///
/// Corresponds to `RustScript` `function name(params): ReturnType { body }`.
/// Lowers to a Rust `fn` item.
#[derive(Debug, Clone)]
pub struct FnDecl {
    /// The function name.
    pub name: Ident,
    /// The parameter list.
    pub params: Vec<Param>,
    /// The return type annotation, if present. Absent means `void`.
    pub return_type: Option<TypeAnnotation>,
    /// The function body.
    pub body: Block,
    /// The span covering the entire function declaration.
    pub span: Span,
}

/// A type definition: `type Name = { field: Type, ... }`.
///
/// Lowers to a Rust `struct` with `pub` fields.
#[derive(Debug, Clone)]
pub struct TypeDef {
    /// The type name.
    pub name: Ident,
    /// The fields of the type definition.
    pub fields: Vec<FieldDef>,
    /// The span covering the entire type definition.
    pub span: Span,
}

/// A field in a type definition.
///
/// Corresponds to `name: Type` within a type definition body.
/// Lowers to a `pub` field in the Rust struct.
#[derive(Debug, Clone)]
pub struct FieldDef {
    /// The field name.
    pub name: Ident,
    /// The field type annotation.
    pub type_ann: TypeAnnotation,
    /// The span covering the field definition.
    pub span: Span,
}

/// A function parameter with a name and type annotation.
///
/// Corresponds to `name: Type` in a function parameter list.
#[derive(Debug, Clone)]
pub struct Param {
    /// The parameter name.
    pub name: Ident,
    /// The type annotation.
    pub type_ann: TypeAnnotation,
    /// The span covering the parameter.
    pub span: Span,
}

/// A type annotation on a variable or parameter.
///
/// Wraps a [`TypeKind`] with a source span.
#[derive(Debug, Clone)]
pub struct TypeAnnotation {
    /// The kind of type being annotated.
    pub kind: TypeKind,
    /// The span covering the type annotation.
    pub span: Span,
}

/// The kinds of types expressible in Phase 0 `RustScript`.
///
/// `Named` covers primitive types (`i32`, `i64`, `f64`, `bool`, `string`) and
/// user-defined types. `Void` represents the absence of a return value.
#[derive(Debug, Clone)]
pub enum TypeKind {
    /// A named type (e.g., `i32`, `bool`, `string`, or a user-defined name).
    Named(Ident),
    /// The void type, indicating no return value. Lowers to Rust `()`.
    Void,
}

/// An identifier with its source span.
#[derive(Debug, Clone)]
pub struct Ident {
    /// The identifier text.
    pub name: String,
    /// The span covering the identifier.
    pub span: Span,
}

/// A block of statements enclosed in braces.
///
/// Corresponds to `{ stmt; stmt; ... }` in `RustScript`.
#[derive(Debug, Clone)]
pub struct Block {
    /// The statements within the block.
    pub stmts: Vec<Stmt>,
    /// The span covering the entire block, including braces.
    pub span: Span,
}

/// A statement within a block.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// A variable declaration (`const` or `let` binding).
    VarDecl(VarDecl),
    /// An expression statement.
    Expr(Expr),
    /// A `return` statement.
    Return(ReturnStmt),
    /// An `if`/`else` statement.
    If(IfStmt),
    /// A `while` loop.
    While(WhileStmt),
    /// A destructuring declaration: `const { name, age } = user;`.
    /// Lowers to Rust `let TypeName { field1, field2, .. } = expr;`.
    Destructure(DestructureStmt),
}

/// A variable declaration with an initializer.
///
/// Corresponds to `const name: Type = expr` or `let name: Type = expr`.
/// `const` lowers to Rust `let` (immutable), `let` lowers to `let mut`.
#[derive(Debug, Clone)]
pub struct VarDecl {
    /// Whether this is a `const` or `let` binding.
    pub binding: VarBinding,
    /// The variable name.
    pub name: Ident,
    /// The optional type annotation.
    pub type_ann: Option<TypeAnnotation>,
    /// The initializer expression.
    pub init: Expr,
    /// The span covering the entire declaration.
    pub span: Span,
}

/// The binding kind for a variable declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarBinding {
    /// An immutable binding (`const`). Lowers to Rust `let`.
    Const,
    /// A mutable binding (`let`). Lowers to Rust `let mut`.
    Let,
}

/// A `return` statement, optionally with a value.
///
/// Corresponds to `return expr;` or bare `return;`.
#[derive(Debug, Clone)]
pub struct ReturnStmt {
    /// The return value, if present.
    pub value: Option<Expr>,
    /// The span covering the `return` keyword and value.
    pub span: Span,
}

/// An `if`/`else` statement.
///
/// Supports `else if` chains via [`ElseClause::ElseIf`].
#[derive(Debug, Clone)]
pub struct IfStmt {
    /// The condition expression.
    pub condition: Expr,
    /// The then-branch block.
    pub then_block: Block,
    /// The optional else clause (block or else-if chain).
    pub else_clause: Option<ElseClause>,
    /// The span covering the entire `if`/`else` statement.
    pub span: Span,
}

/// The else clause of an `if` statement.
#[derive(Debug, Clone)]
pub enum ElseClause {
    /// A plain `else { ... }` block.
    Block(Block),
    /// An `else if ...` chain.
    ElseIf(Box<IfStmt>),
}

/// A `while` loop.
///
/// Corresponds to `while (condition) { body }`.
#[derive(Debug, Clone)]
pub struct WhileStmt {
    /// The loop condition expression.
    pub condition: Expr,
    /// The loop body.
    pub body: Block,
    /// The span covering the entire `while` statement.
    pub span: Span,
}

/// A destructuring declaration: `const { name, age } = user;`.
///
/// Lowers to Rust `let TypeName { field1, field2, .. } = expr;`.
#[derive(Debug, Clone)]
pub struct DestructureStmt {
    /// Whether this is a `const` or `let` binding.
    pub binding: VarBinding,
    /// The field names being extracted.
    pub fields: Vec<Ident>,
    /// The initializer expression being destructured.
    pub init: Expr,
    /// The span covering the entire destructuring statement.
    pub span: Span,
}

/// An expression with its source span.
///
/// Wraps an [`ExprKind`] variant with the span of source text it was parsed from.
#[derive(Debug, Clone)]
pub struct Expr {
    /// The kind of expression.
    pub kind: ExprKind,
    /// The span covering the expression.
    pub span: Span,
}

/// The kinds of expressions in Phase 0 `RustScript`.
#[derive(Debug, Clone)]
pub enum ExprKind {
    /// An integer literal (e.g., `42`).
    IntLit(i64),
    /// A floating-point literal (e.g., `3.14`).
    FloatLit(f64),
    /// A string literal (e.g., `"hello"`).
    StringLit(String),
    /// A boolean literal (`true` or `false`).
    BoolLit(bool),
    /// An identifier reference (e.g., `x`).
    Ident(Ident),
    /// A binary operation (e.g., `a + b`).
    Binary(BinaryExpr),
    /// A unary operation (e.g., `-x`, `!flag`).
    Unary(UnaryExpr),
    /// A function call (e.g., `foo(a, b)`).
    Call(CallExpr),
    /// A method call (e.g., `obj.method(a)`).
    MethodCall(MethodCallExpr),
    /// A parenthesized expression (e.g., `(a + b)`).
    Paren(Box<Expr>),
    /// An assignment expression (e.g., `x = 5`).
    Assign(AssignExpr),
    /// A struct literal: `{ name: "Alice", age: 30 }` or `User { ... }`.
    /// Lowers to a Rust struct construction expression.
    StructLit(StructLitExpr),
    /// Field access: `user.name`.
    /// Lowers to Rust field access `expr.field`.
    FieldAccess(FieldAccessExpr),
}

/// A binary expression with an operator and two operands.
#[derive(Debug, Clone)]
pub struct BinaryExpr {
    /// The binary operator.
    pub op: BinaryOp,
    /// The left-hand operand.
    pub left: Box<Expr>,
    /// The right-hand operand.
    pub right: Box<Expr>,
}

/// Binary operators in `RustScript`.
///
/// Arithmetic, comparison, and logical operators available in Phase 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Sub,
    /// Multiplication (`*`).
    Mul,
    /// Division (`/`).
    Div,
    /// Modulo (`%`).
    Mod,
    /// Equality (`==`).
    Eq,
    /// Inequality (`!=`).
    Ne,
    /// Less than (`<`).
    Lt,
    /// Greater than (`>`).
    Gt,
    /// Less than or equal (`<=`).
    Le,
    /// Greater than or equal (`>=`).
    Ge,
    /// Logical AND (`&&`).
    And,
    /// Logical OR (`||`).
    Or,
}

impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::And => "&&",
            Self::Or => "||",
        };
        f.write_str(s)
    }
}

/// A unary expression with an operator and operand.
#[derive(Debug, Clone)]
pub struct UnaryExpr {
    /// The unary operator.
    pub op: UnaryOp,
    /// The operand.
    pub operand: Box<Expr>,
}

/// Unary operators in `RustScript`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Arithmetic negation (`-`).
    Neg,
    /// Logical NOT (`!`).
    Not,
}

/// A function call expression.
///
/// Corresponds to `callee(args...)`. Lowers to a Rust function call.
#[derive(Debug, Clone)]
pub struct CallExpr {
    /// The function being called.
    pub callee: Ident,
    /// The argument list.
    pub args: Vec<Expr>,
}

/// A method call expression.
///
/// Corresponds to `object.method(args...)`.
#[derive(Debug, Clone)]
pub struct MethodCallExpr {
    /// The receiver object.
    pub object: Box<Expr>,
    /// The method name.
    pub method: Ident,
    /// The argument list.
    pub args: Vec<Expr>,
}

/// An assignment expression.
///
/// Corresponds to `target = value`. Only simple identifier targets are
/// supported in Phase 0.
#[derive(Debug, Clone)]
pub struct AssignExpr {
    /// The assignment target (an identifier in Phase 0).
    pub target: Ident,
    /// The value being assigned.
    pub value: Box<Expr>,
}

/// A struct literal expression.
///
/// Corresponds to `{ name: "Alice", age: 30 }` in expression position.
/// The `type_name` is resolved during lowering from context (e.g., the
/// variable's type annotation).
#[derive(Debug, Clone)]
pub struct StructLitExpr {
    /// The type name (for typed literals like `User { ... }`). None for
    /// untyped object literals.
    pub type_name: Option<Ident>,
    /// The field initializers.
    pub fields: Vec<FieldInit>,
}

/// A field initializer in a struct literal.
///
/// Corresponds to `name: expr` within a struct literal body.
#[derive(Debug, Clone)]
pub struct FieldInit {
    /// The field name.
    pub name: Ident,
    /// The field value expression.
    pub value: Expr,
    /// The span covering the field initializer.
    pub span: Span,
}

/// A field access expression: `expr.field`.
///
/// Supports chaining: `user.address.city` is
/// `FieldAccess(FieldAccess(user, address), city)`.
/// Lowers to Rust field access syntax.
#[derive(Debug, Clone)]
pub struct FieldAccessExpr {
    /// The object expression being accessed.
    pub object: Box<Expr>,
    /// The field name.
    pub field: Ident,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a span for tests.
    fn span(start: u32, end: u32) -> Span {
        Span::new(start, end)
    }

    /// Helper to create an identifier for tests.
    fn ident(name: &str, start: u32, end: u32) -> Ident {
        Ident {
            name: name.to_owned(),
            span: span(start, end),
        }
    }

    /// Helper to create a simple integer expression for tests.
    fn int_expr(value: i64, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::IntLit(value),
            span: span(start, end),
        }
    }

    #[test]
    fn test_fn_decl_with_two_params_field_access() {
        let decl = FnDecl {
            name: ident("add", 0, 3),
            params: vec![
                Param {
                    name: ident("a", 4, 5),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("i32", 7, 10)),
                        span: span(7, 10),
                    },
                    span: span(4, 10),
                },
                Param {
                    name: ident("b", 12, 13),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("i32", 15, 18)),
                        span: span(15, 18),
                    },
                    span: span(12, 18),
                },
            ],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("i32", 21, 24)),
                span: span(21, 24),
            }),
            body: Block {
                stmts: vec![],
                span: span(25, 27),
            },
            span: span(0, 27),
        };

        assert_eq!(decl.name.name, "add");
        assert_eq!(decl.params.len(), 2);
        assert_eq!(decl.params[0].name.name, "a");
        assert_eq!(decl.params[1].name.name, "b");
        assert!(decl.return_type.is_some());
    }

    #[test]
    fn test_if_stmt_with_else_if_chain_nesting() {
        let inner_if = IfStmt {
            condition: Expr {
                kind: ExprKind::BoolLit(false),
                span: span(30, 35),
            },
            then_block: Block {
                stmts: vec![],
                span: span(36, 38),
            },
            else_clause: Some(ElseClause::Block(Block {
                stmts: vec![],
                span: span(44, 46),
            })),
            span: span(25, 46),
        };

        let outer_if = IfStmt {
            condition: Expr {
                kind: ExprKind::BoolLit(true),
                span: span(3, 7),
            },
            then_block: Block {
                stmts: vec![],
                span: span(8, 10),
            },
            else_clause: Some(ElseClause::ElseIf(Box::new(inner_if))),
            span: span(0, 46),
        };

        // Verify nesting: outer else clause is ElseIf
        match &outer_if.else_clause {
            Some(ElseClause::ElseIf(inner)) => {
                // Inner else clause is a Block
                assert!(matches!(inner.else_clause, Some(ElseClause::Block(_))));
            }
            _ => panic!("expected ElseIf clause"),
        }
    }

    #[test]
    fn test_while_stmt_with_var_decl_and_assign() {
        let var_decl = Stmt::VarDecl(VarDecl {
            binding: VarBinding::Let,
            name: ident("x", 10, 11),
            type_ann: None,
            init: int_expr(0, 14, 15),
            span: span(6, 16),
        });

        let assign = Stmt::Expr(Expr {
            kind: ExprKind::Assign(AssignExpr {
                target: ident("x", 17, 18),
                value: Box::new(Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Add,
                        left: Box::new(Expr {
                            kind: ExprKind::Ident(ident("x", 21, 22)),
                            span: span(21, 22),
                        }),
                        right: Box::new(int_expr(1, 25, 26)),
                    }),
                    span: span(21, 26),
                }),
            }),
            span: span(17, 26),
        });

        let while_stmt = WhileStmt {
            condition: Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Lt,
                    left: Box::new(Expr {
                        kind: ExprKind::Ident(ident("x", 0, 1)),
                        span: span(0, 1),
                    }),
                    right: Box::new(int_expr(10, 4, 6)),
                }),
                span: span(0, 6),
            },
            body: Block {
                stmts: vec![var_decl, assign],
                span: span(6, 27),
            },
            span: span(0, 27),
        };

        assert_eq!(while_stmt.body.stmts.len(), 2);
        assert!(matches!(while_stmt.body.stmts[0], Stmt::VarDecl(_)));
        assert!(matches!(while_stmt.body.stmts[1], Stmt::Expr(_)));
    }

    #[test]
    fn test_every_expr_kind_variant_spans_stored() {
        let cases: Vec<Expr> = vec![
            Expr {
                kind: ExprKind::IntLit(42),
                span: span(0, 2),
            },
            Expr {
                kind: ExprKind::FloatLit(3.14),
                span: span(10, 14),
            },
            Expr {
                kind: ExprKind::StringLit("hello".to_owned()),
                span: span(20, 27),
            },
            Expr {
                kind: ExprKind::BoolLit(true),
                span: span(30, 34),
            },
            Expr {
                kind: ExprKind::Ident(ident("x", 40, 41)),
                span: span(40, 41),
            },
            Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Add,
                    left: Box::new(int_expr(1, 50, 51)),
                    right: Box::new(int_expr(2, 54, 55)),
                }),
                span: span(50, 55),
            },
            Expr {
                kind: ExprKind::Unary(UnaryExpr {
                    op: UnaryOp::Neg,
                    operand: Box::new(int_expr(5, 61, 62)),
                }),
                span: span(60, 62),
            },
            Expr {
                kind: ExprKind::Call(CallExpr {
                    callee: ident("foo", 70, 73),
                    args: vec![],
                }),
                span: span(70, 75),
            },
            Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(Expr {
                        kind: ExprKind::Ident(ident("obj", 80, 83)),
                        span: span(80, 83),
                    }),
                    method: ident("bar", 84, 87),
                    args: vec![],
                }),
                span: span(80, 89),
            },
            Expr {
                kind: ExprKind::Paren(Box::new(int_expr(1, 91, 92))),
                span: span(90, 93),
            },
            Expr {
                kind: ExprKind::Assign(AssignExpr {
                    target: ident("x", 100, 101),
                    value: Box::new(int_expr(5, 104, 105)),
                }),
                span: span(100, 105),
            },
        ];

        // Verify each variant stores its span correctly.
        let expected_starts = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        for (expr, &expected_start) in cases.iter().zip(&expected_starts) {
            assert_eq!(
                expr.span.start.0,
                expected_start,
                "span mismatch for {:?}",
                std::mem::discriminant(&expr.kind)
            );
        }
    }

    #[test]
    fn test_var_decl_const_and_let_with_and_without_type_ann() {
        let const_with_type = VarDecl {
            binding: VarBinding::Const,
            name: ident("x", 6, 7),
            type_ann: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("i32", 9, 12)),
                span: span(9, 12),
            }),
            init: int_expr(42, 15, 17),
            span: span(0, 18),
        };

        let let_without_type = VarDecl {
            binding: VarBinding::Let,
            name: ident("y", 4, 5),
            type_ann: None,
            init: int_expr(99, 8, 10),
            span: span(0, 11),
        };

        assert_eq!(const_with_type.binding, VarBinding::Const);
        assert!(const_with_type.type_ann.is_some());

        assert_eq!(let_without_type.binding, VarBinding::Let);
        assert!(let_without_type.type_ann.is_none());
    }

    #[test]
    fn test_binary_op_display_all_variants() {
        assert_eq!(BinaryOp::Add.to_string(), "+");
        assert_eq!(BinaryOp::Sub.to_string(), "-");
        assert_eq!(BinaryOp::Mul.to_string(), "*");
        assert_eq!(BinaryOp::Div.to_string(), "/");
        assert_eq!(BinaryOp::Mod.to_string(), "%");
        assert_eq!(BinaryOp::Eq.to_string(), "==");
        assert_eq!(BinaryOp::Ne.to_string(), "!=");
        assert_eq!(BinaryOp::Lt.to_string(), "<");
        assert_eq!(BinaryOp::Gt.to_string(), ">");
        assert_eq!(BinaryOp::Le.to_string(), "<=");
        assert_eq!(BinaryOp::Ge.to_string(), ">=");
        assert_eq!(BinaryOp::And.to_string(), "&&");
        assert_eq!(BinaryOp::Or.to_string(), "||");
    }
}
