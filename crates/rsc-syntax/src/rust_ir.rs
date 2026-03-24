//! Rust IR type definitions for the Phase 0 syntax subset.
//!
//! The Rust IR is a separate type hierarchy closer to actual Rust syntax than
//! the `RustScript` AST. The lowering pass transforms the AST into IR, and the
//! emitter produces `.rs` source text from it. IR nodes carry `Option<Span>` —
//! `Some` when derived from source, `None` when compiler-generated.

use crate::span::Span;

/// A complete Rust source file, the root of the Rust IR.
///
/// Produced by the lowering pass, consumed by the emitter.
#[derive(Debug, Clone)]
pub struct RustFile {
    /// `use` declarations at the top of the file.
    /// Populated by Tasks 017 and 024.
    pub uses: Vec<RustUseDecl>,
    /// `mod` declarations at the top of the file.
    /// Populated by Task 024.
    pub mod_decls: Vec<RustModDecl>,
    /// The top-level items in this file.
    pub items: Vec<RustItem>,
}

/// A Rust `use` declaration.
///
/// Represents `use path;` in the generated source.
#[derive(Debug, Clone)]
pub struct RustUseDecl {
    /// The use path (e.g., `"std::collections::HashMap"`).
    pub path: String,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `mod` declaration.
///
/// Represents `[pub] mod name;` in the generated source.
#[derive(Debug, Clone)]
pub struct RustModDecl {
    /// The module name.
    pub name: String,
    /// Whether this is a `pub mod` declaration.
    pub public: bool,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A top-level item in a Rust file.
///
/// Phase 0 supports only function declarations.
#[derive(Debug, Clone)]
pub enum RustItem {
    /// A `fn` declaration.
    Function(RustFnDecl),
}

/// A Rust function declaration.
///
/// Produced by lowering a `RustScript` [`FnDecl`](crate::ast::FnDecl).
#[derive(Debug, Clone)]
pub struct RustFnDecl {
    /// The function name.
    pub name: String,
    /// The parameter list.
    pub params: Vec<RustParam>,
    /// The return type, if not unit.
    pub return_type: Option<RustType>,
    /// The function body.
    pub body: RustBlock,
    /// The source span of the original `RustScript` function, if applicable.
    pub span: Option<Span>,
}

/// A Rust function parameter.
#[derive(Debug, Clone)]
pub struct RustParam {
    /// The parameter name.
    pub name: String,
    /// The parameter type.
    pub ty: RustType,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// Rust types available in the IR.
///
/// Each variant corresponds to a concrete Rust type used in emitted code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustType {
    /// Rust `i8`.
    I8,
    /// Rust `i16`.
    I16,
    /// Rust `i32`.
    I32,
    /// Rust `i64`.
    I64,
    /// Rust `u8`.
    U8,
    /// Rust `u16`.
    U16,
    /// Rust `u32`.
    U32,
    /// Rust `u64`.
    U64,
    /// Rust `f32`.
    F32,
    /// Rust `f64`.
    F64,
    /// Rust `bool`.
    Bool,
    /// Rust `String`.
    String,
    /// Rust unit type `()`.
    Unit,
}

impl std::fmt::Display for RustType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Bool => "bool",
            Self::String => "String",
            Self::Unit => "()",
        };
        f.write_str(s)
    }
}

/// A block of Rust statements with an optional trailing expression.
///
/// In Rust, a block can return a value via a trailing expression without a
/// semicolon. The `expr` field captures this.
#[derive(Debug, Clone)]
pub struct RustBlock {
    /// The statements in the block.
    pub stmts: Vec<RustStmt>,
    /// Optional trailing expression (Rust blocks return last value without semicolon).
    pub expr: Option<Box<RustExpr>>,
}

/// A Rust statement.
#[derive(Debug, Clone)]
pub enum RustStmt {
    /// A `let` binding (with optional `mut`).
    Let(RustLetStmt),
    /// An expression without a trailing semicolon (used as trailing block expr).
    Expr(RustExpr),
    /// An expression with a trailing semicolon.
    Semi(RustExpr),
    /// A `return` statement.
    Return(RustReturnStmt),
    /// An `if`/`else` statement.
    If(RustIfStmt),
    /// A `while` loop.
    While(RustWhileStmt),
}

