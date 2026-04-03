//! Recursive descent parser for `RustScript` source files.
//!
//! Consumes the token stream from the lexer and produces a [`rsc_syntax::ast::Module`].
//! Implements error recovery at statement boundaries so that parsing continues
//! past syntax errors, accumulating diagnostics along the way.

#[allow(clippy::wildcard_imports)]
// Class completeness requires many AST types for the new member kinds
use rsc_syntax::ast::{
    ArrayDestructureElement, ArrayDestructureStmt, ArrayElement, AssignExpr, BinaryExpr, BinaryOp,
    Block, BreakStmt, CallExpr, ClassConstructor, ClassDef, ClassField, ClassGetter, ClassMember,
    ClassMethod, ClassSetter, ClosureBody, ClosureExpr, ConstructorParam, ContinueStmt, Decorator,
    DestructureField, DestructureStmt, DoWhileStmt, ElseClause, EnumDef, EnumVariant, Expr,
    ExprKind, FieldAccessExpr, FieldAssignExpr, FieldDef, FieldInit, FnDecl, ForClassicStmt,
    ForInStmt, ForInit, ForOfStmt, Ident, IfStmt, ImportDecl, IndexAssignExpr, IndexExpr,
    IndexSignature, InlineRustBlock, InterfaceDef, InterfaceMethod, Item, ItemKind,
    LogicalAssignExpr, LogicalAssignOp, MappedModifier, MethodCallExpr, Module, NewExpr,
    NullishCoalescingExpr, OptionalAccess, OptionalChainExpr, Param, ReExportDecl, ReturnStmt,
    ReturnTypeAnnotation, Stmt, StringLiteral, StructLitExpr, SwitchCase, SwitchStmt,
    TemplateLitExpr, TemplatePart, TestBlock, TestBlockKind, TestBody, TryCatchStmt,
    TypeAnnotation, TypeDef, TypeKind, TypeParam, TypeParams, UnaryExpr, UnaryOp, UsingDecl,
    VarBinding, VarDecl, Visibility, WhileStmt, WildcardReExportDecl,
};
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::source::FileId;
use rsc_syntax::span::Span;

use crate::token::{Token, TokenKind};

/// Maximum nesting depth for expressions to prevent stack overflow on
/// adversarial input (e.g., deeply nested parentheses).
///
/// Set conservatively to account for the full precedence chain per depth
/// level in debug builds. Each expression depth level uses ~18+ stack
/// frames through the precedence hierarchy (assignment, ternary, bitwise,
/// shift, exponentiation, postfix, etc.), including arrow function
/// disambiguation lookahead and logical assignment checks.
const MAX_EXPR_DEPTH: usize = 30;

/// Maximum nesting depth for blocks / statements to prevent stack overflow
/// on adversarial input (e.g., 50 nested `function` declarations).
const MAX_BLOCK_DEPTH: usize = 50;

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
pub(crate) struct Parser<'src> {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    file_id: FileId,
    expr_depth: usize,
    block_depth: usize,
    /// The original source text, needed for extracting raw content from
    /// inline Rust blocks (where we need the verbatim text between braces).
    source: &'src str,
    /// Pending `JSDoc` comment from a `/** ... */` token. Drained and attached
    /// to the next declaration the parser encounters.
    pending_doc: Option<String>,
    /// Spans of regex literals parsed from the source. Used to filter out
    /// lexer diagnostics that fall within regex literal regions, since the
    /// lexer cannot disambiguate `/` as regex vs division.
    regex_literal_spans: Vec<Span>,
}

