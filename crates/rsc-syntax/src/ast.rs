//! `RustScript` AST type definitions.
//!
//! Covers the full language surface through Phase 3: functions, types, enums,
//! interfaces, classes, async/await, closures, template literals, imports/exports,
//! try/catch, switch/match, optional types, array/string method calls, and all
//! syntax supported by the formatter and LSP.
//!
//! The parser produces this AST from `.rts` source. The lowering pass consumes
//! it and produces Rust IR. Each node carries a [`Span`] for diagnostic reporting.

use crate::span::Span;

/// A complete `RustScript` module (one `.rts` file).
///
/// This is the root AST node. Named `Module` rather than `SourceFile` to avoid
/// collision with [`crate::source::SourceFile`] and to correctly reflect that
/// each `.rts` file maps to a Rust module.
#[derive(Debug, Clone)]
pub struct Module {
    /// The top-level items in this module.
    pub items: Vec<Item>,
    /// The span covering the entire module.
    pub span: Span,
}

/// A top-level item in a `RustScript` module.
///
/// Wraps an [`ItemKind`] with metadata common to all items (export status,
/// source span). Supports function declarations, type definitions, enums,
/// interfaces, classes, imports, and re-exports.
#[derive(Debug, Clone)]
pub struct Item {
    /// The kind of item.
    pub kind: ItemKind,
    /// Whether this item is exported (`export` keyword). Defaults to `false`
    /// until the module system is implemented (Task 024).
    pub exported: bool,
    /// The span covering the entire item.
    pub span: Span,
}

/// The kinds of top-level items in a `RustScript` module.
///
/// Supports function declarations, type definitions, enum definitions,
/// interface definitions, class definitions, imports, and re-exports.
#[derive(Debug, Clone)]
pub enum ItemKind {
    /// A function declaration (`function name(...) { ... }`).
    Function(FnDecl),
    /// A type definition (`type Name = { field: Type, ... }`).
    /// Lowers to a Rust `struct`.
    TypeDef(TypeDef),
    /// An enum definition (`type Direction = "north" | "south" | ...`).
    /// Lowers to a Rust `enum`.
    EnumDef(EnumDef),
    /// An interface definition (`interface Name { method(): Type; ... }`).
    /// Lowers to a Rust `trait`.
    Interface(InterfaceDef),
    /// An import declaration (`import { Name } from "./module"`).
    /// Lowers to a Rust `use` declaration.
    Import(ImportDecl),
    /// A re-export declaration (`export { Name } from "./module"`).
    /// Lowers to a Rust `pub use` declaration.
    ReExport(ReExportDecl),
    /// A class definition (`class Name { fields; constructor() { }; methods() { } }`).
    /// Lowers to a Rust `struct` + `impl` block.
    Class(ClassDef),
    /// A raw Rust code block at module level (`rust { ... }`).
    /// The contents are passed through to the generated `.rs` file unchanged.
    RustBlock(InlineRustBlock),
    /// A top-level variable declaration (`const name: Type = expr;` or `let name: Type = expr;`).
    /// Lowers to a Rust `const` or `static` item depending on the initializer.
    Const(VarDecl),
}

/// An import declaration: `import { User, Post } from "./models"`.
///
/// Lowers to `use crate::models::{User, Post};` in Rust.
#[derive(Debug, Clone)]
pub struct ImportDecl {
    /// The names being imported.
    pub names: Vec<Ident>,
    /// The module path as a string literal (e.g., `"./models"`).
    pub source: StringLiteral,
    /// The span covering the entire import declaration.
    pub span: Span,
}

/// A string literal used in import/export source paths.
#[derive(Debug, Clone)]
pub struct StringLiteral {
    /// The string value (without quotes).
    pub value: String,
    /// The span covering the string literal including quotes.
    pub span: Span,
}

/// A re-export declaration: `export { Name1, Name2 } from "./module"`.
///
/// Lowers to `pub use crate::module::{Name1, Name2};` in Rust.
#[derive(Debug, Clone)]
pub struct ReExportDecl {
    /// The names being re-exported.
    pub names: Vec<Ident>,
    /// The module path as a string literal.
    pub source: StringLiteral,
    /// The span covering the entire re-export declaration.
    pub span: Span,
}

/// A generic type parameter: `T` or `T extends Constraint`.
///
/// Appears in generic function declarations and generic type definitions.
/// The optional `constraint` maps to a Rust trait bound (`T: Constraint`).
#[derive(Debug, Clone)]
pub struct TypeParam {
    /// The type parameter name (e.g., `T`, `U`).
    pub name: Ident,
    /// Optional constraint: `extends Comparable` maps to a trait bound.
    pub constraint: Option<TypeAnnotation>,
    /// The span covering the type parameter.
    pub span: Span,
}

/// Type parameters on a function or type definition: `<T, U extends Clone>`.
#[derive(Debug, Clone)]
pub struct TypeParams {
    /// The individual type parameters.
    pub params: Vec<TypeParam>,
    /// The span covering the entire `<...>` type parameter list.
    pub span: Span,
}

/// A function return type with optional throws annotation.
///
/// Corresponds to `: ReturnType throws ErrorType` in a function declaration.
/// When `throws` is present, the function lowers to `-> Result<T, E>`.
#[derive(Debug, Clone)]
pub struct ReturnTypeAnnotation {
    /// The success return type, if present. `None` means `void` (unit).
    pub type_ann: Option<TypeAnnotation>,
    /// The error type for `throws`. Present when the function is fallible.
    pub throws: Option<TypeAnnotation>,
    /// The span covering the return type annotation.
    pub span: Span,
}