/// A Rust `let` binding.
///
/// Corresponds to `let [mut] name [: ty] = init;`.
#[derive(Debug, Clone)]
pub struct RustLetStmt {
    /// Whether the binding is `mut`.
    pub mutable: bool,
    /// The variable name.
    pub name: String,
    /// The optional type annotation.
    pub ty: Option<RustType>,
    /// The initializer expression.
    pub init: RustExpr,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `return` statement.
#[derive(Debug, Clone)]
pub struct RustReturnStmt {
    /// The return value, if present.
    pub value: Option<RustExpr>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `if`/`else` statement.
#[derive(Debug, Clone)]
pub struct RustIfStmt {
    /// The condition expression.
    pub condition: RustExpr,
    /// The then-branch block.
    pub then_block: RustBlock,
    /// The optional else clause.
    pub else_clause: Option<RustElse>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// The else clause of a Rust `if` statement.
#[derive(Debug, Clone)]
pub enum RustElse {
    /// A plain `else { ... }` block.
    Block(RustBlock),
    /// An `else if ...` chain.
    ElseIf(Box<RustIfStmt>),
}

/// A Rust `while` loop.
#[derive(Debug, Clone)]
pub struct RustWhileStmt {
    /// The loop condition expression.
    pub condition: RustExpr,
    /// The loop body.
    pub body: RustBlock,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust expression with optional source span.
///
/// Uses a kind+span wrapper pattern. Source-derived expressions carry
/// `Some(span)`, compiler-generated expressions carry `None`.
#[derive(Debug, Clone)]
pub struct RustExpr {
    /// The kind of expression.
    pub kind: RustExprKind,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

impl RustExpr {
    /// Create an expression with a source span.
    #[must_use]
    pub fn new(kind: RustExprKind, span: Span) -> Self {
        Self {
            kind,
            span: Some(span),
        }
    }

    /// Create a compiler-generated expression with no source span.
    #[must_use]
    pub fn synthetic(kind: RustExprKind) -> Self {
        Self { kind, span: None }
    }
}

/// The kinds of Rust expressions in the Phase 0 IR.
#[derive(Debug, Clone)]
pub enum RustExprKind {
    /// An integer literal (e.g., `42`).
    IntLit(i64),
    /// A floating-point literal (e.g., `3.14`).
    FloatLit(f64),
    /// A string literal (e.g., `"hello"`).
    StringLit(String),
    /// A boolean literal (`true` or `false`).
    BoolLit(bool),
    /// An identifier reference (e.g., `x`).
    Ident(String),
    /// A binary operation (e.g., `a + b`).
    Binary {
        /// The binary operator.
        op: RustBinaryOp,
        /// The left-hand operand.
        left: Box<RustExpr>,
        /// The right-hand operand.
        right: Box<RustExpr>,
    },
    /// A unary operation (e.g., `-x`).
    Unary {
        /// The unary operator.
        op: RustUnaryOp,
        /// The operand.
        operand: Box<RustExpr>,
    },
    /// A function call (e.g., `foo(a, b)`).
    Call {
        /// The function name.
        func: String,
        /// The argument list.
        args: Vec<RustExpr>,
    },
    /// A method call (e.g., `receiver.method(args)`).
    MethodCall {
        /// The receiver expression.
        receiver: Box<RustExpr>,
        /// The method name.
        method: String,
        /// The argument list.
        args: Vec<RustExpr>,
    },
    /// A parenthesized expression.
    Paren(Box<RustExpr>),
    /// An assignment expression (e.g., `x = value`).
    Assign {
        /// The assignment target.
        target: String,
        /// The value being assigned.
        value: Box<RustExpr>,
    },
    /// A macro invocation (e.g., `println!`).
    ///
    /// Used for lowering `console.log()` to `println!`.
    Macro {
        /// The macro name (without the `!`).
        name: String,
        /// The arguments to the macro.
        args: Vec<RustExpr>,
    },
    /// A `.clone()` call inserted for ownership correctness.
    Clone(Box<RustExpr>),
    /// A `.to_string()` conversion.
    ToString(Box<RustExpr>),
    /// A compound assignment expression (e.g., `x += 1`).
    CompoundAssign {
        /// The assignment target.
        target: String,
        /// The compound assignment operator.
        op: RustCompoundAssignOp,
        /// The right-hand side value.
        value: Box<RustExpr>,
    },
}

/// A compound assignment operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustCompoundAssignOp {
    /// Addition assignment (`+=`).
    AddAssign,
    /// Subtraction assignment (`-=`).
    SubAssign,
    /// Multiplication assignment (`*=`).
    MulAssign,
    /// Division assignment (`/=`).
    DivAssign,
    /// Remainder assignment (`%=`).
    RemAssign,
}

impl std::fmt::Display for RustCompoundAssignOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::AddAssign => "+=",
            Self::SubAssign => "-=",
            Self::MulAssign => "*=",
            Self::DivAssign => "/=",
            Self::RemAssign => "%=",
        };
        f.write_str(s)
    }
}

/// Rust binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustBinaryOp {
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Sub,
    /// Multiplication (`*`).
    Mul,
    /// Division (`/`).
    Div,
    /// Remainder (`%`).
    Rem,
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

impl std::fmt::Display for RustBinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
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

/// Rust unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustUnaryOp {
    /// Arithmetic negation (`-`).
    Neg,
    /// Logical NOT (`!`).
    Not,
}

