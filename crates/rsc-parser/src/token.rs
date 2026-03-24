//! Token types produced by the lexer.
//!
//! These types are internal to `rsc-parser` — the parser consumes them
//! but they are not part of the crate's public API.

use rsc_syntax::span::Span;

/// A single token produced by the lexer, carrying its kind and source span.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    /// What kind of token this is (keyword, literal, operator, etc.).
    pub kind: TokenKind,
    /// The byte range in the source file that this token covers.
    pub span: Span,
}

/// The kind of a lexed token.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenKind {
    // Literals
    /// An integer literal parsed as `i64`.
    IntLit(i64),
    /// A floating-point literal parsed as `f64`.
    FloatLit(f64),
    /// A string literal with escape sequences already resolved.
    StringLit(String),

    // Identifier
    /// An identifier (variable name, type name, etc.).
    Ident(String),

    // Keywords
    /// `function`
    Function,
    /// `const`
    Const,
    /// `let`
    Let,
    /// `if`
    If,
    /// `else`
    Else,
    /// `while`
    While,
    /// `return`
    Return,
    /// `true`
    True,
    /// `false`
    False,
    /// `type`
    Type,
    /// `extends`
    Extends,
    /// `switch`
    Switch,
    /// `case`
    Case,

    // Operators
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `==`
    EqEq,
    /// `!=`
    BangEq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    LtEq,
    /// `>=`
    GtEq,
    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,
    /// `!`
    Bang,
    /// `=`
    Eq,
    /// `+=`
    PlusEq,
    /// `-=`
    MinusEq,
    /// `*=`
    StarEq,
    /// `/=`
    SlashEq,
    /// `%=`
    PercentEq,
    /// `|` (used in union type syntax)
    Pipe,

    // Delimiters
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `:`
    Colon,
    /// `;`
    Semicolon,
    /// `.`
    Dot,

    // Template literals
    /// The start of a template literal: `` `text${ `` — the string before the first interpolation.
    TemplateHead(String),
    /// A middle segment: `}text${ ` — string between interpolations.
    TemplateMiddle(String),
    /// The end of a template literal: `` }text` `` — the string after the last interpolation.
    TemplateTail(String),
    /// A template literal with no interpolations: `` `text` ``.
    TemplateNoSub(String),

    // Special
    /// End of file marker — always the last token in the stream.
    Eof,
}