/// A function declaration.
///
/// Corresponds to `RustScript` `function name<T>(params): ReturnType { body }`.
/// Lowers to a Rust `fn` item. Generic type parameters are optional.
/// `async function` declarations lower to `async fn` in Rust.
#[derive(Debug, Clone)]
pub struct FnDecl {
    /// Whether this is an `async function`.
    pub is_async: bool,
    /// The function name.
    pub name: Ident,
    /// Optional generic type parameters: `<T, U extends Clone>`.
    pub type_params: Option<TypeParams>,
    /// The parameter list.
    pub params: Vec<Param>,
    /// The return type annotation, if present. Absent means `void`.
    pub return_type: Option<ReturnTypeAnnotation>,
    /// The function body.
    pub body: Block,
    /// The span covering the entire function declaration.
    pub span: Span,
}

/// A type definition: `type Name<T> = { field: Type, ... }`.
///
/// Lowers to a Rust `struct` with `pub` fields. Generic type parameters
/// are optional.
#[derive(Debug, Clone)]
pub struct TypeDef {
    /// The type name.
    pub name: Ident,
    /// Optional generic type parameters: `<T, U extends Clone>`.
    pub type_params: Option<TypeParams>,
    /// The fields of the type definition.
    pub fields: Vec<FieldDef>,
    /// The span covering the entire type definition.
    pub span: Span,
}

/// A simple enum or discriminated union definition.
///
/// Simple enum: `type Direction = "north" | "south" | "east" | "west"`.
/// Discriminated union: `type Shape = | { kind: "circle", radius: f64 } | ...`.
/// Lowers to a Rust `enum`.
#[derive(Debug, Clone)]
pub struct EnumDef {
    /// The enum name.
    pub name: Ident,
    /// The variants of the enum.
    pub variants: Vec<EnumVariant>,
    /// The span covering the entire enum definition.
    pub span: Span,
}

/// A single enum variant.
///
/// `Simple` variants come from string literal unions (`"north"`).
/// `Data` variants come from discriminated union objects (`{ kind: "circle", radius: f64 }`).
#[derive(Debug, Clone)]
pub enum EnumVariant {
    /// A simple string variant: `"north"`. The `Ident` is the capitalized variant name.
    Simple(Ident, Span),
    /// A data variant: `{ kind: "circle", radius: f64 }`.
    /// The discriminant value (`"circle"`) becomes the variant name (capitalized).
    /// The `kind` field is consumed — only data fields appear.
    Data {
        /// The raw discriminant string value (e.g., `"circle"`).
        discriminant_value: String,
        /// The capitalized variant name (e.g., `Circle`).
        name: Ident,
        /// The data fields (excluding the `kind` discriminant).
        fields: Vec<FieldDef>,
        /// The span covering this variant.
        span: Span,
    },
}

/// An interface definition: `interface Serializable { ... }`.
///
/// Corresponds to a `RustScript` interface declaration. Lowers to a Rust `trait`.
/// Interface methods have no body — they are abstract method signatures.
#[derive(Debug, Clone)]
pub struct InterfaceDef {
    /// The interface name.
    pub name: Ident,
    /// Optional generic type parameters: `<T>`.
    pub type_params: Option<TypeParams>,
    /// The method signatures declared in this interface.
    pub methods: Vec<InterfaceMethod>,
    /// The span covering the entire interface definition.
    pub span: Span,
}

/// A method signature in an interface (no body).
///
/// Corresponds to `methodName(params): ReturnType;` within an interface body.
/// Lowers to a Rust trait method with `&self` as the first parameter.
#[derive(Debug, Clone)]
pub struct InterfaceMethod {
    /// The method name.
    pub name: Ident,
    /// The parameter list (excluding the implicit `self`).
    pub params: Vec<Param>,
    /// The return type annotation, if present. Absent means `void`.
    pub return_type: Option<ReturnTypeAnnotation>,
    /// The span covering the method signature.
    pub span: Span,
}

/// A class definition: `class Name implements Trait { fields; constructor; methods }`.
///
/// Lowers to a Rust `struct` + `impl` block. If `implements` is present,
/// trait methods are separated into `impl TraitName for ClassName` blocks.
#[derive(Debug, Clone)]
pub struct ClassDef {
    /// The class name.
    pub name: Ident,
    /// Optional generic type parameters: `<T>`.
    pub type_params: Option<TypeParams>,
    /// Interfaces this class implements.
    pub implements: Vec<Ident>,
    /// The class members (fields, constructor, methods).
    pub members: Vec<ClassMember>,
    /// The span covering the entire class definition.
    pub span: Span,
}

/// A member of a class definition.
#[derive(Debug, Clone)]
pub enum ClassMember {
    /// A class field declaration.
    Field(ClassField),
    /// The class constructor.
    Constructor(ClassConstructor),
    /// A class method.
    Method(ClassMethod),
    /// A getter accessor: `get name(): Type { ... }`.
    /// Lowers to a `fn name(&self) -> Type` method.
    Getter(ClassGetter),
    /// A setter accessor: `set name(value: Type) { ... }`.
    /// Lowers to a `fn set_name(&mut self, value: Type)` method.
    Setter(ClassSetter),
}

/// A class field declaration: `[private|public] [readonly] [static] name: Type [= expr];`.
///
/// Lowers to a struct field, with visibility controlling `pub`.
/// Static fields lower to associated constants in the impl block.
/// Readonly fields are enforced at `RustScript` compile time.
#[derive(Debug, Clone)]
pub struct ClassField {
    /// The visibility modifier (`public` or `private`).
    pub visibility: Visibility,
    /// The field name.
    pub name: Ident,
    /// The field type annotation.
    pub type_ann: TypeAnnotation,
    /// Optional field initializer (default value).
    pub initializer: Option<Expr>,
    /// Whether this field is readonly.
    pub readonly: bool,
    /// Whether this field is static.
    pub is_static: bool,
    /// The span covering the field declaration.
    pub span: Span,
}

/// Visibility modifier for class members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Public (default). Lowers to `pub` in Rust.
    Public,
    /// Private. Lowers to no visibility modifier in Rust.
    Private,
}

