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
    /// `new`
    New,
    /// `null`
    Null,
    /// `throw`
    Throw,
    /// `throws`
    Throws,
    /// `try`
    Try,
    /// `catch`
    Catch,
    /// `finally`
    Finally,
    /// `move`
    Move,
    /// `interface`
    Interface,
    /// `for`
    For,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `import`
    Import,
    /// `export`
    Export,
    /// `from`
    From,
    /// `class`
    Class,
    /// `constructor`
    Constructor,
    /// `this`
    This,
    /// `private`
    Private,
    /// `public`
    Public,
    /// `implements`
    Implements,
    /// `async`
    Async,
    /// `await`
    Await,
    /// `rust` (introduces an inline Rust block)
    Rust,

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
    /// `?.` (optional chaining operator)
    QuestionDot,
    /// `??` (nullish coalescing operator)
    QuestionQuestion,
    /// `===` (strict equality — treated same as `==`)
    EqEqEq,
    /// `!==` (strict inequality — treated same as `!=`)
    BangEqEq,
    /// `=>` (fat arrow, used in arrow functions / closures)
    FatArrow,
    /// `&` (ampersand, used in intersection types and bitwise AND)
    Ampersand,
    /// `...` (spread/rest operator)
    DotDotDot,
    /// `**` (exponentiation operator)
    StarStar,
    /// `^` (bitwise XOR)
    Caret,
    /// `~` (bitwise NOT)
    Tilde,
    /// `as` (type casting keyword)
    As,
    /// `typeof` (type query operator)
    TypeOf,
    /// `abstract` (abstract class/method keyword)
    Abstract,
    /// `override` (method override keyword)
    Override,
    /// `satisfies` (type satisfaction check)
    Satisfies,
    /// `?` (question mark, used in optional parameters and ternary operator)
    Question,

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
    /// `[`
    LBracket,
    /// `]`
    RBracket,

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
    /// A `JSDoc` comment (`/** ... */`) with delimiters and leading `*` stripped.
    JsDoc(String),
    /// End of file marker — always the last token in the stream.
    Eof,
}
