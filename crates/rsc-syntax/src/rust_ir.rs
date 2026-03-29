//! Rust IR type definitions.
//!
//! Covers the full Rust output surface through Phase 3: functions (sync and async),
//! structs, enums, traits, impl blocks, closures, iterator chains, Result/Option
//! wrapping, match statements, source map spans, and all expression/statement forms.
//!
//! The Rust IR is a separate type hierarchy closer to actual Rust syntax than
//! the `RustScript` AST. The lowering pass transforms the AST into IR, and the
//! emitter produces `.rs` source text from it. IR nodes carry `Option<Span>` —
//! `Some` when derived from source, `None` when compiler-generated.

use crate::span::Span;

/// A complete Rust source file, the root of the Rust IR.
///
/// Produced by the lowering pass, consumed by the emitter.
#[derive(Debug, Clone)]
pub struct RustFile {
    /// `use` declarations at the top of the file.
    /// Populated by Tasks 017 and 024.
    pub uses: Vec<RustUseDecl>,
    /// `mod` declarations at the top of the file.
    /// Populated by Task 024.
    pub mod_decls: Vec<RustModDecl>,
    /// The top-level items in this file.
    pub items: Vec<RustItem>,
    /// Optional test module produced by `test()` / `describe()` / `it()` blocks.
    /// Emitted as `#[cfg(test)] mod tests { use super::*; ... }`.
    pub test_module: Option<RustTestModule>,
}

