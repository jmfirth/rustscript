//! Internal emitter implementation.
//!
//! Walks the Rust IR tree and produces formatted `.rs` source text.

use rsc_syntax::rust_ir::{
    RustBlock, RustElse, RustExpr, RustExprKind, RustFile, RustFnDecl, RustIfStmt, RustItem,
    RustStmt,
};

/// Walks Rust IR and builds a formatted `.rs` source string.
struct Emitter {
    /// The accumulated output text.
    output: String,
    /// The current indentation level (each level = 4 spaces).
    indent: usize,
}

impl Emitter {
    /// Create a new emitter with empty output and zero indentation.
    fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    /// Append raw text to the output.
    fn write(&mut self, s: &str) {
        self.output.push_str(s);
    }

    /// Append text followed by a newline.
    fn writeln(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    /// Append a bare newline.
    fn newline(&mut self) {
        self.output.push('\n');
    }

    /// Write indentation spaces for the current level.
    fn write_indent(&mut self) {
        for _ in 0..self.indent * 4 {
            self.output.push(' ');
        }
    }

    /// Increase the indentation level by one.
    fn push_indent(&mut self) {
        self.indent += 1;
    }

    /// Decrease the indentation level by one.
    fn pop_indent(&mut self) {
        self.indent -= 1;
    }

    /// Emit an entire Rust source file.
    fn emit_file(&mut self, file: &RustFile) {
        for (i, item) in file.items.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.emit_item(item);
        }
    }

    /// Emit a top-level item.
    fn emit_item(&mut self, item: &RustItem) {
        match item {
            RustItem::Function(f) => self.emit_fn(f),
        }
    }

