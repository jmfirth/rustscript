//! Lexer for `RustScript` source files.
//!
//! Takes raw source text and produces a stream of [`Token`]s with accurate
//! source spans. The lexer recovers from invalid characters by emitting a
//! diagnostic and continuing, so the token stream is always complete.

use rustscript_syntax::diagnostic::Diagnostic;
use rustscript_syntax::source::FileId;
use rustscript_syntax::span::Span;

use crate::token::{Token, TokenKind};

/// Lexer for `RustScript` source text.
///
/// Created with [`Lexer::new`] and consumed by [`Lexer::tokenize`], which
/// produces the full token stream along with any diagnostics encountered.
pub struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    file_id: FileId,
    diagnostics: Vec<Diagnostic>,
    /// Buffered tokens from template literal lexing.
    ///
    /// Template literals produce multiple tokens (head, expression tokens,
    /// middle, more expression tokens, tail) in a single lex operation.
    /// These are queued here and drained before lexing new tokens.
    token_buffer: Vec<Token>,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer over the given source text.
    #[must_use]
    pub fn new(source: &'a str, file_id: FileId) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            file_id,
            diagnostics: Vec::new(),
            token_buffer: Vec::new(),
        }
    }

    /// Tokenize the entire source, returning tokens and any diagnostics.
    ///
    /// The returned token vector always ends with an [`TokenKind::Eof`] token.
    /// Diagnostics are emitted for invalid characters, unterminated strings,
    /// and unknown escape sequences — but lexing always completes.
    #[must_use]
    pub fn tokenize(mut self) -> (Vec<Token>, Vec<Diagnostic>) {
        let mut tokens = Vec::new();

        loop {
            // Drain any buffered tokens first (from template literal lexing
            // or JSDoc comment capture).
            if !self.token_buffer.is_empty() {
                tokens.append(&mut self.token_buffer);
            }

            self.skip_whitespace_and_comments();

            // Drain any tokens produced during whitespace/comment skipping
            // (specifically JSDoc tokens captured by skip_whitespace_and_comments).
            if !self.token_buffer.is_empty() {
                tokens.append(&mut self.token_buffer);
            }

            if self.is_at_end() {
                tokens.push(self.make_eof());
                break;
            }

            if let Some(tok) = self.next_token() {
                tokens.push(tok);
            }
        }

        (tokens, self.diagnostics)
    }

    /// Peek at the current byte without advancing.
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Peek at the byte one position ahead of current.
    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    /// Advance past the current byte and return it.
    fn advance(&mut self) -> Option<u8> {
        let byte = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        Some(byte)
    }

    /// Advance past the current UTF-8 character, returning the char and its
    /// byte width. This is used when the current byte is non-ASCII so that we
    /// skip the full multi-byte character rather than landing on a continuation
    /// byte (which would panic on `str` slicing).
    fn advance_char(&mut self) -> Option<(char, usize)> {
        let remaining = self.source.get(self.pos..)?;
        let ch = remaining.chars().next()?;
        let width = ch.len_utf8();
        self.pos += width;
        Some((ch, width))
    }

    /// Whether we have consumed all source bytes.
    fn is_at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Create a span from `start` to the current position.
    fn span_from(&self, start: usize) -> Span {
        #[allow(clippy::cast_possible_truncation)]
        // Source files larger than 4 GiB are not supported.
        Span::new(start as u32, self.pos as u32)
    }

    /// Create the EOF token at the current position.
    fn make_eof(&self) -> Token {
        #[allow(clippy::cast_possible_truncation)]
        let pos = self.pos as u32;
        Token {
            kind: TokenKind::Eof,
            span: Span::new(pos, pos),
        }
    }

    /// Skip whitespace, line comments, and block comments.
    ///
    /// `JSDoc` comments (`/** ... */`) are NOT skipped — they are captured
    /// as `JsDoc` tokens and buffered for the parser to consume.
    /// Regular block comments (`/* ... */`) and line comments are skipped.
    ///
    /// Also skips the Unicode BOM (U+FEFF) which some editors prepend to files.
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace (including BOM which is 0xEF 0xBB 0xBF in UTF-8)
            while let Some(b) = self.peek() {
                if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                    self.advance();
                } else if b == 0xEF
                    && self.bytes.get(self.pos + 1).copied() == Some(0xBB)
                    && self.bytes.get(self.pos + 2).copied() == Some(0xBF)
                {
                    // Unicode BOM (U+FEFF): skip all 3 bytes
                    self.pos += 3;
                } else {
                    break;
                }
            }

            // Check for line comment
            if self.peek() == Some(b'/') && self.peek_next() == Some(b'/') {
                self.advance(); // consume first /
                self.advance(); // consume second /
                while let Some(b) = self.peek() {
                    if b == b'\n' {
                        break;
                    }
                    self.advance();
                }
                continue;
            }

            // Check for JSDoc comment: `/**` (but not `/**/` which is an empty block comment)
            if self.peek() == Some(b'/')
                && self.peek_next() == Some(b'*')
                && self.bytes.get(self.pos + 2).copied() == Some(b'*')
                && self.bytes.get(self.pos + 3).copied() != Some(b'/')
            {
                let start = self.pos;
                self.advance(); // consume /
                self.advance(); // consume *
                self.advance(); // consume *

                let content_start = self.pos;
                let mut found_end = false;
                while !self.is_at_end() {
                    if self.peek() == Some(b'*') && self.peek_next() == Some(b'/') {
                        let content_end = self.pos;
                        self.advance(); // consume *
                        self.advance(); // consume /
                        found_end = true;

                        let raw = &self.source[content_start..content_end];
                        let stripped = strip_jsdoc_content(raw);
                        self.token_buffer.push(Token {
                            kind: TokenKind::JsDoc(stripped),
                            span: self.span_from(start),
                        });
                        break;
                    }
                    self.advance();
                }
                if !found_end {
                    self.diagnostics.push(
                        Diagnostic::error("unterminated JSDoc comment").with_label(
                            self.span_from(start),
                            self.file_id,
                            "JSDoc comment starts here",
                        ),
                    );
                }
                continue;
            }

            // Check for block comment
            if self.peek() == Some(b'/') && self.peek_next() == Some(b'*') {
                let start = self.pos;
                self.advance(); // consume /
                self.advance(); // consume *
                let mut found_end = false;
                while !self.is_at_end() {
                    if self.peek() == Some(b'*') && self.peek_next() == Some(b'/') {
                        self.advance(); // consume *
                        self.advance(); // consume /
                        found_end = true;
                        break;
                    }
                    self.advance();
                }
                if !found_end {
                    self.diagnostics.push(
                        Diagnostic::error("unterminated block comment").with_label(
                            self.span_from(start),
                            self.file_id,
                            "comment starts here",
                        ),
                    );
                }
                continue;
            }

            // Nothing more to skip
            break;
        }
    }

    /// Lex the next token from the current position.
    ///
    /// Returns `None` if the current character is invalid (a diagnostic is
    /// emitted and the position advances past it).
    fn next_token(&mut self) -> Option<Token> {
        let start = self.pos;

        let byte = self.peek()?;

        // String literal
        if byte == b'"' {
            return Some(self.lex_string(start));
        }

        // Template literal
        if byte == b'`' {
            return self.lex_template_literal(start);
        }

        // Number literal
        if byte.is_ascii_digit() {
            return Some(self.lex_number(start));
        }

        // Hash-private identifier: `#field`
        if byte == b'#'
            && self
                .peek_next()
                .is_some_and(|next| next.is_ascii_alphabetic() || next == b'_')
        {
            return Some(self.lex_hash_ident(start));
        }

        // Identifier or keyword
        if byte.is_ascii_alphabetic() || byte == b'_' {
            return Some(self.lex_ident(start));
        }

        // Two-character operators (checked before single-char)
        if let Some(tok) = self.try_two_char_operator(start) {
            return Some(tok);
        }

        // Single-character operators and delimiters
        if let Some(kind) = Self::single_char_kind(byte) {
            self.advance();
            return Some(Token {
                kind,
                span: self.span_from(start),
            });
        }

        // Invalid character — emit diagnostic, advance past full UTF-8 char, return None.
        // We use advance_char() instead of advance() so that multi-byte characters
        // (e.g., BOM U+FEFF, accented letters) are skipped entirely rather than
        // leaving the position on a continuation byte.
        let ch = if let Some((c, _)) = self.advance_char() {
            c
        } else {
            self.advance();
            char::REPLACEMENT_CHARACTER
        };
        let span = self.span_from(start);
        self.diagnostics.push(
            Diagnostic::error(format!("unexpected character `{ch}`")).with_label(
                span,
                self.file_id,
                "not recognized",
            ),
        );
        None
    }

    /// Lex a string literal starting at the opening `"`.
    fn lex_string(&mut self, start: usize) -> Token {
        self.advance(); // consume opening "

        let mut value = String::new();

        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    // Unterminated string
                    let span = self.span_from(start);
                    self.diagnostics.push(
                        Diagnostic::error("unterminated string literal").with_label(
                            span,
                            self.file_id,
                            "string starts here",
                        ),
                    );
                    return Token {
                        kind: TokenKind::StringLit(value),
                        span,
                    };
                }
                Some(b'"') => {
                    self.advance(); // consume closing "
                    return Token {
                        kind: TokenKind::StringLit(value),
                        span: self.span_from(start),
                    };
                }
                Some(b'\\') => {
                    let escape_start = self.pos;
                    self.advance(); // consume backslash
                    match self.peek() {
                        Some(b'\\') => {
                            value.push('\\');
                            self.advance();
                        }
                        Some(b'"') => {
                            value.push('"');
                            self.advance();
                        }
                        Some(b'n') => {
                            value.push('\n');
                            self.advance();
                        }
                        Some(b't') => {
                            value.push('\t');
                            self.advance();
                        }
                        Some(b'r') => {
                            value.push('\r');
                            self.advance();
                        }
                        Some(b'0') => {
                            value.push('\0');
                            self.advance();
                        }
                        Some(b) => {
                            // Unknown escape sequence — emit diagnostic, keep the char
                            let ch = b as char;
                            self.advance();
                            let span = self.span_from(escape_start);
                            self.diagnostics.push(
                                Diagnostic::error(format!("unknown escape sequence `\\{ch}`"))
                                    .with_label(span, self.file_id, "unknown escape"),
                            );
                            value.push(ch);
                        }
                        None => {
                            // EOF after backslash — unterminated string
                            let span = self.span_from(start);
                            self.diagnostics.push(
                                Diagnostic::error("unterminated string literal").with_label(
                                    span,
                                    self.file_id,
                                    "string starts here",
                                ),
                            );
                            return Token {
                                kind: TokenKind::StringLit(value),
                                span,
                            };
                        }
                    }
                }
                Some(b) => {
                    value.push(b as char);
                    self.advance();
                }
            }
        }
    }

    /// Lex a template literal starting at the opening backtick.
    ///
    /// Produces one or more tokens depending on the template content:
    /// - No interpolation: returns `TemplateNoSub` directly
    /// - With interpolations: returns the first `TemplateHead` token and pushes
    ///   expression tokens, `TemplateMiddle`s, and a `TemplateTail` into the
    ///   token buffer
    fn lex_template_literal(&mut self, start: usize) -> Option<Token> {
        self.advance(); // consume opening backtick

        let mut text = String::new();

        loop {
            match self.peek() {
                None => {
                    // Unterminated template literal
                    let span = self.span_from(start);
                    self.diagnostics.push(
                        Diagnostic::error("unterminated template literal").with_label(
                            span,
                            self.file_id,
                            "template literal starts here",
                        ),
                    );
                    return Some(Token {
                        kind: TokenKind::TemplateNoSub(text),
                        span,
                    });
                }
                Some(b'`') => {
                    // Closing backtick — no interpolations (or final tail)
                    self.advance();
                    return Some(Token {
                        kind: TokenKind::TemplateNoSub(text),
                        span: self.span_from(start),
                    });
                }
                Some(b'$') if self.peek_next() == Some(b'{') => {
                    // Start of interpolation — emit head, then lex expressions
                    self.advance(); // consume `$`
                    self.advance(); // consume `{`

                    let head_span = self.span_from(start);
                    let head = Token {
                        kind: TokenKind::TemplateHead(text),
                        span: head_span,
                    };

                    // Lex the expression tokens and remaining template parts
                    self.lex_template_expr_and_rest(start);

                    return Some(head);
                }
                Some(b'\\') => {
                    // Escape sequence within template literal
                    self.advance(); // consume backslash
                    match self.peek() {
                        Some(b'\\') => {
                            text.push('\\');
                            self.advance();
                        }
                        Some(b'`') => {
                            text.push('`');
                            self.advance();
                        }
                        Some(b'$') => {
                            text.push('$');
                            self.advance();
                        }
                        Some(b'n') => {
                            text.push('\n');
                            self.advance();
                        }
                        Some(b't') => {
                            text.push('\t');
                            self.advance();
                        }
                        Some(b'r') => {
                            text.push('\r');
                            self.advance();
                        }
                        Some(b) => {
                            let ch = b as char;
                            let escape_start = self.pos - 1;
                            self.advance();
                            let span = self.span_from(escape_start);
                            self.diagnostics.push(
                                Diagnostic::error(format!("unknown escape sequence `\\{ch}`"))
                                    .with_label(span, self.file_id, "unknown escape"),
                            );
                            text.push(ch);
                        }
                        None => {
                            let span = self.span_from(start);
                            self.diagnostics.push(
                                Diagnostic::error("unterminated template literal").with_label(
                                    span,
                                    self.file_id,
                                    "template literal starts here",
                                ),
                            );
                            return Some(Token {
                                kind: TokenKind::TemplateNoSub(text),
                                span,
                            });
                        }
                    }
                }
                Some(b) => {
                    text.push(b as char);
                    self.advance();
                }
            }
        }
    }

    /// Lex expression tokens within a template interpolation, then continue
    /// lexing the remaining template string parts.
    ///
    /// Tracks brace nesting depth so that `}` inside the expression (e.g.,
    /// object literals) doesn't prematurely end the interpolation.
    /// All tokens (expression tokens, middle segments, tail segment) are
    /// pushed into `self.token_buffer`.
    #[allow(clippy::too_many_lines)]
    // Template expression and continuation lexing handles two modes (expr + string)
    // in a single function; splitting would obscure the control flow.
    fn lex_template_expr_and_rest(&mut self, template_start: usize) {
        let mut brace_depth: usize = 0;

        // Lex expression tokens until we find the matching `}`
        loop {
            self.skip_whitespace_and_comments();

            if self.is_at_end() {
                let span = self.span_from(template_start);
                self.diagnostics.push(
                    Diagnostic::error("unterminated template literal interpolation").with_label(
                        span,
                        self.file_id,
                        "template literal starts here",
                    ),
                );
                // Push an empty tail so the parser can recover
                self.token_buffer.push(Token {
                    kind: TokenKind::TemplateTail(String::new()),
                    span,
                });
                return;
            }

            // Check for closing `}` at depth 0
            if self.peek() == Some(b'}') && brace_depth == 0 {
                self.advance(); // consume `}`
                break;
            }

            // Track brace nesting
            if self.peek() == Some(b'{') {
                brace_depth += 1;
            } else if self.peek() == Some(b'}') {
                brace_depth = brace_depth.saturating_sub(1);
            }

            // Lex a normal token
            if let Some(tok) = self.next_token() {
                self.token_buffer.push(tok);
            }
        }

        // Now continue scanning the template string after the `}`
        let mut text = String::new();

        loop {
            match self.peek() {
                None => {
                    // Unterminated template literal
                    let span = self.span_from(template_start);
                    self.diagnostics.push(
                        Diagnostic::error("unterminated template literal").with_label(
                            span,
                            self.file_id,
                            "template literal starts here",
                        ),
                    );
                    self.token_buffer.push(Token {
                        kind: TokenKind::TemplateTail(text),
                        span,
                    });
                    return;
                }
                Some(b'`') => {
                    // End of template literal
                    self.advance();
                    let span = self.span_from(template_start);
                    self.token_buffer.push(Token {
                        kind: TokenKind::TemplateTail(text),
                        span,
                    });
                    return;
                }
                Some(b'$') if self.peek_next() == Some(b'{') => {
                    // Another interpolation — emit middle token
                    self.advance(); // consume `$`
                    self.advance(); // consume `{`

                    let span = self.span_from(template_start);
                    self.token_buffer.push(Token {
                        kind: TokenKind::TemplateMiddle(text),
                        span,
                    });

                    // Recurse to lex the next expression and remaining parts
                    self.lex_template_expr_and_rest(template_start);
                    return;
                }
                Some(b'\\') => {
                    // Escape sequence
                    self.advance(); // consume backslash
                    match self.peek() {
                        Some(b'\\') => {
                            text.push('\\');
                            self.advance();
                        }
                        Some(b'`') => {
                            text.push('`');
                            self.advance();
                        }
                        Some(b'$') => {
                            text.push('$');
                            self.advance();
                        }
                        Some(b'n') => {
                            text.push('\n');
                            self.advance();
                        }
                        Some(b't') => {
                            text.push('\t');
                            self.advance();
                        }
                        Some(b'r') => {
                            text.push('\r');
                            self.advance();
                        }
                        Some(b) => {
                            let ch = b as char;
                            let escape_start = self.pos - 1;
                            self.advance();
                            let span = self.span_from(escape_start);
                            self.diagnostics.push(
                                Diagnostic::error(format!("unknown escape sequence `\\{ch}`"))
                                    .with_label(span, self.file_id, "unknown escape"),
                            );
                            text.push(ch);
                        }
                        None => {
                            let span = self.span_from(template_start);
                            self.diagnostics.push(
                                Diagnostic::error("unterminated template literal").with_label(
                                    span,
                                    self.file_id,
                                    "template literal starts here",
                                ),
                            );
                            self.token_buffer.push(Token {
                                kind: TokenKind::TemplateTail(text),
                                span,
                            });
                            return;
                        }
                    }
                }
                Some(b) => {
                    text.push(b as char);
                    self.advance();
                }
            }
        }
    }

    /// Lex a numeric literal (integer or float).
    ///
    /// Supports decimal, hexadecimal (`0x`/`0X`), binary (`0b`/`0B`), and
    /// octal (`0o`/`0O`) integer literals, as well as decimal floats.
    fn lex_number(&mut self, start: usize) -> Token {
        // Check for radix prefix: 0x, 0b, 0o.
        // At entry, self.pos == start and the byte at start is the first digit.
        // If it's `0`, peek at the next byte for a radix prefix.
        if self.bytes.get(start) == Some(&b'0')
            && let Some(prefix) = self.bytes.get(start + 1).copied()
        {
            match prefix {
                b'x' | b'X' => {
                    self.advance(); // consume '0'
                    self.advance(); // consume 'x'/'X'
                    while let Some(b) = self.peek() {
                        if b.is_ascii_hexdigit() {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let digits = &self.source[start + 2..self.pos];
                    let val = i64::from_str_radix(digits, 16).unwrap_or(0);
                    return Token {
                        kind: TokenKind::IntLit(val),
                        span: self.span_from(start),
                    };
                }
                b'b' | b'B' => {
                    self.advance(); // consume '0'
                    self.advance(); // consume 'b'/'B'
                    while let Some(b) = self.peek() {
                        if b == b'0' || b == b'1' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let digits = &self.source[start + 2..self.pos];
                    let val = i64::from_str_radix(digits, 2).unwrap_or(0);
                    return Token {
                        kind: TokenKind::IntLit(val),
                        span: self.span_from(start),
                    };
                }
                b'o' | b'O' => {
                    self.advance(); // consume '0'
                    self.advance(); // consume 'o'/'O'
                    while let Some(b) = self.peek() {
                        if (b'0'..=b'7').contains(&b) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let digits = &self.source[start + 2..self.pos];
                    let val = i64::from_str_radix(digits, 8).unwrap_or(0);
                    return Token {
                        kind: TokenKind::IntLit(val),
                        span: self.span_from(start),
                    };
                }
                _ => {}
            }
        }

        // Consume remaining decimal digits
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }

        // Check for a dot followed by digits (float)
        if self.peek() == Some(b'.') && self.peek_next().is_some_and(|b| b.is_ascii_digit()) {
            self.advance(); // consume dot
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            let text = &self.source[start..self.pos];
            // parse will not fail for well-formed digit.digit sequences
            let val: f64 = text.parse().unwrap_or(0.0);
            return Token {
                kind: TokenKind::FloatLit(val),
                span: self.span_from(start),
            };
        }

        let text = &self.source[start..self.pos];
        let val: i64 = text.parse().unwrap_or(0);
        Token {
            kind: TokenKind::IntLit(val),
            span: self.span_from(start),
        }
    }

    /// Lex an identifier or keyword.
    fn lex_ident(&mut self, start: usize) -> Token {
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance();
            } else {
                break;
            }
        }

        let text = &self.source[start..self.pos];
        let kind = match text {
            "function" => TokenKind::Function,
            "const" => TokenKind::Const,
            "let" => TokenKind::Let,
            "var" => TokenKind::Var,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "do" => TokenKind::Do,
            "return" => TokenKind::Return,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "type" => TokenKind::Type,
            "extends" => TokenKind::Extends,
            "switch" => TokenKind::Switch,
            "case" => TokenKind::Case,
            "new" => TokenKind::New,
            "null" => TokenKind::Null,
            "throw" => TokenKind::Throw,
            "throws" => TokenKind::Throws,
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "finally" => TokenKind::Finally,
            "move" => TokenKind::Move,
            "interface" => TokenKind::Interface,
            "for" => TokenKind::For,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "import" => TokenKind::Import,
            "export" => TokenKind::Export,
            "from" => TokenKind::From,
            "class" => TokenKind::Class,
            "constructor" => TokenKind::Constructor,
            "this" => TokenKind::This,
            "super" => TokenKind::Super,
            "private" => TokenKind::Private,
            "public" => TokenKind::Public,
            "implements" => TokenKind::Implements,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            "rust" => TokenKind::Rust,
            "derives" => TokenKind::Derives,
            "yield" => TokenKind::Yield,
            "infer" => TokenKind::Infer,
            "as" => TokenKind::As,
            "typeof" => TokenKind::TypeOf,
            "keyof" => TokenKind::KeyOf,
            "abstract" => TokenKind::Abstract,
            "override" => TokenKind::Override,
            "satisfies" => TokenKind::Satisfies,
            "delete" => TokenKind::Delete,
            "void" => TokenKind::Void,
            "in" => TokenKind::In,
            "instanceof" => TokenKind::InstanceOf,
            "declare" => TokenKind::Declare,
            "enum" => TokenKind::Enum,
            "debugger" => TokenKind::Debugger,
            _ => TokenKind::Ident(text.to_owned()),
        };

        Token {
            kind,
            span: self.span_from(start),
        }
    }

    /// Lex a hash-private identifier: `#fieldName`.
    ///
    /// The `#` prefix is included in the token text (e.g., `#password`).
    /// Produces an `Ident` token with the `#` prefix preserved.
    fn lex_hash_ident(&mut self, start: usize) -> Token {
        self.advance(); // consume `#`
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance();
            } else {
                break;
            }
        }

        let text = &self.source[start..self.pos];
        Token {
            kind: TokenKind::Ident(text.to_owned()),
            span: self.span_from(start),
        }
    }

    /// Peek at the byte two positions ahead of current.
    fn peek_two_ahead(&self) -> Option<u8> {
        self.bytes.get(self.pos + 2).copied()
    }

    /// Try to lex a multi-character operator. Returns `None` if the current
    /// position does not start a multi-character operator.
    ///
    /// Handles 3-char tokens (`===`, `!==`) before 2-char tokens to avoid
    /// partial matches.
    fn try_two_char_operator(&mut self, start: usize) -> Option<Token> {
        let first = self.peek()?;
        let second = self.peek_next()?;

        // 3-char operators: `===`, `!==`, `...`, `??=`, `||=`, `&&=`
        if let Some(third) = self.peek_two_ahead() {
            let kind_3 = match (first, second, third) {
                (b'=', b'=', b'=') => Some(TokenKind::EqEqEq),
                (b'!', b'=', b'=') => Some(TokenKind::BangEqEq),
                (b'.', b'.', b'.') => Some(TokenKind::DotDotDot),
                (b'?', b'?', b'=') => Some(TokenKind::QuestionQuestionEq),
                (b'|', b'|', b'=') => Some(TokenKind::PipePipeEq),
                (b'&', b'&', b'=') => Some(TokenKind::AmpAmpEq),
                _ => None,
            };
            if let Some(kind) = kind_3 {
                self.advance();
                self.advance();
                self.advance();
                return Some(Token {
                    kind,
                    span: self.span_from(start),
                });
            }
        }

        let kind = match (first, second) {
            (b'=', b'=') => TokenKind::EqEq,
            (b'!', b'=') => TokenKind::BangEq,
            (b'<', b'=') => TokenKind::LtEq,
            (b'>', b'=') => TokenKind::GtEq,
            (b'&', b'&') => TokenKind::AmpAmp,
            (b'|', b'|') => TokenKind::PipePipe,
            (b'*', b'*') => TokenKind::StarStar,
            (b'+', b'=') => TokenKind::PlusEq,
            (b'-', b'=') => TokenKind::MinusEq,
            (b'*', b'=') => TokenKind::StarEq,
            (b'/', b'=') => TokenKind::SlashEq,
            (b'%', b'=') => TokenKind::PercentEq,
            (b'?', b'.') => TokenKind::QuestionDot,
            (b'?', b'?') => TokenKind::QuestionQuestion,
            (b'=', b'>') => TokenKind::FatArrow,
            (b'+', b'+') => TokenKind::PlusPlus,
            (b'-', b'-') => TokenKind::MinusMinus,
            _ => return None,
        };

        self.advance();
        self.advance();
        Some(Token {
            kind,
            span: self.span_from(start),
        })
    }

    /// Map a single byte to its corresponding single-character token kind.
    fn single_char_kind(byte: u8) -> Option<TokenKind> {
        match byte {
            b'+' => Some(TokenKind::Plus),
            b'-' => Some(TokenKind::Minus),
            b'*' => Some(TokenKind::Star),
            b'/' => Some(TokenKind::Slash),
            b'%' => Some(TokenKind::Percent),
            b'!' => Some(TokenKind::Bang),
            b'=' => Some(TokenKind::Eq),
            b'<' => Some(TokenKind::Lt),
            b'>' => Some(TokenKind::Gt),
            b'(' => Some(TokenKind::LParen),
            b')' => Some(TokenKind::RParen),
            b'{' => Some(TokenKind::LBrace),
            b'}' => Some(TokenKind::RBrace),
            b',' => Some(TokenKind::Comma),
            b':' => Some(TokenKind::Colon),
            b';' => Some(TokenKind::Semicolon),
            b'.' => Some(TokenKind::Dot),
            b'|' => Some(TokenKind::Pipe),
            b'[' => Some(TokenKind::LBracket),
            b']' => Some(TokenKind::RBracket),
            b'&' => Some(TokenKind::Ampersand),
            b'^' => Some(TokenKind::Caret),
            b'~' => Some(TokenKind::Tilde),
            b'?' => Some(TokenKind::Question),
            b'@' => Some(TokenKind::At),
            _ => None,
        }
    }
}