/// A Rust `use` declaration.
///
/// Represents `[pub] use path;` in the generated source.
#[derive(Debug, Clone)]
pub struct RustUseDecl {
    /// The use path (e.g., `"std::collections::HashMap"`).
    pub path: String,
    /// Whether this is a `pub use` declaration (for re-exports).
    pub public: bool,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `mod` declaration.
///
/// Represents `[pub] mod name;` in the generated source.
#[derive(Debug, Clone)]
pub struct RustModDecl {
    /// The module name.
    pub name: String,
    /// Whether this is a `pub mod` declaration.
    pub public: bool,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A top-level item in a Rust file.
///
/// Supports function declarations, struct definitions, enum definitions,
/// trait definitions, and impl blocks.
#[derive(Debug, Clone)]
pub enum RustItem {
    /// A `fn` declaration.
    Function(RustFnDecl),
    /// A `struct` definition.
    Struct(RustStructDef),
    /// An `enum` definition.
    Enum(RustEnumDef),
    /// A `trait` definition.
    Trait(RustTraitDef),
    /// An inherent impl block: `impl TypeName { ... }`.
    Impl(RustImplBlock),
    /// A trait impl block: `impl TraitName for TypeName { ... }`.
    TraitImpl(RustTraitImplBlock),
    /// A raw Rust code block at the top level. The contents are emitted verbatim.
    RawRust(String),
    /// A module-level `const` declaration: `const NAME: Type = value;`.
    /// Produced by lowering top-level `const` declarations in `RustScript`.
    Const(RustConstItem),
    /// A type alias: `type Name = Type;`.
    /// Produced by lowering pure index signature types:
    /// `type Config = { [key: string]: string }` → `type Config = HashMap<String, String>;`
    TypeAlias(RustTypeAlias),
}

/// A Rust type alias: `type Name = Type;`.
///
/// Produced by lowering pure index signature type definitions:
/// `type Config = { [key: string]: string }` → `type Config = HashMap<String, String>;`
#[derive(Debug, Clone)]
pub struct RustTypeAlias {
    /// Whether this type alias is `pub`.
    pub public: bool,
    /// The alias name.
    pub name: String,
    /// The aliased type.
    pub ty: RustType,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `const` item at module level.
///
/// Produced by lowering top-level `const` declarations in `RustScript`.
/// Emits as `const NAME: Type = value;` in the generated `.rs` file.
#[derive(Debug, Clone)]
pub struct RustConstItem {
    /// Whether this const is `pub`.
    pub public: bool,
    /// The constant name (`SCREAMING_SNAKE_CASE` by convention).
    pub name: String,
    /// The type of the constant.
    pub ty: RustType,
    /// The initializer expression.
    pub init: RustExpr,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A test module: `#[cfg(test)] mod tests { ... }`.
///
/// Produced by lowering `test()`, `describe()`, and `it()` blocks from
/// `RustScript` source. All test blocks in a file are collected into a single
/// test module.
#[derive(Debug, Clone)]
pub struct RustTestModule {
    /// Test items: functions and nested `describe` modules.
    pub items: Vec<RustTestItem>,
}

/// An item inside a test module.
///
/// Either a `#[test]` function or a nested `mod` block (from `describe`).
#[derive(Debug, Clone)]
pub enum RustTestItem {
    /// A `#[test] fn name() { ... }` test function.
    TestFn(RustTestFn),
    /// A nested module from `describe("name", () => { ... })`.
    DescribeModule(RustDescribeModule),
}

/// A `#[test]` function inside a test module.
///
/// Produced by lowering `test("name", () => { ... })` or `it("name", () => { ... })`.
#[derive(Debug, Clone)]
pub struct RustTestFn {
    /// The sanitized function name (derived from the test description string).
    pub name: String,
    /// The function body.
    pub body: RustBlock,
}

/// A nested module inside a test module, from `describe("name", ...)`.
///
/// Contains `#[test]` functions and further nested `describe` modules.
#[derive(Debug, Clone)]
pub struct RustDescribeModule {
    /// The sanitized module name (derived from the describe description string).
    pub name: String,
    /// The items inside this describe block.
    pub items: Vec<RustTestItem>,
}

/// A generic type parameter in Rust: `T` or `T: Bound`.
///
/// Produced by lowering a `RustScript` [`TypeParam`](crate::ast::TypeParam).
#[derive(Debug, Clone)]
pub struct RustTypeParam {
    /// The type parameter name (e.g., `T`).
    pub name: String,
    /// Trait bound names (e.g., `["Clone", "PartialOrd"]`).
    pub bounds: Vec<String>,
}

/// A Rust struct definition.
///
/// Produced by lowering a `RustScript` [`TypeDef`](crate::ast::TypeDef).
#[derive(Debug, Clone)]
pub struct RustStructDef {
    /// Whether this struct is `pub` (exported from the module).
    pub public: bool,
    /// The struct name.
    pub name: String,
    /// Generic type parameters on the struct.
    pub type_params: Vec<RustTypeParam>,
    /// The fields of the struct.
    pub fields: Vec<RustFieldDef>,
    /// Derive macros to apply: `#[derive(Debug, Clone, ...)]`.
    pub derives: Vec<String>,
    /// Outer attributes on the struct (e.g., `#[serde(rename_all = "camelCase")]`).
    /// Populated from `@decorator` syntax in `RustScript` source.
    pub attributes: Vec<RustAttribute>,
    /// Doc comment from `JSDoc`, if any.
    pub doc_comment: Option<String>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A field in a Rust struct.
///
/// Each field has a name, type, and visibility.
#[derive(Debug, Clone)]
pub struct RustFieldDef {
    /// Whether this field is `pub`.
    pub public: bool,
    /// The field name.
    pub name: String,
    /// The field type.
    pub ty: RustType,
    /// Doc comment from `JSDoc`, if any.
    pub doc_comment: Option<String>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust enum definition.
///
/// Produced by lowering a `RustScript` [`EnumDef`](crate::ast::EnumDef).
/// Supports both fieldless variants (simple enums) and struct variants (data enums).
#[derive(Debug, Clone)]
pub struct RustEnumDef {
    /// Whether this enum is `pub` (exported from the module).
    pub public: bool,
    /// The enum name.
    pub name: String,
    /// The variants of the enum.
    pub variants: Vec<RustEnumVariant>,
    /// Derive macros to apply: `#[derive(Debug, Clone, ...)]`.
    pub derives: Vec<String>,
    /// Outer attributes on the enum (e.g., `#[serde(tag = "type")]`).
    /// Populated from `@decorator` syntax in `RustScript` source.
    pub attributes: Vec<RustAttribute>,
    /// Doc comment from `JSDoc`, if any.
    pub doc_comment: Option<String>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust enum variant.
///
/// Fieldless for simple enums, with named fields for data enums,
/// or with tuple types for generated union enums.
#[derive(Debug, Clone)]
pub struct RustEnumVariant {
    /// The variant name (e.g., `North`, `Circle`, `String`).
    pub name: String,
    /// Named fields for struct-style variants. Empty for simple or tuple variants.
    pub fields: Vec<RustFieldDef>,
    /// Positional types for tuple-style variants (e.g., `String(String)`).
    /// Empty for simple or struct-style variants.
    pub tuple_types: Vec<RustType>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust trait definition.
///
/// Produced by lowering a `RustScript` [`InterfaceDef`](crate::ast::InterfaceDef).
#[derive(Debug, Clone)]
pub struct RustTraitDef {
    /// Whether this trait is `pub` (exported from the module).
    pub public: bool,
    /// The trait name.
    pub name: String,
    /// Generic type parameters on the trait.
    pub type_params: Vec<RustTypeParam>,
    /// The method signatures in this trait.
    pub methods: Vec<RustTraitMethod>,
    /// Doc comment from `JSDoc`, if any.
    pub doc_comment: Option<String>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A method signature in a Rust trait.
///
/// Produced by lowering a `RustScript` [`InterfaceMethod`](crate::ast::InterfaceMethod)
/// or an abstract class definition.
#[derive(Debug, Clone)]
pub struct RustTraitMethod {
    /// The method name.
    pub name: String,
    /// The parameter list (excluding `&self`).
    pub params: Vec<RustParam>,
    /// The return type, if not unit.
    pub return_type: Option<RustType>,
    /// Whether the method takes `&self` as the first parameter.
    pub has_self: bool,
    /// Optional default method body (for concrete methods in abstract classes).
    pub default_body: Option<RustBlock>,
    /// Doc comment from `JSDoc`, if any.
    pub doc_comment: Option<String>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// An inherent impl block: `impl TypeName { ... }`.
///
/// Produced by lowering a `RustScript` [`ClassDef`](crate::ast::ClassDef).
#[derive(Debug, Clone)]
pub struct RustImplBlock {
    /// The type name (e.g., `Counter`).
    pub type_name: String,
    /// Generic type parameters on the impl block.
    pub type_params: Vec<RustTypeParam>,
    /// Associated constants in this impl block (from static fields).
    pub associated_consts: Vec<RustConstItem>,
    /// The methods in this impl block.
    pub methods: Vec<RustMethod>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A trait impl block: `impl TraitName for TypeName { ... }`.
///
/// Produced by lowering a class that `implements` an interface,
/// or by lowering a generator function into an `Iterator` impl.
#[derive(Debug, Clone)]
pub struct RustTraitImplBlock {
    /// The trait name (e.g., `Describable`, `Iterator`).
    pub trait_name: String,
    /// The implementing type name (e.g., `User`, `RangeIter`).
    pub type_name: String,
    /// Generic type parameters on the impl block.
    pub type_params: Vec<RustTypeParam>,
    /// Associated types in the trait impl (e.g., `type Item = i32;`).
    pub associated_types: Vec<(String, RustType)>,
    /// The methods implementing the trait.
    pub methods: Vec<RustMethod>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A method in an impl block.
///
/// Unlike [`RustTraitMethod`] which is a signature only, this has a body.
#[derive(Debug, Clone)]
pub struct RustMethod {
    /// Whether this is an `async` method.
    pub is_async: bool,
    /// The method name.
    pub name: String,
    /// The self parameter (`&self`, `&mut self`, or none for associated functions).
    pub self_param: Option<RustSelfParam>,
    /// The parameter list (excluding `self`).
    pub params: Vec<RustParam>,
    /// The return type, if not unit.
    pub return_type: Option<RustType>,
    /// The method body.
    pub body: RustBlock,
    /// Doc comment from `JSDoc`, if any.
    pub doc_comment: Option<String>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// The self parameter on a method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustSelfParam {
    /// `&self` — immutable borrow.
    Ref,
    /// `&mut self` — mutable borrow.
    RefMut,
}

/// A Rust attribute: `#[name]` or `#[name(args)]`.
///
/// Used for outer attributes on function declarations, such as `#[tokio::main]`.
#[derive(Debug, Clone)]
pub struct RustAttribute {
    /// The attribute path (e.g., `"tokio::main"`).
    pub path: String,
    /// Optional parenthesized arguments (e.g., `"flavor = \"current_thread\""` for
    /// `#[tokio::main(flavor = "current_thread")]`).
    pub args: Option<String>,
}

/// A Rust function declaration.
///
/// Produced by lowering a `RustScript` [`FnDecl`](crate::ast::FnDecl).
#[derive(Debug, Clone)]
pub struct RustFnDecl {
    /// Outer attributes on the function (e.g., `#[tokio::main]`).
    pub attributes: Vec<RustAttribute>,
    /// Whether this is an `async fn`.
    pub is_async: bool,
    /// Whether this function is `pub` (exported from the module).
    pub public: bool,
    /// The function name.
    pub name: String,
    /// Generic type parameters on the function.
    pub type_params: Vec<RustTypeParam>,
    /// The parameter list.
    pub params: Vec<RustParam>,
    /// The return type, if not unit.
    pub return_type: Option<RustType>,
    /// The function body.
    pub body: RustBlock,
    /// Doc comment from `JSDoc`, if any.
    pub doc_comment: Option<String>,
    /// The source span of the original `RustScript` function, if applicable.
    pub span: Option<Span>,
}

/// How a function parameter is passed — owned or borrowed.
///
/// Tier 1 uses `Owned` for everything. Tier 2 analysis may upgrade
/// parameters to `Borrowed` or `BorrowedStr` when safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamMode {
    /// Parameter takes ownership: `name: String`, `items: Vec<T>`.
    Owned,
    /// Parameter borrows immutably: `name: &String`, `items: &Vec<T>`.
    Borrowed,
    /// String parameter borrows as str slice: `name: &str`.
    /// This is the most impactful optimization — avoids allocation.
    BorrowedStr,
}

/// A Rust function parameter.
///
/// The `mode` field indicates whether this parameter should be emitted as
/// owned (`T`), borrowed (`&T`), or as a str slice (`&str`). The emitter
/// in Task 046 will use `mode` to emit the appropriate reference prefix.
/// Until then, the emitter ignores `mode` and always emits `T`.
#[derive(Debug, Clone)]
pub struct RustParam {
    /// The parameter name.
    pub name: String,
    /// The parameter type.
    pub ty: RustType,
    /// How this parameter is passed (owned vs borrowed).
    pub mode: ParamMode,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// Rust types available in the IR.
///
/// Each variant corresponds to a concrete Rust type used in emitted code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustType {
    /// Rust `i8`.
    I8,
    /// Rust `i16`.
    I16,
    /// Rust `i32`.
    I32,
    /// Rust `i64`.
    I64,
    /// Rust `u8`.
    U8,
    /// Rust `u16`.
    U16,
    /// Rust `u32`.
    U32,
    /// Rust `u64`.
    U64,
    /// Rust `f32`.
    F32,
    /// Rust `f64`.
    F64,
    /// Rust `bool`.
    Bool,
    /// Rust `String`.
    String,
    /// Rust unit type `()`.
    Unit,
    /// A user-defined named type (e.g., `User`, `Point`).
    Named(String),
    /// A generic type instantiation: `Vec<String>`, `HashMap<String, u32>`.
    Generic(Box<RustType>, Vec<RustType>),
    /// A type parameter reference: `T`.
    TypeParam(String),
    /// `Option<T>` — from `T | null`.
    Option(Box<RustType>),
    /// `Result<T, E>` — from `T throws E`.
    Result(Box<RustType>, Box<RustType>),
    /// Function/closure type: `impl Fn(i32) -> i32`.
    /// Produced by lowering `(i32) => i32` in parameter position.
    ImplFn(Vec<RustType>, Box<RustType>),
    /// The `Self` type in trait method signatures.
    /// Produced by lowering `Self` in interface method return types.
    SelfType,
    /// The inferred type placeholder `_`.
    /// Used in turbofish positions like `.collect::<Vec<_>>()`.
    Infer,
    /// `Arc<Mutex<T>>` — from `shared<T>`.
    /// Keeps the inner type explicit for derive analysis and display.
    ArcMutex(Box<RustType>),
    /// Tuple type: `(T1, T2, ...)`.
    /// Produced by lowering `[T1, T2]` tuple type annotations.
    Tuple(Vec<RustType>),
    /// Dynamic trait reference: `&dyn TraitName`.
    /// Produced when a function parameter has a base class type that is extended,
    /// enabling polymorphic dispatch.
    DynRef(String),
    /// Auto-generated union enum type: `StringOrI32`.
    /// Produced by lowering `string | i32` (non-null general union types).
    /// The `name` is the deterministic enum name (e.g., `"StringOrI32"`),
    /// and `variants` contains `(VariantName, InnerType)` pairs.
    GeneratedUnion {
        /// The generated enum name (e.g., `StringOrI32`).
        name: String,
        /// The variants: `(VariantName, InnerType)` pairs.
        variants: Vec<(String, RustType)>,
    },
}

impl std::fmt::Display for RustType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Bool => "bool",
            Self::String => "String",
            Self::Unit => "()",
            Self::Named(name) | Self::TypeParam(name) | Self::GeneratedUnion { name, .. } => {
                return f.write_str(name);
            }
            Self::DynRef(trait_name) => return write!(f, "&dyn {trait_name}"),
            Self::Generic(base, args) => {
                write!(f, "{base}<")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                return write!(f, ">");
            }
            Self::Option(inner) => return write!(f, "Option<{inner}>"),
            Self::Result(ok, err) => return write!(f, "Result<{ok}, {err}>"),
            Self::ImplFn(params, ret) => {
                write!(f, "impl Fn(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{param}")?;
                }
                return write!(f, ") -> {ret}");
            }
            Self::SelfType => "Self",
            Self::Infer => "_",
            Self::ArcMutex(inner) => return write!(f, "Arc<Mutex<{inner}>>"),
            Self::Tuple(types) => {
                write!(f, "(")?;
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{ty}")?;
                }
                return write!(f, ")");
            }
        };
        f.write_str(s)
    }
}

/// A block of Rust statements with an optional trailing expression.
///
/// In Rust, a block can return a value via a trailing expression without a
/// semicolon. The `expr` field captures this.
#[derive(Debug, Clone)]
pub struct RustBlock {
    /// The statements in the block.
    pub stmts: Vec<RustStmt>,
    /// Optional trailing expression (Rust blocks return last value without semicolon).
    pub expr: Option<Box<RustExpr>>,
}

/// A Rust statement.
#[derive(Debug, Clone)]
pub enum RustStmt {
    /// A `let` binding (with optional `mut`).
    Let(RustLetStmt),
    /// An expression without a trailing semicolon (used as trailing block expr).
    Expr(RustExpr),
    /// An expression with a trailing semicolon.
    Semi(RustExpr),
    /// A `return` statement.
    Return(RustReturnStmt),
    /// An `if`/`else` statement.
    If(RustIfStmt),
    /// A `while` loop.
    While(RustWhileStmt),
    /// Destructuring let: `let TypeName { field1, field2, .. } = expr;`.
    Destructure(RustDestructureStmt),
    /// A `match` statement.
    Match(RustMatchStmt),
    /// An `if let Some(name) = expr { then } [else { else }]` statement.
    /// Produced by lowering null check narrowing (`if (x !== null)`).
    IfLet(RustIfLetStmt),
    /// `match expr { Ok(val) => { ... }, Err(e) => { ... } }`.
    /// Produced by lowering `try/catch` blocks.
    MatchResult(RustMatchResultStmt),
    /// A `for x in iter { ... }` loop.
    /// Produced by lowering `for (const x of items) { ... }`.
    ForIn(RustForInStmt),
    /// A `let Some(name) = expr else { diverging_block };` statement.
    /// Produced by lowering null-guard patterns where `if (x === null) { throw/return; }`
    /// narrows `x` to non-null in the continuation scope.
    LetElse(RustLetElseStmt),
    /// Tuple destructuring: `let (a, b, c) = expr;`.
    /// Produced by lowering `const [a, b] = await Promise.all([...])`.
    TupleDestructure(RustTupleDestructureStmt),
    /// Destructuring with defaults: individual `let` bindings with `unwrap_or_else`.
    /// Produced when destructuring fields have default values.
    DestructureDefaults(RustDestructureDefaultsStmt),
    /// A `break;` statement.
    Break(Option<Span>),
    /// A `continue;` statement.
    Continue(Option<Span>),
    /// A raw Rust code block in a function body. The contents are emitted verbatim.
    RawRust(String),
    /// A `try {} finally {}` block (no catch). The try body is followed by
    /// finally cleanup statements, all emitted within a single block.
    TryFinally(RustTryFinallyStmt),
    /// A `while let Some(binding) = expr.next().await { body }` loop.
    /// Produced by lowering `for await (const item of stream) { ... }`.
    WhileLet(RustWhileLetStmt),
    /// An infinite `loop { ... }` with an internal break condition.
    /// Produced by lowering `do { ... } while (condition)`.
    Loop(RustLoopStmt),
}

/// A `try {} finally {}` block without catch.
///
/// Produced by lowering `try { body } finally { cleanup }`.
/// Emits the try body followed by the finally statements in a single block.
#[derive(Debug, Clone)]
pub struct RustTryFinallyStmt {
    /// The try block body.
    pub try_block: RustBlock,
    /// The finally statements — emitted after the try block.
    pub finally_stmts: Vec<RustStmt>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A `while let Some(binding) = expr.next().await { body }` loop.
///
/// Produced by lowering `for await (const item of stream) { body }`.
/// Iterates over an async `Stream`, consuming items until the stream is exhausted.
#[derive(Debug, Clone)]
pub struct RustWhileLetStmt {
    /// The pattern binding name (the `item` in `Some(item)`).
    pub binding: String,
    /// The stream expression (the `stream` in `stream.next().await`).
    pub stream: RustExpr,
    /// The loop body.
    pub body: RustBlock,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `let` binding.
///
/// Corresponds to `let [mut] name [: ty] = init;`.
#[derive(Debug, Clone)]
pub struct RustLetStmt {
    /// Whether the binding is `mut`.
    pub mutable: bool,
    /// The variable name.
    pub name: String,
    /// The optional type annotation.
    pub ty: Option<RustType>,
    /// The initializer expression.
    pub init: RustExpr,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `return` statement.
#[derive(Debug, Clone)]
pub struct RustReturnStmt {
    /// The return value, if present.
    pub value: Option<RustExpr>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `if`/`else` statement.
#[derive(Debug, Clone)]
pub struct RustIfStmt {
    /// The condition expression.
    pub condition: RustExpr,
    /// The then-branch block.
    pub then_block: RustBlock,
    /// The optional else clause.
    pub else_clause: Option<RustElse>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// The else clause of a Rust `if` statement.
#[derive(Debug, Clone)]
pub enum RustElse {
    /// A plain `else { ... }` block.
    Block(RustBlock),
    /// An `else if ...` chain.
    ElseIf(Box<RustIfStmt>),
}

/// A Rust `while` loop.
#[derive(Debug, Clone)]
pub struct RustWhileStmt {
    /// The loop condition expression.
    pub condition: RustExpr,
    /// The loop body.
    pub body: RustBlock,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `loop { ... }` with body statements.
///
/// Produced by lowering `do { ... } while (condition)` to
/// `loop { body; if !condition { break; } }`.
#[derive(Debug, Clone)]
pub struct RustLoopStmt {
    /// The loop body (includes the break-condition `if` at the end).
    pub body: RustBlock,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `for x in iter { ... }` loop.
///
/// Produced by lowering a `RustScript` [`ForOfStmt`](crate::ast::ForOfStmt).
#[derive(Debug, Clone)]
pub struct RustForInStmt {
    /// The loop variable name.
    pub variable: String,
    /// The iterable expression (typically a `&collection` reference).
    pub iterable: RustExpr,
    /// The loop body.
    pub body: RustBlock,
    /// Whether to use a destructuring reference pattern (`for &n in &items`)
    /// instead of a plain variable pattern (`for n in &items`).
    /// Set to true for Copy element types so the loop variable has value type.
    pub deref_pattern: bool,
    /// Whether the iterable is already a reference (e.g., a borrowed parameter).
    /// When true, the emitter omits the `&` prefix to avoid double-borrowing (`&&Vec<T>`).
    pub iterable_is_borrowed: bool,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust destructuring let statement.
///
/// Corresponds to `let TypeName { field1, field2, .. } = expr;`.
/// Supports field renames: `let TypeName { field: local, .. } = expr;`.
#[derive(Debug, Clone)]
pub struct RustDestructureStmt {
    /// The struct type name for the destructuring pattern.
    pub type_name: String,
    /// The fields to extract, each with an optional rename (local binding name).
    pub fields: Vec<RustDestructureField>,
    /// The initializer expression.
    pub init: RustExpr,
    /// Whether the bindings are `mut`.
    pub mutable: bool,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A single field in a Rust struct destructuring pattern.
///
/// Represents either `field` or `field: local_name` in a pattern.
#[derive(Debug, Clone)]
pub struct RustDestructureField {
    /// The struct field name being matched.
    pub field_name: String,
    /// The local binding name. If different from `field_name`, emits `field: local`.
    pub local_name: Option<String>,
}

/// A destructuring-with-defaults statement, emitted as individual `let` bindings.
///
/// When destructuring fields have default values, struct pattern destructuring
/// cannot be used. Instead, each field is extracted individually with
/// `unwrap_or_else` for `Option` fields.
#[derive(Debug, Clone)]
pub struct RustDestructureDefaultsStmt {
    /// The individual field extractions.
    pub fields: Vec<RustDestructureDefaultField>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A single field extraction with an optional default value.
#[derive(Debug, Clone)]
pub struct RustDestructureDefaultField {
    /// The local variable name to bind.
    pub local_name: String,
    /// The expression to access the field: `source.field_name`.
    pub access_expr: RustExpr,
    /// If present, the default value expression for `unwrap_or_else`.
    pub default_value: Option<RustExpr>,
    /// Whether the binding is `mut`.
    pub mutable: bool,
}

/// A Rust tuple destructuring statement: `let (a, b, c) = expr;`.
///
/// Produced by lowering `const [a, b] = await Promise.all([...])`.
#[derive(Debug, Clone)]
pub struct RustTupleDestructureStmt {
    /// The variable names to bind from the tuple.
    pub bindings: Vec<String>,
    /// The initializer expression (typically a `tokio::join!` call).
    pub init: RustExpr,
    /// Whether the bindings are `mut`.
    pub mutable: bool,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A Rust `match` expression used as a statement.
///
/// Produced by lowering a `RustScript` `switch` statement.
#[derive(Debug, Clone)]
pub struct RustMatchStmt {
    /// The scrutinee expression.
    pub scrutinee: RustExpr,
    /// The match arms.
    pub arms: Vec<RustMatchArm>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A single arm of a Rust `match` expression.
#[derive(Debug, Clone)]
pub struct RustMatchArm {
    /// The pattern (e.g., `Direction::North` or `Shape::Circle { radius }`).
    pub pattern: RustPattern,
    /// The body block.
    pub body: RustBlock,
}

/// An `if let Some(name) = expr` statement for null narrowing.
///
/// Produced by lowering `if (x !== null) { ... }` to Rust's `if let Some(x) = x { ... }`.
#[derive(Debug, Clone)]
pub struct RustIfLetStmt {
    /// The variable name to bind the unwrapped value to.
    pub binding: String,
    /// The expression being tested (must be `Option<T>`).
    pub expr: RustExpr,
    /// The then block (value is `Some`).
    pub then_block: RustBlock,
    /// The else block (value is `None`).
    pub else_block: Option<RustBlock>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A `let Some(name) = expr else { diverging_block };` statement.
///
/// Produced by lowering null-guard patterns: `if (x === null) { throw/return; }`.
/// Rebinds the variable as the unwrapped value in the continuation scope.
#[derive(Debug, Clone)]
pub struct RustLetElseStmt {
    /// The variable name to bind the unwrapped value to.
    pub binding: String,
    /// The expression being tested (must be `Option<T>`).
    pub expr: RustExpr,
    /// The diverging block (must contain return/throw/break/continue).
    pub else_block: RustBlock,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A `match` on `Result<T, E>` with `Ok`/`Err` arms and optional finally cleanup.
///
/// Produced by lowering `try/catch` and `try/catch/finally` blocks.
#[derive(Debug, Clone)]
pub struct RustMatchResultStmt {
    /// The expression being matched (must be `Result<T, E>`).
    pub expr: RustExpr,
    /// The binding name for the `Ok` arm.
    pub ok_binding: String,
    /// The block for the `Ok` arm.
    pub ok_block: RustBlock,
    /// The binding name for the `Err` arm.
    pub err_binding: String,
    /// The block for the `Err` arm.
    pub err_block: RustBlock,
    /// Optional finally statements — emitted after the match block.
    pub finally_stmts: Vec<RustStmt>,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

/// A pattern in a Rust `match` arm.
#[derive(Debug, Clone)]
pub enum RustPattern {
    /// A simple enum variant: `Direction::North`.
    /// Fields: `(enum_name, variant_name)`.
    EnumVariant(String, String),
    /// A destructuring enum variant: `Shape::Circle { radius }`.
    /// Fields: `(enum_name, variant_name, field_names)`.
    EnumVariantFields(String, String, Vec<String>),
}

/// A Rust expression with optional source span.
///
/// Uses a kind+span wrapper pattern. Source-derived expressions carry
/// `Some(span)`, compiler-generated expressions carry `None`.
#[derive(Debug, Clone)]
pub struct RustExpr {
    /// The kind of expression.
    pub kind: RustExprKind,
    /// The source span, if derived from source.
    pub span: Option<Span>,
}

impl RustExpr {
    /// Create an expression with a source span.
    #[must_use]
    pub fn new(kind: RustExprKind, span: Span) -> Self {
        Self {
            kind,
            span: Some(span),
        }
    }