    /// Emit a function declaration.
    fn emit_fn(&mut self, f: &RustFnDecl) {
        self.write_indent();
        self.write("fn ");
        self.write(&f.name);
        self.write("(");

        for (i, param) in f.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&param.name);
            self.write(": ");
            self.write(&param.ty.to_string());
        }

        self.write(")");

        if let Some(ref ret) = f.return_type {
            self.write(" -> ");
            self.write(&ret.to_string());
        }

        self.write(" ");
        self.emit_block(&f.body);
        self.newline();
    }

    /// Emit a block `{ stmts; [expr] }`.
    fn emit_block(&mut self, block: &RustBlock) {
        self.writeln("{");
        self.push_indent();

        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }

        if let Some(ref expr) = block.expr {
            self.write_indent();
            self.emit_expr(expr);
            self.newline();
        }

        self.pop_indent();
        self.write_indent();
        self.write("}");
    }

    /// Emit a statement.
    fn emit_stmt(&mut self, stmt: &RustStmt) {
        match stmt {
            RustStmt::Let(let_stmt) => {
                self.write_indent();
                if let_stmt.mutable {
                    self.write("let mut ");
                } else {
                    self.write("let ");
                }
                self.write(&let_stmt.name);
                if let Some(ref ty) = let_stmt.ty {
                    self.write(": ");
                    self.write(&ty.to_string());
                }
                self.write(" = ");
                self.emit_expr(&let_stmt.init);
                self.writeln(";");
            }
            RustStmt::Expr(expr) => {
                self.write_indent();
                self.emit_expr(expr);
                self.newline();
            }
            RustStmt::Semi(expr) => {
                self.write_indent();
                self.emit_expr(expr);
                self.writeln(";");
            }
            RustStmt::Return(ret) => {
                self.write_indent();
                if let Some(ref val) = ret.value {
                    self.write("return ");
                    self.emit_expr(val);
                    self.writeln(";");
                } else {
                    self.writeln("return;");
                }
            }
            RustStmt::If(if_stmt) => {
                self.write_indent();
                self.emit_if(if_stmt);
                self.newline();
            }
            RustStmt::While(while_stmt) => {
                self.write_indent();
                self.write("while ");
                self.emit_expr(&while_stmt.condition);
                self.write(" ");
                self.emit_block(&while_stmt.body);
                self.newline();
            }
        }
    }

    /// Emit an if/else chain (without leading indent — caller handles that).
    fn emit_if(&mut self, if_stmt: &RustIfStmt) {
        self.write("if ");
        self.emit_expr(&if_stmt.condition);
        self.write(" ");
        self.emit_block(&if_stmt.then_block);

        if let Some(ref else_clause) = if_stmt.else_clause {
            match else_clause {
                RustElse::Block(block) => {
                    self.write(" else ");
                    self.emit_block(block);
                }
                RustElse::ElseIf(nested_if) => {
                    self.write(" else ");
                    self.emit_if(nested_if);
                }
            }
        }
    }

    /// Emit an expression.
    #[allow(clippy::too_many_lines)]
    // Expression emission covers all IR node kinds; splitting would obscure the match structure
    fn emit_expr(&mut self, expr: &RustExpr) {
        match &expr.kind {
            RustExprKind::IntLit(n) => {
                self.write(&n.to_string());
            }
            RustExprKind::FloatLit(f) => {
                let s = f.to_string();
                if s.contains('.') {
                    self.write(&s);
                } else {
                    self.write(&s);
                    self.write(".0");
                }
            }
            RustExprKind::StringLit(s) => {
                self.write("\"");
                for ch in s.chars() {
                    match ch {
                        '\\' => self.write("\\\\"),
                        '"' => self.write("\\\""),
                        '\n' => self.write("\\n"),
                        '\t' => self.write("\\t"),
                        '\r' => self.write("\\r"),
                        '\0' => self.write("\\0"),
                        _ => self.output.push(ch),
                    }
                }
                self.write("\"");
            }
            RustExprKind::BoolLit(b) => {
                if *b {
                    self.write("true");
                } else {
                    self.write("false");
                }
            }
            RustExprKind::Ident(name) => {
                self.write(name);
            }
            RustExprKind::Binary { op, left, right } => {
                self.emit_expr(left);
                self.write(" ");
                self.write(&op.to_string());
                self.write(" ");
                self.emit_expr(right);
            }
            RustExprKind::Unary { op, operand } => {
                self.write(&op.to_string());
                self.emit_expr(operand);
            }
            RustExprKind::Call { func, args } => {
                self.write(func);
                self.write("(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_expr(arg);
                }
                self.write(")");
            }
            RustExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                self.emit_expr(receiver);
                self.write(".");
                self.write(method);
                self.write("(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_expr(arg);
                }
                self.write(")");
            }
            RustExprKind::Paren(inner) => {
                self.write("(");
                self.emit_expr(inner);
                self.write(")");
            }
            RustExprKind::Assign { target, value } => {
                self.write(target);
                self.write(" = ");
                self.emit_expr(value);
            }
            RustExprKind::Macro { name, args } => {
                self.write(name);
                self.write("!(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_expr(arg);
                }
                self.write(")");
            }
            RustExprKind::Clone(inner) => {
                self.emit_expr(inner);
                self.write(".clone()");
            }
            RustExprKind::ToString(inner) => {
                self.emit_expr(inner);
                self.write(".to_string()");
            }
        }
    }
}

/// Emit Rust source code from Rust IR.
pub fn emit(file: &RustFile) -> String {
    let mut emitter = Emitter::new();
    emitter.emit_file(file);
    emitter.output
}

#[cfg(test)]
mod tests {
    use rsc_syntax::rust_ir::{
        RustBinaryOp, RustBlock, RustElse, RustExpr, RustExprKind, RustFile, RustFnDecl,
        RustIfStmt, RustItem, RustLetStmt, RustParam, RustReturnStmt, RustStmt, RustType,
        RustUnaryOp, RustWhileStmt,
    };

    use super::emit;

    /// Helper: construct a synthetic expression.
    fn syn(kind: RustExprKind) -> RustExpr {
        RustExpr::synthetic(kind)
    }

    /// Helper: construct an ident expression.
    fn ident(name: &str) -> RustExpr {
        syn(RustExprKind::Ident(name.to_owned()))
    }

    /// Helper: construct an integer literal expression.
    fn int_lit(n: i64) -> RustExpr {
        syn(RustExprKind::IntLit(n))
    }