/// Strip `JSDoc` comment content: remove leading `*` from each line and trim.
///
/// Given the raw text between `/**` and `*/`, produces clean text with:
/// - Leading/trailing whitespace per line trimmed
/// - Leading `* ` or `*` prefix removed from each line
/// - Leading and trailing blank lines removed
fn strip_jsdoc_content(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    let mut result_lines: Vec<String> = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        // Strip leading `* ` or `*` prefix
        let stripped = if let Some(rest) = trimmed.strip_prefix("* ") {
            rest.to_owned()
        } else if let Some(rest) = trimmed.strip_prefix('*') {
            rest.to_owned()
        } else {
            trimmed.to_owned()
        };
        result_lines.push(stripped);
    }

    // Remove leading blank lines
    while result_lines.first().is_some_and(String::is_empty) {
        result_lines.remove(0);
    }
    // Remove trailing blank lines
    while result_lines.last().is_some_and(String::is_empty) {
        result_lines.pop();
    }

    result_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustscript_syntax::source::FileId;
    use rustscript_syntax::span::Span;

    /// Helper: tokenize source and return tokens (discarding diagnostics).
    fn tokenize(source: &str) -> Vec<Token> {
        let lexer = Lexer::new(source, FileId(0));
        let (tokens, _) = lexer.tokenize();
        tokens
    }

    /// Helper: tokenize source and return both tokens and diagnostics.
    fn tokenize_with_diagnostics(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
        let lexer = Lexer::new(source, FileId(0));
        lexer.tokenize()
    }

    // 1. Tokenize `function` → TokenKind::Function with correct span
    #[test]
    fn test_lexer_keyword_function_produces_function_token() {
        let tokens = tokenize("function");
        assert_eq!(tokens.len(), 2); // Function + Eof
        assert_eq!(tokens[0].kind, TokenKind::Function);
        assert_eq!(tokens[0].span, Span::new(0, 8));
    }

    // 2. Tokenize all keywords — each produces the correct TokenKind
    #[test]
    fn test_lexer_all_keywords_produce_correct_tokens() {
        let cases = [
            ("function", TokenKind::Function),
            ("const", TokenKind::Const),
            ("let", TokenKind::Let),
            ("if", TokenKind::If),
            ("else", TokenKind::Else),
            ("while", TokenKind::While),
            ("do", TokenKind::Do),
            ("return", TokenKind::Return),
            ("true", TokenKind::True),
            ("false", TokenKind::False),
            ("type", TokenKind::Type),
            ("extends", TokenKind::Extends),
            ("var", TokenKind::Var),
        ];

        for (source, expected_kind) in cases {
            let tokens = tokenize(source);
            assert_eq!(
                tokens[0].kind, expected_kind,
                "keyword `{source}` should produce {expected_kind:?}"
            );
        }
    }

    // 3. Tokenize identifier `myVar` → Ident("myVar") with correct span
    #[test]
    fn test_lexer_identifier_produces_ident_token() {
        let tokens = tokenize("myVar");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Ident("myVar".into()));
        assert_eq!(tokens[0].span, Span::new(0, 5));
    }

    // 4. Tokenize integer `42` → IntLit(42) with span length 2
    #[test]
    fn test_lexer_integer_literal_produces_int_lit() {
        let tokens = tokenize("42");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(42));
        assert_eq!(tokens[0].span.len(), 2);
    }

    // 5. Tokenize float `3.14` → FloatLit(3.14)
    #[test]
    fn test_lexer_float_literal_produces_float_lit() {
        let tokens = tokenize("1.25");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::FloatLit(1.25));
    }

    // 6. Tokenize string `"hello"` → StringLit("hello")
    #[test]
    fn test_lexer_string_literal_produces_string_lit() {
        let tokens = tokenize(r#""hello""#);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::StringLit("hello".into()));
    }

    // 7. Tokenize string with escapes `"a\nb"` → StringLit("a\nb")
    #[test]
    fn test_lexer_string_literal_with_escapes() {
        let tokens = tokenize(r#""a\nb""#);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::StringLit("a\nb".into()));
    }

    // 8. Unterminated string `"hello` → diagnostic emitted, lexing continues
    #[test]
    fn test_lexer_unterminated_string_emits_diagnostic() {
        let (tokens, diagnostics) = tokenize_with_diagnostics("\"hello");
        // Should still produce a StringLit token (partial) + Eof
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0].kind, TokenKind::StringLit(_)));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("unterminated string"));
    }

    // 9. All single-char operators
    #[test]
    fn test_lexer_single_char_operators() {
        let cases = [
            ("+", TokenKind::Plus),
            ("-", TokenKind::Minus),
            ("*", TokenKind::Star),
            ("/", TokenKind::Slash),
            ("%", TokenKind::Percent),
            ("!", TokenKind::Bang),
            ("=", TokenKind::Eq),
            ("<", TokenKind::Lt),
            (">", TokenKind::Gt),
        ];

        for (source, expected_kind) in cases {
            let tokens = tokenize(source);
            assert_eq!(
                tokens[0].kind, expected_kind,
                "operator `{source}` should produce {expected_kind:?}"
            );
        }
    }

    // 10. All two-char operators
    #[test]
    fn test_lexer_two_char_operators() {
        let cases = [
            ("==", TokenKind::EqEq),
            ("!=", TokenKind::BangEq),
            ("<=", TokenKind::LtEq),
            (">=", TokenKind::GtEq),
            ("&&", TokenKind::AmpAmp),
            ("||", TokenKind::PipePipe),
        ];

        for (source, expected_kind) in cases {
            let tokens = tokenize(source);
            assert_eq!(
                tokens[0].kind, expected_kind,
                "operator `{source}` should produce {expected_kind:?}"
            );
        }
    }

    // 11. Two-char operators win over single-char: `==` is EqEq, not Eq + Eq
    #[test]
    fn test_lexer_two_char_operator_precedence_over_single() {
        let tokens = tokenize("==");
        // Should be a single EqEq token, not two Eq tokens
        assert_eq!(tokens.len(), 2); // EqEq + Eof
        assert_eq!(tokens[0].kind, TokenKind::EqEq);
    }

    // 12. All delimiters
    #[test]
    fn test_lexer_delimiters() {
        let cases = [
            ("(", TokenKind::LParen),
            (")", TokenKind::RParen),
            ("{", TokenKind::LBrace),
            ("}", TokenKind::RBrace),
            (",", TokenKind::Comma),
            (":", TokenKind::Colon),
            (";", TokenKind::Semicolon),
            (".", TokenKind::Dot),
        ];

        for (source, expected_kind) in cases {
            let tokens = tokenize(source);
            assert_eq!(
                tokens[0].kind, expected_kind,
                "delimiter `{source}` should produce {expected_kind:?}"
            );
        }
    }

    // 13. Skip whitespace between tokens
    #[test]
    fn test_lexer_skip_whitespace_between_tokens() {
        let tokens = tokenize("42 + 3");
        // IntLit(42), Plus, IntLit(3), Eof
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(42));
        assert_eq!(tokens[1].kind, TokenKind::Plus);
        assert_eq!(tokens[2].kind, TokenKind::IntLit(3));
        assert_eq!(tokens[3].kind, TokenKind::Eof);
    }

    // 14. Skip line comments
    #[test]
    fn test_lexer_skip_line_comments() {
        let tokens = tokenize("42 // comment\n3");
        assert_eq!(tokens.len(), 3); // IntLit(42), IntLit(3), Eof
        assert_eq!(tokens[0].kind, TokenKind::IntLit(42));
        assert_eq!(tokens[1].kind, TokenKind::IntLit(3));
    }

    // 15. Skip block comments
    #[test]
    fn test_lexer_skip_block_comments() {
        let tokens = tokenize("42 /* comment */ 3");
        assert_eq!(tokens.len(), 3); // IntLit(42), IntLit(3), Eof
        assert_eq!(tokens[0].kind, TokenKind::IntLit(42));
        assert_eq!(tokens[1].kind, TokenKind::IntLit(3));
    }

    // 16. Invalid character emits diagnostic, lexing continues
    #[test]
    fn test_lexer_invalid_character_emits_diagnostic_and_continues() {
        let (tokens, diagnostics) = tokenize_with_diagnostics("42 \u{00a7} 3");
        // IntLit(42), IntLit(3), Eof — the § is skipped
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(42));
        assert_eq!(tokens[1].kind, TokenKind::IntLit(3));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains('\u{00a7}'));
    }

    // 16b. `@` is a valid token (decorator prefix)
    #[test]
    fn test_lexer_at_sign_produces_at_token() {
        let tokens = tokenize("@test");
        assert_eq!(tokens.len(), 3); // At, Ident("test"), Eof
        assert_eq!(tokens[0].kind, TokenKind::At);
        assert_eq!(tokens[1].kind, TokenKind::Ident("test".into()));
    }

    // 17. Full program tokenization
    #[test]
    fn test_lexer_full_program_tokenization() {
        let source = "function add(a: i32, b: i32): i32 { return a + b; }";
        let tokens = tokenize(source);

        let expected_kinds = vec![
            TokenKind::Function,            // function
            TokenKind::Ident("add".into()), // add
            TokenKind::LParen,              // (
            TokenKind::Ident("a".into()),   // a
            TokenKind::Colon,               // :
            TokenKind::Ident("i32".into()), // i32
            TokenKind::Comma,               // ,
            TokenKind::Ident("b".into()),   // b
            TokenKind::Colon,               // :
            TokenKind::Ident("i32".into()), // i32
            TokenKind::RParen,              // )
            TokenKind::Colon,               // :
            TokenKind::Ident("i32".into()), // i32
            TokenKind::LBrace,              // {
            TokenKind::Return,              // return
            TokenKind::Ident("a".into()),   // a
            TokenKind::Plus,                // +
            TokenKind::Ident("b".into()),   // b
            TokenKind::Semicolon,           // ;
            TokenKind::RBrace,              // }
            TokenKind::Eof,
        ];

        assert_eq!(tokens.len(), expected_kinds.len());
        for (i, (tok, expected)) in tokens.iter().zip(expected_kinds.iter()).enumerate() {
            assert_eq!(&tok.kind, expected, "token {i} mismatch");
        }
    }

    // 18. EOF token is always the last token
    #[test]
    fn test_lexer_eof_always_last_token() {
        for source in ["", "42", "function foo() {}", "// comment only"] {
            let tokens = tokenize(source);
            assert!(
                !tokens.is_empty(),
                "token stream should never be empty for `{source}`"
            );
            assert_eq!(
                tokens.last().map(|t| &t.kind),
                Some(&TokenKind::Eof),
                "last token should be Eof for `{source}`"
            );
        }
    }

    // 19. Spans are byte-accurate
    #[test]
    fn test_lexer_spans_byte_accurate() {
        let source = "let x = 42;";
        let tokens = tokenize(source);

        // let: 0..3
        assert_eq!(tokens[0].kind, TokenKind::Let);
        assert_eq!(tokens[0].span, Span::new(0, 3));
        assert_eq!(&source[0..3], "let");

        // x: 4..5
        assert_eq!(tokens[1].kind, TokenKind::Ident("x".into()));
        assert_eq!(tokens[1].span, Span::new(4, 5));
        assert_eq!(&source[4..5], "x");

        // =: 6..7
        assert_eq!(tokens[2].kind, TokenKind::Eq);
        assert_eq!(tokens[2].span, Span::new(6, 7));
        assert_eq!(&source[6..7], "=");

        // 42: 8..10
        assert_eq!(tokens[3].kind, TokenKind::IntLit(42));
        assert_eq!(tokens[3].span, Span::new(8, 10));
        assert_eq!(&source[8..10], "42");

        // ;: 10..11
        assert_eq!(tokens[4].kind, TokenKind::Semicolon);
        assert_eq!(tokens[4].span, Span::new(10, 11));
        assert_eq!(&source[10..11], ";");
    }

    // 20. Compound assignment operators tokenize correctly
    #[test]
    fn test_lexer_compound_assignment_operators() {
        let cases = [
            ("+=", TokenKind::PlusEq),
            ("-=", TokenKind::MinusEq),
            ("*=", TokenKind::StarEq),
            ("/=", TokenKind::SlashEq),
            ("%=", TokenKind::PercentEq),
        ];

        for (source, expected_kind) in cases {
            let tokens = tokenize(source);
            assert_eq!(
                tokens[0].kind, expected_kind,
                "operator `{source}` should produce {expected_kind:?}"
            );
        }
    }

    // 21. Compound assignment operators are two-char, not single + eq
    #[test]
    fn test_lexer_compound_assign_wins_over_single_char() {
        let tokens = tokenize("+=");
        assert_eq!(tokens.len(), 2); // PlusEq + Eof
        assert_eq!(tokens[0].kind, TokenKind::PlusEq);
    }

    // 22. Empty source → single Eof token
    #[test]
    fn test_lexer_empty_source_produces_single_eof() {
        let tokens = tokenize("");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Eof);
        assert_eq!(tokens[0].span, Span::new(0, 0));
    }

    // 23. Template literal with no interpolation → TemplateNoSub
    #[test]
    fn test_lexer_template_no_sub_produces_template_no_sub() {
        let tokens = tokenize("`hello`");
        assert_eq!(tokens.len(), 2); // TemplateNoSub + Eof
        assert_eq!(tokens[0].kind, TokenKind::TemplateNoSub("hello".into()));
    }

    // 24. Template literal with single interpolation → Head, expr, Tail
    #[test]
    fn test_lexer_template_single_interpolation_produces_head_expr_tail() {
        let tokens = tokenize("`hello ${name}`");
        // TemplateHead("hello "), Ident("name"), TemplateTail(""), Eof
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].kind, TokenKind::TemplateHead("hello ".into()));
        assert_eq!(tokens[1].kind, TokenKind::Ident("name".into()));
        assert!(matches!(tokens[2].kind, TokenKind::TemplateTail(ref s) if s.is_empty()));
    }

    // 25. Template literal with multiple interpolations → Head, expr, Middle, expr, Tail
    #[test]
    fn test_lexer_template_multi_interpolation_produces_head_middle_tail() {
        let tokens = tokenize("`${a} and ${b}`");
        // TemplateHead(""), Ident("a"), TemplateMiddle(" and "), Ident("b"), TemplateTail(""), Eof
        assert_eq!(tokens.len(), 6);
        assert!(matches!(tokens[0].kind, TokenKind::TemplateHead(ref s) if s.is_empty()));
        assert_eq!(tokens[1].kind, TokenKind::Ident("a".into()));
        assert_eq!(tokens[2].kind, TokenKind::TemplateMiddle(" and ".into()));
        assert_eq!(tokens[3].kind, TokenKind::Ident("b".into()));
        assert!(matches!(tokens[4].kind, TokenKind::TemplateTail(ref s) if s.is_empty()));
    }

    // 26. Template literal with expression containing operators
    #[test]
    fn test_lexer_template_expression_with_operators() {
        let tokens = tokenize("`Result: ${a + b}`");
        // TemplateHead("Result: "), Ident("a"), Plus, Ident("b"), TemplateTail(""), Eof
        assert_eq!(tokens.len(), 6);
        assert_eq!(tokens[0].kind, TokenKind::TemplateHead("Result: ".into()));
        assert_eq!(tokens[1].kind, TokenKind::Ident("a".into()));
        assert_eq!(tokens[2].kind, TokenKind::Plus);
        assert_eq!(tokens[3].kind, TokenKind::Ident("b".into()));
        assert!(matches!(tokens[4].kind, TokenKind::TemplateTail(ref s) if s.is_empty()));
    }

    // 27. Template literal with text after last interpolation
    #[test]
    fn test_lexer_template_tail_with_text() {
        let tokens = tokenize("`Hello, ${name}!`");
        // TemplateHead("Hello, "), Ident("name"), TemplateTail("!"), Eof
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].kind, TokenKind::TemplateHead("Hello, ".into()));
        assert_eq!(tokens[1].kind, TokenKind::Ident("name".into()));
        assert_eq!(tokens[2].kind, TokenKind::TemplateTail("!".into()));
    }

    // ---------------------------------------------------------------
    // Task 017: Collection tokens
    // ---------------------------------------------------------------

    // 28. `new` keyword tokenizes correctly
    #[test]
    fn test_lexer_new_keyword_produces_new_token() {
        let tokens = tokenize("new");
        assert_eq!(tokens.len(), 2); // New + Eof
        assert_eq!(tokens[0].kind, TokenKind::New);
    }

    // 29. `[` and `]` tokenize correctly
    #[test]
    fn test_lexer_brackets_produce_bracket_tokens() {
        let tokens = tokenize("[1, 2, 3]");
        assert_eq!(tokens[0].kind, TokenKind::LBracket);
        assert_eq!(tokens[6].kind, TokenKind::RBracket);
    }

    // --- Task 020: null, ?., ?? tokens ---

    // 30. `null` keyword tokenizes correctly
    #[test]
    fn test_lexer_null_keyword_produces_null_token() {
        let tokens = tokenize("null");
        assert_eq!(tokens.len(), 2); // Null + Eof
        assert_eq!(tokens[0].kind, TokenKind::Null);
    }

    // 31. `?.` tokenizes as single QuestionDot token
    #[test]
    fn test_lexer_question_dot_produces_question_dot_token() {
        let tokens = tokenize("x?.name");
        assert_eq!(tokens[1].kind, TokenKind::QuestionDot);
    }

    // 32. `??` tokenizes as single QuestionQuestion token
    #[test]
    fn test_lexer_question_question_produces_question_question_token() {
        let tokens = tokenize("x ?? y");
        assert_eq!(tokens[1].kind, TokenKind::QuestionQuestion);
    }

    // 33. `===` tokenizes as EqEqEq token
    #[test]
    fn test_lexer_triple_eq_produces_eq_eq_eq_token() {
        let tokens = tokenize("x === y");
        assert_eq!(tokens[1].kind, TokenKind::EqEqEq);
    }

    // 34. `!==` tokenizes as BangEqEq token
    #[test]
    fn test_lexer_bang_eq_eq_produces_bang_eq_eq_token() {
        let tokens = tokenize("x !== y");
        assert_eq!(tokens[1].kind, TokenKind::BangEqEq);
    }

    // --- Task 021: throw, throws, try, catch tokens ---

    // 35. `throw` keyword tokenizes correctly
    #[test]
    fn test_lexer_throw_keyword_produces_throw_token() {
        let tokens = tokenize("throw");
        assert_eq!(tokens.len(), 2); // Throw + Eof
        assert_eq!(tokens[0].kind, TokenKind::Throw);
    }

    // 36. `throws` keyword tokenizes correctly
    #[test]
    fn test_lexer_throws_keyword_produces_throws_token() {
        let tokens = tokenize("throws");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Throws);
    }

    // 37. `try` keyword tokenizes correctly
    #[test]
    fn test_lexer_try_keyword_produces_try_token() {
        let tokens = tokenize("try");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Try);
    }

    // 38. `catch` keyword tokenizes correctly
    #[test]
    fn test_lexer_catch_keyword_produces_catch_token() {
        let tokens = tokenize("catch");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Catch);
    }

    // `finally` keyword tokenizes correctly
    #[test]
    fn test_lexer_finally_keyword_produces_finally_token() {
        let tokens = tokenize("finally");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Finally);
    }

    // ---------------------------------------------------------------
    // Task 019: Closures and arrow functions
    // ---------------------------------------------------------------

    // 39. `move` keyword tokenizes correctly
    #[test]
    fn test_lexer_move_keyword_produces_move_token() {
        let tokens = tokenize("move");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Move);
    }

    // 40. `=>` fat arrow tokenizes correctly
    #[test]
    fn test_lexer_fat_arrow_produces_fat_arrow_token() {
        let tokens = tokenize("=>");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::FatArrow);
    }

    // 41. `=>` does not conflict with `>=`
    #[test]
    fn test_lexer_fat_arrow_does_not_conflict_with_ge() {
        let tokens = tokenize(">= =>");
        assert_eq!(tokens.len(), 3); // GtEq, FatArrow, Eof
        assert_eq!(tokens[0].kind, TokenKind::GtEq);
        assert_eq!(tokens[1].kind, TokenKind::FatArrow);
    }

    // 42. `=` followed by `>` not confused with `=>` when separated
    #[test]
    fn test_lexer_eq_gt_separate_from_fat_arrow() {
        let tokens = tokenize("= >");
        assert_eq!(tokens.len(), 3); // Eq, Gt, Eof
        assert_eq!(tokens[0].kind, TokenKind::Eq);
        assert_eq!(tokens[1].kind, TokenKind::Gt);
    }

    // ---------------------------------------------------------------
    // Task 018: For-of loops, break, continue tokens
    // ---------------------------------------------------------------

    // 43. `for` keyword tokenizes correctly
    #[test]
    fn test_lexer_for_keyword_produces_for_token() {
        let tokens = tokenize("for");
        assert_eq!(tokens.len(), 2); // For + Eof
        assert_eq!(tokens[0].kind, TokenKind::For);
    }

    // 44. `of` is a contextual keyword — lexed as an identifier
    #[test]
    fn test_lexer_of_is_identifier_not_keyword() {
        let tokens = tokenize("of");
        assert_eq!(tokens.len(), 2); // Ident + Eof
        assert_eq!(tokens[0].kind, TokenKind::Ident("of".to_owned()));
    }

    // 45. `break` keyword tokenizes correctly
    #[test]
    fn test_lexer_break_keyword_produces_break_token() {
        let tokens = tokenize("break");
        assert_eq!(tokens.len(), 2); // Break + Eof
        assert_eq!(tokens[0].kind, TokenKind::Break);
    }

    // 46. `continue` keyword tokenizes correctly
    #[test]
    fn test_lexer_continue_keyword_produces_continue_token() {
        let tokens = tokenize("continue");
        assert_eq!(tokens.len(), 2); // Continue + Eof
        assert_eq!(tokens[0].kind, TokenKind::Continue);
    }

    // 47. `async` is tokenized as a keyword, not an identifier
    #[test]
    fn test_lexer_async_keyword_produces_async_token() {
        let tokens = tokenize("async");
        assert_eq!(tokens.len(), 2); // Async + Eof
        assert_eq!(tokens[0].kind, TokenKind::Async);
        assert_eq!(tokens[0].span, Span::new(0, 5));
    }

    // 48. `await` is tokenized as a keyword, not an identifier
    #[test]
    fn test_lexer_await_keyword_produces_await_token() {
        let tokens = tokenize("await");
        assert_eq!(tokens.len(), 2); // Await + Eof
        assert_eq!(tokens[0].kind, TokenKind::Await);
        assert_eq!(tokens[0].span, Span::new(0, 5));
    }

    // 49. `async function` tokenizes as two keyword tokens
    #[test]
    fn test_lexer_async_function_produces_two_keyword_tokens() {
        let tokens = tokenize("async function");
        assert_eq!(tokens.len(), 3); // Async + Function + Eof
        assert_eq!(tokens[0].kind, TokenKind::Async);
        assert_eq!(tokens[1].kind, TokenKind::Function);
    }

    // 50. `await fetchData()` tokenizes await as keyword
    #[test]
    fn test_lexer_await_in_expression_context() {
        let tokens = tokenize("await fetchData()");
        assert_eq!(tokens[0].kind, TokenKind::Await);
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "fetchData"));
    }

    // --- Task 055: DotDotDot and Question tokens ---

    // 51. `...` tokenizes as DotDotDot
    #[test]
    fn test_lexer_dot_dot_dot_produces_dot_dot_dot_token() {
        let tokens = tokenize("...args");
        assert_eq!(tokens[0].kind, TokenKind::DotDotDot);
    }

    // 52. `...` is distinguished from single `.`
    #[test]
    fn test_lexer_dot_dot_dot_not_three_dots() {
        let tokens = tokenize("...x");
        assert_eq!(tokens.len(), 3); // DotDotDot + Ident + Eof
        assert_eq!(tokens[0].kind, TokenKind::DotDotDot);
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "x"));
    }

    // 53. Single `.` still produces Dot
    #[test]
    fn test_lexer_single_dot_still_works() {
        let tokens = tokenize("a.b");
        assert_eq!(tokens[1].kind, TokenKind::Dot);
    }

    // 54. `?` standalone produces Question
    #[test]
    fn test_lexer_question_mark_produces_question_token() {
        let tokens = tokenize("x?: string");
        // x, ?, :, string, Eof
        assert!(matches!(tokens[0].kind, TokenKind::Ident(_)));
        assert_eq!(tokens[1].kind, TokenKind::Question);
        assert_eq!(tokens[2].kind, TokenKind::Colon);
    }

    // 55. `?.` still produces QuestionDot (not Question + Dot)
    #[test]
    fn test_lexer_question_dot_still_works() {
        let tokens = tokenize("x?.y");
        assert_eq!(tokens[1].kind, TokenKind::QuestionDot);
    }

    // 56. `??` still produces QuestionQuestion
    #[test]
    fn test_lexer_question_question_still_works() {
        let tokens = tokenize("x ?? y");
        assert_eq!(tokens[1].kind, TokenKind::QuestionQuestion);
    }

    // --- Task 054: Operators and Expressions tokens ---

    // 57. `**` (exponentiation) produces StarStar token
    #[test]
    fn test_lexer_star_star_exponentiation() {
        let tokens = tokenize("2 ** 3");
        assert_eq!(tokens[0].kind, TokenKind::IntLit(2));
        assert_eq!(tokens[1].kind, TokenKind::StarStar);
        assert_eq!(tokens[2].kind, TokenKind::IntLit(3));
    }

    // 58. `^` (bitwise XOR) produces Caret token
    #[test]
    fn test_lexer_caret_bitwise_xor() {
        let tokens = tokenize("a ^ b");
        assert_eq!(tokens[1].kind, TokenKind::Caret);
    }

    // 59. `~` (bitwise NOT) produces Tilde token
    #[test]
    fn test_lexer_tilde_bitwise_not() {
        let tokens = tokenize("~x");
        assert_eq!(tokens[0].kind, TokenKind::Tilde);
    }

    // 60. `as` is tokenized as a keyword
    #[test]
    fn test_lexer_as_keyword() {
        let tokens = tokenize("x as f64");
        assert!(matches!(tokens[0].kind, TokenKind::Ident(ref s) if s == "x"));
        assert_eq!(tokens[1].kind, TokenKind::As);
        assert!(matches!(tokens[2].kind, TokenKind::Ident(ref s) if s == "f64"));
    }

    // 61. `typeof` is tokenized as a keyword
    #[test]
    fn test_lexer_typeof_keyword() {
        let tokens = tokenize("typeof x");
        assert_eq!(tokens[0].kind, TokenKind::TypeOf);
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "x"));
    }

    // 62. `?` (question mark) produces Question token for ternary
    #[test]
    fn test_lexer_question_mark_ternary() {
        let tokens = tokenize("a ? b : c");
        assert!(matches!(tokens[0].kind, TokenKind::Ident(_)));
        assert_eq!(tokens[1].kind, TokenKind::Question);
        assert!(matches!(tokens[2].kind, TokenKind::Ident(_)));
        assert_eq!(tokens[3].kind, TokenKind::Colon);
        assert!(matches!(tokens[4].kind, TokenKind::Ident(_)));
    }

    // 63. `?.` still lexes as QuestionDot, not Question + Dot
    #[test]
    fn test_lexer_question_dot_not_split() {
        let tokens = tokenize("a?.b");
        assert_eq!(tokens[1].kind, TokenKind::QuestionDot);
    }

    // 64. `??` still lexes as QuestionQuestion, not two Questions
    #[test]
    fn test_lexer_question_question_not_split() {
        let tokens = tokenize("a ?? b");
        assert_eq!(tokens[1].kind, TokenKind::QuestionQuestion);
    }

    // ---------------------------------------------------------------
    // Task 063: Logical assignment operator tokens
    // ---------------------------------------------------------------

    // 65a. `??=` tokenizes as QuestionQuestionEq
    #[test]
    fn test_lexer_question_question_eq_token() {
        let tokens = tokenize("x ??= 5");
        assert_eq!(tokens[1].kind, TokenKind::QuestionQuestionEq);
    }

    // 65b. `||=` tokenizes as PipePipeEq
    #[test]
    fn test_lexer_pipe_pipe_eq_token() {
        let tokens = tokenize("x ||= true");
        assert_eq!(tokens[1].kind, TokenKind::PipePipeEq);
    }

    // 65c. `&&=` tokenizes as AmpAmpEq
    #[test]
    fn test_lexer_amp_amp_eq_token() {
        let tokens = tokenize("x &&= check()");
        assert_eq!(tokens[1].kind, TokenKind::AmpAmpEq);
    }

    // 65d. `??=` doesn't split into `??` + `=`
    #[test]
    fn test_lexer_question_question_eq_not_split() {
        let tokens = tokenize("a ??= b");
        assert_eq!(tokens.len(), 4); // Ident ??= Ident Eof
        assert_eq!(tokens[1].kind, TokenKind::QuestionQuestionEq);
    }

    // 65e. `||=` doesn't split into `||` + `=`
    #[test]
    fn test_lexer_pipe_pipe_eq_not_split() {
        let tokens = tokenize("a ||= b");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[1].kind, TokenKind::PipePipeEq);
    }

    // 65f. `&&=` doesn't split into `&&` + `=`
    #[test]
    fn test_lexer_amp_amp_eq_not_split() {
        let tokens = tokenize("a &&= b");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[1].kind, TokenKind::AmpAmpEq);
    }

    // ---------------------------------------------------------------
    // JSDoc comment tests
    // ---------------------------------------------------------------

    // 65. `/** text */` produces a JsDoc token
    #[test]
    fn test_lexer_jsdoc_single_line_produces_jsdoc_token() {
        let tokens = tokenize("/** Hello world */\nfunction foo() {}");
        assert!(
            matches!(&tokens[0].kind, TokenKind::JsDoc(text) if text == "Hello world"),
            "Expected JsDoc token, got {:?}",
            tokens[0].kind
        );
        assert_eq!(tokens[1].kind, TokenKind::Function);
    }

    // 66. Regular `/* text */` block comment is skipped
    #[test]
    fn test_lexer_block_comment_is_skipped() {
        let tokens = tokenize("/* regular comment */\nfunction foo() {}");
        assert_eq!(tokens[0].kind, TokenKind::Function);
    }

    // 67. Multi-line JSDoc preserves content with * stripping
    #[test]
    fn test_lexer_jsdoc_multiline_strips_stars() {
        let tokens = tokenize(
            "/**\n * First line\n * Second line\n * @param x - the value\n */\nfunction foo() {}",
        );
        if let TokenKind::JsDoc(text) = &tokens[0].kind {
            assert!(text.contains("First line"), "Missing first line: {text}");
            assert!(text.contains("Second line"), "Missing second line: {text}");
            assert!(
                text.contains("@param x - the value"),
                "Missing param tag: {text}"
            );
        } else {
            panic!("Expected JsDoc token, got {:?}", tokens[0].kind);
        }
    }

    // 68. `/**/` (empty block comment) is NOT treated as JSDoc
    #[test]
    fn test_lexer_empty_block_comment_not_jsdoc() {
        let tokens = tokenize("/**/\nfunction foo() {}");
        // `/**/` is an empty block comment, should be skipped
        assert_eq!(tokens[0].kind, TokenKind::Function);
    }

    // 69. JSDoc followed immediately by declaration
    #[test]
    fn test_lexer_jsdoc_followed_by_function() {
        let tokens = tokenize("/** docs */function foo() {}");
        assert!(matches!(&tokens[0].kind, TokenKind::JsDoc(_)));
        assert_eq!(tokens[1].kind, TokenKind::Function);
    }

    // --- Hex, binary, and octal integer literals ---

    // 70. Hex literal `0xFF` → IntLit(255)
    #[test]
    fn test_lexer_hex_literal_lowercase() {
        let tokens = tokenize("0xff");
        assert_eq!(tokens.len(), 2); // IntLit, Eof
        assert_eq!(tokens[0].kind, TokenKind::IntLit(255));
    }

    // 71. Hex literal `0XFF` → IntLit(255) (uppercase prefix)
    #[test]
    fn test_lexer_hex_literal_uppercase() {
        let tokens = tokenize("0XFF");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(255));
    }

    // 72. Binary literal `0b1100` → IntLit(12)
    #[test]
    fn test_lexer_binary_literal() {
        let tokens = tokenize("0b1100");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(12));
    }

    // 73. Binary literal `0B1010` → IntLit(10) (uppercase prefix)
    #[test]
    fn test_lexer_binary_literal_uppercase() {
        let tokens = tokenize("0B1010");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(10));
    }

    // 74. Octal literal `0o77` → IntLit(63)
    #[test]
    fn test_lexer_octal_literal() {
        let tokens = tokenize("0o77");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(63));
    }

    // 75. Octal literal `0O10` → IntLit(8) (uppercase prefix)
    #[test]
    fn test_lexer_octal_literal_uppercase() {
        let tokens = tokenize("0O10");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(8));
    }

    // 76. Hex literal in expression: `0xFF + 1`
    #[test]
    fn test_lexer_hex_literal_in_expression() {
        let tokens = tokenize("0xFF + 1");
        assert_eq!(tokens[0].kind, TokenKind::IntLit(255));
        assert_eq!(tokens[1].kind, TokenKind::Plus);
        assert_eq!(tokens[2].kind, TokenKind::IntLit(1));
    }

    // 77. Plain `0` still works as IntLit(0)
    #[test]
    fn test_lexer_zero_still_works() {
        let tokens = tokenize("0");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(0));
    }

    // --- Task 067: Minor Syntax Completions ---

    // 78. `abstract` is tokenized as a keyword
    #[test]
    fn test_lexer_abstract_keyword() {
        let tokens = tokenize("abstract");
        assert_eq!(tokens[0].kind, TokenKind::Abstract);
    }

    // 79. `override` is tokenized as a keyword
    #[test]
    fn test_lexer_override_keyword() {
        let tokens = tokenize("override");
        assert_eq!(tokens[0].kind, TokenKind::Override);
    }

    // 80. `satisfies` is tokenized as a keyword
    #[test]
    fn test_lexer_satisfies_keyword() {
        let tokens = tokenize("satisfies");
        assert_eq!(tokens[0].kind, TokenKind::Satisfies);
    }

    // `derives` is tokenized as a keyword
    #[test]
    fn test_lexer_derives_keyword() {
        let tokens = tokenize("derives");
        assert_eq!(tokens[0].kind, TokenKind::Derives);
    }

    // 81. `#field` is tokenized as an identifier with # prefix
    #[test]
    fn test_lexer_hash_private_field() {
        let tokens = tokenize("#password");
        assert_eq!(tokens[0].kind, TokenKind::Ident("#password".to_owned()));
    }

    // 82. `#` alone (not followed by identifier) is not a hash-private identifier
    #[test]
    fn test_lexer_hash_alone_is_invalid() {
        let (tokens, diagnostics) = Lexer::new("# ", FileId(0)).tokenize();
        // Should produce a diagnostic for unexpected character
        assert!(
            !diagnostics.is_empty(),
            "bare # should produce a diagnostic"
        );
        // The hash should not produce an Ident token
        assert!(
            !tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::Ident(n) if n.starts_with('#'))),
            "bare # should not produce a hash-ident"
        );
    }

    // 77. `keyof` is tokenized as a keyword
    #[test]
    fn test_lexer_keyof_keyword() {
        let tokens = tokenize("keyof User");
        assert_eq!(tokens[0].kind, TokenKind::KeyOf);
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "User"));
    }

    // `declare` is tokenized as a keyword
    #[test]
    fn test_lexer_declare_keyword() {
        let tokens = tokenize("declare");
        assert_eq!(tokens[0].kind, TokenKind::Declare);
    }
}