    /// Create a compiler-generated expression with no source span.
    #[must_use]
    pub fn synthetic(kind: RustExprKind) -> Self {
        Self { kind, span: None }
    }
}

/// The kinds of Rust expressions in the IR.
#[derive(Debug, Clone)]
pub enum RustExprKind {
    /// An integer literal (e.g., `42`).
    IntLit(i64),
    /// A floating-point literal (e.g., `3.14`).
    FloatLit(f64),
    /// A string literal (e.g., `"hello"`).
    StringLit(String),
    /// A boolean literal (`true` or `false`).
    BoolLit(bool),
    /// An identifier reference (e.g., `x`).
    Ident(String),
    /// A binary operation (e.g., `a + b`).
    Binary {
        /// The binary operator.
        op: RustBinaryOp,
        /// The left-hand operand.
        left: Box<RustExpr>,
        /// The right-hand operand.
        right: Box<RustExpr>,
    },
    /// A unary operation (e.g., `-x`).
    Unary {
        /// The unary operator.
        op: RustUnaryOp,
        /// The operand.
        operand: Box<RustExpr>,
    },
    /// A function call (e.g., `foo(a, b)`).
    Call {
        /// The function name.
        func: String,
        /// The argument list.
        args: Vec<RustExpr>,
    },
    /// A method call (e.g., `receiver.method(args)` or `receiver.method::<T>(args)`).
    MethodCall {
        /// The receiver expression.
        receiver: Box<RustExpr>,
        /// The method name.
        method: String,
        /// Explicit type arguments for turbofish syntax (e.g., `::<T>`).
        type_args: Vec<RustType>,
        /// The argument list.
        args: Vec<RustExpr>,
    },
    /// A parenthesized expression.
    Paren(Box<RustExpr>),
    /// An assignment expression (e.g., `x = value`).
    Assign {
        /// The assignment target.
        target: String,
        /// The value being assigned.
        value: Box<RustExpr>,
    },
    /// A macro invocation (e.g., `println!`).
    ///
    /// Used for lowering `console.log()` to `println!`.
    Macro {
        /// The macro name (without the `!`).
        name: String,
        /// The arguments to the macro.
        args: Vec<RustExpr>,
    },
    /// A `.clone()` call inserted for ownership correctness.
    Clone(Box<RustExpr>),
    /// A `.to_string()` conversion.
    ToString(Box<RustExpr>),
    /// A compound assignment expression (e.g., `x += 1`).
    CompoundAssign {
        /// The assignment target.
        target: String,
        /// The compound assignment operator.
        op: RustCompoundAssignOp,
        /// The right-hand side value.
        value: Box<RustExpr>,
    },
    /// Struct literal construction: `User { name: ..., age: ... }`.
    StructLit {
        /// The struct type name.
        type_name: String,
        /// The field name-value pairs.
        fields: Vec<(String, RustExpr)>,
    },
    /// Field access: `expr.field`.
    FieldAccess {
        /// The object expression.
        object: Box<RustExpr>,
        /// The field name.
        field: String,
    },
    /// Enum variant construction: `Direction::North`.
    /// Fields: `(enum_name, variant_name)`.
    EnumVariant {
        /// The enum type name.
        enum_name: String,
        /// The variant name.
        variant_name: String,
    },
    /// Vec literal: `vec![1, 2, 3]`.
    /// Produced by lowering `RustScript` array literals.
    VecLit(Vec<RustExpr>),
    /// Static method call: `HashMap::new()`, `HashSet::new()`.
    /// Produced by lowering `new Map()` / `new Set()` expressions.
    StaticCall {
        /// The type name (e.g., `HashMap`, `HashSet`).
        type_name: String,
        /// The method name (e.g., `new`).
        method: String,
        /// The arguments to the method.
        args: Vec<RustExpr>,
    },
    /// Index access: `expr[index]`.
    /// Produced by lowering `RustScript` index expressions.
    Index {
        /// The object being indexed.
        object: Box<RustExpr>,
        /// The index expression.
        index: Box<RustExpr>,
    },
    /// Index assignment: `expr[index] = value`.
    /// Produced by lowering `RustScript` index assignment expressions on non-HashMap types.
    IndexAssign {
        /// The object being indexed.
        object: Box<RustExpr>,
        /// The index expression.
        index: Box<RustExpr>,
        /// The value being assigned.
        value: Box<RustExpr>,
    },
    /// `None` — from `null` literal. Lowers to Rust `None`.
    None,
    /// `Some(expr)` — wrapping a non-null value in `Option` context.
    Some(Box<RustExpr>),
    /// `expr.unwrap_or(default)` — from nullish coalescing `??`.
    UnwrapOr {
        /// The `Option<T>` expression.
        expr: Box<RustExpr>,
        /// The default value.
        default: Box<RustExpr>,
    },
    /// The `?` operator: `expr?`.
    /// Produced by lowering calls to `throws` functions within a `throws` context.
    QuestionMark(Box<RustExpr>),
    /// `Ok(expr)` — wrapping a success value in `Result`.
    Ok(Box<RustExpr>),
    /// `Err(expr)` — wrapping an error value in `Result`.
    Err(Box<RustExpr>),
    /// An immediately-invoked closure: `(|| -> ReturnType { body })()`.
    /// Used for lowering multi-statement `try` blocks.
    /// When `is_async` is true, emits `(async || -> ReturnType { body })().await`.
    ClosureCall {
        /// Whether this is an async closure (needed when body contains `.await`).
        is_async: bool,
        /// The closure body.
        body: RustBlock,
        /// The return type of the closure.
        return_type: RustType,
    },
    /// Optional chaining: `expr.as_ref().map(|v| v.field)`.
    /// Produced by lowering `expr?.field`.
    OptionMap {
        /// The `Option<T>` expression.
        expr: Box<RustExpr>,
        /// The closure parameter name.
        closure_param: String,
        /// The closure body expression.
        closure_body: Box<RustExpr>,
    },
    /// A closure expression: `[async] [move] |params| body`.
    /// Produced by lowering `RustScript` arrow functions.
    Closure {
        /// Whether this is an `async` closure.
        is_async: bool,
        /// Whether this is a `move` closure.
        is_move: bool,
        /// The closure parameters.
        params: Vec<RustClosureParam>,
        /// The return type (optional — omitted when Rust can infer).
        return_type: Option<RustType>,
        /// The closure body — expression or block.
        body: RustClosureBody,
    },
    /// An `.await` expression: `expr.await`.
    /// Note: Rust uses postfix `.await` while `RustScript` uses prefix `await expr`.
    Await(Box<RustExpr>),
    /// `self` — reference to the current instance in an impl method.
    /// Produced by lowering `this` in class methods.
    SelfRef,
    /// `Self { field: value, ... }` — struct literal using `Self` type.
    /// Produced by lowering constructor bodies.
    SelfStructLit {
        /// The field name-value pairs.
        fields: Vec<(String, RustExpr)>,
    },
    /// Field access on `self`: `self.field`.
    /// Produced by lowering `this.field` in class methods.
    SelfFieldAccess {
        /// The field name.
        field: String,
    },
    /// Assignment to a `self` field: `self.field = value`.
    /// Produced by lowering `this.field = value` in class methods.
    SelfFieldAssign {
        /// The field name.
        field: String,
        /// The value being assigned.
        value: Box<RustExpr>,
    },
    /// An async block: `async [move] { body }`.
    /// Produced by lowering `spawn(async () => { ... })`.
    AsyncBlock {
        /// Whether this is a `move` async block (`async move { ... }`).
        is_move: bool,
        /// The body of the async block.
        body: RustBlock,
    },
    /// `tokio::join!(expr1, expr2, ...)` — concurrent execution of futures.
    /// Produced by lowering `await Promise.all([...])`.
    ///
    /// `throwing_elements[i]` is `true` when the i-th future comes from a
    /// function declared with `throws`. After the join, each throwing
    /// element's result must be unwrapped with `?` (in a `throws` context)
    /// or `.unwrap()` (in a non-throws context).
    TokioJoin {
        /// The future expressions passed to `tokio::join!`.
        elements: Vec<RustExpr>,
        /// Per-element flag: `true` when the element is a call to a `throws` function.
        throwing_elements: Vec<bool>,
    },
    /// `tokio::select! { result = expr1 => result, ... }` — first-to-complete.
    /// Produced by lowering `await Promise.race([...])`.
    TokioSelect(Vec<RustExpr>),
    /// `futures::future::select_ok(vec![...]).await` — first-to-succeed.
    /// Produced by lowering `await Promise.any([...])`.
    FuturesSelectOk(Vec<RustExpr>),
    /// A borrow expression: `&expr`.
    ///
    /// Inserted by Tier 2 ownership when a function takes a borrowed parameter.
    /// Rust's auto-deref means `&String` coerces to `&str` where needed.
    Borrow(Box<RustExpr>),
    /// `Arc::new(Mutex::new(expr))` — from `shared(expr)`.
    ArcMutexNew(Box<RustExpr>),
    /// A type cast: `expr as Type`.
    /// Produced by lowering `.length` to `.len() as i64`.
    Cast(Box<RustExpr>, RustType),
    /// An if expression: `if condition { then_expr } else { else_expr }`.
    /// Produced by lowering the ternary operator `condition ? then_expr : else_expr`.
    IfExpr {
        /// The condition expression.
        condition: Box<RustExpr>,
        /// The then-branch expression.
        then_expr: Box<RustExpr>,
        /// The else-branch expression.
        else_expr: Box<RustExpr>,
    },
    /// An iterator chain: `source.iter().ops...terminal`.
    ///
    /// Produced by lowering TypeScript-style array method chains
    /// (e.g., `arr.map(fn).filter(fn)`) to Rust iterator chains.
    IteratorChain {
        /// The source collection expression.
        source: Box<RustExpr>,
        /// The chain of intermediate iterator operations.
        ops: Vec<IteratorOp>,
        /// The terminal operation (collect, fold, find, any, all, `for_each`).
        terminal: IteratorTerminal,
    },
    /// Array spread construction: `[...arr, x, ...arr2]`.
    ///
    /// Represents a sequence of push/extend operations building a `Vec`.
    /// Lowered from `RustScript` array literals containing spread elements.
    SpreadArray {
        /// Initial elements before the first spread (may be empty).
        initial: Vec<RustExpr>,
        /// Sequence of push (single element) or extend (spread) operations.
        ops: Vec<SpreadOp>,
    },
    /// Struct construction with update syntax: `Type { field: val, ..base }`.
    ///
    /// Produced by lowering `{ ...base, field: value }` in `RustScript`.
    StructUpdate {
        /// The struct type name.
        type_name: String,
        /// The overridden field name-value pairs.
        fields: Vec<(String, RustExpr)>,
        /// The base expression (cloned at runtime).
        base: Box<RustExpr>,
    },
    /// Tuple construction: `(a, b, c)`.
    /// Produced by lowering array literals in tuple type context.
    Tuple(Vec<RustExpr>),
    /// Tuple field access: `expr.0`, `expr.1`, etc.
    /// Produced by lowering `expr[0]` on a tuple-typed value.
    TupleField {
        /// The tuple expression.
        object: Box<RustExpr>,
        /// The field index (0-based).
        index: usize,
    },
    /// Raw Rust code emitted verbatim.
    /// Used for generator state machine bodies and other compiler-generated code.
    Raw(String),
}

/// A single intermediate iterator operation in a chain.
///
/// These operations appear between `.iter()` and the terminal operation
/// in the emitted Rust iterator chain.
#[derive(Debug, Clone)]
pub enum IteratorOp {
    /// `.map(|param| body)` — transform each element.
    Map(RustClosureParam, Box<RustExpr>),
    /// `.map(fn_ref)` — transform using a function reference.
    MapFnRef(Box<RustExpr>),
    /// `.filter(|param| body)` — keep elements matching predicate.
    Filter(RustClosureParam, Box<RustExpr>),
    /// `.filter(fn_ref)` — filter using a function reference.
    FilterFnRef(Box<RustExpr>),
    /// `.cloned()` — clone referenced elements to produce owned values.
    Cloned,
}

/// The terminal operation of an iterator chain.
///
/// Determines what the iterator chain produces: a collected `Vec`, a fold
/// result, a found element, a boolean predicate, or a side-effect loop.
#[derive(Debug, Clone)]
pub enum IteratorTerminal {
    /// `.collect::<Vec<_>>()` — collect into a Vec.
    CollectVec,
    /// `.fold(init, |acc, item| body)` — reduce to a single value.
    Fold {
        /// The initial accumulator value.
        init: Box<RustExpr>,
        /// The accumulator parameter name.
        acc_param: String,
        /// The item parameter name.
        item_param: String,
        /// The fold body — expression or block.
        body: RustClosureBody,
    },
    /// `.find(|param| body).cloned()` — find first matching, return `Option<T>`.
    Find(RustClosureParam, Box<RustExpr>),
    /// `.any(|param| body)` — true if any element matches.
    Any(RustClosureParam, Box<RustExpr>),
    /// `.all(|param| body)` — true if all elements match.
    All(RustClosureParam, Box<RustExpr>),
    /// `.for_each(|param| body)` — execute side effect for each element.
    ForEach(RustClosureParam, Box<RustExpr>),
}

/// An operation in an array spread construction.
///
/// Each operation either pushes a single element or extends from an iterable
/// (the spread source).
#[derive(Debug, Clone)]
pub enum SpreadOp {
    /// Push a single element: `__spread.push(expr)`.
    Push(RustExpr),
    /// Extend from an iterable: `__spread.extend(expr.iter().cloned())`.
    Extend(RustExpr),
}

/// A closure parameter (may omit type for inference).
///
/// In the emitted Rust, parameters with types appear as `name: Type`,
/// and parameters without types appear as just `name`.
#[derive(Debug, Clone)]
pub struct RustClosureParam {
    /// The parameter name.
    pub name: String,
    /// The parameter type (explicit or omitted for inference).
    pub ty: Option<RustType>,
}

/// Closure body — either an expression or a block.
///
/// Expression bodies: `|x| x * 2` — no braces, implicit return.
/// Block bodies: `|| { stmt; stmt; }` — braces, explicit statements.
#[derive(Debug, Clone)]
pub enum RustClosureBody {
    /// Expression body: `|x| x * 2`.
    Expr(Box<RustExpr>),
    /// Block body: `|| { stmts }`.
    Block(RustBlock),
}

/// A compound assignment operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustCompoundAssignOp {
    /// Addition assignment (`+=`).
    AddAssign,
    /// Subtraction assignment (`-=`).
    SubAssign,
    /// Multiplication assignment (`*=`).
    MulAssign,
    /// Division assignment (`/=`).
    DivAssign,
    /// Remainder assignment (`%=`).
    RemAssign,
}

impl std::fmt::Display for RustCompoundAssignOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::AddAssign => "+=",
            Self::SubAssign => "-=",
            Self::MulAssign => "*=",
            Self::DivAssign => "/=",
            Self::RemAssign => "%=",
        };
        f.write_str(s)
    }
}

/// Rust binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustBinaryOp {
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Sub,
    /// Multiplication (`*`).
    Mul,
    /// Division (`/`).
    Div,
    /// Remainder (`%`).
    Rem,
    /// Equality (`==`).
    Eq,
    /// Inequality (`!=`).
    Ne,
    /// Less than (`<`).
    Lt,
    /// Greater than (`>`).
    Gt,
    /// Less than or equal (`<=`).
    Le,
    /// Greater than or equal (`>=`).
    Ge,
    /// Logical AND (`&&`).
    And,
    /// Logical OR (`||`).
    Or,
    /// Bitwise AND (`&`).
    BitAnd,
    /// Bitwise OR (`|`).
    BitOr,
    /// Bitwise XOR (`^`).
    BitXor,
    /// Left shift (`<<`).
    Shl,
    /// Right shift (`>>`).
    Shr,
}

impl std::fmt::Display for RustBinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::And => "&&",
            Self::Or => "||",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::BitXor => "^",
            Self::Shl => "<<",
            Self::Shr => ">>",
        };
        f.write_str(s)
    }
}

/// Rust unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustUnaryOp {
    /// Arithmetic negation (`-`).
    Neg,
    /// Logical NOT (`!`).
    Not,
    /// Bitwise NOT (`!` in Rust — same symbol, but on integer types).
    BitNot,
}

impl std::fmt::Display for RustUnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Neg => "-",
            Self::Not | Self::BitNot => "!",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a span for tests.
    fn span(start: u32, end: u32) -> Span {
        Span::new(start, end)
    }

