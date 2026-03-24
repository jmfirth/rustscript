//! Recursive descent parser for `RustScript` source files.
//!
//! Consumes the token stream from the lexer and produces a [`rsc_syntax::ast::Module`].
//! Implements error recovery at statement boundaries so that parsing continues
//! past syntax errors, accumulating diagnostics along the way.

use rsc_syntax::ast::{
    AssignExpr, BinaryExpr, BinaryOp, Block, CallExpr, ClosureBody, ClosureExpr, DestructureStmt,
    ElseClause, EnumDef, EnumVariant, Expr, ExprKind, FieldAccessExpr, FieldDef, FieldInit, FnDecl,
    Ident, IfStmt, IndexExpr, InterfaceDef, InterfaceMethod, Item, ItemKind, MethodCallExpr,
    Module, NewExpr, NullishCoalescingExpr, OptionalAccess, OptionalChainExpr, Param, ReturnStmt,
    ReturnTypeAnnotation, Stmt, StructLitExpr, SwitchCase, SwitchStmt, TemplateLitExpr,
    TemplatePart, TryCatchStmt, TypeAnnotation, TypeDef, TypeKind, TypeParam, TypeParams,
    UnaryExpr, UnaryOp, VarBinding, VarDecl, WhileStmt,
};
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::source::FileId;
use rsc_syntax::span::Span;

use crate::token::{Token, TokenKind};

/// Maximum nesting depth for expressions to prevent stack overflow on
/// adversarial input (e.g., deeply nested parentheses).
///
/// Set conservatively to account for the full precedence chain per depth
/// level in debug builds. Each expression depth level uses ~12 stack
/// frames through the precedence hierarchy, including arrow function
/// disambiguation lookahead.
const MAX_EXPR_DEPTH: usize = 50;