impl<'src> Parser<'src> {
    /// Create a new parser from a token stream.
    pub(crate) fn new(tokens: Vec<Token>, file_id: FileId, source: &'src str) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
            file_id,
            expr_depth: 0,
            block_depth: 0,
            source,
            pending_doc: None,
            regex_literal_spans: Vec::new(),
        }
    }

    /// Consume the parser and return accumulated diagnostics.
    pub(crate) fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    /// Return the spans of any regex literals parsed from the source.
    /// Used to filter out lexer diagnostics that fall within regex regions.
    pub(crate) fn regex_literal_spans(&self) -> &[Span] {
        &self.regex_literal_spans
    }

    /// Drain the pending `JSDoc` comment, if any, and return it.
    fn take_pending_doc(&mut self) -> Option<String> {
        self.pending_doc.take()
    }

    /// Consume any `JsDoc` tokens at the current position, storing the last one
    /// as `pending_doc`. Multiple consecutive `JSDoc` comments: last one wins.
    fn consume_jsdoc_tokens(&mut self) {
        while let TokenKind::JsDoc(text) = self.peek().clone() {
            self.pending_doc = Some(text);
            self.advance();
        }
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

    /// Consume the current token if it is an identifier matching the given name.
    ///
    /// Used for contextual keywords like `of` that should remain valid identifiers
    /// in other contexts.
    fn eat_contextual_keyword(&mut self, name: &str) -> bool {
        if let TokenKind::Ident(ident_name) = self.peek()
            && ident_name == name
        {
            self.advance();
            return true;
        }
        false
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
    #[allow(clippy::too_many_lines)]
    // Token description covers all keyword and operator variants; splitting would obscure the mapping
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
            TokenKind::Do => "`do`",
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
            TokenKind::For => "`for`",
            TokenKind::Break => "`break`",
            TokenKind::Continue => "`continue`",
            TokenKind::Import => "`import`",
            TokenKind::Export => "`export`",
            TokenKind::From => "`from`",
            TokenKind::FatArrow => "`=>`",
            TokenKind::Ampersand => "`&`",
            TokenKind::Class => "`class`",
            TokenKind::Constructor => "`constructor`",
            TokenKind::This => "`this`",
            TokenKind::Super => "`super`",
            TokenKind::Private => "`private`",
            TokenKind::Public => "`public`",
            TokenKind::Implements => "`implements`",
            TokenKind::Async => "`async`",
            TokenKind::Await => "`await`",
            TokenKind::Finally => "`finally`",
            TokenKind::Rust => "`rust`",
            TokenKind::Derives => "`derives`",
            TokenKind::Yield => "`yield`",
            TokenKind::Pipe => "`|`",
            TokenKind::QuestionDot => "`?.`",
            TokenKind::QuestionQuestion => "`??`",
            TokenKind::QuestionQuestionEq => "`??=`",
            TokenKind::PipePipeEq => "`||=`",
            TokenKind::AmpAmpEq => "`&&=`",
            TokenKind::EqEqEq => "`===`",
            TokenKind::BangEqEq => "`!==`",
            TokenKind::LBracket => "`[`",
            TokenKind::RBracket => "`]`",
            TokenKind::DotDotDot => "`...`",
            TokenKind::Question => "`?`",
            TokenKind::TemplateHead(_) | TokenKind::TemplateNoSub(_) => "template literal",
            TokenKind::TemplateMiddle(_) => "template literal middle",
            TokenKind::TemplateTail(_) => "template literal tail",
            TokenKind::StarStar => "`**`",
            TokenKind::Caret => "`^`",
            TokenKind::Tilde => "`~`",
            TokenKind::As => "`as`",
            TokenKind::TypeOf => "`typeof`",
            TokenKind::KeyOf => "`keyof`",
            TokenKind::Abstract => "`abstract`",
            TokenKind::Override => "`override`",
            TokenKind::Satisfies => "`satisfies`",
            TokenKind::Infer => "`infer`",
            TokenKind::At => "`@`",
            TokenKind::Delete => "`delete`",
            TokenKind::Void => "`void`",
            TokenKind::In => "`in`",
            TokenKind::Declare => "`declare`",
            TokenKind::Debugger => "`debugger`",
            TokenKind::PlusPlus => "`++`",
            TokenKind::MinusMinus => "`--`",
            TokenKind::Var => "`var`",
            TokenKind::Enum => "`enum`",
            TokenKind::JsDoc(_) => "JSDoc comment",
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
                | TokenKind::Interface
                | TokenKind::Class
                | TokenKind::Abstract
                | TokenKind::Declare
                | TokenKind::Enum
                | TokenKind::At => return,
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
    // Unsupported syntax diagnostics
    // ---------------------------------------------------------------

    /// Recognize a `namespace` or `module Foo { ... }` declaration, emit a
    /// diagnostic, and skip the body so parsing can continue.
    ///
    /// TypeScript's `namespace` keyword (and the legacy `module Foo {}` form)
    /// is a pre-module-era feature. RustScript uses standard ES module
    /// imports/exports instead, so we reject it with a helpful message.
    fn parse_namespace_diagnostic(&mut self) -> Option<Item> {
        let keyword_token = self.advance(); // consume `namespace` or `module`
        let start = keyword_token.span;

        // Consume the namespace name (identifier).
        let _name = self.advance(); // consume the name

        // Emit the diagnostic.
        self.diagnostics.push(
            Diagnostic::error(
                "namespaces are not supported in RustScript; use module imports (`import`/`export`) instead",
            )
            .with_label(start, self.file_id, "unsupported syntax"),
        );

        // Skip the block body by counting brace depth.
        if self.check(&TokenKind::LBrace) {
            self.advance(); // consume `{`
            let mut depth: u32 = 1;
            while depth > 0 && !self.at_end() {
                match self.peek() {
                    TokenKind::LBrace => {
                        depth += 1;
                        self.advance();
                    }
                    TokenKind::RBrace => {
                        depth -= 1;
                        self.advance();
                    }
                    _ => {
                        self.advance();
                    }
                }
            }
        }

        None
    }

    // ---------------------------------------------------------------
    // Top-level parsing
    // ---------------------------------------------------------------

    /// Parse the entire token stream into a [`Module`].
    pub(crate) fn parse_module(&mut self) -> Module {
        let start_span = self.current_token().span;
        let mut items = Vec::new();

        while !self.at_end() {
            // Consume any JSDoc tokens before the next item so that
            // `pending_doc` is set for the subsequent `parse_item`.
            self.consume_jsdoc_tokens();
            // Parse any decorators before the item.
            let decorators = self.parse_decorators();
            let before = self.pos;
            if let Some(mut item) = self.parse_item() {
                item.decorators = decorators;
                items.push(item);
            }
            // Safety: guarantee forward progress to prevent infinite loops
            // when error recovery returns to the same token position.
            if self.pos == before {
                self.advance();
            }
        }

        let end_span = self.current_token().span;
        Module {
            items,
            span: start_span.merge(end_span),
        }
    }

    /// Parse zero or more decorators: `@name` or `@name(args)`.
    ///
    /// Decorators are parsed greedily before any item declaration. Each decorator
    /// maps to a Rust attribute (`#[...]`) during lowering.
    fn parse_decorators(&mut self) -> Vec<Decorator> {
        let mut decorators = Vec::new();
        while self.check(&TokenKind::At) {
            let at_token = self.advance(); // consume `@`
            let start = at_token.span;

            // Expect an identifier for the decorator name
            let name = if let TokenKind::Ident(name) = self.peek().clone() {
                self.advance();
                name
            } else {
                self.diagnostics.push(
                    Diagnostic::error("expected decorator name after `@`").with_label(
                        self.current_token().span,
                        self.file_id,
                        "expected identifier",
                    ),
                );
                break;
            };

            // Optionally parse parenthesized arguments as a raw string
            let args = if self.check(&TokenKind::LParen) {
                self.advance(); // consume `(`
                let args_start = self.pos;
                let mut depth = 1_u32;
                // Collect tokens until the matching close paren
                while depth > 0 && !self.at_end() {
                    match self.peek() {
                        TokenKind::LParen => {
                            depth += 1;
                            self.advance();
                        }
                        TokenKind::RParen => {
                            depth -= 1;
                            if depth > 0 {
                                self.advance();
                            }
                        }
                        _ => {
                            self.advance();
                        }
                    }
                }
                // Extract the raw text between the parens from the source
                let args_end = self.pos;
                let args_text = if args_start < args_end {
                    let start_byte = self.tokens[args_start].span.start.0 as usize;
                    let end_byte = self.tokens[args_end.min(self.tokens.len() - 1)]
                        .span
                        .start
                        .0 as usize;
                    self.source[start_byte..end_byte].trim().to_owned()
                } else {
                    String::new()
                };
                self.eat(&TokenKind::RParen); // consume `)`
                if args_text.is_empty() {
                    None
                } else {
                    Some(args_text)
                }
            } else {
                None
            };

            let end = self.previous_span();
            decorators.push(Decorator {
                name,
                args,
                span: start.merge(end),
            });
        }
        decorators
    }

    /// Parse a top-level item: function, type definition, interface, import, or export.
    #[allow(clippy::too_many_lines)]
    fn parse_item(&mut self) -> Option<Item> {
        match self.peek() {
            TokenKind::Async => {
                let async_token = self.advance();
                if !self.check(&TokenKind::Function) {
                    self.diagnostics.push(
                        Diagnostic::error("expected `function` after `async`").with_label(
                            self.current_token().span,
                            self.file_id,
                            "expected `function`",
                        ),
                    );
                    self.synchronize();
                    return None;
                }
                self.parse_function_decl(true, Some(async_token.span))
                    .map(|f| {
                        let span = f.span;
                        Item {
                            kind: ItemKind::Function(f),
                            exported: false,
                            decorators: vec![],
                            span,
                        }
                    })
            }
            TokenKind::Function => self.parse_function_decl(false, None).map(|f| {
                let span = f.span;
                Item {
                    kind: ItemKind::Function(f),
                    exported: false,
                    decorators: vec![],
                    span,
                }
            }),
            TokenKind::Type => self.parse_type_or_enum_def(),
            TokenKind::Interface => self.parse_interface_def().map(|iface| {
                let span = iface.span;
                Item {
                    kind: ItemKind::Interface(iface),
                    exported: false,
                    decorators: vec![],
                    span,
                }
            }),
            TokenKind::Class => self.parse_class_def(false).map(|cls| {
                let span = cls.span;
                Item {
                    kind: ItemKind::Class(cls),
                    exported: false,
                    decorators: vec![],
                    span,
                }
            }),
            TokenKind::Abstract => {
                if self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == TokenKind::Class)
                {
                    self.advance(); // consume `abstract`
                    self.parse_class_def(true).map(|cls| {
                        let span = cls.span;
                        Item {
                            kind: ItemKind::Class(cls),
                            exported: false,
                            decorators: vec![],
                            span,
                        }
                    })
                } else {
                    let current = self.current_token().clone();
                    self.diagnostics.push(
                        Diagnostic::error("expected `class` after `abstract`").with_label(
                            current.span,
                            self.file_id,
                            "expected `class`",
                        ),
                    );
                    self.advance();
                    self.synchronize();
                    None
                }
            }
            TokenKind::Import => self.parse_import_decl(),
            TokenKind::Export => self.parse_export_decl(),
            TokenKind::Rust => self.parse_rust_block_item(),
            TokenKind::Const
                if self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == TokenKind::Enum) =>
            {
                self.parse_const_enum_def()
            }
            TokenKind::Enum => self.parse_ts_enum_def(false),
            TokenKind::Const | TokenKind::Let | TokenKind::Var => self.parse_top_level_const(),
            TokenKind::Declare => {
                // Ambient declaration — skip entirely (produces no AST node).
                self.skip_declare_body();
                None
            }
            TokenKind::Ident(name) if matches!(name.as_str(), "test" | "describe" | "it") => {
                self.parse_test_block()
            }
            TokenKind::Ident(name)
                if name == "namespace"
                    || (name == "module"
                        && self
                            .tokens
                            .get(self.pos + 1)
                            .is_some_and(|t| matches!(t.kind, TokenKind::Ident(_)))) =>
            {
                self.parse_namespace_diagnostic()
            }
            _ => {
                let current = self.current_token().clone();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "expected a declaration (function, class, type, const, etc.), found {}",
                        Self::describe_kind(&current.kind)
                    ))
                    .with_label(
                        current.span,
                        self.file_id,
                        "expected declaration",
                    ),
                );
                // Advance past the unexpected token to prevent infinite loops
                // when synchronize() stops on the same token.
                self.advance();
                self.synchronize();
                None
            }
        }
    }

    // ---------------------------------------------------------------
    // Ambient (declare) declarations
    // ---------------------------------------------------------------

    /// Skip an ambient declaration: `declare function foo(): void;`,
    /// `declare const x: string;`, `declare class Foo { ... }`,
    /// `declare module "x" { ... }`, etc.
    ///
    /// Ambient declarations are type-information only and produce no runtime
    /// code. The parser consumes the `declare` keyword and then skips all
    /// tokens until the end of the declaration (semicolon or matching closing
    /// brace), producing no AST node.
    fn skip_declare_body(&mut self) {
        self.advance(); // consume `declare`

        // Skip tokens until we hit a semicolon at brace depth 0,
        // or close a brace that brings us back to depth 0.
        let mut brace_depth: u32 = 0;
        loop {
            match self.peek() {
                TokenKind::Eof => break,
                TokenKind::Semicolon if brace_depth == 0 => {
                    self.advance(); // consume the semicolon
                    break;
                }
                TokenKind::LBrace => {
                    brace_depth += 1;
                    self.advance();
                }
                TokenKind::RBrace => {
                    if brace_depth == 0 {
                        break;
                    }
                    brace_depth -= 1;
                    self.advance();
                    // After closing a top-level brace block, the declaration is done
                    if brace_depth == 0 {
                        // Consume optional trailing semicolon
                        self.eat(&TokenKind::Semicolon);
                        break;
                    }
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    // ---------------------------------------------------------------
    // Import/export declarations
    // ---------------------------------------------------------------

    /// Parse an import declaration: `import { Name1, Name2 } from "path";`.
    fn parse_import_decl(&mut self) -> Option<Item> {
        let import_token = self.advance(); // consume `import`
        let start = import_token.span;

        // Check for `import type { ... }` — type-only imports
        let is_type_only = self.eat(&TokenKind::Type);

        self.expect(&TokenKind::LBrace)?;
        let names = self.parse_import_name_list();
        self.expect(&TokenKind::RBrace)?;

        self.expect_keyword(&TokenKind::From, "from")?;

        let source = self.parse_string_literal()?;

        // Optional semicolon
        if self.check(&TokenKind::Semicolon) {
            self.advance();
        }

        let span = start.merge(source.span);
        Some(Item {
            kind: ItemKind::Import(ImportDecl {
                names,
                source,
                is_type_only,
                span,
            }),
            exported: false,
            decorators: vec![],
            span,
        })
    }

    /// Parse an export declaration.
    ///
    /// Supports:
    /// - `export function ...` — exported function
    /// - `export type ...` — exported type/enum
    /// - `export interface ...` — exported interface
    /// - `export abstract class ...` — exported abstract class
    /// - `export declare ...` — ambient declaration (skipped)
    /// - `export * from "path"` — wildcard re-export
    /// - `export { Name } from "path"` — re-export
    #[allow(clippy::too_many_lines)]
    // Export parsing must disambiguate many syntactic forms; splitting would fragment the grammar
    fn parse_export_decl(&mut self) -> Option<Item> {
        let export_token = self.advance(); // consume `export`
        let start = export_token.span;

        match self.peek() {
            TokenKind::Async => {
                let async_token = self.advance();
                if !self.check(&TokenKind::Function) {
                    self.diagnostics.push(
                        Diagnostic::error("expected `function` after `async`").with_label(
                            self.current_token().span,
                            self.file_id,
                            "expected `function`",
                        ),
                    );
                    self.synchronize();
                    return None;
                }
                let f = self.parse_function_decl(true, Some(async_token.span))?;
                let span = start.merge(f.span);
                Some(Item {
                    kind: ItemKind::Function(f),
                    exported: true,
                    decorators: vec![],
                    span,
                })
            }
            TokenKind::Function => {
                let f = self.parse_function_decl(false, None)?;
                let span = start.merge(f.span);
                Some(Item {
                    kind: ItemKind::Function(f),
                    exported: true,
                    decorators: vec![],
                    span,
                })
            }
            TokenKind::Type => {
                let mut item = self.parse_type_or_enum_def()?;
                item.exported = true;
                item.span = start.merge(item.span);
                Some(item)
            }
            TokenKind::Class => {
                let cls = self.parse_class_def(false)?;
                let span = start.merge(cls.span);
                Some(Item {
                    kind: ItemKind::Class(cls),
                    exported: true,
                    decorators: vec![],
                    span,
                })
            }
            TokenKind::Abstract => {
                if self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == TokenKind::Class)
                {
                    self.advance(); // consume `abstract`
                    let cls = self.parse_class_def(true)?;
                    let span = start.merge(cls.span);
                    Some(Item {
                        kind: ItemKind::Class(cls),
                        exported: true,
                        decorators: vec![],
                        span,
                    })
                } else {
                    let current = self.current_token().clone();
                    self.diagnostics.push(
                        Diagnostic::error("expected `class` after `abstract`").with_label(
                            current.span,
                            self.file_id,
                            "expected `class`",
                        ),
                    );
                    self.synchronize();
                    None
                }
            }
            TokenKind::Interface => {
                let iface = self.parse_interface_def()?;
                let span = start.merge(iface.span);
                Some(Item {
                    kind: ItemKind::Interface(iface),
                    exported: true,
                    decorators: vec![],
                    span,
                })
            }
            TokenKind::Star => {
                // Wildcard re-export: `export * from "path";`
                self.advance(); // consume `*`

                self.expect_keyword(&TokenKind::From, "from")?;

                let source = self.parse_string_literal()?;

                // Optional semicolon
                if self.check(&TokenKind::Semicolon) {
                    self.advance();
                }

                let span = start.merge(source.span);
                Some(Item {
                    kind: ItemKind::WildcardReExport(WildcardReExportDecl { source, span }),
                    exported: true,
                    decorators: vec![],
                    span,
                })
            }
            TokenKind::LBrace => {
                // Re-export: `export { Name } from "path";`
                self.advance(); // consume `{`
                let names = self.parse_import_name_list();
                self.expect(&TokenKind::RBrace)?;

                self.expect_keyword(&TokenKind::From, "from")?;

                let source = self.parse_string_literal()?;

                // Optional semicolon
                if self.check(&TokenKind::Semicolon) {
                    self.advance();
                }

                let span = start.merge(source.span);
                Some(Item {
                    kind: ItemKind::ReExport(ReExportDecl {
                        names,
                        source,
                        span,
                    }),
                    exported: true,
                    decorators: vec![],
                    span,
                })
            }
            TokenKind::Const
                if self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == TokenKind::Enum) =>
            {
                let mut item = self.parse_const_enum_def()?;
                item.exported = true;
                item.span = start.merge(item.span);
                Some(item)
            }
            TokenKind::Enum => {
                let mut item = self.parse_ts_enum_def(false)?;
                item.exported = true;
                item.span = start.merge(item.span);
                Some(item)
            }
            TokenKind::Const | TokenKind::Let | TokenKind::Var => {
                let mut item = self.parse_top_level_const()?;
                item.exported = true;
                item.span = start.merge(item.span);
                Some(item)
            }
            TokenKind::Declare => {
                // `export declare ...` — ambient declaration, skip entirely.
                self.skip_declare_body();
                None
            }
            TokenKind::Ident(name)
                if name == "namespace"
                    || (name == "module"
                        && self
                            .tokens
                            .get(self.pos + 1)
                            .is_some_and(|t| matches!(t.kind, TokenKind::Ident(_)))) =>
            {
                self.parse_namespace_diagnostic();
                None
            }
            _ => {
                let current = self.current_token().clone();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "expected `function`, `type`, `interface`, `class`, `abstract`, or `{{` after `export`, found {}",
                        Self::describe_kind(&current.kind)
                    ))
                    .with_label(current.span, self.file_id, "unexpected token"),
                );
                self.synchronize();
                None
            }
        }
    }

    /// Parse a comma-separated list of identifiers inside `{ ... }` for imports/exports.
    fn parse_import_name_list(&mut self) -> Vec<Ident> {
        let mut names = Vec::new();
        loop {
            if self.check(&TokenKind::RBrace) || self.at_end() {
                break;
            }
            if let Some(name) = self.parse_ident() {
                names.push(name);
            } else {
                break;
            }
            if !self.check(&TokenKind::Comma) {
                break;
            }
            self.advance(); // consume `,`
        }
        names
    }

    /// Parse a string literal token and return a [`StringLiteral`].
    fn parse_string_literal(&mut self) -> Option<StringLiteral> {
        let token = self.current_token().clone();
        if let TokenKind::StringLit(value) = &token.kind {
            let lit = StringLiteral {
                value: value.clone(),
                span: token.span,
            };
            self.advance();
            Some(lit)
        } else {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "expected string literal, found {}",
                    Self::describe_kind(&token.kind)
                ))
                .with_label(token.span, self.file_id, "expected string literal"),
            );
            None
        }
    }

    /// Expect a specific keyword token.
    fn expect_keyword(&mut self, kind: &TokenKind, name: &str) -> Option<Token> {
        if self.check(kind) {
            Some(self.advance())
        } else {
            let current = self.current_token().clone();
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "expected `{name}`, found {}",
                    Self::describe_kind(&current.kind)
                ))
                .with_label(
                    current.span,
                    self.file_id,
                    format!("expected `{name}`"),
                ),
            );
            None
        }
    }

    // ---------------------------------------------------------------
    // Function declarations
    // ---------------------------------------------------------------

    /// Parse a function declaration: `[async] function IDENT<T>( params ) : type { body }`.
    ///
    /// The `is_async` flag indicates whether an `async` keyword was consumed
    /// before calling this method. `async_span` is the span of that keyword
    /// (used to extend the overall function span).
    fn parse_function_decl(&mut self, is_async: bool, async_span: Option<Span>) -> Option<FnDecl> {
        let doc_comment = self.take_pending_doc();
        let fn_token = self.advance(); // consume `function`
        let start = async_span.unwrap_or(fn_token.span);

        // Check for generator syntax: `function*`
        let is_generator = self.eat(&TokenKind::Star);

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

        // Overload signature: `function name(params): Type;` — no body.
        // TypeScript allows multiple overload signatures before the implementation.
        // We silently skip overload signatures (they are for type-checking only).
        if self.eat(&TokenKind::Semicolon) {
            return None;
        }

        // Body
        let body = self.parse_block()?;

        let span = start.merge(body.span);
        Some(FnDecl {
            is_async,
            is_generator,
            name,
            type_params,
            params,
            return_type,
            body,
            doc_comment,
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

            // Check for assertion function: `asserts param is Type` or `asserts param`
            // If the parsed type is `Named("asserts")` and the next token is an identifier,
            // reinterpret as an assertion return type.
            if let TypeKind::Named(ref asserts_ident) = type_ann.kind
                && asserts_ident.name == "asserts"
                && matches!(self.peek(), TokenKind::Ident(_))
            {
                let param = self.parse_ident()?;
                let guarded_type = if self.check_contextual_keyword("is") {
                    self.advance(); // consume `is`
                    Some(Box::new(self.parse_type_annotation()?))
                } else {
                    None
                };
                let end_span = guarded_type.as_ref().map_or(param.span, |t| t.span);
                let asserts_ann = TypeAnnotation {
                    kind: TypeKind::Asserts {
                        param,
                        guarded_type,
                    },
                    span: start_span.merge(end_span),
                };
                return Some(ReturnTypeAnnotation {
                    type_ann: Some(asserts_ann.clone()),
                    throws: None,
                    span: asserts_ann.span,
                });
            }

            // Check for type guard: `param is Type`
            // If the parsed type is a plain identifier and the next token is `is`,
            // reinterpret as a type guard predicate.
            if let TypeKind::Named(ref param_ident) = type_ann.kind
                && self.check_contextual_keyword("is")
            {
                self.advance(); // consume `is`
                let guarded_type = self.parse_type_annotation()?;
                let end_span = guarded_type.span;
                let guard_ann = TypeAnnotation {
                    kind: TypeKind::TypeGuard {
                        param: param_ident.clone(),
                        guarded_type: Box::new(guarded_type),
                    },
                    span: start_span.merge(end_span),
                };
                return Some(ReturnTypeAnnotation {
                    type_ann: Some(guard_ann.clone()),
                    throws: None,
                    span: guard_ann.span,
                });
            }

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
    #[allow(clippy::too_many_lines)]
    // Type/enum/alias discrimination requires testing multiple production paths;
    // splitting would obscure the grammar alternatives.
    fn parse_type_or_enum_def(&mut self) -> Option<Item> {
        let doc_comment = self.take_pending_doc();
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
        let item = match self.peek() {
            TokenKind::StringLit(_) => {
                // Simple enum: type Name = "a" | "b" | "c"
                let mut enum_def = self.parse_simple_enum(name, start)?;
                enum_def.derives = self.parse_derives_clause();
                enum_def.doc_comment = doc_comment;
                // Update span to include derives
                let span = if enum_def.derives.is_empty() {
                    enum_def.span
                } else {
                    start.merge(self.previous_span())
                };
                enum_def.span = span;
                Some(Item {
                    kind: ItemKind::EnumDef(enum_def),
                    exported: false,
                    decorators: vec![],
                    span,
                })
            }
            TokenKind::Pipe => {
                // Data enum: type Name = | { kind: "a", ... } | { kind: "b", ... }
                let mut enum_def = self.parse_data_enum(name, start)?;
                enum_def.derives = self.parse_derives_clause();
                enum_def.doc_comment = doc_comment;
                let span = if enum_def.derives.is_empty() {
                    enum_def.span
                } else {
                    start.merge(self.previous_span())
                };
                enum_def.span = span;
                Some(Item {
                    kind: ItemKind::EnumDef(enum_def),
                    exported: false,
                    decorators: vec![],
                    span,
                })
            }
            TokenKind::LBrace => {
                // Could be:
                // - Mapped type: type Name = { [K in keyof T]: V }
                // - Struct type def: type Name = { field: Type, ... }
                // - Index signature: type Name = { [key: string]: string }
                let brace_start = self.current_token().span;
                self.expect(&TokenKind::LBrace)?;

                // Check for mapped type: `[` ident `in` or `readonly [` ident `in`
                if self.check_mapped_type_start() || self.check_readonly_mapped_type_start() {
                    let is_readonly = self.check_readonly_mapped_type_start();
                    let mapped = self.parse_mapped_type(brace_start, is_readonly)?;
                    let derives = self.parse_derives_clause();
                    let span = if derives.is_empty() {
                        start.merge(mapped.span)
                    } else {
                        start.merge(self.previous_span())
                    };
                    let td = TypeDef {
                        name,
                        type_params,
                        fields: Vec::new(),
                        index_signature: None,
                        type_alias: Some(mapped),
                        derives,
                        doc_comment,
                        span,
                    };
                    Some(Item {
                        kind: ItemKind::TypeDef(td),
                        exported: false,
                        decorators: vec![],
                        span,
                    })
                } else {
                    let (fields, index_signature) = self.parse_field_def_list();
                    let close = self.expect(&TokenKind::RBrace)?;
                    let derives = self.parse_derives_clause();
                    let span = if derives.is_empty() {
                        start.merge(close.span)
                    } else {
                        start.merge(self.previous_span())
                    };
                    let td = TypeDef {
                        name,
                        type_params,
                        fields,
                        index_signature,
                        type_alias: None,
                        derives,
                        doc_comment,
                        span,
                    };
                    Some(Item {
                        kind: ItemKind::TypeDef(td),
                        exported: false,
                        decorators: vec![],
                        span,
                    })
                }
            }
            _ => {
                // Type alias: type Name = SomeType or type Name = Utility<T>
                let alias_ann = self.parse_type_annotation()?;
                let derives = self.parse_derives_clause();
                let span = if derives.is_empty() {
                    start.merge(alias_ann.span)
                } else {
                    start.merge(self.previous_span())
                };
                let td = TypeDef {
                    name,
                    type_params,
                    fields: Vec::new(),
                    index_signature: None,
                    type_alias: Some(alias_ann),
                    derives,
                    doc_comment,
                    span,
                };
                Some(Item {
                    kind: ItemKind::TypeDef(td),
                    exported: false,
                    decorators: vec![],
                    span,
                })
            }
        };

        // Optional trailing semicolon
        if self.check(&TokenKind::Semicolon) {
            self.advance();
        }

        item
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
            derives: Vec::new(),
            doc_comment: None,
            is_const: false,
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
                    optional: false,
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
            derives: Vec::new(),
            doc_comment: None,
            is_const: false,
            span,
        })
    }

    /// Parse a `const enum Name { Variant, ... }` declaration.
    ///
    /// Consumes `const` and `enum`, then delegates to [`parse_ts_enum_def`]
    /// with `is_const = true`.
    fn parse_const_enum_def(&mut self) -> Option<Item> {
        self.advance(); // consume `const`
        self.parse_ts_enum_def(true)
    }

    /// Parse a TypeScript-style `enum Name { Variant, Variant = value, ... }` declaration.
    ///
    /// Produces an `ItemKind::EnumDef` with `Simple` variants. Each variant is an
    /// identifier (optionally with `= <integer>` value, which is currently ignored).
    /// When `is_const` is true, the `EnumDef.is_const` flag is set.
    fn parse_ts_enum_def(&mut self, is_const: bool) -> Option<Item> {
        let doc_comment = self.take_pending_doc();
        let start = self.advance().span; // consume `enum`

        let name = self.parse_ident()?;

        self.expect(&TokenKind::LBrace)?;

        let mut variants = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let variant_name = self.parse_ident()?;
            let variant_span = variant_name.span;

            // Optional initializer: `= <expr>` — we parse and discard the value
            if self.eat(&TokenKind::Eq) {
                // Consume the initializer expression (supports negative numbers too)
                if self.check(&TokenKind::Minus) {
                    self.advance(); // consume `-`
                }
                if self.check(&TokenKind::Eof) {
                    break;
                }
                self.advance(); // consume the value
            }

            variants.push(EnumVariant::Simple(variant_name, variant_span));

            // Allow trailing comma
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::RBrace)?;

        let derives = self.parse_derives_clause();
        let span = start.merge(self.previous_span());

        let enum_def = EnumDef {
            name,
            variants,
            derives,
            doc_comment,
            is_const,
            span,
        };

        Some(Item {
            kind: ItemKind::EnumDef(enum_def),
            exported: false,
            decorators: vec![],
            span,
        })
    }

    /// Parse an optional `derives Ident, Ident, ...` clause.
    ///
    /// Returns an empty vec if no `derives` keyword is present.
    /// Used after type definitions, enum definitions, and in class headers.
    fn parse_derives_clause(&mut self) -> Vec<Ident> {
        if !self.check(&TokenKind::Derives) {
            return Vec::new();
        }
        self.advance(); // consume `derives`

        let mut derives = Vec::new();
        while let Some(name) = self.parse_ident() {
            derives.push(name);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            // Stop if we hit a semicolon or opening brace
            if self.check(&TokenKind::Semicolon) || self.check(&TokenKind::LBrace) {
                break;
            }
        }
        derives
    }

    /// Parse a comma-separated list of field definitions: `name: Type, ...`.
    fn parse_field_def_list(&mut self) -> (Vec<FieldDef>, Option<IndexSignature>) {
        let mut fields = Vec::new();
        let mut index_sig = None;

        if self.check(&TokenKind::RBrace) || self.at_end() {
            return (fields, index_sig);
        }

        loop {
            // Check for index signature: `[key: KeyType]: ValueType`
            if self.check(&TokenKind::LBracket) {
                if let Some(sig) = self.parse_index_signature() {
                    index_sig = Some(sig);
                    // Allow trailing comma after index signature
                    self.eat(&TokenKind::Comma);
                    if self.check(&TokenKind::RBrace) || self.at_end() {
                        break;
                    }
                    continue;
                }
                break;
            }

            let field_start = self.current_token().span;
            let Some(name) = self.parse_ident() else {
                break;
            };
            // Check for optional field syntax: `name?: Type`
            let optional = self.eat(&TokenKind::Question);
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
                optional,
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

        (fields, index_sig)
    }

    /// Parse an index signature: `[key: KeyType]: ValueType`.
    ///
    /// Called when `[` is seen in a type definition body. Returns `None` if
    /// the pattern doesn't match the index signature syntax.
    fn parse_index_signature(&mut self) -> Option<IndexSignature> {
        let start = self.current_token().span;
        self.advance(); // consume `[`

        let key_name = self.parse_ident()?;
        self.expect(&TokenKind::Colon)?;
        let key_type = self.parse_type_annotation()?;
        self.expect(&TokenKind::RBracket)?;
        self.expect(&TokenKind::Colon)?;
        let value_type = self.parse_type_annotation()?;

        let span = start.merge(value_type.span);
        Some(IndexSignature {
            key_name,
            key_type,
            value_type,
            span,
        })
    }

    /// Check whether the current position starts a mapped type: `[` ident `in`.
    ///
    /// This distinguishes `{ [K in keyof T]: V }` (mapped type) from
    /// `{ [key: string]: V }` (index signature).
    fn check_mapped_type_start(&self) -> bool {
        if !self.check(&TokenKind::LBracket) {
            return false;
        }
        // Lookahead: `[` ident `in`
        let after_bracket = self.tokens.get(self.pos + 1).map(|t| &t.kind);
        let after_ident = self.tokens.get(self.pos + 2).map(|t| &t.kind);
        matches!(after_bracket, Some(TokenKind::Ident(_)))
            && matches!(after_ident, Some(TokenKind::In))
    }

    /// Check whether the current position starts a readonly mapped type: `readonly` `[` ident `in`.
    fn check_readonly_mapped_type_start(&self) -> bool {
        let cur = self.peek();
        let is_readonly = matches!(cur, TokenKind::Ident(name) if name == "readonly");
        if !is_readonly {
            return false;
        }
        let after_readonly = self.tokens.get(self.pos + 1).map(|t| &t.kind);
        let after_bracket = self.tokens.get(self.pos + 2).map(|t| &t.kind);
        let after_ident = self.tokens.get(self.pos + 3).map(|t| &t.kind);
        matches!(after_readonly, Some(TokenKind::LBracket))
            && matches!(after_bracket, Some(TokenKind::Ident(_)))
            && matches!(after_ident, Some(TokenKind::In))
    }

    /// Parse a mapped type: `{ [K in keyof T]: V }`, `{ readonly [K in keyof T]?: V }`, etc.
    ///
    /// Called after `{` has been consumed. `has_readonly` indicates whether a `readonly` prefix
    /// was detected by lookahead.
    fn parse_mapped_type(&mut self, start: Span, has_readonly: bool) -> Option<TypeAnnotation> {
        // Parse optional readonly modifier
        let readonly = if has_readonly {
            self.advance(); // consume `readonly`
            Some(MappedModifier::Add)
        } else {
            None
        };

        self.expect(&TokenKind::LBracket)?; // consume `[`
        let type_param = self.parse_ident()?;
        self.expect(&TokenKind::In)?; // consume `in`
        let constraint = self.parse_type_annotation()?;
        self.expect(&TokenKind::RBracket)?; // consume `]`

        // Parse optional modifier: `?`, `-?`, `+?`
        let optional = if self.eat(&TokenKind::Question) {
            Some(MappedModifier::Add)
        } else if self.check(&TokenKind::Minus) {
            // Lookahead: `-?`
            if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Question) {
                self.advance(); // consume `-`
                self.advance(); // consume `?`
                Some(MappedModifier::Remove)
            } else {
                None
            }
        } else if self.check(&TokenKind::Plus) {
            // Lookahead: `+?`
            if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Question) {
                self.advance(); // consume `+`
                self.advance(); // consume `?`
                Some(MappedModifier::Add)
            } else {
                None
            }
        } else {
            None
        };

        self.expect(&TokenKind::Colon)?; // consume `:`
        let value_type = self.parse_type_annotation()?;

        // Allow optional semicolon or comma before closing brace
        self.eat(&TokenKind::Semicolon);
        self.eat(&TokenKind::Comma);

        let close = self.expect(&TokenKind::RBrace)?;
        let span = start.merge(close.span);
        Some(TypeAnnotation {
            kind: TypeKind::MappedType {
                type_param,
                constraint: Box::new(constraint),
                value_type: Box::new(value_type),
                optional,
                readonly,
            },
            span,
        })
    }

    // ---------------------------------------------------------------
    // Interface definitions
    // ---------------------------------------------------------------

    /// Parse an interface definition: `interface Name<T> { method(): Type; ... }`.
    fn parse_interface_def(&mut self) -> Option<InterfaceDef> {
        let doc_comment = self.take_pending_doc();
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
            doc_comment,
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
    // Class definitions
    // ---------------------------------------------------------------

    /// Parse a class definition: `class Name [<T>] [implements I1, I2] { members }`.
    fn parse_class_def(&mut self, is_abstract: bool) -> Option<ClassDef> {
        let doc_comment = self.take_pending_doc();
        let class_token = self.advance(); // consume `class`
        let start = class_token.span;

        let name = self.parse_ident()?;

        // Optional generic type parameters
        let type_params = if self.check(&TokenKind::Lt) {
            Some(self.parse_type_params()?)
        } else {
            None
        };

        // Optional extends clause (single inheritance)
        let extends = if self.check(&TokenKind::Extends) {
            self.advance(); // consume `extends`
            Some(self.parse_ident()?)
        } else {
            None
        };

        // Optional implements clause
        let mut implements = Vec::new();
        let mut derives = Vec::new();
        if self.check(&TokenKind::Implements) {
            self.advance(); // consume `implements`
            loop {
                // Check for `derives` keyword — signals end of implements, start of derives
                if self.check(&TokenKind::Derives) {
                    break;
                }
                let iface = self.parse_ident()?;
                implements.push(iface);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

        // Optional derives clause: `derives Serialize, Deserialize`
        // Can appear after implements, or standalone before the class body
        if self.check(&TokenKind::Derives) {
            self.advance(); // consume `derives`
            loop {
                let derive_name = self.parse_ident()?;
                derives.push(derive_name);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                // Stop if we hit the opening brace
                if self.check(&TokenKind::LBrace) {
                    break;
                }
            }
        }

        self.expect(&TokenKind::LBrace)?;

        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            // Consume JSDoc tokens before each class member
            self.consume_jsdoc_tokens();
            let before = self.pos;
            if let Some(member) = self.parse_class_member() {
                members.push(member);
            } else if self.pos == before {
                // Error recovery: only skip when no forward progress was made.
                // Overload signatures (constructor or method) return None after
                // consuming tokens, so we must not enter recovery in that case.
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

        Some(ClassDef {
            is_abstract,
            name,
            type_params,
            extends,
            implements,
            derives,
            members,
            doc_comment,
            span,
        })
    }

    /// Parse a class expression: `class { ... }` or `class Name { ... }`.
    ///
    /// Similar to [`Self::parse_class_def`] but the name is optional. When the
    /// class is anonymous, a placeholder name `__AnonymousClass` is used; the
    /// lowering pass replaces it with the binding variable name.
    fn parse_class_expr(&mut self) -> Option<Expr> {
        let class_token = self.advance(); // consume `class`
        let start = class_token.span;

        // Name is optional for class expressions
        let name = if matches!(self.peek(), TokenKind::Ident(_)) && !self.check(&TokenKind::Extends)
        {
            self.parse_ident()?
        } else {
            Ident {
                name: "__AnonymousClass".to_owned(),
                span: start,
            }
        };

        // Optional generic type parameters
        let type_params = if self.check(&TokenKind::Lt) {
            Some(self.parse_type_params()?)
        } else {
            None
        };

        // Optional extends clause
        let extends = if self.check(&TokenKind::Extends) {
            self.advance(); // consume `extends`
            Some(self.parse_ident()?)
        } else {
            None
        };

        // Optional implements clause
        let mut implements = Vec::new();
        let mut derives = Vec::new();
        if self.check(&TokenKind::Implements) {
            self.advance(); // consume `implements`
            loop {
                if self.check(&TokenKind::Derives) {
                    break;
                }
                let iface = self.parse_ident()?;
                implements.push(iface);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

        // Optional derives clause
        if self.check(&TokenKind::Derives) {
            self.advance(); // consume `derives`
            loop {
                let derive_name = self.parse_ident()?;
                derives.push(derive_name);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                if self.check(&TokenKind::LBrace) {
                    break;
                }
            }
        }

        self.expect(&TokenKind::LBrace)?;

        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            self.consume_jsdoc_tokens();
            let before = self.pos;
            if let Some(member) = self.parse_class_member() {
                members.push(member);
            } else if self.pos == before {
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

        Some(Expr {
            kind: ExprKind::ClassExpr(ClassDef {
                is_abstract: false,
                name,
                type_params,
                extends,
                implements,
                derives,
                members,
                doc_comment: None,
                span,
            }),
            span,
        })
    }

    /// Check whether the current token is an identifier matching the given name.
    ///
    /// Used for contextual keywords (`get`, `set`, `static`, `readonly`) that are
    /// only special in certain positions and should remain valid identifiers elsewhere.
    fn check_contextual_keyword(&self, name: &str) -> bool {
        matches!(self.peek(), TokenKind::Ident(ident_name) if ident_name == name)
    }

    /// Parse a single class member: field, constructor, method, getter, or setter.
    ///
    /// Disambiguates based on leading tokens:
    /// - `constructor(` → constructor
    /// - `get name()` → getter
    /// - `set name(param)` → setter
    /// - `[private|public] [static] [readonly] name: Type [= expr];` → field
    /// - `[private|public] [static] [async] name(params): ReturnType { body }` → method
    #[allow(clippy::too_many_lines)]
    // Class member parsing must disambiguate many syntactic forms; splitting would
    // fragment the grammar-driven structure.
    fn parse_class_member(&mut self) -> Option<ClassMember> {
        let doc_comment = self.take_pending_doc();

        // Check for constructor
        if matches!(self.peek(), TokenKind::Constructor) {
            return self
                .parse_class_constructor(doc_comment)
                .map(ClassMember::Constructor);
        }

        // Parse optional visibility modifier
        let visibility = match self.peek() {
            TokenKind::Private => {
                self.advance();
                Visibility::Private
            }
            TokenKind::Public => {
                self.advance();
                Visibility::Public
            }
            _ => Visibility::Public,
        };

        let member_start = self.current_token().span;

        // Check for getter: `get name(): Type { ... }`
        // Lookahead to disambiguate: `get name(` means getter, `get(` means method named "get"
        if self.check_contextual_keyword("get") {
            let next_is_ident_or_keyword = self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| matches!(t.kind, TokenKind::Ident(_)));
            if next_is_ident_or_keyword {
                self.advance(); // consume `get`
                let name = self.parse_ident()?;
                self.expect(&TokenKind::LParen)?;
                self.expect(&TokenKind::RParen)?;
                let return_type = self.parse_return_type_annotation();
                let body = self.parse_block()?;
                let span = member_start.merge(body.span);
                return Some(ClassMember::Getter(ClassGetter {
                    visibility,
                    name,
                    return_type,
                    body,
                    span,
                }));
            }
        }

        // Check for setter: `set name(param: Type) { ... }`
        if self.check_contextual_keyword("set") {
            let next_is_ident_or_keyword = self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| matches!(t.kind, TokenKind::Ident(_)));
            if next_is_ident_or_keyword {
                self.advance(); // consume `set`
                let name = self.parse_ident()?;
                self.expect(&TokenKind::LParen)?;
                let params = self.parse_param_list();
                let param = params.into_iter().next()?;
                self.expect(&TokenKind::RParen)?;
                let body = self.parse_block()?;
                let span = member_start.merge(body.span);
                return Some(ClassMember::Setter(ClassSetter {
                    visibility,
                    name,
                    param,
                    body,
                    span,
                }));
            }
        }

        // Check for static initialization block: `static { ... }`
        // Must come before consuming `static` as a modifier, since the block
        // syntax is `static` immediately followed by `{`.
        if self.check_contextual_keyword("static")
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == TokenKind::LBrace)
        {
            self.advance(); // consume `static`
            let block = self.parse_block()?;
            return Some(ClassMember::StaticBlock(block));
        }

        // Check for `abstract` modifier
        let is_abstract = self.eat(&TokenKind::Abstract);

        // Check for `override` modifier
        let is_override = self.eat(&TokenKind::Override);

        // Check for `static` modifier (contextual keyword)
        let is_static = self.eat_contextual_keyword("static");

        // Check for `readonly` modifier (contextual keyword)
        let readonly = self.eat_contextual_keyword("readonly");

        // Check for optional `async` modifier before the method name
        let is_async = !readonly && self.eat(&TokenKind::Async);

        // Check for hash-private field: `#field: Type;`
        let is_hash_private = matches!(self.peek(), TokenKind::Ident(n) if n.starts_with('#'));

        let name = self.parse_ident()?;

        // Strip `#` prefix from field name in the AST (privacy is tracked via is_hash_private)
        let field_name = if is_hash_private {
            Ident {
                name: name.name.trim_start_matches('#').to_owned(),
                span: name.span,
            }
        } else {
            name.clone()
        };

        // Field: `name: Type [= expr];` (only if not async — async fields don't exist)
        if !is_async && self.check(&TokenKind::Colon) {
            self.advance(); // consume `:`
            let type_ann = self.parse_type_annotation()?;

            // Optional initializer: `= expr`
            let initializer = if self.eat(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };

            self.expect(&TokenKind::Semicolon)?;
            let span = member_start.merge(self.previous_span());
            return Some(ClassMember::Field(ClassField {
                visibility,
                name: field_name,
                type_ann,
                initializer,
                readonly,
                is_static,
                is_hash_private,
                doc_comment,
                span,
            }));
        }

        // Method: `[abstract] [override] [static] [async] name<T>(params): ReturnType { body }`
        // or abstract method: `abstract name(params): ReturnType;` (no body)
        let type_params = if self.check(&TokenKind::Lt) {
            Some(self.parse_type_params()?)
        } else {
            None
        };

        self.expect(&TokenKind::LParen)?;
        let params = self.parse_param_list();
        self.expect(&TokenKind::RParen)?;

        let return_type = self.parse_return_type_annotation();

        // Abstract methods have no body — terminated with `;`
        if is_abstract {
            // Optional semicolon for abstract methods
            if self.check(&TokenKind::Semicolon) {
                self.advance();
            }
            let span = member_start.merge(self.previous_span());
            return Some(ClassMember::Method(ClassMethod {
                is_async,
                is_static,
                is_abstract: true,
                is_override,
                visibility,
                name,
                type_params,
                params,
                return_type,
                body: Block {
                    stmts: Vec::new(),
                    span,
                },
                doc_comment,
                span,
            }));
        }

        // Method overload signature: `name(params): Type;` — no body.
        // TypeScript allows multiple overload signatures before the implementation.
        // We silently skip overload signatures (they are for type-checking only).
        if self.eat(&TokenKind::Semicolon) {
            return None;
        }

        let body = self.parse_block()?;
        let span = member_start.merge(body.span);

        Some(ClassMember::Method(ClassMethod {
            is_async,
            is_static,
            is_abstract: false,
            is_override,
            visibility,
            name,
            type_params,
            params,
            return_type,
            body,
            doc_comment,
            span,
        }))
    }

    /// Parse a class constructor: `constructor([public|private] params) { body }`.
    ///
    /// Constructor parameters may have visibility modifiers to create parameter properties.
    /// Constructor overload signatures (`constructor(params);`) are silently skipped.
    fn parse_class_constructor(&mut self, doc_comment: Option<String>) -> Option<ClassConstructor> {
        let ctor_token = self.advance(); // consume `constructor`
        let start = ctor_token.span;

        self.expect(&TokenKind::LParen)?;
        let params = self.parse_constructor_param_list();
        self.expect(&TokenKind::RParen)?;

        // Constructor overload signature: `constructor(params);` — no body.
        // TypeScript allows multiple overload signatures before the implementation.
        // We silently skip overload signatures (they are for type-checking only).
        if self.eat(&TokenKind::Semicolon) {
            return None;
        }

        let body = self.parse_block()?;
        let span = start.merge(body.span);

        Some(ClassConstructor {
            params,
            body,
            doc_comment,
            span,
        })
    }

    /// Parse a constructor parameter list with optional visibility modifiers.
    ///
    /// Each parameter may be prefixed with `public` or `private` to create
    /// a parameter property that auto-generates a field.
    fn parse_constructor_param_list(&mut self) -> Vec<ConstructorParam> {
        use rsc_syntax::ast::ConstructorParam;
        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.at_end() {
            let param_start = self.current_token().span;

            // Check for optional visibility modifier (parameter property)
            let property_visibility = match self.peek() {
                TokenKind::Public => {
                    self.advance();
                    Some(Visibility::Public)
                }
                TokenKind::Private => {
                    self.advance();
                    Some(Visibility::Private)
                }
                _ => None,
            };

            let Some(name) = self.parse_ident() else {
                break;
            };
            if !self.eat(&TokenKind::Colon) {
                break;
            }
            let Some(type_ann) = self.parse_type_annotation() else {
                break;
            };
            let span = param_start.merge(self.previous_span());
            params.push(ConstructorParam {
                property_visibility,
                name,
                type_ann,
                span,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        params
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

    /// Parse a single parameter: `IDENT : type`, `IDENT?: type`,
    /// `IDENT: type = default`, or `...IDENT: Array<type>`.
    fn parse_param(&mut self) -> Option<Param> {
        let start = self.current_token().span;

        // Rest parameter: `...name: Array<Type>`
        let is_rest = self.eat(&TokenKind::DotDotDot);

        let name = self.parse_ident()?;

        // Optional parameter: `name?: Type`
        let optional = self.eat(&TokenKind::Question);

        self.expect(&TokenKind::Colon)?;
        let type_ann = self.parse_type_annotation()?;

        // Default value: `name: Type = expr`
        let default_value = if self.eat(&TokenKind::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let end_span = default_value.as_ref().map_or(type_ann.span, |dv| dv.span);
        let span = start.merge(end_span);
        Some(Param {
            name,
            type_ann,
            optional,
            default_value,
            is_rest,
            span,
        })
    }

    /// Parse a comma-separated closure parameter list (without the surrounding parens).
    ///
    /// Closure parameters may omit their type annotations: `(a, b)` is valid
    /// in addition to `(a: i32, b: i32)`.
    fn parse_closure_param_list(&mut self) -> Vec<Param> {
        let mut params = Vec::new();

        if self.check(&TokenKind::RParen) || self.at_end() {
            return params;
        }

        loop {
            if let Some(param) = self.parse_closure_param() {
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

    /// Parse a single closure parameter: `IDENT` or `IDENT : type`.
    ///
    /// When the type annotation is omitted, uses `TypeKind::Inferred`.
    fn parse_closure_param(&mut self) -> Option<Param> {
        let start = self.current_token().span;
        let name = self.parse_ident()?;

        // Type annotation is optional in closures
        if self.check(&TokenKind::Colon) {
            self.advance(); // consume `:`
            let type_ann = self.parse_type_annotation()?;
            let span = start.merge(type_ann.span);
            Some(Param {
                name,
                type_ann,
                optional: false,
                default_value: None,
                is_rest: false,
                span,
            })
        } else {
            let span = name.span;
            Some(Param {
                name,
                type_ann: TypeAnnotation {
                    kind: TypeKind::Inferred,
                    span,
                },
                optional: false,
                default_value: None,
                is_rest: false,
                span,
            })
        }
    }

    /// Parse a type annotation: `void`, a named type, a generic type, a union type,
    /// an intersection type, or a conditional type.
    ///
    /// Handles `void`, `i32`, `Container<T>`, `Map<string, u32>`, `T | null`,
    /// `Serializable & Printable`, `T extends U ? A : B`, etc.
    fn parse_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let base = self.parse_non_conditional_type()?;

        // Check for conditional type: `T extends U ? TrueType : FalseType`
        // Conditional types bind loosely — they wrap function types and unions.
        if self.check(&TokenKind::Extends) {
            return self.parse_conditional_type(base);
        }

        Some(base)
    }

    /// Parse a type annotation that is NOT a conditional type.
    ///
    /// Used for positions where conditional types should not be consumed
    /// (e.g., function return types, generic arguments). This prevents
    /// `(A) => B extends C ? D : E` from being parsed as `(A) => (B extends C ? D : E)`.
    fn parse_non_conditional_type(&mut self) -> Option<TypeAnnotation> {
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

    /// Parse a conditional type after the check type has been parsed.
    ///
    /// Called when `extends` is the next token. Parses the constraint type,
    /// `?`, true branch, `:`, and false branch.
    /// Syntax: `CheckType extends ConstraintType ? TrueType : FalseType`
    fn parse_conditional_type(&mut self, check_type: TypeAnnotation) -> Option<TypeAnnotation> {
        let start_span = check_type.span;
        self.advance(); // consume `extends`
        let extends_type = self.parse_base_type_annotation()?;
        self.expect(&TokenKind::Question)?;
        let true_type = self.parse_type_annotation()?;
        self.expect(&TokenKind::Colon)?;
        let false_type = self.parse_type_annotation()?;
        let span = start_span.merge(false_type.span);
        Some(TypeAnnotation {
            kind: TypeKind::Conditional {
                check_type: Box::new(check_type),
                extends_type: Box::new(extends_type),
                true_type: Box::new(true_type),
                false_type: Box::new(false_type),
            },
            span,
        })
    }

    /// Parse a base (non-union) type annotation: `void`, named, generic, `null`, or function type.
    #[allow(clippy::too_many_lines)]
    // Base type annotation parsing covers all token kinds that start a type; splitting would obscure the match
    fn parse_base_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let token = self.current_token().clone();
        let base_result = match &token.kind {
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
            TokenKind::Void => {
                self.advance();
                Some(TypeAnnotation {
                    kind: TypeKind::Void,
                    span: token.span,
                })
            }
            TokenKind::Ident(name) if name == "never" => {
                self.advance();
                Some(TypeAnnotation {
                    kind: TypeKind::Never,
                    span: token.span,
                })
            }
            TokenKind::Ident(name) if name == "unknown" => {
                self.advance();
                Some(TypeAnnotation {
                    kind: TypeKind::Unknown,
                    span: token.span,
                })
            }
            TokenKind::Ident(name) if name == "shared" => {
                let start = token.span;
                self.advance(); // consume `shared`

                // Require `<T>` type parameter
                if !self.check(&TokenKind::Lt) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "`shared` type requires a type parameter: `shared<T>`".to_owned(),
                        )
                        .with_label(
                            start,
                            self.file_id,
                            "expected `<` after `shared`",
                        ),
                    );
                    return None;
                }
                self.advance(); // consume `<`
                let inner = self.parse_type_annotation()?;
                let close = self.expect(&TokenKind::Gt)?;
                let span = start.merge(close.span);
                Some(TypeAnnotation {
                    kind: TypeKind::Shared(Box::new(inner)),
                    span,
                })
            }
            TokenKind::LBracket => {
                // Tuple type: `[T1, T2, ...]`
                self.parse_tuple_type_annotation()
            }
            TokenKind::Ident(name) if name == "readonly" => {
                // `readonly` type modifier: `readonly T[]` or `readonly [T, U]`
                let start = token.span;
                self.advance(); // consume `readonly`
                let inner = self.parse_base_type_annotation()?;
                let span = start.merge(inner.span);
                Some(TypeAnnotation {
                    kind: TypeKind::Readonly(Box::new(inner)),
                    span,
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
            TokenKind::LBrace => {
                // Could be:
                // - Mapped type: `{ [K in keyof T]: V }` or `{ readonly [K in ...]: V }`
                // - Index signature: `{ [key: string]: T }`
                let start = token.span;
                self.advance(); // consume `{`

                // Check for mapped type with `readonly` prefix
                if self.check_mapped_type_start() {
                    return self.parse_mapped_type(start, false);
                }
                if self.check_readonly_mapped_type_start() {
                    return self.parse_mapped_type(start, true);
                }

                if self.check(&TokenKind::LBracket) {
                    let sig = self.parse_index_signature()?;
                    // Allow trailing comma
                    self.eat(&TokenKind::Comma);
                    let close = self.expect(&TokenKind::RBrace)?;
                    let span = start.merge(close.span);
                    Some(TypeAnnotation {
                        kind: TypeKind::IndexSignature(Box::new(sig)),
                        span,
                    })
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "expected index signature or mapped type in type annotation",
                        )
                        .with_label(
                            self.current_token().span,
                            self.file_id,
                            "expected `[`",
                        ),
                    );
                    None
                }
            }
            TokenKind::StringLit(value) => {
                // String literal type, used in utility type args: Pick<User, "name" | "age">
                let value = value.clone();
                self.advance();
                Some(TypeAnnotation {
                    kind: TypeKind::StringLiteral(value),
                    span: token.span,
                })
            }
            TokenKind::KeyOf => {
                // keyof T — produces a union of string literal types for the keys of T
                let start = token.span;
                self.advance(); // consume `keyof`
                let inner = self.parse_base_type_annotation()?;
                let span = start.merge(inner.span);
                Some(TypeAnnotation {
                    kind: TypeKind::KeyOf(Box::new(inner)),
                    span,
                })
            }
            TokenKind::TypeOf => {
                // typeof x in type position — resolves to the declared type of x
                let start = token.span;
                self.advance(); // consume `typeof`
                let ident = self.parse_ident()?;
                let span = start.merge(ident.span);
                Some(TypeAnnotation {
                    kind: TypeKind::TypeOf(ident),
                    span,
                })
            }
            TokenKind::Infer => {
                // infer R — bind a type variable in a conditional type extends clause
                let start = token.span;
                self.advance(); // consume `infer`
                let ident = self.parse_ident()?;
                let span = start.merge(ident.span);
                Some(TypeAnnotation {
                    kind: TypeKind::Infer(ident),
                    span,
                })
            }
            TokenKind::TemplateNoSub(_) | TokenKind::TemplateHead(_) => {
                self.parse_template_literal_type()
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
        };

        // Parse postfix index access types: `T[K]`, `T["field"]`
        // Loop to handle chained access: `T[K][J]`
        let mut result: Option<TypeAnnotation> = base_result;
        while result.is_some() && self.check(&TokenKind::LBracket) {
            let base_ty = result.take().expect("checked is_some");
            self.advance(); // consume `[`
            let Some(index_type) = self.parse_type_annotation() else {
                result = Some(base_ty);
                break;
            };
            let close = self.expect(&TokenKind::RBracket)?;
            let span = base_ty.span.merge(close.span);
            result = Some(TypeAnnotation {
                kind: TypeKind::IndexAccess(Box::new(base_ty), Box::new(index_type)),
                span,
            });
        }

        result
    }

    /// Parse a function type annotation: `(i32, string) => i32`.
    ///
    /// Also handles named parameters: `(x: i32, y: string) => bool`.
    /// Named parameter names are discarded — only the types matter.
    ///
    /// Called when `(` is seen in type annotation position.
    fn parse_function_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let start = self.current_token().span;
        self.advance(); // consume `(`

        let mut param_types = Vec::new();
        if !self.check(&TokenKind::RParen) && !self.at_end() {
            loop {
                // If we see `Ident :`, it's a named parameter — consume the name
                // and colon, then parse the type. This supports TypeScript-style
                // function types like `(x: i32) => i32`.
                if matches!(self.peek(), TokenKind::Ident(_))
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .map(|t| &t.kind)
                        == Some(&TokenKind::Colon)
                {
                    self.advance(); // consume parameter name
                    self.advance(); // consume `:`
                }

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
        // Use non-conditional parse so `(A) => B extends C ? D : E`
        // is parsed as `((A) => B) extends C ? D : E`, not `(A) => (B extends ...)`
        let return_type = self.parse_non_conditional_type()?;
        let span = start.merge(return_type.span);

        Some(TypeAnnotation {
            kind: TypeKind::Function(param_types, Box::new(return_type)),
            span,
        })
    }

    /// Parse a tuple type annotation: `[T1, T2, ...]`.
    ///
    /// Called when `[` is seen in type annotation position.
    fn parse_tuple_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let start = self.current_token().span;
        self.advance(); // consume `[`
        let mut types = Vec::new();

        if !self.check(&TokenKind::RBracket) && !self.at_end() {
            loop {
                // Check for spread element: `...T`
                if self.check(&TokenKind::DotDotDot) {
                    let spread_start = self.current_token().span;
                    self.advance(); // consume `...`
                    let inner = self.parse_type_annotation()?;
                    let spread_span = spread_start.merge(inner.span);
                    types.push(TypeAnnotation {
                        kind: TypeKind::TupleSpread(Box::new(inner)),
                        span: spread_span,
                    });
                } else {
                    let ty = self.parse_type_annotation()?;
                    types.push(ty);
                }

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
        Some(TypeAnnotation {
            kind: TypeKind::Tuple(types),
            span,
        })
    }

    /// Parse a template literal type: `` `hello ${string}` `` in type position.
    ///
    /// Template literal types represent compile-time string patterns in TypeScript.
    /// They lower to `String` in Rust since Rust's type system cannot express string patterns.
    /// The parser produces `TypeKind::TemplateLiteralType { quasis, types }` where
    /// `quasis` are the static string fragments and `types` are the interpolated type annotations.
    fn parse_template_literal_type(&mut self) -> Option<TypeAnnotation> {
        let token = self.current_token().clone();
        match &token.kind {
            TokenKind::TemplateNoSub(text) => {
                // No interpolation: `` `hello` ``
                let text = text.clone();
                self.advance();
                Some(TypeAnnotation {
                    kind: TypeKind::TemplateLiteralType {
                        quasis: vec![text],
                        types: Vec::new(),
                    },
                    span: token.span,
                })
            }
            TokenKind::TemplateHead(head_text) => {
                // With interpolations: `` `hello ${Type}...` ``
                let head_text = head_text.clone();
                let start_span = token.span;
                self.advance(); // consume TemplateHead

                let mut quasis = vec![head_text];
                let mut types = Vec::new();

                loop {
                    // Parse the interpolated type
                    let ty = self.parse_type_annotation()?;
                    types.push(ty);

                    // After the type, expect TemplateMiddle or TemplateTail
                    let next = self.current_token().clone();
                    match &next.kind {
                        TokenKind::TemplateTail(tail_text) => {
                            let tail_text = tail_text.clone();
                            let tail_token = self.advance();
                            quasis.push(tail_text);
                            let end_span = tail_token.span;
                            return Some(TypeAnnotation {
                                kind: TypeKind::TemplateLiteralType { quasis, types },
                                span: start_span.merge(end_span),
                            });
                        }
                        TokenKind::TemplateMiddle(mid_text) => {
                            let mid_text = mid_text.clone();
                            self.advance();
                            quasis.push(mid_text);
                            // Continue loop to parse next type
                        }
                        _ => {
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "expected template literal type continuation, found {}",
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
            _ => {
                // Should not be called with other token kinds
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "expected template literal type, found {}",
                        Self::describe_kind(&token.kind)
                    ))
                    .with_label(
                        token.span,
                        self.file_id,
                        "expected template literal",
                    ),
                );
                None
            }
        }
    }

    /// Parse an identifier token into an [`Ident`] AST node.
    ///
    /// Also accepts `delete`, `void`, and `in` keywords as identifiers since
    /// they can be used as method/field names (e.g., `map.delete("key")`).
    fn parse_ident(&mut self) -> Option<Ident> {
        let token = self.current_token().clone();
        match &token.kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Some(Ident {
                    name,
                    span: token.span,
                })
            }
            TokenKind::Delete => {
                self.advance();
                Some(Ident {
                    name: "delete".to_owned(),
                    span: token.span,
                })
            }
            TokenKind::Void => {
                self.advance();
                Some(Ident {
                    name: "void".to_owned(),
                    span: token.span,
                })
            }
            TokenKind::In => {
                self.advance();
                Some(Ident {
                    name: "in".to_owned(),
                    span: token.span,
                })
            }
            TokenKind::From => {
                self.advance();
                Some(Ident {
                    name: "from".to_owned(),
                    span: token.span,
                })
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "expected identifier, found {}",
                        Self::describe_kind(&token.kind)
                    ))
                    .with_label(
                        token.span,
                        self.file_id,
                        "expected identifier",
                    ),
                );
                None
            }
        }
    }

    // ---------------------------------------------------------------
    // Blocks and statements
    // ---------------------------------------------------------------

    /// Parse a block: `{ stmt* }`.
    ///
    /// Tracks block nesting depth to prevent stack overflow on adversarial
    /// input (e.g., 50 nested function declarations). Also guarantees forward
    /// progress: if `parse_stmt` fails to advance the position, the parser
    /// forcibly skips one token to prevent infinite loops.
    fn parse_block(&mut self) -> Option<Block> {
        let open = self.current_token().span;
        self.expect(&TokenKind::LBrace)?;

        self.block_depth += 1;
        if self.block_depth > MAX_BLOCK_DEPTH {
            self.diagnostics.push(
                Diagnostic::error("block nesting depth exceeded maximum").with_label(
                    open,
                    self.file_id,
                    "here",
                ),
            );
            self.block_depth -= 1;
            // Skip to closing brace or EOF to avoid cascading errors
            let mut brace_count = 1u32;
            while !self.at_end() {
                if self.check(&TokenKind::LBrace) {
                    brace_count = brace_count.saturating_add(1);
                } else if self.check(&TokenKind::RBrace) {
                    brace_count = brace_count.saturating_sub(1);
                    if brace_count == 0 {
                        self.advance();
                        break;
                    }
                }
                self.advance();
            }
            return Some(Block {
                stmts: Vec::new(),
                span: open.merge(self.previous_span()),
            });
        }

        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            let before = self.pos;
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            }
            // Safety: if no progress was made, force advance one token to
            // prevent infinite loops from error recovery landing on the same
            // token repeatedly (e.g., `function` keyword inside a block that
            // parse_stmt cannot handle).
            if self.pos == before {
                self.advance();
            }
        }

        self.block_depth -= 1;

        let close_span = if let Some(close) = self.expect(&TokenKind::RBrace) {
            close.span
        } else {
            self.diagnostics.push(
                Diagnostic::error("unterminated block; expected closing `}`").with_label(
                    open,
                    self.file_id,
                    "block starts here",
                ),
            );
            self.previous_span()
        };

        Some(Block {
            stmts,
            span: open.merge(close_span),
        })
    }

    /// Parse a statement.
    ///
    /// Detects labeled loops: `identifier: while/for/do { ... }`.
    fn parse_stmt(&mut self) -> Option<Stmt> {
        // Check for labeled loop: `ident : (while | for | do)`
        if let TokenKind::Ident(name) = self.peek() {
            let name = name.clone();
            if self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == TokenKind::Colon)
            {
                let after_colon = self.tokens.get(self.pos + 2).map(|t| &t.kind);
                if matches!(
                    after_colon,
                    Some(TokenKind::While | TokenKind::For | TokenKind::Do)
                ) {
                    self.advance(); // consume ident
                    self.advance(); // consume ':'
                    return self.parse_labeled_stmt(name);
                }
            }
        }

        // Check for `using ident =` (contextual keyword, not a reserved keyword)
        if let TokenKind::Ident(name) = self.peek()
            && name == "using"
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Ident(_))
            )
        {
            return self.parse_using_decl(false);
        }

        // Check for `await using ident =` (await is a real keyword)
        if matches!(self.peek(), TokenKind::Await)
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Ident(name)) if name == "using"
            )
            && matches!(
                self.tokens.get(self.pos + 2).map(|t| &t.kind),
                Some(TokenKind::Ident(_))
            )
        {
            return self.parse_using_decl(true);
        }

        match self.peek() {
            TokenKind::Const
                if self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == TokenKind::Enum) =>
            {
                // `const enum` at statement level — parse as an item wrapped in a statement.
                // This mirrors top-level parsing: const enum lowers like a regular enum.
                // We can't represent an enum as a statement directly, so we emit a diagnostic.
                let current = self.current_token().clone();
                self.diagnostics.push(
                    Diagnostic::error("`const enum` must appear at the top level").with_label(
                        current.span,
                        self.file_id,
                        "not allowed here",
                    ),
                );
                self.advance(); // consume `const`
                self.advance(); // consume `enum`
                self.synchronize();
                None
            }
            TokenKind::Const | TokenKind::Let | TokenKind::Var => self.parse_var_decl(),
            TokenKind::If => self.parse_if_stmt().map(Stmt::If),
            TokenKind::While => self.parse_while_stmt(None).map(Stmt::While),
            TokenKind::Do => self.parse_do_while_stmt(None).map(Stmt::DoWhile),
            TokenKind::Return => self.parse_return_stmt().map(Stmt::Return),
            TokenKind::Switch => self.parse_switch_stmt().map(Stmt::Switch),
            TokenKind::Try => self.parse_try_catch_stmt().map(Stmt::TryCatch),
            TokenKind::For => self.parse_for_stmt(None),
            TokenKind::Break => Some(Stmt::Break(self.parse_break_stmt())),
            TokenKind::Continue => Some(Stmt::Continue(self.parse_continue_stmt())),
            TokenKind::Rust => self.parse_rust_block_stmt(),
            TokenKind::Debugger => {
                let token = self.advance(); // consume `debugger`
                let span = token.span;
                self.eat(&TokenKind::Semicolon); // optional semicolon
                Some(Stmt::Debugger(span))
            }
            _ => self.parse_expr_stmt(),
        }
    }

    /// Parse a labeled loop statement after consuming `label:`.
    fn parse_labeled_stmt(&mut self, label: String) -> Option<Stmt> {
        match self.peek() {
            TokenKind::While => self.parse_while_stmt(Some(label)).map(Stmt::While),
            TokenKind::Do => self.parse_do_while_stmt(Some(label)).map(Stmt::DoWhile),
            TokenKind::For => self.parse_for_stmt(Some(label)),
            _ => {
                self.diagnostics.push(
                    Diagnostic::error("label must precede a loop statement").with_label(
                        self.current_token().span,
                        self.file_id,
                        "expected `while`, `for`, or `do`",
                    ),
                );
                None
            }
        }
    }

    /// Parse a variable declaration or destructuring:
    /// - `(const | let | var) IDENT (: type)? = expr ;`
    /// - `(const | let | var) { field, ... } = expr ;`
    fn parse_var_decl(&mut self) -> Option<Stmt> {
        let keyword = self.advance();
        let start = keyword.span;
        let binding = match keyword.kind {
            TokenKind::Const => VarBinding::Const,
            TokenKind::Var => VarBinding::Var,
            _ => VarBinding::Let,
        };

        // Check for object destructuring: `const { ... } = expr;`
        if self.check(&TokenKind::LBrace) {
            return self.parse_destructure(binding, start);
        }

        // Check for array destructuring: `const [ ... ] = expr;`
        if self.check(&TokenKind::LBracket) {
            return self.parse_array_destructure(binding, start);
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

    /// Parse a `using` or `await using` declaration.
    ///
    /// `using name (: type)? = expr ;`
    /// `await using name (: type)? = expr ;`
    ///
    /// Lowers to a normal `let` binding — Rust's RAII handles resource disposal.
    fn parse_using_decl(&mut self, is_await: bool) -> Option<Stmt> {
        let start = self.current_token().span;

        if is_await {
            self.advance(); // consume `await`
        }
        self.advance(); // consume `using` (contextual keyword ident)

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
            init.span
        };

        Some(Stmt::Using(UsingDecl {
            name,
            type_ann,
            init,
            is_await,
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
    fn parse_while_stmt(&mut self, label: Option<String>) -> Option<WhileStmt> {
        let while_token = self.advance(); // consume `while`
        let start = while_token.span;

        self.expect(&TokenKind::LParen)?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen)?;

        let body = self.parse_block()?;
        let body_span = body.span;

        Some(WhileStmt {
            label,
            condition,
            body,
            span: start.merge(body_span),
        })
    }

    /// Parse a do-while statement: `do { body } while ( condition ) ;`.
    fn parse_do_while_stmt(&mut self, label: Option<String>) -> Option<DoWhileStmt> {
        let do_token = self.advance(); // consume `do`
        let start = do_token.span;

        let body = self.parse_block()?;

        if !matches!(self.peek(), TokenKind::While) {
            self.diagnostics.push(
                Diagnostic::error("expected `while` after `do` block").with_label(
                    self.current_token().span,
                    self.file_id,
                    "expected `while`",
                ),
            );
            return None;
        }
        self.advance(); // consume `while`

        self.expect(&TokenKind::LParen)?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen)?;

        // Consume optional trailing semicolon
        if matches!(self.peek(), TokenKind::Semicolon) {
            let semi = self.advance();
            return Some(DoWhileStmt {
                label,
                body,
                condition,
                span: start.merge(semi.span),
            });
        }

        let end = self.previous_span();
        Some(DoWhileStmt {
            label,
            body,
            condition,
            span: start.merge(end),
        })
    }

    /// Parse a for loop statement, dispatching to `for...of`, `for...in`, or classic.
    ///
    /// `for (const/let IDENT of EXPR) BLOCK` → `Stmt::For`
    /// `for (const/let IDENT in EXPR) BLOCK` → `Stmt::ForIn`
    /// `for (init; cond; update) BLOCK` → `Stmt::ForClassic`
    fn parse_for_stmt(&mut self, label: Option<String>) -> Option<Stmt> {
        let for_token = self.advance(); // consume `for`
        let start = for_token.span;

        // Check for `for await (...)` — async iteration syntax (only valid with `of`)
        let is_await = self.current_token().kind == TokenKind::Await;
        if is_await {
            self.advance(); // consume `await`
        }

        self.expect(&TokenKind::LParen)?;

        // Check for `for (;;)` — empty init means classic for
        if self.check(&TokenKind::Semicolon) {
            // Classic for loop with empty init: `for (; cond; update)`
            return self.parse_for_classic_rest(label, start, None);
        }

        // Check for binding keyword (const/let/var) — could be for-of/for-in or classic
        let binding_token = self.current_token().clone();
        let has_binding = matches!(
            &binding_token.kind,
            TokenKind::Const | TokenKind::Let | TokenKind::Var
        );

        if has_binding {
            let binding = match &binding_token.kind {
                TokenKind::Const => {
                    self.advance();
                    VarBinding::Const
                }
                TokenKind::Let => {
                    self.advance();
                    VarBinding::Let
                }
                TokenKind::Var => {
                    self.advance();
                    VarBinding::Var
                }
                _ => unreachable!(),
            };

            let Some(variable) = self.parse_ident() else {
                self.synchronize();
                return None;
            };

            // Check what follows the variable: `of`, `in`, or `=` / `:` / `;`
            let is_for_in = self.check(&TokenKind::In);
            let is_for_of = self.check_contextual_keyword("of");

            if is_for_in || is_for_of {
                // for-of or for-in
                self.advance(); // consume `of` or `in`

                let Some(iterable) = self.parse_expr() else {
                    self.synchronize();
                    return None;
                };

                self.expect(&TokenKind::RParen)?;

                let body = self.parse_block()?;
                let body_span = body.span;

                if is_for_in {
                    return Some(Stmt::ForIn(ForInStmt {
                        label,
                        binding,
                        variable,
                        iterable,
                        body,
                        span: start.merge(body_span),
                    }));
                }
                return Some(Stmt::For(ForOfStmt {
                    label,
                    binding,
                    variable,
                    iterable,
                    body,
                    is_await,
                    span: start.merge(body_span),
                }));
            }

            // Classic for loop with variable declaration: `for (let i = 0; ...)`
            // Parse optional type annotation
            let type_ann = if self.check(&TokenKind::Colon) {
                self.advance();
                Some(self.parse_type_annotation()?)
            } else {
                None
            };

            // Parse initializer `= expr`
            let init_expr = if self.eat(&TokenKind::Eq) {
                self.parse_expr()?
            } else {
                // Default to 0 if no initializer
                Expr {
                    kind: ExprKind::IntLit(0),
                    span: variable.span,
                }
            };

            let init_span = variable.span.merge(init_expr.span);
            let init = Some(ForInit::VarDecl(VarDecl {
                binding,
                name: variable,
                type_ann,
                init: init_expr,
                span: init_span,
            }));

            return self.parse_for_classic_rest(label, start, init);
        }

        // No binding keyword — either an expression init or error for for-of/for-in
        // Try to parse as expression init (e.g., `for (i = 0; ...)`)
        let Some(init_expr) = self.parse_expr() else {
            self.synchronize();
            return None;
        };

        if self.check(&TokenKind::Semicolon) {
            // Classic for loop with expression init
            let init = Some(ForInit::Expr(init_expr));
            return self.parse_for_classic_rest(label, start, init);
        }

        // Not a classic for loop and no binding keyword — error
        let current = self.current_token().clone();
        self.diagnostics.push(
            Diagnostic::error("expected `;`, `of`, or `in` in for loop").with_label(
                current.span,
                self.file_id,
                "expected `;`, `of`, or `in`",
            ),
        );
        None
    }

    /// Parse the rest of a classic for loop after the init has been parsed.
    ///
    /// Expects: `; condition ; update ) body`
    fn parse_for_classic_rest(
        &mut self,
        label: Option<String>,
        start: Span,
        init: Option<ForInit>,
    ) -> Option<Stmt> {
        // Consume the first `;` (after init)
        self.expect(&TokenKind::Semicolon)?;

        // Parse optional condition
        let condition = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };

        // Consume the second `;` (after condition)
        self.expect(&TokenKind::Semicolon)?;

        // Parse optional update
        let update = if self.check(&TokenKind::RParen) {
            None
        } else {
            Some(self.parse_expr()?)
        };

        self.expect(&TokenKind::RParen)?;

        let body = self.parse_block()?;
        let body_span = body.span;

        Some(Stmt::ForClassic(ForClassicStmt {
            init,
            condition,
            update,
            body,
            label,
            span: start.merge(body_span),
        }))
    }

    /// Parse a `break [label];` statement.
    fn parse_break_stmt(&mut self) -> BreakStmt {
        let break_token = self.advance(); // consume `break`
        let start = break_token.span;

        // Check for optional label: `break label;`
        let label = if let TokenKind::Ident(name) = self.peek() {
            let name = name.clone();
            self.advance();
            Some(name)
        } else {
            None
        };

        let end = if let Some(semi) = self.expect(&TokenKind::Semicolon) {
            semi.span
        } else {
            self.previous_span()
        };
        BreakStmt {
            label,
            span: start.merge(end),
        }
    }

    /// Parse a `continue [label];` statement.
    fn parse_continue_stmt(&mut self) -> ContinueStmt {
        let continue_token = self.advance(); // consume `continue`
        let start = continue_token.span;

        // Check for optional label: `continue label;`
        let label = if let TokenKind::Ident(name) = self.peek() {
            let name = name.clone();
            self.advance();
            Some(name)
        } else {
            None
        };

        let end = if let Some(semi) = self.expect(&TokenKind::Semicolon) {
            semi.span
        } else {
            self.previous_span()
        };
        ContinueStmt {
            label,
            span: start.merge(end),
        }
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

    /// Parse a try/catch/finally statement.
    ///
    /// Supports three forms:
    /// - `try { ... } catch (name: Type) { ... }`
    /// - `try { ... } catch (name: Type) { ... } finally { ... }`
    /// - `try { ... } finally { ... }`
    fn parse_try_catch_stmt(&mut self) -> Option<TryCatchStmt> {
        let try_token = self.advance(); // consume `try`
        let start = try_token.span;

        let try_block = self.parse_block()?;

        // Parse optional catch block
        let (catch_binding, catch_type, catch_block) = if self.check(&TokenKind::Catch) {
            self.advance(); // consume `catch`
            self.expect(&TokenKind::LParen)?;
            let binding = self.parse_ident()?;

            // Optional type annotation on catch binding
            let ty = if self.eat(&TokenKind::Colon) {
                Some(self.parse_type_annotation()?)
            } else {
                None
            };

            self.expect(&TokenKind::RParen)?;
            let block = self.parse_block()?;
            (Some(binding), ty, Some(block))
        } else {
            (None, None, None)
        };

        // Parse optional finally block
        let finally_block = if self.eat(&TokenKind::Finally) {
            Some(self.parse_block()?)
        } else {
            None
        };

        // Must have at least catch or finally
        if catch_block.is_none() && finally_block.is_none() {
            let current = self.current_token();
            let span = current.span;
            self.diagnostics.push(
                Diagnostic::error("expected `catch` or `finally` after try block").with_label(
                    span,
                    self.file_id,
                    "unexpected token",
                ),
            );
            return None;
        }

        let end = finally_block
            .as_ref()
            .map(|b| b.span)
            .or(catch_block.as_ref().map(|b| b.span))
            .unwrap_or(try_block.span);

        Some(TryCatchStmt {
            try_block,
            catch_binding,
            catch_type,
            catch_block,
            finally_block,
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

    /// Parse a destructuring statement: `{ field, field: local, field = default, ... } = expr ;`.
    ///
    /// The keyword (`const`/`let`) and binding have already been consumed.
    /// Supports rename (`field: localName`) and defaults (`field = expr`),
    /// as well as combined rename + default (`field: local = expr`).
    fn parse_destructure(&mut self, binding: VarBinding, start: Span) -> Option<Stmt> {
        self.advance(); // consume `{`

        let mut fields = Vec::new();
        if !self.check(&TokenKind::RBrace) && !self.at_end() {
            loop {
                let field_start = self.current_token().span;
                let Some(field_name) = self.parse_ident() else {
                    self.synchronize();
                    return None;
                };

                // Check for rename: `field: localName`
                let local_name = if self.eat(&TokenKind::Colon) {
                    let Some(local) = self.parse_ident() else {
                        self.synchronize();
                        return None;
                    };
                    Some(local)
                } else {
                    None
                };

                // Check for default: `field = expr` or `field: local = expr`
                let default_value = if self.eat(&TokenKind::Eq) {
                    let Some(default_expr) = self.parse_expr() else {
                        self.synchronize();
                        return None;
                    };
                    Some(Box::new(default_expr))
                } else {
                    None
                };

                let field_end = local_name.as_ref().map_or(field_name.span, |l| l.span);
                let field_end = default_value.as_ref().map_or(field_end, |d| d.span);

                fields.push(DestructureField {
                    field_name,
                    local_name,
                    default_value,
                    span: field_start.merge(field_end),
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

    /// Parse an array destructuring statement: `const [a, b, c] = expr;`.
    ///
    /// Supports rest elements: `const [first, ...rest] = arr;`.
    /// Lowers to Rust tuple destructuring or indexed access with rest slice.
    fn parse_array_destructure(&mut self, binding: VarBinding, start: Span) -> Option<Stmt> {
        self.advance(); // consume `[`

        let mut elements = Vec::new();
        if !self.check(&TokenKind::RBracket) && !self.at_end() {
            loop {
                // Check for rest element: `...name`
                if self.eat(&TokenKind::DotDotDot) {
                    let Some(rest_name) = self.parse_ident() else {
                        self.synchronize();
                        return None;
                    };
                    elements.push(ArrayDestructureElement::Rest(rest_name));
                    // Rest must be last element
                    break;
                }

                let Some(elem_name) = self.parse_ident() else {
                    self.synchronize();
                    return None;
                };
                elements.push(ArrayDestructureElement::Single(elem_name));

                if !self.eat(&TokenKind::Comma) {
                    break;
                }

                // Allow trailing comma
                if self.check(&TokenKind::RBracket) {
                    break;
                }
            }
        }

        if self.expect(&TokenKind::RBracket).is_none() {
            self.synchronize();
            return None;
        }

        // Optional type annotation: `const [a, b]: [string, i32] = expr`
        let type_ann = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
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
            init.span
        };

        Some(Stmt::ArrayDestructure(ArrayDestructureStmt {
            binding,
            elements,
            type_ann,
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

    /// Parse assignment: `IDENT = assignment | IDENT op= assignment | ternary`.
    ///
    /// Assignment is right-associative: `a = b = c` parses as `a = (b = c)`.
    /// Compound assignments (`+=`, `-=`, etc.) are desugared to `x = x op rhs`.
    /// Logical assignments (`??=`, `||=`, `&&=`) produce `LogicalAssign` nodes.
    #[allow(clippy::too_many_lines)]
    // Assignment parsing covers simple, compound, logical, and field assignment
    // forms as well as ternary — splitting would fragment the coherent precedence logic.
    fn parse_assignment(&mut self) -> Option<Expr> {
        let expr = self.parse_ternary()?;

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

        // Check for logical assignment operators: ??=, ||=, &&=
        let logical_op = match self.peek() {
            TokenKind::QuestionQuestionEq => Some(LogicalAssignOp::NullishAssign),
            TokenKind::PipePipeEq => Some(LogicalAssignOp::OrAssign),
            TokenKind::AmpAmpEq => Some(LogicalAssignOp::AndAssign),
            _ => None,
        };

        if let Some(op) = logical_op {
            if let ExprKind::Ident(ref ident) = expr.kind {
                let ident = ident.clone();
                self.advance(); // consume logical assignment operator
                let rhs = self.parse_assignment()?;
                let span = ident.span.merge(rhs.span);
                return Some(Expr {
                    kind: ExprKind::LogicalAssign(LogicalAssignExpr {
                        target: ident,
                        op,
                        value: Box::new(rhs),
                    }),
                    span,
                });
            }
            // Logical assignment to non-identifier — emit diagnostic
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
            // Field assignment: `obj.field = value` (e.g., `this.count = 0`)
            if let ExprKind::FieldAccess(fa) = expr.kind {
                self.advance(); // consume `=`
                let value = self.parse_assignment()?;
                let span = fa.object.span.merge(value.span);
                return Some(Expr {
                    kind: ExprKind::FieldAssign(FieldAssignExpr {
                        object: fa.object,
                        field: fa.field,
                        value: Box::new(value),
                    }),
                    span,
                });
            }
            // Index assignment: `obj["key"] = value` (e.g., `config["debug"] = "true"`)
            if let ExprKind::Index(idx) = expr.kind {
                self.advance(); // consume `=`
                let value = self.parse_assignment()?;
                let span = idx.object.span.merge(value.span);
                return Some(Expr {
                    kind: ExprKind::IndexAssign(IndexAssignExpr {
                        object: idx.object,
                        index: idx.index,
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

    /// Parse ternary: `nullish_coalesce ( "?" assignment ":" assignment )?`.
    ///
    /// The ternary operator has lower precedence than nullish coalescing.
    /// Right-associative: `a ? b : c ? d : e` parses as `a ? b : (c ? d : e)`.
    fn parse_ternary(&mut self) -> Option<Expr> {
        let condition = self.parse_nullish_coalesce()?;

        if self.check(&TokenKind::Question) {
            self.advance(); // consume `?`
            let consequent = self.parse_assignment()?;
            self.expect(&TokenKind::Colon)?;
            let alternate = self.parse_assignment()?;
            let span = condition.span.merge(alternate.span);
            return Some(Expr {
                kind: ExprKind::Ternary(
                    Box::new(condition),
                    Box::new(consequent),
                    Box::new(alternate),
                ),
                span,
            });
        }

        Some(condition)
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

    /// Parse logical AND: `bitwise_or ( "&&" bitwise_or )*`.
    fn parse_logic_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_bitwise_or()?;

        while self.check(&TokenKind::AmpAmp) {
            self.advance();
            let right = self.parse_bitwise_or()?;
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

    /// Parse bitwise OR: `bitwise_xor ( "|" bitwise_xor )*`.
    ///
    /// The `|` token is `Pipe` — but only when it's not part of `||` (which is lexed as
    /// `PipePipe`). We need to be careful not to consume `|` when it's used for union types
    /// in other contexts, but in expression position `|` is always bitwise OR.
    fn parse_bitwise_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_bitwise_xor()?;

        while self.check(&TokenKind::Pipe) {
            self.advance();
            let right = self.parse_bitwise_xor()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::BitOr,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
    }

    /// Parse bitwise XOR: `bitwise_and ( "^" bitwise_and )*`.
    fn parse_bitwise_xor(&mut self) -> Option<Expr> {
        let mut left = self.parse_bitwise_and()?;

        while self.check(&TokenKind::Caret) {
            self.advance();
            let right = self.parse_bitwise_and()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::BitXor,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                span,
            };
        }

        Some(left)
    }

    /// Parse bitwise AND: `equality ( "&" equality )*`.
    ///
    /// Uses `Ampersand` token. `&&` is already lexed as `AmpAmp` for logical AND.
    fn parse_bitwise_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_equality()?;

        while self.check(&TokenKind::Ampersand) {
            self.advance();
            let right = self.parse_equality()?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::BitAnd,
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

    /// Parse comparison: `shift ( ("<" | ">" | "<=" | ">=" | "in") shift )*`.
    fn parse_comparison(&mut self) -> Option<Expr> {
        let mut left = self.parse_shift()?;

        loop {
            let op = match self.peek() {
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::LtEq => BinaryOp::Le,
                TokenKind::GtEq => BinaryOp::Ge,
                TokenKind::In => BinaryOp::In,
                _ => break,
            };
            self.advance();
            let right = self.parse_shift()?;
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

    /// Parse shift: `addition ( ("<<" | ">>") addition )*`.
    ///
    /// `<<` and `>>` are not lexed as single tokens (to avoid conflicts with
    /// generic type syntax like `Array<Array<i32>>`). Instead, we detect two
    /// consecutive `<` or `>` tokens and consume them as shift operators.
    fn parse_shift(&mut self) -> Option<Expr> {
        let mut left = self.parse_addition()?;

        loop {
            let op = if self.check(&TokenKind::Lt)
                && self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Lt)
            {
                Some(BinaryOp::Shl)
            } else if self.check(&TokenKind::Gt)
                && self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Gt)
            {
                Some(BinaryOp::Shr)
            } else {
                None
            };

            let Some(op) = op else { break };
            self.advance(); // consume first `<` or `>`
            self.advance(); // consume second `<` or `>`
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

    /// Parse multiplication: `exponentiation ( ("*" | "/" | "%") exponentiation )*`.
    fn parse_multiplication(&mut self) -> Option<Expr> {
        let mut left = self.parse_exponentiation()?;

        loop {
            let op = match self.peek() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_exponentiation()?;
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

    /// Parse exponentiation: `unary ( "**" exponentiation )?`.
    ///
    /// Right-associative: `a ** b ** c` parses as `a ** (b ** c)`.
    fn parse_exponentiation(&mut self) -> Option<Expr> {
        let base = self.parse_unary()?;

        if self.check(&TokenKind::StarStar) {
            self.advance();
            let exp = self.parse_exponentiation()?;
            let span = base.span.merge(exp.span);
            return Some(Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Pow,
                    left: Box::new(base),
                    right: Box::new(exp),
                }),
                span,
            });
        }

        Some(base)
    }

    /// Parse unary: `("-" | "!" | "~" | "typeof") unary | "throw" expr | "await" unary | postfix`.
    #[allow(clippy::too_many_lines)]
    // Unary parsing covers negation, not, bitwise not, typeof, throw, await, yield,
    // delete, void, and prefix increment/decrement — splitting would fragment precedence logic.
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
            TokenKind::Tilde => {
                let op_token = self.advance();
                let operand = self.parse_unary()?;
                let span = op_token.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::Unary(UnaryExpr {
                        op: UnaryOp::BitNot,
                        operand: Box::new(operand),
                    }),
                    span,
                })
            }
            TokenKind::TypeOf => {
                let typeof_token = self.advance();
                let operand = self.parse_unary()?;
                let span = typeof_token.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::TypeOf(Box::new(operand)),
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
            TokenKind::Await => {
                let await_token = self.advance();
                let value = self.parse_unary()?;
                let span = await_token.span.merge(value.span);
                Some(Expr {
                    kind: ExprKind::Await(Box::new(value)),
                    span,
                })
            }
            TokenKind::Yield => {
                let yield_token = self.advance();
                let value = self.parse_unary()?;
                let span = yield_token.span.merge(value.span);
                Some(Expr {
                    kind: ExprKind::Yield(Box::new(value)),
                    span,
                })
            }
            TokenKind::Delete => {
                let delete_token = self.advance();
                let operand = self.parse_unary()?;
                let span = delete_token.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::Delete(Box::new(operand)),
                    span,
                })
            }
            TokenKind::Void => {
                let void_token = self.advance();
                let operand = self.parse_unary()?;
                let span = void_token.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::Void(Box::new(operand)),
                    span,
                })
            }
            TokenKind::PlusPlus => {
                let op_token = self.advance();
                let operand = self.parse_unary()?;
                let span = op_token.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::PrefixIncrement(Box::new(operand)),
                    span,
                })
            }
            TokenKind::MinusMinus => {
                let op_token = self.advance();
                let operand = self.parse_unary()?;
                let span = op_token.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::PrefixDecrement(Box::new(operand)),
                    span,
                })
            }
            TokenKind::Lt => {
                // Angle bracket type cast: `<Type>expr`
                // In expression-start position, `<` is always a type cast
                // (less-than is handled as a binary op in parse_comparison).
                let open = self.advance(); // consume `<`
                let ty = self.parse_type_annotation()?;
                self.expect(&TokenKind::Gt)?;
                let operand = self.parse_unary()?;
                let span = open.span.merge(operand.span);
                Some(Expr {
                    kind: ExprKind::Cast(Box::new(operand), ty),
                    span,
                })
            }
            _ => self.parse_postfix(),
        }
    }

    /// Parse postfix operators: `as Type` and `!` (non-null assertion).
    ///
    /// These are applied after call/member-access parsing:
    /// `expr.method()!` or `expr as f64`.
    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut expr = self.parse_call()?;

        loop {
            if self.check(&TokenKind::As) {
                self.advance(); // consume `as`
                // Check for `as const` — special case before type annotation
                if self.check(&TokenKind::Const) {
                    let const_token = self.advance();
                    let span = expr.span.merge(const_token.span);
                    expr = Expr {
                        kind: ExprKind::AsConst(Box::new(expr)),
                        span,
                    };
                } else {
                    let ty = self.parse_type_annotation()?;
                    let span = expr.span.merge(ty.span);
                    expr = Expr {
                        kind: ExprKind::Cast(Box::new(expr), ty),
                        span,
                    };
                }
            } else if self.check(&TokenKind::Satisfies) {
                self.advance(); // consume `satisfies`
                let ty = self.parse_type_annotation()?;
                let span = expr.span.merge(ty.span);
                expr = Expr {
                    kind: ExprKind::Satisfies(Box::new(expr), ty),
                    span,
                };
            } else if self.check(&TokenKind::Bang) {
                // Postfix `!` — non-null assertion.
                // Must not be followed by `=` (that would be `!=` or `!==`).
                // The lexer already handles `!=` and `!==` as separate tokens,
                // so if we see a bare `Bang` here, it's always postfix `!`.
                let bang = self.advance();
                let span = expr.span.merge(bang.span);
                expr = Expr {
                    kind: ExprKind::NonNullAssert(Box::new(expr)),
                    span,
                };
            } else if self.check(&TokenKind::PlusPlus) {
                let pp = self.advance();
                let span = expr.span.merge(pp.span);
                expr = Expr {
                    kind: ExprKind::PostfixIncrement(Box::new(expr)),
                    span,
                };
            } else if self.check(&TokenKind::MinusMinus) {
                let mm = self.advance();
                let span = expr.span.merge(mm.span);
                expr = Expr {
                    kind: ExprKind::PostfixDecrement(Box::new(expr)),
                    span,
                };
            } else {
                break;
            }
        }

        Some(expr)
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
                } else if matches!(expr.kind, ExprKind::Super) {
                    // `super(args)` — constructor delegation call.
                    // Parsed as MethodCall(Super, "new", args) and lowered to
                    // Base::new(args) during class lowering.
                    self.advance(); // consume `(`
                    let args = self.parse_arg_list();
                    let close = self.expect(&TokenKind::RParen)?;
                    let span = expr.span.merge(close.span);
                    expr = Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(expr),
                            method: Ident {
                                name: "new".to_owned(),
                                span: close.span,
                            },
                            args,
                        }),
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
            } else if matches!(
                self.current_token().kind,
                TokenKind::TemplateNoSub(_) | TokenKind::TemplateHead(_)
            ) {
                // Tagged template literal: `expr\`text ${v} text\``
                // The previously parsed expression is the tag function.
                expr = self.parse_tagged_template(expr)?;
            } else {
                break;
            }
        }

        Some(expr)
    }

    /// Parse a comma-separated argument list (without the surrounding parens).
    ///
    /// Handles spread arguments: `...expr` is parsed as `SpreadArg(expr)`.
    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();

        if self.check(&TokenKind::RParen) || self.at_end() {
            return args;
        }

        loop {
            // Check for spread argument: `...expr`
            if self.check(&TokenKind::DotDotDot) {
                let start = self.current_token().span;
                self.advance(); // consume `...`
                if let Some(inner) = self.parse_expr() {
                    let span = start.merge(inner.span);
                    args.push(Expr {
                        kind: ExprKind::SpreadArg(Box::new(inner)),
                        span,
                    });
                }
            } else if let Some(arg) = self.parse_expr() {
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

        // `{ }` — empty struct/object literal
        if matches!(after_brace, Some(TokenKind::RBrace)) {
            return true;
        }

        // `{ ident: expr, ... }` — standard struct literal
        if matches!(
            (after_brace, after_ident),
            (Some(TokenKind::Ident(_)), Some(TokenKind::Colon))
        ) {
            return true;
        }

        // `{ ...expr }` — object spread literal
        if matches!(after_brace, Some(TokenKind::DotDotDot)) {
            return true;
        }

        // `{ [expr]: value, ... }` — computed property name
        matches!(after_brace, Some(TokenKind::LBracket))
    }

    /// Parse a struct literal: `{ name: expr, ... }` or `{ [expr]: value, ... }`.
    ///
    /// The `type_name` is provided when the struct type is known from context
    /// (e.g., from a type annotation on the variable declaration).
    /// Supports computed property names: `{ [key_expr]: value }`.
    #[allow(clippy::too_many_lines)]
    fn parse_struct_literal(&mut self, type_name: Option<Ident>) -> Option<Expr> {
        let open = self.advance(); // consume `{`
        let start = type_name.as_ref().map_or(open.span, |n| n.span);

        let mut spread = None;
        let mut fields = Vec::new();

        if !self.check(&TokenKind::RBrace) && !self.at_end() {
            // Check for spread base: `{ ...expr, field: value }`
            if self.check(&TokenKind::DotDotDot) {
                self.advance(); // consume `...`
                let spread_expr = self.parse_expr()?;
                spread = Some(Box::new(spread_expr));

                // Optionally consume comma and continue to field overrides
                if self.eat(&TokenKind::Comma) && self.check(&TokenKind::RBrace) {
                    // Trailing comma after spread with no fields — fall through
                } else if spread.is_some() && !self.check(&TokenKind::RBrace) {
                    // Parse field overrides after spread
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
                            computed_key: None,
                            span: field_span,
                        });

                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }

                        if self.check(&TokenKind::RBrace) {
                            break;
                        }
                    }
                }
            } else {
                // No spread — parse fields (static or computed)
                loop {
                    let field_start = self.current_token().span;

                    // Computed property: `[expr]: value`
                    if self.check(&TokenKind::LBracket) {
                        self.advance(); // consume `[`
                        let Some(key_expr) = self.parse_expr() else {
                            break;
                        };
                        if self.expect(&TokenKind::RBracket).is_none() {
                            break;
                        }
                        if self.expect(&TokenKind::Colon).is_none() {
                            break;
                        }
                        let Some(value) = self.parse_expr() else {
                            break;
                        };
                        let field_span = field_start.merge(value.span);
                        let placeholder = Ident {
                            name: "__computed".to_owned(),
                            span: key_expr.span,
                        };
                        fields.push(FieldInit {
                            name: placeholder,
                            value,
                            computed_key: Some(Box::new(key_expr)),
                            span: field_span,
                        });
                    } else {
                        // Static property: `name: value`
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
                            computed_key: None,
                            span: field_span,
                        });
                    }

                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }

                    // Allow trailing comma
                    if self.check(&TokenKind::RBrace) {
                        break;
                    }
                }
            }
        }

        let close = self.expect(&TokenKind::RBrace)?;
        let span = start.merge(close.span);

        Some(Expr {
            kind: ExprKind::StructLit(StructLitExpr {
                type_name,
                spread,
                fields,
            }),
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
        // If we see `move` or `async`, it's always an arrow function
        if self.check(&TokenKind::Move) || self.check(&TokenKind::Async) {
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
        // Skip the base type name (identifiers and keyword types like `void`)
        match self.tokens.get(i).map(|t| &t.kind) {
            Some(TokenKind::Ident(_) | TokenKind::Void) => i += 1,
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
    /// - `async (params) => expr`
    /// - `async move (params) => expr`
    fn parse_arrow_function(&mut self) -> Option<Expr> {
        let start = self.current_token().span;

        // Optional `async` keyword
        let is_async = self.eat(&TokenKind::Async);

        // Optional `move` keyword
        let is_move = self.eat(&TokenKind::Move);

        // Parameter list — closure params allow omitted type annotations
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_closure_param_list();
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
                is_async,
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
                // Check for `shared(expr)` constructor
                if matches!(&token.kind, TokenKind::Ident(name) if name == "shared")
                    && self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::LParen)
                {
                    let start = self.current_token().span;
                    self.advance(); // consume `shared`
                    self.advance(); // consume `(`
                    let inner = self.parse_expr()?;
                    let close = self.expect(&TokenKind::RParen)?;
                    let span = start.merge(close.span);
                    return Some(Expr {
                        kind: ExprKind::Shared(Box::new(inner)),
                        span,
                    });
                }
                // Check for single-param arrow function shorthand: `n => expr`
                if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::FatArrow) {
                    let start = self.current_token().span;
                    let name = self.parse_ident()?;
                    let param_span = name.span;
                    self.expect(&TokenKind::FatArrow)?;
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
                    return Some(Expr {
                        kind: ExprKind::Closure(ClosureExpr {
                            is_async: false,
                            is_move: false,
                            params: vec![Param {
                                name,
                                type_ann: TypeAnnotation {
                                    kind: TypeKind::Inferred,
                                    span: param_span,
                                },
                                optional: false,
                                default_value: None,
                                is_rest: false,
                                span: param_span,
                            }],
                            return_type: None,
                            body,
                        }),
                        span: start.merge(end),
                    });
                }
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
                    Diagnostic::error("unexpected `{` in expression position; for an object literal, use `TypeName { field: value }` syntax").with_label(
                        token.span,
                        self.file_id,
                        "expected expression",
                    ),
                );
                None
            }
            TokenKind::LBracket => self.parse_array_literal(),
            TokenKind::This => {
                let this_token = self.advance();
                Some(Expr {
                    kind: ExprKind::This,
                    span: this_token.span,
                })
            }
            TokenKind::Super => {
                let super_token = self.advance();
                Some(Expr {
                    kind: ExprKind::Super,
                    span: super_token.span,
                })
            }
            TokenKind::Import => {
                // `import.meta` meta-property
                if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Dot) {
                    if let Some(next) = self.tokens.get(self.pos + 2) {
                        if let TokenKind::Ident(name) = &next.kind {
                            if name == "meta" {
                                let start = self.advance(); // consume `import`
                                self.advance(); // consume `.`
                                let meta_token = self.advance(); // consume `meta`
                                let span = start.span.merge(meta_token.span);
                                return Some(Expr {
                                    kind: ExprKind::ImportMeta,
                                    span,
                                });
                            }
                        }
                    }
                }
                // Dynamic import expression: `import("module")`
                if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::LParen) {
                    let start = self.advance(); // consume `import`
                    self.advance(); // consume `(`
                    let arg_token = self.current_token().clone();
                    let module_path = match &arg_token.kind {
                        TokenKind::StringLit(s) => s.clone(),
                        _ => {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "dynamic import requires a string literal argument",
                                )
                                .with_label(
                                    arg_token.span,
                                    self.file_id,
                                    "expected string literal",
                                ),
                            );
                            return None;
                        }
                    };
                    self.advance(); // consume string literal
                    let close = self.expect(&TokenKind::RParen)?;
                    let span = start.span.merge(close.span);
                    Some(Expr {
                        kind: ExprKind::DynamicImport(module_path),
                        span,
                    })
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "unexpected `import` in expression position; use `import(\"...\")` for dynamic import",
                        )
                        .with_label(
                            token.span,
                            self.file_id,
                            "expected `(` for dynamic import",
                        ),
                    );
                    None
                }
            }
            TokenKind::Class => self.parse_class_expr(),
            TokenKind::Slash => self.parse_regex_literal(),
            TokenKind::New => self.parse_new_expr(),
            TokenKind::TemplateNoSub(_) => Some(self.parse_template_no_sub()),
            TokenKind::TemplateHead(_) => self.parse_template_literal(),
            TokenKind::Move => {
                // `move` keyword in expression position — must be a move closure
                self.parse_arrow_function()
            }
            TokenKind::Async => {
                // `async` keyword in expression position — must be an async closure
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
                    let first = self.parse_expr()?;

                    // Check for comma operator: `(a, b, c)`
                    if self.check(&TokenKind::Comma) {
                        let mut exprs = vec![first];
                        while self.eat(&TokenKind::Comma) {
                            let next = self.parse_expr()?;
                            exprs.push(next);
                        }
                        let close = self.expect(&TokenKind::RParen)?;
                        let span = open.span.merge(close.span);
                        Some(Expr {
                            kind: ExprKind::Comma(exprs),
                            span,
                        })
                    } else {
                        let close = self.expect(&TokenKind::RParen)?;
                        let span = open.span.merge(close.span);
                        Some(Expr {
                            kind: ExprKind::Paren(Box::new(first)),
                            span,
                        })
                    }
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
    // Regex literals
    // ---------------------------------------------------------------

    /// Parse a regex literal: `/pattern/flags`.
    ///
    /// Called from `parse_primary` when a `Slash` token appears in expression-start
    /// position — where a division operator is not expected. Rescans the source
    /// text from the opening `/` to extract the pattern and flags.
    fn parse_regex_literal(&mut self) -> Option<Expr> {
        let slash_token = self.advance(); // consume the `Slash` token
        let start = slash_token.span;
        let src_start = start.start.0 as usize;

        // Scan the source from just after the opening `/` for the pattern.
        let source_bytes = self.source.as_bytes();
        let mut i = src_start + 1; // skip the opening `/`
        let mut pattern = String::new();

        // Scan for the closing `/`, handling escape sequences.
        loop {
            if i >= source_bytes.len() {
                self.diagnostics
                    .push(Diagnostic::error("unterminated regex literal").with_label(
                        start,
                        self.file_id,
                        "regex starts here",
                    ));
                return None;
            }
            let byte = source_bytes[i];
            if byte == b'/' {
                // Found the closing delimiter.
                break;
            }
            if byte == b'\\' {
                // Escape sequence: include both the backslash and the next char.
                pattern.push('\\');
                i += 1;
                if i < source_bytes.len() {
                    pattern.push(source_bytes[i] as char);
                    i += 1;
                }
                continue;
            }
            if byte == b'\n' || byte == b'\r' {
                self.diagnostics.push(
                    Diagnostic::error("unterminated regex literal — newline in pattern")
                        .with_label(start, self.file_id, "regex starts here"),
                );
                return None;
            }
            pattern.push(byte as char);
            i += 1;
        }

        // `i` now points at the closing `/`. Skip it.
        i += 1;

        // Scan flags (identifier-like characters after the closing `/`).
        let mut flags = String::new();
        while i < source_bytes.len() && source_bytes[i].is_ascii_alphabetic() {
            flags.push(source_bytes[i] as char);
            i += 1;
        }

        #[allow(clippy::cast_possible_truncation)]
        let end = i as u32;
        let span = Span::new(start.start.0, end);

        // Record this regex literal span so lexer diagnostics within it can be filtered.
        self.regex_literal_spans.push(span);

        // Skip past any tokens the lexer produced that fall within our regex literal span.
        // The lexer would have tokenized the pattern content as various tokens.
        while self.pos < self.tokens.len() {
            let tok = &self.tokens[self.pos];
            if tok.span.start.0 >= end || matches!(tok.kind, TokenKind::Eof) {
                break;
            }
            self.pos += 1;
        }

        Some(Expr {
            kind: ExprKind::RegexLit { pattern, flags },
            span,
        })
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
                // Check for spread element: `...expr`
                if self.check(&TokenKind::DotDotDot) {
                    let spread_start = self.current_token().span;
                    self.advance(); // consume `...`
                    let inner = self.parse_expr()?;
                    let span = spread_start.merge(inner.span);
                    elements.push(ArrayElement::Spread(Expr {
                        kind: inner.kind,
                        span,
                    }));
                } else {
                    let elem = self.parse_expr()?;
                    elements.push(ArrayElement::Expr(elem));
                }

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

        // `new.target` meta-property
        if self.check(&TokenKind::Dot) {
            if let Some(next) = self.tokens.get(self.pos + 1) {
                if let TokenKind::Ident(name) = &next.kind {
                    if name == "target" {
                        self.advance(); // consume `.`
                        let target_token = self.advance(); // consume `target`
                        let span = start.merge(target_token.span);
                        return Some(Expr {
                            kind: ExprKind::NewTarget,
                            span,
                        });
                    }
                }
            }
        }

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

    // ---------------------------------------------------------------
    // Tagged template literals
    // ---------------------------------------------------------------

    /// Parse a tagged template literal: `` tag`text ${expr} more` ``.
    ///
    /// The `tag` expression has already been parsed. This method consumes the
    /// template literal tokens and produces a `TaggedTemplate` node containing
    /// the tag expression, the static string segments (quasis), and the
    /// interpolated expressions.
    fn parse_tagged_template(&mut self, tag: Expr) -> Option<Expr> {
        let start_span = tag.span;

        match &self.current_token().kind {
            TokenKind::TemplateNoSub(_) => {
                // No interpolations: `` tag`plain text` ``
                let token = self.advance();
                let TokenKind::TemplateNoSub(text) = token.kind else {
                    unreachable!("parse_tagged_template called without TemplateNoSub");
                };
                let span = start_span.merge(token.span);
                Some(Expr {
                    kind: ExprKind::TaggedTemplate {
                        tag: Box::new(tag),
                        quasis: vec![text],
                        expressions: vec![],
                    },
                    span,
                })
            }
            TokenKind::TemplateHead(_) => {
                // Has interpolations: `` tag`text ${expr} more` ``
                let head_token = self.advance();
                let TokenKind::TemplateHead(head_text) = head_token.kind else {
                    unreachable!("parse_tagged_template called without TemplateHead");
                };

                let mut quasis = vec![head_text];
                let mut expressions = Vec::new();

                loop {
                    let inner_expr = self.parse_expr()?;
                    expressions.push(inner_expr);

                    let next = self.current_token().clone();
                    match &next.kind {
                        TokenKind::TemplateTail(_) => {
                            let tail_token = self.advance();
                            let TokenKind::TemplateTail(tail_text) = tail_token.kind else {
                                unreachable!();
                            };
                            quasis.push(tail_text);
                            let span = start_span.merge(tail_token.span);
                            return Some(Expr {
                                kind: ExprKind::TaggedTemplate {
                                    tag: Box::new(tag),
                                    quasis,
                                    expressions,
                                },
                                span,
                            });
                        }
                        TokenKind::TemplateMiddle(_) => {
                            let mid_token = self.advance();
                            let TokenKind::TemplateMiddle(mid_text) = mid_token.kind else {
                                unreachable!();
                            };
                            quasis.push(mid_text);
                        }
                        _ => {
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
            _ => {
                unreachable!("parse_tagged_template called without template token");
            }
        }
    }

    // ---------------------------------------------------------------
    // Inline Rust blocks
    // ---------------------------------------------------------------

    /// Parse a `rust { ... }` block as a top-level item.
    ///
    /// The `rust` keyword has already been peeked. Consumes `rust {`,
    /// captures the raw contents with brace balancing, and returns an item.
    fn parse_rust_block_item(&mut self) -> Option<Item> {
        let rust_block = self.parse_inline_rust_block()?;
        let span = rust_block.span;
        Some(Item {
            kind: ItemKind::RustBlock(rust_block),
            exported: false,
            decorators: vec![],
            span,
        })
    }

    /// Parse a top-level `const`, `let`, or `var` declaration as a module-level item.
    ///
    /// Syntax: `const name: Type = expr;` or `let name: Type = expr;` or `var name: Type = expr;`
    /// Produces an `ItemKind::Const(VarDecl)`.
    fn parse_top_level_const(&mut self) -> Option<Item> {
        let keyword = self.advance();
        let start = keyword.span;
        let binding = match keyword.kind {
            TokenKind::Const => VarBinding::Const,
            TokenKind::Var => VarBinding::Var,
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
            init.span
        };

        let span = start.merge(end);
        Some(Item {
            kind: ItemKind::Const(VarDecl {
                binding,
                name,
                type_ann,
                init,
                span,
            }),
            exported: false,
            decorators: vec![],
            span,
        })
    }

    /// Parse a `test("...", () => { ... })`, `describe("...", () => { ... })`, or
    /// `it("...", () => { ... })` block at the top level.
    ///
    /// The identifier (`test`, `describe`, or `it`) has been peeked but not consumed.
    fn parse_test_block(&mut self) -> Option<Item> {
        let ident_token = self.advance();
        let start = ident_token.span;
        let kind = match &ident_token.kind {
            TokenKind::Ident(name) => match name.as_str() {
                "test" => TestBlockKind::Test,
                "describe" => TestBlockKind::Describe,
                "it" => TestBlockKind::It,
                _ => return None,
            },
            _ => return None,
        };

        // Expect `(`
        if self.expect(&TokenKind::LParen).is_none() {
            self.synchronize();
            return None;
        }

        // Parse the description string
        let desc_token = self.advance();
        let description = if let TokenKind::StringLit(value) = &desc_token.kind {
            value.clone()
        } else {
            self.diagnostics.push(
                Diagnostic::error("expected string literal for test description").with_label(
                    desc_token.span,
                    self.file_id,
                    "expected string literal",
                ),
            );
            self.synchronize();
            return None;
        };

        // Expect `,`
        if self.expect(&TokenKind::Comma).is_none() {
            self.synchronize();
            return None;
        }

        // Expect `() => {` or `() => { ... }`
        // Parse the arrow function: () => { body }
        if self.expect(&TokenKind::LParen).is_none() {
            self.synchronize();
            return None;
        }
        if self.expect(&TokenKind::RParen).is_none() {
            self.synchronize();
            return None;
        }
        if self.expect(&TokenKind::FatArrow).is_none() {
            self.synchronize();
            return None;
        }

        // Parse the body based on kind
        let body = if kind == TestBlockKind::Describe {
            // Describe blocks contain nested test blocks (it/test/describe)
            self.parse_describe_body()?
        } else {
            // test/it blocks contain statements
            let block = self.parse_block()?;
            TestBody::Stmts(block)
        };

        // Expect `)`
        if self.expect(&TokenKind::RParen).is_none() {
            self.synchronize();
            return None;
        }

        // Optional semicolon
        self.eat(&TokenKind::Semicolon);
        let end = self.previous_span();
        let span = start.merge(end);

        Some(Item {
            kind: ItemKind::TestBlock(TestBlock {
                kind,
                description,
                body,
                span,
            }),
            exported: false,
            decorators: vec![],
            span,
        })
    }

    /// Parse the body of a `describe` block — a brace-enclosed list of nested test items.
    ///
    /// Each item inside is either `it(...)`, `test(...)`, or nested `describe(...)`.
    fn parse_describe_body(&mut self) -> Option<TestBody> {
        let _open = self.expect(&TokenKind::LBrace)?;
        let mut items = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            if let TokenKind::Ident(name) = self.peek() {
                let name_str = name.clone();
                match name_str.as_str() {
                    "test" | "describe" | "it" => {
                        if let Some(Item {
                            kind: ItemKind::TestBlock(tb),
                            ..
                        }) = self.parse_test_block()
                        {
                            items.push(tb);
                        }
                    }
                    _ => {
                        // Skip unrecognized tokens inside describe
                        self.advance();
                    }
                }
            } else {
                // Skip non-identifier tokens
                self.advance();
            }
        }

        self.expect(&TokenKind::RBrace)?;
        Some(TestBody::Items(items))
    }

    /// Parse a `rust { ... }` block as a statement.
    ///
    /// The `rust` keyword has already been peeked. Consumes `rust {`,
    /// captures the raw contents with brace balancing, and returns a statement.
    fn parse_rust_block_stmt(&mut self) -> Option<Stmt> {
        let rust_block = self.parse_inline_rust_block()?;
        Some(Stmt::RustBlock(rust_block))
    }

    /// Parse the shared `rust { ... }` block, returning an [`InlineRustBlock`].
    ///
    /// Expects the current token to be `rust`. Consumes `rust`, expects `{`,
    /// then scans with brace-depth tracking until depth returns to 0.
    /// The raw text between the outer `{ }` is captured as-is from the source.
    ///
    /// Limitation: brace balancing is simplified — braces inside Rust string
    /// literals or comments within the block are counted. If a user has
    /// unbalanced braces inside a string literal, they will get a parse error.
    fn parse_inline_rust_block(&mut self) -> Option<InlineRustBlock> {
        let rust_token = self.advance(); // consume `rust`
        let start = rust_token.span;

        if !self.check(&TokenKind::LBrace) {
            self.diagnostics
                .push(Diagnostic::error("expected `{` after `rust`").with_label(
                    self.current_token().span,
                    self.file_id,
                    "expected `{`",
                ));
            self.synchronize();
            return None;
        }

        let open_token = self.advance(); // consume `{`
        let content_start = open_token.span.end.0 as usize;

        // Track brace depth: we just consumed the opening `{`, so depth starts at 1.
        // Scan tokens and count braces to find the matching `}`.
        let mut depth: usize = 1;

        while depth > 0 && !self.at_end() {
            match self.peek() {
                TokenKind::LBrace => {
                    depth += 1;
                    self.advance();
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    self.advance();
                }
                TokenKind::Eof => {
                    break;
                }
                _ => {
                    self.advance();
                }
            }
        }

        if depth > 0 {
            self.diagnostics
                .push(Diagnostic::error("unclosed `rust` block").with_label(
                    start,
                    self.file_id,
                    "`rust` block starts here",
                ));
            return None;
        }

        // The closing `}` is at current position — extract the source text between
        // the opening `{` and closing `}`.
        let content_end = self.current_token().span.start.0 as usize;
        let close_token = self.advance(); // consume closing `}`

        // Extract the raw text from the original source using byte positions.
        let code = self.source[content_start..content_end].to_owned();
        let block_span = start.merge(close_token.span);

        Some(InlineRustBlock {
            code,
            span: block_span,
        })
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
            other => panic!("expected Named, got {other:?}"),
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
    // 5b. var declaration
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_var_declaration() {
        let module = parse_ok("function f() { var x = 1; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(v) => {
                assert_eq!(v.binding, VarBinding::Var);
                assert_eq!(v.name.name, "x");
                assert!(v.type_ann.is_none());
                match &v.init.kind {
                    ExprKind::IntLit(1) => {}
                    other => panic!("expected IntLit(1), got {other:?}"),
                }
            }
            other => panic!("expected VarDecl, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 5c. var declaration with type annotation
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_var_with_type() {
        let module = parse_ok("function f() { var x: i32 = 1; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(v) => {
                assert_eq!(v.binding, VarBinding::Var);
                assert_eq!(v.name.name, "x");
                assert!(v.type_ann.is_some());
            }
            other => panic!("expected VarDecl, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // 5d. var declaration with string type
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_var_string_type() {
        let module = parse_ok("function f() { var x: string = \"hello\"; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(v) => {
                assert_eq!(v.binding, VarBinding::Var);
                assert_eq!(v.name.name, "x");
                assert!(v.type_ann.is_some());
                match &v.init.kind {
                    ExprKind::StringLit(s) => assert_eq!(s.as_str(), "hello"),
                    other => panic!("expected StringLit, got {other:?}"),
                }
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
    // Task 063: Logical assignment operators
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_nullish_assign_produces_logical_assign() {
        let module = parse_ok("function f() { x ??= 5; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::LogicalAssign(la) => {
                    assert_eq!(la.target.name, "x");
                    assert_eq!(la.op, LogicalAssignOp::NullishAssign);
                    assert!(
                        matches!(la.value.kind, ExprKind::IntLit(5)),
                        "expected IntLit(5), got {:?}",
                        la.value.kind
                    );
                }
                other => panic!("expected LogicalAssign, got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_or_assign_produces_logical_assign() {
        let module = parse_ok("function f() { x ||= true; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::LogicalAssign(la) => {
                    assert_eq!(la.target.name, "x");
                    assert_eq!(la.op, LogicalAssignOp::OrAssign);
                    assert!(
                        matches!(la.value.kind, ExprKind::BoolLit(true)),
                        "expected BoolLit(true), got {:?}",
                        la.value.kind
                    );
                }
                other => panic!("expected LogicalAssign, got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_and_assign_produces_logical_assign() {
        let module = parse_ok("function f() { x &&= false; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Expr(e) => match &e.kind {
                ExprKind::LogicalAssign(la) => {
                    assert_eq!(la.target.name, "x");
                    assert_eq!(la.op, LogicalAssignOp::AndAssign);
                    assert!(
                        matches!(la.value.kind, ExprKind::BoolLit(false)),
                        "expected BoolLit(false), got {:?}",
                        la.value.kind
                    );
                }
                other => panic!("expected LogicalAssign, got {other:?}"),
            },
            other => panic!("expected Expr, got {other:?}"),
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

    // Test: Parse optional field in type def: `port?: i32` produces optional field
    #[test]
    fn test_parser_optional_field_in_type_def() {
        let source = "type Config = { port?: i32 }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "Config");
                assert_eq!(td.fields.len(), 1);
                assert_eq!(td.fields[0].name.name, "port");
                assert!(td.fields[0].optional, "port field should be optional");
            }
            _ => panic!("expected TypeDef"),
        }
    }

    // Test: Mix of required and optional fields in type def
    #[test]
    fn test_parser_optional_field_mixed() {
        let source = "type Config = { host: string, port?: i32, debug?: bool }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "Config");
                assert_eq!(td.fields.len(), 3);
                assert_eq!(td.fields[0].name.name, "host");
                assert!(!td.fields[0].optional, "host should not be optional");
                assert_eq!(td.fields[1].name.name, "port");
                assert!(td.fields[1].optional, "port should be optional");
                assert_eq!(td.fields[2].name.name, "debug");
                assert!(td.fields[2].optional, "debug should be optional");
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
                assert_eq!(destr.fields[0].field_name.name, "name");
                assert!(destr.fields[0].local_name.is_none());
                assert!(destr.fields[0].default_value.is_none());
                assert_eq!(destr.fields[1].field_name.name, "age");
                assert!(destr.fields[1].local_name.is_none());
                assert!(destr.fields[1].default_value.is_none());
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

    // ---------------------------------------------------------------
    // Tagged template literal parsing tests
    // ---------------------------------------------------------------

    // Test: Parse tagged template with no interpolations
    #[test]
    fn test_parser_tagged_template_basic() {
        let module = parse_ok("function main() { const x = tag`hello`; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        let ExprKind::TaggedTemplate {
            tag,
            quasis,
            expressions,
        } = &decl.init.kind
        else {
            panic!("expected TaggedTemplate, got {:?}", decl.init.kind);
        };
        // Tag is the identifier `tag`
        assert!(matches!(&tag.kind, ExprKind::Ident(ident) if ident.name == "tag"));
        // One quasi, no expressions
        assert_eq!(quasis.len(), 1);
        assert_eq!(quasis[0], "hello");
        assert!(expressions.is_empty());
    }

    // Test: Parse tagged template with a single expression
    #[test]
    fn test_parser_tagged_template_with_expr() {
        let module = parse_ok("function main() { const x = tag`hello ${name}`; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        let ExprKind::TaggedTemplate {
            tag,
            quasis,
            expressions,
        } = &decl.init.kind
        else {
            panic!("expected TaggedTemplate, got {:?}", decl.init.kind);
        };
        assert!(matches!(&tag.kind, ExprKind::Ident(ident) if ident.name == "tag"));
        assert_eq!(quasis.len(), 2);
        assert_eq!(quasis[0], "hello ");
        assert_eq!(quasis[1], "");
        assert_eq!(expressions.len(), 1);
        assert!(matches!(&expressions[0].kind, ExprKind::Ident(ident) if ident.name == "name"));
    }

    // Test: Parse tagged template with multiple expressions
    #[test]
    fn test_parser_tagged_template_multiple_exprs() {
        let module = parse_ok("function main() { const x = tag`${a} and ${b}`; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        let ExprKind::TaggedTemplate {
            tag,
            quasis,
            expressions,
        } = &decl.init.kind
        else {
            panic!("expected TaggedTemplate, got {:?}", decl.init.kind);
        };
        assert!(matches!(&tag.kind, ExprKind::Ident(ident) if ident.name == "tag"));
        assert_eq!(quasis.len(), 3);
        assert_eq!(quasis[0], "");
        assert_eq!(quasis[1], " and ");
        assert_eq!(quasis[2], "");
        assert_eq!(expressions.len(), 2);
        assert!(matches!(&expressions[0].kind, ExprKind::Ident(ident) if ident.name == "a"));
        assert!(matches!(&expressions[1].kind, ExprKind::Ident(ident) if ident.name == "b"));
    }

    // Test: Untagged template literal is not affected by tagged template parsing
    #[test]
    fn test_parser_untagged_template_not_affected() {
        let module = parse_ok("function main() { const x = `hello ${name}`; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        let ExprKind::TemplateLit(tpl) = &decl.init.kind else {
            panic!(
                "expected TemplateLit (not TaggedTemplate), got {:?}",
                decl.init.kind
            );
        };
        // Should still parse as regular template with 3 parts
        assert_eq!(tpl.parts.len(), 3);
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
                assert!(matches!(
                    elements[0],
                    ArrayElement::Expr(Expr {
                        kind: ExprKind::IntLit(1),
                        ..
                    })
                ));
                assert!(matches!(
                    elements[1],
                    ArrayElement::Expr(Expr {
                        kind: ExprKind::IntLit(2),
                        ..
                    })
                ));
                assert!(matches!(
                    elements[2],
                    ArrayElement::Expr(Expr {
                        kind: ExprKind::IntLit(3),
                        ..
                    })
                ));
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
                let binding = tc.catch_binding.as_ref().expect("expected catch binding");
                assert_eq!(binding.name, "err");
                let catch_type = tc.catch_type.as_ref().expect("expected catch type");
                assert!(matches!(&catch_type.kind, TypeKind::Named(id) if id.name == "string"));
                let catch_block = tc.catch_block.as_ref().expect("expected catch block");
                assert_eq!(catch_block.stmts.len(), 1);
                assert!(tc.finally_block.is_none());
            }
            _ => panic!("expected TryCatch"),
        }
    }

    // Parse try/catch/finally
    #[test]
    fn test_parser_try_catch_finally_produces_all_blocks() {
        let source = "\
function main() {
  try {
    riskyOp();
  } catch (err: string) {
    console.log(err);
  } finally {
    cleanup();
  }
}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::TryCatch(tc) => {
                assert_eq!(tc.try_block.stmts.len(), 1);
                let binding = tc.catch_binding.as_ref().expect("expected catch binding");
                assert_eq!(binding.name, "err");
                assert!(tc.catch_type.is_some());
                let catch_block = tc.catch_block.as_ref().expect("expected catch block");
                assert_eq!(catch_block.stmts.len(), 1);
                let finally_block = tc.finally_block.as_ref().expect("expected finally block");
                assert_eq!(finally_block.stmts.len(), 1);
            }
            _ => panic!("expected TryCatch"),
        }
    }

    // Parse try/finally (no catch)
    #[test]
    fn test_parser_try_finally_without_catch_produces_finally_block() {
        let source = "\
function main() {
  try {
    riskyOp();
  } finally {
    cleanup();
  }
}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::TryCatch(tc) => {
                assert_eq!(tc.try_block.stmts.len(), 1);
                assert!(tc.catch_binding.is_none());
                assert!(tc.catch_type.is_none());
                assert!(tc.catch_block.is_none());
                let finally_block = tc.finally_block.as_ref().expect("expected finally block");
                assert_eq!(finally_block.stmts.len(), 1);
            }
            _ => panic!("expected TryCatch"),
        }
    }

    // Parse finally with multiple statements
    #[test]
    fn test_parser_finally_block_with_multiple_statements() {
        let source = "\
function main() {
  try {
    riskyOp();
  } catch (err: string) {
    console.log(err);
  } finally {
    cleanup();
    console.log(\"done\");
    reset();
  }
}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::TryCatch(tc) => {
                let finally_block = tc.finally_block.as_ref().expect("expected finally block");
                assert_eq!(finally_block.stmts.len(), 3);
            }
            _ => panic!("expected TryCatch"),
        }
    }

    // Verify finally is optional (existing try/catch still works)
    #[test]
    fn test_parser_try_catch_without_finally_still_works() {
        let source = "\
function main() {
  try {
    riskyOp();
  } catch (err: string) {
    console.log(err);
  }
}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        match &f.body.stmts[0] {
            Stmt::TryCatch(tc) => {
                assert!(tc.catch_binding.is_some());
                assert!(tc.catch_block.is_some());
                assert!(tc.finally_block.is_none());
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

    #[test]
    fn test_fn_type_named_params() {
        let source = "function apply(fn_: (x: i32) => i32, value: i32): i32 { return fn_(value); }";
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
        assert_eq!(f.params[0].name.name, "fn_");
        match &f.params[0].type_ann.kind {
            TypeKind::Function(params, ret) => {
                assert_eq!(params.len(), 1);
                assert!(matches!(&params[0].kind, TypeKind::Named(ident) if ident.name == "i32"));
                assert!(matches!(&ret.kind, TypeKind::Named(ident) if ident.name == "i32"));
            }
            other => panic!("expected Function type, got {other:?}"),
        }
    }

    #[test]
    fn test_fn_type_multiple_named_params() {
        let source =
            "function apply(fn_: (x: i32, y: string) => bool): bool { return fn_(1, \"\"); }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        match &f.params[0].type_ann.kind {
            TypeKind::Function(params, ret) => {
                assert_eq!(params.len(), 2);
                assert!(matches!(&params[0].kind, TypeKind::Named(ident) if ident.name == "i32"));
                assert!(
                    matches!(&params[1].kind, TypeKind::Named(ident) if ident.name == "string")
                );
                assert!(matches!(&ret.kind, TypeKind::Named(ident) if ident.name == "bool"));
            }
            other => panic!("expected Function type, got {other:?}"),
        }
    }

    #[test]
    fn test_fn_type_unnamed_still_works() {
        // Regression: unnamed params must still work after named-param support added.
        let source = "function apply(f: (i32) => i32): i32 { return f(1); }";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected function"),
        };
        match &f.params[0].type_ann.kind {
            TypeKind::Function(params, ret) => {
                assert_eq!(params.len(), 1);
                assert!(matches!(&params[0].kind, TypeKind::Named(ident) if ident.name == "i32"));
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

    // ---------------------------------------------------------------
    // Task 018: For-of loops, break, continue
    // ---------------------------------------------------------------

    // T018-1: Parse `for (const x of items) { console.log(x); }` → ForOfStmt
    #[test]
    fn test_parser_for_of_const_produces_for_of_stmt() {
        let source = r#"function main() {
  for (const x of items) {
    console.log(x);
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::For(for_of) => {
                assert_eq!(for_of.binding, VarBinding::Const);
                assert_eq!(for_of.variable.name, "x");
                match &for_of.iterable.kind {
                    ExprKind::Ident(ident) => assert_eq!(ident.name, "items"),
                    other => panic!("expected Ident iterable, got {other:?}"),
                }
                assert!(!for_of.body.stmts.is_empty());
            }
            other => panic!("expected For statement, got {other:?}"),
        }
    }

    // T018-2: Parse `for (let x of items) { ... }` → ForOfStmt with Let binding
    #[test]
    fn test_parser_for_of_let_produces_for_of_stmt() {
        let source = r#"function main() {
  for (let x of items) {
    console.log(x);
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::For(for_of) => {
                assert_eq!(for_of.binding, VarBinding::Let);
                assert_eq!(for_of.variable.name, "x");
            }
            other => panic!("expected For statement, got {other:?}"),
        }
    }

    // T018-3: Parse `break;` → BreakStmt
    #[test]
    fn test_parser_break_produces_break_stmt() {
        let source = r#"function main() {
  while (true) {
    break;
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::While(while_stmt) => {
                assert!(!while_stmt.body.stmts.is_empty());
                match &while_stmt.body.stmts[0] {
                    Stmt::Break(_) => {}
                    other => panic!("expected Break statement, got {other:?}"),
                }
            }
            other => panic!("expected While statement, got {other:?}"),
        }
    }

    // T018-4: Parse `continue;` → ContinueStmt
    #[test]
    fn test_parser_continue_produces_continue_stmt() {
        let source = r#"function main() {
  while (true) {
    continue;
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::While(while_stmt) => {
                assert!(!while_stmt.body.stmts.is_empty());
                match &while_stmt.body.stmts[0] {
                    Stmt::Continue(_) => {}
                    other => panic!("expected Continue statement, got {other:?}"),
                }
            }
            other => panic!("expected While statement, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 024: import/export parsing
    // ---------------------------------------------------------------

    // Test 1: Parse `import { User } from "./models";` → ImportDecl
    #[test]
    fn test_parser_import_decl_single_name() {
        let module = parse_ok("import { User } from \"./models\";");
        assert_eq!(module.items.len(), 1);
        let item = &module.items[0];
        assert!(!item.exported);
        match &item.kind {
            ItemKind::Import(import) => {
                assert_eq!(import.names.len(), 1);
                assert_eq!(import.names[0].name, "User");
                assert_eq!(import.source.value, "./models");
            }
            other => panic!("expected Import item, got {other:?}"),
        }
    }

    // Test 1b: Parse import with multiple names
    #[test]
    fn test_parser_import_decl_multiple_names() {
        let module = parse_ok("import { User, Post } from \"./models\";");
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::Import(import) => {
                assert_eq!(import.names.len(), 2);
                assert_eq!(import.names[0].name, "User");
                assert_eq!(import.names[1].name, "Post");
                assert_eq!(import.source.value, "./models");
            }
            other => panic!("expected Import item, got {other:?}"),
        }
    }

    // Test 2: Parse `export function greet(): void { ... }` → exported function
    #[test]
    fn test_parser_export_function_decl() {
        let module = parse_ok("export function greet(): void { return; }");
        assert_eq!(module.items.len(), 1);
        let item = &module.items[0];
        assert!(item.exported, "function should be exported");
        match &item.kind {
            ItemKind::Function(f) => {
                assert_eq!(f.name.name, "greet");
            }
            other => panic!("expected Function item, got {other:?}"),
        }
    }

    // Test 3: Parse `export type User = { ... }` → exported type
    #[test]
    fn test_parser_export_type_def() {
        let module = parse_ok("export type User = { name: string, age: u32 }");
        assert_eq!(module.items.len(), 1);
        let item = &module.items[0];
        assert!(item.exported, "type should be exported");
        match &item.kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "User");
                assert_eq!(td.fields.len(), 2);
            }
            other => panic!("expected TypeDef item, got {other:?}"),
        }
    }

    // Test 4: Parse `export { User } from "./models";` → ReExportDecl
    #[test]
    fn test_parser_re_export_decl() {
        let module = parse_ok("export { User } from \"./models\";");
        assert_eq!(module.items.len(), 1);
        let item = &module.items[0];
        assert!(item.exported, "re-export should be exported");
        match &item.kind {
            ItemKind::ReExport(re) => {
                assert_eq!(re.names.len(), 1);
                assert_eq!(re.names[0].name, "User");
                assert_eq!(re.source.value, "./models");
            }
            other => panic!("expected ReExport item, got {other:?}"),
        }
    }

    // Test: Parse `export * from "./utils"` → WildcardReExportDecl
    #[test]
    fn test_parser_export_star_from() {
        let module = parse_ok("export * from \"./utils\";");
        assert_eq!(module.items.len(), 1);
        let item = &module.items[0];
        assert!(item.exported, "wildcard re-export should be exported");
        match &item.kind {
            ItemKind::WildcardReExport(re) => {
                assert_eq!(re.source.value, "./utils");
            }
            other => panic!("expected WildcardReExport item, got {other:?}"),
        }
    }

    // Test: Parse `export * from "serde"` with external crate name
    #[test]
    fn test_parser_export_star_from_package() {
        let module = parse_ok("export * from \"serde\";");
        assert_eq!(module.items.len(), 1);
        let item = &module.items[0];
        assert!(item.exported, "wildcard re-export should be exported");
        match &item.kind {
            ItemKind::WildcardReExport(re) => {
                assert_eq!(re.source.value, "serde");
            }
            other => panic!("expected WildcardReExport item, got {other:?}"),
        }
    }

    // Test: Source module string is preserved in wildcard re-export
    #[test]
    fn test_parser_export_star_preserves_source() {
        let module = parse_ok("export * from \"./deeply/nested/module\";");
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::WildcardReExport(re) => {
                assert_eq!(re.source.value, "./deeply/nested/module");
            }
            other => panic!("expected WildcardReExport item, got {other:?}"),
        }
    }

    // Test: export interface
    #[test]
    fn test_parser_export_interface_def() {
        let module = parse_ok("export interface Printable { display(): string; }");
        assert_eq!(module.items.len(), 1);
        let item = &module.items[0];
        assert!(item.exported, "interface should be exported");
        match &item.kind {
            ItemKind::Interface(iface) => {
                assert_eq!(iface.name.name, "Printable");
            }
            other => panic!("expected Interface item, got {other:?}"),
        }
    }

    // Test: import without semicolon (should still parse)
    #[test]
    fn test_parser_import_decl_no_semicolon() {
        let module = parse_ok("import { User } from \"./models\"\nfunction main() {}");
        assert_eq!(module.items.len(), 2);
        match &module.items[0].kind {
            ItemKind::Import(import) => {
                assert_eq!(import.names[0].name, "User");
            }
            other => panic!("expected Import item, got {other:?}"),
        }
    }

    // Test: export enum
    #[test]
    fn test_parser_export_enum_def() {
        let module = parse_ok("export type Direction = \"north\" | \"south\"");
        assert_eq!(module.items.len(), 1);
        let item = &module.items[0];
        assert!(item.exported, "enum should be exported");
        match &item.kind {
            ItemKind::EnumDef(ed) => {
                assert_eq!(ed.name.name, "Direction");
            }
            other => panic!("expected EnumDef item, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 023: Class parsing tests
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_class_with_fields_constructor_methods_produces_class_def() {
        let source = "\
class Counter {
  private count: i32;

  constructor(initial: i32) {
    this.count = initial;
  }

  increment(): void {
    this.count = this.count + 1;
  }

  get(): i32 {
    return this.count;
  }
}";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::Class(cls) => {
                assert_eq!(cls.name.name, "Counter");
                // 1 field + 1 constructor + 2 methods = 4 members
                assert_eq!(cls.members.len(), 4);

                // First member: private field
                match &cls.members[0] {
                    ClassMember::Field(f) => {
                        assert_eq!(f.name.name, "count");
                        assert_eq!(f.visibility, Visibility::Private);
                    }
                    other => panic!("expected Field, got {other:?}"),
                }

                // Second member: constructor
                match &cls.members[1] {
                    ClassMember::Constructor(ctor) => {
                        assert_eq!(ctor.params.len(), 1);
                        assert_eq!(ctor.params[0].name.name, "initial");
                    }
                    other => panic!("expected Constructor, got {other:?}"),
                }

                // Third member: method increment
                match &cls.members[2] {
                    ClassMember::Method(m) => {
                        assert_eq!(m.name.name, "increment");
                        assert_eq!(m.params.len(), 0);
                    }
                    other => panic!("expected Method, got {other:?}"),
                }

                // Fourth member: method get
                match &cls.members[3] {
                    ClassMember::Method(m) => {
                        assert_eq!(m.name.name, "get");
                    }
                    other => panic!("expected Method, got {other:?}"),
                }
            }
            other => panic!("expected Class item, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_class_private_and_public_visibility() {
        let source = "\
class Foo {
  private x: i32;
  public y: i32;
  z: i32;
}";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Class(cls) => {
                match &cls.members[0] {
                    ClassMember::Field(f) => assert_eq!(f.visibility, Visibility::Private),
                    other => panic!("expected Field, got {other:?}"),
                }
                match &cls.members[1] {
                    ClassMember::Field(f) => assert_eq!(f.visibility, Visibility::Public),
                    other => panic!("expected Field, got {other:?}"),
                }
                // Default visibility is public
                match &cls.members[2] {
                    ClassMember::Field(f) => assert_eq!(f.visibility, Visibility::Public),
                    other => panic!("expected Field, got {other:?}"),
                }
            }
            other => panic!("expected Class, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_class_this_field_access_produces_field_access_on_this() {
        let source = "\
class Foo {
  x: i32;
  constructor() {
    this.x = 0;
  }
  get(): i32 {
    return this.x;
  }
}";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Class(cls) => {
                // Check constructor body has a FieldAssign
                match &cls.members[1] {
                    ClassMember::Constructor(ctor) => {
                        assert_eq!(ctor.body.stmts.len(), 1);
                        match &ctor.body.stmts[0] {
                            Stmt::Expr(expr) => match &expr.kind {
                                ExprKind::FieldAssign(fa) => {
                                    assert!(
                                        matches!(fa.object.kind, ExprKind::This),
                                        "expected This"
                                    );
                                    assert_eq!(fa.field.name, "x");
                                }
                                other => panic!("expected FieldAssign, got {other:?}"),
                            },
                            other => panic!("expected Expr, got {other:?}"),
                        }
                    }
                    other => panic!("expected Constructor, got {other:?}"),
                }
            }
            other => panic!("expected Class, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_class_implements_produces_implements_list() {
        let source = "\
class Foo implements Bar, Baz {
  describe(): string {
    return \"foo\";
  }
}";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Class(cls) => {
                assert_eq!(cls.implements.len(), 2);
                assert_eq!(cls.implements[0].name, "Bar");
                assert_eq!(cls.implements[1].name, "Baz");
            }
            other => panic!("expected Class, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Async/await tests (Task 028)
    // ---------------------------------------------------------------

    // 2. Parser — async function: parses as FnDecl { is_async: true }
    #[test]
    fn test_parser_async_function_produces_async_fn_decl() {
        let module = parse_ok("async function foo(): string { return \"hi\"; }");
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::Function(f) => {
                assert!(f.is_async);
                assert_eq!(f.name.name, "foo");
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // 3. Parser — await expression: `await fetchData()` parses as Await(Call { ... })
    #[test]
    fn test_parser_await_expression_produces_await_node() {
        let module = parse_ok("async function test() { const x = await fetchData(); }");
        match &module.items[0].kind {
            ItemKind::Function(f) => {
                assert!(f.is_async);
                match &f.body.stmts[0] {
                    Stmt::VarDecl(decl) => match &decl.init.kind {
                        ExprKind::Await(inner) => match &inner.kind {
                            ExprKind::Call(call) => {
                                assert_eq!(call.callee.name, "fetchData");
                                assert!(call.args.is_empty());
                            }
                            other => panic!("expected Call inside Await, got {other:?}"),
                        },
                        other => panic!("expected Await, got {other:?}"),
                    },
                    other => panic!("expected VarDecl, got {other:?}"),
                }
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // 4. Parser — await precedence: `await a + b` parses as Binary(Await(a), Add, b)
    #[test]
    fn test_parser_await_precedence_lower_than_binary() {
        let module = parse_ok("async function test() { const x = await a + b; }");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::VarDecl(decl) => match &decl.init.kind {
                    ExprKind::Binary(bin) => {
                        assert!(matches!(bin.op, BinaryOp::Add));
                        match &bin.left.kind {
                            ExprKind::Await(inner) => match &inner.kind {
                                ExprKind::Ident(ident) => assert_eq!(ident.name, "a"),
                                other => panic!("expected Ident, got {other:?}"),
                            },
                            other => panic!("expected Await, got {other:?}"),
                        }
                        match &bin.right.kind {
                            ExprKind::Ident(ident) => assert_eq!(ident.name, "b"),
                            other => panic!("expected Ident, got {other:?}"),
                        }
                    }
                    other => panic!("expected Binary, got {other:?}"),
                },
                other => panic!("expected VarDecl, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // 5. Parser — await with method call: `await obj.fetch()` parses as Await(MethodCall { ... })
    #[test]
    fn test_parser_await_with_method_call() {
        let module = parse_ok("async function test() { const x = await obj.fetch(); }");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::VarDecl(decl) => match &decl.init.kind {
                    ExprKind::Await(inner) => match &inner.kind {
                        ExprKind::MethodCall(mc) => {
                            assert_eq!(mc.method.name, "fetch");
                        }
                        other => panic!("expected MethodCall inside Await, got {other:?}"),
                    },
                    other => panic!("expected Await, got {other:?}"),
                },
                other => panic!("expected VarDecl, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // 6. Parser — async closure: `async (x: i32) => x`
    #[test]
    fn test_parser_async_closure_produces_async_closure_expr() {
        let module = parse_ok("function test() { const f = async (x: i32) => x; }");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::VarDecl(decl) => match &decl.init.kind {
                    ExprKind::Closure(closure) => {
                        assert!(closure.is_async);
                        assert!(!closure.is_move);
                        assert_eq!(closure.params.len(), 1);
                        assert_eq!(closure.params[0].name.name, "x");
                    }
                    other => panic!("expected Closure, got {other:?}"),
                },
                other => panic!("expected VarDecl, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // 7. Parser — async move closure: `async move () => { process(); }`
    #[test]
    fn test_parser_async_move_closure_produces_both_flags() {
        let module = parse_ok("function test() { const f = async move () => { process(); }; }");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::VarDecl(decl) => match &decl.init.kind {
                    ExprKind::Closure(closure) => {
                        assert!(closure.is_async);
                        assert!(closure.is_move);
                    }
                    other => panic!("expected Closure, got {other:?}"),
                },
                other => panic!("expected VarDecl, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // 8. Parser — non-async function still parses with is_async: false
    #[test]
    fn test_parser_non_async_function_has_is_async_false() {
        let module = parse_ok("function foo() { }");
        match &module.items[0].kind {
            ItemKind::Function(f) => {
                assert!(!f.is_async);
                assert_eq!(f.name.name, "foo");
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Extra: export async function
    #[test]
    fn test_parser_export_async_function() {
        let module = parse_ok("export async function handler(): string { return \"ok\"; }");
        assert_eq!(module.items.len(), 1);
        assert!(module.items[0].exported);
        match &module.items[0].kind {
            ItemKind::Function(f) => {
                assert!(f.is_async);
                assert_eq!(f.name.name, "handler");
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 030: Array destructuring parser tests
    // ---------------------------------------------------------------

    // Test: const [a, b] = expr; parses as ArrayDestructure
    #[test]
    fn test_parse_array_destructure_two_elements() {
        let source = r#"function test() {
            const [user, posts] = getResults();
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::ArrayDestructure(adestr) => {
                    assert_eq!(adestr.elements.len(), 2);
                    match &adestr.elements[0] {
                        ArrayDestructureElement::Single(ident) => assert_eq!(ident.name, "user"),
                        other => panic!("expected Single, got {other:?}"),
                    }
                    match &adestr.elements[1] {
                        ArrayDestructureElement::Single(ident) => assert_eq!(ident.name, "posts"),
                        other => panic!("expected Single, got {other:?}"),
                    }
                    assert_eq!(adestr.binding, VarBinding::Const);
                }
                other => panic!("expected ArrayDestructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: let [a, b, c] = expr; with trailing comma
    #[test]
    fn test_parse_array_destructure_trailing_comma() {
        let source = r#"function test() {
            let [x, y, z,] = getData();
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::ArrayDestructure(adestr) => {
                    assert_eq!(adestr.elements.len(), 3);
                    assert_eq!(adestr.binding, VarBinding::Let);
                }
                other => panic!("expected ArrayDestructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 062: Destructuring rename, defaults, and array rest tests
    // ---------------------------------------------------------------

    // Test: const { name: n } = user; parses with rename
    #[test]
    fn test_parse_destructure_rename() {
        let source = r#"function test() {
            const { name: n } = user;
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::Destructure(destr) => {
                    assert_eq!(destr.fields.len(), 1);
                    assert_eq!(destr.fields[0].field_name.name, "name");
                    assert_eq!(
                        destr.fields[0].local_name.as_ref().map(|l| &l.name[..]),
                        Some("n")
                    );
                    assert!(destr.fields[0].default_value.is_none());
                }
                other => panic!("expected Destructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: const { x: a, y: b } = point; parses multiple renames
    #[test]
    fn test_parse_destructure_multiple_renames() {
        let source = r#"function test() {
            const { x: a, y: b } = point;
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::Destructure(destr) => {
                    assert_eq!(destr.fields.len(), 2);
                    assert_eq!(destr.fields[0].field_name.name, "x");
                    assert_eq!(
                        destr.fields[0].local_name.as_ref().map(|l| &l.name[..]),
                        Some("a")
                    );
                    assert_eq!(destr.fields[1].field_name.name, "y");
                    assert_eq!(
                        destr.fields[1].local_name.as_ref().map(|l| &l.name[..]),
                        Some("b")
                    );
                }
                other => panic!("expected Destructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: const { name, age: a } = user; mixed rename and simple
    #[test]
    fn test_parse_destructure_mixed_rename() {
        let source = r#"function test() {
            const { name, age: a } = user;
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::Destructure(destr) => {
                    assert_eq!(destr.fields.len(), 2);
                    assert_eq!(destr.fields[0].field_name.name, "name");
                    assert!(destr.fields[0].local_name.is_none());
                    assert_eq!(destr.fields[1].field_name.name, "age");
                    assert_eq!(
                        destr.fields[1].local_name.as_ref().map(|l| &l.name[..]),
                        Some("a")
                    );
                }
                other => panic!("expected Destructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: const { name = "x" } = config; parses with default
    #[test]
    fn test_parse_destructure_default() {
        let source = r#"function test() {
            const { name = "x" } = config;
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::Destructure(destr) => {
                    assert_eq!(destr.fields.len(), 1);
                    assert_eq!(destr.fields[0].field_name.name, "name");
                    assert!(destr.fields[0].local_name.is_none());
                    assert!(destr.fields[0].default_value.is_some());
                }
                other => panic!("expected Destructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: const { name: n = "x" } = config; rename + default
    #[test]
    fn test_parse_destructure_rename_and_default() {
        let source = r#"function test() {
            const { name: n = "x" } = config;
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::Destructure(destr) => {
                    assert_eq!(destr.fields.len(), 1);
                    assert_eq!(destr.fields[0].field_name.name, "name");
                    assert_eq!(
                        destr.fields[0].local_name.as_ref().map(|l| &l.name[..]),
                        Some("n")
                    );
                    assert!(destr.fields[0].default_value.is_some());
                }
                other => panic!("expected Destructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: const [first, ...rest] = arr; parses with rest element
    #[test]
    fn test_parse_array_destructure_rest() {
        let source = r#"function test() {
            const [first, ...rest] = arr;
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::ArrayDestructure(adestr) => {
                    assert_eq!(adestr.elements.len(), 2);
                    match &adestr.elements[0] {
                        ArrayDestructureElement::Single(ident) => {
                            assert_eq!(ident.name, "first");
                        }
                        other => panic!("expected Single, got {other:?}"),
                    }
                    match &adestr.elements[1] {
                        ArrayDestructureElement::Rest(ident) => {
                            assert_eq!(ident.name, "rest");
                        }
                        other => panic!("expected Rest, got {other:?}"),
                    }
                }
                other => panic!("expected ArrayDestructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: const [a, b, ...rest] = arr; multiple single + rest
    #[test]
    fn test_parse_array_destructure_multi_then_rest() {
        let source = r#"function test() {
            const [a, b, ...rest] = arr;
        }"#;
        let (module, diags) = parse_source(source);
        assert!(diags.is_empty(), "unexpected parse diagnostics: {diags:?}");
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::ArrayDestructure(adestr) => {
                    assert_eq!(adestr.elements.len(), 3);
                    assert!(
                        matches!(&adestr.elements[0], ArrayDestructureElement::Single(i) if i.name == "a")
                    );
                    assert!(
                        matches!(&adestr.elements[1], ArrayDestructureElement::Single(i) if i.name == "b")
                    );
                    assert!(
                        matches!(&adestr.elements[2], ArrayDestructureElement::Rest(i) if i.name == "rest")
                    );
                }
                other => panic!("expected ArrayDestructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Inline Rust block tests
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_rust_block_simple() {
        let source = r#"function main(): void {
  rust {
    println!("hello");
  }
}"#;
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::RustBlock(rb) => {
                    assert!(
                        rb.code.contains("println!"),
                        "expected rust block code to contain println!, got: {:?}",
                        rb.code
                    );
                }
                other => panic!("expected RustBlock statement, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_rust_block_nested_braces() {
        let source = r#"function main(): void {
  rust {
    if true {
      if false {
        println!("deeply nested");
      }
    }
  }
}"#;
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::RustBlock(rb) => {
                    assert!(
                        rb.code.contains("deeply nested"),
                        "expected deeply nested content, got: {:?}",
                        rb.code
                    );
                }
                other => panic!("expected RustBlock, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_rust_block_empty() {
        let source = r#"function main(): void {
  rust { }
}"#;
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Function(f) => match &f.body.stmts[0] {
                Stmt::RustBlock(rb) => {
                    assert!(
                        rb.code.trim().is_empty(),
                        "expected empty rust block, got: {:?}",
                        rb.code
                    );
                }
                other => panic!("expected RustBlock, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_rust_block_module_level() {
        let source = r#"rust {
  type Pair = (i32, i32);
}

function main(): void {
  console.log("hello");
}"#;
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::RustBlock(rb) => {
                assert!(
                    rb.code.contains("Pair"),
                    "expected type alias in rust block, got: {:?}",
                    rb.code
                );
            }
            other => panic!("expected RustBlock item, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_rust_block_unclosed_produces_diagnostic() {
        let source = r#"function main(): void {
  rust {
    let x = 1;
}"#;
        let (_, diagnostics) = parse_source(source);
        assert!(
            !diagnostics.is_empty(),
            "expected diagnostic for unclosed rust block"
        );
        let has_unclosed = diagnostics
            .iter()
            .any(|d| d.message.contains("unclosed") || d.message.contains("unterminated"));
        assert!(
            has_unclosed,
            "expected unclosed rust block diagnostic, got: {diagnostics:?}"
        );
    }

    // ---------------------------------------------------------------
    // shared<T> type and shared(expr) constructor tests
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_shared_type_in_type_position_produces_shared_kind() {
        let module = parse_ok("function main() { const x: shared<i32> = shared(0); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            let type_ann = decl.type_ann.as_ref().expect("expected type annotation");
            match &type_ann.kind {
                TypeKind::Shared(inner) => {
                    assert!(
                        matches!(&inner.kind, TypeKind::Named(ident) if ident.name == "i32"),
                        "expected inner type i32, got: {:?}",
                        inner.kind
                    );
                }
                other => panic!("expected TypeKind::Shared, got: {other:?}"),
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_shared_constructor_in_expr_position_produces_shared_expr() {
        let module = parse_ok("function main() { const x: shared<i32> = shared(0); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::Shared(inner) => {
                    assert!(
                        matches!(&inner.kind, ExprKind::IntLit(0)),
                        "expected IntLit(0), got: {:?}",
                        inner.kind
                    );
                }
                other => panic!("expected ExprKind::Shared, got: {other:?}"),
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_shared_without_type_param_produces_diagnostic() {
        let source = "function main() { const x: shared = shared(0); }";
        let (_module, diagnostics) = parse_source(source);
        assert!(
            !diagnostics.is_empty(),
            "expected diagnostic for shared without type parameter"
        );
        assert!(
            diagnostics.iter().any(|d| d.message.contains("shared")),
            "expected shared-related diagnostic, got: {diagnostics:?}"
        );
    }

    // ---------------------------------------------------------------------------
    // Task 055: Function Features — Optional, Default, Rest Parameters
    // ---------------------------------------------------------------------------

    // T055-P1: Optional parameter
    #[test]
    fn test_parser_optional_param() {
        let source = "function greet(name: string, title?: string): void { }";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");

        if let ItemKind::Function(f) = &module.items[0].kind {
            assert_eq!(f.params.len(), 2);
            assert!(!f.params[0].optional);
            assert!(f.params[1].optional);
            assert_eq!(f.params[1].name.name, "title");
        } else {
            panic!("expected function");
        }
    }

    // T055-P2: Default parameter
    #[test]
    fn test_parser_default_param() {
        let source = "function connect(host: string, port: i32 = 8080): void { }";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");

        if let ItemKind::Function(f) = &module.items[0].kind {
            assert_eq!(f.params.len(), 2);
            assert!(f.params[1].default_value.is_some());
            if let Some(Expr {
                kind: ExprKind::IntLit(v),
                ..
            }) = &f.params[1].default_value
            {
                assert_eq!(*v, 8080);
            } else {
                panic!("expected IntLit default value");
            }
        } else {
            panic!("expected function");
        }
    }

    // T055-P3: Rest parameter
    #[test]
    fn test_parser_rest_param() {
        let source = "function log(...messages: Array<string>): void { }";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");

        if let ItemKind::Function(f) = &module.items[0].kind {
            assert_eq!(f.params.len(), 1);
            assert!(f.params[0].is_rest);
            assert_eq!(f.params[0].name.name, "messages");
        } else {
            panic!("expected function");
        }
    }

    // T055-P4: Combined params
    #[test]
    fn test_parser_combined_params() {
        let source = "function foo(a: string, b?: i32, c: bool = true): void { }";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");

        if let ItemKind::Function(f) = &module.items[0].kind {
            assert_eq!(f.params.len(), 3);
            assert!(!f.params[0].optional);
            assert!(f.params[0].default_value.is_none());
            assert!(!f.params[0].is_rest);

            assert!(f.params[1].optional);
            assert!(f.params[1].default_value.is_none());

            assert!(!f.params[2].optional);
            assert!(f.params[2].default_value.is_some());
        } else {
            panic!("expected function");
        }
    }

    // T055-P5: Spread argument in call
    #[test]
    fn test_parser_spread_arg() {
        let source = "function main() { foo(...items); }";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");

        if let ItemKind::Function(f) = &module.items[0].kind {
            if let Stmt::Expr(expr) = &f.body.stmts[0] {
                if let ExprKind::Call(call) = &expr.kind {
                    assert_eq!(call.args.len(), 1);
                    assert!(matches!(call.args[0].kind, ExprKind::SpreadArg(_)));
                } else {
                    panic!("expected call expression");
                }
            } else {
                panic!("expected expression statement");
            }
        } else {
            panic!("expected function");
        }
    }

    // T055-P6: DotDotDot token
    #[test]
    fn test_parser_dot_dot_dot_token() {
        let source = "function f(...args: Array<i32>): void { }";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");

        if let ItemKind::Function(f) = &module.items[0].kind {
            assert!(f.params[0].is_rest);
        } else {
            panic!("expected function");
        }
    }

    // --- Task 054: Operators and Expressions ---

    #[test]
    fn test_parser_ternary_produces_ternary_node() {
        let module = parse_ok("function main() { const x: i32 = true ? 1 : 0; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            assert!(
                matches!(&decl.init.kind, ExprKind::Ternary(_, _, _)),
                "expected Ternary, got: {:?}",
                decl.init.kind
            );
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_nested_ternary_right_associative() {
        let module = parse_ok("function f() { const x: i32 = a ? 1 : b ? 2 : 3; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Ternary(_, _, alternate) = &decl.init.kind {
                assert!(
                    matches!(&alternate.kind, ExprKind::Ternary(_, _, _)),
                    "alternate should be another ternary"
                );
            } else {
                panic!("expected Ternary");
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_exponentiation_produces_pow_binary() {
        let module = parse_ok("function f() { const x: i64 = 2 ** 10; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::Pow);
            } else {
                panic!("expected Binary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_exponentiation_right_associative() {
        let module = parse_ok("function f() { const x: i64 = 2 ** 3 ** 2; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::Pow);
                // Right-hand side should also be a Pow binary
                assert!(
                    matches!(&bin.right.kind, ExprKind::Binary(inner) if inner.op == BinaryOp::Pow),
                    "expected right-associative ** parsing"
                );
            } else {
                panic!("expected Binary");
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_non_null_assert_postfix_bang() {
        let module = parse_ok("function f() { const x: i32 = y!; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            assert!(
                matches!(&decl.init.kind, ExprKind::NonNullAssert(_)),
                "expected NonNullAssert, got: {:?}",
                decl.init.kind
            );
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_as_cast() {
        let module = parse_ok("function f() { const x: f64 = y as f64; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Cast(_, ref ty) = decl.init.kind {
                assert!(
                    matches!(&ty.kind, TypeKind::Named(ident) if ident.name == "f64"),
                    "expected f64 type, got: {:?}",
                    ty.kind
                );
            } else {
                panic!("expected Cast, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_angle_bracket_cast() {
        let module = parse_ok("function f() { const x: string = <string>value; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Cast(ref inner, ref ty) = decl.init.kind {
                assert!(
                    matches!(&ty.kind, TypeKind::Named(ident) if ident.name == "string"),
                    "expected string type, got: {:?}",
                    ty.kind
                );
                assert!(
                    matches!(&inner.kind, ExprKind::Ident(ident) if ident.name == "value"),
                    "expected ident 'value', got: {:?}",
                    inner.kind
                );
            } else {
                panic!("expected Cast, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_angle_bracket_cast_complex_type() {
        let module = parse_ok("function f() { const x = <Array<i32>>y; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Cast(ref inner, ref ty) = decl.init.kind {
                assert!(
                    matches!(&ty.kind, TypeKind::Generic(ident, args)
                        if ident.name == "Array" && args.len() == 1),
                    "expected Array<i32> type, got: {:?}",
                    ty.kind
                );
                assert!(
                    matches!(&inner.kind, ExprKind::Ident(ident) if ident.name == "y"),
                    "expected ident 'y', got: {:?}",
                    inner.kind
                );
            } else {
                panic!("expected Cast, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_less_than_not_confused() {
        // `a < b` in binary position should still parse as comparison, not cast
        let module = parse_ok("function f() { const x: bool = a < b; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(ref bin) = decl.init.kind {
                assert_eq!(bin.op, BinaryOp::Lt);
            } else {
                panic!("expected Binary Lt, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_typeof_prefix() {
        let module = parse_ok("function f() { const x: string = typeof 42; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            assert!(
                matches!(&decl.init.kind, ExprKind::TypeOf(_)),
                "expected TypeOf, got: {:?}",
                decl.init.kind
            );
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_bitwise_and() {
        let module = parse_ok("function f() { const x: i64 = a & b; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::BitAnd);
            } else {
                panic!("expected Binary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_bitwise_or() {
        let module = parse_ok("function f() { const x: i64 = a | b; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::BitOr);
            } else {
                panic!("expected Binary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_bitwise_xor() {
        let module = parse_ok("function f() { const x: i64 = a ^ b; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::BitXor);
            } else {
                panic!("expected Binary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_bitwise_not() {
        let module = parse_ok("function f() { const x: i64 = ~a; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Unary(un) = &decl.init.kind {
                assert_eq!(un.op, UnaryOp::BitNot);
            } else {
                panic!("expected Unary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_left_shift() {
        let module = parse_ok("function f() { const x: i64 = 1 << 4; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::Shl);
            } else {
                panic!("expected Binary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_right_shift() {
        let module = parse_ok("function f() { const x: i64 = 16 >> 2; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::Shr);
            } else {
                panic!("expected Binary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_triple_equals_produces_eq() {
        let module = parse_ok("function f() { const x: bool = a === b; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::Eq);
            } else {
                panic!("expected Binary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_triple_not_equals_produces_ne() {
        let module = parse_ok("function f() { const x: bool = a !== b; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::Binary(bin) = &decl.init.kind {
                assert_eq!(bin.op, BinaryOp::Ne);
            } else {
                panic!("expected Binary, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    // ---- Task 057: Class Completeness parser tests ----

    /// Helper: extract the first class definition from a module.
    fn first_class(module: &Module) -> &ClassDef {
        for item in &module.items {
            if let ItemKind::Class(cls) = &item.kind {
                return cls;
            }
        }
        panic!("expected class item");
    }

    #[test]
    fn test_parser_class_field_initializer() {
        let module = parse_ok(
            "class Config {
                host: string = \"localhost\";
                port: i32 = 8080;
                constructor() {}
            }",
        );
        let cls = first_class(&module);
        let fields: Vec<&ClassField> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ClassMember::Field(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(fields.len(), 2);
        assert!(
            fields[0].initializer.is_some(),
            "host should have initializer"
        );
        assert!(
            fields[1].initializer.is_some(),
            "port should have initializer"
        );
        assert_eq!(fields[0].name.name, "host");
        assert_eq!(fields[1].name.name, "port");
    }

    #[test]
    fn test_parser_constructor_param_properties() {
        let module = parse_ok(
            "class User {
                constructor(public name: string, private age: i32) {}
            }",
        );
        let cls = first_class(&module);
        let ctor = cls.members.iter().find_map(|m| match m {
            ClassMember::Constructor(c) => Some(c),
            _ => None,
        });
        let ctor = ctor.expect("should have constructor");
        assert_eq!(ctor.params.len(), 2);
        assert_eq!(ctor.params[0].property_visibility, Some(Visibility::Public));
        assert_eq!(ctor.params[0].name.name, "name");
        assert_eq!(
            ctor.params[1].property_visibility,
            Some(Visibility::Private)
        );
        assert_eq!(ctor.params[1].name.name, "age");
    }

    #[test]
    fn test_parser_static_method() {
        let module = parse_ok(
            "class Service {
                static create(): Service { return new Service(); }
            }",
        );
        let cls = first_class(&module);
        let method = cls.members.iter().find_map(|m| match m {
            ClassMember::Method(m) => Some(m),
            _ => None,
        });
        let method = method.expect("should have method");
        assert!(method.is_static, "method should be static");
        assert_eq!(method.name.name, "create");
    }

    #[test]
    fn test_parser_static_field() {
        let module = parse_ok(
            "class Config {
                static DEFAULT_PORT: i32 = 8080;
                constructor() {}
            }",
        );
        let cls = first_class(&module);
        let field = cls.members.iter().find_map(|m| match m {
            ClassMember::Field(f) => Some(f),
            _ => None,
        });
        let field = field.expect("should have field");
        assert!(field.is_static, "field should be static");
        assert_eq!(field.name.name, "DEFAULT_PORT");
        assert!(
            field.initializer.is_some(),
            "static field should have initializer"
        );
    }

    #[test]
    fn test_parser_getter_declaration() {
        let module = parse_ok(
            "class User {
                private _name: string;
                constructor(name: string) { this._name = name; }
                get name(): string { return this._name; }
            }",
        );
        let cls = first_class(&module);
        let getter = cls.members.iter().find_map(|m| match m {
            ClassMember::Getter(g) => Some(g),
            _ => None,
        });
        let getter = getter.expect("should have getter");
        assert_eq!(getter.name.name, "name");
    }

    #[test]
    fn test_parser_setter_declaration() {
        let module = parse_ok(
            "class User {
                private _name: string;
                constructor(name: string) { this._name = name; }
                set name(value: string) { this._name = value; }
            }",
        );
        let cls = first_class(&module);
        let setter = cls.members.iter().find_map(|m| match m {
            ClassMember::Setter(s) => Some(s),
            _ => None,
        });
        let setter = setter.expect("should have setter");
        assert_eq!(setter.name.name, "name");
        assert_eq!(setter.param.name.name, "value");
    }

    #[test]
    fn test_parser_readonly_field() {
        let module = parse_ok(
            "class Config {
                readonly host: string = \"localhost\";
                constructor() {}
            }",
        );
        let cls = first_class(&module);
        let field = cls.members.iter().find_map(|m| match m {
            ClassMember::Field(f) => Some(f),
            _ => None,
        });
        let field = field.expect("should have field");
        assert!(field.readonly, "field should be readonly");
        assert_eq!(field.name.name, "host");
    }

    #[test]
    fn test_parser_method_named_get_is_not_getter() {
        // A method named `get` with `()` directly should be a regular method,
        // not a getter (since getter syntax is `get name()`)
        let module = parse_ok(
            "class Counter {
                private count: i32;
                constructor(initial: i32) { this.count = initial; }
                get(): i32 { return this.count; }
            }",
        );
        let cls = first_class(&module);
        // Should be a regular method named "get", not a getter
        let method = cls.members.iter().find_map(|m| match m {
            ClassMember::Method(m) if m.name.name == "get" => Some(m),
            _ => None,
        });
        assert!(method.is_some(), "should be a regular method named 'get'");
        let getter = cls.members.iter().find_map(|m| match m {
            ClassMember::Getter(_) => Some(()),
            _ => None,
        });
        assert!(getter.is_none(), "should not have a getter");
    }

    #[test]
    fn test_parser_class_combined_all_features() {
        let module = parse_ok(
            "class Complete {
                static MAX: i32 = 100;
                private _value: i32;
                public label: string = \"default\";
                readonly id: i32;

                constructor(public name: string, id: i32) {
                    this._value = 0;
                    this.id = id;
                }

                static create(name: string): Complete {
                    return new Complete(name, 0);
                }

                get value(): i32 { return this._value; }
                set value(v: i32) { this._value = v; }

                increment(): void {
                    this._value = this._value + 1;
                }
            }",
        );
        let cls = first_class(&module);

        // Count member types
        let mut fields = 0;
        let mut ctors = 0;
        let mut methods = 0;
        let mut getters = 0;
        let mut setters = 0;
        for m in &cls.members {
            match m {
                ClassMember::Field(_) => fields += 1,
                ClassMember::Constructor(_) => ctors += 1,
                ClassMember::Method(_) => methods += 1,
                ClassMember::Getter(_) => getters += 1,
                ClassMember::Setter(_) => setters += 1,
                ClassMember::StaticBlock(_) => {}
            }
        }
        assert_eq!(fields, 4, "4 fields (MAX, _value, label, id)");
        assert_eq!(ctors, 1, "1 constructor");
        assert_eq!(methods, 2, "2 methods (create, increment)");
        assert_eq!(getters, 1, "1 getter");
        assert_eq!(setters, 1, "1 setter");
    }

    // ---------------------------------------------------------------
    // Task 056: Spread operator in arrays and objects
    // ---------------------------------------------------------------

    // T056-P1: Single spread in array literal
    #[test]
    fn test_parser_array_spread_single() {
        let source = "function main() { const x = [...arr]; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        if let ExprKind::ArrayLit(elements) = &decl.init.kind {
            assert_eq!(elements.len(), 1);
            assert!(
                matches!(&elements[0], ArrayElement::Spread(_)),
                "expected Spread element"
            );
        } else {
            panic!("expected ArrayLit");
        }
    }

    // T056-P2: Spread then elements
    #[test]
    fn test_parser_array_spread_then_elements() {
        let source = "function main() { const x = [...arr, 1, 2]; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        if let ExprKind::ArrayLit(elements) = &decl.init.kind {
            assert_eq!(elements.len(), 3);
            assert!(matches!(&elements[0], ArrayElement::Spread(_)));
            assert!(matches!(&elements[1], ArrayElement::Expr(_)));
            assert!(matches!(&elements[2], ArrayElement::Expr(_)));
        } else {
            panic!("expected ArrayLit");
        }
    }

    // T056-P3: Elements then spread
    #[test]
    fn test_parser_array_elements_then_spread() {
        let source = "function main() { const x = [1, 2, ...arr]; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        if let ExprKind::ArrayLit(elements) = &decl.init.kind {
            assert_eq!(elements.len(), 3);
            assert!(matches!(&elements[0], ArrayElement::Expr(_)));
            assert!(matches!(&elements[1], ArrayElement::Expr(_)));
            assert!(matches!(&elements[2], ArrayElement::Spread(_)));
        } else {
            panic!("expected ArrayLit");
        }
    }

    // T056-P4: Multiple spreads
    #[test]
    fn test_parser_array_multiple_spreads() {
        let source = "function main() { const x = [...a, ...b]; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        if let ExprKind::ArrayLit(elements) = &decl.init.kind {
            assert_eq!(elements.len(), 2);
            assert!(matches!(&elements[0], ArrayElement::Spread(_)));
            assert!(matches!(&elements[1], ArrayElement::Spread(_)));
        } else {
            panic!("expected ArrayLit");
        }
    }

    // T056-P5: Object spread with field overrides
    #[test]
    fn test_parser_struct_spread_with_fields() {
        let source = "function main() { const x: User = { ...obj, name: \"Bob\" }; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        if let ExprKind::StructLit(slit) = &decl.init.kind {
            assert!(slit.spread.is_some(), "expected spread base");
            assert_eq!(slit.fields.len(), 1);
            assert_eq!(slit.fields[0].name.name, "name");
        } else {
            panic!("expected StructLit, got {:?}", decl.init.kind);
        }
    }

    // T056-P6: Pure object copy
    #[test]
    fn test_parser_struct_spread_pure_copy() {
        let source = "function main() { const x: User = { ...obj }; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        let Stmt::VarDecl(decl) = stmt else {
            panic!("expected VarDecl");
        };
        if let ExprKind::StructLit(slit) = &decl.init.kind {
            assert!(slit.spread.is_some(), "expected spread base");
            assert!(slit.fields.is_empty(), "expected no field overrides");
        } else {
            panic!("expected StructLit, got {:?}", decl.init.kind);
        }
    }

    // ---------------------------------------------------------------
    // JSDoc attachment tests
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_jsdoc_before_function_attached_to_fn_decl() {
        let source = "/** Creates a user */\nfunction createUser() {}";
        let (module, _) = parse_source(source);
        assert_eq!(module.items.len(), 1);
        if let ItemKind::Function(f) = &module.items[0].kind {
            assert_eq!(f.doc_comment.as_deref(), Some("Creates a user"));
        } else {
            panic!("Expected function");
        }
    }

    #[test]
    fn test_parser_jsdoc_before_type_attached_to_typedef() {
        let source = "/** A user type */\ntype User = { name: string }";
        let (module, _) = parse_source(source);
        assert_eq!(module.items.len(), 1);
        if let ItemKind::TypeDef(td) = &module.items[0].kind {
            assert_eq!(td.doc_comment.as_deref(), Some("A user type"));
        } else {
            panic!("Expected type def");
        }
    }

    #[test]
    fn test_parser_jsdoc_before_class_attached_to_class_def() {
        let source = "/** A counter class */\nclass Counter {\n  count: i32;\n}";
        let (module, _) = parse_source(source);
        assert_eq!(module.items.len(), 1);
        if let ItemKind::Class(cls) = &module.items[0].kind {
            assert_eq!(cls.doc_comment.as_deref(), Some("A counter class"));
        } else {
            panic!("Expected class def");
        }
    }

    #[test]
    fn test_parser_no_jsdoc_produces_none() {
        let source = "function foo() {}";
        let (module, _) = parse_source(source);
        if let ItemKind::Function(f) = &module.items[0].kind {
            assert!(f.doc_comment.is_none());
        } else {
            panic!("Expected function");
        }
    }

    #[test]
    fn test_parser_multiple_jsdoc_last_one_wins() {
        let source = "/** First */\n/** Second */\nfunction foo() {}";
        let (module, _) = parse_source(source);
        if let ItemKind::Function(f) = &module.items[0].kind {
            assert_eq!(f.doc_comment.as_deref(), Some("Second"));
        } else {
            panic!("Expected function");
        }
    }

    #[test]
    fn test_parser_jsdoc_before_class_method_attached_to_method() {
        let source = "\
class Foo {
  /** Does something */
  doIt(): void {}
}";
        let (module, _) = parse_source(source);
        if let ItemKind::Class(cls) = &module.items[0].kind {
            if let ClassMember::Method(m) = &cls.members[0] {
                assert_eq!(m.doc_comment.as_deref(), Some("Does something"));
            } else {
                panic!("Expected method");
            }
        } else {
            panic!("Expected class");
        }
    }

    // ---------------------------------------------------------------
    // Tuple type parsing tests (Task 064)
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_tuple_type_two_elements() {
        let module = parse_ok("function main() { const x: [string, i32] = [\"hi\", 42]; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            let type_ann = decl.type_ann.as_ref().expect("expected type annotation");
            match &type_ann.kind {
                TypeKind::Tuple(types) => {
                    assert_eq!(types.len(), 2);
                    assert!(
                        matches!(&types[0].kind, TypeKind::Named(ident) if ident.name == "string")
                    );
                    assert!(
                        matches!(&types[1].kind, TypeKind::Named(ident) if ident.name == "i32")
                    );
                }
                other => panic!("expected TypeKind::Tuple, got: {other:?}"),
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_tuple_type_three_elements() {
        let module =
            parse_ok("function main() { const x: [string, i32, bool] = [\"hi\", 1, true]; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            let type_ann = decl.type_ann.as_ref().expect("expected type annotation");
            match &type_ann.kind {
                TypeKind::Tuple(types) => {
                    assert_eq!(types.len(), 3);
                }
                other => panic!("expected TypeKind::Tuple, got: {other:?}"),
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_tuple_type_in_function_param() {
        let module = parse_ok("function swap(pair: [i32, i32]): void {}");
        let f = first_fn(&module);
        assert_eq!(f.params.len(), 1);
        match &f.params[0].type_ann.kind {
            TypeKind::Tuple(types) => {
                assert_eq!(types.len(), 2);
            }
            other => panic!("expected TypeKind::Tuple, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_tuple_type_in_return_position() {
        let module = parse_ok("function swap(pair: [i32, i32]): [i32, i32] { return [1, 2]; }");
        let f = first_fn(&module);
        let ret = f.return_type.as_ref().expect("expected return type");
        let ret_ann = ret.type_ann.as_ref().expect("expected type annotation");
        match &ret_ann.kind {
            TypeKind::Tuple(types) => {
                assert_eq!(types.len(), 2);
            }
            other => panic!("expected TypeKind::Tuple, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_array_destructure_with_tuple_type_annotation() {
        let module = parse_ok("function main() { const [a, b]: [string, i32] = pair; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::ArrayDestructure(adestr) = stmt {
            assert_eq!(adestr.elements.len(), 2);
            let type_ann = adestr.type_ann.as_ref().expect("expected type annotation");
            match &type_ann.kind {
                TypeKind::Tuple(types) => {
                    assert_eq!(types.len(), 2);
                }
                other => panic!("expected TypeKind::Tuple, got: {other:?}"),
            }
        } else {
            panic!("expected ArrayDestructure");
        }
    }

    // ---------------------------------------------------------------
    // Task 066: Async iteration and Promise methods
    // ---------------------------------------------------------------

    // T066-1: Parse `for await (const item of stream) { ... }` → ForOfStmt with is_await = true
    #[test]
    fn test_parser_for_await_const_produces_for_of_stmt_with_is_await() {
        let source = r#"async function main() {
  for await (const msg of channel) {
    console.log(msg);
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::For(for_of) => {
                assert!(for_of.is_await, "expected is_await = true");
                assert_eq!(for_of.binding, VarBinding::Const);
                assert_eq!(for_of.variable.name, "msg");
                match &for_of.iterable.kind {
                    ExprKind::Ident(ident) => assert_eq!(ident.name, "channel"),
                    other => panic!("expected Ident iterable, got {other:?}"),
                }
                assert!(!for_of.body.stmts.is_empty());
            }
            other => panic!("expected For statement, got {other:?}"),
        }
    }

    // T066-2: Parse regular `for (const x of items)` still has is_await = false
    #[test]
    fn test_parser_regular_for_of_has_is_await_false() {
        let source = r#"function main() {
  for (const x of items) {
    console.log(x);
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::For(for_of) => {
                assert!(!for_of.is_await, "regular for-of should not have is_await");
            }
            other => panic!("expected For statement, got {other:?}"),
        }
    }

    // T066-3: Parse `for await (let item of stream)` with let binding
    #[test]
    fn test_parser_for_await_let_binding() {
        let source = r#"async function process() {
  for await (let item of stream) {
    console.log(item);
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::For(for_of) => {
                assert!(for_of.is_await, "expected is_await = true");
                assert_eq!(for_of.binding, VarBinding::Let);
                assert_eq!(for_of.variable.name, "item");
            }
            other => panic!("expected For statement, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Task 067: Minor Syntax Completions — parser tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parser_import_type_sets_is_type_only() {
        let source = "import type { User } from \"./models\";";
        let (module, diagnostics) = parse_source(source);
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected: {diagnostics:?}"
        );
        if let ItemKind::Import(import) = &module.items[0].kind {
            assert!(import.is_type_only, "is_type_only should be true");
            assert_eq!(import.names[0].name, "User");
        } else {
            panic!("Expected import");
        }
    }

    #[test]
    fn test_parser_regular_import_not_type_only() {
        let source = "import { Post } from \"./models\";";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty());
        if let ItemKind::Import(import) = &module.items[0].kind {
            assert!(
                !import.is_type_only,
                "is_type_only should be false for regular import"
            );
        } else {
            panic!("Expected import");
        }
    }

    #[test]
    fn test_parser_abstract_class() {
        let source = "\
abstract class Shape {
  abstract area(): f64;
}";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "no diagnostics: {diagnostics:?}");
        if let ItemKind::Class(cls) = &module.items[0].kind {
            assert!(cls.is_abstract, "class should be abstract");
            assert_eq!(cls.name.name, "Shape");
            assert_eq!(cls.members.len(), 1);
            if let ClassMember::Method(m) = &cls.members[0] {
                assert!(m.is_abstract, "method should be abstract");
                assert_eq!(m.name.name, "area");
            } else {
                panic!("Expected method member");
            }
        } else {
            panic!("Expected class");
        }
    }

    #[test]
    fn test_parser_class_extends() {
        let source = "\
class Circle extends Shape {
  radius: f64;
}";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "no diagnostics: {diagnostics:?}");
        if let ItemKind::Class(cls) = &module.items[0].kind {
            assert_eq!(
                cls.extends.as_ref().map(|e| &e.name),
                Some(&"Shape".to_owned())
            );
        } else {
            panic!("Expected class");
        }
    }

    #[test]
    fn test_parser_override_method() {
        let source = "\
class Impl {
  override greet(): string {
    return \"hello\";
  }
}";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "no diagnostics: {diagnostics:?}");
        if let ItemKind::Class(cls) = &module.items[0].kind {
            if let ClassMember::Method(m) = &cls.members[0] {
                assert!(m.is_override, "method should be marked override");
            } else {
                panic!("Expected method");
            }
        } else {
            panic!("Expected class");
        }
    }

    #[test]
    fn test_parser_hash_private_field() {
        let source = "\
class User {
  #password: string;
}";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "no diagnostics: {diagnostics:?}");
        if let ItemKind::Class(cls) = &module.items[0].kind {
            if let ClassMember::Field(f) = &cls.members[0] {
                assert!(f.is_hash_private, "field should be hash-private");
                assert_eq!(
                    f.name.name, "password",
                    "# should be stripped from field name"
                );
            } else {
                panic!("Expected field");
            }
        } else {
            panic!("Expected class");
        }
    }

    #[test]
    fn test_parser_satisfies_expression() {
        let source = "\
function main() {
  const x: i64 = 42 satisfies i64;
}";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "no diagnostics: {diagnostics:?}");
    }

    #[test]
    fn test_parser_export_abstract_class() {
        let source = "\
export abstract class Shape {
  abstract area(): f64;
}";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "no diagnostics: {diagnostics:?}");
        assert!(module.items[0].exported, "should be exported");
        if let ItemKind::Class(cls) = &module.items[0].kind {
            assert!(cls.is_abstract);
        } else {
            panic!("Expected class");
        }
    }

    // ---- Trailing semicolon on type/enum definitions ----

    #[test]
    fn test_parser_simple_enum_with_semicolon() {
        let source = r#"type Role = "admin" | "user" | "guest";"#;
        let module = parse_ok(source);
        let ed = first_enum(&module);
        assert_eq!(ed.name.name, "Role");
        assert_eq!(ed.variants.len(), 3);
    }

    #[test]
    fn test_parser_simple_enum_without_semicolon() {
        let source = r#"type Role = "admin" | "user" | "guest""#;
        let module = parse_ok(source);
        let ed = first_enum(&module);
        assert_eq!(ed.name.name, "Role");
        assert_eq!(ed.variants.len(), 3);
    }

    #[test]
    fn test_parser_data_enum_with_semicolon() {
        let source = r#"
type Shape =
  | { kind: "circle", radius: f64 }
  | { kind: "rect", width: f64 };
"#;
        let module = parse_ok(source);
        let ed = first_enum(&module);
        assert_eq!(ed.name.name, "Shape");
        assert_eq!(ed.variants.len(), 2);
    }

    #[test]
    fn test_parser_struct_type_with_semicolon() {
        let source = "type User = { name: string, age: u32 };";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "User");
                assert_eq!(td.fields.len(), 2);
            }
            _ => panic!("expected TypeDef"),
        }
    }

    // ---- derives keyword tests ----

    #[test]
    fn test_parser_type_def_derives_single() {
        let source = "type Foo = { x: i32 } derives Serialize";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "Foo");
                assert_eq!(td.fields.len(), 1);
                assert_eq!(td.derives.len(), 1);
                assert_eq!(td.derives[0].name, "Serialize");
            }
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn test_parser_type_def_derives_multiple() {
        let source = "type Foo = { x: i32 } derives Serialize, Deserialize";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "Foo");
                assert_eq!(td.derives.len(), 2);
                assert_eq!(td.derives[0].name, "Serialize");
                assert_eq!(td.derives[1].name, "Deserialize");
            }
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn test_parser_type_def_no_derives() {
        let source = "type Foo = { x: i32 }";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert!(td.derives.is_empty());
            }
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn test_parser_simple_enum_derives() {
        let source = r#"type Dir = "n" | "s" derives Clone, Serialize"#;
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::EnumDef(ed) => {
                assert_eq!(ed.name.name, "Dir");
                assert_eq!(ed.variants.len(), 2);
                assert_eq!(ed.derives.len(), 2);
                assert_eq!(ed.derives[0].name, "Clone");
                assert_eq!(ed.derives[1].name, "Serialize");
            }
            _ => panic!("expected EnumDef"),
        }
    }

    #[test]
    fn test_parser_data_enum_derives() {
        let source = r#"type Shape = | { kind: "circle", radius: f64 } | { kind: "rect", width: f64 } derives Serialize"#;
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::EnumDef(ed) => {
                assert_eq!(ed.name.name, "Shape");
                assert_eq!(ed.variants.len(), 2);
                assert_eq!(ed.derives.len(), 1);
                assert_eq!(ed.derives[0].name, "Serialize");
            }
            _ => panic!("expected EnumDef"),
        }
    }

    #[test]
    fn test_parser_class_derives() {
        let source = "class Foo derives Debug { name: string; }";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Class(cls) => {
                assert_eq!(cls.name.name, "Foo");
                assert_eq!(cls.derives.len(), 1);
                assert_eq!(cls.derives[0].name, "Debug");
                assert!(cls.implements.is_empty());
            }
            _ => panic!("expected ClassDef"),
        }
    }

    #[test]
    fn test_parser_class_implements_and_derives() {
        let source =
            "class Foo implements Displayable, derives Serialize, Deserialize { name: string; }";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Class(cls) => {
                assert_eq!(cls.name.name, "Foo");
                assert_eq!(cls.implements.len(), 1);
                assert_eq!(cls.implements[0].name, "Displayable");
                assert_eq!(cls.derives.len(), 2);
                assert_eq!(cls.derives[0].name, "Serialize");
                assert_eq!(cls.derives[1].name, "Deserialize");
            }
            _ => panic!("expected ClassDef"),
        }
    }

    #[test]
    fn test_parser_type_def_derives_with_semicolon() {
        let source = "type Foo = { x: i32 } derives Serialize;";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.derives.len(), 1);
                assert_eq!(td.derives[0].name, "Serialize");
            }
            _ => panic!("expected TypeDef"),
        }
    }

    // ---------------------------------------------------------------
    // Index signature parsing
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_pure_index_signature_type_def() {
        let source = "type Config = { [key: string]: string }";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "Config");
                assert!(td.fields.is_empty(), "expected no regular fields");
                let sig = td
                    .index_signature
                    .as_ref()
                    .expect("expected index signature");
                assert_eq!(sig.key_name.name, "key");
                assert!(
                    matches!(sig.key_type.kind, TypeKind::Named(ref ident) if ident.name == "string")
                );
                assert!(
                    matches!(sig.value_type.kind, TypeKind::Named(ref ident) if ident.name == "string")
                );
            }
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn test_parser_index_signature_with_numeric_key() {
        let source = "type Scores = { [id: i32]: string }";
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "Scores");
                let sig = td
                    .index_signature
                    .as_ref()
                    .expect("expected index signature");
                assert_eq!(sig.key_name.name, "id");
                assert!(
                    matches!(sig.key_type.kind, TypeKind::Named(ref ident) if ident.name == "i32")
                );
                assert!(
                    matches!(sig.value_type.kind, TypeKind::Named(ref ident) if ident.name == "string")
                );
            }
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn test_parser_inline_index_signature_type_annotation() {
        let source = r#"
            function foo(config: { [key: string]: i32 }): void {
            }
        "#;
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Function(f) => {
                assert!(!f.params.is_empty(), "expected at least one param");
                let param_type = &f.params[0].type_ann;
                assert!(
                    matches!(param_type.kind, TypeKind::IndexSignature(_)),
                    "expected IndexSignature type, got {:?}",
                    param_type.kind
                );
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn test_parser_index_assign_expression() {
        let source = r#"
            function main() {
                let config: { [key: string]: string } = {};
                config["debug"] = "true";
            }
        "#;
        let module = parse_ok(source);
        match &module.items[0].kind {
            ItemKind::Function(f) => {
                // First statement: let config = {}
                assert!(matches!(f.body.stmts[0], Stmt::VarDecl(_)));
                // Second statement: config["debug"] = "true" (IndexAssign)
                if let Stmt::Expr(ref expr) = f.body.stmts[1] {
                    assert!(
                        matches!(expr.kind, ExprKind::IndexAssign(_)),
                        "expected IndexAssign, got {:?}",
                        expr.kind
                    );
                } else {
                    panic!("expected Expr statement, got {:?}", f.body.stmts[1]);
                }
            }
            _ => panic!("expected Function"),
        }
    }

    // ---- keyof and typeof type operators ----

    #[test]
    fn test_parser_keyof_type_alias() {
        let module = parse_ok("type User = { name: string, age: u32 }\ntype UserKey = keyof User");
        assert_eq!(module.items.len(), 2);
        match &module.items[1].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "UserKey");
                let alias = td.type_alias.as_ref().expect("expected type_alias");
                match &alias.kind {
                    TypeKind::KeyOf(inner) => {
                        if let TypeKind::Named(ident) = &inner.kind {
                            assert_eq!(ident.name, "User");
                        } else {
                            panic!("expected Named inside KeyOf, got: {:?}", inner.kind);
                        }
                    }
                    _ => panic!("expected KeyOf, got: {:?}", alias.kind),
                }
            }
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn test_parser_keyof_in_param_type() {
        let module = parse_ok(
            "type User = { name: string }\nfunction f(key: keyof User): string { return key; }",
        );
        let f = match &module.items[1].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected Function"),
        };
        match &f.params[0].type_ann.kind {
            TypeKind::KeyOf(inner) => {
                if let TypeKind::Named(ident) = &inner.kind {
                    assert_eq!(ident.name, "User");
                } else {
                    panic!("expected Named inside KeyOf");
                }
            }
            _ => panic!("expected KeyOf, got: {:?}", f.params[0].type_ann.kind),
        }
    }

    #[test]
    fn test_parser_typeof_type_alias() {
        let module = parse_ok("function f() { const x: i32 = 42; }\ntype Config = typeof config");
        assert_eq!(module.items.len(), 2);
        match &module.items[1].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "Config");
                let alias = td.type_alias.as_ref().expect("expected type_alias");
                match &alias.kind {
                    TypeKind::TypeOf(ident) => {
                        assert_eq!(ident.name, "config");
                    }
                    _ => panic!("expected TypeOf, got: {:?}", alias.kind),
                }
            }
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn test_parser_typeof_in_param_type() {
        let module = parse_ok("function f(c: typeof config): void { }");
        let f = match &module.items[0].kind {
            ItemKind::Function(f) => f,
            _ => panic!("expected Function"),
        };
        match &f.params[0].type_ann.kind {
            TypeKind::TypeOf(ident) => {
                assert_eq!(ident.name, "config");
            }
            _ => panic!("expected TypeOf, got: {:?}", f.params[0].type_ann.kind),
        }
    }

    // ---- Conditional types and infer ----

    #[test]
    fn test_parser_conditional_type_simple() {
        let source = "type X = string extends string ? bool : i32";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::Conditional {
                check_type,
                extends_type,
                true_type,
                false_type,
            } => {
                assert!(matches!(&check_type.kind, TypeKind::Named(i) if i.name == "string"));
                assert!(matches!(&extends_type.kind, TypeKind::Named(i) if i.name == "string"));
                assert!(matches!(&true_type.kind, TypeKind::Named(i) if i.name == "bool"));
                assert!(matches!(&false_type.kind, TypeKind::Named(i) if i.name == "i32"));
            }
            other => panic!("expected Conditional, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_conditional_type_with_infer() {
        let source = "type X = (i32) => bool extends (i32) => infer R ? R : string";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::Conditional {
                check_type,
                extends_type,
                true_type,
                false_type,
            } => {
                // Check type should be a function type
                assert!(matches!(&check_type.kind, TypeKind::Function(_, _)));
                // Extends type should be a function type with infer in return position
                match &extends_type.kind {
                    TypeKind::Function(_, ret) => {
                        assert!(matches!(&ret.kind, TypeKind::Infer(i) if i.name == "R"));
                    }
                    other => panic!("expected Function extends type, got: {other:?}"),
                }
                // True type references the inferred variable
                assert!(matches!(&true_type.kind, TypeKind::Named(i) if i.name == "R"));
                // False type
                assert!(matches!(&false_type.kind, TypeKind::Named(i) if i.name == "string"));
            }
            other => panic!("expected Conditional, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_infer_keyword_in_type_position() {
        let source = "type X = i32 extends infer T ? T : string";
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::Conditional { extends_type, .. } => {
                assert!(matches!(&extends_type.kind, TypeKind::Infer(i) if i.name == "T"));
            }
            other => panic!("expected Conditional, got: {other:?}"),
        }
    }

    // ---- Decorator parsing tests ----

    #[test]
    fn test_parser_decorator_simple_name() {
        let module = parse_ok("@test\nfunction foo() {}");
        assert_eq!(module.items.len(), 1);
        assert_eq!(module.items[0].decorators.len(), 1);
        assert_eq!(module.items[0].decorators[0].name, "test");
        assert!(module.items[0].decorators[0].args.is_none());
    }

    #[test]
    fn test_parser_decorator_with_args() {
        let module = parse_ok("@derive(Clone, Debug)\ntype X = { x: i32 }");
        assert_eq!(module.items.len(), 1);
        assert_eq!(module.items[0].decorators.len(), 1);
        assert_eq!(module.items[0].decorators[0].name, "derive");
        assert_eq!(
            module.items[0].decorators[0].args.as_deref(),
            Some("Clone, Debug")
        );
    }

    #[test]
    fn test_parser_multiple_decorators() {
        let module = parse_ok("@inline\n@must_use\nfunction bar(): i32 { return 0; }");
        assert_eq!(module.items.len(), 1);
        assert_eq!(module.items[0].decorators.len(), 2);
        assert_eq!(module.items[0].decorators[0].name, "inline");
        assert_eq!(module.items[0].decorators[1].name, "must_use");
    }

    #[test]
    fn test_parser_decorator_on_exported_item() {
        let module = parse_ok("@test\nexport function foo() {}");
        assert_eq!(module.items.len(), 1);
        assert_eq!(module.items[0].decorators.len(), 1);
        assert_eq!(module.items[0].decorators[0].name, "test");
        assert!(module.items[0].exported);
    }

    #[test]
    fn test_parser_decorator_on_type_def() {
        let module = parse_ok("@derive(Serialize)\ntype Config = { host: string }");
        assert_eq!(module.items.len(), 1);
        assert_eq!(module.items[0].decorators.len(), 1);
        assert_eq!(module.items[0].decorators[0].name, "derive");
        assert!(matches!(module.items[0].kind, ItemKind::TypeDef(_)));
    }

    #[test]
    fn test_parser_decorator_on_enum() {
        let module = parse_ok("@derive(Clone)\ntype Dir = \"n\" | \"s\"");
        assert_eq!(module.items.len(), 1);
        assert_eq!(module.items[0].decorators.len(), 1);
        assert!(matches!(module.items[0].kind, ItemKind::EnumDef(_)));
    }

    #[test]
    fn test_parser_decorator_on_class() {
        let module =
            parse_ok("@derive(Debug)\nclass Foo { x: i32; constructor(x: i32) { this.x = x; } }");
        assert_eq!(module.items.len(), 1);
        assert_eq!(module.items[0].decorators.len(), 1);
        assert!(matches!(module.items[0].kind, ItemKind::Class(_)));
    }

    #[test]
    fn test_parser_no_decorators() {
        let module = parse_ok("function foo() {}");
        assert_eq!(module.items.len(), 1);
        assert!(module.items[0].decorators.is_empty());
    }

    // ---------------------------------------------------------------
    // Variadic tuple spread parsing tests
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_tuple_spread_single_element() {
        let module = parse_ok("type X = [...Pair, bool]");
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::Tuple(types) => {
                assert_eq!(types.len(), 2);
                match &types[0].kind {
                    TypeKind::TupleSpread(inner) => {
                        assert!(matches!(&inner.kind, TypeKind::Named(i) if i.name == "Pair"));
                    }
                    other => panic!("expected TupleSpread, got: {other:?}"),
                }
                assert!(matches!(&types[1].kind, TypeKind::Named(i) if i.name == "bool"));
            }
            other => panic!("expected Tuple, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_tuple_spread_prepend() {
        let module = parse_ok("type X = [i32, ...T]");
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::Tuple(types) => {
                assert_eq!(types.len(), 2);
                assert!(matches!(&types[0].kind, TypeKind::Named(i) if i.name == "i32"));
                match &types[1].kind {
                    TypeKind::TupleSpread(inner) => {
                        assert!(matches!(&inner.kind, TypeKind::Named(i) if i.name == "T"));
                    }
                    other => panic!("expected TupleSpread, got: {other:?}"),
                }
            }
            other => panic!("expected Tuple, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_tuple_spread_multiple_spreads() {
        let module = parse_ok("type X = [...A, ...B]");
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::Tuple(types) => {
                assert_eq!(types.len(), 2);
                assert!(matches!(&types[0].kind, TypeKind::TupleSpread(_)));
                assert!(matches!(&types[1].kind, TypeKind::TupleSpread(_)));
            }
            other => panic!("expected Tuple, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_tuple_spread_in_middle() {
        let module = parse_ok("type X = [bool, ...Middle, f64]");
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::Tuple(types) => {
                assert_eq!(types.len(), 3);
                assert!(matches!(&types[0].kind, TypeKind::Named(i) if i.name == "bool"));
                assert!(matches!(&types[1].kind, TypeKind::TupleSpread(_)));
                assert!(matches!(&types[2].kind, TypeKind::Named(i) if i.name == "f64"));
            }
            other => panic!("expected Tuple, got: {other:?}"),
        }
    }

    // T110-1: Parse `for (const k in obj) { ... }` → ForInStmt
    #[test]
    fn test_parser_for_in_const_produces_for_in_stmt() {
        let source = r#"function main() {
  for (const k in obj) {
    console.log(k);
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForIn(for_in) => {
                assert_eq!(for_in.binding, VarBinding::Const);
                assert_eq!(for_in.variable.name, "k");
                match &for_in.iterable.kind {
                    ExprKind::Ident(ident) => assert_eq!(ident.name, "obj"),
                    other => panic!("expected Ident, got: {other:?}"),
                }
                assert!(!for_in.body.stmts.is_empty());
            }
            other => panic!("expected ForIn, got: {other:?}"),
        }
    }

    // T110-2: Parse `for (let k in obj) { ... }` → ForInStmt with Let binding
    #[test]
    fn test_parser_for_in_let_produces_for_in_stmt() {
        let source = r#"function main() {
  for (let k in obj) {
    console.log(k);
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForIn(for_in) => {
                assert_eq!(for_in.binding, VarBinding::Let);
                assert_eq!(for_in.variable.name, "k");
            }
            other => panic!("expected ForIn, got: {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // T118: Type guard return type (`x is Type`)
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_type_guard_return_type() {
        let source = r#"function isString(x: string | i32): x is string {
  return typeof x === "string";
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "isString");
        let ret = f.return_type.as_ref().expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected type annotation");
        match &type_ann.kind {
            TypeKind::TypeGuard {
                param,
                guarded_type,
            } => {
                assert_eq!(param.name, "x");
                match &guarded_type.kind {
                    TypeKind::Named(ident) => assert_eq!(ident.name, "string"),
                    other => panic!("expected Named(string), got {other:?}"),
                }
            }
            other => panic!("expected TypeGuard, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_type_guard_preserves_param_name() {
        let source = r#"function isNumber(value: string | i32): value is i32 {
  return true;
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let ret = f.return_type.as_ref().expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected type annotation");
        match &type_ann.kind {
            TypeKind::TypeGuard {
                param,
                guarded_type,
            } => {
                assert_eq!(param.name, "value");
                // Verify the param name matches one of the function parameters
                assert!(
                    f.params.iter().any(|p| p.name.name == param.name),
                    "type guard param should match a function parameter"
                );
                match &guarded_type.kind {
                    TypeKind::Named(ident) => assert_eq!(ident.name, "i32"),
                    other => panic!("expected Named(i32), got {other:?}"),
                }
            }
            other => panic!("expected TypeGuard, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_type_guard_complex_type() {
        let source = r#"function isArray(x: Array<string> | i32): x is Array<string> {
  return true;
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let ret = f.return_type.as_ref().expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected type annotation");
        match &type_ann.kind {
            TypeKind::TypeGuard {
                param,
                guarded_type,
            } => {
                assert_eq!(param.name, "x");
                match &guarded_type.kind {
                    TypeKind::Generic(ident, args) => {
                        assert_eq!(ident.name, "Array");
                        assert_eq!(args.len(), 1);
                        match &args[0].kind {
                            TypeKind::Named(inner) => assert_eq!(inner.name, "string"),
                            other => panic!("expected Named(string), got {other:?}"),
                        }
                    }
                    other => panic!("expected Generic(Array<string>), got {other:?}"),
                }
            }
            other => panic!("expected TypeGuard, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_is_not_keyword_in_non_return_position() {
        // `is` should not be reserved — it should be a valid variable name
        let source = r#"function main() {
  const is: i32 = 5;
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(v) => assert_eq!(v.name.name, "is"),
            other => panic!("expected VarDecl, got {other:?}"),
        }
    }

    // T110-3: for-in and for-of parse to different AST nodes
    #[test]
    fn test_parser_for_in_vs_for_of_distinct_ast_nodes() {
        let source_in = r#"function main() {
  for (const k in obj) { console.log(k); }
}"#;
        let source_of = r#"function main() {
  for (const k of items) { console.log(k); }
}"#;

        let module_in = parse_ok(source_in);
        let module_of = parse_ok(source_of);

        let stmt_in = first_stmt(first_fn(&module_in));
        let stmt_of = first_stmt(first_fn(&module_of));

        assert!(
            matches!(stmt_in, Stmt::ForIn(_)),
            "for-in should produce ForIn"
        );
        assert!(matches!(stmt_of, Stmt::For(_)), "for-of should produce For");
    }

    // ---------------------------------------------------------------
    // Task 116: `never` type support
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_never_return_type_produces_never_kind() {
        let source = r#"function fail(): never { throw new Error("fail"); }"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "fail");
        let ret = f.return_type.as_ref().expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected type annotation");
        assert!(
            matches!(type_ann.kind, TypeKind::Never),
            "expected TypeKind::Never, got {:?}",
            type_ann.kind
        );
    }

    #[test]
    fn test_parser_never_param_type_produces_never_kind() {
        let source = "function f(x: never): void { }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.params.len(), 1);
        assert!(
            matches!(f.params[0].type_ann.kind, TypeKind::Never),
            "expected TypeKind::Never param, got {:?}",
            f.params[0].type_ann.kind
        );
    }

    #[test]
    fn test_parser_never_in_union_produces_union_with_never() {
        let source = "type X = string | never;";
        let (module, diagnostics) = parse_source(source);
        assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");
        let item = &module.items[0];
        if let ItemKind::TypeDef(td) = &item.kind {
            if let TypeKind::Union(members) = &td.type_alias.as_ref().unwrap().kind {
                assert_eq!(members.len(), 2);
                assert!(matches!(members[0].kind, TypeKind::Named(_)));
                assert!(
                    matches!(members[1].kind, TypeKind::Never),
                    "expected TypeKind::Never in union, got {:?}",
                    members[1].kind
                );
            } else {
                panic!("expected Union type alias");
            }
        } else {
            panic!("expected TypeDef item");
        }
    }

    // ---------------------------------------------------------------------------
    // Task 117: `unknown` type support
    // ---------------------------------------------------------------------------

    #[test]
    fn test_parser_unknown_param_type() {
        let source = "function f(x: unknown): void {}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.params.len(), 1);
        assert!(
            matches!(f.params[0].type_ann.kind, TypeKind::Unknown),
            "expected TypeKind::Unknown for param type"
        );
    }

    #[test]
    fn test_parser_unknown_return_type() {
        let source = "function f(): unknown { }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let ret = f.return_type.as_ref().expect("expected return type");
        let type_ann = ret
            .type_ann
            .as_ref()
            .expect("expected return type annotation");
        assert!(
            matches!(type_ann.kind, TypeKind::Unknown),
            "expected TypeKind::Unknown for return type"
        );
    }

    #[test]
    fn test_parser_unknown_in_union() {
        let source = "function f(x: string | unknown): void {}";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let param_type = &f.params[0].type_ann.kind;
        match param_type {
            TypeKind::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(matches!(members[0].kind, TypeKind::Named(_)));
                assert!(
                    matches!(members[1].kind, TypeKind::Unknown),
                    "expected TypeKind::Unknown in union"
                );
            }
            other => panic!("expected Union type, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_unknown_variable() {
        let source = "function main() { const x: unknown = 42; }";
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(v) => {
                let type_ann = v.type_ann.as_ref().expect("expected type annotation");
                assert!(
                    matches!(type_ann.kind, TypeKind::Unknown),
                    "expected TypeKind::Unknown for variable type"
                );
            }
            other => panic!("expected VarDecl, got {other:?}"),
        }
    }

    // ---- Task 119: `as const` assertion ----

    // T119-1: `as const` on an array literal produces AsConst wrapping ArrayLit
    #[test]
    fn test_parser_as_const_array() {
        let module = parse_ok("function f() { const x = [1, 2, 3] as const; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::AsConst(inner) = &decl.init.kind {
                assert!(
                    matches!(&inner.kind, ExprKind::ArrayLit(_)),
                    "expected ArrayLit inside AsConst, got: {:?}",
                    inner.kind
                );
            } else {
                panic!("expected AsConst, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    // T119-2: `as const` on an object literal produces AsConst wrapping StructLit
    #[test]
    fn test_parser_as_const_object() {
        let source = r#"function f() { const x = { a: 1 } as const; }"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::AsConst(inner) = &decl.init.kind {
                assert!(
                    matches!(&inner.kind, ExprKind::StructLit(_)),
                    "expected StructLit inside AsConst, got: {:?}",
                    inner.kind
                );
            } else {
                panic!("expected AsConst, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    // T119-3: `as const` on a literal produces AsConst wrapping the literal
    #[test]
    fn test_parser_as_const_literal() {
        let module = parse_ok("function f() { const x = 42 as const; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            if let ExprKind::AsConst(inner) = &decl.init.kind {
                assert!(
                    matches!(&inner.kind, ExprKind::IntLit(42)),
                    "expected IntLit(42) inside AsConst, got: {:?}",
                    inner.kind
                );
            } else {
                panic!("expected AsConst, got: {:?}", decl.init.kind);
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    // T119-4: `as Type` still works and is not confused with `as const`
    #[test]
    fn test_parser_as_type_still_works() {
        let module = parse_ok("function f() { const x: f64 = y as f64; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            assert!(
                matches!(&decl.init.kind, ExprKind::Cast(_, _)),
                "expected Cast for `as f64`, got: {:?}",
                decl.init.kind
            );
        } else {
            panic!("expected VarDecl");
        }
    }

    // ========================================================================
    // Task 120: readonly type modifier
    // ========================================================================

    #[test]
    fn test_parser_readonly_array_type() {
        // `readonly Array<string>` should parse as Readonly wrapping a generic Array type.
        let module = parse_ok("function process(data: readonly Array<string>): void {}");
        let f = first_fn(&module);
        assert_eq!(f.params.len(), 1);
        match &f.params[0].type_ann.kind {
            TypeKind::Readonly(inner) => match &inner.kind {
                TypeKind::Generic(ident, args) => {
                    assert_eq!(ident.name, "Array");
                    assert_eq!(args.len(), 1);
                    assert!(matches!(&args[0].kind, TypeKind::Named(id) if id.name == "string"));
                }
                other => panic!("expected Generic inside Readonly, got: {other:?}"),
            },
            other => panic!("expected TypeKind::Readonly, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_readonly_tuple_type() {
        // `readonly [string, i32]` should parse as Readonly wrapping a tuple type.
        let module =
            parse_ok("function main() { const pair: readonly [string, i32] = [\"hello\", 42]; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            let type_ann = decl.type_ann.as_ref().expect("expected type annotation");
            match &type_ann.kind {
                TypeKind::Readonly(inner) => match &inner.kind {
                    TypeKind::Tuple(types) => {
                        assert_eq!(types.len(), 2);
                        assert!(
                            matches!(&types[0].kind, TypeKind::Named(id) if id.name == "string")
                        );
                        assert!(matches!(&types[1].kind, TypeKind::Named(id) if id.name == "i32"));
                    }
                    other => panic!("expected Tuple inside Readonly, got: {other:?}"),
                },
                other => panic!("expected TypeKind::Readonly, got: {other:?}"),
            }
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn test_parser_readonly_array_generic_named() {
        // `ReadonlyArray<string>` should parse as a Generic named type (not Readonly wrapper).
        let module = parse_ok("function process(data: ReadonlyArray<string>): void {}");
        let f = first_fn(&module);
        assert_eq!(f.params.len(), 1);
        match &f.params[0].type_ann.kind {
            TypeKind::Generic(ident, args) => {
                assert_eq!(ident.name, "ReadonlyArray");
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0].kind, TypeKind::Named(id) if id.name == "string"));
            }
            other => panic!("expected TypeKind::Generic, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_readonly_does_not_conflict_with_class_field() {
        // `readonly` as a class field modifier should still work alongside
        // `readonly` as a type modifier.
        let module = parse_ok(
            "class Config {
                readonly items: readonly Array<string>;
                constructor() {}
            }",
        );
        let cls = first_class(&module);
        let field = cls.members.iter().find_map(|m| match m {
            ClassMember::Field(f) => Some(f),
            _ => None,
        });
        let field = field.expect("should have field");
        assert!(field.readonly, "field should be marked readonly");
        assert_eq!(field.name.name, "items");
        // The type annotation should be Readonly wrapping a Generic Array
        match &field.type_ann.kind {
            TypeKind::Readonly(inner) => match &inner.kind {
                TypeKind::Generic(ident, _) => {
                    assert_eq!(ident.name, "Array");
                }
                other => panic!("expected Generic inside Readonly, got: {other:?}"),
            },
            other => panic!("expected TypeKind::Readonly for field type, got: {other:?}"),
        }
    }

    // ===================================================================
    // Task 121: Computed property name tests
    // ===================================================================

    #[test]
    fn test_parser_computed_property_name() {
        let source = r#"function main() { const obj = { [key]: "value" }; }"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(decl) => match &decl.init.kind {
                ExprKind::StructLit(slit) => {
                    assert_eq!(slit.fields.len(), 1);
                    let field = &slit.fields[0];
                    assert!(
                        field.computed_key.is_some(),
                        "field should have computed_key"
                    );
                    let key = field.computed_key.as_ref().unwrap();
                    match &key.kind {
                        ExprKind::Ident(ident) => assert_eq!(ident.name, "key"),
                        other => panic!("expected Ident key, got: {other:?}"),
                    }
                }
                other => panic!("expected StructLit, got: {other:?}"),
            },
            other => panic!("expected VarDecl, got: {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 122: Static blocks in classes
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_static_block_empty() {
        let module = parse_ok(
            "class Foo {
                static { }
            }",
        );
        let cls = first_class(&module);
        assert_eq!(cls.members.len(), 1);
        match &cls.members[0] {
            ClassMember::StaticBlock(block) => {
                assert!(block.stmts.is_empty(), "empty static block");
            }
            other => panic!("expected StaticBlock, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_computed_property_string_expr() {
        let source = r#"function main() { const obj = { ["hello"]: 1 }; }"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(decl) => match &decl.init.kind {
                ExprKind::StructLit(slit) => {
                    assert_eq!(slit.fields.len(), 1);
                    let field = &slit.fields[0];
                    assert!(
                        field.computed_key.is_some(),
                        "field should have computed_key"
                    );
                    let key = field.computed_key.as_ref().unwrap();
                    match &key.kind {
                        ExprKind::StringLit(s) => assert_eq!(s, "hello"),
                        other => panic!("expected StringLit key, got: {other:?}"),
                    }
                }
                other => panic!("expected StructLit, got: {other:?}"),
            },
            other => panic!("expected VarDecl, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_static_block_with_statements() {
        let module = parse_ok(
            "class Foo {
                static {
                    const x: i32 = 1;
                }
            }",
        );
        let cls = first_class(&module);
        assert_eq!(cls.members.len(), 1);
        match &cls.members[0] {
            ClassMember::StaticBlock(block) => {
                assert_eq!(block.stmts.len(), 1, "should have one statement");
            }
            other => panic!("expected StaticBlock, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_mixed_static_computed_properties() {
        let source = r#"function main() { const obj = { a: 1, [b]: 2 }; }"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(decl) => match &decl.init.kind {
                ExprKind::StructLit(slit) => {
                    assert_eq!(slit.fields.len(), 2);
                    // First field is static
                    assert!(
                        slit.fields[0].computed_key.is_none(),
                        "first field should be static"
                    );
                    assert_eq!(slit.fields[0].name.name, "a");
                    // Second field is computed
                    assert!(
                        slit.fields[1].computed_key.is_some(),
                        "second field should be computed"
                    );
                }
                other => panic!("expected StructLit, got: {other:?}"),
            },
            other => panic!("expected VarDecl, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_computed_property_complex_expr() {
        let source = r#"function main() { const obj = { [a + b]: "value" }; }"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(decl) => match &decl.init.kind {
                ExprKind::StructLit(slit) => {
                    assert_eq!(slit.fields.len(), 1);
                    let field = &slit.fields[0];
                    assert!(
                        field.computed_key.is_some(),
                        "field should have computed_key"
                    );
                    let key = field.computed_key.as_ref().unwrap();
                    assert!(
                        matches!(&key.kind, ExprKind::Binary(_)),
                        "expected Binary expr key, got: {:?}",
                        key.kind
                    );
                }
                other => panic!("expected StructLit, got: {other:?}"),
            },
            other => panic!("expected VarDecl, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_static_block_mixed_with_methods() {
        let module = parse_ok(
            "class Foo {
                value: i32;
                constructor() {}

                static {
                    const x: i32 = 42;
                }

                greet(): void {}
            }",
        );
        let cls = first_class(&module);
        // field + constructor + static block + method = 4
        assert_eq!(cls.members.len(), 4);
        let has_static_block = cls
            .members
            .iter()
            .any(|m| matches!(m, ClassMember::StaticBlock(_)));
        assert!(has_static_block, "should contain a static block");
        let has_method = cls
            .members
            .iter()
            .any(|m| matches!(m, ClassMember::Method(m) if m.name.name == "greet"));
        assert!(has_method, "should still have greet method");
    }

    #[test]
    fn test_parser_static_keyword_method_not_confused_with_static_block() {
        // `static greet(): void {}` should parse as a static method, not a static block
        let module = parse_ok(
            "class Foo {
                static greet(): void {}
            }",
        );
        let cls = first_class(&module);
        assert_eq!(cls.members.len(), 1);
        match &cls.members[0] {
            ClassMember::Method(m) => {
                assert!(m.is_static, "method should be static");
                assert_eq!(m.name.name, "greet");
            }
            other => panic!("expected static Method, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // super expressions
    // -----------------------------------------------------------------------

    #[test]
    fn test_parser_super_method_call_produces_method_call_on_super() {
        let source = "\
class Animal {
  speak(): string { return \"hello\"; }
}
class Dog extends Animal {
  speak(): string { return super.speak(); }
}";
        let module = parse_ok(source);
        // Find Dog's speak method body
        let dog = &module.items[1];
        let cls = match &dog.kind {
            ItemKind::Class(c) => c,
            other => panic!("expected class, got {other:?}"),
        };
        let method = cls.members.iter().find_map(|m| match m {
            ClassMember::Method(m) if m.name.name == "speak" => Some(m),
            _ => None,
        });
        let method = method.expect("should have speak method");
        // The return statement should have a method call on super
        let ret_stmt = &method.body.stmts[0];
        if let Stmt::Return(ret) = ret_stmt {
            let expr = ret.value.as_ref().expect("should have return value");
            match &expr.kind {
                ExprKind::MethodCall(mc) => {
                    assert!(
                        matches!(mc.object.kind, ExprKind::Super),
                        "receiver should be Super, got {:?}",
                        mc.object.kind
                    );
                    assert_eq!(mc.method.name, "speak");
                    assert!(mc.args.is_empty());
                }
                other => panic!("expected MethodCall, got {other:?}"),
            }
        } else {
            panic!("expected Return statement, got {ret_stmt:?}");
        }
    }

    #[test]
    fn test_parser_super_constructor_call_produces_method_call_on_super() {
        let source = "\
class Animal {
  name: string;
  constructor(name: string) { this.name = name; }
}
class Dog extends Animal {
  constructor(name: string) { super(name); }
}";
        let module = parse_ok(source);
        let dog = &module.items[1];
        let cls = match &dog.kind {
            ItemKind::Class(c) => c,
            other => panic!("expected class, got {other:?}"),
        };
        let ctor = cls.members.iter().find_map(|m| match m {
            ClassMember::Constructor(c) => Some(c),
            _ => None,
        });
        let ctor = ctor.expect("should have constructor");
        // The body should contain `super(name)` which parses as MethodCall(Super, "new", args)
        let stmt = &ctor.body.stmts[0];
        if let Stmt::Expr(expr) = stmt {
            match &expr.kind {
                ExprKind::MethodCall(mc) => {
                    assert!(
                        matches!(mc.object.kind, ExprKind::Super),
                        "object should be Super, got {:?}",
                        mc.object.kind
                    );
                    assert_eq!(mc.method.name, "new");
                    assert_eq!(mc.args.len(), 1);
                }
                other => panic!("expected MethodCall, got {other:?}"),
            }
        } else {
            panic!("expected Expr statement, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_super_method_with_args_parses_arguments() {
        let source = "\
class Base {
  greet(a: string, b: i32): string { return a; }
}
class Child extends Base {
  greet(a: string, b: i32): string { return super.greet(a, b); }
}";
        let module = parse_ok(source);
        let child = &module.items[1];
        let cls = match &child.kind {
            ItemKind::Class(c) => c,
            other => panic!("expected class, got {other:?}"),
        };
        let method = cls.members.iter().find_map(|m| match m {
            ClassMember::Method(m) if m.name.name == "greet" => Some(m),
            _ => None,
        });
        let method = method.expect("should have greet method");
        let ret_stmt = &method.body.stmts[0];
        if let Stmt::Return(ret) = ret_stmt {
            let expr = ret.value.as_ref().expect("return value");
            match &expr.kind {
                ExprKind::MethodCall(mc) => {
                    assert!(matches!(mc.object.kind, ExprKind::Super));
                    assert_eq!(mc.method.name, "greet");
                    assert_eq!(mc.args.len(), 2);
                }
                other => panic!("expected MethodCall, got {other:?}"),
            }
        } else {
            panic!("expected Return statement");
        }
    }

    // ---------------------------------------------------------------
    // Dynamic import expression: import("module")
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_dynamic_import_expression() {
        let module = parse_ok("function main() { const m = import(\"./utils\"); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::DynamicImport(path) => {
                    assert_eq!(path, "./utils");
                }
                other => panic!("expected DynamicImport, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_dynamic_import_with_await() {
        let module = parse_ok("async function main() { const m = await import(\"./mod\"); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::Await(inner) => match &inner.kind {
                    ExprKind::DynamicImport(path) => {
                        assert_eq!(path, "./mod");
                    }
                    other => panic!("expected DynamicImport inside Await, got {other:?}"),
                },
                other => panic!("expected Await, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_static_import_not_confused_with_dynamic() {
        // Ensure static `import { X } from "mod"` still works correctly
        let module = parse_ok("import { greet } from \"./utils\";");
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::Import(decl) => {
                assert_eq!(decl.source.value, "./utils");
                assert_eq!(decl.names.len(), 1);
                assert_eq!(decl.names[0].name, "greet");
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_dynamic_import_requires_string_literal() {
        let (_module, diagnostics) = parse_source("function main() { const m = import(42); }");
        assert!(
            !diagnostics.is_empty(),
            "expected diagnostic for non-string import argument"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("string literal")),
            "expected diagnostic about string literal, got: {diagnostics:?}"
        );
    }

    // ---------------------------------------------------------------
    // Template literal type parsing tests (Task 128)
    // ---------------------------------------------------------------

    // Test: Parse template literal type with no interpolation: `type X = `hello``
    #[test]
    fn test_parser_template_literal_type_simple() {
        let source = "type X = `hello`";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "X");
                let alias = td.type_alias.as_ref().expect("expected type alias");
                match &alias.kind {
                    TypeKind::TemplateLiteralType { quasis, types } => {
                        assert_eq!(quasis.len(), 1);
                        assert_eq!(quasis[0], "hello");
                        assert!(types.is_empty());
                    }
                    other => panic!("expected TemplateLiteralType, got: {other:?}"),
                }
            }
            other => panic!("expected TypeDef, got: {other:?}"),
        }
    }

    // Test: Parse template literal type with one interpolation: `type X = `hello ${string}``
    #[test]
    fn test_parser_template_literal_type_with_interpolation() {
        let source = "type X = `hello ${string}`";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "X");
                let alias = td.type_alias.as_ref().expect("expected type alias");
                match &alias.kind {
                    TypeKind::TemplateLiteralType { quasis, types } => {
                        assert_eq!(quasis.len(), 2);
                        assert_eq!(quasis[0], "hello ");
                        assert_eq!(quasis[1], "");
                        assert_eq!(types.len(), 1);
                        match &types[0].kind {
                            TypeKind::Named(ident) => assert_eq!(ident.name, "string"),
                            other => panic!("expected Named(string), got: {other:?}"),
                        }
                    }
                    other => panic!("expected TemplateLiteralType, got: {other:?}"),
                }
            }
            other => panic!("expected TypeDef, got: {other:?}"),
        }
    }

    // Test: Parse template literal type with multiple interpolations: `type X = `${string}_${number}``
    #[test]
    fn test_parser_template_literal_type_multiple() {
        let source = "type X = `${string}_${number}`";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "X");
                let alias = td.type_alias.as_ref().expect("expected type alias");
                match &alias.kind {
                    TypeKind::TemplateLiteralType { quasis, types } => {
                        assert_eq!(quasis.len(), 3);
                        assert_eq!(quasis[0], "");
                        assert_eq!(quasis[1], "_");
                        assert_eq!(quasis[2], "");
                        assert_eq!(types.len(), 2);
                        match &types[0].kind {
                            TypeKind::Named(ident) => assert_eq!(ident.name, "string"),
                            other => panic!("expected Named(string), got: {other:?}"),
                        }
                        match &types[1].kind {
                            TypeKind::Named(ident) => assert_eq!(ident.name, "number"),
                            other => panic!("expected Named(number), got: {other:?}"),
                        }
                    }
                    other => panic!("expected TemplateLiteralType, got: {other:?}"),
                }
            }
            other => panic!("expected TypeDef, got: {other:?}"),
        }
    }

    // Test: Parse empty template literal type: `type X = ``
    #[test]
    fn test_parser_template_literal_type_empty() {
        let source = "type X = ``";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::TypeDef(td) => {
                assert_eq!(td.name.name, "X");
                let alias = td.type_alias.as_ref().expect("expected type alias");
                match &alias.kind {
                    TypeKind::TemplateLiteralType { quasis, types } => {
                        assert_eq!(quasis.len(), 1);
                        assert_eq!(quasis[0], "");
                        assert!(types.is_empty());
                    }
                    other => panic!("expected TemplateLiteralType, got: {other:?}"),
                }
            }
            other => panic!("expected TypeDef, got: {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // using / await using declarations
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_using_declaration() {
        let module = parse_ok("function main() { using x = getResource(); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Using(decl) => {
                assert_eq!(decl.name.name, "x");
                assert!(!decl.is_await);
                assert!(decl.type_ann.is_none());
                match &decl.init.kind {
                    ExprKind::Call(call) => {
                        assert_eq!(call.callee.name, "getResource");
                    }
                    other => panic!("expected Call, got: {other:?}"),
                }
            }
            other => panic!("expected Using, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_await_using_declaration() {
        let module = parse_ok("function main() { await using conn = getDbConnection(); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Using(decl) => {
                assert_eq!(decl.name.name, "conn");
                assert!(decl.is_await);
                assert!(decl.type_ann.is_none());
                match &decl.init.kind {
                    ExprKind::Call(call) => {
                        assert_eq!(call.callee.name, "getDbConnection");
                    }
                    other => panic!("expected Call, got: {other:?}"),
                }
            }
            other => panic!("expected Using, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_using_with_type_annotation() {
        let module = parse_ok("function main() { using f: File = openFile(); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::Using(decl) => {
                assert_eq!(decl.name.name, "f");
                assert!(!decl.is_await);
                assert!(decl.type_ann.is_some());
            }
            other => panic!("expected Using, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_using_not_confused_with_identifier() {
        // `using` as a variable name in a call expression should not be parsed
        // as a using declaration.
        let module = parse_ok("function main() { using(42); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        // Should parse as expression statement (call to `using`), not Using decl
        match stmt {
            Stmt::Expr(expr) => match &expr.kind {
                ExprKind::Call(call) => {
                    assert_eq!(call.callee.name, "using");
                }
                other => panic!("expected Call expression, got: {other:?}"),
            },
            other => panic!("expected Expr statement, got: {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // T148: Assertion functions (`asserts x is Type`)
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_asserts_is_type() {
        let source = r#"function assertString(value: unknown): asserts value is string {
  if (typeof value !== "string") {
    throw new TypeError("Expected string");
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "assertString");
        let ret = f.return_type.as_ref().expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected type annotation");
        match &type_ann.kind {
            TypeKind::Asserts {
                param,
                guarded_type,
            } => {
                assert_eq!(param.name, "value");
                let gt = guarded_type.as_ref().expect("expected guarded type");
                match &gt.kind {
                    TypeKind::Named(ident) => assert_eq!(ident.name, "string"),
                    other => panic!("expected Named(string), got {other:?}"),
                }
            }
            other => panic!("expected Asserts, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Overload signatures
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_overload_signatures_skipped() {
        // Overload signatures (no body, ending with `;`) should not produce AST nodes
        let source = "\
function greet(name: string): string;
function greet(name: string, greeting: string): string;
function greet(name: string, greeting?: string): string {
  return (greeting || \"Hello\") + \" \" + name;
}";
        let module = parse_ok(source);
        // Only the implementation (with body) should be in the AST
        assert_eq!(
            module.items.len(),
            1,
            "expected only the implementation function"
        );
        let f = first_fn(&module);
        assert_eq!(f.name.name, "greet");
        assert_eq!(f.params.len(), 2);
        assert!(!f.body.stmts.is_empty());
    }

    #[test]
    fn test_parser_overload_implementation_preserved() {
        // The implementation function with a body should parse normally
        let source = "\
function add(a: number): number;
function add(a: number, b: number): number;
function add(a: number, b?: number): number {
  return a + (b || 0);
}";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "add");
        assert_eq!(f.params.len(), 2);
    }

    #[test]
    fn test_parser_overload_no_return_type() {
        // Overload signature without explicit return type
        let source = "\
function log(msg: string);
function log(msg: string, level: string);
function log(msg: string, level?: string) {
  console.log(msg);
}";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "log");
    }

    #[test]
    fn test_parser_overload_exported_function() {
        // Exported overload signatures should also be skipped
        let source = "\
export function greet(name: string): string;
export function greet(name: string, greeting: string): string;
export function greet(name: string, greeting?: string): string {
  return (greeting || \"Hello\") + \" \" + name;
}";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::Function(f) => {
                assert_eq!(f.name.name, "greet");
                assert!(module.items[0].exported);
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_asserts_without_type() {
        let source = r#"function assertDefined(value: unknown): asserts value {
  if (value === null) {
    throw new TypeError("Expected non-null");
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "assertDefined");
        let ret = f.return_type.as_ref().expect("expected return type");
        let type_ann = ret.type_ann.as_ref().expect("expected type annotation");
        match &type_ann.kind {
            TypeKind::Asserts {
                param,
                guarded_type,
            } => {
                assert_eq!(param.name, "value");
                assert!(
                    guarded_type.is_none(),
                    "expected no guarded type for bare `asserts value`"
                );
            }
            other => panic!("expected Asserts, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_overload_in_class_methods() {
        // Method overloads inside a class should be skipped
        let source = "\
class Greeter {
  greet(name: string): string;
  greet(name: string, greeting: string): string;
  greet(name: string, greeting?: string): string {
    return (greeting || \"Hello\") + \" \" + name;
  }
}";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::Class(cls) => {
                assert_eq!(cls.name.name, "Greeter");
                // Only the implementation method should be in the members
                assert_eq!(
                    cls.members.len(),
                    1,
                    "expected only the implementation method"
                );
                match &cls.members[0] {
                    ClassMember::Method(m) => {
                        assert_eq!(m.name.name, "greet");
                        assert_eq!(m.params.len(), 2);
                    }
                    other => panic!("expected Method, got {other:?}"),
                }
            }
            other => panic!("expected Class, got {other:?}"),
        }
    }

    #[test]
    fn test_asserts_not_reserved_keyword() {
        // `asserts` should not be reserved — it should be a valid variable name
        let source = r#"function main() {
  const asserts: i32 = 5;
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::VarDecl(decl) => {
                assert_eq!(decl.name.name, "asserts");
            }
            other => panic!("expected VarDecl, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_overload_constructor() {
        // Constructor overloads inside a class should be skipped
        let source = "\
class Point {
  x: number;
  y: number;
  constructor(x: number);
  constructor(x: number, y: number);
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].kind {
            ItemKind::Class(cls) => {
                assert_eq!(cls.name.name, "Point");
                // 2 fields + 1 constructor = 3 members (overloads skipped)
                assert_eq!(cls.members.len(), 3, "expected 2 fields + 1 constructor");
                match &cls.members[2] {
                    ClassMember::Constructor(ctor) => {
                        assert_eq!(ctor.params.len(), 2);
                    }
                    other => panic!("expected Constructor, got {other:?}"),
                }
            }
            other => panic!("expected Class, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_overload_single_no_overloads() {
        // A regular function without overloads should still work
        let source = "function hello(name: string): string { return \"Hi \" + name; }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "hello");
    }

    #[test]
    fn test_parser_overload_async_function() {
        // Async function overloads
        let source = "\
async function fetch(url: string): Promise<string>;
async function fetch(url: string, options: object): Promise<string>;
async function fetch(url: string, options?: object): Promise<string> {
  return \"data\";
}";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let f = first_fn(&module);
        assert_eq!(f.name.name, "fetch");
        assert!(f.is_async);
        assert_eq!(f.params.len(), 2);
    }

    // ---------------------------------------------------------------
    // declare ambient declarations (Task 150)
    // ---------------------------------------------------------------

    // declare function is parsed and skipped (produces no AST items)
    #[test]
    fn test_parser_declare_function() {
        let source = "declare function fetch(url: string): void;";
        let module = parse_ok(source);
        assert!(
            module.items.is_empty(),
            "declare function should produce no items"
        );
    }

    // declare const is parsed and skipped
    #[test]
    fn test_parser_declare_const() {
        let source = "declare const API_KEY: string;";
        let module = parse_ok(source);
        assert!(
            module.items.is_empty(),
            "declare const should produce no items"
        );
    }

    // declare class is parsed and skipped
    #[test]
    fn test_parser_declare_class() {
        let source = "declare class Window { title: string; close(): void; }";
        let module = parse_ok(source);
        assert!(
            module.items.is_empty(),
            "declare class should produce no items"
        );
    }

    // declare module is parsed and skipped
    #[test]
    fn test_parser_declare_module() {
        let source = r#"declare module "my-lib" { export function hello(): void; }"#;
        let module = parse_ok(source);
        assert!(
            module.items.is_empty(),
            "declare module should produce no items"
        );
    }

    // export declare is parsed and skipped
    #[test]
    fn test_parser_export_declare() {
        let source = "export declare function fetch(url: string): void;";
        let module = parse_ok(source);
        assert!(
            module.items.is_empty(),
            "export declare should produce no items"
        );
    }

    // declare does not break normal items — regular functions still work
    #[test]
    fn test_declare_does_not_break_normal_items() {
        let source = r#"
            declare function fetch(url: string): void;
            function main() {}
            declare const API_KEY: string;
        "#;
        let module = parse_ok(source);
        assert_eq!(
            module.items.len(),
            1,
            "only the non-declared function should appear"
        );
        match &module.items[0].kind {
            ItemKind::Function(f) => assert_eq!(f.name.name, "main"),
            other => panic!("expected Function, got: {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Namespace diagnostic tests (Task 151)
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_namespace_emits_diagnostic() {
        let (module, diagnostics) = parse_source("namespace Foo { }");
        assert!(
            module.items.is_empty(),
            "namespace should not produce an item"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0]
                .message
                .contains("namespaces are not supported"),
            "unexpected message: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_parser_namespace_skips_body() {
        let (module, diagnostics) = parse_source(
            "namespace Foo { export function doStuff(): void {} }\nfunction main() {}",
        );
        // The namespace body should be skipped, and `main` parsed normally.
        assert_eq!(module.items.len(), 1, "expected one item (main fn)");
        match &module.items[0].kind {
            ItemKind::Function(f) => assert_eq!(f.name.name, "main"),
            other => panic!("expected Function, got: {other:?}"),
        }
        assert_eq!(
            diagnostics.len(),
            1,
            "expected one diagnostic for namespace"
        );
    }

    // declare class with nested braces is skipped correctly
    #[test]
    fn test_parser_declare_class_nested_braces() {
        let source = r#"
            declare class Window {
                document: { title: string; };
                close(): void;
            }
            function main() {}
        "#;
        let module = parse_ok(source);
        assert_eq!(
            module.items.len(),
            1,
            "only the non-declared function should appear"
        );
        match &module.items[0].kind {
            ItemKind::Function(f) => assert_eq!(f.name.name, "main"),
            other => panic!("expected Function, got: {other:?}"),
        }
    }

    #[test]
    fn test_parser_namespace_does_not_break_other_code() {
        let source = "\
namespace Foo { const x: i32 = 1; }
function add(a: i32, b: i32): i32 { return a + b; }
namespace Bar { }
function sub(a: i32, b: i32): i32 { return a - b; }";
        let (module, diagnostics) = parse_source(source);
        // Two functions should parse, two namespaces should produce diagnostics.
        assert_eq!(module.items.len(), 2, "expected two function items");
        assert_eq!(diagnostics.len(), 2, "expected two namespace diagnostics");
        match &module.items[0].kind {
            ItemKind::Function(f) => assert_eq!(f.name.name, "add"),
            other => panic!("expected Function(add), got: {other:?}"),
        }
        match &module.items[1].kind {
            ItemKind::Function(f) => assert_eq!(f.name.name, "sub"),
            other => panic!("expected Function(sub), got: {other:?}"),
        }
    }

    #[test]
    fn test_namespace_diagnostic_message() {
        let (_, diagnostics) = parse_source("namespace MyLib { }");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "namespaces are not supported in RustScript; use module imports (`import`/`export`) instead"
        );
    }

    #[test]
    fn test_parser_module_keyword_as_namespace_emits_diagnostic() {
        let (module, diagnostics) = parse_source("module Foo { }");
        assert!(
            module.items.is_empty(),
            "module-as-namespace should not produce an item"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0]
                .message
                .contains("namespaces are not supported"),
            "unexpected message: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_parser_export_namespace_emits_diagnostic() {
        let (module, diagnostics) = parse_source("export namespace Foo { }");
        assert!(
            module.items.is_empty(),
            "export namespace should not produce an item"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0]
                .message
                .contains("namespaces are not supported"),
            "unexpected message: {}",
            diagnostics[0].message
        );
    }

    // ---------------------------------------------------------------
    // T152: Classic C-style for loops
    // ---------------------------------------------------------------

    // T152-1: Parse `for (let i = 0; i < 10; i++) {}` → ForClassicStmt
    #[test]
    fn test_parser_classic_for_loop() {
        let source = r#"function main() {
  for (let i = 0; i < 10; i++) {
    console.log(i);
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForClassic(fc) => {
                // Check init
                assert!(fc.init.is_some());
                if let Some(ForInit::VarDecl(decl)) = &fc.init {
                    assert_eq!(decl.binding, VarBinding::Let);
                    assert_eq!(decl.name.name, "i");
                    assert!(matches!(decl.init.kind, ExprKind::IntLit(0)));
                } else {
                    panic!("expected VarDecl init");
                }
                // Check condition
                assert!(fc.condition.is_some());
                if let Some(ref cond) = fc.condition {
                    assert!(matches!(cond.kind, ExprKind::Binary(ref b) if b.op == BinaryOp::Lt));
                }
                // Check update
                assert!(fc.update.is_some());
                if let Some(ref update) = fc.update {
                    assert!(
                        matches!(&update.kind, ExprKind::PostfixIncrement(inner) if matches!(inner.kind, ExprKind::Ident(ref id) if id.name == "i")),
                        "expected PostfixIncrement(i), got: {:?}",
                        update.kind
                    );
                }
                assert!(!fc.body.stmts.is_empty());
            }
            other => panic!("expected ForClassic, got: {other:?}"),
        }
    }

    // T152-2: Parse `i++` as update in classic for
    #[test]
    fn test_parser_classic_for_with_increment() {
        let source = r#"function main() {
  for (let i = 0; i < 5; i++) {}
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForClassic(fc) => {
                let update = fc.update.as_ref().expect("expected update");
                assert!(
                    matches!(&update.kind, ExprKind::PostfixIncrement(_)),
                    "expected PostfixIncrement, got: {:?}",
                    update.kind
                );
            }
            other => panic!("expected ForClassic, got: {other:?}"),
        }
    }

    // T152-3: Parse `for (;;) {}` (infinite loop)
    #[test]
    fn test_parser_classic_for_empty_parts() {
        let source = r#"function main() {
  for (;;) {
    break;
  }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForClassic(fc) => {
                assert!(fc.init.is_none(), "expected no init");
                assert!(fc.condition.is_none(), "expected no condition");
                assert!(fc.update.is_none(), "expected no update");
            }
            other => panic!("expected ForClassic, got: {other:?}"),
        }
    }

    // T152-4: Parse `for (i = 0; i < 10; i++)` without let (expression init)
    #[test]
    fn test_parser_classic_for_expr_init() {
        let source = r#"function main() {
  for (i = 0; i < 10; i++) {}
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForClassic(fc) => {
                assert!(
                    matches!(&fc.init, Some(ForInit::Expr(_))),
                    "expected Expr init, got: {:?}",
                    fc.init
                );
                assert!(fc.condition.is_some());
                assert!(fc.update.is_some());
            }
            other => panic!("expected ForClassic, got: {other:?}"),
        }
    }

    // T152-5: Regression: for-in still works
    #[test]
    fn test_parser_for_in_still_works() {
        let source = r#"function main() {
  for (const k in obj) { console.log(k); }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        assert!(
            matches!(stmt, Stmt::ForIn(_)),
            "for-in should produce ForIn"
        );
    }

    // T152-6: Regression: for-of still works
    #[test]
    fn test_parser_for_of_still_works() {
        let source = r#"function main() {
  for (const x of items) { console.log(x); }
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        assert!(matches!(stmt, Stmt::For(_)), "for-of should produce For");
    }

    // T152-7: Parse prefix increment in for loop
    #[test]
    fn test_parser_classic_for_prefix_increment() {
        let source = r#"function main() {
  for (let i = 0; i < 5; ++i) {}
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForClassic(fc) => {
                let update = fc.update.as_ref().expect("expected update");
                assert!(
                    matches!(&update.kind, ExprKind::PrefixIncrement(_)),
                    "expected PrefixIncrement, got: {:?}",
                    update.kind
                );
            }
            other => panic!("expected ForClassic, got: {other:?}"),
        }
    }

    // T152-8: Parse postfix decrement in for loop
    #[test]
    fn test_parser_classic_for_decrement() {
        let source = r#"function main() {
  for (let i = 10; i > 0; i--) {}
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForClassic(fc) => {
                let update = fc.update.as_ref().expect("expected update");
                assert!(
                    matches!(&update.kind, ExprKind::PostfixDecrement(_)),
                    "expected PostfixDecrement, got: {:?}",
                    update.kind
                );
            }
            other => panic!("expected ForClassic, got: {other:?}"),
        }
    }

    // T152-9: Parse compound assignment as update
    #[test]
    fn test_parser_classic_for_compound_update() {
        let source = r#"function main() {
  for (let i = 0; i < 20; i += 2) {}
}"#;
        let module = parse_ok(source);
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        match stmt {
            Stmt::ForClassic(fc) => {
                // i += 2 is desugared to i = i + 2 (Assign with Binary)
                let update = fc.update.as_ref().expect("expected update");
                assert!(
                    matches!(&update.kind, ExprKind::Assign(_)),
                    "expected Assign (desugared compound), got: {:?}",
                    update.kind
                );
            }
            other => panic!("expected ForClassic, got: {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Regex literal parsing (Task 154)
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_regex_literal_simple() {
        let module = parse_ok("function main() { const re = /hello/; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::RegexLit { pattern, flags } => {
                    assert_eq!(pattern, "hello");
                    assert!(flags.is_empty());
                }
                other => panic!("expected RegexLit, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_regex_literal_with_flags() {
        let module = parse_ok("function main() { const re = /\\d+/gi; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::RegexLit { pattern, flags } => {
                    assert_eq!(pattern, "\\d+");
                    assert_eq!(flags, "gi");
                }
                other => panic!("expected RegexLit, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_regex_vs_division() {
        // `a / b` in expression context should parse as division, not regex.
        let module = parse_ok("function main() { const x = a / b; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::Binary(bin) => {
                    assert_eq!(bin.op, BinaryOp::Div);
                }
                other => panic!("expected Binary(Div), got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_regex_literal_with_escape() {
        let module = parse_ok("function main() { const re = /foo\\/bar/; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::RegexLit { pattern, flags } => {
                    assert_eq!(pattern, "foo\\/bar");
                    assert!(flags.is_empty());
                }
                other => panic!("expected RegexLit, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_regex_literal_case_insensitive_flag() {
        let module = parse_ok("function main() { const re = /pattern/i; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::RegexLit { pattern, flags } => {
                    assert_eq!(pattern, "pattern");
                    assert_eq!(flags, "i");
                }
                other => panic!("expected RegexLit, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    // ---------------------------------------------------------------
    // Task 155: Mapped types and index access types
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_mapped_type_basic() {
        let source = "type Identity<T> = { [K in keyof T]: T[K] }";
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        assert_eq!(td.name.name, "Identity");
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::MappedType {
                type_param,
                constraint,
                value_type,
                optional,
                readonly,
            } => {
                assert_eq!(type_param.name, "K");
                assert!(matches!(constraint.kind, TypeKind::KeyOf(_)));
                // value_type is T[K]
                assert!(matches!(value_type.kind, TypeKind::IndexAccess(_, _)));
                assert!(optional.is_none());
                assert!(readonly.is_none());
            }
            other => panic!("expected MappedType, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_mapped_type_optional() {
        let source = "type PartialT<T> = { [K in keyof T]?: T[K] }";
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::MappedType { optional, .. } => {
                assert!(
                    matches!(optional, Some(MappedModifier::Add)),
                    "expected Add modifier"
                );
            }
            other => panic!("expected MappedType, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_mapped_type_remove_optional() {
        let source = "type RequiredT<T> = { [K in keyof T]-?: T[K] }";
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::MappedType { optional, .. } => {
                assert!(
                    matches!(optional, Some(MappedModifier::Remove)),
                    "expected Remove modifier"
                );
            }
            other => panic!("expected MappedType, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_index_access_type() {
        // T["name"] parses as IndexAccess
        let source = r#"type X = User["name"]"#;
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::IndexAccess(obj, idx) => {
                assert!(
                    matches!(&obj.kind, TypeKind::Named(ident) if ident.name == "User"),
                    "expected Named User, got {:?}",
                    obj.kind
                );
                assert!(
                    matches!(&idx.kind, TypeKind::StringLiteral(s) if s == "name"),
                    "expected StringLiteral 'name', got {:?}",
                    idx.kind
                );
            }
            other => panic!("expected IndexAccess, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_index_access_type_ident_key() {
        // T[K] parses as IndexAccess where K is a named type
        let source = "type X<T, K> = T[K]";
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::IndexAccess(obj, idx) => {
                assert!(matches!(&obj.kind, TypeKind::Named(ident) if ident.name == "T"));
                assert!(matches!(&idx.kind, TypeKind::Named(ident) if ident.name == "K"));
            }
            other => panic!("expected IndexAccess, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_mapped_vs_index_signature() {
        // Index signature should still work: { [key: string]: T }
        let source = "type Config = { [key: string]: string }";
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        // Index signatures are stored as index_signature field, not type_alias
        assert!(td.type_alias.is_none(), "should not be a type alias");
        let sig = td
            .index_signature
            .as_ref()
            .expect("expected index signature");
        assert_eq!(sig.key_name.name, "key");
        assert!(matches!(sig.key_type.kind, TypeKind::Named(ref ident) if ident.name == "string"));
        assert!(
            matches!(sig.value_type.kind, TypeKind::Named(ref ident) if ident.name == "string")
        );
    }

    #[test]
    fn test_parser_mapped_type_nullable() {
        // Type alias for mapped type that makes all fields nullable
        let source = "type Nullable<T> = { [K in keyof T]: T[K] | null }";
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        assert_eq!(td.name.name, "Nullable");
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::MappedType {
                type_param,
                value_type,
                ..
            } => {
                assert_eq!(type_param.name, "K");
                // value_type should be a union: T[K] | null
                assert!(
                    matches!(value_type.kind, TypeKind::Union(_)),
                    "expected Union value type, got {:?}",
                    value_type.kind
                );
            }
            other => panic!("expected MappedType, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_mapped_type_readonly() {
        let source = "type ReadonlyT<T> = { readonly [K in keyof T]: T[K] }";
        let module = parse_ok(source);
        let ItemKind::TypeDef(td) = &module.items[0].kind else {
            panic!("expected TypeDef");
        };
        let alias = td.type_alias.as_ref().expect("expected type alias");
        match &alias.kind {
            TypeKind::MappedType { readonly, .. } => {
                assert!(
                    matches!(readonly, Some(MappedModifier::Add)),
                    "expected readonly Add modifier"
                );
            }
            other => panic!("expected MappedType, got {other:?}"),
        }
    }

    #[test]
    fn test_parser_inline_mapped_type_annotation() {
        // Mapped type used as a type annotation (not just type alias)
        let source = r#"
            function foo(x: { [K in keyof User]: User[K] | null }): void {
            }
        "#;
        let module = parse_ok(source);
        let ItemKind::Function(f) = &module.items[0].kind else {
            panic!("expected Function");
        };
        let param_type = &f.params[0].type_ann;
        assert!(
            matches!(param_type.kind, TypeKind::MappedType { .. }),
            "expected MappedType param type, got {:?}",
            param_type.kind
        );
    }

    // ---- Task 156: const enum tests ----

    /// T156-1: `const enum Dir { Up, Down }` parses with is_const true
    #[test]
    fn test_parser_const_enum() {
        let source = "const enum Dir { Up, Down }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let ItemKind::EnumDef(ed) = &module.items[0].kind else {
            panic!("expected EnumDef, got {:?}", module.items[0].kind);
        };
        assert!(ed.is_const, "expected is_const to be true");
        assert_eq!(ed.name.name, "Dir");
        assert_eq!(ed.variants.len(), 2);
        match &ed.variants[0] {
            EnumVariant::Simple(ident, _) => assert_eq!(ident.name, "Up"),
            other => panic!("expected Simple variant, got {other:?}"),
        }
        match &ed.variants[1] {
            EnumVariant::Simple(ident, _) => assert_eq!(ident.name, "Down"),
            other => panic!("expected Simple variant, got {other:?}"),
        }
    }

    /// T156-2: `const x = 1` still works (not confused with const enum)
    #[test]
    fn test_parser_const_enum_not_confused_with_const_var() {
        let source = "const x: i32 = 1;";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        assert!(
            matches!(&module.items[0].kind, ItemKind::Const(_)),
            "expected Const item, got {:?}",
            module.items[0].kind
        );
    }

    /// T156-3: bare `enum` (non-const) also parses
    #[test]
    fn test_parser_bare_enum() {
        let source = "enum Color { Red, Green, Blue }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let ItemKind::EnumDef(ed) = &module.items[0].kind else {
            panic!("expected EnumDef");
        };
        assert!(!ed.is_const, "expected is_const to be false");
        assert_eq!(ed.name.name, "Color");
        assert_eq!(ed.variants.len(), 3);
    }

    /// T156-4: const enum with explicit values
    #[test]
    fn test_parser_const_enum_with_values() {
        let source = "const enum Priority { Low = 1, High = 2 }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let ItemKind::EnumDef(ed) = &module.items[0].kind else {
            panic!("expected EnumDef");
        };
        assert!(ed.is_const);
        assert_eq!(ed.name.name, "Priority");
        assert_eq!(ed.variants.len(), 2);
        match &ed.variants[0] {
            EnumVariant::Simple(ident, _) => assert_eq!(ident.name, "Low"),
            other => panic!("expected Simple variant, got {other:?}"),
        }
        match &ed.variants[1] {
            EnumVariant::Simple(ident, _) => assert_eq!(ident.name, "High"),
            other => panic!("expected Simple variant, got {other:?}"),
        }
    }

    /// T156-5: export const enum
    #[test]
    fn test_parser_export_const_enum() {
        let source = "export const enum Status { Active, Inactive }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        assert!(module.items[0].exported);
        let ItemKind::EnumDef(ed) = &module.items[0].kind else {
            panic!("expected EnumDef");
        };
        assert!(ed.is_const);
        assert_eq!(ed.name.name, "Status");
        assert_eq!(ed.variants.len(), 2);
    }

    /// T156-6: export bare enum
    #[test]
    fn test_parser_export_bare_enum() {
        let source = "export enum Fruit { Apple, Banana }";
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        assert!(module.items[0].exported);
        let ItemKind::EnumDef(ed) = &module.items[0].kind else {
            panic!("expected EnumDef");
        };
        assert!(!ed.is_const);
        assert_eq!(ed.name.name, "Fruit");
    }

    /// T156-7: trailing comma in enum
    #[test]
    fn test_parser_const_enum_trailing_comma() {
        let source = "const enum Dir { Up, Down, }";
        let module = parse_ok(source);
        let ItemKind::EnumDef(ed) = &module.items[0].kind else {
            panic!("expected EnumDef");
        };
        assert_eq!(ed.variants.len(), 2);
    }

    /// T156-8: regular `type` enum is_const is false
    #[test]
    fn test_parser_type_enum_is_not_const() {
        let source = r#"type Dir = "up" | "down";"#;
        let module = parse_ok(source);
        let ItemKind::EnumDef(ed) = &module.items[0].kind else {
            panic!("expected EnumDef");
        };
        assert!(!ed.is_const);
    }

    // ---------------------------------------------------------------
    // Class expressions
    // ---------------------------------------------------------------
    #[test]
    fn test_parser_class_expression_anonymous() {
        let source = r#"
            const MyClass = class {
                value: i32;
                getValue(): i32 {
                    return this.value;
                }
            };
        "#;
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let ItemKind::Const(decl) = &module.items[0].kind else {
            panic!("expected Const item");
        };
        assert_eq!(decl.name.name, "MyClass");
        let ExprKind::ClassExpr(cls) = &decl.init.kind else {
            panic!("expected ClassExpr, got {:?}", decl.init.kind);
        };
        // Anonymous class gets a placeholder name
        assert_eq!(cls.name.name, "__AnonymousClass");
        assert_eq!(cls.members.len(), 2);
        assert!(matches!(cls.members[0], ClassMember::Field(_)));
        assert!(matches!(cls.members[1], ClassMember::Method(_)));
    }

    #[test]
    fn test_parser_class_expression_named() {
        let source = r#"
            const Foo = class Bar {
                x: i32;
            };
        "#;
        let module = parse_ok(source);
        assert_eq!(module.items.len(), 1);
        let ItemKind::Const(decl) = &module.items[0].kind else {
            panic!("expected Const item");
        };
        assert_eq!(decl.name.name, "Foo");
        let ExprKind::ClassExpr(cls) = &decl.init.kind else {
            panic!("expected ClassExpr, got {:?}", decl.init.kind);
        };
        // Named class expression uses the inner name
        assert_eq!(cls.name.name, "Bar");
        assert_eq!(cls.members.len(), 1);
    }

    #[test]
    fn test_parser_class_expression_with_extends() {
        let source = r#"
            const Sub = class extends Base {
                extra: string;
            };
        "#;
        let module = parse_ok(source);
        let ItemKind::Const(decl) = &module.items[0].kind else {
            panic!("expected Const item");
        };
        let ExprKind::ClassExpr(cls) = &decl.init.kind else {
            panic!("expected ClassExpr");
        };
        assert_eq!(cls.name.name, "__AnonymousClass");
        assert_eq!(cls.extends.as_ref().map(|e| e.name.as_str()), Some("Base"));
    }

    #[test]
    fn test_parser_class_expression_empty() {
        let source = "const Empty = class {};";
        let module = parse_ok(source);
        let ItemKind::Const(decl) = &module.items[0].kind else {
            panic!("expected Const item");
        };
        let ExprKind::ClassExpr(cls) = &decl.init.kind else {
            panic!("expected ClassExpr");
        };
        assert!(cls.members.is_empty());
    }

    // ---------------------------------------------------------------
    // Task 159: new.target, import.meta, debugger
    // ---------------------------------------------------------------

    #[test]
    fn test_parser_new_target() {
        let module = parse_ok("function main() { const x = new.target; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            assert!(
                matches!(decl.init.kind, ExprKind::NewTarget),
                "expected NewTarget, got {:?}",
                decl.init.kind
            );
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_import_meta() {
        let module = parse_ok("function main() { const x = import.meta; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            assert!(
                matches!(decl.init.kind, ExprKind::ImportMeta),
                "expected ImportMeta, got {:?}",
                decl.init.kind
            );
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_import_meta_url() {
        // `import.meta.url` should parse as a field access on ImportMeta
        let module = parse_ok("function main() { const x = import.meta.url; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::FieldAccess(fa) => {
                    assert!(
                        matches!(fa.object.kind, ExprKind::ImportMeta),
                        "expected ImportMeta as object, got {:?}",
                        fa.object.kind
                    );
                    assert_eq!(fa.field.name, "url");
                }
                other => panic!("expected FieldAccess on ImportMeta, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }

    #[test]
    fn test_parser_debugger_statement() {
        let module = parse_ok("function main() { debugger; }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        assert!(
            matches!(stmt, Stmt::Debugger(_)),
            "expected Debugger, got {stmt:?}"
        );
    }

    #[test]
    fn test_parser_debugger_no_semicolon() {
        // debugger without semicolon should also work
        let module = parse_ok("function main() { debugger }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        assert!(
            matches!(stmt, Stmt::Debugger(_)),
            "expected Debugger, got {stmt:?}"
        );
    }

    #[test]
    fn test_new_still_works() {
        // Regression: ensure `new Foo()` still parses correctly
        let module = parse_ok("function main() { const x = new Map<string, i32>(); }");
        let f = first_fn(&module);
        let stmt = first_stmt(f);
        if let Stmt::VarDecl(decl) = stmt {
            match &decl.init.kind {
                ExprKind::New(new_expr) => {
                    assert_eq!(new_expr.type_name.name, "Map");
                    assert_eq!(new_expr.type_args.len(), 2);
                }
                other => panic!("expected New, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl, got {stmt:?}");
        }
    }
}
