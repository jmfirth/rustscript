//! Recursive descent parser for `RustScript` source files.
//!
//! Consumes the token stream from the lexer and produces a [`rsc_syntax::ast::Module`].
//! Implements error recovery at statement boundaries so that parsing continues
//! past syntax errors, accumulating diagnostics along the way.

use rsc_syntax::ast::{
    AssignExpr, BinaryExpr, BinaryOp, Block, CallExpr, DestructureStmt, ElseClause, Expr, ExprKind,
    FieldAccessExpr, FieldDef, FieldInit, FnDecl, Ident, IfStmt, Item, ItemKind, MethodCallExpr,
    Module, Param, ReturnStmt, Stmt, StructLitExpr, TemplateLitExpr, TemplatePart, TypeAnnotation,
    TypeDef, TypeKind, TypeParam, TypeParams, UnaryExpr, UnaryOp, VarBinding, VarDecl, WhileStmt,
};
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::source::FileId;
use rsc_syntax::span::Span;

use crate::token::{Token, TokenKind};

/// Maximum nesting depth for expressions to prevent stack overflow on
/// adversarial input (e.g., deeply nested parentheses).
///
/// Set conservatively to account for the full precedence chain per depth
/// level in debug builds. Each expression depth level uses ~10 stack
/// frames through the precedence hierarchy.
const MAX_EXPR_DEPTH: usize = 64;

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
            TokenKind::PlusEq => "`+=`",
            TokenKind::MinusEq => "`-=`",
            TokenKind::StarEq => "`*=`",
            TokenKind::SlashEq => "`/=`",
            TokenKind::PercentEq => "`%=`",
            TokenKind::LParen => "`(`",
            TokenKind::RParen => "`)`",
            TokenKind::LBrace => "`{`",
            TokenKind::RBrace => "`}`",
            TokenKind::Comma => "`,`",
            TokenKind::Colon => "`:`",
            TokenKind::Semicolon => "`;`",
            TokenKind::Dot => "`.`",
            TokenKind::Type => "`type`",
            TokenKind::Extends => "`extends`",
            TokenKind::TemplateHead(_) | TokenKind::TemplateNoSub(_) => "template literal",
            TokenKind::TemplateMiddle(_) => "template literal middle",
            TokenKind::TemplateTail(_) => "template literal tail",
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
                | TokenKind::Function
                | TokenKind::Type => return,
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

    /// Parse a top-level item: function declaration or type definition.
    fn parse_item(&mut self) -> Option<Item> {
        match self.peek() {
            TokenKind::Function => self.parse_function_decl().map(|f| {
                let span = f.span;
                Item {
                    kind: ItemKind::Function(f),
                    exported: false,
                    span,
                }
            }),
            TokenKind::Type => self.parse_type_def().map(|td| {
                let span = td.span;
                Item {
                    kind: ItemKind::TypeDef(td),
                    exported: false,
                    span,
                }
            }),
            _ => {
                let current = self.current_token().clone();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "expected item, found {}",
                        Self::describe_kind(&current.kind)
                    ))
                    .with_label(
                        current.span,
                        self.file_id,
                        "unexpected token",
                    ),
                );
                self.synchronize();
                None
            }
        }
    }

    // ---------------------------------------------------------------
    // Function declarations
    // ---------------------------------------------------------------

    /// Parse a function declaration: `function IDENT<T>( params ) : type { body }`.
    fn parse_function_decl(&mut self) -> Option<FnDecl> {
        let fn_token = self.advance(); // consume `function`
        let start = fn_token.span;

        // Function name
        let name = self.parse_ident()?;

        // Optional generic type parameters: `<T, U extends Clone>`
        let type_params = if self.check(&TokenKind::Lt) {
            Some(self.parse_type_params()?)
        } else {
            None
        };

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
            type_params,
            params,
            return_type,
            body,
            span,
        })
    }

    // ---------------------------------------------------------------
    // Type definitions
    // ---------------------------------------------------------------

    /// Parse a type definition: `type NAME<T> = { field: Type, ... }`.
    fn parse_type_def(&mut self) -> Option<TypeDef> {
        let type_token = self.advance(); // consume `type`
        let start = type_token.span;

        let name = self.parse_ident()?;

        // Optional generic type parameters: `<T, U extends Clone>`
        let type_params = if self.check(&TokenKind::Lt) {
            Some(self.parse_type_params()?)
        } else {
            None
        };

        self.expect(&TokenKind::Eq)?;
        self.expect(&TokenKind::LBrace)?;

        let fields = self.parse_field_def_list();

        let close = self.expect(&TokenKind::RBrace)?;
        let span = start.merge(close.span);

        Some(TypeDef {
            name,
            type_params,
            fields,
            span,
        })
    }

    /// Parse a comma-separated list of field definitions: `name: Type, ...`.
    fn parse_field_def_list(&mut self) -> Vec<FieldDef> {
        let mut fields = Vec::new();

        if self.check(&TokenKind::RBrace) || self.at_end() {
            return fields;
        }

        loop {
            let field_start = self.current_token().span;
            let Some(name) = self.parse_ident() else {
                break;
            };
            if self.expect(&TokenKind::Colon).is_none() {
                break;
            }
            let Some(type_ann) = self.parse_type_annotation() else {
                break;
            };
            let field_span = field_start.merge(type_ann.span);
            fields.push(FieldDef {
                name,
                type_ann,
                span: field_span,
            });

            if !self.eat(&TokenKind::Comma) {
                break;
            }

            // Allow trailing comma
            if self.check(&TokenKind::RBrace) {
                break;
            }
        }

        fields
    }

    // ---------------------------------------------------------------
    // Generic type parameters
    // ---------------------------------------------------------------

    /// Parse generic type parameters: `< T, U extends Clone >`.
    ///
    /// Called when `<` has been peeked after a function name or type name.
    fn parse_type_params(&mut self) -> Option<TypeParams> {
        let open = self.advance(); // consume `<`
        let start = open.span;
        let mut params = Vec::new();

        if !self.check(&TokenKind::Gt) && !self.at_end() {
            loop {
                let param_start = self.current_token().span;
                let name = self.parse_ident()?;

                // Optional constraint: `extends Bound`
                let constraint = if self.check(&TokenKind::Extends) {
                    self.advance(); // consume `extends`
                    let bound = self.parse_type_annotation()?;
                    Some(bound)
                } else {
                    None
                };

                let param_end = constraint.as_ref().map_or(name.span, |c| c.span);
                params.push(TypeParam {
                    name,
                    constraint,
                    span: param_start.merge(param_end),
                });

                if !self.eat(&TokenKind::Comma) {
                    break;
                }

                // Allow trailing comma
                if self.check(&TokenKind::Gt) {
                    break;
                }
            }
        }

        let close = self.expect(&TokenKind::Gt)?;
        let span = start.merge(close.span);

        Some(TypeParams { params, span })
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

    /// Parse a type annotation: `void`, a named type, or a generic type.
    ///
    /// Handles `void`, `i32`, `Container<T>`, `Map<string, u32>`, etc.
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
                let start_span = ident.span;

                // Check for generic type arguments: `<T, U>`
                if self.check(&TokenKind::Lt) {
                    self.advance(); // consume `<`
                    let mut args = Vec::new();

                    if !self.check(&TokenKind::Gt) && !self.at_end() {
                        loop {
                            let arg = self.parse_type_annotation()?;
                            args.push(arg);

                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }

                            // Allow trailing comma
                            if self.check(&TokenKind::Gt) {
                                break;
                            }
                        }
                    }

                    let close = self.expect(&TokenKind::Gt)?;
                    let span = start_span.merge(close.span);
                    Some(TypeAnnotation {
                        kind: TypeKind::Generic(ident, args),
                        span,
                    })
                } else {
                    Some(TypeAnnotation {
                        kind: TypeKind::Named(ident),
                        span: start_span,
                    })
                }
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

    /// Parse a variable declaration or destructuring:
    /// - `(const | let) IDENT (: type)? = expr ;`
    /// - `(const | let) { field, ... } = expr ;`
    fn parse_var_decl(&mut self) -> Option<Stmt> {
        let keyword = self.advance();
        let start = keyword.span;
        let binding = match keyword.kind {
            TokenKind::Const => VarBinding::Const,
            _ => VarBinding::Let,
        };

        // Check for destructuring: `const { ... } = expr;`
        if self.check(&TokenKind::LBrace) {
            return self.parse_destructure(binding, start);
        }

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
        let body_span = body.span;

        Some(WhileStmt {
            condition,
            body,
            span: start.merge(body_span),
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

    /// Parse a destructuring statement: `{ field, ... } = expr ;`.
    ///
    /// The keyword (`const`/`let`) and binding have already been consumed.
    fn parse_destructure(&mut self, binding: VarBinding, start: Span) -> Option<Stmt> {
        self.advance(); // consume `{`

        let mut fields = Vec::new();
        if !self.check(&TokenKind::RBrace) && !self.at_end() {
            loop {
                let Some(field_name) = self.parse_ident() else {
                    self.synchronize();
                    return None;
                };
                fields.push(field_name);

                if !self.eat(&TokenKind::Comma) {
                    break;
                }

                // Allow trailing comma
                if self.check(&TokenKind::RBrace) {
                    break;
                }
            }
        }

        if self.expect(&TokenKind::RBrace).is_none() {
            self.synchronize();
            return None;
        }

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
            init.span
        };

        Some(Stmt::Destructure(DestructureStmt {
            binding,
            fields,
            init,
            span: start.merge(end),
        }))
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

    /// Parse assignment: `IDENT = assignment | IDENT op= assignment | logic_or`.
    ///
    /// Assignment is right-associative: `a = b = c` parses as `a = (b = c)`.
    /// Compound assignments (`+=`, `-=`, etc.) are desugared to `x = x op rhs`.
    fn parse_assignment(&mut self) -> Option<Expr> {
        let expr = self.parse_logic_or()?;

        // Check for compound assignment operators
        let compound_op = match self.peek() {
            TokenKind::PlusEq => Some(BinaryOp::Add),
            TokenKind::MinusEq => Some(BinaryOp::Sub),
            TokenKind::StarEq => Some(BinaryOp::Mul),
            TokenKind::SlashEq => Some(BinaryOp::Div),
            TokenKind::PercentEq => Some(BinaryOp::Mod),
            _ => None,
        };

        if let Some(op) = compound_op {
            if let ExprKind::Ident(ref ident) = expr.kind {
                let ident = ident.clone();
                self.advance(); // consume compound operator
                let rhs = self.parse_assignment()?;
                let rhs_span = rhs.span;
                // Desugar x += rhs to x = x + rhs
                let binary = Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        op,
                        left: Box::new(Expr {
                            kind: ExprKind::Ident(ident.clone()),
                            span: ident.span,
                        }),
                        right: Box::new(rhs),
                    }),
                    span: ident.span.merge(rhs_span),
                };
                let span = ident.span.merge(rhs_span);
                return Some(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident,
                        value: Box::new(binary),
                    }),
                    span,
                });
            }
            // Compound assignment to non-identifier — emit diagnostic
            let op_token = self.advance();
            self.diagnostics
                .push(Diagnostic::error("invalid assignment target").with_label(
                    expr.span,
                    self.file_id,
                    "cannot assign to this expression",
                ));
            let _ = self.parse_assignment();
            return Some(Expr {
                kind: ExprKind::IntLit(0),
                span: expr.span.merge(op_token.span),
            });
        }

        // Check if this is a simple assignment
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

    /// Parse call and member expressions:
    /// `primary ( "(" args ")" | "." IDENT "(" args ")" | "." IDENT )*`.
    ///
    /// Handles function calls, method calls, and field access. Field access
    /// is distinguished from method calls by the absence of `(` after the
    /// field name.
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
                let member = self.parse_ident()?;

                if self.check(&TokenKind::LParen) {
                    // Method call: `.method(args)`
                    self.advance(); // consume `(`
                    let args = self.parse_arg_list();
                    let close = self.expect(&TokenKind::RParen)?;
                    let span = expr.span.merge(close.span);
                    expr = Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(expr),
                            method: member,
                            args,
                        }),
                        span,
                    };
                } else {
                    // Field access: `.field`
                    let span = expr.span.merge(member.span);
                    expr = Expr {
                        kind: ExprKind::FieldAccess(FieldAccessExpr {
                            object: Box::new(expr),
                            field: member,
                        }),
                        span,
                    };
                }
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

    // ---------------------------------------------------------------
    // Struct literals
    // ---------------------------------------------------------------

    /// Look ahead to determine if `{` starts a struct literal (`{ ident: expr, ... }`).
    ///
    /// Returns true if the token after `{` is `ident :`, which disambiguates
    /// from block expressions.
    fn is_struct_literal_ahead(&self) -> bool {
        // Current token is `{`. Check pos+1 and pos+2.
        let after_brace = self.tokens.get(self.pos + 1).map(|t| &t.kind);
        let after_ident = self.tokens.get(self.pos + 2).map(|t| &t.kind);

        matches!(
            (after_brace, after_ident),
            (Some(TokenKind::Ident(_)), Some(TokenKind::Colon))
        )
    }

    /// Parse a struct literal: `{ name: expr, ... }`.
    ///
    /// The `type_name` is provided when the struct type is known from context
    /// (e.g., from a type annotation on the variable declaration).
    fn parse_struct_literal(&mut self, type_name: Option<Ident>) -> Option<Expr> {
        let open = self.advance(); // consume `{`
        let start = type_name.as_ref().map_or(open.span, |n| n.span);

        let mut fields = Vec::new();

        if !self.check(&TokenKind::RBrace) && !self.at_end() {
            loop {
                let field_start = self.current_token().span;
                let Some(name) = self.parse_ident() else {
                    break;
                };
                if self.expect(&TokenKind::Colon).is_none() {
                    break;
                }
                let Some(value) = self.parse_expr() else {
                    break;
                };
                let field_span = field_start.merge(value.span);
                fields.push(FieldInit {
                    name,
                    value,
                    span: field_span,
                });

                if !self.eat(&TokenKind::Comma) {
                    break;
                }

                // Allow trailing comma
                if self.check(&TokenKind::RBrace) {
                    break;
                }
            }
        }

        let close = self.expect(&TokenKind::RBrace)?;
        let span = start.merge(close.span);

        Some(Expr {
            kind: ExprKind::StructLit(StructLitExpr { type_name, fields }),
            span,
        })
    }

    // ---------------------------------------------------------------
    // Primary expressions
    // ---------------------------------------------------------------

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
            TokenKind::LBrace => {
                // Struct literal: `{ name: expr, ... }`
                // Disambiguate: look ahead for `ident :` pattern
                if self.is_struct_literal_ahead() {
                    return self.parse_struct_literal(None);
                }
                // Otherwise fall through to error (blocks are not expressions in Phase 1)
                self.diagnostics.push(
                    Diagnostic::error("unexpected `{` in expression position").with_label(
                        token.span,
                        self.file_id,
                        "expected expression",
                    ),
                );
                None
            }
            TokenKind::TemplateNoSub(_) => Some(self.parse_template_no_sub()),
            TokenKind::TemplateHead(_) => self.parse_template_literal(),
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

    // ---------------------------------------------------------------
    // Template literals
    // ---------------------------------------------------------------

    /// Parse a template literal with no interpolations: `` `text` ``.
    fn parse_template_no_sub(&mut self) -> Expr {
        let token = self.advance();
        let TokenKind::TemplateNoSub(text) = token.kind else {
            unreachable!("parse_template_no_sub called without TemplateNoSub token");
        };
        Expr {
            kind: ExprKind::TemplateLit(TemplateLitExpr {
                parts: vec![TemplatePart::String(text, token.span)],
            }),
            span: token.span,
        }
    }

    /// Parse a template literal with interpolations: `` `text${expr}text` ``.
    ///
    /// Consumes `TemplateHead`, expression tokens, and `TemplateMiddle`/`TemplateTail`
    /// tokens to build the complete `TemplateLitExpr`.
    fn parse_template_literal(&mut self) -> Option<Expr> {
        let head_token = self.advance();
        let start_span = head_token.span;
        let TokenKind::TemplateHead(head_text) = head_token.kind else {
            unreachable!("parse_template_literal called without TemplateHead token");
        };

        let mut parts = Vec::new();
        parts.push(TemplatePart::String(head_text, head_token.span));

        loop {
            // Parse the interpolated expression
            let expr = self.parse_expr()?;
            parts.push(TemplatePart::Expr(expr));

            // After the expression, expect TemplateMiddle or TemplateTail
            let next = self.current_token().clone();
            match &next.kind {
                TokenKind::TemplateTail(_) => {
                    let tail_token = self.advance();
                    let TokenKind::TemplateTail(tail_text) = tail_token.kind else {
                        unreachable!();
                    };
                    parts.push(TemplatePart::String(tail_text, tail_token.span));
                    let end_span = tail_token.span;
                    return Some(Expr {
                        kind: ExprKind::TemplateLit(TemplateLitExpr { parts }),
                        span: start_span.merge(end_span),
                    });
                }
                TokenKind::TemplateMiddle(_) => {
                    let mid_token = self.advance();
                    let TokenKind::TemplateMiddle(mid_text) = mid_token.kind else {
                        unreachable!();
                    };
                    parts.push(TemplatePart::String(mid_text, mid_token.span));
                    // Continue loop to parse next expression
                }
                _ => {
                    // Error — expected template continuation
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "expected template literal continuation, found {}",
                            Self::describe_kind(&next.kind)
                        ))
                        .with_label(
                            next.span,
                            self.file_id,
                            "expected template middle or tail",
                        ),
                    );
                    return None;
                }
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
        match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function item"),
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
            TypeKind::Generic(_, _) => panic!("expected Named, got Generic"),
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
        match &module.items[0].kind {
            ItemKind::Function(f) => assert_eq!(f.name.name, "a"),
            _ => panic!("expected function"),
        }
        match &module.items[1].kind {
            ItemKind::Function(f) => assert_eq!(f.name.name, "b"),
            _ => panic!("expected function"),
        }
        match &module.items[2].kind {
            ItemKind::Function(f) => assert_eq!(f.name.name, "c"),
            _ => panic!("expected function"),
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

    // ---------------------------------------------------------------
    // Compound assignment operators
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_compound_assign_plus_eq_desugars_to_binary() {
        let module = parse_ok("function f() { x += 1; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Assign(a) => {
                    assert_eq!(a.target.name, "x");
                    match &a.value.kind {
                        ExprKind::Binary(b) => {
                            assert_eq!(b.op, BinaryOp::Add);
                            match &b.left.kind {
                                ExprKind::Ident(id) => assert_eq!(id.name, "x"),
                                other => panic!("expected Ident(x), got {other:?}"),
                            }
                            match &b.right.kind {
                                ExprKind::IntLit(1) => {}
                                other => panic!("expected IntLit(1), got {other:?}"),
                            }
                        }
                        other => panic!("expected Binary(Add), got {other:?}"),
                    }
                }
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_compound_assign_all_operators() {
        let cases = [
            ("x += 1;", BinaryOp::Add),
            ("x -= 1;", BinaryOp::Sub),
            ("x *= 1;", BinaryOp::Mul),
            ("x /= 1;", BinaryOp::Div),
            ("x %= 1;", BinaryOp::Mod),
        ];

        for (source, expected_op) in cases {
            let full = format!("function f() {{ {source} }}");
            let module = parse_ok(&full);
            let f = first_fn(&module);
            let stmt = first_stmt(f);
            match stmt {
                Stmt::Expr(e) => match &e.kind {
                    ExprKind::Assign(a) => match &a.value.kind {
                        ExprKind::Binary(b) => assert_eq!(
                            b.op, expected_op,
                            "compound assign `{source}` should desugar to {expected_op}"
                        ),
                        other => panic!("expected Binary for `{source}`, got {other:?}"),
                    },
                    other => panic!("expected Assign for `{source}`, got {other:?}"),
                },
                other => panic!("expected Expr for `{source}`, got {other:?}"),
            }
        }
    }

    // ---------------------------------------------------------------
    // Recursion depth limit
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_deeply_nested_expr_produces_diagnostic() {
        // Build a deeply nested expression: (((((...(1)...)))))
        // with nesting > MAX_EXPR_DEPTH (64)
        let depth = 66;
        let open = "(".repeat(depth);
        let close = ")".repeat(depth);
        let source = format!("function f() {{ {open}1{close}; }}");
        let (_, diagnostics) = parse_source(&source);
        assert!(
            !diagnostics.is_empty(),
            "should produce a diagnostic for depth > {MAX_EXPR_DEPTH}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("nesting depth")),
            "diagnostic should mention nesting depth"
        );
    }

    #[test]
    fn test_parser_expr_at_max_depth_minus_one_still_parses() {
        // Build a nested expression just under the limit
        let depth = 63;
        let open = "(".repeat(depth);
        let close = ")".repeat(depth);
        let source = format!("function f() {{ {open}1{close}; }}");
        let (module, diagnostics) = parse_source(&source);
        assert!(
            diagnostics.is_empty(),
            "depth {depth} should not produce diagnostics, got: {diagnostics:?}"
        );
        assert_eq!(module.items.len(), 1);
    }

    // ---------------------------------------------------------------
    // Task 014: Type definitions and struct sugar
    // ---------------------------------------------------------------

    // Test T14-1: Parse `type User = { name: string, age: u32 }` -> TypeDef with 2 fields
    #[test]
    fn test_parser_type_def_two_fields() {
        let source = "type User = { name: string, age: u32 }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "User");
                assert_eq!(td.fields.len(), 2);
                assert_eq!(td.fields[0].name.name, "name");
                assert_eq!(td.fields[1].name.name, "age");
            }
            _ => panic!("expected TypeDef"),
        }
    }

    // Test T14-2: Parse struct literal in variable initializer
    #[test]
    fn test_parser_struct_literal_in_var_decl() {
        let source = r#"function main() { const user: User = { name: "Alice", age: 30 }; }"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(decl) => {
                assert_eq!(decl.name.name, "user");
                match &decl.init.kind {
                    ExprKind::StructLit(slit) => {
                        assert_eq!(slit.fields.len(), 2);
                        assert_eq!(slit.fields[0].name.name, "name");
                        assert_eq!(slit.fields[1].name.name, "age");
                    }
                    _ => panic!("expected StructLit expression"),
                }
            }
            _ => panic!("expected VarDecl"),
        }
    }

    // Test T14-3: Parse field access `user.name`
    #[test]
    fn test_parser_field_access() {
        let source = "function main() { console.log(user.name); }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        // The statement should contain a method call whose argument is a field access
        match stmt {
            Stmt::Expr(expr) => match &expr.kind {
                ExprKind::MethodCall(mc) => {
                    assert_eq!(mc.args.len(), 1);
                    match &mc.args[0].kind {
                        ExprKind::FieldAccess(fa) => {
                            assert_eq!(fa.field.name, "name");
                        }
                        _ => panic!("expected FieldAccess in arg"),
                    }
                }
                _ => panic!("expected MethodCall"),
            },
            _ => panic!("expected Expr statement"),
        }
    }

    // Test T14-4: Parse chained field access `user.address.city`
    #[test]
    fn test_parser_chained_field_access() {
        let source = "function main() { console.log(user.address.city); }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(expr) => match &expr.kind {
                ExprKind::MethodCall(mc) => {
                    assert_eq!(mc.args.len(), 1);
                    match &mc.args[0].kind {
                        ExprKind::FieldAccess(outer) => {
                            assert_eq!(outer.field.name, "city");
                            match &outer.object.kind {
                                ExprKind::FieldAccess(inner) => {
                                    assert_eq!(inner.field.name, "address");
                                }
                                _ => panic!("expected inner FieldAccess"),
                            }
                        }
                        _ => panic!("expected FieldAccess"),
                    }
                }
                _ => panic!("expected MethodCall"),
            },
            _ => panic!("expected Expr statement"),
        }
    }

    // Test T14-5: Parse destructuring `const { name, age } = user;`
    #[test]
    fn test_parser_destructuring() {
        let source = "function main() { const { name, age } = user; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Destructure(destr) => {
                assert_eq!(destr.binding, VarBinding::Const);
                assert_eq!(destr.fields.len(), 2);
                assert_eq!(destr.fields[0].name, "name");
                assert_eq!(destr.fields[1].name, "age");
                match &destr.init.kind {
                    ExprKind::Ident(ident) => assert_eq!(ident.name, "user"),
                    _ => panic!("expected Ident init"),
                }
            }
            _ => panic!("expected Destructure statement"),
        }
    }

    // Test T14-6: Parse type def with trailing comma
    #[test]
    fn test_parser_type_def_trailing_comma() {
        let source = "type Point = { x: f64, y: f64, }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.fields.len(), 2);
            }
            _ => panic!("expected TypeDef"),
        }
    }

    // Test T14-7: type keyword is lexed correctly
    #[test]
    fn test_parser_type_keyword_in_lexer() {
        let source = "type Foo = { x: i32 }";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => assert_eq!(td.name.name, "Foo"),
            _ => panic!("expected TypeDef"),
        }
    }

    // ---- Task 016: Generics ----

    // Test T16-1: Parse `function id<T>(x: T): T`
    #[test]
    fn test_parser_generic_fn_single_type_param() {
        let source = "function id<T>(x: T): T { return x; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "id");
        let tp = f.type_params.as_ref().expect("expected type params");
        assert_eq!(tp.params.len(), 1);
        assert_eq!(tp.params[0].name.name, "T");
        assert!(tp.params[0].constraint.is_none());
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].name.name, "x");
    }

    // Test T16-2: Parse `function merge<T extends Comparable>(a: T, b: T): T`
    #[test]
    fn test_parser_generic_fn_constrained_type_param() {
        let source = "function merge<T extends Comparable>(a: T, b: T): T { return a; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "merge");
        let tp = f.type_params.as_ref().expect("expected type params");
        assert_eq!(tp.params.len(), 1);
        assert_eq!(tp.params[0].name.name, "T");
        let constraint = tp.params[0]
            .constraint
            .as_ref()
            .expect("expected constraint");
        match &constraint.kind {
            TypeKind::Named(ident) => assert_eq!(ident.name, "Comparable"),
            _ => panic!("expected Named constraint"),
        }
    }

    // Test T16-3: Parse `type Container<T> = { value: T }`
    #[test]
    fn test_parser_generic_type_def() {
        let source = "type Container<T> = { value: T }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "Container");
                let tp = td.type_params.as_ref().expect("expected type params");
                assert_eq!(tp.params.len(), 1);
                assert_eq!(tp.params[0].name.name, "T");
                assert_eq!(td.fields.len(), 1);
                assert_eq!(td.fields[0].name.name, "value");
                match &td.fields[0].type_ann.kind {
                    TypeKind::Named(ident) => assert_eq!(ident.name, "T"),
                    _ => panic!("expected Named type T for field"),
                }
            }
            _ => panic!("expected TypeDef"),
        }
    }

    // Test T16-4: Parse `const x: Array<string>` → TypeKind::Generic
    #[test]
    fn test_parser_generic_type_annotation_single_arg() {
        let source = "function main() { const x: Array<string> = 0; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = &f.body.stmts[0];
        match stmt {
            Stmt::VarDecl(vd) => {
                let ann = vd.type_ann.as_ref().expect("expected type ann");
                match &ann.kind {
                    TypeKind::Generic(ident, args) => {
                        assert_eq!(ident.name, "Array");
                        assert_eq!(args.len(), 1);
                        match &args[0].kind {
                            TypeKind::Named(n) => assert_eq!(n.name, "string"),
                            _ => panic!("expected Named string arg"),
                        }
                    }
                    _ => panic!("expected Generic type annotation"),
                }
            }
            _ => panic!("expected VarDecl"),
        }
    }

    // Test T16-5: Parse `const m: Map<string, u32>` → generic type with 2 args
    #[test]
    fn test_parser_generic_type_annotation_two_args() {
        let source = "function main() { const m: Map<string, u32> = 0; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = &f.body.stmts[0];
        match stmt {
            Stmt::VarDecl(vd) => {
                let ann = vd.type_ann.as_ref().expect("expected type ann");
                match &ann.kind {
                    TypeKind::Generic(ident, args) => {
                        assert_eq!(ident.name, "Map");
                        assert_eq!(args.len(), 2);
                        match &args[0].kind {
                            TypeKind::Named(n) => assert_eq!(n.name, "string"),
                            _ => panic!("expected Named string"),
                        }
                        match &args[1].kind {
                            TypeKind::Named(n) => assert_eq!(n.name, "u32"),
                            _ => panic!("expected Named u32"),
                        }
                    }
                    _ => panic!("expected Generic type annotation"),
                }
            }
            _ => panic!("expected VarDecl"),
        }
    }

    // Test T16-6: Multiple generic type params: `function swap<T, U>(a: T, b: U): T`
    #[test]
    fn test_parser_generic_fn_multiple_type_params() {
        let source = "function swap<T, U>(a: T, b: U): T { return a; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let tp = f.type_params.as_ref().expect("expected type params");
        assert_eq!(tp.params.len(), 2);
        assert_eq!(tp.params[0].name.name, "T");
        assert_eq!(tp.params[1].name.name, "U");
    }

    // Test T16-7: Non-generic function has type_params = None
    #[test]
    fn test_parser_non_generic_fn_has_no_type_params() {
        let source = "function add(a: i32, b: i32): i32 { return a; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert!(f.type_params.is_none());
    }

    // Test T16-8: extends keyword lexes correctly
    #[test]
    fn test_parser_extends_keyword_in_generics() {
        let source = "function max<T extends PartialOrd>(a: T, b: T): T { return a; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let tp = f.type_params.as_ref().expect("expected type params");
        assert_eq!(tp.params[0].name.name, "T");
        let constraint = tp.params[0]
            .constraint
            .as_ref()
            .expect("expected constraint");
        match &constraint.kind {
            TypeKind::Named(ident) => assert_eq!(ident.name, "PartialOrd"),
            _ => panic!("expected Named constraint"),
        }
    }

    // ---------------------------------------------------------------
    // Template literal parsing tests
    // ---------------------------------------------------------------

    // Test: Parse template literal with no interpolation
    #[test]
    fn test_parser_template_no_interpolation_produces_single_string_part() {
        let module = parse_ok("function main() { const x = `hello`; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        let ExprKind::TemplateLit(tpl) = &decl.init.kind else {
            panic!("expected TemplateLit, got {:?}", decl.init.kind);
        };
        assert_eq!(tpl.parts.len(), 1);
        match &tpl.parts[0] {
            TemplatePart::String(s, _) => assert_eq!(s, "hello"),
            _ => panic!("expected String part"),
        }
    }

    // Test: Parse template literal with single interpolation
    #[test]
    fn test_parser_template_single_interpolation_produces_three_parts() {
        let module = parse_ok("function main() { const x = `Hello, ${name}!`; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        let ExprKind::TemplateLit(tpl) = &decl.init.kind else {
            panic!("expected TemplateLit, got {:?}", decl.init.kind);
        };
        // parts: String("Hello, "), Expr(name), String("!")
        assert_eq!(tpl.parts.len(), 3);
        match &tpl.parts[0] {
            TemplatePart::String(s, _) => assert_eq!(s, "Hello, "),
            _ => panic!("expected String part"),
        }
        match &tpl.parts[1] {
            TemplatePart::Expr(e) => {
                assert!(matches!(&e.kind, ExprKind::Ident(ident) if ident.name == "name"));
            }
            _ => panic!("expected Expr part"),
        }
        match &tpl.parts[2] {
            TemplatePart::String(s, _) => assert_eq!(s, "!"),
            _ => panic!("expected String part"),
        }
    }

    // Test: Parse template literal with expression interpolation
    #[test]
    fn test_parser_template_expression_interpolation_parses_binary() {
        let module = parse_ok("function main() { const x = `${a + b}`; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        let ExprKind::TemplateLit(tpl) = &decl.init.kind else {
            panic!("expected TemplateLit, got {:?}", decl.init.kind);
        };
        // parts: String(""), Expr(a + b), String("")
        assert_eq!(tpl.parts.len(), 3);
        match &tpl.parts[1] {
            TemplatePart::Expr(e) => {
                assert!(matches!(&e.kind, ExprKind::Binary(_)));
            }
            _ => panic!("expected Expr part"),
        }
    }

    // Test: Parse template literal with multiple interpolations
    #[test]
    fn test_parser_template_multiple_interpolations_five_parts() {
        let module = parse_ok("function main() { const x = `${a} + ${b} = ${c}`; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        let ExprKind::TemplateLit(tpl) = &decl.init.kind else {
            panic!("expected TemplateLit, got {:?}", decl.init.kind);
        };
        // parts: String(""), Expr(a), String(" + "), Expr(b), String(" = "), Expr(c), String("")
        assert_eq!(tpl.parts.len(), 7);
        match &tpl.parts[2] {
            TemplatePart::String(s, _) => assert_eq!(s, " + "),
            _ => panic!("expected String part"),
        }
        match &tpl.parts[4] {
            TemplatePart::String(s, _) => assert_eq!(s, " = "),
            _ => panic!("expected String part"),
        }
    }
}