impl std::fmt::Display for RustUnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Neg => "-",
            Self::Not => "!",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a span for tests.
    fn span(start: u32, end: u32) -> Span {
        Span::new(start, end)
    }

    #[test]
    fn test_rust_fn_decl_complete_construction() {
        let decl = RustFnDecl {
            name: "add".to_owned(),
            params: vec![
                RustParam {
                    name: "a".to_owned(),
                    ty: RustType::I32,
                    span: Some(span(4, 10)),
                },
                RustParam {
                    name: "b".to_owned(),
                    ty: RustType::I32,
                    span: Some(span(12, 18)),
                },
            ],
            return_type: Some(RustType::I32),
            body: RustBlock {
                stmts: vec![],
                expr: Some(Box::new(RustExpr::new(
                    RustExprKind::Binary {
                        op: RustBinaryOp::Add,
                        left: Box::new(RustExpr::new(
                            RustExprKind::Ident("a".to_owned()),
                            span(25, 26),
                        )),
                        right: Box::new(RustExpr::new(
                            RustExprKind::Ident("b".to_owned()),
                            span(29, 30),
                        )),
                    },
                    span(25, 30),
                ))),
            },
            span: Some(span(0, 32)),
        };

        assert_eq!(decl.name, "add");
        assert_eq!(decl.params.len(), 2);
        assert_eq!(decl.return_type, Some(RustType::I32));
        assert!(decl.span.is_some());
    }

    #[test]
    fn test_rust_block_trailing_expr_vs_stmt_only() {
        // Block with trailing expression (returns a value).
        let with_expr = RustBlock {
            stmts: vec![RustStmt::Semi(RustExpr::new(
                RustExprKind::IntLit(1),
                span(0, 1),
            ))],
            expr: Some(Box::new(RustExpr::new(RustExprKind::IntLit(2), span(3, 4)))),
        };

        assert_eq!(with_expr.stmts.len(), 1);
        assert!(with_expr.expr.is_some());

        // Block without trailing expression (returns unit).
        let stmt_only = RustBlock {
            stmts: vec![
                RustStmt::Semi(RustExpr::new(RustExprKind::IntLit(1), span(0, 1))),
                RustStmt::Semi(RustExpr::new(RustExprKind::IntLit(2), span(3, 4))),
            ],
            expr: None,
        };

        assert_eq!(stmt_only.stmts.len(), 2);
        assert!(stmt_only.expr.is_none());
    }

    #[test]
    fn test_rust_type_display_all_variants() {
        assert_eq!(RustType::I8.to_string(), "i8");
        assert_eq!(RustType::I16.to_string(), "i16");
        assert_eq!(RustType::I32.to_string(), "i32");
        assert_eq!(RustType::I64.to_string(), "i64");
        assert_eq!(RustType::U8.to_string(), "u8");
        assert_eq!(RustType::U16.to_string(), "u16");
        assert_eq!(RustType::U32.to_string(), "u32");
        assert_eq!(RustType::U64.to_string(), "u64");
        assert_eq!(RustType::F32.to_string(), "f32");
        assert_eq!(RustType::F64.to_string(), "f64");
        assert_eq!(RustType::Bool.to_string(), "bool");
        assert_eq!(RustType::String.to_string(), "String");
        assert_eq!(RustType::Unit.to_string(), "()");
    }

    #[test]
    fn test_rust_binary_op_display_all_variants() {
        assert_eq!(RustBinaryOp::Add.to_string(), "+");
        assert_eq!(RustBinaryOp::Sub.to_string(), "-");
        assert_eq!(RustBinaryOp::Mul.to_string(), "*");
        assert_eq!(RustBinaryOp::Div.to_string(), "/");
        assert_eq!(RustBinaryOp::Rem.to_string(), "%");
        assert_eq!(RustBinaryOp::Eq.to_string(), "==");
        assert_eq!(RustBinaryOp::Ne.to_string(), "!=");
        assert_eq!(RustBinaryOp::Lt.to_string(), "<");
        assert_eq!(RustBinaryOp::Gt.to_string(), ">");
        assert_eq!(RustBinaryOp::Le.to_string(), "<=");
        assert_eq!(RustBinaryOp::Ge.to_string(), ">=");
        assert_eq!(RustBinaryOp::And.to_string(), "&&");
        assert_eq!(RustBinaryOp::Or.to_string(), "||");
    }

    #[test]
    fn test_rust_unary_op_display_both_variants() {
        assert_eq!(RustUnaryOp::Neg.to_string(), "-");
        assert_eq!(RustUnaryOp::Not.to_string(), "!");
    }

    #[test]
    fn test_rust_expr_synthetic_span_is_none() {
        let expr = RustExpr::synthetic(RustExprKind::Clone(Box::new(RustExpr::synthetic(
            RustExprKind::Ident("x".to_owned()),
        ))));

        assert!(expr.span.is_none());
        match &expr.kind {
            RustExprKind::Clone(inner) => assert!(inner.span.is_none()),
            _ => panic!("expected Clone variant"),
        }
    }

    #[test]
    fn test_rust_expr_new_span_is_some() {
        let s = span(10, 20);
        let expr = RustExpr::new(RustExprKind::IntLit(42), s);

        assert_eq!(expr.span, Some(s));
    }
}