/// A class constructor: `constructor(params) { body }`.
///
/// Lowers to an associated `fn new(params) -> Self { Self { fields } }` in Rust.
/// Constructor parameters may have visibility modifiers (`public`/`private`), which
/// auto-generate struct fields and `self.field = param` assignments.
#[derive(Debug, Clone)]
pub struct ClassConstructor {
    /// The constructor parameters (may include parameter properties).
    pub params: Vec<ConstructorParam>,
    /// The constructor body.
    pub body: Block,
    /// The span covering the constructor declaration.
    pub span: Span,
}

/// A constructor parameter, optionally a parameter property.
///
/// When `property_visibility` is `Some`, this parameter auto-generates a struct
/// field with the matching visibility and a `self.field = param` assignment.
#[derive(Debug, Clone)]
pub struct ConstructorParam {
    /// Visibility if this is a parameter property (auto-generates a field).
    pub property_visibility: Option<Visibility>,
    /// The parameter name.
    pub name: Ident,
    /// The type annotation.
    pub type_ann: TypeAnnotation,
    /// The span covering the parameter.
    pub span: Span,
}

/// A class method: `[private|public] [static] [async] name(params): ReturnType { body }`.
///
/// Lowers to a method in an `impl` block with `&self` or `&mut self`.
/// Static methods lower to associated functions (no `self` parameter).
/// `async` methods lower to `async fn` in Rust.
#[derive(Debug, Clone)]
pub struct ClassMethod {
    /// Whether this is an `async` method.
    pub is_async: bool,
    /// Whether this is a static method (no `&self`).
    pub is_static: bool,
    /// The visibility modifier (`public` or `private`).
    pub visibility: Visibility,
    /// The method name.
    pub name: Ident,
    /// Optional generic type parameters.
    pub type_params: Option<TypeParams>,
    /// The parameter list (excluding the implicit `this`/`self`).
    pub params: Vec<Param>,
    /// The return type annotation, if present. Absent means `void`.
    pub return_type: Option<ReturnTypeAnnotation>,
    /// The method body.
    pub body: Block,
    /// The span covering the method declaration.
    pub span: Span,
}

/// A getter accessor in a class: `get name(): Type { ... }`.
///
/// Lowers to a `fn name(&self) -> Type` method in the impl block.
/// Property-style access (`obj.name`) is transformed to `obj.name()` at call sites.
#[derive(Debug, Clone)]
pub struct ClassGetter {
    /// The visibility modifier.
    pub visibility: Visibility,
    /// The getter name.
    pub name: Ident,
    /// The return type annotation.
    pub return_type: Option<ReturnTypeAnnotation>,
    /// The getter body.
    pub body: Block,
    /// The span covering the getter declaration.
    pub span: Span,
}

/// A setter accessor in a class: `set name(value: Type) { ... }`.
///
/// Lowers to a `fn set_name(&mut self, value: Type)` method in the impl block.
/// Property-style assignment (`obj.name = x`) is transformed to `obj.set_name(x)`.
#[derive(Debug, Clone)]
pub struct ClassSetter {
    /// The visibility modifier.
    pub visibility: Visibility,
    /// The setter name.
    pub name: Ident,
    /// The setter parameter.
    pub param: Param,
    /// The setter body.
    pub body: Block,
    /// The span covering the setter declaration.
    pub span: Span,
}

/// A field in a type definition.
///
/// Corresponds to `name: Type` within a type definition body.
/// Lowers to a `pub` field in the Rust struct.
#[derive(Debug, Clone)]
pub struct FieldDef {
    /// The field name.
    pub name: Ident,
    /// The field type annotation.
    pub type_ann: TypeAnnotation,
    /// The span covering the field definition.
    pub span: Span,
}

/// A function parameter with a name and type annotation.
///
/// Corresponds to `name: Type`, `name?: Type`, `name: Type = default`,
/// or `...name: Array<Type>` in a function parameter list.
#[derive(Debug, Clone)]
pub struct Param {
    /// The parameter name.
    pub name: Ident,
    /// The type annotation.
    pub type_ann: TypeAnnotation,
    /// Whether this parameter is optional (`name?:` syntax).
    /// Lowers to `Option<T>` in Rust with `None` at call sites.
    pub optional: bool,
    /// Optional default value expression.
    /// When present, the default is inlined at call sites that omit this argument.
    pub default_value: Option<Expr>,
    /// Whether this is a rest parameter (`...name` syntax).
    /// Must be the last parameter. Lowers to `Vec<T>` in Rust.
    pub is_rest: bool,
    /// The span covering the parameter.
    pub span: Span,
}

/// A type annotation on a variable or parameter.
///
/// Wraps a [`TypeKind`] with a source span.
#[derive(Debug, Clone)]
pub struct TypeAnnotation {
    /// The kind of type being annotated.
    pub kind: TypeKind,
    /// The span covering the type annotation.
    pub span: Span,
}

/// The kinds of types expressible in `RustScript`.
///
/// `Named` covers primitive types (`i32`, `i64`, `f64`, `bool`, `string`) and
/// user-defined types. `Void` represents the absence of a return value.
/// `Generic` represents parameterized types like `Array<string>`.
/// `Union` represents `T | null` syntax (only `T | null` is currently supported).
#[derive(Debug, Clone)]
pub enum TypeKind {
    /// A named type (e.g., `i32`, `bool`, `string`, or a user-defined name).
    Named(Ident),
    /// The void type, indicating no return value. Lowers to Rust `()`.
    Void,
    /// A generic type instantiation: `Array<string>`, `Map<string, u32>`.
    /// The `Ident` is the base type name; the `Vec` is the type arguments.
    Generic(Ident, Vec<TypeAnnotation>),
    /// Union type: `T | null`. Only `T | null` is currently supported.
    /// Lowers to `Option<T>` in Rust.
    Union(Vec<TypeAnnotation>),
    /// Function type: `(i32, i32) => i32`.
    /// Lowers to `impl Fn(i32, i32) -> i32` in Rust.
    Function(Vec<TypeAnnotation>, Box<TypeAnnotation>),
    /// Intersection type: `Serializable & Printable`.
    /// Used for trait bounds in function parameters.
    /// Lowers to generic type parameters with multiple bounds: `T: A + B`.
    Intersection(Vec<TypeAnnotation>),
    /// An inferred type — the type annotation was omitted.
    /// Only valid in closure parameters (e.g., `(n) => n * 2`).
    Inferred,
    /// A shared type: `shared<T>` in `RustScript`.
    /// Lowers to `Arc<Mutex<T>>` in Rust.
    Shared(Box<TypeAnnotation>),
}