    #[test]
    fn test_rust_fn_decl_complete_construction() {
        let decl = RustFnDecl {
            attributes: vec![],
            is_async: false,
            public: false,
            name: "add".to_owned(),
            type_params: vec![],
            params: vec![
                RustParam {
                    name: "a".to_owned(),
                    ty: RustType::I32,
                    mode: ParamMode::Owned,
                    span: Some(span(4, 10)),
                },
                RustParam {
                    name: "b".to_owned(),
                    ty: RustType::I32,
                    mode: ParamMode::Owned,
                    span: Some(span(12, 18)),
                },
            ],
            return_type: Some(RustType::I32),
            body: RustBlock {
                stmts: vec![],
                expr: Some(Box::new(RustExpr::new(
                    RustExprKind::Binary {
                        op: RustBinaryOp::Add,
                        left: Box::new(RustExpr::new(
                            RustExprKind::Ident("a".to_owned()),
                            span(25, 26),
                        )),
                        right: Box::new(RustExpr::new(
                            RustExprKind::Ident("b".to_owned()),
                            span(29, 30),
                        )),
                    },
                    span(25, 30),
                ))),
            },
            doc_comment: None,
            span: Some(span(0, 32)),
        };

        assert_eq!(decl.name, "add");
        assert_eq!(decl.params.len(), 2);
        assert_eq!(decl.return_type, Some(RustType::I32));
        assert!(decl.span.is_some());
    }