    /// Helper: construct a function with no params, no return type, and given body.
    fn simple_fn(name: &str, stmts: Vec<RustStmt>, expr: Option<RustExpr>) -> RustFile {
        RustFile {
            items: vec![RustItem::Function(RustFnDecl {
                name: name.to_owned(),
                params: vec![],
                return_type: None,
                body: RustBlock {
                    stmts,
                    expr: expr.map(Box::new),
                },
                span: None,
            })],
        }
    }

    // ---- Test 1: Emit empty function ----
    #[test]
    fn test_emit_empty_function_produces_fn_main() {
        let file = simple_fn("main", vec![], None);
        let output = emit(&file);
        assert_eq!(output, "fn main() {\n}\n");
    }

    // ---- Test 2: Emit function with params and return type ----
    #[test]
    fn test_emit_fn_with_params_and_return_type() {
        let file = RustFile {
            items: vec![RustItem::Function(RustFnDecl {
                name: "add".to_owned(),
                params: vec![
                    RustParam {
                        name: "a".to_owned(),
                        ty: RustType::I32,
                        span: None,
                    },
                    RustParam {
                        name: "b".to_owned(),
                        ty: RustType::I32,
                        span: None,
                    },
                ],
                return_type: Some(RustType::I32),
                body: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert_eq!(output, "fn add(a: i32, b: i32) -> i32 {\n}\n");
    }

    // ---- Test 3: Emit void function — no -> () ----
    #[test]
    fn test_emit_void_fn_omits_unit_return() {
        let file = simple_fn("greet", vec![], None);
        let output = emit(&file);
        assert!(!output.contains("-> ()"), "void fn should not show -> ()");
        assert!(output.starts_with("fn greet()"));
    }

    // ---- Test 4: Emit let x: i32 = 42; ----
    #[test]
    fn test_emit_let_binding_with_type() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "x".to_owned(),
                ty: Some(RustType::I32),
                init: int_lit(42),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("    let x: i32 = 42;\n"));
    }

    // ---- Test 5: Emit let mut x: i64 = 0; ----
    #[test]
    fn test_emit_let_mut_binding_with_type() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: true,
                name: "x".to_owned(),
                ty: Some(RustType::I64),
                init: int_lit(0),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("    let mut x: i64 = 0;\n"));
    }