/// An identifier with its source span.
#[derive(Debug, Clone)]
pub struct Ident {
    /// The identifier text.
    pub name: String,
    /// The span covering the identifier.
    pub span: Span,
}

/// A block of statements enclosed in braces.
///
/// Corresponds to `{ stmt; stmt; ... }` in `RustScript`.
#[derive(Debug, Clone)]
pub struct Block {
    /// The statements within the block.
    pub stmts: Vec<Stmt>,
    /// The span covering the entire block, including braces.
    pub span: Span,
}

/// A statement within a block.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// A variable declaration (`const` or `let` binding).
    VarDecl(VarDecl),
    /// An expression statement.
    Expr(Expr),
    /// A `return` statement.
    Return(ReturnStmt),
    /// An `if`/`else` statement.
    If(IfStmt),
    /// A `while` loop.
    While(WhileStmt),
    /// A destructuring declaration: `const { name, age } = user;`.
    /// Lowers to Rust `let TypeName { field1, field2, .. } = expr;`.
    Destructure(DestructureStmt),
    /// A `switch` statement for pattern matching over enums.
    /// Lowers to a Rust `match` expression.
    Switch(SwitchStmt),
    /// A `try { ... } catch (name: Type) { ... }` statement.
    /// Lowers to a Rust `match` on `Result`.
    TryCatch(TryCatchStmt),
    /// A `for (const/let x of items) { ... }` loop.
    /// Lowers to Rust `for x in &items { ... }`.
    For(ForOfStmt),
    /// An array destructuring declaration: `const [a, b] = expr;`.
    /// Lowers to Rust tuple destructuring: `let (a, b) = expr;`.
    ArrayDestructure(ArrayDestructureStmt),
    /// A `break;` statement.
    Break(BreakStmt),
    /// A `continue;` statement.
    Continue(ContinueStmt),
    /// A raw Rust code block in a function body (`rust { ... }`).
    /// The contents are passed through to the generated `.rs` file unchanged.
    RustBlock(InlineRustBlock),
}

/// A variable declaration with an initializer.
///
/// Corresponds to `const name: Type = expr` or `let name: Type = expr`.
/// `const` lowers to Rust `let` (immutable), `let` lowers to `let mut`.
#[derive(Debug, Clone)]
pub struct VarDecl {
    /// Whether this is a `const` or `let` binding.
    pub binding: VarBinding,
    /// The variable name.
    pub name: Ident,
    /// The optional type annotation.
    pub type_ann: Option<TypeAnnotation>,
    /// The initializer expression.
    pub init: Expr,
    /// The span covering the entire declaration.
    pub span: Span,
}

/// The binding kind for a variable declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarBinding {
    /// An immutable binding (`const`). Lowers to Rust `let`.
    Const,
    /// A mutable binding (`let`). Lowers to Rust `let mut`.
    Let,
}

/// A `return` statement, optionally with a value.
///
/// Corresponds to `return expr;` or bare `return;`.
#[derive(Debug, Clone)]
pub struct ReturnStmt {
    /// The return value, if present.
    pub value: Option<Expr>,
    /// The span covering the `return` keyword and value.
    pub span: Span,
}

/// An `if`/`else` statement.
///
/// Supports `else if` chains via [`ElseClause::ElseIf`].
#[derive(Debug, Clone)]
pub struct IfStmt {
    /// The condition expression.
    pub condition: Expr,
    /// The then-branch block.
    pub then_block: Block,
    /// The optional else clause (block or else-if chain).
    pub else_clause: Option<ElseClause>,
    /// The span covering the entire `if`/`else` statement.
    pub span: Span,
}

/// The else clause of an `if` statement.
#[derive(Debug, Clone)]
pub enum ElseClause {
    /// A plain `else { ... }` block.
    Block(Block),
    /// An `else if ...` chain.
    ElseIf(Box<IfStmt>),
}

/// A `while` loop.
///
/// Corresponds to `while (condition) { body }`.
#[derive(Debug, Clone)]
pub struct WhileStmt {
    /// The loop condition expression.
    pub condition: Expr,
    /// The loop body.
    pub body: Block,
    /// The span covering the entire `while` statement.
    pub span: Span,
}

/// A destructuring declaration: `const { name, age } = user;`.
///
/// Lowers to Rust `let TypeName { field1, field2, .. } = expr;`.
#[derive(Debug, Clone)]
pub struct DestructureStmt {
    /// Whether this is a `const` or `let` binding.
    pub binding: VarBinding,
    /// The field names being extracted.
    pub fields: Vec<Ident>,
    /// The initializer expression being destructured.
    pub init: Expr,
    /// The span covering the entire destructuring statement.
    pub span: Span,
}

/// An array destructuring declaration: `const [a, b] = expr;`.
///
/// Lowers to Rust tuple destructuring: `let (a, b) = expr;`.
/// Used primarily for `Promise.all` results where the concurrent
/// futures return a tuple.
#[derive(Debug, Clone)]
pub struct ArrayDestructureStmt {
    /// Whether this is a `const` or `let` binding.
    pub binding: VarBinding,
    /// The element names being extracted (positional).
    pub elements: Vec<Ident>,
    /// The initializer expression being destructured.
    pub init: Expr,
    /// The span covering the entire array destructuring statement.
    pub span: Span,
}

