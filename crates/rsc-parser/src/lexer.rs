//! Lexer for `RustScript` source files.
//!
//! Takes raw source text and produces a stream of [`Token`]s with accurate
//! source spans. The lexer recovers from invalid characters by emitting a
//! diagnostic and continuing, so the token stream is always complete.

use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::source::FileId;
use rsc_syntax::span::Span;

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
            self.skip_whitespace_and_comments();

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
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while let Some(b) = self.peek() {
                if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                    self.advance();
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

        // Number literal
        if byte.is_ascii_digit() {
            return Some(self.lex_number(start));
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

        // Invalid character — emit diagnostic, advance, return None
        self.advance();
        let span = self.span_from(start);
        let ch = self.source[start..self.pos]
            .chars()
            .next()
            .unwrap_or(char::REPLACEMENT_CHARACTER);
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

    /// Lex a numeric literal (integer or float).
    fn lex_number(&mut self, start: usize) -> Token {
        // Consume all leading digits
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
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "return" => TokenKind::Return,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            _ => TokenKind::Ident(text.to_owned()),
        };

        Token {
            kind,
            span: self.span_from(start),
        }
    }

    /// Try to lex a two-character operator. Returns `None` if the current
    /// position does not start a two-character operator.
    fn try_two_char_operator(&mut self, start: usize) -> Option<Token> {
        let first = self.peek()?;
        let second = self.peek_next()?;

        let kind = match (first, second) {
            (b'=', b'=') => TokenKind::EqEq,
            (b'!', b'=') => TokenKind::BangEq,
            (b'<', b'=') => TokenKind::LtEq,
            (b'>', b'=') => TokenKind::GtEq,
            (b'&', b'&') => TokenKind::AmpAmp,
            (b'|', b'|') => TokenKind::PipePipe,
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
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::source::FileId;
    use rsc_syntax::span::Span;

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

    // 2. Tokenize all 9 keywords — each produces the correct TokenKind
    #[test]
    fn test_lexer_all_keywords_produce_correct_tokens() {
        let cases = [
            ("function", TokenKind::Function),
            ("const", TokenKind::Const),
            ("let", TokenKind::Let),
            ("if", TokenKind::If),
            ("else", TokenKind::Else),
            ("while", TokenKind::While),
            ("return", TokenKind::Return),
            ("true", TokenKind::True),
            ("false", TokenKind::False),
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

    // 16. Invalid character `@` → diagnostic, lexing continues
    #[test]
    fn test_lexer_invalid_character_emits_diagnostic_and_continues() {
        let (tokens, diagnostics) = tokenize_with_diagnostics("42 @ 3");
        // IntLit(42), IntLit(3), Eof — the @ is skipped
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(42));
        assert_eq!(tokens[1].kind, TokenKind::IntLit(3));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains('@'));
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

    // 20. Empty source → single Eof token
    #[test]
    fn test_lexer_empty_source_produces_single_eof() {
        let tokens = tokenize("");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Eof);
        assert_eq!(tokens[0].span, Span::new(0, 0));
    }
}
