//! Internal emitter implementation.
//!
//! Walks the Rust IR tree and produces formatted `.rs` source text.

use rsc_syntax::rust_ir::{
    RustBlock, RustClosureBody, RustElse, RustEnumDef, RustExpr, RustExprKind, RustFile,
    RustFnDecl, RustForInStmt, RustIfLetStmt, RustIfStmt, RustItem, RustMatchResultStmt,
    RustMatchStmt, RustPattern, RustStmt, RustStructDef, RustTraitDef, RustTypeParam,
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
        // Emit use declarations first
        for use_decl in &file.uses {
            self.write("use ");
            self.write(&use_decl.path);
            self.writeln(";");
        }

        // Emit mod declarations
        for mod_decl in &file.mod_decls {
            if mod_decl.public {
                self.write("pub mod ");
            } else {
                self.write("mod ");
            }
            self.write(&mod_decl.name);
            self.writeln(";");
        }

        // Blank line between declarations and items if any declarations exist
        if !file.uses.is_empty() || !file.mod_decls.is_empty() {
            self.newline();
        }

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
            RustItem::Struct(s) => self.emit_struct(s),
            RustItem::Enum(e) => self.emit_enum(e),
            RustItem::Trait(t) => self.emit_trait(t),
        }
    }

    /// Emit a struct definition.
    fn emit_struct(&mut self, s: &RustStructDef) {
        self.write_indent();
        self.write("struct ");
        self.write(&s.name);
        self.emit_type_params(&s.type_params);
        self.writeln(" {");
        self.push_indent();

        for field in &s.fields {
            self.write_indent();
            if field.public {
                self.write("pub ");
            }
            self.write(&field.name);
            self.write(": ");
            self.write(&field.ty.to_string());
            self.writeln(",");
        }

        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    /// Emit an enum definition.
    fn emit_enum(&mut self, e: &RustEnumDef) {
        self.write_indent();
        self.write("enum ");
        self.write(&e.name);
        self.writeln(" {");
        self.push_indent();

        for variant in &e.variants {
            self.write_indent();
            self.write(&variant.name);
            if variant.fields.is_empty() {
                self.writeln(",");
            } else {
                self.writeln(" {");
                self.push_indent();
                for field in &variant.fields {
                    self.write_indent();
                    if field.public {
                        self.write("pub ");
                    }
                    self.write(&field.name);
                    self.write(": ");
                    self.write(&field.ty.to_string());
                    self.writeln(",");
                }
                self.pop_indent();
                self.write_indent();
                self.writeln("},");
            }
        }

        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    /// Emit a trait definition.
    fn emit_trait(&mut self, t: &RustTraitDef) {
        self.write_indent();
        self.write("trait ");
        self.write(&t.name);
        self.emit_type_params(&t.type_params);
        self.writeln(" {");
        self.push_indent();

        for method in &t.methods {
            self.write_indent();
            self.write("fn ");
            self.write(&method.name);
            self.write("(");

            // Emit &self as the first parameter if applicable
            if method.has_self {
                self.write("&self");
                if !method.params.is_empty() {
                    self.write(", ");
                }
            }

            for (i, param) in method.params.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&param.name);
                self.write(": ");
                self.write(&param.ty.to_string());
            }

            self.write(")");

            if let Some(ref ret) = method.return_type {
                self.write(" -> ");
                self.write(&ret.to_string());
            }

            self.writeln(";");
        }

        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    /// Emit a match statement.
    fn emit_match(&mut self, m: &RustMatchStmt) {
        self.write("match ");
        self.emit_expr(&m.scrutinee);
        self.writeln(" {");
        self.push_indent();

        for arm in &m.arms {
            self.write_indent();
            self.emit_pattern(&arm.pattern);
            self.write(" => ");
            self.emit_block(&arm.body);
            self.newline();
        }

        self.pop_indent();
        self.write_indent();
        self.write("}");
    }

    /// Emit a match pattern.
    fn emit_pattern(&mut self, pattern: &RustPattern) {
        match pattern {
            RustPattern::EnumVariant(enum_name, variant_name) => {
                self.write(enum_name);
                self.write("::");
                self.write(variant_name);
            }
            RustPattern::EnumVariantFields(enum_name, variant_name, fields) => {
                self.write(enum_name);
                self.write("::");
                self.write(variant_name);
                self.write(" { ");
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(field);
                }
                self.write(" }");
            }
        }
    }

    /// Emit a function declaration.
    fn emit_fn(&mut self, f: &RustFnDecl) {
        self.write_indent();
        self.write("fn ");
        self.write(&f.name);
        self.emit_type_params(&f.type_params);
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

    /// Emit generic type parameters: `<T: Bound, U>`.
    ///
    /// Emits nothing if the type parameter list is empty.
    fn emit_type_params(&mut self, type_params: &[RustTypeParam]) {
        if type_params.is_empty() {
            return;
        }
        self.write("<");
        for (i, param) in type_params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&param.name);
            if !param.bounds.is_empty() {
                self.write(": ");
                for (j, bound) in param.bounds.iter().enumerate() {
                    if j > 0 {
                        self.write(" + ");
                    }
                    self.write(bound);
                }
            }
        }
        self.write(">");
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
            RustStmt::Destructure(destr) => {
                self.write_indent();
                if destr.mutable {
                    self.write("let mut ");
                } else {
                    self.write("let ");
                }
                self.write(&destr.type_name);
                self.write(" { ");
                for (i, field) in destr.fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(field);
                }
                self.write(", .. } = ");
                self.emit_expr(&destr.init);
                self.writeln(";");
            }
            RustStmt::Match(match_stmt) => {
                self.write_indent();
                self.emit_match(match_stmt);
                self.newline();
            }
            RustStmt::IfLet(if_let) => {
                self.write_indent();
                self.emit_if_let(if_let);
                self.newline();
            }
            RustStmt::MatchResult(match_result) => {
                self.write_indent();
                self.emit_match_result(match_result);
                self.newline();
            }
            RustStmt::ForIn(for_in) => {
                self.write_indent();
                self.emit_for_in(for_in);
                self.newline();
            }
            RustStmt::Break(_) => {
                self.write_indent();
                self.writeln("break;");
            }
            RustStmt::Continue(_) => {
                self.write_indent();
                self.writeln("continue;");
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

    /// Emit an `if let Some(name) = expr { ... } [else { ... }]`.
    fn emit_if_let(&mut self, if_let: &RustIfLetStmt) {
        self.write("if let Some(");
        self.write(&if_let.binding);
        self.write(") = ");
        self.emit_expr(&if_let.expr);
        self.write(" ");
        self.emit_block(&if_let.then_block);

        if let Some(ref else_block) = if_let.else_block {
            self.write(" else ");
            self.emit_block(else_block);
        }
    }

    /// Emit a `match` on `Result` for try/catch lowering.
    fn emit_match_result(&mut self, m: &RustMatchResultStmt) {
        self.write("match ");
        self.emit_expr(&m.expr);
        self.writeln(" {");
        self.push_indent();

        // Ok arm
        self.write_indent();
        self.write("Ok(");
        self.write(&m.ok_binding);
        self.write(") => ");
        self.emit_block(&m.ok_block);
        self.newline();

        // Err arm
        self.write_indent();
        self.write("Err(");
        self.write(&m.err_binding);
        self.write(") => ");
        self.emit_block(&m.err_block);
        self.newline();

        self.pop_indent();
        self.write_indent();
        self.write("}");
    }

    /// Emit a for-in loop: `for variable in &iterable { body }`.
    fn emit_for_in(&mut self, for_in: &RustForInStmt) {
        self.write("for ");
        self.write(&for_in.variable);
        self.write(" in &");
        self.emit_expr(&for_in.iterable);
        self.write(" ");
        self.emit_block(&for_in.body);
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
            RustExprKind::CompoundAssign { target, op, value } => {
                self.write(target);
                self.write(" ");
                self.write(&op.to_string());
                self.write(" ");
                self.emit_expr(value);
            }
            RustExprKind::StructLit { type_name, fields } => {
                self.write(type_name);
                self.write(" { ");
                for (i, (name, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(name);
                    self.write(": ");
                    self.emit_expr(value);
                }
                self.write(" }");
            }
            RustExprKind::FieldAccess { object, field } => {
                self.emit_expr(object);
                self.write(".");
                self.write(field);
            }
            RustExprKind::EnumVariant {
                enum_name,
                variant_name,
            } => {
                self.write(enum_name);
                self.write("::");
                self.write(variant_name);
            }
            RustExprKind::VecLit(elements) => {
                self.write("vec![");
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_expr(elem);
                }
                self.write("]");
            }
            RustExprKind::StaticCall {
                type_name,
                method,
                args,
            } => {
                self.write(type_name);
                self.write("::");
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
            RustExprKind::Index { object, index } => {
                self.emit_expr(object);
                self.write("[");
                self.emit_expr(index);
                self.write("]");
            }
            RustExprKind::None => {
                self.write("None");
            }
            RustExprKind::Some(inner) => {
                self.write("Some(");
                self.emit_expr(inner);
                self.write(")");
            }
            RustExprKind::UnwrapOr { expr, default } => {
                self.emit_expr(expr);
                self.write(".unwrap_or(");
                self.emit_expr(default);
                self.write(")");
            }
            RustExprKind::QuestionMark(inner) => {
                self.emit_expr(inner);
                self.write("?");
            }
            RustExprKind::Ok(inner) => {
                self.write("Ok(");
                self.emit_expr(inner);
                self.write(")");
            }
            RustExprKind::Err(inner) => {
                self.write("Err(");
                self.emit_expr(inner);
                self.write(")");
            }
            RustExprKind::ClosureCall { body, return_type } => {
                self.write("(|| -> ");
                self.write(&return_type.to_string());
                self.write(" ");
                self.emit_block(body);
                self.write(")()");
            }
            RustExprKind::OptionMap {
                expr,
                closure_param,
                closure_body,
            } => {
                self.emit_expr(expr);
                self.write(".map(|");
                self.write(closure_param);
                self.write("| ");
                self.emit_expr(closure_body);
                self.write(")");
            }
            RustExprKind::Closure {
                is_move,
                params,
                return_type,
                body,
            } => {
                if *is_move {
                    self.write("move ");
                }
                self.write("|");
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&param.name);
                    if let Some(ref ty) = param.ty {
                        self.write(": ");
                        self.write(&ty.to_string());
                    }
                }
                self.write("|");
                if let Some(ret) = return_type {
                    self.write(" -> ");
                    self.write(&ret.to_string());
                }
                match body {
                    RustClosureBody::Expr(expr) => {
                        self.write(" ");
                        self.emit_expr(expr);
                    }
                    RustClosureBody::Block(block) => {
                        self.write(" ");
                        self.emit_block(block);
                    }
                }
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
        RustBinaryOp, RustBlock, RustDestructureStmt, RustElse, RustEnumDef, RustEnumVariant,
        RustExpr, RustExprKind, RustFieldDef, RustFile, RustFnDecl, RustForInStmt, RustIfLetStmt,
        RustIfStmt, RustItem, RustLetStmt, RustMatchArm, RustMatchResultStmt, RustMatchStmt,
        RustModDecl, RustParam, RustPattern, RustReturnStmt, RustStmt, RustStructDef, RustTraitDef,
        RustTraitMethod, RustType, RustTypeParam, RustUnaryOp, RustUseDecl, RustWhileStmt,
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
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: name.to_owned(),
                type_params: vec![],
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
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "add".to_owned(),
                type_params: vec![],
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
            uses: vec![],
            mod_decls: vec![],
            items: vec![
                RustItem::Function(RustFnDecl {
                    name: "foo".to_owned(),
                    type_params: vec![],
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
                    type_params: vec![],
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
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "answer".to_owned(),
                type_params: vec![],
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
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "fibonacci".to_owned(),
                type_params: vec![],
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

    // ---- Test 25: Emit use declarations before items ----
    #[test]
    fn test_emit_use_decls_before_items() {
        let file = RustFile {
            uses: vec![RustUseDecl {
                path: "std::collections::HashMap".to_owned(),
                span: None,
            }],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "main".to_owned(),
                type_params: vec![],
                params: vec![],
                return_type: None,
                body: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        let expected = "\
use std::collections::HashMap;

fn main() {
}
";
        assert_eq!(output, expected);
    }

    // ---- Test 26: Emit mod declarations ----
    #[test]
    fn test_emit_mod_decls_before_items() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![
                RustModDecl {
                    name: "utils".to_owned(),
                    public: false,
                    span: None,
                },
                RustModDecl {
                    name: "api".to_owned(),
                    public: true,
                    span: None,
                },
            ],
            items: vec![RustItem::Function(RustFnDecl {
                name: "main".to_owned(),
                type_params: vec![],
                params: vec![],
                return_type: None,
                body: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        let expected = "\
mod utils;
pub mod api;

fn main() {
}
";
        assert_eq!(output, expected);
    }

    // ---- Test 27: Empty uses/mod_decls produce no extra blank line ----
    #[test]
    fn test_emit_empty_uses_and_mods_no_extra_blank_line() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "main".to_owned(),
                type_params: vec![],
                params: vec![],
                return_type: None,
                body: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert_eq!(output, "fn main() {\n}\n");
    }

    // ---- Test: Emit CompoundAssign ----
    #[test]
    fn test_emit_compound_assign_add() {
        use rsc_syntax::rust_ir::RustCompoundAssignOp;
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::CompoundAssign {
                target: "x".to_owned(),
                op: RustCompoundAssignOp::AddAssign,
                value: Box::new(int_lit(1)),
            }))],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("x += 1;"),
            "expected `x += 1;` in output, got: {output}"
        );
    }

    #[test]
    fn test_emit_compound_assign_all_operators() {
        use rsc_syntax::rust_ir::RustCompoundAssignOp;
        let cases = [
            (RustCompoundAssignOp::AddAssign, "x += 1;"),
            (RustCompoundAssignOp::SubAssign, "x -= 1;"),
            (RustCompoundAssignOp::MulAssign, "x *= 1;"),
            (RustCompoundAssignOp::DivAssign, "x /= 1;"),
            (RustCompoundAssignOp::RemAssign, "x %= 1;"),
        ];

        for (op, expected) in cases {
            let file = simple_fn(
                "main",
                vec![RustStmt::Semi(syn(RustExprKind::CompoundAssign {
                    target: "x".to_owned(),
                    op,
                    value: Box::new(int_lit(1)),
                }))],
                None,
            );
            let output = emit(&file);
            assert!(
                output.contains(expected),
                "expected `{expected}` in output for {op:?}, got: {output}"
            );
        }
    }

    // ---------------------------------------------------------------
    // Task 014: Struct emission
    // ---------------------------------------------------------------

    // Test T14-10: Emit struct definition
    #[test]
    fn test_emit_struct_definition_matches_snapshot() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Struct(RustStructDef {
                name: "User".to_owned(),
                type_params: vec![],
                fields: vec![
                    RustFieldDef {
                        public: true,
                        name: "name".to_owned(),
                        ty: RustType::String,
                        span: None,
                    },
                    RustFieldDef {
                        public: true,
                        name: "age".to_owned(),
                        ty: RustType::U32,
                        span: None,
                    },
                ],
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(output.contains("struct User {"), "output: {output}");
        assert!(output.contains("pub name: String,"), "output: {output}");
        assert!(output.contains("pub age: u32,"), "output: {output}");
    }

    // Test T14-11: Emit struct literal
    #[test]
    fn test_emit_struct_literal_expression() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "p".to_owned(),
                ty: None,
                init: syn(RustExprKind::StructLit {
                    type_name: "Point".to_owned(),
                    fields: vec![
                        ("x".to_owned(), syn(RustExprKind::FloatLit(1.0))),
                        ("y".to_owned(), syn(RustExprKind::FloatLit(2.0))),
                    ],
                }),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("Point { x: 1.0, y: 2.0 }"),
            "output: {output}"
        );
    }

    // Test T14-12: Emit field access
    #[test]
    fn test_emit_field_access_expression() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::FieldAccess {
                object: Box::new(ident("user")),
                field: "name".to_owned(),
            }))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("user.name"), "output: {output}");
    }

    // Test T14-13: Emit destructuring
    #[test]
    fn test_emit_destructuring_let() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Destructure(RustDestructureStmt {
                type_name: "User".to_owned(),
                fields: vec!["name".to_owned(), "age".to_owned()],
                init: ident("user"),
                mutable: false,
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("let User { name, age, .. } = user;"),
            "output: {output}"
        );
    }

    // Test T14-14: Emit RustType::Named
    #[test]
    fn test_emit_named_type_display() {
        assert_eq!(RustType::Named("User".to_owned()).to_string(), "User");
        assert_eq!(RustType::Named("Point".to_owned()).to_string(), "Point");
    }

    // ---- Task 016: Generics emission ----

    // Test T16-9: Emit `fn id<T>(x: T) -> T { ... }`
    #[test]
    fn test_emit_generic_fn_single_type_param() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "id".to_owned(),
                type_params: vec![RustTypeParam {
                    name: "T".to_owned(),
                    bounds: vec![],
                }],
                params: vec![RustParam {
                    name: "x".to_owned(),
                    ty: RustType::TypeParam("T".to_owned()),
                    span: None,
                }],
                return_type: Some(RustType::TypeParam("T".to_owned())),
                body: RustBlock {
                    stmts: vec![RustStmt::Return(RustReturnStmt {
                        value: Some(ident("x")),
                        span: None,
                    })],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert_eq!(output, "fn id<T>(x: T) -> T {\n    return x;\n}\n");
    }

    // Test T16-10: Emit `fn merge<T: Comparable>(a: T, b: T) -> T { ... }`
    #[test]
    fn test_emit_generic_fn_with_bound() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "merge".to_owned(),
                type_params: vec![RustTypeParam {
                    name: "T".to_owned(),
                    bounds: vec!["Comparable".to_owned()],
                }],
                params: vec![
                    RustParam {
                        name: "a".to_owned(),
                        ty: RustType::TypeParam("T".to_owned()),
                        span: None,
                    },
                    RustParam {
                        name: "b".to_owned(),
                        ty: RustType::TypeParam("T".to_owned()),
                        span: None,
                    },
                ],
                return_type: Some(RustType::TypeParam("T".to_owned())),
                body: RustBlock {
                    stmts: vec![RustStmt::Return(RustReturnStmt {
                        value: Some(ident("a")),
                        span: None,
                    })],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert_eq!(
            output,
            "fn merge<T: Comparable>(a: T, b: T) -> T {\n    return a;\n}\n"
        );
    }

    // Test T16-11: Emit `struct Container<T> { pub value: T, }`
    #[test]
    fn test_emit_generic_struct() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Struct(RustStructDef {
                name: "Container".to_owned(),
                type_params: vec![RustTypeParam {
                    name: "T".to_owned(),
                    bounds: vec![],
                }],
                fields: vec![RustFieldDef {
                    public: true,
                    name: "value".to_owned(),
                    ty: RustType::TypeParam("T".to_owned()),
                    span: None,
                }],
                span: None,
            })],
        };
        let output = emit(&file);
        assert_eq!(output, "struct Container<T> {\n    pub value: T,\n}\n");
    }

    // Test T16-12: Emit `Vec<String>` for generic type
    #[test]
    fn test_emit_generic_type_display() {
        let ty = RustType::Generic(
            Box::new(RustType::Named("Vec".to_owned())),
            vec![RustType::String],
        );
        assert_eq!(ty.to_string(), "Vec<String>");
    }

    // Test T16-13: Emit `TypeParam("T")` as `T`
    #[test]
    fn test_emit_type_param_display() {
        assert_eq!(RustType::TypeParam("T".to_owned()).to_string(), "T");
    }

    // ---- Task 015: Enum and Match emission tests ----

    // Test T015-8: Emit simple enum
    #[test]
    fn test_emit_simple_enum_fieldless_variants() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Enum(RustEnumDef {
                name: "Direction".to_owned(),
                variants: vec![
                    RustEnumVariant {
                        name: "North".to_owned(),
                        fields: vec![],
                        span: None,
                    },
                    RustEnumVariant {
                        name: "South".to_owned(),
                        fields: vec![],
                        span: None,
                    },
                    RustEnumVariant {
                        name: "East".to_owned(),
                        fields: vec![],
                        span: None,
                    },
                    RustEnumVariant {
                        name: "West".to_owned(),
                        fields: vec![],
                        span: None,
                    },
                ],
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(output.contains("enum Direction {"));
        assert!(output.contains("    North,"));
        assert!(output.contains("    South,"));
        assert!(output.contains("    East,"));
        assert!(output.contains("    West,"));
    }

    // Test T015-9: Emit data enum with struct variants
    #[test]
    fn test_emit_data_enum_struct_variants() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Enum(RustEnumDef {
                name: "Shape".to_owned(),
                variants: vec![
                    RustEnumVariant {
                        name: "Circle".to_owned(),
                        fields: vec![RustFieldDef {
                            public: true,
                            name: "radius".to_owned(),
                            ty: RustType::F64,
                            span: None,
                        }],
                        span: None,
                    },
                    RustEnumVariant {
                        name: "Rect".to_owned(),
                        fields: vec![
                            RustFieldDef {
                                public: true,
                                name: "width".to_owned(),
                                ty: RustType::F64,
                                span: None,
                            },
                            RustFieldDef {
                                public: true,
                                name: "height".to_owned(),
                                ty: RustType::F64,
                                span: None,
                            },
                        ],
                        span: None,
                    },
                ],
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(output.contains("enum Shape {"));
        assert!(output.contains("    Circle {"));
        assert!(output.contains("        pub radius: f64,"));
        assert!(output.contains("    Rect {"));
        assert!(output.contains("        pub width: f64,"));
        assert!(output.contains("        pub height: f64,"));
    }

    // Test T015-10: Emit match on simple enum
    #[test]
    fn test_emit_match_simple_enum_variant_patterns() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "test".to_owned(),
                type_params: vec![],
                params: vec![],
                return_type: None,
                body: RustBlock {
                    stmts: vec![RustStmt::Match(RustMatchStmt {
                        scrutinee: ident("dir"),
                        arms: vec![
                            RustMatchArm {
                                pattern: RustPattern::EnumVariant(
                                    "Direction".to_owned(),
                                    "North".to_owned(),
                                ),
                                body: RustBlock {
                                    stmts: vec![RustStmt::Semi(int_lit(1))],
                                    expr: None,
                                },
                            },
                            RustMatchArm {
                                pattern: RustPattern::EnumVariant(
                                    "Direction".to_owned(),
                                    "South".to_owned(),
                                ),
                                body: RustBlock {
                                    stmts: vec![RustStmt::Semi(int_lit(2))],
                                    expr: None,
                                },
                            },
                        ],
                        span: None,
                    })],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(output.contains("match dir {"));
        assert!(output.contains("Direction::North => {"));
        assert!(output.contains("Direction::South => {"));
    }

    // Test T015-11: Emit match on data enum with field binding
    #[test]
    fn test_emit_match_data_enum_field_destructuring() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "area".to_owned(),
                type_params: vec![],
                params: vec![],
                return_type: None,
                body: RustBlock {
                    stmts: vec![RustStmt::Match(RustMatchStmt {
                        scrutinee: ident("shape"),
                        arms: vec![
                            RustMatchArm {
                                pattern: RustPattern::EnumVariantFields(
                                    "Shape".to_owned(),
                                    "Circle".to_owned(),
                                    vec!["radius".to_owned()],
                                ),
                                body: RustBlock {
                                    stmts: vec![RustStmt::Semi(ident("radius"))],
                                    expr: None,
                                },
                            },
                            RustMatchArm {
                                pattern: RustPattern::EnumVariantFields(
                                    "Shape".to_owned(),
                                    "Rect".to_owned(),
                                    vec!["width".to_owned(), "height".to_owned()],
                                ),
                                body: RustBlock {
                                    stmts: vec![RustStmt::Semi(ident("width"))],
                                    expr: None,
                                },
                            },
                        ],
                        span: None,
                    })],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(output.contains("Shape::Circle { radius }"));
        assert!(output.contains("Shape::Rect { width, height }"));
    }

    // Test T015-emit: Emit EnumVariant expression
    #[test]
    fn test_emit_enum_variant_expression() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "dir".to_owned(),
                ty: None,
                init: syn(RustExprKind::EnumVariant {
                    enum_name: "Direction".to_owned(),
                    variant_name: "North".to_owned(),
                }),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("let dir = Direction::North;"));
    }

    // ---------------------------------------------------------------
    // Task 017: Collection emission
    // ---------------------------------------------------------------

    // Test T17-11: Emit `vec![1, 2, 3]`
    #[test]
    fn test_emit_vec_lit_three_elements() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "nums".to_owned(),
                ty: None,
                init: syn(RustExprKind::VecLit(vec![
                    int_lit(1),
                    int_lit(2),
                    int_lit(3),
                ])),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("vec![1, 2, 3]"), "output: {output}");
    }

    // Test T17-12: Emit `HashMap::new()`
    #[test]
    fn test_emit_hashmap_new_static_call() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "lookup".to_owned(),
                ty: None,
                init: syn(RustExprKind::StaticCall {
                    type_name: "HashMap".to_owned(),
                    method: "new".to_owned(),
                    args: vec![],
                }),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("HashMap::new()"), "output: {output}");
    }

    // Test T17-13: Emit `expr[0]`
    #[test]
    fn test_emit_index_access() {
        let file = simple_fn(
            "main",
            vec![RustStmt::Semi(syn(RustExprKind::Index {
                object: Box::new(ident("arr")),
                index: Box::new(int_lit(0)),
            }))],
            None,
        );
        let output = emit(&file);
        assert!(output.contains("arr[0]"), "output: {output}");
    }

    // Test T17-14: Emit `use std::collections::HashMap;`
    #[test]
    fn test_emit_use_declaration() {
        let file = RustFile {
            uses: vec![RustUseDecl {
                path: "std::collections::HashMap".to_owned(),
                span: None,
            }],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "main".to_owned(),
                type_params: vec![],
                params: vec![],
                return_type: None,
                body: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(
            output.contains("use std::collections::HashMap;"),
            "output: {output}"
        );
    }

    // ---- Task 020: Option/null emitter tests ----

    // Test 12: Emit `Option<String>` for `string | null`
    #[test]
    fn test_emit_option_string_type() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "find".to_owned(),
                type_params: vec![],
                params: vec![],
                return_type: Some(RustType::Option(Box::new(RustType::String))),
                body: RustBlock {
                    stmts: vec![RustStmt::Return(RustReturnStmt {
                        value: Some(syn(RustExprKind::None)),
                        span: None,
                    })],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(
            output.contains("-> Option<String>"),
            "expected -> Option<String>, got:\n{output}"
        );
    }

    // Test 13: Emit `None` for null
    #[test]
    fn test_emit_none() {
        let file = simple_fn(
            "test",
            vec![RustStmt::Return(RustReturnStmt {
                value: Some(syn(RustExprKind::None)),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("return None;"),
            "expected 'return None;' in output:\n{output}"
        );
    }

    // Test 14: Emit `Some(value)` for wrapping
    #[test]
    fn test_emit_some() {
        let file = simple_fn(
            "test",
            vec![RustStmt::Return(RustReturnStmt {
                value: Some(syn(RustExprKind::Some(Box::new(syn(
                    RustExprKind::StringLit("hello".to_owned()),
                ))))),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("return Some(\"hello\");"),
            "expected 'return Some(\"hello\");' in output:\n{output}"
        );
    }

    // Test 15: Emit `if let Some(x) = expr { ... }`
    #[test]
    fn test_emit_if_let_some() {
        let file = simple_fn(
            "test",
            vec![RustStmt::IfLet(RustIfLetStmt {
                binding: "name".to_owned(),
                expr: ident("value"),
                then_block: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                else_block: None,
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("if let Some(name) = value"),
            "expected 'if let Some(name) = value' in output:\n{output}"
        );
    }

    // --- Task 021: Result, ?, Ok, Err, MatchResult ---

    // Emit Result<T, E> in function return type
    #[test]
    fn test_emit_result_return_type_produces_result_syntax() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "fetch".to_owned(),
                type_params: vec![],
                params: vec![],
                return_type: Some(RustType::Result(
                    Box::new(RustType::I32),
                    Box::new(RustType::String),
                )),
                body: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(
            output.contains("-> Result<i32, String>"),
            "expected Result<i32, String> in output:\n{output}"
        );
    }

    // Emit expr?
    #[test]
    fn test_emit_question_mark_produces_question_mark_syntax() {
        let file = simple_fn(
            "test",
            vec![RustStmt::Semi(syn(RustExprKind::QuestionMark(Box::new(
                syn(RustExprKind::Call {
                    func: "fetch".to_owned(),
                    args: vec![],
                }),
            ))))],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("fetch()?"),
            "expected 'fetch()?' in output:\n{output}"
        );
    }

    // Emit Ok(expr)
    #[test]
    fn test_emit_ok_produces_ok_syntax() {
        let file = simple_fn(
            "test",
            vec![RustStmt::Return(RustReturnStmt {
                value: Some(syn(RustExprKind::Ok(Box::new(int_lit(42))))),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("return Ok(42)"),
            "expected 'return Ok(42)' in output:\n{output}"
        );
    }

    // Emit Err(expr)
    #[test]
    fn test_emit_err_produces_err_syntax() {
        let file = simple_fn(
            "test",
            vec![RustStmt::Return(RustReturnStmt {
                value: Some(syn(RustExprKind::Err(Box::new(syn(
                    RustExprKind::StringLit("oops".to_owned()),
                ))))),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("return Err(\"oops\")"),
            "expected 'return Err(\"oops\")' in output:\n{output}"
        );
    }

    // Emit match Result for try/catch
    #[test]
    fn test_emit_match_result_produces_match_ok_err() {
        let file = simple_fn(
            "test",
            vec![RustStmt::MatchResult(RustMatchResultStmt {
                expr: syn(RustExprKind::Call {
                    func: "fetch".to_owned(),
                    args: vec![],
                }),
                ok_binding: "val".to_owned(),
                ok_block: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                err_binding: "err".to_owned(),
                err_block: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("match fetch()"),
            "expected 'match fetch()' in output:\n{output}"
        );
        assert!(
            output.contains("Ok(val) =>"),
            "expected 'Ok(val) =>' in output:\n{output}"
        );
        assert!(
            output.contains("Err(err) =>"),
            "expected 'Err(err) =>' in output:\n{output}"
        );
    }

    // ---------------------------------------------------------------
    // Task 019: Closures and arrow functions
    // ---------------------------------------------------------------

    // Test T19-8: Emit expression-body closure: `|x: i32| -> i32 x * 2`
    #[test]
    fn test_emit_closure_expr_body() {
        use rsc_syntax::rust_ir::{RustClosureBody, RustClosureParam};
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "double".to_owned(),
                ty: None,
                init: syn(RustExprKind::Closure {
                    is_move: false,
                    params: vec![RustClosureParam {
                        name: "x".to_owned(),
                        ty: Some(RustType::I32),
                    }],
                    return_type: Some(RustType::I32),
                    body: RustClosureBody::Expr(Box::new(syn(RustExprKind::Binary {
                        op: RustBinaryOp::Mul,
                        left: Box::new(ident("x")),
                        right: Box::new(int_lit(2)),
                    }))),
                }),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("|x: i32| -> i32 x * 2"),
            "expected closure in output:\n{output}"
        );
    }

    // Test T19-9: Emit block-body closure: `|| { ... }`
    #[test]
    fn test_emit_closure_block_body() {
        use rsc_syntax::rust_ir::RustClosureBody;
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "greet".to_owned(),
                ty: None,
                init: syn(RustExprKind::Closure {
                    is_move: false,
                    params: vec![],
                    return_type: None,
                    body: RustClosureBody::Block(RustBlock {
                        stmts: vec![RustStmt::Semi(syn(RustExprKind::Macro {
                            name: "println".to_owned(),
                            args: vec![syn(RustExprKind::StringLit("hello".to_owned()))],
                        }))],
                        expr: None,
                    }),
                }),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("|| {"),
            "expected closure in output:\n{output}"
        );
    }

    // Test T19-10: Emit move closure: `move || { ... }`
    #[test]
    fn test_emit_closure_move() {
        use rsc_syntax::rust_ir::RustClosureBody;
        let file = simple_fn(
            "main",
            vec![RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "handler".to_owned(),
                ty: None,
                init: syn(RustExprKind::Closure {
                    is_move: true,
                    params: vec![],
                    return_type: None,
                    body: RustClosureBody::Block(RustBlock {
                        stmts: vec![RustStmt::Semi(syn(RustExprKind::Call {
                            func: "process".to_owned(),
                            args: vec![ident("ctx")],
                        }))],
                        expr: None,
                    }),
                }),
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("move || {"),
            "expected move closure in output:\n{output}"
        );
    }

    // Test T19-11: Emit impl Fn type
    #[test]
    fn test_emit_impl_fn_type_in_param() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "apply".to_owned(),
                type_params: vec![],
                params: vec![
                    RustParam {
                        name: "x".to_owned(),
                        ty: RustType::I32,
                        span: None,
                    },
                    RustParam {
                        name: "f".to_owned(),
                        ty: RustType::ImplFn(vec![RustType::I32], Box::new(RustType::I32)),
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
        assert!(
            output.contains("f: impl Fn(i32) -> i32"),
            "expected impl Fn in output:\n{output}"
        );
    }

    // ---- Task 022: Trait emission tests ----

    #[test]
    fn test_emit_trait_definition_with_self_and_return() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Trait(RustTraitDef {
                name: "Serializable".to_owned(),
                type_params: vec![],
                methods: vec![RustTraitMethod {
                    name: "serialize".to_owned(),
                    params: vec![],
                    return_type: Some(RustType::String),
                    has_self: true,
                    span: None,
                }],
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(
            output.contains("trait Serializable {"),
            "expected trait definition in output:\n{output}"
        );
        assert!(
            output.contains("fn serialize(&self) -> String;"),
            "expected method with &self in output:\n{output}"
        );
    }

    #[test]
    fn test_emit_fn_with_generic_trait_bounds() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Function(RustFnDecl {
                name: "process".to_owned(),
                type_params: vec![RustTypeParam {
                    name: "T".to_owned(),
                    bounds: vec!["Serializable".to_owned(), "Printable".to_owned()],
                }],
                params: vec![RustParam {
                    name: "input".to_owned(),
                    ty: RustType::TypeParam("T".to_owned()),
                    span: None,
                }],
                return_type: Some(RustType::String),
                body: RustBlock {
                    stmts: vec![],
                    expr: None,
                },
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(
            output.contains("fn process<T: Serializable + Printable>(input: T) -> String"),
            "expected generic fn with trait bounds in output:\n{output}"
        );
    }

    #[test]
    fn test_emit_trait_self_return_type() {
        let file = RustFile {
            uses: vec![],
            mod_decls: vec![],
            items: vec![RustItem::Trait(RustTraitDef {
                name: "Cloneable".to_owned(),
                type_params: vec![],
                methods: vec![RustTraitMethod {
                    name: "clone".to_owned(),
                    params: vec![],
                    return_type: Some(RustType::SelfType),
                    has_self: true,
                    span: None,
                }],
                span: None,
            })],
        };
        let output = emit(&file);
        assert!(
            output.contains("fn clone(&self) -> Self;"),
            "expected Self return type in output:\n{output}"
        );
    }

    // ---------------------------------------------------------------
    // Task 018: For-of loops, break, continue emission
    // ---------------------------------------------------------------

    // T018-8: Emit `for x in &items { ... }`
    #[test]
    fn test_emit_for_in_with_borrow() {
        let file = simple_fn(
            "main",
            vec![RustStmt::ForIn(RustForInStmt {
                variable: "x".to_owned(),
                iterable: ident("items"),
                body: RustBlock {
                    stmts: vec![RustStmt::Semi(syn(RustExprKind::Macro {
                        name: "println".to_owned(),
                        args: vec![ident("x")],
                    }))],
                    expr: None,
                },
                span: None,
            })],
            None,
        );
        let output = emit(&file);
        assert!(
            output.contains("for x in &items"),
            "expected `for x in &items` in output:\n{output}"
        );
    }

    // T018-9: Emit `break;`
    #[test]
    fn test_emit_break_statement() {
        let file = simple_fn("main", vec![RustStmt::Break(None)], None);
        let output = emit(&file);
        assert!(
            output.contains("break;"),
            "expected `break;` in output:\n{output}"
        );
    }

    // T018-10: Emit `continue;`
    #[test]
    fn test_emit_continue_statement() {
        let file = simple_fn("main", vec![RustStmt::Continue(None)], None);
        let output = emit(&file);
        assert!(
            output.contains("continue;"),
            "expected `continue;` in output:\n{output}"
        );
    }
}