/// A switch statement for pattern matching over enums.
///
/// Corresponds to `switch (expr) { case "variant": stmts; ... }`.
/// Lowers to a Rust `match` expression with enum variant patterns.
#[derive(Debug, Clone)]
pub struct SwitchStmt {
    /// The scrutinee expression being matched.
    pub scrutinee: Expr,
    /// The case arms.
    pub cases: Vec<SwitchCase>,
    /// The span covering the entire switch statement.
    pub span: Span,
}

/// A single case in a switch statement.
///
/// Corresponds to `case "variant_name": body_stmts;`.
/// Lowers to a single match arm.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// The pattern string literal (enum variant name, e.g., `"north"` or `"circle"`).
    pub pattern: String,
    /// The body of the case.
    pub body: Vec<Stmt>,
    /// The span covering this case.
    pub span: Span,
}

/// A `try/catch/finally` statement for catching `Result` errors.
///
/// Corresponds to `try { ... } catch (name: ErrorType) { ... } finally { ... }`.
/// Lowers to a Rust `match` on `Ok`/`Err` with optional cleanup statements after.
/// Supports `try {} catch {} finally {}`, `try {} catch {}`, and `try {} finally {}`.
#[derive(Debug, Clone)]
pub struct TryCatchStmt {
    /// The try block containing fallible operations.
    pub try_block: Block,
    /// The catch binding name (None for `try {} finally {}` without catch).
    pub catch_binding: Option<Ident>,
    /// The optional error type annotation.
    pub catch_type: Option<TypeAnnotation>,
    /// The catch block executed when an error occurs (None for `try {} finally {}`).
    pub catch_block: Option<Block>,
    /// Optional finally block — runs after both try and catch.
    pub finally_block: Option<Block>,
    /// The span covering the entire try/catch/finally statement.
    pub span: Span,
}

/// A for-of loop: `for (const x of items) { ... }`.
///
/// Corresponds to `RustScript` `for (const/let IDENT of EXPR) { body }`.
/// Lowers to Rust `for x in &items { body }`.
#[derive(Debug, Clone)]
pub struct ForOfStmt {
    /// The binding kind (`const` or `let`).
    pub binding: VarBinding,
    /// The loop variable name.
    pub variable: Ident,
    /// The iterable expression.
    pub iterable: Expr,
    /// The loop body.
    pub body: Block,
    /// The span covering the entire for-of statement.
    pub span: Span,
}

/// A `break` statement.
///
/// Terminates the innermost loop. No labeled breaks are supported.
#[derive(Debug, Clone)]
pub struct BreakStmt {
    /// The span covering the `break;` keyword and semicolon.
    pub span: Span,
}

/// A `continue` statement.
///
/// Skips to the next iteration of the innermost loop. No labeled continues are supported.
#[derive(Debug, Clone)]
pub struct ContinueStmt {
    /// The span covering the `continue;` keyword and semicolon.
    pub span: Span,
}

/// A raw Rust code block that passes through to the generated `.rs` file unchanged.
///
/// Syntax: `rust { <raw rust code> }`
/// The contents are not parsed as `RustScript` — they are preserved as-is.
/// Appears both as a statement in function bodies and as a top-level item.
#[derive(Debug, Clone)]
pub struct InlineRustBlock {
    /// The raw Rust source code inside the block (excluding the outer braces).
    pub code: String,
    /// The span covering the entire `rust { ... }` block.
    pub span: Span,
}

/// An expression with its source span.
///
/// Wraps an [`ExprKind`] variant with the span of source text it was parsed from.
#[derive(Debug, Clone)]
pub struct Expr {
    /// The kind of expression.
    pub kind: ExprKind,
    /// The span covering the expression.
    pub span: Span,
}

/// The kinds of expressions in `RustScript`.
#[derive(Debug, Clone)]
pub enum ExprKind {
    /// An integer literal (e.g., `42`).
    IntLit(i64),
    /// A floating-point literal (e.g., `3.14`).
    FloatLit(f64),
    /// A string literal (e.g., `"hello"`).
    StringLit(String),
    /// A boolean literal (`true` or `false`).
    BoolLit(bool),
    /// An identifier reference (e.g., `x`).
    Ident(Ident),
    /// A binary operation (e.g., `a + b`).
    Binary(BinaryExpr),
    /// A unary operation (e.g., `-x`, `!flag`).
    Unary(UnaryExpr),
    /// A function call (e.g., `foo(a, b)`).
    Call(CallExpr),
    /// A method call (e.g., `obj.method(a)`).
    MethodCall(MethodCallExpr),
    /// A parenthesized expression (e.g., `(a + b)`).
    Paren(Box<Expr>),
    /// An assignment expression (e.g., `x = 5`).
    Assign(AssignExpr),
    /// Field assignment: `obj.field = value` (e.g., `this.count = 0`).
    /// Used for `this.field = value` in class methods/constructors.
    FieldAssign(FieldAssignExpr),
    /// A struct literal: `{ name: "Alice", age: 30 }` or `User { ... }`.
    /// Lowers to a Rust struct construction expression.
    StructLit(StructLitExpr),
    /// Field access: `user.name`.
    /// Lowers to Rust field access `expr.field`.
    FieldAccess(FieldAccessExpr),
    /// Template literal: `` `Hello, ${name}!` ``.
    /// Lowers to `format!("Hello, {}!", name)` or a simple string for no-interpolation cases.
    TemplateLit(TemplateLitExpr),
    /// Array literal: `[1, 2, 3]`.
    /// Lowers to `vec![1, 2, 3]` in Rust.
    ArrayLit(Vec<Expr>),
    /// Constructor call: `new Map()`, `new Set()`, `new Array()`.
    /// Lowers to `HashMap::new()`, `HashSet::new()`, or `Vec::new()`.
    New(NewExpr),
    /// Index access: `arr[0]`, `map["key"]`.
    /// Lowers to Rust index syntax `expr[index]`.
    Index(IndexExpr),
    /// The `null` literal. Lowers to `None` in Rust.
    NullLit,
    /// Optional chaining: `expr?.field` or `expr?.method(args)`.
    /// Lowers to `expr.as_ref().map(|v| v.field)` or equivalent.
    OptionalChain(OptionalChainExpr),
    /// Nullish coalescing: `expr ?? default`.
    /// Lowers to `expr.unwrap_or(default)`.
    NullishCoalescing(NullishCoalescingExpr),
    /// A `throw` expression: `throw expr`.
    /// In a `throws` function, lowers to `return Err(expr)`.
    Throw(Box<Expr>),
    /// Arrow function / closure: `(x: i32): i32 => x * 2` or `() => { ... }`.
    /// Lowers to a Rust closure expression.
    Closure(ClosureExpr),
    /// The `this` keyword — refers to the current class instance.
    /// Lowers to `self` in Rust methods.
    This,
    /// An `await` expression: `await expr`.
    /// Lowers to Rust's postfix `.await`: `expr.await`.
    Await(Box<Expr>),
    /// A `shared(expr)` constructor: wraps a value in `Arc::new(Mutex::new(expr))`.
    Shared(Box<Expr>),
    /// A spread argument in a function call: `...expr`.
    /// In function calls to rest-parameter functions, passes a `Vec<T>` directly.
    SpreadArg(Box<Expr>),
    /// Ternary conditional: `condition ? consequent : alternate`.
    /// Lowers to `if condition { consequent } else { alternate }`.
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Non-null assertion: `expr!`. Asserts the value is not null/None.
    /// Lowers to `expr.unwrap()`.
    NonNullAssert(Box<Expr>),
    /// Type cast: `expr as Type`.
    /// Lowers to Rust `expr as Type` for numeric casts.
    Cast(Box<Expr>, TypeAnnotation),
    /// typeof operator: `typeof expr`.
    /// Lowers to a string literal for known types at compile time.
    TypeOf(Box<Expr>),
}