    #[test]
    fn test_rust_block_trailing_expr_vs_stmt_only() {
        // Block with trailing expression (returns a value).
        let with_expr = RustBlock {
            stmts: vec![RustStmt::Semi(RustExpr::new(
                RustExprKind::IntLit(1),
                span(0, 1),
            ))],
            expr: Some(Box::new(RustExpr::new(RustExprKind::IntLit(2), span(3, 4)))),
        };

        assert_eq!(with_expr.stmts.len(), 1);
        assert!(with_expr.expr.is_some());

        // Block without trailing expression (returns unit).
        let stmt_only = RustBlock {
            stmts: vec![
                RustStmt::Semi(RustExpr::new(RustExprKind::IntLit(1), span(0, 1))),
                RustStmt::Semi(RustExpr::new(RustExprKind::IntLit(2), span(3, 4))),
            ],
            expr: None,
        };

        assert_eq!(stmt_only.stmts.len(), 2);
        assert!(stmt_only.expr.is_none());
    }

    #[test]
    fn test_rust_type_display_all_variants() {
        assert_eq!(RustType::I8.to_string(), "i8");
        assert_eq!(RustType::I16.to_string(), "i16");
        assert_eq!(RustType::I32.to_string(), "i32");
        assert_eq!(RustType::I64.to_string(), "i64");
        assert_eq!(RustType::U8.to_string(), "u8");
        assert_eq!(RustType::U16.to_string(), "u16");
        assert_eq!(RustType::U32.to_string(), "u32");
        assert_eq!(RustType::U64.to_string(), "u64");
        assert_eq!(RustType::F32.to_string(), "f32");
        assert_eq!(RustType::F64.to_string(), "f64");
        assert_eq!(RustType::Bool.to_string(), "bool");
        assert_eq!(RustType::String.to_string(), "String");
        assert_eq!(RustType::Unit.to_string(), "()");
    }

