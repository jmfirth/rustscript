//! Recursive descent parser for `RustScript` source files.
//!
//! Consumes the token stream from the lexer and produces a [`rsc_syntax::ast::Module`].
//! Implements error recovery at statement boundaries so that parsing continues
//! past syntax errors, accumulating diagnostics along the way.

use rsc_syntax::ast::{
    AssignExpr, BinaryExpr, BinaryOp, Block, CallExpr, ElseClause, Expr, ExprKind, FnDecl, Ident,
    IfStmt, Item, MethodCallExpr, Module, Param, ReturnStmt, Stmt, TypeAnnotation, TypeKind,
    UnaryExpr, UnaryOp, VarBinding, VarDecl, WhileStmt,
};
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::source::FileId;
use rsc_syntax::span::Span;

use crate::token::{Token, TokenKind};

/// Maximum nesting depth for expressions to prevent stack overflow on
/// adversarial input (e.g., deeply nested parentheses).
const MAX_EXPR_DEPTH: usize = 256;

/// Recursive descent parser for the `RustScript` Phase 0 grammar.
///
/// Created with a token stream from the lexer and a file identifier.
/// Call [`Parser::parse_module`] to produce the AST.
pub(crate) struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    file_id: FileId,
    expr_depth: usize,
}

impl Parser {
    /// Create a new parser from a token stream.
    pub(crate) fn new(tokens: Vec<Token>, file_id: FileId) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
            file_id,
            expr_depth: 0,
        }
    }

    /// Consume the parser and return accumulated diagnostics.
    pub(crate) fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    // ---------------------------------------------------------------
    // Token navigation
    // ---------------------------------------------------------------

    /// Look at the current token's kind without consuming it.
    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map_or(&TokenKind::Eof, |t| &t.kind)
    }

    /// Look at the current token (kind + span) without consuming it.
    fn current_token(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    /// Consume the current token and return it.
    fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    /// Check whether the current token matches the given kind without consuming it.
    fn check(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    /// Whether we have reached the end of the token stream.
    fn at_end(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    /// Consume if the current token matches the given kind, otherwise emit a diagnostic.
    fn expect(&mut self, kind: &TokenKind) -> Option<Token> {
        if self.check(kind) {
            Some(self.advance())
        } else {
            let current = self.current_token();
            let span = current.span;
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "expected {}, found {}",
                    Self::describe_kind(kind),
                    Self::describe_kind(&current.kind)
                ))
                .with_label(span, self.file_id, "unexpected token"),
            );
            None
        }
    }

    /// Consume the current token if it matches `kind`, returning true if consumed.
    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// The span of the previously consumed token.
    fn previous_span(&self) -> Span {
        if self.pos == 0 {
            self.tokens[0].span
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    /// Produce a human-readable description of a token kind for diagnostics.
    fn describe_kind(kind: &TokenKind) -> &'static str {
        match kind {
            TokenKind::IntLit(_) => "integer literal",
            TokenKind::FloatLit(_) => "float literal",
            TokenKind::StringLit(_) => "string literal",
            TokenKind::Ident(_) => "identifier",
            TokenKind::Function => "`function`",
            TokenKind::Const => "`const`",
            TokenKind::Let => "`let`",
            TokenKind::If => "`if`",
            TokenKind::Else => "`else`",
            TokenKind::While => "`while`",
            TokenKind::Return => "`return`",
            TokenKind::True => "`true`",
            TokenKind::False => "`false`",
            TokenKind::Plus => "`+`",
            TokenKind::Minus => "`-`",
            TokenKind::Star => "`*`",
            TokenKind::Slash => "`/`",
            TokenKind::Percent => "`%`",
            TokenKind::EqEq => "`==`",
            TokenKind::BangEq => "`!=`",
            TokenKind::Lt => "`<`",
            TokenKind::Gt => "`>`",
            TokenKind::LtEq => "`<=`",
            TokenKind::GtEq => "`>=`",
            TokenKind::AmpAmp => "`&&`",
            TokenKind::PipePipe => "`||`",
            TokenKind::Bang => "`!`",
            TokenKind::Eq => "`=`",
            TokenKind::LParen => "`(`",
            TokenKind::RParen => "`)`",
            TokenKind::LBrace => "`{`",
            TokenKind::RBrace => "`}`",
            TokenKind::Comma => "`,`",
            TokenKind::Colon => "`:`",
            TokenKind::Semicolon => "`;`",
            TokenKind::Dot => "`.`",
            TokenKind::Eof => "end of file",
        }
    }

    // ---------------------------------------------------------------
    // Error recovery
    // ---------------------------------------------------------------

    /// Skip tokens until we reach a statement boundary or a keyword that
    /// starts a new statement. Used for error recovery at the statement level.
    fn synchronize(&mut self) {
        loop {
            match self.peek() {
                TokenKind::Eof
                | TokenKind::RBrace
                | TokenKind::Const
                | TokenKind::Let
                | TokenKind::If
                | TokenKind::While
                | TokenKind::Return
                | TokenKind::Function => return,
                TokenKind::Semicolon => {
                    self.advance();
                    return;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    // ---------------------------------------------------------------
    // Top-level parsing
    // ---------------------------------------------------------------

    /// Parse the entire token stream into a [`Module`].
    pub(crate) fn parse_module(&mut self) -> Module {
        let start_span = self.current_token().span;
        let mut items = Vec::new();

        while !self.at_end() {
            if let Some(item) = self.parse_item() {
                items.push(item);
            }
        }

        let end_span = self.current_token().span;
        Module {
            items,
            span: start_span.merge(end_span),
        }
    }

    /// Parse a top-level item. In Phase 0, the only item is a function declaration.
    fn parse_item(&mut self) -> Option<Item> {
        if self.peek() == &TokenKind::Function {
            self.parse_function_decl().map(Item::Function)
        } else {
            let current = self.current_token().clone();
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "expected function declaration, found {}",
                    Self::describe_kind(&current.kind)
                ))
                .with_label(current.span, self.file_id, "unexpected token"),
            );
            self.synchronize();
            None
        }
    }

    // ---------------------------------------------------------------
    // Function declarations
    // ---------------------------------------------------------------

    /// Parse a function declaration: `function IDENT ( params ) : type { body }`.
    fn parse_function_decl(&mut self) -> Option<FnDecl> {
        let fn_token = self.advance(); // consume `function`
        let start = fn_token.span;

        // Function name
        let name = self.parse_ident()?;

        // Parameter list
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_param_list();
        if self.expect(&TokenKind::RParen).is_none() {
            // Try to recover — skip to `{` if possible
            while !self.at_end() && !self.check(&TokenKind::LBrace) {
                self.advance();
            }
        }

        // Optional return type
        let return_type = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        // Body
        let body = self.parse_block()?;

        let span = start.merge(body.span);
        Some(FnDecl {
            name,
            params,
            return_type,
            body,
            span,
        })
    }

    /// Parse a comma-separated parameter list (without the surrounding parens).
    fn parse_param_list(&mut self) -> Vec<Param> {
        let mut params = Vec::new();

        if self.check(&TokenKind::RParen) || self.at_end() {
            return params;
        }

        loop {
            if let Some(param) = self.parse_param() {
                params.push(param);
            }

            if !self.eat(&TokenKind::Comma) {
                break;
            }

            // Allow trailing comma
            if self.check(&TokenKind::RParen) {
                break;
            }
        }

        params
    }

    /// Parse a single parameter: `IDENT : type`.
    fn parse_param(&mut self) -> Option<Param> {
        let start = self.current_token().span;
        let name = self.parse_ident()?;
        self.expect(&TokenKind::Colon)?;
        let type_ann = self.parse_type_annotation()?;
        let span = start.merge(type_ann.span);
        Some(Param {
            name,
            type_ann,
            span,
        })
    }

    /// Parse a type annotation: `void` or an identifier.
    fn parse_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let token = self.current_token().clone();
        match &token.kind {
            TokenKind::Ident(name) if name == "void" => {
                self.advance();
                Some(TypeAnnotation {
                    kind: TypeKind::Void,
                    span: token.span,
                })
            }
            TokenKind::Ident(_) => {
                let ident = self.parse_ident()?;
                let span = ident.span;
                Some(TypeAnnotation {
                    kind: TypeKind::Named(ident),
                    span,
                })
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "expected type, found {}",
                        Self::describe_kind(&token.kind)
                    ))
                    .with_label(token.span, self.file_id, "expected type"),
                );
                None
            }
        }
    }

    /// Parse an identifier token into an [`Ident`] AST node.
    fn parse_ident(&mut self) -> Option<Ident> {
        let token = self.current_token().clone();
        if let TokenKind::Ident(name) = &token.kind {
            let name = name.clone();
            self.advance();
            Some(Ident {
                name,
                span: token.span,
            })
        } else {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "expected identifier, found {}",
                    Self::describe_kind(&token.kind)
                ))
                .with_label(token.span, self.file_id, "expected identifier"),
            );
            None
        }
    }

    // ---------------------------------------------------------------
    // Blocks and statements
    // ---------------------------------------------------------------

    /// Parse a block: `{ stmt* }`.
    fn parse_block(&mut self) -> Option<Block> {
        let open = self.current_token().span;
        self.expect(&TokenKind::LBrace)?;

        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            }
        }

        let close_span = if let Some(close) = self.expect(&TokenKind::RBrace) {
            close.span
        } else {
            self.diagnostics
                .push(Diagnostic::error("unterminated block").with_label(
                    open,
                    self.file_id,
                    "block starts here",
                ));
            self.previous_span()
        };

        Some(Block {
            stmts,
            span: open.merge(close_span),
        })
    }

    /// Parse a statement.
    fn parse_stmt(&mut self) -> Option<Stmt> {
        match self.peek() {
            TokenKind::Const | TokenKind::Let => self.parse_var_decl(),
            TokenKind::If => self.parse_if_stmt().map(Stmt::If),
            TokenKind::While => self.parse_while_stmt().map(Stmt::While),
            TokenKind::Return => self.parse_return_stmt().map(Stmt::Return),
            _ => self.parse_expr_stmt(),
        }
    }

    /// Parse a variable declaration: `(const | let) IDENT (: type)? = expr ;`.
    fn parse_var_decl(&mut self) -> Option<Stmt> {
        let keyword = self.advance();
        let start = keyword.span;
        let binding = match keyword.kind {
            TokenKind::Const => VarBinding::Const,
            _ => VarBinding::Let,
        };

        let Some(name) = self.parse_ident() else {
            self.synchronize();
            return None;
        };

        // Optional type annotation
        let type_ann = if self.eat(&TokenKind::Colon) {
            let Some(t) = self.parse_type_annotation() else {
                self.synchronize();
                return None;
            };
            Some(t)
        } else {
            None
        };

        if self.expect(&TokenKind::Eq).is_none() {
            self.synchronize();
            return None;
        }

        let Some(init) = self.parse_expr() else {
            self.synchronize();
            return None;
        };

        let end = if let Some(semi) = self.expect(&TokenKind::Semicolon) {
            semi.span
        } else {
            // Missing semicolon — recover without consuming
            init.span
        };

        Some(Stmt::VarDecl(VarDecl {
            binding,
            name,
            type_ann,
            init,
            span: start.merge(end),
        }))
    }

    /// Parse an if statement: `if ( expr ) block (else (if_stmt | block))?`.
    fn parse_if_stmt(&mut self) -> Option<IfStmt> {
        let if_token = self.advance(); // consume `if`
        let start = if_token.span;

        self.expect(&TokenKind::LParen)?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen)?;

        let then_block = self.parse_block()?;

        let else_clause = if self.eat(&TokenKind::Else) {
            if self.check(&TokenKind::If) {
                let inner_if = self.parse_if_stmt()?;
                Some(ElseClause::ElseIf(Box::new(inner_if)))
            } else {
                let block = self.parse_block()?;
                Some(ElseClause::Block(block))
            }
        } else {
            None
        };

        let end = match &else_clause {
            Some(ElseClause::Block(b)) => b.span,
            Some(ElseClause::ElseIf(i)) => i.span,
            None => then_block.span,
        };

        Some(IfStmt {
            condition,
            then_block,
            else_clause,
            span: start.merge(end),
        })
    }

    /// Parse a while statement: `while ( expr ) block`.
    fn parse_while_stmt(&mut self) -> Option<WhileStmt> {
        let while_token = self.advance(); // consume `while`
        let start = while_token.span;

        self.expect(&TokenKind::LParen)?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen)?;

        let body = self.parse_block()?;

        Some(WhileStmt {
            condition,
            body: body.clone(),
            span: start.merge(body.span),
        })
    }

    /// Parse a return statement: `return expr? ;`.
    fn parse_return_stmt(&mut self) -> Option<ReturnStmt> {
        let ret_token = self.advance(); // consume `return`
        let start = ret_token.span;

        let value = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };

        let end = if let Some(semi) = self.expect(&TokenKind::Semicolon) {
            semi.span
        } else {
            value.as_ref().map_or(start, |v| v.span)
        };

        Some(ReturnStmt {
            value,
            span: start.merge(end),
        })
    }

    /// Parse an expression statement: `expr ;`.
    fn parse_expr_stmt(&mut self) -> Option<Stmt> {
        let Some(expr) = self.parse_expr() else {
            self.synchronize();
            return None;
        };

        if self.expect(&TokenKind::Semicolon).is_none() {
            // Missing semicolon — continue without consuming
        }

        Some(Stmt::Expr(expr))
    }

    // ---------------------------------------------------------------
    // Expressions — recursive descent by precedence
    // ---------------------------------------------------------------

    /// Parse an expression (entry point — starts at assignment level).
    ///
    /// Tracks recursion depth to prevent stack overflow on adversarial input.
    fn parse_expr(&mut self) -> Option<Expr> {
        self.expr_depth += 1;
        if self.expr_depth > MAX_EXPR_DEPTH {
            let span = self.current_token().span;
            self.diagnostics.push(
                Diagnostic::error("expression nesting depth exceeded maximum").with_label(
                    span,
                    self.file_id,
                    "here",
                ),
            );
            self.expr_depth -= 1;
            return None;
        }
        let result = self.parse_assignment();
        self.expr_depth -= 1;
        result
    }

    /// Parse assignment: `IDENT = assignment | logic_or`.
    ///
    /// Assignment is right-associative: `a = b = c` parses as `a = (b = c)`.
    fn parse_assignment(&mut self) -> Option<Expr> {
        let expr = self.parse_logic_or()?;

        // Check if this is an assignment
        if self.check(&TokenKind::Eq) {
            if let ExprKind::Ident(ident) = expr.kind {
                self.advance(); // consume `=`
                let value = self.parse_assignment()?;
                let span = ident.span.merge(value.span);
                return Some(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident,
                        value: Box::new(value),
                    }),
                    span,
                });
            }
            // Assignment to non-identifier — emit diagnostic
            let eq_token = self.advance();
            self.diagnostics
                .push(Diagnostic::error("invalid assignment target").with_label(
                    expr.span,
                    self.file_id,
                    "cannot assign to this expression",
                ));
            // Still parse the right-hand side to recover
            let _ = self.parse_assignment();
            return Some(Expr {
                kind: ExprKind::IntLit(0),
                span: expr.span.merge(eq_token.span),
            });
        }

        Some(expr)
    }

    /// Parse logical OR: `logic_and ( "||" logic_and )*`.
    fn parse_logic_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_logic_and()?;

        while self.check(&TokenKind::PipePipe) {
            self.advance();
            let right = self.parse_logic_and()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
    }

    /// Parse logical AND: `equality ( "&&" equality )*`.
    fn parse_logic_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_equality()?;

        while self.check(&TokenKind::AmpAmp) {
            self.advance();
            let right = self.parse_equality()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
    }

    /// Parse equality: `comparison ( ("==" | "!=") comparison )*`.
    fn parse_equality(&mut self) -> Option<Expr> {
        let mut left = self.parse_comparison()?;

        loop {
            let op = match self.peek() {
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::BangEq => BinaryOp::Ne,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
    }

    /// Parse comparison: `addition ( ("<" | ">" | "<=" | ">=") addition )*`.
    fn parse_comparison(&mut self) -> Option<Expr> {
        let mut left = self.parse_addition()?;

        loop {
            let op = match self.peek() {
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::LtEq => BinaryOp::Le,
                TokenKind::GtEq => BinaryOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_addition()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
    }

    /// Parse addition: `multiplication ( ("+" | "-") multiplication )*`.
    fn parse_addition(&mut self) -> Option<Expr> {
        let mut left = self.parse_multiplication()?;

        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplication()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
    }

    /// Parse multiplication: `unary ( ("*" | "/" | "%") unary )*`.
    fn parse_multiplication(&mut self) -> Option<Expr> {
        let mut left = self.parse_unary()?;

        loop {
            let op = match self.peek() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
    }

    /// Parse unary: `("-" | "!") unary | call`.
    fn parse_unary(&mut self) -> Option<Expr> {
        match self.peek() {
            TokenKind::Minus => {
                let op_token = self.advance();
                let operand = self.parse_unary()?;
                let span = op_token.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::Unary(UnaryExpr {
                        op: UnaryOp::Neg,
                        operand: Box::new(operand),
                    }),
                    span,
                })
            }
            TokenKind::Bang => {
                let op_token = self.advance();
                let operand = self.parse_unary()?;
                let span = op_token.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::Unary(UnaryExpr {
                        op: UnaryOp::Not,
                        operand: Box::new(operand),
                    }),
                    span,
                })
            }
            _ => self.parse_call(),
        }
    }

    /// Parse call expressions: `primary ( "(" args ")" | "." IDENT "(" args ")" )*`.
    fn parse_call(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.check(&TokenKind::LParen) {
                // Function call — the expression must be an identifier
                if let ExprKind::Ident(callee) = expr.kind {
                    self.advance(); // consume `(`
                    let args = self.parse_arg_list();
                    let close = self.expect(&TokenKind::RParen)?;
                    let span = callee.span.merge(close.span);
                    expr = Expr {
                        kind: ExprKind::Call(CallExpr { callee, args }),
                        span,
                    };
                } else {
                    // Non-identifier call target — not supported in Phase 0
                    break;
                }
            } else if self.check(&TokenKind::Dot) {
                self.advance(); // consume `.`
                let method = self.parse_ident()?;
                self.expect(&TokenKind::LParen)?;
                let args = self.parse_arg_list();
                let close = self.expect(&TokenKind::RParen)?;
                let span = expr.span.merge(close.span);
                expr = Expr {
                    kind: ExprKind::MethodCall(MethodCallExpr {
                        object: Box::new(expr),
                        method,
                        args,
                    }),
                    span,
                };
            } else {
                break;
            }
        }

        Some(expr)
    }

    /// Parse a comma-separated argument list (without the surrounding parens).
    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();

        if self.check(&TokenKind::RParen) || self.at_end() {
            return args;
        }

        loop {
            if let Some(arg) = self.parse_expr() {
                args.push(arg);
            }

            if !self.eat(&TokenKind::Comma) {
                break;
            }

            // Allow trailing comma
            if self.check(&TokenKind::RParen) {
                break;
            }
        }

        args
    }

    /// Parse a primary expression: literal, identifier, or parenthesized expression.
    #[allow(clippy::too_many_lines)]
    // Primary match covers all terminal expression types; splitting would obscure the grammar
    fn parse_primary(&mut self) -> Option<Expr> {
        let token = self.current_token().clone();

        match &token.kind {
            TokenKind::IntLit(value) => {
                let value = *value;
                self.advance();
                Some(Expr {
                    kind: ExprKind::IntLit(value),
                    span: token.span,
                })
            }
            TokenKind::FloatLit(value) => {
                let value = *value;
                self.advance();
                Some(Expr {
                    kind: ExprKind::FloatLit(value),
                    span: token.span,
                })
            }
            TokenKind::StringLit(value) => {
                let value = value.clone();
                self.advance();
                Some(Expr {
                    kind: ExprKind::StringLit(value),
                    span: token.span,
                })
            }
            TokenKind::True => {
                self.advance();
                Some(Expr {
                    kind: ExprKind::BoolLit(true),
                    span: token.span,
                })
            }
            TokenKind::False => {
                self.advance();
                Some(Expr {
                    kind: ExprKind::BoolLit(false),
                    span: token.span,
                })
            }
            TokenKind::Ident(_) => {
                let ident = self.parse_ident()?;
                let span = ident.span;
                Some(Expr {
                    kind: ExprKind::Ident(ident),
                    span,
                })
            }
            TokenKind::LParen => {
                let open = self.advance();
                let inner = self.parse_expr()?;
                let close = self.expect(&TokenKind::RParen)?;
                let span = open.span.merge(close.span);
                Some(Expr {
                    kind: ExprKind::Paren(Box::new(inner)),
                    span,
                })
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "expected expression, found {}",
                        Self::describe_kind(&token.kind)
                    ))
                    .with_label(
                        token.span,
                        self.file_id,
                        "expected expression",
                    ),
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use rsc_syntax::source::FileId;

    /// Helper: parse source and return (Module, diagnostics).
    fn parse_source(source: &str) -> (Module, Vec<Diagnostic>) {
        parse(source, FileId(0))
    }

    /// Helper: parse source, assert no diagnostics, return module.
    fn parse_ok(source: &str) -> Module {
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got: {diagnostics:?}"
        );
        module
    }

    /// Helper: extract the first (and only) function declaration from a module.
    fn first_fn(module: &Module) -> &FnDecl {
        assert_eq!(module.items.len(), 1, "expected exactly one item");
        match &module.items[0] {
            Item::Function(f) => f,
        }
    }

    /// Helper: extract the first statement from a function body.
    fn first_stmt(f: &FnDecl) -> &Stmt {
        assert!(
            !f.body.stmts.is_empty(),
            "expected at least one statement in body"
        );
        &f.body.stmts[0]
    }

    // ---------------------------------------------------------------
    // 1. Empty source
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_empty_source_produces_empty_module() {
        let module = parse_ok("");
        assert!(module.items.is_empty());
    }

    // ---------------------------------------------------------------
    // 2. function main() {}
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_empty_fn_produces_fn_decl_node() {
        let module = parse_ok("function main() {}");
        let f = first_fn(&module);
        assert_eq!(f.name.name, "main");
        assert!(f.params.is_empty());
        assert!(f.return_type.is_none());
        assert!(f.body.stmts.is_empty());
    }

    // ---------------------------------------------------------------
    // 3. Function with params and return type
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_fn_with_params_and_return_type() {
        let module = parse_ok("function add(a: i32, b: i32): i32 { return a + b; }");
        let f = first_fn(&module);
        assert_eq!(f.name.name, "add");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name.name, "a");
        assert_eq!(f.params[1].name.name, "b");
        // Check return type
        let ret = f.return_type.as_ref().expect("expected return type");
        match &ret.kind {
            TypeKind::Named(ident) => assert_eq!(ident.name, "i32"),
            TypeKind::Void => panic!("expected Named, got Void"),
        }
        // Body has one return statement
        assert_eq!(f.body.stmts.len(), 1);
        match &f.body.stmts[0] {
            Stmt::Return(r) => {
                assert!(r.value.is_some());
                // The return value should be Binary(Add, ...)
                let val = r.value.as_ref().unwrap();
                match &val.kind {
                    ExprKind::Binary(b) => assert_eq!(b.op, BinaryOp::Add),
                    other => panic!("expected Binary, got {other:?}"),
                }
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 4. const with type annotation
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_const_var_decl_with_type_annotation() {
        let module = parse_ok("function f() { const x: i32 = 42; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(v) => {
                assert_eq!(v.binding, VarBinding::Const);
                assert_eq!(v.name.name, "x");
                assert!(v.type_ann.is_some());
                match &v.init.kind {
                    ExprKind::IntLit(42) => {}
                    other => panic!("expected IntLit(42), got {other:?}"),
                }
            }
            other => panic!("expected VarDecl, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 5. let without type annotation
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_let_var_decl_no_type_annotation() {
        let module = parse_ok("function f() { let x = 42; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(v) => {
                assert_eq!(v.binding, VarBinding::Let);
                assert_eq!(v.name.name, "x");
                assert!(v.type_ann.is_none());
            }
            other => panic!("expected VarDecl, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 6. if/else
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_if_else_stmt() {
        let module = parse_ok("function f() { if (x > 0) { return x; } else { return 0; } }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::If(i) => {
                // Condition is a comparison
                match &i.condition.kind {
                    ExprKind::Binary(b) => assert_eq!(b.op, BinaryOp::Gt),
                    other => panic!("expected Binary(Gt), got {other:?}"),
                }
                // Then block has return
                assert_eq!(i.then_block.stmts.len(), 1);
                assert!(matches!(i.then_block.stmts[0], Stmt::Return(_)));
                // Else block
                match &i.else_clause {
                    Some(ElseClause::Block(b)) => {
                        assert_eq!(b.stmts.len(), 1);
                        assert!(matches!(b.stmts[0], Stmt::Return(_)));
                    }
                    other => panic!("expected ElseClause::Block, got {other:?}"),
                }
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 7. else-if chain
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_else_if_chain() {
        let module = parse_ok("function f() { if (a) { } else if (b) { } else { } }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::If(i) => {
                match &i.else_clause {
                    Some(ElseClause::ElseIf(inner)) => {
                        match &inner.else_clause {
                            Some(ElseClause::Block(_)) => {} // correct
                            other => panic!("expected inner else Block, got {other:?}"),
                        }
                    }
                    other => panic!("expected ElseIf, got {other:?}"),
                }
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 8. while loop
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_while_stmt() {
        let module = parse_ok("function f() { while (x > 0) { x = x - 1; } }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::While(w) => {
                match &w.condition.kind {
                    ExprKind::Binary(b) => assert_eq!(b.op, BinaryOp::Gt),
                    other => panic!("expected Binary(Gt), got {other:?}"),
                }
                assert_eq!(w.body.stmts.len(), 1);
            }
            other => panic!("expected While, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 9. Operator precedence: multiplication binds tighter than addition
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_precedence_mul_over_add() {
        // 1 + 2 * 3 should parse as Binary(Add, 1, Binary(Mul, 2, 3))
        let module = parse_ok("function f() { 1 + 2 * 3; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Binary(b) => {
                    assert_eq!(b.op, BinaryOp::Add);
                    match &b.left.kind {
                        ExprKind::IntLit(1) => {}
                        other => panic!("expected IntLit(1), got {other:?}"),
                    }
                    match &b.right.kind {
                        ExprKind::Binary(inner) => {
                            assert_eq!(inner.op, BinaryOp::Mul);
                            match &inner.left.kind {
                                ExprKind::IntLit(2) => {}
                                other => panic!("expected IntLit(2), got {other:?}"),
                            }
                            match &inner.right.kind {
                                ExprKind::IntLit(3) => {}
                                other => panic!("expected IntLit(3), got {other:?}"),
                            }
                        }
                        other => panic!("expected Binary(Mul), got {other:?}"),
                    }
                }
                other => panic!("expected Binary(Add), got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 10. AND binds tighter than OR
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_precedence_and_over_or() {
        // a || b && c should parse as Binary(Or, a, Binary(And, b, c))
        let module = parse_ok("function f() { a || b && c; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Binary(b) => {
                    assert_eq!(b.op, BinaryOp::Or);
                    match &b.right.kind {
                        ExprKind::Binary(inner) => assert_eq!(inner.op, BinaryOp::And),
                        other => panic!("expected Binary(And), got {other:?}"),
                    }
                }
                other => panic!("expected Binary(Or), got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 11. Unary operators
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_unary_neg() {
        let module = parse_ok("function f() { -x; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Unary(u) => {
                    assert_eq!(u.op, UnaryOp::Neg);
                    match &u.operand.kind {
                        ExprKind::Ident(id) => assert_eq!(id.name, "x"),
                        other => panic!("expected Ident(x), got {other:?}"),
                    }
                }
                other => panic!("expected Unary(Neg), got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_unary_not() {
        let module = parse_ok("function f() { !flag; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Unary(u) => {
                    assert_eq!(u.op, UnaryOp::Not);
                    match &u.operand.kind {
                        ExprKind::Ident(id) => assert_eq!(id.name, "flag"),
                        other => panic!("expected Ident(flag), got {other:?}"),
                    }
                }
                other => panic!("expected Unary(Not), got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 12. Function call
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_function_call() {
        let module = parse_ok("function f() { foo(1, 2); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Call(c) => {
                    assert_eq!(c.callee.name, "foo");
                    assert_eq!(c.args.len(), 2);
                    match &c.args[0].kind {
                        ExprKind::IntLit(1) => {}
                        other => panic!("expected IntLit(1), got {other:?}"),
                    }
                    match &c.args[1].kind {
                        ExprKind::IntLit(2) => {}
                        other => panic!("expected IntLit(2), got {other:?}"),
                    }
                }
                other => panic!("expected Call, got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 13. Method call
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_method_call() {
        let module = parse_ok("function f() { console.log(\"hello\"); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::MethodCall(m) => {
                    match &m.object.kind {
                        ExprKind::Ident(id) => assert_eq!(id.name, "console"),
                        other => panic!("expected Ident(console), got {other:?}"),
                    }
                    assert_eq!(m.method.name, "log");
                    assert_eq!(m.args.len(), 1);
                    match &m.args[0].kind {
                        ExprKind::StringLit(s) => assert_eq!(s, "hello"),
                        other => panic!("expected StringLit(\"hello\"), got {other:?}"),
                    }
                }
                other => panic!("expected MethodCall, got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 14. Parenthesized expression
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_paren_expr_nesting() {
        // (a + b) * c — paren forces addition first
        let module = parse_ok("function f() { (a + b) * c; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Binary(b) => {
                    assert_eq!(b.op, BinaryOp::Mul);
                    match &b.left.kind {
                        ExprKind::Paren(inner) => match &inner.kind {
                            ExprKind::Binary(inner_b) => assert_eq!(inner_b.op, BinaryOp::Add),
                            other => panic!("expected Binary(Add) inside paren, got {other:?}"),
                        },
                        other => panic!("expected Paren, got {other:?}"),
                    }
                }
                other => panic!("expected Binary(Mul), got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 15. Assignment as expression statement
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_assignment_expr_stmt() {
        let module = parse_ok("function f() { x = 42; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Assign(a) => {
                    assert_eq!(a.target.name, "x");
                    match &a.value.kind {
                        ExprKind::IntLit(42) => {}
                        other => panic!("expected IntLit(42), got {other:?}"),
                    }
                }
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 16. String, boolean, float literals
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_literals_string_bool_float() {
        let module = parse_ok(r#"function f() { "hello"; true; false; 3.14; }"#);
        let f = first_fn(&module);
        assert_eq!(f.body.stmts.len(), 4);

        match &f.body.stmts[0] {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::StringLit(s) => assert_eq!(s, "hello"),
                other => panic!("expected StringLit, got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }

        match &f.body.stmts[1] {
            Stmt::Expr(e) => assert!(matches!(e.kind, ExprKind::BoolLit(true))),
            other => panic!("expected Expr(BoolLit(true)), got {other:?}"),
        }

        match &f.body.stmts[2] {
            Stmt::Expr(e) => assert!(matches!(e.kind, ExprKind::BoolLit(false))),
            other => panic!("expected Expr(BoolLit(false)), got {other:?}"),
        }

        match &f.body.stmts[3] {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::FloatLit(v) => assert!((*v - 3.14).abs() < f64::EPSILON),
                other => panic!("expected FloatLit(3.14), got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 17. return with no value
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_return_no_value() {
        let module = parse_ok("function f() { return; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Return(r) => assert!(r.value.is_none()),
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 18. Multiple functions
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_multiple_functions() {
        let module = parse_ok("function a() {} function b() {} function c() {}");
        assert_eq!(module.items.len(), 3);
        match &module.items[0] {
            Item::Function(f) => assert_eq!(f.name.name, "a"),
        }
        match &module.items[1] {
            Item::Function(f) => assert_eq!(f.name.name, "b"),
        }
        match &module.items[2] {
            Item::Function(f) => assert_eq!(f.name.name, "c"),
        }
    }

    // ---------------------------------------------------------------
    // 19. Error recovery: unexpected semicolon in initializer
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_error_recovery_unexpected_semi_in_init() {
        let source = "function f() { const x = ; let y = 1; }";
        let (module, diagnostics) = parse_source(source);
        // Should have at least one diagnostic
        assert!(
            !diagnostics.is_empty(),
            "expected diagnostic for unexpected `;`"
        );
        // Parsing should continue — the function should still parse, and
        // the `let y = 1;` statement should be present
        let f = first_fn(&module);
        // The body should have at least one successfully parsed statement
        let has_let_y = f.body.stmts.iter().any(|s| match s {
            Stmt::VarDecl(v) => v.name.name == "y",
            _ => false,
        });
        assert!(has_let_y, "expected `let y = 1` to parse after recovery");
    }

    // ---------------------------------------------------------------
    // 20. Error recovery: missing closing paren in function params
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_error_recovery_missing_rparen() {
        let source = "function foo( { }";
        let (_, diagnostics) = parse_source(source);
        // Should have a diagnostic for the missing `)`
        assert!(
            !diagnostics.is_empty(),
            "expected diagnostic for missing `)`"
        );
    }

    // ---------------------------------------------------------------
    // 21. Spans are correct: function spans from keyword to closing brace
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_fn_span_covers_keyword_to_closing_brace() {
        let source = "function main() {}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        // The span should start at 0 (the 'f' in 'function') and end at 18 (after '}')
        assert_eq!(f.span.start.0, 0);
        assert_eq!(f.span.end.0, 18);
    }

    // ---------------------------------------------------------------
    // Correctness scenario 1: Fibonacci
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_correctness_fibonacci() {
        let source = r#"function fibonacci(n: i32): i32 {
  if (n <= 1) {
    return n;
  }
  return fibonacci(n - 1) + fibonacci(n - 2);
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);

        // Name
        assert_eq!(f.name.name, "fibonacci");

        // One param: n: i32
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].name.name, "n");
        match &f.params[0].type_ann.kind {
            TypeKind::Named(id) => assert_eq!(id.name, "i32"),
            other => panic!("expected Named(i32), got {other:?}"),
        }

        // Return type: i32
        let ret = f.return_type.as_ref().expect("expected return type");
        match &ret.kind {
            TypeKind::Named(id) => assert_eq!(id.name, "i32"),
            other => panic!("expected Named(i32), got {other:?}"),
        }

        // Body: if statement + return statement
        assert_eq!(f.body.stmts.len(), 2);
        assert!(
            matches!(f.body.stmts[0], Stmt::If(_)),
            "first stmt should be If"
        );

        // Second statement: return fibonacci(n-1) + fibonacci(n-2)
        match &f.body.stmts[1] {
            Stmt::Return(r) => {
                let val = r.value.as_ref().expect("expected return value");
                match &val.kind {
                    ExprKind::Binary(b) => {
                        assert_eq!(b.op, BinaryOp::Add);
                        // Both sides should be Call expressions
                        match &b.left.kind {
                            ExprKind::Call(c) => assert_eq!(c.callee.name, "fibonacci"),
                            other => panic!("expected Call(fibonacci), got {other:?}"),
                        }
                        match &b.right.kind {
                            ExprKind::Call(c) => assert_eq!(c.callee.name, "fibonacci"),
                            other => panic!("expected Call(fibonacci), got {other:?}"),
                        }
                    }
                    other => panic!("expected Binary(Add), got {other:?}"),
                }
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Correctness scenario 2: Variable mutation (countdown)
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_correctness_countdown() {
        let source = r#"function countdown(n: i32): void {
  let count = n;
  while (count > 0) {
    console.log(count);
    count = count - 1;
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);

        // Name
        assert_eq!(f.name.name, "countdown");

        // Return type: void
        let ret = f.return_type.as_ref().expect("expected return type");
        assert!(matches!(ret.kind, TypeKind::Void));

        // Body: let count = n; while (...) { ... }
        assert_eq!(f.body.stmts.len(), 2);

        // First statement: let count = n
        match &f.body.stmts[0] {
            Stmt::VarDecl(v) => {
                assert_eq!(v.binding, VarBinding::Let);
                assert_eq!(v.name.name, "count");
            }
            other => panic!("expected VarDecl, got {other:?}"),
        }

        // Second statement: while loop
        match &f.body.stmts[1] {
            Stmt::While(w) => {
                assert_eq!(w.body.stmts.len(), 2);

                // First in while body: console.log(count) — method call
                match &w.body.stmts[0] {
                    Stmt::Expr(e) => match &e.kind {
                        ExprKind::MethodCall(m) => {
                            assert_eq!(m.method.name, "log");
                        }
                        other => panic!("expected MethodCall, got {other:?}"),
                    },
                    other => panic!("expected Expr, got {other:?}"),
                }

                // Second in while body: count = count - 1 — assignment
                match &w.body.stmts[1] {
                    Stmt::Expr(e) => match &e.kind {
                        ExprKind::Assign(a) => {
                            assert_eq!(a.target.name, "count");
                        }
                        other => panic!("expected Assign, got {other:?}"),
                    },
                    other => panic!("expected Expr, got {other:?}"),
                }
            }
            other => panic!("expected While, got {other:?}"),
        }
    }
}