/// A binary expression with an operator and two operands.
#[derive(Debug, Clone)]
pub struct BinaryExpr {
    /// The binary operator.
    pub op: BinaryOp,
    /// The left-hand operand.
    pub left: Box<Expr>,
    /// The right-hand operand.
    pub right: Box<Expr>,
}

/// Binary operators in `RustScript`.
///
/// Arithmetic, comparison, and logical operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Sub,
    /// Multiplication (`*`).
    Mul,
    /// Division (`/`).
    Div,
    /// Modulo (`%`).
    Mod,
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
    /// Exponentiation (`**`). Lowers to `.pow()` or `.powf()`.
    Pow,
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

impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::And => "&&",
            Self::Or => "||",
            Self::Pow => "**",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::BitXor => "^",
            Self::Shl => "<<",
            Self::Shr => ">>",
        };
        f.write_str(s)
    }
}

/// A unary expression with an operator and operand.
#[derive(Debug, Clone)]
pub struct UnaryExpr {
    /// The unary operator.
    pub op: UnaryOp,
    /// The operand.
    pub operand: Box<Expr>,
}

/// Unary operators in `RustScript`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Arithmetic negation (`-`).
    Neg,
    /// Logical NOT (`!`).
    Not,
    /// Bitwise NOT (`~`). Lowers to Rust `!` on integer types.
    BitNot,
}

/// A function call expression.
///
/// Corresponds to `callee(args...)`. Lowers to a Rust function call.
#[derive(Debug, Clone)]
pub struct CallExpr {
    /// The function being called.
    pub callee: Ident,
    /// The argument list.
    pub args: Vec<Expr>,
}

/// A method call expression.
///
/// Corresponds to `object.method(args...)`.
#[derive(Debug, Clone)]
pub struct MethodCallExpr {
    /// The receiver object.
    pub object: Box<Expr>,
    /// The method name.
    pub method: Ident,
    /// The argument list.
    pub args: Vec<Expr>,
}

/// An assignment expression.
///
/// Corresponds to `target = value`. Supports simple identifier targets,
/// field access targets, and indexed targets.
#[derive(Debug, Clone)]
pub struct AssignExpr {
    /// The assignment target.
    pub target: Ident,
    /// The value being assigned.
    pub value: Box<Expr>,
}

/// A field assignment expression: `obj.field = value`.
///
/// Produced when the LHS of an assignment is a field access (e.g., `this.count = 0`).
/// Lowers to `self.field = value` in Rust methods.
#[derive(Debug, Clone)]
pub struct FieldAssignExpr {
    /// The object being assigned to (e.g., `this`).
    pub object: Box<Expr>,
    /// The field name being assigned to.
    pub field: Ident,
    /// The value being assigned.
    pub value: Box<Expr>,
}

/// A struct literal expression.
///
/// Corresponds to `{ name: "Alice", age: 30 }` in expression position.
/// The `type_name` is resolved during lowering from context (e.g., the
/// variable's type annotation).
#[derive(Debug, Clone)]
pub struct StructLitExpr {
    /// The type name (for typed literals like `User { ... }`). None for
    /// untyped object literals.
    pub type_name: Option<Ident>,
    /// The field initializers.
    pub fields: Vec<FieldInit>,
}

/// A field initializer in a struct literal.
///
/// Corresponds to `name: expr` within a struct literal body.
#[derive(Debug, Clone)]
pub struct FieldInit {
    /// The field name.
    pub name: Ident,
    /// The field value expression.
    pub value: Expr,
    /// The span covering the field initializer.
    pub span: Span,
}

/// A field access expression: `expr.field`.
///
/// Supports chaining: `user.address.city` is
/// `FieldAccess(FieldAccess(user, address), city)`.
/// Lowers to Rust field access syntax.
#[derive(Debug, Clone)]
pub struct FieldAccessExpr {
    /// The object expression being accessed.
    pub object: Box<Expr>,
    /// The field name.
    pub field: Ident,
}