    #[test]
    fn test_rust_binary_op_display_all_variants() {
        assert_eq!(RustBinaryOp::Add.to_string(), "+");
        assert_eq!(RustBinaryOp::Sub.to_string(), "-");
        assert_eq!(RustBinaryOp::Mul.to_string(), "*");
        assert_eq!(RustBinaryOp::Div.to_string(), "/");
        assert_eq!(RustBinaryOp::Rem.to_string(), "%");
        assert_eq!(RustBinaryOp::Eq.to_string(), "==");
        assert_eq!(RustBinaryOp::Ne.to_string(), "!=");
        assert_eq!(RustBinaryOp::Lt.to_string(), "<");
        assert_eq!(RustBinaryOp::Gt.to_string(), ">");
        assert_eq!(RustBinaryOp::Le.to_string(), "<=");
        assert_eq!(RustBinaryOp::Ge.to_string(), ">=");
        assert_eq!(RustBinaryOp::And.to_string(), "&&");
        assert_eq!(RustBinaryOp::Or.to_string(), "||");
    }

    #[test]
    fn test_rust_unary_op_display_both_variants() {
        assert_eq!(RustUnaryOp::Neg.to_string(), "-");
        assert_eq!(RustUnaryOp::Not.to_string(), "!");
    }

    #[test]
    fn test_rust_expr_synthetic_span_is_none() {
        let expr = RustExpr::synthetic(RustExprKind::Clone(Box::new(RustExpr::synthetic(
            RustExprKind::Ident("x".to_owned()),
        ))));

        assert!(expr.span.is_none());
        match &expr.kind {
            RustExprKind::Clone(inner) => assert!(inner.span.is_none()),
            _ => panic!("expected Clone variant"),
        }
    }