/// Capitalize the first letter of a string.
///
/// Used to derive Rust enum variant names from `RustScript` string literals
/// (e.g., `"north"` → `"North"`).
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
            TokenKind::Switch => "`switch`",
            TokenKind::Case => "`case`",
            TokenKind::New => "`new`",
            TokenKind::Null => "`null`",
            TokenKind::Throw => "`throw`",
            TokenKind::Throws => "`throws`",
            TokenKind::Try => "`try`",
            TokenKind::Catch => "`catch`",
            TokenKind::Move => "`move`",
            TokenKind::Interface => "`interface`",
            TokenKind::FatArrow => "`=>`",
            TokenKind::Ampersand => "`&`",
            TokenKind::Pipe => "`|`",
            TokenKind::QuestionDot => "`?.`",
            TokenKind::QuestionQuestion => "`??`",
            TokenKind::EqEqEq => "`===`",
            TokenKind::BangEqEq => "`!==`",
            TokenKind::LBracket => "`[`",
            TokenKind::RBracket => "`]`",
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
                | TokenKind::Type
                | TokenKind::Switch
                | TokenKind::Try
                | TokenKind::Throw
                | TokenKind::Interface => return,
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

    /// Parse a top-level item: function declaration, type definition, or interface.
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
            TokenKind::Type => self.parse_type_or_enum_def(),
            TokenKind::Interface => self.parse_interface_def().map(|iface| {
                let span = iface.span;
                Item {
                    kind: ItemKind::Interface(iface),
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

        // Optional return type with throws
        let return_type = self.parse_return_type_annotation();
        // If parse_return_type_annotation started consuming tokens (colon/throws)
        // but failed to parse the type, propagate the error.
        // (Check handled inside the method via diagnostics.)

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

    /// Parse an optional return type annotation with optional throws.
    ///
    /// Handles:
    /// - No annotation: `function foo() { ... }` → `None`
    /// - Type only: `: ReturnType` → `Some(ReturnTypeAnnotation { type_ann: Some(..), throws: None })`
    /// - Type + throws: `: ReturnType throws ErrorType`
    /// - Throws only: `throws ErrorType` (no colon, no return type — void success)
    ///
    /// Returns `None` when no annotation is present.
    fn parse_return_type_annotation(&mut self) -> Option<ReturnTypeAnnotation> {
        let has_colon = self.eat(&TokenKind::Colon);

        if has_colon {
            let type_ann = self.parse_type_annotation()?;
            let start_span = type_ann.span;

            // Check for `throws ErrorType`
            if self.check(&TokenKind::Throws) {
                self.advance(); // consume `throws`
                let throws_type = self.parse_type_annotation()?;
                let end_span = throws_type.span;
                return Some(ReturnTypeAnnotation {
                    type_ann: Some(type_ann),
                    throws: Some(throws_type),
                    span: start_span.merge(end_span),
                });
            }

            return Some(ReturnTypeAnnotation {
                type_ann: Some(type_ann.clone()),
                throws: None,
                span: type_ann.span,
            });
        }

        // Check for `throws ErrorType` without return type (void + throws)
        if self.check(&TokenKind::Throws) {
            let throws_token = self.advance(); // consume `throws`
            let start_span = throws_token.span;
            let throws_type = self.parse_type_annotation()?;
            let end_span = throws_type.span;
            return Some(ReturnTypeAnnotation {
                type_ann: None,
                throws: Some(throws_type),
                span: start_span.merge(end_span),
            });
        }

        None
    }

    // ---------------------------------------------------------------
    // Type definitions
    // ---------------------------------------------------------------

    /// Disambiguate and parse a type definition or enum definition.
    ///
    /// After `type Name =`, the next token determines what we're parsing:
    /// - `{` followed by `ident :` → struct type def
    /// - String literal → simple enum
    /// - `|` → data enum (discriminated union)
    fn parse_type_or_enum_def(&mut self) -> Option<Item> {
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

        // Disambiguate: string literal → simple enum, `|` → data enum, `{` → struct
        match self.peek() {
            TokenKind::StringLit(_) => {
                // Simple enum: type Name = "a" | "b" | "c"
                let enum_def = self.parse_simple_enum(name, start)?;
                let span = enum_def.span;
                Some(Item {
                    kind: ItemKind::EnumDef(enum_def),
                    exported: false,
                    span,
                })
            }
            TokenKind::Pipe => {
                // Data enum: type Name = | { kind: "a", ... } | { kind: "b", ... }
                let enum_def = self.parse_data_enum(name, start)?;
                let span = enum_def.span;
                Some(Item {
                    kind: ItemKind::EnumDef(enum_def),
                    exported: false,
                    span,
                })
            }
            _ => {
                // Struct type def: type Name = { field: Type, ... }
                self.expect(&TokenKind::LBrace)?;
                let fields = self.parse_field_def_list();
                let close = self.expect(&TokenKind::RBrace)?;
                let span = start.merge(close.span);
                let td = TypeDef {
                    name,
                    type_params,
                    fields,
                    span,
                };
                Some(Item {
                    kind: ItemKind::TypeDef(td),
                    exported: false,
                    span,
                })
            }
        }
    }

    /// Parse a simple enum: `"a" | "b" | "c"`.
    ///
    /// Called after `type Name =` has been consumed.
    fn parse_simple_enum(&mut self, name: Ident, start: Span) -> Option<EnumDef> {
        let mut variants = Vec::new();

        loop {
            let token = self.current_token().clone();
            if let TokenKind::StringLit(value) = &token.kind {
                let variant_name = capitalize_first(&value.clone());
                self.advance();
                variants.push(EnumVariant::Simple(
                    Ident {
                        name: variant_name,
                        span: token.span,
                    },
                    token.span,
                ));
            } else {
                self.diagnostics.push(
                    Diagnostic::error("expected string literal for enum variant").with_label(
                        token.span,
                        self.file_id,
                        "expected string literal",
                    ),
                );
                return None;
            }

            if !self.eat(&TokenKind::Pipe) {
                break;
            }
        }

        let span = start.merge(self.previous_span());
        Some(EnumDef {
            name,
            variants,
            span,
        })
    }

    /// Parse a data enum (discriminated union): `| { kind: "a", ... } | { kind: "b", ... }`.
    ///
    /// Called after `type Name =` has been consumed, with `|` as current token.
    fn parse_data_enum(&mut self, name: Ident, start: Span) -> Option<EnumDef> {
        let mut variants = Vec::new();

        while self.eat(&TokenKind::Pipe) {
            self.expect(&TokenKind::LBrace)?;

            // First field must be the discriminant: `kind: "value"`
            let kind_ident = self.parse_ident()?;
            if kind_ident.name != "kind" {
                self.diagnostics.push(
                    Diagnostic::error("data enum variants must start with a `kind` discriminant")
                        .with_label(kind_ident.span, self.file_id, "expected `kind`"),
                );
                return None;
            }
            self.expect(&TokenKind::Colon)?;

            let disc_token = self.current_token().clone();
            let discriminant_value =
                if let TokenKind::StringLit(value) = &disc_token.kind {
                    let v = value.clone();
                    self.advance();
                    v
                } else {
                    self.diagnostics.push(
                        Diagnostic::error("expected string literal for discriminant value")
                            .with_label(disc_token.span, self.file_id, "expected string literal"),
                    );
                    return None;
                };

            let variant_name = capitalize_first(&discriminant_value);
            let variant_start = kind_ident.span;

            // Parse remaining data fields after comma
            let mut fields = Vec::new();
            while self.eat(&TokenKind::Comma) {
                // Allow trailing comma before `}`
                if self.check(&TokenKind::RBrace) {
                    break;
                }
                let field_start = self.current_token().span;
                let field_name = self.parse_ident()?;
                self.expect(&TokenKind::Colon)?;
                let type_ann = self.parse_type_annotation()?;
                let field_span = field_start.merge(type_ann.span);
                fields.push(FieldDef {
                    name: field_name,
                    type_ann,
                    span: field_span,
                });
            }

            let close = self.expect(&TokenKind::RBrace)?;
            let variant_span = variant_start.merge(close.span);

            variants.push(EnumVariant::Data {
                discriminant_value,
                name: Ident {
                    name: variant_name,
                    span: variant_start,
                },
                fields,
                span: variant_span,
            });
        }

        let span = start.merge(self.previous_span());
        Some(EnumDef {
            name,
            variants,
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
    // Interface definitions
    // ---------------------------------------------------------------

    /// Parse an interface definition: `interface Name<T> { method(): Type; ... }`.
    fn parse_interface_def(&mut self) -> Option<InterfaceDef> {
        let interface_token = self.advance(); // consume `interface`
        let start = interface_token.span;

        let name = self.parse_ident()?;

        // Optional generic type parameters: `<T, U extends Clone>`
        let type_params = if self.check(&TokenKind::Lt) {
            Some(self.parse_type_params()?)
        } else {
            None
        };

        self.expect(&TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            if let Some(method) = self.parse_interface_method() {
                methods.push(method);
            } else {
                // Error recovery: skip to next semicolon or closing brace
                while !self.at_end()
                    && !self.check(&TokenKind::Semicolon)
                    && !self.check(&TokenKind::RBrace)
                {
                    self.advance();
                }
                self.eat(&TokenKind::Semicolon);
            }
        }

        let close = self.expect(&TokenKind::RBrace)?;
        let span = start.merge(close.span);

        Some(InterfaceDef {
            name,
            type_params,
            methods,
            span,
        })
    }

    /// Parse a single interface method signature: `name(params): ReturnType;`.
    fn parse_interface_method(&mut self) -> Option<InterfaceMethod> {
        let start = self.current_token().span;
        let name = self.parse_ident()?;

        self.expect(&TokenKind::LParen)?;
        let params = self.parse_param_list();
        self.expect(&TokenKind::RParen)?;

        // Optional return type
        let return_type = self.parse_return_type_annotation();

        self.expect(&TokenKind::Semicolon)?;

        let span = start.merge(self.previous_span());
        Some(InterfaceMethod {
            name,
            params,
            return_type,
            span,
        })
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

    /// Parse a type annotation: `void`, a named type, a generic type, a union type,
    /// or an intersection type.
    ///
    /// Handles `void`, `i32`, `Container<T>`, `Map<string, u32>`, `T | null`,
    /// `Serializable & Printable`, etc.
    fn parse_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let base = self.parse_base_type_annotation()?;

        // Check for union type: `T | null`
        if self.check(&TokenKind::Pipe) {
            let start_span = base.span;
            let mut members = vec![base];

            while self.eat(&TokenKind::Pipe) {
                let member = self.parse_base_type_annotation()?;
                members.push(member);
            }

            let end_span = members.last().map_or(start_span, |m| m.span);
            return Some(TypeAnnotation {
                kind: TypeKind::Union(members),
                span: start_span.merge(end_span),
            });
        }

        // Check for intersection type: `Serializable & Printable`
        if self.check(&TokenKind::Ampersand) {
            let start_span = base.span;
            let mut members = vec![base];

            while self.eat(&TokenKind::Ampersand) {
                let member = self.parse_base_type_annotation()?;
                members.push(member);
            }

            let end_span = members.last().map_or(start_span, |m| m.span);
            return Some(TypeAnnotation {
                kind: TypeKind::Intersection(members),
                span: start_span.merge(end_span),
            });
        }

        Some(base)
    }

    /// Parse a base (non-union) type annotation: `void`, named, generic, `null`, or function type.
    fn parse_base_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let token = self.current_token().clone();
        match &token.kind {
            TokenKind::LParen => {
                // Function type: `(i32, i32) => i32`
                self.parse_function_type_annotation()
            }
            TokenKind::Null => {
                self.advance();
                // Represent `null` as a named type "null" in the union — the
                // lowering pass detects this pattern.
                Some(TypeAnnotation {
                    kind: TypeKind::Named(Ident {
                        name: "null".to_owned(),
                        span: token.span,
                    }),
                    span: token.span,
                })
            }
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

    /// Parse a function type annotation: `(i32, string) => i32`.
    ///
    /// Called when `(` is seen in type annotation position.
    fn parse_function_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let start = self.current_token().span;
        self.advance(); // consume `(`

        let mut param_types = Vec::new();
        if !self.check(&TokenKind::RParen) && !self.at_end() {
            loop {
                let ty = self.parse_type_annotation()?;
                param_types.push(ty);

                if !self.eat(&TokenKind::Comma) {
                    break;
                }

                // Allow trailing comma
                if self.check(&TokenKind::RParen) {
                    break;
                }
            }
        }

        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::FatArrow)?;
        let return_type = self.parse_type_annotation()?;
        let span = start.merge(return_type.span);

        Some(TypeAnnotation {
            kind: TypeKind::Function(param_types, Box::new(return_type)),
            span,
        })
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
            TokenKind::Switch => self.parse_switch_stmt().map(Stmt::Switch),
            TokenKind::Try => self.parse_try_catch_stmt().map(Stmt::TryCatch),
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

    /// Parse a switch statement: `switch (expr) { case "v": stmts; ... }`.
    fn parse_switch_stmt(&mut self) -> Option<SwitchStmt> {
        let switch_token = self.advance(); // consume `switch`
        let start = switch_token.span;

        self.expect(&TokenKind::LParen)?;
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::RParen)?;

        self.expect(&TokenKind::LBrace)?;

        let mut cases = Vec::new();
        while self.check(&TokenKind::Case) {
            let case_token = self.advance(); // consume `case`
            let case_start = case_token.span;

            let pattern_token = self.current_token().clone();
            let pattern = if let TokenKind::StringLit(value) = &pattern_token.kind {
                let v = value.clone();
                self.advance();
                v
            } else {
                self.diagnostics.push(
                    Diagnostic::error("expected string literal for case pattern").with_label(
                        pattern_token.span,
                        self.file_id,
                        "expected string literal",
                    ),
                );
                return None;
            };

            self.expect(&TokenKind::Colon)?;

            // Parse the body: statements until next `case` or `}`
            let mut body = Vec::new();
            while !self.check(&TokenKind::Case) && !self.check(&TokenKind::RBrace) && !self.at_end()
            {
                if let Some(stmt) = self.parse_stmt() {
                    body.push(stmt);
                }
            }

            let case_end = self.previous_span();
            cases.push(SwitchCase {
                pattern,
                body,
                span: case_start.merge(case_end),
            });
        }

        let close = self.expect(&TokenKind::RBrace)?;
        Some(SwitchStmt {
            scrutinee,
            cases,
            span: start.merge(close.span),
        })
    }

    /// Parse a try/catch statement: `try { ... } catch (name: Type) { ... }`.
    fn parse_try_catch_stmt(&mut self) -> Option<TryCatchStmt> {
        let try_token = self.advance(); // consume `try`
        let start = try_token.span;

        let try_block = self.parse_block()?;

        self.expect(&TokenKind::Catch)?;
        self.expect(&TokenKind::LParen)?;
        let catch_binding = self.parse_ident()?;

        // Optional type annotation on catch binding
        let catch_type = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        self.expect(&TokenKind::RParen)?;
        let catch_block = self.parse_block()?;
        let end = catch_block.span;

        Some(TryCatchStmt {
            try_block,
            catch_binding,
            catch_type,
            catch_block,
            span: start.merge(end),
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

    /// Parse assignment: `IDENT = assignment | IDENT op= assignment | nullish_coalesce`.
    ///
    /// Assignment is right-associative: `a = b = c` parses as `a = (b = c)`.
    /// Compound assignments (`+=`, `-=`, etc.) are desugared to `x = x op rhs`.
    fn parse_assignment(&mut self) -> Option<Expr> {
        let expr = self.parse_nullish_coalesce()?;

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

    /// Parse nullish coalescing: `logic_or ( "??" logic_or )*`.
    ///
    /// `??` has lower precedence than `||`, so `a || b ?? c` is `(a || b) ?? c`.
    fn parse_nullish_coalesce(&mut self) -> Option<Expr> {
        let mut left = self.parse_logic_or()?;

        while self.check(&TokenKind::QuestionQuestion) {
            self.advance();
            let right = self.parse_logic_or()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::NullishCoalescing(NullishCoalescingExpr {
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
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

    /// Parse equality: `comparison ( ("==" | "!=" | "===" | "!==") comparison )*`.
    fn parse_equality(&mut self) -> Option<Expr> {
        let mut left = self.parse_comparison()?;

        loop {
            let op = match self.peek() {
                TokenKind::EqEq | TokenKind::EqEqEq => BinaryOp::Eq,
                TokenKind::BangEq | TokenKind::BangEqEq => BinaryOp::Ne,
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

    /// Parse unary: `("-" | "!") unary | "throw" expr | call`.
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
            TokenKind::Throw => {
                let throw_token = self.advance();
                let value = self.parse_unary()?;
                let span = throw_token.span.merge(value.span);
                Some(Expr {
                    kind: ExprKind::Throw(Box::new(value)),
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
            } else if self.check(&TokenKind::LBracket) {
                // Index access: `expr[index]`
                self.advance(); // consume `[`
                let index = self.parse_expr()?;
                let close = self.expect(&TokenKind::RBracket)?;
                let span = expr.span.merge(close.span);
                expr = Expr {
                    kind: ExprKind::Index(IndexExpr {
                        object: Box::new(expr),
                        index: Box::new(index),
                    }),
                    span,
                };
            } else if self.check(&TokenKind::QuestionDot) {
                self.advance(); // consume `?.`
                let member = self.parse_ident()?;

                if self.check(&TokenKind::LParen) {
                    // Optional method call: `?.method(args)`
                    self.advance(); // consume `(`
                    let args = self.parse_arg_list();
                    let close = self.expect(&TokenKind::RParen)?;
                    let span = expr.span.merge(close.span);
                    expr = Expr {
                        kind: ExprKind::OptionalChain(OptionalChainExpr {
                            object: Box::new(expr),
                            access: OptionalAccess::Method(member, args),
                        }),
                        span,
                    };
                } else {
                    // Optional field access: `?.field`
                    let span = expr.span.merge(member.span);
                    expr = Expr {
                        kind: ExprKind::OptionalChain(OptionalChainExpr {
                            object: Box::new(expr),
                            access: OptionalAccess::Field(member),
                        }),
                        span,
                    };
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
    // Arrow functions (closures)
    // ---------------------------------------------------------------

    /// Determine whether the current token sequence starts an arrow function.
    ///
    /// Uses a non-recursive scan through the token stream. Scans from `(`
    /// through a parameter list, checks for `)` optionally followed by
    /// `: ReturnType` and then `=>`. Never calls recursive parse functions
    /// to avoid stack overflow on deeply nested parenthesized expressions.
    fn is_arrow_function_ahead(&self) -> bool {
        // If we see `move`, it's always an arrow function
        if self.check(&TokenKind::Move) {
            return true;
        }

        // Must start with `(`
        if !self.check(&TokenKind::LParen) {
            return false;
        }

        let mut i = self.pos + 1; // skip past `(`

        // Scan past the parameter list to find the matching `)`
        // Parameters are: ident [: type], ident [: type], ...
        // We need to handle nested `<>` for generic types and nested `()`
        // for function types in annotations.

        // Empty parens: `() => ...`
        if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::RParen) {
            i += 1;
            // Optional return type: `: Type`
            if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::Colon) {
                i += 1;
                // Skip through the return type tokens to `=>`
                i = self.skip_type_tokens(i);
            }
            return self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::FatArrow);
        }

        // Non-empty params: scan for pattern `ident :` at start
        // If the first token after `(` is not an ident, or the second is
        // not `:` or `,` or `)`, this is not an arrow function.
        if !matches!(
            self.tokens.get(i).map(|t| &t.kind),
            Some(TokenKind::Ident(_))
        ) {
            return false;
        }

        // Quick heuristic: check if second token after ident is `:` or `,` or `)`
        let after_ident = self.tokens.get(i + 1).map(|t| &t.kind);
        let looks_like_param = matches!(
            after_ident,
            Some(TokenKind::Colon | TokenKind::Comma | TokenKind::RParen)
        );
        if !looks_like_param {
            return false;
        }

        // Scan to matching `)`, tracking nesting depth for `<>` and `()`
        let mut paren_depth: u32 = 1; // we already consumed the opening `(`
        while let Some(token) = self.tokens.get(i) {
            match &token.kind {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        i += 1; // skip past `)`
                        break;
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
            i += 1;
        }

        if paren_depth != 0 {
            return false;
        }

        // Optional return type annotation: `: Type`
        if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::Colon) {
            i += 1;
            i = self.skip_type_tokens(i);
        }

        self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::FatArrow)
    }

    /// Skip over type annotation tokens starting at position `i`.
    ///
    /// Handles nested `<>` for generic types. Returns the position of the
    /// first token after the type. Used only by `is_arrow_function_ahead`.
    fn skip_type_tokens(&self, mut i: usize) -> usize {
        // Skip the base type name
        match self.tokens.get(i).map(|t| &t.kind) {
            Some(TokenKind::Ident(_)) => i += 1,
            Some(TokenKind::LParen) => {
                // Function type in return position: `(i32) => i32`
                // Skip past matching `)`
                let mut depth: u32 = 1;
                i += 1;
                while let Some(token) = self.tokens.get(i) {
                    match &token.kind {
                        TokenKind::LParen => depth += 1,
                        TokenKind::RParen => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        TokenKind::Eof => return i,
                        _ => {}
                    }
                    i += 1;
                }
                // After `)`, expect `=>` and then the return type
                if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::FatArrow) {
                    i += 1;
                    return self.skip_type_tokens(i);
                }
                return i;
            }
            _ => return i,
        }

        // Check for generic args: `<...>`
        if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::Lt) {
            let mut depth: u32 = 1;
            i += 1;
            while let Some(token) = self.tokens.get(i) {
                match &token.kind {
                    TokenKind::Lt => depth += 1,
                    TokenKind::Gt => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    TokenKind::Eof => return i,
                    _ => {}
                }
                i += 1;
            }
        }

        // Check for union type: `| null`
        while self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::Pipe) {
            i += 1; // skip `|`
            i = self.skip_type_tokens(i);
        }

        i
    }

    /// Parse an arrow function (closure) expression.
    ///
    /// Syntax:
    /// - `(params): ReturnType => expr`
    /// - `(params): ReturnType => { block }`
    /// - `move (params) => expr`
    fn parse_arrow_function(&mut self) -> Option<Expr> {
        let start = self.current_token().span;

        // Optional `move` keyword
        let is_move = self.eat(&TokenKind::Move);

        // Parameter list
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_param_list();
        self.expect(&TokenKind::RParen)?;

        // Optional return type annotation
        let return_type = if self.check(&TokenKind::Colon) {
            self.advance(); // consume `:`
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        // Fat arrow
        self.expect(&TokenKind::FatArrow)?;

        // Body: block or expression
        let body = if self.check(&TokenKind::LBrace) {
            let block = self.parse_block()?;
            ClosureBody::Block(block)
        } else {
            let expr = self.parse_assignment()?;
            ClosureBody::Expr(Box::new(expr))
        };

        let end = match &body {
            ClosureBody::Block(block) => block.span,
            ClosureBody::Expr(expr) => expr.span,
        };

        Some(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                is_move,
                params,
                return_type,
                body,
            }),
            span: start.merge(end),
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
            TokenKind::Null => {
                self.advance();
                Some(Expr {
                    kind: ExprKind::NullLit,
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
            TokenKind::LBracket => self.parse_array_literal(),
            TokenKind::New => self.parse_new_expr(),
            TokenKind::TemplateNoSub(_) => Some(self.parse_template_no_sub()),
            TokenKind::TemplateHead(_) => self.parse_template_literal(),
            TokenKind::Move => {
                // `move` keyword in expression position — must be a move closure
                self.parse_arrow_function()
            }
            TokenKind::LParen => {
                // Attempt arrow function disambiguation:
                // Save position, try parsing as arrow function params.
                // If `=>` follows, commit as arrow function.
                // Otherwise restore and parse as parenthesized expression.
                if self.is_arrow_function_ahead() {
                    self.parse_arrow_function()
                } else {
                    let open = self.advance();
                    let inner = self.parse_expr()?;
                    let close = self.expect(&TokenKind::RParen)?;
                    let span = open.span.merge(close.span);
                    Some(Expr {
                        kind: ExprKind::Paren(Box::new(inner)),
                        span,
                    })
                }
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
    // ---------------------------------------------------------------
    // Array literals and constructor calls
    // ---------------------------------------------------------------

    /// Parse an array literal: `[ expr, expr, ... ]`.
    fn parse_array_literal(&mut self) -> Option<Expr> {
        let open = self.advance(); // consume `[`
        let start = open.span;
        let mut elements = Vec::new();

        if !self.check(&TokenKind::RBracket) && !self.at_end() {
            loop {
                let elem = self.parse_expr()?;
                elements.push(elem);

                if !self.eat(&TokenKind::Comma) {
                    break;
                }

                // Allow trailing comma
                if self.check(&TokenKind::RBracket) {
                    break;
                }
            }
        }

        let close = self.expect(&TokenKind::RBracket)?;
        let span = start.merge(close.span);

        Some(Expr {
            kind: ExprKind::ArrayLit(elements),
            span,
        })
    }

    /// Parse a `new` expression: `new TypeName<Args>(args)`.
    fn parse_new_expr(&mut self) -> Option<Expr> {
        let new_token = self.advance(); // consume `new`
        let start = new_token.span;

        let type_name = self.parse_ident()?;

        // Optional type arguments: `<string, u32>`
        let type_args = if self.check(&TokenKind::Lt) {
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

            self.expect(&TokenKind::Gt)?;
            args
        } else {
            Vec::new()
        };

        // Argument list
        self.expect(&TokenKind::LParen)?;
        let args = self.parse_arg_list();
        let close = self.expect(&TokenKind::RParen)?;
        let span = start.merge(close.span);

        Some(Expr {
            kind: ExprKind::New(NewExpr {
                type_name,
                type_args,
                args,
            }),
            span,
        })
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
        assert!(ret.throws.is_none());
        let type_ann = ret
            .type_ann
            .as_ref()
            .expect("expected return type annotation");
        match &type_ann.kind {
            TypeKind::Named(ident) => assert_eq!(ident.name, "i32"),
            TypeKind::Void => panic!("expected Named, got Void"),
            TypeKind::Generic(_, _) => panic!("expected Named, got Generic"),
            TypeKind::Union(_) => panic!("expected Named, got Union"),
            TypeKind::Function(_, _) => panic!("expected Named, got Function"),
            TypeKind::Intersection(_) => panic!("expected Named, got Intersection"),
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
        let type_ann = ret
            .type_ann
            .as_ref()
            .expect("expected return type annotation");
        match &type_ann.kind {
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
        let type_ann = ret
            .type_ann
            .as_ref()
            .expect("expected return type annotation");
        assert!(matches!(type_ann.kind, TypeKind::Void));

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
        // with nesting > MAX_EXPR_DEPTH
        let depth = MAX_EXPR_DEPTH + 2;
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
        let depth = MAX_EXPR_DEPTH - 1;
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

    // ---- Task 015: Enum and Switch tests ----

    /// Helper: extract the first enum def from a module.
    fn first_enum(module: &Module) -> &EnumDef {
        for item in &module.items {
            if let ItemKind::EnumDef(ed) = &item.kind {
                return ed;
            }
        }
        panic!("expected enum def item");
    }

    // Test T015-1: Parse simple enum definition → EnumDef with 4 Simple variants
    #[test]
    fn test_parser_simple_enum_four_variants() {
        let source = r#"type Direction = "north" | "south" | "east" | "west""#;
        let module = parse_ok(source);
        let ed = first_enum(&module);
        assert_eq!(ed.name.name, "Direction");
        assert_eq!(ed.variants.len(), 4);
        match &ed.variants[0] {
            EnumVariant::Simple(ident, _) => assert_eq!(ident.name, "North"),
            _ => panic!("expected Simple variant"),
        }
        match &ed.variants[3] {
            EnumVariant::Simple(ident, _) => assert_eq!(ident.name, "West"),
            _ => panic!("expected Simple variant"),
        }
    }

    // Test T015-2: Parse data enum definition → EnumDef with Data variants
    #[test]
    fn test_parser_data_enum_two_variants_with_fields() {
        let source = r#"
type Shape =
  | { kind: "circle", radius: f64 }
  | { kind: "rect", width: f64, height: f64 }
"#;
        let module = parse_ok(source);
        let ed = first_enum(&module);
        assert_eq!(ed.name.name, "Shape");
        assert_eq!(ed.variants.len(), 2);
        match &ed.variants[0] {
            EnumVariant::Data {
                discriminant_value,
                name,
                fields,
                ..
            } => {
                assert_eq!(discriminant_value, "circle");
                assert_eq!(name.name, "Circle");
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name.name, "radius");
            }
            _ => panic!("expected Data variant"),
        }
        match &ed.variants[1] {
            EnumVariant::Data {
                discriminant_value,
                name,
                fields,
                ..
            } => {
                assert_eq!(discriminant_value, "rect");
                assert_eq!(name.name, "Rect");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name.name, "width");
                assert_eq!(fields[1].name.name, "height");
            }
            _ => panic!("expected Data variant"),
        }
    }

    // Test T015-3: Parse switch statement → SwitchStmt with cases
    #[test]
    fn test_parser_switch_stmt_two_cases() {
        let source = r#"
function test(dir: Direction): Direction {
  switch (dir) {
    case "north":
      return "south";
    case "south":
      return "north";
  }
}
"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.body.stmts.len(), 1);
        match &f.body.stmts[0] {
            Stmt::Switch(switch) => {
                assert_eq!(switch.cases.len(), 2);
                assert_eq!(switch.cases[0].pattern, "north");
                assert_eq!(switch.cases[1].pattern, "south");
                assert_eq!(switch.cases[0].body.len(), 1);
                assert_eq!(switch.cases[1].body.len(), 1);
            }
            _ => panic!("expected Switch statement"),
        }
    }

    // ---------------------------------------------------------------
    // Task 017: Collection parsing
    // ---------------------------------------------------------------

    // Test T17-1: Parse `[1, 2, 3]` → ExprKind::ArrayLit with 3 elements
    #[test]
    fn test_parser_array_literal_three_elements() {
        let module = parse_ok("function main() { const x = [1, 2, 3]; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        match &decl.init.kind {
            ExprKind::ArrayLit(elements) => {
                assert_eq!(elements.len(), 3);
                assert!(matches!(elements[0].kind, ExprKind::IntLit(1)));
                assert!(matches!(elements[1].kind, ExprKind::IntLit(2)));
                assert!(matches!(elements[2].kind, ExprKind::IntLit(3)));
            }
            _ => panic!("expected ArrayLit, got {:?}", decl.init.kind),
        }
    }

    // Test T17-2: Parse `[]` → empty ArrayLit
    #[test]
    fn test_parser_empty_array_literal() {
        let module = parse_ok("function main() { const x = []; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        match &decl.init.kind {
            ExprKind::ArrayLit(elements) => {
                assert!(elements.is_empty());
            }
            _ => panic!("expected ArrayLit, got {:?}", decl.init.kind),
        }
    }

    // Test T17-3: Parse `new Map()` → ExprKind::New with type_name "Map"
    #[test]
    fn test_parser_new_map_expression() {
        let module = parse_ok("function main() { const x = new Map(); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        match &decl.init.kind {
            ExprKind::New(new_expr) => {
                assert_eq!(new_expr.type_name.name, "Map");
                assert!(new_expr.type_args.is_empty());
                assert!(new_expr.args.is_empty());
            }
            _ => panic!("expected New, got {:?}", decl.init.kind),
        }
    }

    // Test T17-4: Parse `new Set()` → ExprKind::New with type_name "Set"
    #[test]
    fn test_parser_new_set_expression() {
        let module = parse_ok("function main() { const x = new Set(); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        match &decl.init.kind {
            ExprKind::New(new_expr) => {
                assert_eq!(new_expr.type_name.name, "Set");
                assert!(new_expr.type_args.is_empty());
                assert!(new_expr.args.is_empty());
            }
            _ => panic!("expected New, got {:?}", decl.init.kind),
        }
    }

    // Test T17-5: Parse `arr[0]` → ExprKind::Index
    #[test]
    fn test_parser_index_access() {
        let module = parse_ok("function main() { const x = arr[0]; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        match &decl.init.kind {
            ExprKind::Index(index_expr) => {
                assert!(matches!(index_expr.object.kind, ExprKind::Ident(_)));
                assert!(matches!(index_expr.index.kind, ExprKind::IntLit(0)));
            }
            _ => panic!("expected Index, got {:?}", decl.init.kind),
        }
    }

    // Test T17-6: Parse `const names: Array<string> = ["Alice", "Bob"]`
    #[test]
    fn test_parser_array_type_annotation_with_literal() {
        let module =
            parse_ok("function main() { const names: Array<string> = [\"Alice\", \"Bob\"]; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        assert_eq!(decl.name.name, "names");
        // Check type annotation
        let type_ann = decl.type_ann.as_ref().expect("expected type annotation");
        match &type_ann.kind {
            TypeKind::Generic(base, args) => {
                assert_eq!(base.name, "Array");
                assert_eq!(args.len(), 1);
                match &args[0].kind {
                    TypeKind::Named(n) => assert_eq!(n.name, "string"),
                    _ => panic!("expected Named type arg"),
                }
            }
            _ => panic!("expected Generic type annotation"),
        }
        // Check initializer is array literal
        match &decl.init.kind {
            ExprKind::ArrayLit(elements) => {
                assert_eq!(elements.len(), 2);
            }
            _ => panic!("expected ArrayLit"),
        }
    }

    // --- Task 020: T | null, null literal, optional chaining, nullish coalescing ---

    // Test 1: Parse `const x: string | null = null;`
    #[test]
    fn test_parser_union_type_string_or_null() {
        let source = r#"function main() { const x: string | null = null; }"#;
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");
        let f = first_fn(&module);
        let stmt = &f.body.stmts[0];
        match stmt {
            Stmt::VarDecl(decl) => {
                assert_eq!(decl.name.name, "x");
                let ann = decl.type_ann.as_ref().expect("expected type annotation");
                match &ann.kind {
                    TypeKind::Union(members) => {
                        assert_eq!(members.len(), 2);
                        match &members[0].kind {
                            TypeKind::Named(n) => assert_eq!(n.name, "string"),
                            _ => panic!("expected Named"),
                        }
                        match &members[1].kind {
                            TypeKind::Named(n) => assert_eq!(n.name, "null"),
                            _ => panic!("expected Named(null)"),
                        }
                    }
                    _ => panic!("expected Union type annotation"),
                }
                assert!(matches!(decl.init.kind, ExprKind::NullLit));
            }
            _ => panic!("expected VarDecl"),
        }
    }

    // Test 2: Parse `null` → ExprKind::NullLit
    #[test]
    fn test_parser_null_literal() {
        let source = r#"function main() { return null; }"#;
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::Return(r) => {
                let val = r.value.as_ref().expect("expected return value");
                assert!(matches!(val.kind, ExprKind::NullLit));
            }
            _ => panic!("expected Return"),
        }
    }

    // Test 3: Parse `user?.name` → OptionalChainExpr
    #[test]
    fn test_parser_optional_chain_field() {
        let source = r#"function main() { const x = user?.name; }"#;
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::VarDecl(decl) => match &decl.init.kind {
                ExprKind::OptionalChain(chain) => match &chain.access {
                    OptionalAccess::Field(field) => assert_eq!(field.name, "name"),
                    _ => panic!("expected Field access"),
                },
                _ => panic!("expected OptionalChain, got {:?}", decl.init.kind),
            },
            _ => panic!("expected VarDecl"),
        }
    }

    // Test 4: Parse `name ?? "Anonymous"` → NullishCoalescingExpr
    #[test]
    fn test_parser_nullish_coalescing() {
        let source = r#"function main() { const x = name ?? "Anonymous"; }"#;
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::VarDecl(decl) => match &decl.init.kind {
                ExprKind::NullishCoalescing(nc) => {
                    assert!(matches!(nc.left.kind, ExprKind::Ident(_)));
                    assert!(matches!(nc.right.kind, ExprKind::StringLit(_)));
                }
                _ => panic!("expected NullishCoalescing"),
            },
            _ => panic!("expected VarDecl"),
        }
    }

    // Test 5: Parse `if (x !== null)` → IfStmt with binary Ne comparison to null
    #[test]
    fn test_parser_if_not_null_check() {
        let source = r#"function main() { if (x !== null) { return x; } }"#;
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::If(if_stmt) => match &if_stmt.condition.kind {
                ExprKind::Binary(bin) => {
                    assert_eq!(bin.op, BinaryOp::Ne);
                    assert!(matches!(bin.left.kind, ExprKind::Ident(_)));
                    assert!(matches!(bin.right.kind, ExprKind::NullLit));
                }
                _ => panic!("expected Binary"),
            },
            _ => panic!("expected If"),
        }
    }

    // Test: Parse `===` and `!==` operators
    #[test]
    fn test_parser_strict_equality_operators() {
        let source = r#"function main() { if (x === null) { } if (y !== null) { } }"#;
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::If(if_stmt) => match &if_stmt.condition.kind {
                ExprKind::Binary(bin) => {
                    assert_eq!(bin.op, BinaryOp::Eq);
                    assert!(matches!(bin.right.kind, ExprKind::NullLit));
                }
                _ => panic!("expected Binary"),
            },
            _ => panic!("expected If"),
        }
        match &f.body.stmts[1] {
            Stmt::If(if_stmt) => match &if_stmt.condition.kind {
                ExprKind::Binary(bin) => {
                    assert_eq!(bin.op, BinaryOp::Ne);
                    assert!(matches!(bin.right.kind, ExprKind::NullLit));
                }
                _ => panic!("expected Binary"),
            },
            _ => panic!("expected If"),
        }
    }

    // --- Task 021: throws, try/catch, throw ---

    // Parse `function fetch(): User throws ApiError { }`
    #[test]
    fn test_parser_throws_return_type_produces_return_type_annotation() {
        let source = "function fetch(): string throws string { }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "fetch");

        let ret = f.return_type.as_ref().expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected success type");
        assert!(matches!(&type_ann.kind, TypeKind::Named(id) if id.name == "string"));
        let throws = ret.throws.as_ref().expect("expected throws type");
        assert!(matches!(&throws.kind, TypeKind::Named(id) if id.name == "string"));
    }

    // Parse `function fail() throws MyError { }`
    #[test]
    fn test_parser_void_throws_produces_return_type_with_no_success_type() {
        let source = "function fail() throws string { }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "fail");

        let ret = f.return_type.as_ref().expect("expected return type");
        assert!(ret.type_ann.is_none());
        let throws = ret.throws.as_ref().expect("expected throws type");
        assert!(matches!(&throws.kind, TypeKind::Named(id) if id.name == "string"));
    }

    // Parse try/catch block
    #[test]
    fn test_parser_try_catch_produces_try_catch_stmt() {
        let source = "\
function main() {
  try {
    const x = 1;
  } catch (err: string) {
    const y = 2;
  }
}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::TryCatch(tc) => {
                assert_eq!(tc.try_block.stmts.len(), 1);
                assert_eq!(tc.catch_binding.name, "err");
                let catch_type = tc.catch_type.as_ref().expect("expected catch type");
                assert!(matches!(&catch_type.kind, TypeKind::Named(id) if id.name == "string"));
                assert_eq!(tc.catch_block.stmts.len(), 1);
            }
            _ => panic!("expected TryCatch"),
        }
    }

    // Parse throw expression
    #[test]
    fn test_parser_throw_expression_produces_throw_expr() {
        let source = "\
function fail() throws string {
  throw \"error\";
}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::Expr(expr) => match &expr.kind {
                ExprKind::Throw(inner) => {
                    assert!(matches!(&inner.kind, ExprKind::StringLit(s) if s == "error"));
                }
                _ => panic!("expected Throw expression"),
            },
            _ => panic!("expected Expr statement"),
        }
    }

    // ---------------------------------------------------------------
    // Task 019: Closures and arrow functions
    // ---------------------------------------------------------------

    // Test T19-1: Parse `(x: i32): i32 => x * 2` → ClosureExpr with expression body
    #[test]
    fn test_parser_closure_expr_body_with_types() {
        let source = "function main() { const double = (x: i32): i32 => x * 2; }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        let decl = match &f.body.stmts[0] {
            Stmt::VarDecl(d) => d,
            _ => panic!("expected VarDecl"),
        };
        assert_eq!(decl.name.name, "double");
        let closure = match &decl.init.kind {
            ExprKind::Closure(c) => c,
            other => panic!("expected Closure, got {other:?}"),
        };
        assert!(!closure.is_move);
        assert_eq!(closure.params.len(), 1);
        assert_eq!(closure.params[0].name.name, "x");
        assert!(closure.return_type.is_some());
        assert!(matches!(closure.body, ClosureBody::Expr(_)));
    }

    // Test T19-2: Parse `() => { console.log("hello"); }` → ClosureExpr with block body
    #[test]
    fn test_parser_closure_block_body() {
        let source = "function main() { const greet = () => { console.log(\"hello\"); }; }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        let decl = match &f.body.stmts[0] {
            Stmt::VarDecl(d) => d,
            _ => panic!("expected VarDecl"),
        };
        let closure = match &decl.init.kind {
            ExprKind::Closure(c) => c,
            other => panic!("expected Closure, got {other:?}"),
        };
        assert!(!closure.is_move);
        assert!(closure.params.is_empty());
        assert!(closure.return_type.is_none());
        assert!(matches!(closure.body, ClosureBody::Block(_)));
    }

    // Test T19-3: Parse `move () => { process(ctx); }` → ClosureExpr with is_move true
    #[test]
    fn test_parser_closure_move() {
        let source = "function main() { const handler = move () => { process(ctx); }; }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        let decl = match &f.body.stmts[0] {
            Stmt::VarDecl(d) => d,
            _ => panic!("expected VarDecl"),
        };
        let closure = match &decl.init.kind {
            ExprKind::Closure(c) => c,
            other => panic!("expected Closure, got {other:?}"),
        };
        assert!(closure.is_move);
        assert!(closure.params.is_empty());
        assert!(matches!(closure.body, ClosureBody::Block(_)));
    }

    // Test T19-4: Parse closure as function argument
    #[test]
    fn test_parser_closure_as_argument() {
        let source = "function main() { apply(5, (x: i32): i32 => x * 2); }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        let call = match &f.body.stmts[0] {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Call(c) => c,
                other => panic!("expected Call, got {other:?}"),
            },
            _ => panic!("expected Expr statement"),
        };
        assert_eq!(call.args.len(), 2);
        assert!(matches!(call.args[1].kind, ExprKind::Closure(_)));
    }

    // Test T19-5: Parse function type annotation `(i32) => i32`
    #[test]
    fn test_parser_function_type_annotation() {
        let source = "function apply(x: i32, f: (i32) => i32): i32 { return f(x); }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[1].name.name, "f");
        match &f.params[1].type_ann.kind {
            TypeKind::Function(params, ret) => {
                assert_eq!(params.len(), 1);
                assert!(matches!(&ret.kind, TypeKind::Named(ident) if ident.name == "i32"));
            }
            other => panic!("expected Function type, got {other:?}"),
        }
    }

    // Test T19-6: Disambiguate paren expression from arrow function
    #[test]
    fn test_parser_paren_expr_not_confused_with_closure() {
        let source = "function main() { const x = (1 + 2); }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        let decl = match &f.body.stmts[0] {
            Stmt::VarDecl(d) => d,
            _ => panic!("expected VarDecl"),
        };
        assert!(matches!(decl.init.kind, ExprKind::Paren(_)));
    }

    // Test T19-7: FatArrow token lexed via parser
    #[test]
    fn test_parser_fat_arrow_in_closure() {
        // Verify `=>` is properly lexed and parsed in arrow function context
        let source = "function f() { const g = (x: i32): i32 => x; }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        let decl = match &f.body.stmts[0] {
            Stmt::VarDecl(d) => d,
            _ => panic!("expected VarDecl"),
        };
        assert!(matches!(decl.init.kind, ExprKind::Closure(_)));
    }

    // Test T19-8: Closure with multi-param
    #[test]
    fn test_parser_closure_multiple_params() {
        let source = "function f() { const add = (a: i32, b: i32): i32 => a; }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        let decl = match &f.body.stmts[0] {
            Stmt::VarDecl(d) => d,
            _ => panic!("expected VarDecl"),
        };
        let closure = match &decl.init.kind {
            ExprKind::Closure(c) => c,
            other => panic!("expected Closure, got {other:?}"),
        };
        assert_eq!(closure.params.len(), 2);
        assert_eq!(closure.params[0].name.name, "a");
        assert_eq!(closure.params[1].name.name, "b");
    }

    // ---- Task 022: Interface parsing tests ----

    #[test]
    fn test_parser_interface_single_method_produces_interface_def() {
        let source = r#"interface Serializable {
  serialize(): string;
}"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(module.items.len(), 1);
        let iface = match &module.items[0].kind {
            ItemKind::Interface(i) => i,
            other => panic!("expected Interface, got {other:?}"),
        };
        assert_eq!(iface.name.name, "Serializable");
        assert_eq!(iface.methods.len(), 1);
        assert_eq!(iface.methods[0].name.name, "serialize");
        assert!(iface.methods[0].params.is_empty());
        let ret = iface.methods[0]
            .return_type
            .as_ref()
            .expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected type annotation");
        match &type_ann.kind {
            TypeKind::Named(ident) => assert_eq!(ident.name, "string"),
            other => panic!("expected Named type, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_interface_multiple_methods_correct_count() {
        let source = r#"interface Handler {
  handle(data: string): void;
  status(): i32;
  reset(): void;
}"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let iface = match &module.items[0].kind {
            ItemKind::Interface(i) => i,
            other => panic!("expected Interface, got {other:?}"),
        };
        assert_eq!(iface.methods.len(), 3);
        assert_eq!(iface.methods[0].name.name, "handle");
        assert_eq!(iface.methods[0].params.len(), 1);
        assert_eq!(iface.methods[0].params[0].name.name, "data");
        assert_eq!(iface.methods[1].name.name, "status");
        assert_eq!(iface.methods[2].name.name, "reset");
    }

    #[test]
    fn test_parser_interface_self_return_type_parsed_as_named_self() {
        let source = r#"interface Cloneable {
  clone(): Self;
}"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let iface = match &module.items[0].kind {
            ItemKind::Interface(i) => i,
            other => panic!("expected Interface, got {other:?}"),
        };
        let ret = iface.methods[0]
            .return_type
            .as_ref()
            .expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected type annotation");
        match &type_ann.kind {
            TypeKind::Named(ident) => assert_eq!(ident.name, "Self"),
            other => panic!("expected Named(Self), got {other:?}"),
        }
    }

    #[test]
    fn test_parser_intersection_type_in_parameter() {
        let source = r#"function process(input: Serializable & Printable): string {
  return input.serialize();
}"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let func = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            other => panic!("expected Function, got {other:?}"),
        };
        assert_eq!(func.params.len(), 1);
        match &func.params[0].type_ann.kind {
            TypeKind::Intersection(members) => {
                assert_eq!(members.len(), 2);
                match &members[0].kind {
                    TypeKind::Named(ident) => assert_eq!(ident.name, "Serializable"),
                    other => panic!("expected Named, got {other:?}"),
                }
                match &members[1].kind {
                    TypeKind::Named(ident) => assert_eq!(ident.name, "Printable"),
                    other => panic!("expected Named, got {other:?}"),
                }
            }
            other => panic!("expected Intersection, got {other:?}"),
        }
    }
}