/// A template literal expression: `` `Hello, ${name}!` ``.
///
/// Contains alternating string segments and interpolated expressions.
/// Lowers to `format!("Hello, {}!", name)` when interpolations are present,
/// or to a simple `"text".to_string()` when there are none.
#[derive(Debug, Clone)]
pub struct TemplateLitExpr {
    /// The parts of the template, alternating between string segments and expressions.
    pub parts: Vec<TemplatePart>,
}

/// A part of a template literal — either a string segment or an interpolated expression.
#[derive(Debug, Clone)]
pub enum TemplatePart {
    /// A literal string segment.
    String(String, Span),
    /// An interpolated expression: `${expr}`.
    Expr(Expr),
}

/// A `new` constructor call: `new Map()`, `new Set<string>()`.
///
/// Lowers to a static method call like `HashMap::new()` or `HashSet::new()`.
#[derive(Debug, Clone)]
pub struct NewExpr {
    /// The type name being constructed (e.g., `Map`, `Set`, `Array`).
    pub type_name: Ident,
    /// Optional generic type arguments (e.g., `<string, u32>`).
    pub type_args: Vec<TypeAnnotation>,
    /// The constructor arguments.
    pub args: Vec<Expr>,
}

/// Index access expression: `arr[0]`, `map["key"]`.
///
/// Supports chaining: `arr[0][1]` is `Index(Index(arr, 0), 1)`.
/// Lowers to Rust index syntax `expr[index]`.
#[derive(Debug, Clone)]
pub struct IndexExpr {
    /// The object being indexed.
    pub object: Box<Expr>,
    /// The index expression.
    pub index: Box<Expr>,
}

/// Optional chaining expression: `expr?.field` or `expr?.method(args)`.
///
/// Lowers to `expr.as_ref().map(|v| v.field)` or equivalent.
#[derive(Debug, Clone)]
pub struct OptionalChainExpr {
    /// The object expression (must be `T | null`).
    pub object: Box<Expr>,
    /// The kind of access after `?.`.
    pub access: OptionalAccess,
}

/// The kind of access in an optional chaining expression.
#[derive(Debug, Clone)]
pub enum OptionalAccess {
    /// Field access: `expr?.field`.
    Field(Ident),
    /// Method call: `expr?.method(args)`.
    Method(Ident, Vec<Expr>),
}

/// Nullish coalescing expression: `expr ?? default`.
///
/// Lowers to `expr.unwrap_or(default)` or `expr.unwrap_or_else(|| default)`.
#[derive(Debug, Clone)]
pub struct NullishCoalescingExpr {
    /// The left-hand side (must be `T | null`).
    pub left: Box<Expr>,
    /// The default value to use when left is `null`.
    pub right: Box<Expr>,
}

/// A closure / arrow function expression.
///
/// Corresponds to `[async] (params): ReturnType => body` in `RustScript`.
/// Lowers to a Rust closure expression: `[async] |params| -> RetType { body }`.
#[derive(Debug, Clone)]
pub struct ClosureExpr {
    /// Whether this is an `async` closure.
    pub is_async: bool,
    /// Whether this is a `move` closure.
    pub is_move: bool,
    /// Parameters (with type annotations).
    pub params: Vec<Param>,
    /// Return type annotation (optional).
    pub return_type: Option<TypeAnnotation>,
    /// The body — either a single expression or a block.
    pub body: ClosureBody,
}

/// The body of a closure — either a single expression or a block.
///
/// Expression body: `(x) => x * 2` — implicit return.
/// Block body: `() => { stmt; stmt; return value; }` — explicit statements.
#[derive(Debug, Clone)]
pub enum ClosureBody {
    /// Expression body: `(x) => x * 2`.
    Expr(Box<Expr>),
    /// Block body: `() => { stmt; stmt; return value; }`.
    Block(Block),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a span for tests.
    fn span(start: u32, end: u32) -> Span {
        Span::new(start, end)
    }

    /// Helper to create an identifier for tests.
    fn ident(name: &str, start: u32, end: u32) -> Ident {
        Ident {
            name: name.to_owned(),
            span: span(start, end),
        }
    }