    // ---- Test 6: Emit let x = 42; (no type) ----
    #[test]
    fn test_emit_let_binding_no_type() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "x".to_owned(),
                ty: None,
                init: int_lit(42),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("    let x = 42;\n"));
        // Ensure no colon (type annotation) present.
        assert!(!output.contains("let x:"));
    }

    // ---- Test 7: Emit if/else ----
    #[test]
    fn test_emit_if_else() {
        let file = simple_fn(
            "main",
            vec![RustStmt::If(RustIfStmt {
                condition: ident("condition"),
                then_block: RustBlock {
                    stmts: vec![RustStmt::Semi(int_lit(1))],
                    expr: None,
                },
                else_clause: Some(RustElse::Block(RustBlock {
                    stmts: vec![RustStmt::Semi(int_lit(2))],
                    expr: None,
                })),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        let expected = "\
fn main() {
    if condition {
        1;
    } else {
        2;
    }
}
";
        assert_eq!(output, expected);
    }

    // ---- Test 8: Emit if/else if/else chain ----
    #[test]
    fn test_emit_if_else_if_else_chain() {
        let file = simple_fn(
            "main",
            vec![RustStmt::If(RustIfStmt {
                condition: ident("a"),
                then_block: RustBlock {
                    stmts: vec![RustStmt::Semi(int_lit(1))],
                    expr: None,
                },
                else_clause: Some(RustElse::ElseIf(Box::new(RustIfStmt {
                    condition: ident("b"),
                    then_block: RustBlock {
                        stmts: vec![RustStmt::Semi(int_lit(2))],
                        expr: None,
                    },
                    else_clause: Some(RustElse::Block(RustBlock {
                        stmts: vec![RustStmt::Semi(int_lit(3))],
                        expr: None,
                    })),
                    span: None,
                }))),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("} else if b {"),
            "should have `}} else if` on same line"
        );
        assert!(
            output.contains("} else {"),
            "should have `}} else {{` on same line"
        );
    }

    // ---- Test 9: Emit while loop ----
    #[test]
    fn test_emit_while_loop() {
        let file = simple_fn(
            "main",
            vec![RustStmt::While(RustWhileStmt {
                condition: ident("running"),
                body: RustBlock {
                    stmts: vec![RustStmt::Semi(int_lit(1))],
                    expr: None,
                },
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        let expected = "\
fn main() {
    while running {
        1;
    }
}
";
        assert_eq!(output, expected);
    }

    // ---- Test 10: Emit binary expression a + b ----
    #[test]
    fn test_emit_binary_expr_add() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::Binary {
                op: RustBinaryOp::Add,
                left: Box::new(ident("a")),
                right: Box::new(ident("b")),
            }))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("a + b;"));
    }

    // ---- Test 11: Emit nested binary a + b * c (no unnecessary parens) ----
    #[test]
    fn test_emit_nested_binary_no_parens() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::Binary {
                op: RustBinaryOp::Add,
                left: Box::new(ident("a")),
                right: Box::new(syn(RustExprKind::Binary {
                    op: RustBinaryOp::Mul,
                    left: Box::new(ident("b")),
                    right: Box::new(ident("c")),
                })),
            }))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("a + b * c;"));
    }

    // ---- Test 12: Emit unary expressions ----
    #[test]
    fn test_emit_unary_neg_and_not() {
        let file = simple_fn(
            "main",
            vec![
                RustStmt::Semi(syn(RustExprKind::Unary {
                    op: RustUnaryOp::Neg,
                    operand: Box::new(ident("x")),
                })),
                RustStmt::Semi(syn(RustExprKind::Unary {
                    op: RustUnaryOp::Not,
                    operand: Box::new(ident("flag")),
                })),
            ],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("-x;"));
        assert!(output.contains("!flag;"));
    }

    // ---- Test 13: Emit function call ----
    #[test]
    fn test_emit_function_call() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::Call {
                func: "foo".to_owned(),
                args: vec![ident("a"), ident("b")],
            }))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("foo(a, b);"));
    }

    // ---- Test 14: Emit method call ----
    #[test]
    fn test_emit_method_call() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::MethodCall {
                receiver: Box::new(ident("receiver")),
                method: "method".to_owned(),
                args: vec![ident("a"), ident("b")],
            }))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("receiver.method(a, b);"));
    }

    // ---- Test 15: Emit println!("{}", x) ----
    #[test]
    fn test_emit_println_single_arg() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::Macro {
                name: "println".to_owned(),
                args: vec![syn(RustExprKind::StringLit("{}".to_owned())), ident("x")],
            }))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("println!(\"{}\", x);"), "got: {}", output);
    }

    // ---- Test 16: Emit println!("{} {}", x, y) ----
    #[test]
    fn test_emit_println_multi_arg() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::Macro {
                name: "println".to_owned(),
                args: vec![
                    syn(RustExprKind::StringLit("{} {}".to_owned())),
                    ident("x"),
                    ident("y"),
                ],
            }))],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("println!(\"{} {}\", x, y);"),
            "got: {}",
            output
        );
    }

    // ---- Test 17: Emit .clone() ----
    #[test]
    fn test_emit_clone() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::Clone(Box::new(ident(
                "x",
            )))))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("x.clone();"));
    }

    // ---- Test 18: Emit .to_string() ----
    #[test]
    fn test_emit_to_string() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::ToString(Box::new(
                ident("x"),
            ))))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("x.to_string();"));
    }

    // ---- Test 19: Emit return x; and return; ----
    #[test]
    fn test_emit_return_with_value() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Return(RustReturnStmt {
                value: Some(ident("x")),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("return x;"));
    }

    #[test]
    fn test_emit_return_bare() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Return(RustReturnStmt {
                value: None,
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("return;"));
    }

    // ---- Test 20: Emit string with escapes ----
    #[test]
    fn test_emit_string_with_escapes() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::StringLit(
                "hello\nworld".to_owned(),
            )))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains(r#""hello\nworld""#), "got: {}", output);
    }

    // ---- Test 21: Emit float 3.0 (not 3) ----
    #[test]
    fn test_emit_float_always_has_decimal() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::FloatLit(3.0)))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("3.0;"), "got: {}", output);
    }

    // ---- Test 22: Emit nested blocks — correct indentation ----
    #[test]
    fn test_emit_nested_blocks_indentation() {
        let file = simple_fn(
            "main",
            vec![RustStmt::If(RustIfStmt {
                condition: ident("a"),
                then_block: RustBlock {
                    stmts: vec![RustStmt::If(RustIfStmt {
                        condition: ident("b"),
                        then_block: RustBlock {
                            stmts: vec![RustStmt::Semi(int_lit(42))],
                            expr: None,
                        },
                        else_clause: None,
                        span: None,
                    })],
                    expr: None,
                },
                else_clause: None,
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        let expected = "\
fn main() {
    if a {
        if b {
            42;
        }
    }
}
";
        assert_eq!(output, expected);
    }

    // ---- Test 23: Emit multiple functions — separated by blank line ----
    #[test]
    fn test_emit_multiple_functions_blank_line_separator() {
        let file = RustFile {
            items: vec![
                RustItem::Function(RustFnDecl {
                    name: "foo".to_owned(),
                    params: vec![],
                    return_type: None,
                    body: RustBlock {
                        stmts: vec![],
                        expr: None,
                    },
                    span: None,
                }),
                RustItem::Function(RustFnDecl {
                    name: "bar".to_owned(),
                    params: vec![],
                    return_type: None,
                    body: RustBlock {
                        stmts: vec![],
                        expr: None,
                    },
                    span: None,
                }),
            ],
        };
        let output = emit(&file);
        let expected = "fn foo() {\n}\n\nfn bar() {\n}\n";
        assert_eq!(output, expected);
    }

    // ---- Test 24: Emit block with trailing expression (no semicolon) ----
    #[test]
    fn test_emit_block_trailing_expression_no_semicolon() {
        let file = RustFile {
            items: vec![RustItem::Function(RustFnDecl {
                name: "answer".to_owned(),
                params: vec![],
                return_type: Some(RustType::I32),
                body: RustBlock {
                    stmts: vec![],
                    expr: Some(Box::new(int_lit(42))),
                },
                span: None,
            })],
        };
        let output = emit(&file);
        let expected = "\
fn answer() -> i32 {
    42
}
";
        assert_eq!(output, expected);
    }

    // ---- Correctness Scenario 1: Fibonacci emission ----
    #[test]
    fn test_correctness_fibonacci_emission() {
        // fn fibonacci(n: i32) -> i32 {
        //     if n <= 1 {
        //         return n;
        //     }
        //     return fibonacci(n - 1) + fibonacci(n - 2);
        // }
        let file = RustFile {
            items: vec![RustItem::Function(RustFnDecl {
                name: "fibonacci".to_owned(),
                params: vec![RustParam {
                    name: "n".to_owned(),
                    ty: RustType::I32,
                    span: None,
                }],
                return_type: Some(RustType::I32),
                body: RustBlock {
                    stmts: vec![
                        RustStmt::If(RustIfStmt {
                            condition: syn(RustExprKind::Binary {
                                op: RustBinaryOp::Le,
                                left: Box::new(ident("n")),
                                right: Box::new(int_lit(1)),
                            }),
                            then_block: RustBlock {
                                stmts: vec![RustStmt::Return(RustReturnStmt {
                                    value: Some(ident("n")),
                                    span: None,
                                })],
                                expr: None,
                            },
                            else_clause: None,
                            span: None,
                        }),
                        RustStmt::Return(RustReturnStmt {
                            value: Some(syn(RustExprKind::Binary {
                                op: RustBinaryOp::Add,
                                left: Box::new(syn(RustExprKind::Call {
                                    func: "fibonacci".to_owned(),
                                    args: vec![syn(RustExprKind::Binary {
                                        op: RustBinaryOp::Sub,
                                        left: Box::new(ident("n")),
                                        right: Box::new(int_lit(1)),
                                    })],
                                })),
                                right: Box::new(syn(RustExprKind::Call {
                                    func: "fibonacci".to_owned(),
                                    args: vec![syn(RustExprKind::Binary {
                                        op: RustBinaryOp::Sub,
                                        left: Box::new(ident("n")),
                                        right: Box::new(int_lit(2)),
                                    })],
                                })),
                            })),
                            span: None,
                        }),
                    ],
                    expr: None,
                },
                span: None,
            })],
        };

        let output = emit(&file);
        let expected = "\
fn fibonacci(n: i32) -> i32 {
    if n <= 1 {
        return n;
    }
    return fibonacci(n - 1) + fibonacci(n - 2);
}
";
        assert_eq!(output, expected);
    }

    // ---- Correctness Scenario 2: Multi-statement function ----
    #[test]
    fn test_correctness_multi_statement_function() {
        // fn complex() {
        //     let x: i32 = 10;
        //     let mut y: i32 = 0;
        //     if x > 5 {
        //         y = x;
        //     } else {
        //         y = 0;
        //     }
        //     while y > 0 {
        //         println!("{}", y);
        //         y = y - 1;
        //     }
        //     return;
        // }
        let file = simple_fn(
            "complex",
            vec![
                RustStmt::Let(RustLetStmt {
                    mutable: false,
                    name: "x".to_owned(),
                    ty: Some(RustType::I32),
                    init: int_lit(10),
                    span: None,
                }),
                RustStmt::Let(RustLetStmt {
                    mutable: true,
                    name: "y".to_owned(),
                    ty: Some(RustType::I32),
                    init: int_lit(0),
                    span: None,
                }),
                RustStmt::If(RustIfStmt {
                    condition: syn(RustExprKind::Binary {
                        op: RustBinaryOp::Gt,
                        left: Box::new(ident("x")),
                        right: Box::new(int_lit(5)),
                    }),
                    then_block: RustBlock {
                        stmts: vec![RustStmt::Semi(syn(RustExprKind::Assign {
                            target: "y".to_owned(),
                            value: Box::new(ident("x")),
                        }))],
                        expr: None,
                    },
                    else_clause: Some(RustElse::Block(RustBlock {
                        stmts: vec![RustStmt::Semi(syn(RustExprKind::Assign {
                            target: "y".to_owned(),
                            value: Box::new(int_lit(0)),
                        }))],
                        expr: None,
                    })),
                    span: None,
                }),
                RustStmt::While(RustWhileStmt {
                    condition: syn(RustExprKind::Binary {
                        op: RustBinaryOp::Gt,
                        left: Box::new(ident("y")),
                        right: Box::new(int_lit(0)),
                    }),
                    body: RustBlock {
                        stmts: vec![
                            RustStmt::Semi(syn(RustExprKind::Macro {
                                name: "println".to_owned(),
                                args: vec![
                                    syn(RustExprKind::StringLit("{}".to_owned())),
                                    ident("y"),
                                ],
                            })),
                            RustStmt::Semi(syn(RustExprKind::Assign {
                                target: "y".to_owned(),
                                value: Box::new(syn(RustExprKind::Binary {
                                    op: RustBinaryOp::Sub,
                                    left: Box::new(ident("y")),
                                    right: Box::new(int_lit(1)),
                                })),
                            })),
                        ],
                        expr: None,
                    },
                    span: None,
                }),
                RustStmt::Return(RustReturnStmt {
                    value: None,
                    span: None,
                }),
            ],
            None,
        );

        let output = emit(&file);
        let expected = "\
fn complex() {
    let x: i32 = 10;
    let mut y: i32 = 0;
    if x > 5 {
        y = x;
    } else {
        y = 0;
    }
    while y > 0 {
        println!(\"{}\", y);
        y = y - 1;
    }
    return;
}
";
        assert_eq!(output, expected);
    }
}