    #[test]
    fn test_rust_expr_new_span_is_some() {
        let s = span(10, 20);
        let expr = RustExpr::new(RustExprKind::IntLit(42), s);

        assert_eq!(expr.span, Some(s));
    }

    #[test]
    fn test_rust_type_arc_mutex_display_simple() {
        let ty = RustType::ArcMutex(Box::new(RustType::I32));
        assert_eq!(ty.to_string(), "Arc<Mutex<i32>>");
    }

    #[test]
    fn test_rust_type_arc_mutex_display_string() {
        let ty = RustType::ArcMutex(Box::new(RustType::String));
        assert_eq!(ty.to_string(), "Arc<Mutex<String>>");
    }

    #[test]
    fn test_rust_type_arc_mutex_display_nested_generic() {
        let inner = RustType::Generic(
            Box::new(RustType::Named("Vec".to_owned())),
            vec![RustType::I32],
        );
        let ty = RustType::ArcMutex(Box::new(inner));
        assert_eq!(ty.to_string(), "Arc<Mutex<Vec<i32>>>");
    }

    #[test]
    fn test_rust_type_tuple_display_two_elements() {
        let ty = RustType::Tuple(vec![RustType::String, RustType::I32]);
        assert_eq!(ty.to_string(), "(String, i32)");
    }

    #[test]
    fn test_rust_type_tuple_display_three_elements() {
        let ty = RustType::Tuple(vec![RustType::String, RustType::I32, RustType::Bool]);
        assert_eq!(ty.to_string(), "(String, i32, bool)");
    }

    #[test]
    fn test_rust_type_tuple_display_nested() {
        let ty = RustType::Tuple(vec![
            RustType::String,
            RustType::Tuple(vec![RustType::I32, RustType::Bool]),
        ]);
        assert_eq!(ty.to_string(), "(String, (i32, bool))");
    }
}