    /// Helper to create a simple integer expression for tests.
    fn int_expr(value: i64, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::IntLit(value),
            span: span(start, end),
        }
    }

    #[test]
    fn test_fn_decl_with_two_params_field_access() {
        let decl = FnDecl {
            is_async: false,
            name: ident("add", 0, 3),
            type_params: None,
            params: vec![
                Param {
                    name: ident("a", 4, 5),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("i32", 7, 10)),
                        span: span(7, 10),
                    },
                    optional: false,
                    default_value: None,
                    is_rest: false,
                    span: span(4, 10),
                },
                Param {
                    name: ident("b", 12, 13),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("i32", 15, 18)),
                        span: span(15, 18),
                    },
                    optional: false,
                    default_value: None,
                    is_rest: false,
                    span: span(12, 18),
                },
            ],
            return_type: Some(ReturnTypeAnnotation {
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 21, 24)),
                    span: span(21, 24),
                }),
                throws: None,
                span: span(21, 24),
            }),
            body: Block {
                stmts: vec![],
                span: span(25, 27),
            },
            span: span(0, 27),
        };

        assert_eq!(decl.name.name, "add");
        assert_eq!(decl.params.len(), 2);
        assert_eq!(decl.params[0].name.name, "a");
        assert_eq!(decl.params[1].name.name, "b");
        assert!(decl.return_type.is_some());
    }

    #[test]
    fn test_if_stmt_with_else_if_chain_nesting() {
        let inner_if = IfStmt {
            condition: Expr {
                kind: ExprKind::BoolLit(false),
                span: span(30, 35),
            },
            then_block: Block {
                stmts: vec![],
                span: span(36, 38),
            },
            else_clause: Some(ElseClause::Block(Block {
                stmts: vec![],
                span: span(44, 46),
            })),
            span: span(25, 46),
        };

        let outer_if = IfStmt {
            condition: Expr {
                kind: ExprKind::BoolLit(true),
                span: span(3, 7),
            },
            then_block: Block {
                stmts: vec![],
                span: span(8, 10),
            },
            else_clause: Some(ElseClause::ElseIf(Box::new(inner_if))),
            span: span(0, 46),
        };

        // Verify nesting: outer else clause is ElseIf
        match &outer_if.else_clause {
            Some(ElseClause::ElseIf(inner)) => {
                // Inner else clause is a Block
                assert!(matches!(inner.else_clause, Some(ElseClause::Block(_))));
            }
            _ => panic!("expected ElseIf clause"),
        }
    }

    #[test]
    fn test_while_stmt_with_var_decl_and_assign() {
        let var_decl = Stmt::VarDecl(VarDecl {
            binding: VarBinding::Let,
            name: ident("x", 10, 11),
            type_ann: None,
            init: int_expr(0, 14, 15),
            span: span(6, 16),
        });

        let assign = Stmt::Expr(Expr {
            kind: ExprKind::Assign(AssignExpr {
                target: ident("x", 17, 18),
                value: Box::new(Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Add,
                        left: Box::new(Expr {
                            kind: ExprKind::Ident(ident("x", 21, 22)),
                            span: span(21, 22),
                        }),
                        right: Box::new(int_expr(1, 25, 26)),
                    }),
                    span: span(21, 26),
                }),
            }),
            span: span(17, 26),
        });

        let while_stmt = WhileStmt {
            condition: Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Lt,
                    left: Box::new(Expr {
                        kind: ExprKind::Ident(ident("x", 0, 1)),
                        span: span(0, 1),
                    }),
                    right: Box::new(int_expr(10, 4, 6)),
                }),
                span: span(0, 6),
            },
            body: Block {
                stmts: vec![var_decl, assign],
                span: span(6, 27),
            },
            span: span(0, 27),
        };

        assert_eq!(while_stmt.body.stmts.len(), 2);
        assert!(matches!(while_stmt.body.stmts[0], Stmt::VarDecl(_)));
        assert!(matches!(while_stmt.body.stmts[1], Stmt::Expr(_)));
    }

    #[test]
    fn test_every_expr_kind_variant_spans_stored() {
        let cases: Vec<Expr> = vec![
            Expr {
                kind: ExprKind::IntLit(42),
                span: span(0, 2),
            },
            Expr {
                kind: ExprKind::FloatLit(3.14),
                span: span(10, 14),
            },
            Expr {
                kind: ExprKind::StringLit("hello".to_owned()),
                span: span(20, 27),
            },
            Expr {
                kind: ExprKind::BoolLit(true),
                span: span(30, 34),
            },
            Expr {
                kind: ExprKind::Ident(ident("x", 40, 41)),
                span: span(40, 41),
            },
            Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Add,
                    left: Box::new(int_expr(1, 50, 51)),
                    right: Box::new(int_expr(2, 54, 55)),
                }),
                span: span(50, 55),
            },
            Expr {
                kind: ExprKind::Unary(UnaryExpr {
                    op: UnaryOp::Neg,
                    operand: Box::new(int_expr(5, 61, 62)),
                }),
                span: span(60, 62),
            },
            Expr {
                kind: ExprKind::Call(CallExpr {
                    callee: ident("foo", 70, 73),
                    args: vec![],
                }),
                span: span(70, 75),
            },
            Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(Expr {
                        kind: ExprKind::Ident(ident("obj", 80, 83)),
                        span: span(80, 83),
                    }),
                    method: ident("bar", 84, 87),
                    args: vec![],
                }),
                span: span(80, 89),
            },
            Expr {
                kind: ExprKind::Paren(Box::new(int_expr(1, 91, 92))),
                span: span(90, 93),
            },
            Expr {
                kind: ExprKind::Assign(AssignExpr {
                    target: ident("x", 100, 101),
                    value: Box::new(int_expr(5, 104, 105)),
                }),
                span: span(100, 105),
            },
        ];

        // Verify each variant stores its span correctly.
        let expected_starts = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        for (expr, &expected_start) in cases.iter().zip(&expected_starts) {
            assert_eq!(
                expr.span.start.0,
                expected_start,
                "span mismatch for {:?}",
                std::mem::discriminant(&expr.kind)
            );
        }
    }

    #[test]
    fn test_var_decl_const_and_let_with_and_without_type_ann() {
        let const_with_type = VarDecl {
            binding: VarBinding::Const,
            name: ident("x", 6, 7),
            type_ann: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("i32", 9, 12)),
                span: span(9, 12),
            }),
            init: int_expr(42, 15, 17),
            span: span(0, 18),
        };

        let let_without_type = VarDecl {
            binding: VarBinding::Let,
            name: ident("y", 4, 5),
            type_ann: None,
            init: int_expr(99, 8, 10),
            span: span(0, 11),
        };

        assert_eq!(const_with_type.binding, VarBinding::Const);
        assert!(const_with_type.type_ann.is_some());

        assert_eq!(let_without_type.binding, VarBinding::Let);
        assert!(let_without_type.type_ann.is_none());
    }

    #[test]
    fn test_binary_op_display_all_variants() {
        assert_eq!(BinaryOp::Add.to_string(), "+");
        assert_eq!(BinaryOp::Sub.to_string(), "-");
        assert_eq!(BinaryOp::Mul.to_string(), "*");
        assert_eq!(BinaryOp::Div.to_string(), "/");
        assert_eq!(BinaryOp::Mod.to_string(), "%");
        assert_eq!(BinaryOp::Eq.to_string(), "==");
        assert_eq!(BinaryOp::Ne.to_string(), "!=");
        assert_eq!(BinaryOp::Lt.to_string(), "<");
        assert_eq!(BinaryOp::Gt.to_string(), ">");
        assert_eq!(BinaryOp::Le.to_string(), "<=");
        assert_eq!(BinaryOp::Ge.to_string(), ">=");
        assert_eq!(BinaryOp::And.to_string(), "&&");
        assert_eq!(BinaryOp::Or.to_string(), "||");
    }
}
