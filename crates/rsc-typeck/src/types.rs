//! The canonical type representation used throughout the compiler.
//!
//! Every `RustScript` type resolves to a [`Type`]. This is the single source of
//! truth for what a value's type is, used by the type checker, lowering pass,
//! and emitter.

/// The canonical type representation used throughout the compiler.
///
/// Every `RustScript` type resolves to a `Type`. This is the single source of truth
/// for what a value's type is, used by the type checker, lowering pass, and emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Primitive types that map directly to Rust.
    Primitive(PrimitiveType),
    /// The `string` type — always `String` (owned) in Rust.
    String,
    /// The unit type `()` — used for `void` returns.
    Unit,
    /// A user-defined named type (struct, enum, type alias).
    /// The String is the type name. Generic arguments are tracked separately.
    Named(String),
    /// A generic instantiation: `Array<i32>`, `Map<string, u32>`, etc.
    Generic(String, Vec<Type>),
    /// `Option<T>` — from `T | null`.
    Option(Box<Type>),
    /// `Result<T, E>` — from `throws E`.
    Result(Box<Type>, Box<Type>),
    /// A function type: `(param_types) -> return_type`.
    /// Used for closures and function references.
    Function(Vec<Type>, Box<Type>),
    /// A type variable (for generic type parameters like `T`).
    TypeVar(String),
    /// `Arc<Mutex<T>>` — from `shared<T>`.
    ArcMutex(Box<Type>),
    /// A tuple type: `(T1, T2, ...)` — from `[T1, T2]`.
    Tuple(Vec<Type>),
    /// The `unknown` type — the type-safe top type.
    /// Lowers to `Box<dyn std::any::Any>` in Rust.
    Unknown,
    /// A general union type: `string | i32`, `string | i32 | bool`.
    /// Distinguished from `Option` (`T | null`). The members are the non-null
    /// component types, sorted alphabetically by their canonical names for
    /// deterministic enum name generation.
    Union(Vec<Type>),
    /// Type could not be resolved — used for error recovery.
    Error,
}

/// Primitive numeric and boolean types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveType {
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
}
