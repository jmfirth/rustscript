//! Builtin function and method registry for the lowering pass.
//!
//! Data-driven mapping of known method calls (e.g., `console.log`) to their
//! Rust equivalents (e.g., `println!`). New builtins are added by writing
//! a lowering function and registering it — no changes to the transform module.
//!
//! String methods (e.g., `.toUpperCase()`, `.split()`) are registered
//! separately from object-based builtins because their receiver is any
//! expression of type `String`, not a named object like `"console"`.
//!
//! Collection methods (e.g., `.map()`, `.filter()`, `.reduce()`) are registered
//! for array/collection types and produce `IteratorChain` IR nodes that emit
//! as Rust iterator chains.

use std::collections::HashMap;

use rsc_syntax::rust_ir::{
    IteratorOp, IteratorTerminal, RustBlock, RustClosureBody, RustClosureParam, RustExpr,
    RustExprKind, RustType,
};

#[cfg(test)]
use rsc_syntax::rust_ir::RustBinaryOp;
use rsc_syntax::span::Span;

/// How a builtin call is lowered to Rust IR.
///
/// Receives already-lowered arguments (`Vec<RustExpr>`), NOT raw AST expressions.
/// This keeps `builtins.rs` independent of `transform.rs`.
pub(crate) type BuiltinLowering = fn(args: Vec<RustExpr>, arg_span: Span) -> RustExpr;

/// Metadata for a registered builtin method.
struct BuiltinEntry {
    /// The lowering function.
    lowering: BuiltinLowering,
    /// Whether the builtin's arguments are passed by reference (not moved).
    ref_args: bool,
}

/// How a string method call is lowered to Rust IR.
///
/// Receives the already-lowered receiver expression and arguments.
/// The receiver is the expression the method is called on (e.g., `name` in
/// `name.toUpperCase()`). This signature differs from `BuiltinLowering`
/// because string methods have a receiver, while object-based builtins
/// like `console.log` do not.
pub(crate) type StringMethodLowering =
    fn(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr;

/// How a collection method call is lowered to an iterator chain IR node.
///
/// Receives the already-lowered receiver expression, arguments, and span.
/// Returns an `IteratorChain` `RustExpr` representing the full iterator operation.
pub(crate) type CollectionMethodLowering =
    fn(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr;

/// Registry of builtin functions and methods.
///
/// Maps `(object_name, method_name)` pairs to their lowering functions.
/// Uses a nested `HashMap` to avoid allocating on every lookup.
/// Registers `console.log` -> `println!`, string method mappings
/// (e.g., `.toUpperCase()` -> `.to_uppercase()`), and collection method
/// mappings (e.g., `.map()` -> `.iter().map().collect()`).
pub(crate) struct BuiltinRegistry {
    methods: HashMap<String, HashMap<String, BuiltinEntry>>,
    string_methods: HashMap<String, StringMethodLowering>,
    collection_methods: HashMap<String, CollectionMethodLowering>,
    /// Map/Set-specific methods that require type-aware dispatch.
    /// These are only consulted when the receiver is known to be a Map or Set.
    map_set_methods: HashMap<String, StringMethodLowering>,
    /// Date-specific instance methods that require type-aware dispatch.
    /// These are only consulted when the receiver is known to be a `Date` (`SystemTime`).
    date_methods: HashMap<String, StringMethodLowering>,
    /// Builtin free functions (e.g., `spawn`).
    functions: HashMap<String, BuiltinLowering>,
}

impl BuiltinRegistry {
    /// Create a new registry with the default builtins registered.
    pub fn new() -> Self {
        let mut registry = Self {
            methods: HashMap::new(),
            string_methods: HashMap::new(),
            collection_methods: HashMap::new(),
            map_set_methods: HashMap::new(),
            date_methods: HashMap::new(),
            functions: HashMap::new(),
        };
        register_defaults(&mut registry);
        registry
    }

    /// Register a builtin method.
    fn register_method(
        &mut self,
        object: &str,
        method: &str,
        lowering: BuiltinLowering,
        ref_args: bool,
    ) {
        self.methods
            .entry(object.to_owned())
            .or_default()
            .insert(method.to_owned(), BuiltinEntry { lowering, ref_args });
    }

    /// Register a string method lowering function.
    fn register_string_method(&mut self, name: &str, lowering: StringMethodLowering) {
        self.string_methods.insert(name.to_owned(), lowering);
    }

    /// Look up a string method by name.
    ///
    /// Returns the lowering function if the method is a known string method
    /// (e.g., `"toUpperCase"`, `"split"`, `"trim"`).
    pub fn lookup_string_method(&self, method: &str) -> Option<&StringMethodLowering> {
        self.string_methods.get(method)
    }

    /// Register a collection method lowering function.
    fn register_collection_method(&mut self, name: &str, lowering: CollectionMethodLowering) {
        self.collection_methods.insert(name.to_owned(), lowering);
    }

    /// Look up a collection method by name.
    ///
    /// Returns the lowering function if the method is a known collection method
    /// (e.g., `"map"`, `"filter"`, `"reduce"`, `"find"`).
    pub fn lookup_collection_method(&self, method: &str) -> Option<&CollectionMethodLowering> {
        self.collection_methods.get(method)
    }

    /// Register a Map/Set-specific method that requires type-aware dispatch.
    fn register_map_set_method(&mut self, name: &str, lowering: StringMethodLowering) {
        self.map_set_methods.insert(name.to_owned(), lowering);
    }

    /// Look up a Map/Set method by name.
    ///
    /// Returns the lowering function if the method is a known Map/Set method.
    /// Only consulted when the receiver is known to be a `HashMap` or `HashSet`.
    pub fn lookup_map_set_method(&self, method: &str) -> Option<&StringMethodLowering> {
        self.map_set_methods.get(method)
    }

    /// Register a Date-specific instance method that requires type-aware dispatch.
    fn register_date_method(&mut self, name: &str, lowering: StringMethodLowering) {
        self.date_methods.insert(name.to_owned(), lowering);
    }

    /// Look up a Date instance method by name.
    ///
    /// Returns the lowering function if the method is a known Date method.
    /// Only consulted when the receiver is known to be a `Date` (`SystemTime`).
    pub fn lookup_date_method(&self, method: &str) -> Option<&StringMethodLowering> {
        self.date_methods.get(method)
    }

    /// Register a builtin free function.
    fn register_function(&mut self, name: &str, lowering: BuiltinLowering) {
        self.functions.insert(name.to_owned(), lowering);
    }

    /// Look up a free function by name.
    ///
    /// Returns the lowering function if the name is a known builtin free function
    /// (e.g., `"spawn"`).
    pub fn lookup_function(&self, name: &str) -> Option<&BuiltinLowering> {
        self.functions.get(name)
    }

    /// Check if a method call is a builtin and return its lowering function.
    pub fn lookup_method(&self, object: &str, method: &str) -> Option<&BuiltinLowering> {
        self.methods
            .get(object)?
            .get(method)
            .map(|entry| &entry.lowering)
    }

    /// Check if a method call's arguments are passed by reference.
    ///
    /// Builtins like `println!` take references, so their args are NOT move positions.
    /// Returns `false` for unknown methods (they are assumed to move).
    pub fn is_ref_args(&self, object: &str, method: &str) -> bool {
        self.methods
            .get(object)
            .and_then(|m| m.get(method))
            .is_some_and(|entry| entry.ref_args)
    }
}

/// Register default builtins.
fn register_defaults(registry: &mut BuiltinRegistry) {
    // console.log -> println!
    registry.register_method("console", "log", lower_console_log, true);

    // String methods
    registry.register_string_method("toUpperCase", lower_to_upper_case);
    registry.register_string_method("toLowerCase", lower_to_lower_case);
    registry.register_string_method("startsWith", lower_starts_with);
    registry.register_string_method("endsWith", lower_ends_with);
    registry.register_string_method("split", lower_split);
    registry.register_string_method("trim", lower_trim);
    registry.register_string_method("includes", lower_includes);
    registry.register_string_method("replace", lower_replace);

    // Phase 2: spawn builtin
    registry.register_function("spawn", lower_spawn);

    // Phase 5: Additional string methods
    registry.register_string_method("charAt", lower_char_at);
    registry.register_string_method("charCodeAt", lower_char_code_at);
    registry.register_string_method("indexOf", lower_string_index_of);
    registry.register_string_method("lastIndexOf", lower_string_last_index_of);
    registry.register_string_method("slice", lower_string_slice);
    registry.register_string_method("substring", lower_string_substring);
    registry.register_string_method("padStart", lower_pad_start);
    registry.register_string_method("padEnd", lower_pad_end);
    registry.register_string_method("repeat", lower_repeat);
    registry.register_string_method("concat", lower_string_concat);
    registry.register_string_method("at", lower_string_at);
    registry.register_string_method("trimStart", lower_trim_start);
    registry.register_string_method("trimEnd", lower_trim_end);
    registry.register_string_method("replaceAll", lower_replace_all);

    // Phase 2: collection methods (array iterator chains)
    registry.register_collection_method("map", lower_array_map);
    registry.register_collection_method("filter", lower_array_filter);
    registry.register_collection_method("reduce", lower_array_reduce);
    registry.register_collection_method("find", lower_array_find);
    registry.register_collection_method("forEach", lower_array_for_each);
    registry.register_collection_method("some", lower_array_some);
    registry.register_collection_method("every", lower_array_every);

    // Phase 5: Additional array iterator-based methods
    registry.register_collection_method("flat", lower_array_flat);
    registry.register_collection_method("flatMap", lower_array_flat_map);
    registry.register_collection_method("findIndex", lower_array_find_index);
    registry.register_collection_method("findLast", lower_array_find_last);
    registry.register_collection_method("findLastIndex", lower_array_find_last_index);

    // Phase 5: Array non-iterator methods (registered as string methods for dispatch)
    registry.register_string_method("push", lower_array_push);
    registry.register_string_method("pop", lower_array_pop);
    registry.register_string_method("shift", lower_array_shift);
    registry.register_string_method("unshift", lower_array_unshift);
    registry.register_string_method("reverse", lower_array_reverse);
    registry.register_string_method("sort", lower_array_sort);
    registry.register_string_method("join", lower_array_join);
    registry.register_string_method("fill", lower_array_fill);

    // Phase 5: Map/Set methods — registered in map_set_methods for type-aware dispatch.
    // These method names (get, set, has, delete, clear, keys, values, entries, add)
    // are common and would conflict with user-defined class methods if registered
    // as string methods.
    registry.register_map_set_method("get", lower_map_get);
    registry.register_map_set_method("set", lower_map_set);
    registry.register_map_set_method("has", lower_map_has);
    registry.register_map_set_method("delete", lower_map_delete);
    registry.register_map_set_method("clear", lower_clear);
    registry.register_map_set_method("keys", lower_keys);
    registry.register_map_set_method("values", lower_values);
    registry.register_map_set_method("entries", lower_entries);
    registry.register_map_set_method("add", lower_set_add);

    // Phase 5: Math object methods
    registry.register_method("Math", "floor", lower_math_floor, false);
    registry.register_method("Math", "ceil", lower_math_ceil, false);
    registry.register_method("Math", "round", lower_math_round, false);
    registry.register_method("Math", "abs", lower_math_abs, false);
    registry.register_method("Math", "sqrt", lower_math_sqrt, false);
    registry.register_method("Math", "min", lower_math_min, false);
    registry.register_method("Math", "max", lower_math_max, false);
    registry.register_method("Math", "random", lower_math_random, false);
    registry.register_method("Math", "pow", lower_math_pow, false);
    registry.register_method("Math", "log", lower_math_log, false);
    registry.register_method("Math", "sin", lower_math_sin, false);
    registry.register_method("Math", "cos", lower_math_cos, false);
    registry.register_method("Math", "tan", lower_math_tan, false);

    // Phase 5: console extensions
    registry.register_method("console", "error", lower_console_error, true);
    registry.register_method("console", "warn", lower_console_warn, true);
    registry.register_method("console", "debug", lower_console_debug, true);

    // Phase 5: Number functions
    registry.register_method("Number", "parseInt", lower_number_parse_int, false);
    registry.register_method("Number", "parseFloat", lower_number_parse_float, false);
    registry.register_method("Number", "isNaN", lower_number_is_nan, false);
    registry.register_method("Number", "isFinite", lower_number_is_finite, false);
    registry.register_method("Number", "isInteger", lower_number_is_integer, false);

    // Phase 5: Object utilities
    registry.register_method("Object", "keys", lower_object_keys, false);
    registry.register_method("Object", "values", lower_object_values, false);
    registry.register_method("Object", "entries", lower_object_entries, false);

    // Phase 5: JSON methods
    registry.register_method("JSON", "stringify", lower_json_stringify, false);
    registry.register_method("JSON", "parse", lower_json_parse, false);

    // Task 129: Date class — static methods
    registry.register_method("Date", "now", lower_date_now, false);

    // Task 129: Date class — instance methods (type-aware dispatch)
    registry.register_date_method("getTime", lower_date_get_time);
    registry.register_date_method("toISOString", lower_date_to_iso_string);
    registry.register_date_method("toString", lower_date_to_string);
}

/// Lower `console.log(args...)` to `println!("{} {} ...", arg1, arg2, ...)`.
///
/// Builds a format string with one `{}` per argument, space-separated.
/// Strips `.to_string()` wrappers from arguments since `println!` takes
/// all arguments by reference via `{}` formatting.
fn lower_console_log(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    let format_str = args.iter().map(|_| "{}").collect::<Vec<_>>().join(" ");
    let mut macro_args = vec![RustExpr::synthetic(RustExprKind::StringLit(format_str))];
    macro_args.extend(args.into_iter().map(strip_to_string));
    RustExpr::new(
        RustExprKind::Macro {
            name: "println".into(),
            args: macro_args,
        },
        arg_span,
    )
}

/// Strip a `ToString` wrapper from an expression, returning the inner expression.
///
/// `println!` takes arguments by reference, so `.to_string()` on string
/// literals is unnecessary noise.
fn strip_to_string(expr: RustExpr) -> RustExpr {
    if let RustExprKind::ToString(inner) = expr.kind {
        *inner
    } else {
        expr
    }
}

// ---------------------------------------------------------------------------
// Concurrency builtin lowering functions
// ---------------------------------------------------------------------------

/// Lower `spawn(async () => { body })` to `tokio::spawn(async move { body })`.
///
/// Extracts the closure body from the async closure argument and wraps it in
/// an `AsyncBlock` with `is_move: true` (spawned tasks must be `'static`).
/// The result is a `tokio::spawn(...)` call with the async block as argument.
fn lower_spawn(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    // Extract the async closure body from the first argument.
    // The argument should be a Closure { is_async: true, body: ... }.
    let async_block = if let Some(arg) = args.into_iter().next() {
        match arg.kind {
            RustExprKind::Closure { body, .. } => {
                let block = match body {
                    RustClosureBody::Block(b) => b,
                    RustClosureBody::Expr(e) => RustBlock {
                        stmts: vec![],
                        expr: Some(e),
                    },
                };
                RustExpr::synthetic(RustExprKind::AsyncBlock {
                    is_move: true,
                    body: block,
                })
            }
            // If the argument is not a closure, pass it through directly
            other => RustExpr::synthetic(other),
        }
    } else {
        // No arguments — produce an empty async block
        RustExpr::synthetic(RustExprKind::AsyncBlock {
            is_move: true,
            body: RustBlock {
                stmts: vec![],
                expr: None,
            },
        })
    };

    RustExpr::new(
        RustExprKind::Call {
            func: "tokio::spawn".into(),
            args: vec![async_block],
        },
        arg_span,
    )
}

// ---------------------------------------------------------------------------
// String method lowering functions
// ---------------------------------------------------------------------------

/// Lower `.toUpperCase()` to `.to_uppercase()`.
fn lower_to_upper_case(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "to_uppercase".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.toLowerCase()` to `.to_lowercase()`.
fn lower_to_lower_case(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "to_lowercase".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.startsWith(prefix)` to `.starts_with(prefix)`.
fn lower_starts_with(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "starts_with".into(),
            type_args: vec![],
            args: strip_to_string_args(args),
        },
        span,
    )
}

/// Lower `.endsWith(suffix)` to `.ends_with(suffix)`.
fn lower_ends_with(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "ends_with".into(),
            type_args: vec![],
            args: strip_to_string_args(args),
        },
        span,
    )
}

/// Lower `.includes(substr)` to `.contains(substr)`.
fn lower_includes(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "contains".into(),
            type_args: vec![],
            args: strip_to_string_args(args),
        },
        span,
    )
}

/// Lower `.trim()` to `.trim().to_string()`.
///
/// Rust's `trim()` returns `&str`, so we wrap in `.to_string()` to produce
/// an owned `String` matching `RustScript` semantics.
fn lower_trim(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let trim_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "trim".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(trim_call),
            method: "to_string".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.replace(from, to)` to `.replace(from, to)`.
///
/// Rust's `replace` takes `&str` patterns; string literal args are stripped
/// of `.to_string()` wrappers so they pass as `&str` via deref coercion.
fn lower_replace(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "replace".into(),
            type_args: vec![],
            args: strip_to_string_args(args),
        },
        span,
    )
}

/// Lower `.split(sep)` to `.split(sep).map(|s| s.to_string()).collect::<Vec<String>>()`.
///
/// Rust's `split` returns an iterator of `&str` slices. `RustScript`'s `split`
/// returns `Array<string>` (owned `Vec<String>`), so we chain `.map()` and
/// `.collect()` to convert.
fn lower_split(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    // Step 1: receiver.split(sep)
    let split_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "split".into(),
            type_args: vec![],
            args: strip_to_string_args(args),
        },
        span,
    );

    // Step 2: .map(|s| s.to_string())
    let closure_param = RustClosureParam {
        name: "s".into(),
        ty: None,
    };
    let closure_body = RustExpr::synthetic(RustExprKind::MethodCall {
        receiver: Box::new(RustExpr::synthetic(RustExprKind::Ident("s".into()))),
        method: "to_string".into(),
        type_args: vec![],
        args: vec![],
    });
    let map_closure = RustExpr::synthetic(RustExprKind::Closure {
        is_async: false,
        is_move: false,
        params: vec![closure_param],
        return_type: None,
        body: RustClosureBody::Expr(Box::new(closure_body)),
    });
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(split_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![map_closure],
        },
        span,
    );

    // Step 3: .collect::<Vec<String>>()
    let vec_string_type = RustType::Generic(
        Box::new(RustType::Named("Vec".into())),
        vec![RustType::String],
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "collect".into(),
            type_args: vec![vec_string_type],
            args: vec![],
        },
        span,
    )
}

/// Strip `.to_string()` wrappers from all arguments.
///
/// String method arguments that are string literals get lowered with a
/// `.to_string()` wrapper (since `RustScript` strings are owned). But Rust's
/// `str` methods take `&str` patterns, and Rust can auto-deref `&String`
/// to `&str`. Stripping the wrapper produces cleaner output (e.g.,
/// `starts_with("A")` instead of `starts_with("A".to_string())`).
/// For non-literal String args (variables), adds `&` to borrow as `&str`.
fn strip_to_string_args(args: Vec<RustExpr>) -> Vec<RustExpr> {
    args.into_iter()
        .map(|arg| {
            let stripped = strip_to_string(arg);
            // If the arg is still a String variable (not a literal), borrow it
            // so Rust's str methods get &str via auto-deref
            if matches!(stripped.kind, RustExprKind::Ident(_)) {
                RustExpr::synthetic(RustExprKind::Borrow(Box::new(stripped)))
            } else {
                stripped
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Phase 5: New string method lowering functions
// ---------------------------------------------------------------------------

/// Lower `.charAt(index)` to `.chars().nth(index as usize).map(|c| c.to_string()).unwrap_or_default()`.
fn lower_char_at(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let index = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    // receiver.chars().nth(index as usize).map(|c| c.to_string()).unwrap_or_default()
    let chars_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "chars".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let cast_expr = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(index),
        RustType::Named("usize".into()),
    ));
    let nth_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(chars_call),
            method: "nth".into(),
            type_args: vec![],
            args: vec![cast_expr],
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(nth_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "c".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(
                    RustExprKind::MethodCall {
                        receiver: Box::new(RustExpr::synthetic(RustExprKind::Ident("c".into()))),
                        method: "to_string".into(),
                        type_args: vec![],
                        args: vec![],
                    },
                ))),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "unwrap_or_default".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.charCodeAt(index)` to `.chars().nth(index as usize).map(|c| c as i64).unwrap_or(-1)`.
fn lower_char_code_at(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let index = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let chars_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "chars".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let cast_expr = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(index),
        RustType::Named("usize".into()),
    ));
    let nth_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(chars_call),
            method: "nth".into(),
            type_args: vec![],
            args: vec![cast_expr],
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(nth_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "c".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Cast(
                    Box::new(RustExpr::synthetic(RustExprKind::Ident("c".into()))),
                    RustType::I64,
                )))),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(-1))],
        },
        span,
    )
}

/// Lower `.indexOf(substr)` on strings to `.find(substr).map(|i| i as i64).unwrap_or(-1)`.
fn lower_string_index_of(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let substr = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let find_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "find".into(),
            type_args: vec![],
            args: strip_to_string_args(vec![substr]),
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(find_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "i".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Cast(
                    Box::new(RustExpr::synthetic(RustExprKind::Ident("i".into()))),
                    RustType::I64,
                )))),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(-1))],
        },
        span,
    )
}

/// Lower `.lastIndexOf(substr)` on strings to `.rfind(substr).map(|i| i as i64).unwrap_or(-1)`.
fn lower_string_last_index_of(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let substr = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let rfind_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "rfind".into(),
            type_args: vec![],
            args: strip_to_string_args(vec![substr]),
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(rfind_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "i".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Cast(
                    Box::new(RustExpr::synthetic(RustExprKind::Ident("i".into()))),
                    RustType::I64,
                )))),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(-1))],
        },
        span,
    )
}

/// Lower `.slice(start, end?)` on strings.
///
/// One arg:  `s.get(start as usize..).unwrap_or_default().to_string()`
/// Two args: `s.get(start as usize..end as usize).unwrap_or_default().to_string()`
fn lower_string_slice(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let start = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let end = iter.next();

    let start_cast = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(start),
        RustType::Named("usize".into()),
    ));

    let range_expr = if let Some(end_expr) = end {
        let end_cast = RustExpr::synthetic(RustExprKind::Cast(
            Box::new(end_expr),
            RustType::Named("usize".into()),
        ));
        RustExpr::synthetic(RustExprKind::Ident(format!(
            "{}..{}",
            emit_inline(&start_cast),
            emit_inline(&end_cast)
        )))
    } else {
        RustExpr::synthetic(RustExprKind::Ident(format!(
            "{}..",
            emit_inline(&start_cast)
        )))
    };

    let index_expr = RustExpr::new(
        RustExprKind::Index {
            object: Box::new(receiver),
            index: Box::new(range_expr),
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(index_expr),
            method: "to_string".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.substring(start, end?)` — identical to slice for positive indices.
fn lower_string_substring(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    lower_string_slice(receiver, args, span)
}

/// Lower `.padStart(length, fill?)` to format-based space padding.
///
/// Without custom fill: `format!("{:>width$}", s, width = length as usize)`
fn lower_pad_start(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let length = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let cast_len = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(length),
        RustType::Named("usize".into()),
    ));
    RustExpr::new(
        RustExprKind::Macro {
            name: "format".into(),
            args: vec![
                RustExpr::synthetic(RustExprKind::StringLit("{:>width$}".into())),
                receiver,
                RustExpr::synthetic(RustExprKind::Ident(format!(
                    "width = {}",
                    emit_inline(&cast_len)
                ))),
            ],
        },
        span,
    )
}

/// Lower `.padEnd(length, fill?)` to format-based space padding.
///
/// Without custom fill: `format!("{:<width$}", s, width = length as usize)`
fn lower_pad_end(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let length = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let cast_len = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(length),
        RustType::Named("usize".into()),
    ));
    RustExpr::new(
        RustExprKind::Macro {
            name: "format".into(),
            args: vec![
                RustExpr::synthetic(RustExprKind::StringLit("{:<width$}".into())),
                receiver,
                RustExpr::synthetic(RustExprKind::Ident(format!(
                    "width = {}",
                    emit_inline(&cast_len)
                ))),
            ],
        },
        span,
    )
}

/// Lower `.repeat(count)` to `.repeat(count as usize)`.
fn lower_repeat(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let count = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(1)));
    let cast_count = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(count),
        RustType::Named("usize".into()),
    ));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "repeat".into(),
            type_args: vec![],
            args: vec![cast_count],
        },
        span,
    )
}

/// Lower `.concat(other)` on strings to `format!("{}{}", s, other)`.
fn lower_string_concat(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let other = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    RustExpr::new(
        RustExprKind::Macro {
            name: "format".into(),
            args: vec![
                RustExpr::synthetic(RustExprKind::StringLit("{}{}".into())),
                receiver,
                strip_to_string(other),
            ],
        },
        span,
    )
}

/// Lower `.at(index)` on strings to `.chars().nth(index as usize).map(|c| c.to_string())`.
fn lower_string_at(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let index = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let chars_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "chars".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let cast_expr = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(index),
        RustType::Named("usize".into()),
    ));
    let nth_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(chars_call),
            method: "nth".into(),
            type_args: vec![],
            args: vec![cast_expr],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(nth_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "c".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(
                    RustExprKind::MethodCall {
                        receiver: Box::new(RustExpr::synthetic(RustExprKind::Ident("c".into()))),
                        method: "to_string".into(),
                        type_args: vec![],
                        args: vec![],
                    },
                ))),
            })],
        },
        span,
    )
}

/// Lower `.trimStart()` to `.trim_start().to_string()`.
fn lower_trim_start(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let trim_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "trim_start".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(trim_call),
            method: "to_string".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.trimEnd()` to `.trim_end().to_string()`.
fn lower_trim_end(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let trim_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "trim_end".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(trim_call),
            method: "to_string".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.replaceAll(from, to)` to `.replace(from, to)` — Rust's replace already replaces all.
fn lower_replace_all(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    lower_replace(receiver, args, span)
}

// ---------------------------------------------------------------------------
// Phase 5: Array mutating / non-iterator method lowering functions
// ---------------------------------------------------------------------------

/// Lower `.push(item)` to `.push(item)`.
fn lower_array_push(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "push".into(),
            type_args: vec![],
            args,
        },
        span,
    )
}

/// Lower `.pop()` to `.pop()`.
fn lower_array_pop(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "pop".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.shift()` to `.remove(0)` — remove first element.
fn lower_array_shift(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "remove".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(0))],
        },
        span,
    )
}

/// Lower `.unshift(item)` to `.insert(0, item)` — insert at beginning.
fn lower_array_unshift(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let item = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "insert".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(0)), item],
        },
        span,
    )
}

/// Lower `.reverse()` to `.reverse()` — in-place, mutating.
fn lower_array_reverse(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "reverse".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.sort()` to `.sort()` — in-place, mutating.
fn lower_array_sort(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "sort".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.join(sep)` to `.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(sep)`.
fn lower_array_join(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let sep = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(",".into())));
    // receiver.iter()
    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    // .map(|x| x.to_string())
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "x".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(
                    RustExprKind::MethodCall {
                        receiver: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                        method: "to_string".into(),
                        type_args: vec![],
                        args: vec![],
                    },
                ))),
            })],
        },
        span,
    );
    // .collect::<Vec<_>>()
    let collect_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    );
    // .join(sep)
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(collect_call),
            method: "join".into(),
            type_args: vec![],
            args: strip_to_string_args(vec![sep]),
        },
        span,
    )
}

/// Lower `.fill(value)` to `.iter_mut().for_each(|x| *x = value.clone())`.
fn lower_array_fill(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let value = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    // receiver.fill(value)
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "fill".into(),
            type_args: vec![],
            args: vec![value],
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Phase 5: Map method lowering functions
// ---------------------------------------------------------------------------

/// Lower `.get(key)` to `.get(&key).cloned()`.
fn lower_map_get(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let key = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let get_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "get".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Borrow(Box::new(
                strip_to_string(key),
            )))],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(get_call),
            method: "cloned".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.set(key, value)` to `.insert(key, value)` — mutating.
fn lower_map_set(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let key = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let value = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "insert".into(),
            type_args: vec![],
            args: vec![key, value],
        },
        span,
    )
}

/// Lower `.has(key)` to `.contains_key(&key)` — Map-specific.
///
/// This handles `HashMap.has()`. For `HashSet.has()`, the call site dispatches
/// to [`lower_set_has`] instead, which emits `.contains()`.
fn lower_map_has(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let key = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "contains_key".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Borrow(Box::new(
                strip_to_string(key),
            )))],
        },
        span,
    )
}

/// Lower `.delete(key)` to `.remove(&key)` — mutating.
fn lower_map_delete(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let key = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "remove".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Borrow(Box::new(
                strip_to_string(key),
            )))],
        },
        span,
    )
}

/// Lower `.clear()` to `.clear()` — mutating (works for Map, Set, and Vec).
fn lower_clear(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "clear".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.keys()` to `.keys().cloned().collect::<Vec<_>>()`.
fn lower_keys(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let keys_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "keys".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let cloned_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(keys_call),
            method: "cloned".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(cloned_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

/// Lower `.values()` to `.values().cloned().collect::<Vec<_>>()`.
fn lower_values(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let values_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "values".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let cloned_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(values_call),
            method: "cloned".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(cloned_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

/// Lower `.entries()` to `.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>()`.
fn lower_entries(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "(k, v)".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Ident(
                    "(k.clone(), v.clone())".into(),
                )))),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Phase 5: Set method lowering functions
// ---------------------------------------------------------------------------

/// Lower `.has(value)` to `.contains(&value)` — Set-specific.
///
/// Unlike Map's `.has()` which lowers to `.contains_key()`, Set uses `.contains()`.
/// This is dispatched from the call site when the receiver is known to be a `HashSet`.
pub(crate) fn lower_set_has(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let key = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "contains".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Borrow(Box::new(
                strip_to_string(key),
            )))],
        },
        span,
    )
}

/// Lower `.add(value)` to `.insert(value)` — Set-specific.
fn lower_set_add(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let value = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "insert".into(),
            type_args: vec![],
            args: vec![value],
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Phase 5: Math object lowering functions
// ---------------------------------------------------------------------------

/// Lower `Math.floor(x)` to `x.floor()`.
fn lower_math_floor(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "floor".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.ceil(x)` to `x.ceil()`.
fn lower_math_ceil(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "ceil".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.round(x)` to `x.round()`.
fn lower_math_round(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "round".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.abs(x)` to `x.abs()`.
fn lower_math_abs(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "abs".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.sqrt(x)` to `x.sqrt()`.
fn lower_math_sqrt(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "sqrt".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.min(a, b)` to `a.min(b)`.
fn lower_math_min(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let a = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    let b = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(a),
            method: "min".into(),
            type_args: vec![],
            args: vec![b],
        },
        span,
    )
}

/// Lower `Math.max(a, b)` to `a.max(b)`.
fn lower_math_max(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let a = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    let b = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(a),
            method: "max".into(),
            type_args: vec![],
            args: vec![b],
        },
        span,
    )
}

/// Lower `Math.random()` to `rand::random::<f64>()`.
fn lower_math_random(_args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::Call {
            func: "rand::random::<f64>".into(),
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.pow(base, exp)` to `base.powf(exp)`.
fn lower_math_pow(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let base = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    let exp = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(base),
            method: "powf".into(),
            type_args: vec![],
            args: vec![exp],
        },
        span,
    )
}

/// Lower `Math.log(x)` to `x.ln()`.
fn lower_math_log(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "ln".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.sin(x)` to `x.sin()`.
fn lower_math_sin(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "sin".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.cos(x)` to `x.cos()`.
fn lower_math_cos(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "cos".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.tan(x)` to `x.tan()`.
fn lower_math_tan(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "tan".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Helper: extract the first argument, defaulting to `0.0` (f64 literal).
fn first_arg_or_zero(args: Vec<RustExpr>) -> RustExpr {
    args.into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)))
}

// ---------------------------------------------------------------------------
// Phase 5: console extension lowering functions
// ---------------------------------------------------------------------------

/// Lower `console.error(args...)` to `eprintln!("{} {} ...", arg1, arg2, ...)`.
fn lower_console_error(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let format_str = args.iter().map(|_| "{}").collect::<Vec<_>>().join(" ");
    let mut macro_args = vec![RustExpr::synthetic(RustExprKind::StringLit(format_str))];
    macro_args.extend(args.into_iter().map(strip_to_string));
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.warn(args...)` to `eprintln!("warning: {} {} ...", arg1, arg2, ...)`.
fn lower_console_warn(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let format_placeholders = args.iter().map(|_| "{}").collect::<Vec<_>>().join(" ");
    let format_str = format!("warning: {format_placeholders}");
    let mut macro_args = vec![RustExpr::synthetic(RustExprKind::StringLit(format_str))];
    macro_args.extend(args.into_iter().map(strip_to_string));
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.debug(args...)` to `eprintln!("debug: {} {} ...", arg1, arg2, ...)`.
fn lower_console_debug(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let format_placeholders = args.iter().map(|_| "{}").collect::<Vec<_>>().join(" ");
    let format_str = format!("debug: {format_placeholders}");
    let mut macro_args = vec![RustExpr::synthetic(RustExprKind::StringLit(format_str))];
    macro_args.extend(args.into_iter().map(strip_to_string));
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Phase 5: Number function lowering
// ---------------------------------------------------------------------------

/// Lower `Number.parseInt(str)` to `str.parse::<i64>().unwrap_or(0)`.
fn lower_number_parse_int(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let parse_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "parse".into(),
            type_args: vec![RustType::I64],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(parse_call),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(0))],
        },
        span,
    )
}

/// Lower `Number.parseFloat(str)` to `str.parse::<f64>().unwrap_or(0.0)`.
fn lower_number_parse_float(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let parse_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "parse".into(),
            type_args: vec![RustType::F64],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(parse_call),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::FloatLit(0.0))],
        },
        span,
    )
}

/// Lower `Number.isNaN(x)` to `x.is_nan()`.
fn lower_number_is_nan(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "is_nan".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Number.isFinite(x)` to `x.is_finite()`.
fn lower_number_is_finite(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "is_finite".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Number.isInteger(x)` to `((x as i64 as f64) == x)`.
fn lower_number_is_integer(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    // Build: (x as i64 as f64) == x
    // We need the argument used in two places, so we emit the comparison
    // using Ident references. Since args are already lowered expressions,
    // we just use the expression directly (may evaluate twice — acceptable
    // for simple expressions; JS semantics do the same).
    let int_cast = RustExpr::synthetic(RustExprKind::Cast(Box::new(arg.clone()), RustType::I64));
    let float_cast = RustExpr::synthetic(RustExprKind::Cast(Box::new(int_cast), RustType::F64));
    RustExpr::new(
        RustExprKind::Binary {
            op: rsc_syntax::rust_ir::RustBinaryOp::Eq,
            left: Box::new(float_cast),
            right: Box::new(arg),
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Phase 5: Object utility lowering functions
// ---------------------------------------------------------------------------

/// Lower `Object.keys(map)` to `map.keys().cloned().collect::<Vec<_>>()`.
fn lower_object_keys(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    let keys_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "keys".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let cloned_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(keys_call),
            method: "cloned".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(cloned_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

/// Lower `Object.values(map)` to `map.values().cloned().collect::<Vec<_>>()`.
fn lower_object_values(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    let values_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "values".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let cloned_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(values_call),
            method: "cloned".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(cloned_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

/// Lower `Object.entries(map)` to `map.iter().map(|(k,v)| (k.clone(), v.clone())).collect::<Vec<_>>()`.
fn lower_object_entries(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "(k, v)".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Ident(
                    "(k.clone(), v.clone())".into(),
                )))),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Phase 5: JSON method lowering functions
// ---------------------------------------------------------------------------

/// Lower `JSON.stringify(obj)` to `serde_json::to_string(&obj).unwrap_or_default()`.
fn lower_json_stringify(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    let borrow_arg = RustExpr::synthetic(RustExprKind::Borrow(Box::new(arg)));
    let call = RustExpr::new(
        RustExprKind::Call {
            func: "serde_json::to_string".into(),
            args: vec![borrow_arg],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(call),
            method: "unwrap_or_default".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `JSON.parse(str)` to `serde_json::from_str(&str).unwrap_or_default()`.
fn lower_json_parse(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let borrow_arg = RustExpr::synthetic(RustExprKind::Borrow(Box::new(arg)));
    let call = RustExpr::new(
        RustExprKind::Call {
            func: "serde_json::from_str".into(),
            args: vec![borrow_arg],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(call),
            method: "unwrap_or_default".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Check whether an expression uses `Math.PI` or `Math.E` properties.
///
/// This is called from the `FieldAccess` lowering in `expr_lower.rs` to
/// intercept `Math.PI` and `Math.E` before they are lowered as normal
/// field access expressions.
pub(crate) fn lower_math_constant(object_name: &str, field_name: &str) -> Option<RustExpr> {
    if object_name != "Math" {
        return None;
    }
    match field_name {
        "PI" => Some(RustExpr::synthetic(RustExprKind::Ident(
            "std::f64::consts::PI".into(),
        ))),
        "E" => Some(RustExpr::synthetic(RustExprKind::Ident(
            "std::f64::consts::E".into(),
        ))),
        _ => None,
    }
}

/// Check whether the given object and method pair uses JSON methods
/// that require the `serde_json` crate.
pub(crate) fn needs_serde_json(object: &str, method: &str) -> bool {
    object == "JSON" && (method == "stringify" || method == "parse")
}

/// Check whether the given object and method pair uses `Math.random()`
/// which requires the `rand` crate.
pub(crate) fn needs_rand_crate(object: &str, method: &str) -> bool {
    object == "Math" && method == "random"
}

/// Inline emit a `RustExpr` to a Rust code string (mini-emitter for range expressions).
///
/// Used by `lower_string_slice` to build range expressions inside index syntax.
fn emit_inline(expr: &RustExpr) -> String {
    match &expr.kind {
        RustExprKind::IntLit(n) => n.to_string(),
        RustExprKind::Ident(name) => name.clone(),
        RustExprKind::Cast(inner, ty) => format!("{} as {ty}", emit_inline(inner)),
        _ => format!("{:?}", expr.kind),
    }
}

// ---------------------------------------------------------------------------
// Collection method lowering functions
// ---------------------------------------------------------------------------

/// Extract the closure parameter name and body expression from a `Closure` arg.
///
/// Collection method lowerings receive already-lowered `RustExpr` args.
/// The first arg is typically a closure. This helper extracts the parameter
/// name and body for building `IteratorOp` or `IteratorTerminal` nodes.
/// Takes ownership of the expression to avoid unnecessary cloning.
fn extract_closure_owned(arg: RustExpr) -> Option<(RustClosureParam, RustExpr)> {
    if let RustExprKind::Closure { params, body, .. } = arg.kind {
        let param = params.into_iter().next().unwrap_or(RustClosureParam {
            name: "_".into(),
            ty: None,
        });
        let body_expr = match body {
            RustClosureBody::Expr(e) => *e,
            RustClosureBody::Block(block) => {
                if let Some(expr) = block.expr {
                    *expr
                } else if let Some(rsc_syntax::rust_ir::RustStmt::Expr(e)) =
                    block.stmts.into_iter().last()
                {
                    e
                } else {
                    return None;
                }
            }
        };
        Some((param, body_expr))
    } else {
        None
    }
}

/// Extract the closure parameter and body from a reference (non-owning).
///
/// Used by `merge_into_chain` where we borrow from the args vector.
fn extract_closure_ref(arg: &RustExpr) -> Option<(RustClosureParam, RustExpr)> {
    if let RustExprKind::Closure { params, body, .. } = &arg.kind {
        let param = params.first().cloned().unwrap_or(RustClosureParam {
            name: "_".into(),
            ty: None,
        });
        let body_expr = match body {
            RustClosureBody::Expr(e) => (**e).clone(),
            RustClosureBody::Block(block) => {
                if let Some(expr) = &block.expr {
                    (**expr).clone()
                } else if let Some(rsc_syntax::rust_ir::RustStmt::Expr(e)) = block.stmts.last() {
                    e.clone()
                } else {
                    return None;
                }
            }
        };
        Some((param, body_expr))
    } else {
        None
    }
}

/// Lower `.map(fn)` to an `IteratorChain` with `CollectVec` terminal.
///
/// `arr.map(x => x * 2)` → `arr.iter().map(|x| x * 2).collect::<Vec<_>>()`
fn lower_array_map(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut arg_iter = args.into_iter();
    let first_arg = arg_iter.next();

    // Try to extract as closure; if not, treat as function reference
    let ops = if let Some(arg) = first_arg {
        if let Some((param, body)) = extract_closure_owned(arg.clone()) {
            vec![IteratorOp::Map(param, Box::new(body))]
        } else {
            // Function reference: `.map(fn_name)`
            vec![IteratorOp::MapFnRef(Box::new(arg))]
        }
    } else {
        vec![IteratorOp::Map(
            RustClosureParam {
                name: "_".into(),
                ty: None,
            },
            Box::new(RustExpr::synthetic(RustExprKind::Ident("_".into()))),
        )]
    };

    RustExpr::new(
        RustExprKind::IteratorChain {
            source: Box::new(receiver),
            ops,
            terminal: IteratorTerminal::CollectVec,
        },
        span,
    )
}

/// Lower `.filter(fn)` to an `IteratorChain` with `.cloned().collect()`.
///
/// `arr.filter(x => x > 0)` → `arr.iter().filter(|x| *x > 0).cloned().collect::<Vec<_>>()`
fn lower_array_filter(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut arg_iter = args.into_iter();
    let first_arg = arg_iter.next();

    // Try to extract as closure; if not, treat as function reference
    let mut ops = if let Some(arg) = first_arg {
        if let Some((param, body)) = extract_closure_owned(arg.clone()) {
            vec![IteratorOp::Filter(param, Box::new(body))]
        } else {
            // Function reference: `.filter(fn_name)`
            vec![IteratorOp::FilterFnRef(Box::new(arg))]
        }
    } else {
        vec![IteratorOp::Filter(
            RustClosureParam {
                name: "_".into(),
                ty: None,
            },
            Box::new(RustExpr::synthetic(RustExprKind::BoolLit(true))),
        )]
    };
    ops.push(IteratorOp::Cloned);

    RustExpr::new(
        RustExprKind::IteratorChain {
            source: Box::new(receiver),
            ops,
            terminal: IteratorTerminal::CollectVec,
        },
        span,
    )
}

/// Lower `.reduce(fn, init)` to an `IteratorChain` with `Fold` terminal.
///
/// `arr.reduce((acc, x) => acc + x, 0)` → `arr.iter().fold(0, |acc, x| acc + x)`
/// Note: argument order is swapped — JS `reduce(fn, init)` → Rust `fold(init, fn)`.
fn lower_array_reduce(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let closure_arg = iter.next();
    let init_arg = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));

    let (acc_param, item_param, body) = if let Some(closure) = closure_arg
        && let RustExprKind::Closure { params, body, .. } = closure.kind
    {
        let acc = params
            .first()
            .map_or_else(|| "acc".into(), |p| p.name.clone());
        let item = params
            .get(1)
            .map_or_else(|| "item".into(), |p| p.name.clone());
        (acc, item, body)
    } else {
        (
            "acc".into(),
            "item".into(),
            RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Ident(
                "_".into(),
            )))),
        )
    };

    RustExpr::new(
        RustExprKind::IteratorChain {
            source: Box::new(receiver),
            ops: vec![],
            terminal: IteratorTerminal::Fold {
                init: Box::new(init_arg),
                acc_param,
                item_param,
                body,
            },
        },
        span,
    )
}

/// Lower `.find(fn)` to an `IteratorChain` with `Find` terminal.
///
/// `arr.find(x => x > 3)` → `arr.iter().find(|x| *x > 3).cloned()`
fn lower_array_find(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let (param, body) = args
        .into_iter()
        .next()
        .and_then(extract_closure_owned)
        .unwrap_or_else(|| {
            (
                RustClosureParam {
                    name: "_".into(),
                    ty: None,
                },
                RustExpr::synthetic(RustExprKind::BoolLit(true)),
            )
        });
    RustExpr::new(
        RustExprKind::IteratorChain {
            source: Box::new(receiver),
            ops: vec![],
            terminal: IteratorTerminal::Find(param, Box::new(body)),
        },
        span,
    )
}

/// Lower `.forEach(fn)` to an `IteratorChain` with `ForEach` terminal.
///
/// `arr.forEach(x => console.log(x))` → `arr.iter().for_each(|x| println!("{}", x))`
fn lower_array_for_each(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let (param, body) = args
        .into_iter()
        .next()
        .and_then(extract_closure_owned)
        .unwrap_or_else(|| {
            (
                RustClosureParam {
                    name: "_".into(),
                    ty: None,
                },
                RustExpr::synthetic(RustExprKind::Ident("()".into())),
            )
        });
    RustExpr::new(
        RustExprKind::IteratorChain {
            source: Box::new(receiver),
            ops: vec![],
            terminal: IteratorTerminal::ForEach(param, Box::new(body)),
        },
        span,
    )
}

/// Lower `.some(fn)` to an `IteratorChain` with `Any` terminal.
///
/// `arr.some(x => x > 5)` → `arr.iter().any(|x| *x > 5)`
fn lower_array_some(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let (param, body) = args
        .into_iter()
        .next()
        .and_then(extract_closure_owned)
        .unwrap_or_else(|| {
            (
                RustClosureParam {
                    name: "_".into(),
                    ty: None,
                },
                RustExpr::synthetic(RustExprKind::BoolLit(false)),
            )
        });
    RustExpr::new(
        RustExprKind::IteratorChain {
            source: Box::new(receiver),
            ops: vec![],
            terminal: IteratorTerminal::Any(param, Box::new(body)),
        },
        span,
    )
}

/// Lower `.every(fn)` to an `IteratorChain` with `All` terminal.
///
/// `arr.every(x => x > 0)` → `arr.iter().all(|x| *x > 0)`
fn lower_array_every(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let (param, body) = args
        .into_iter()
        .next()
        .and_then(extract_closure_owned)
        .unwrap_or_else(|| {
            (
                RustClosureParam {
                    name: "_".into(),
                    ty: None,
                },
                RustExpr::synthetic(RustExprKind::BoolLit(true)),
            )
        });
    RustExpr::new(
        RustExprKind::IteratorChain {
            source: Box::new(receiver),
            ops: vec![],
            terminal: IteratorTerminal::All(param, Box::new(body)),
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Phase 5: Additional array iterator-based collection methods
// ---------------------------------------------------------------------------

/// Lower `.flat()` to an `IteratorChain` with flatten semantics.
///
/// `arr.flat()` → `arr.into_iter().flatten().collect::<Vec<_>>()`
fn lower_array_flat(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let into_iter = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "into_iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let flatten = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(into_iter),
            method: "flatten".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(flatten),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

/// Lower `.flatMap(fn)` to an `IteratorChain` with `flat_map` semantics.
///
/// `arr.flatMap(x => f(x))` → `arr.iter().flat_map(|x| f(x)).collect::<Vec<_>>()`
fn lower_array_flat_map(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let (param, body) = args
        .into_iter()
        .next()
        .and_then(extract_closure_owned)
        .unwrap_or_else(|| {
            (
                RustClosureParam {
                    name: "x".into(),
                    ty: None,
                },
                RustExpr::synthetic(RustExprKind::Ident("x".into())),
            )
        });
    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let flat_map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "flat_map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![param],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(body)),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(flat_map_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("Vec".into())),
                vec![RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

/// Lower `.findIndex(fn)` to an `IteratorChain` with `Position` terminal semantics.
///
/// `arr.findIndex(x => x > 3)` → `arr.iter().position(|x| *x > 3).map(|i| i as i64).unwrap_or(-1)`
fn lower_array_find_index(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let (param, body) = args
        .into_iter()
        .next()
        .and_then(extract_closure_owned)
        .unwrap_or_else(|| {
            (
                RustClosureParam {
                    name: "_".into(),
                    ty: None,
                },
                RustExpr::synthetic(RustExprKind::BoolLit(true)),
            )
        });
    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let position_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "position".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![param],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(body)),
            })],
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(position_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "i".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Cast(
                    Box::new(RustExpr::synthetic(RustExprKind::Ident("i".into()))),
                    RustType::I64,
                )))),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(-1))],
        },
        span,
    )
}

/// Lower `.findLast(fn)` to `arr.iter().rev().find(|x| fn(x)).cloned()`.
fn lower_array_find_last(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let (param, body) = args
        .into_iter()
        .next()
        .and_then(extract_closure_owned)
        .unwrap_or_else(|| {
            (
                RustClosureParam {
                    name: "_".into(),
                    ty: None,
                },
                RustExpr::synthetic(RustExprKind::BoolLit(true)),
            )
        });
    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let rev_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "rev".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let find_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(rev_call),
            method: "find".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![param],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(body)),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(find_call),
            method: "cloned".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.findLastIndex(fn)` to `arr.iter().rposition(|x| fn(x)).map(|i| i as i64).unwrap_or(-1)`.
fn lower_array_find_last_index(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let (param, body) = args
        .into_iter()
        .next()
        .and_then(extract_closure_owned)
        .unwrap_or_else(|| {
            (
                RustClosureParam {
                    name: "_".into(),
                    ty: None,
                },
                RustExpr::synthetic(RustExprKind::BoolLit(true)),
            )
        });
    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let rposition_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "rposition".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![param],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(body)),
            })],
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(rposition_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "i".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Cast(
                    Box::new(RustExpr::synthetic(RustExprKind::Ident("i".into()))),
                    RustType::I64,
                )))),
            })],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(-1))],
        },
        span,
    )
}

/// Merge an outer collection method call onto an existing `IteratorChain`.
///
/// When we see `arr.map(fn1).filter(fn2)`, the inner `map` produces an
/// `IteratorChain`. The outer `filter` should append to that chain rather
/// than creating a new nested chain. This function handles that composition.
#[allow(clippy::too_many_lines)]
// Match arms for all 7 collection method variants; splitting would obscure the chain composition logic
pub(crate) fn merge_into_chain(
    inner_chain: RustExpr,
    outer_method: &str,
    outer_args: &[RustExpr],
    span: Span,
) -> Option<RustExpr> {
    if let RustExprKind::IteratorChain {
        source,
        mut ops,
        terminal,
    } = inner_chain.kind
    {
        // The inner terminal must be CollectVec for chaining to work.
        // (You can't chain .filter() after .find() — find is terminal.)
        if !matches!(terminal, IteratorTerminal::CollectVec) {
            return None;
        }

        // Build the outer operation as the new terminal.
        match outer_method {
            "map" => {
                if let Some((param, body)) = outer_args.first().and_then(extract_closure_ref) {
                    ops.push(IteratorOp::Map(param, Box::new(body)));
                } else if let Some(fn_ref) = outer_args.first() {
                    ops.push(IteratorOp::MapFnRef(Box::new(fn_ref.clone())));
                } else {
                    return None;
                }
                Some(RustExpr::new(
                    RustExprKind::IteratorChain {
                        source,
                        ops,
                        terminal: IteratorTerminal::CollectVec,
                    },
                    span,
                ))
            }
            "filter" => {
                // Remove trailing Cloned from previous filter if present,
                // since we're continuing the chain and will add Cloned at the end.
                if matches!(ops.last(), Some(IteratorOp::Cloned)) {
                    ops.pop();
                }
                if let Some((param, body)) = outer_args.first().and_then(extract_closure_ref) {
                    ops.push(IteratorOp::Filter(param, Box::new(body)));
                } else if let Some(fn_ref) = outer_args.first() {
                    ops.push(IteratorOp::FilterFnRef(Box::new(fn_ref.clone())));
                } else {
                    return None;
                }
                ops.push(IteratorOp::Cloned);
                Some(RustExpr::new(
                    RustExprKind::IteratorChain {
                        source,
                        ops,
                        terminal: IteratorTerminal::CollectVec,
                    },
                    span,
                ))
            }
            "reduce" => merge_reduce_into_chain(source, ops, outer_args, span),
            "find" => {
                let (param, body) = outer_args.first().and_then(extract_closure_ref)?;
                Some(RustExpr::new(
                    RustExprKind::IteratorChain {
                        source,
                        ops,
                        terminal: IteratorTerminal::Find(param, Box::new(body)),
                    },
                    span,
                ))
            }
            "forEach" => {
                let (param, body) = outer_args.first().and_then(extract_closure_ref)?;
                Some(RustExpr::new(
                    RustExprKind::IteratorChain {
                        source,
                        ops,
                        terminal: IteratorTerminal::ForEach(param, Box::new(body)),
                    },
                    span,
                ))
            }
            "some" => {
                let (param, body) = outer_args.first().and_then(extract_closure_ref)?;
                Some(RustExpr::new(
                    RustExprKind::IteratorChain {
                        source,
                        ops,
                        terminal: IteratorTerminal::Any(param, Box::new(body)),
                    },
                    span,
                ))
            }
            "every" => {
                let (param, body) = outer_args.first().and_then(extract_closure_ref)?;
                Some(RustExpr::new(
                    RustExprKind::IteratorChain {
                        source,
                        ops,
                        terminal: IteratorTerminal::All(param, Box::new(body)),
                    },
                    span,
                ))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// Helper for merging a reduce call into an existing iterator chain.
fn merge_reduce_into_chain(
    source: Box<RustExpr>,
    ops: Vec<IteratorOp>,
    outer_args: &[RustExpr],
    span: Span,
) -> Option<RustExpr> {
    let closure_arg = outer_args.first();
    let init_arg = outer_args
        .get(1)
        .cloned()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    if let Some(closure) = closure_arg
        && let RustExprKind::Closure { params, body, .. } = &closure.kind
    {
        let acc = params
            .first()
            .map_or_else(|| "acc".into(), |p| p.name.clone());
        let item = params
            .get(1)
            .map_or_else(|| "item".into(), |p| p.name.clone());
        Some(RustExpr::new(
            RustExprKind::IteratorChain {
                source,
                ops,
                terminal: IteratorTerminal::Fold {
                    init: Box::new(init_arg),
                    acc_param: acc,
                    item_param: item,
                    body: body.clone(),
                },
            },
            span,
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Date class lowering functions (Task 129)
// ---------------------------------------------------------------------------

/// Lower `Date.now()` to epoch milliseconds as `i64`.
///
/// Emits: `std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64`
fn lower_date_now(_args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::Raw(
            "std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64".to_owned(),
        ),
        span,
    )
}

/// Lower `.getTime()` on a `Date` instance to epoch milliseconds as `i64`.
///
/// Emits: `receiver.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64`
fn lower_date_get_time(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(RustExpr::new(
                RustExprKind::MethodCall {
                    receiver: Box::new(RustExpr::new(
                        RustExprKind::MethodCall {
                            receiver: Box::new(receiver),
                            method: "duration_since".to_owned(),
                            type_args: vec![],
                            args: vec![RustExpr::new(
                                RustExprKind::Ident("std::time::UNIX_EPOCH".to_owned()),
                                span,
                            )],
                        },
                        span,
                    )),
                    method: "unwrap".to_owned(),
                    type_args: vec![],
                    args: vec![],
                },
                span,
            )),
            method: "as_millis".to_owned(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.toISOString()` on a `Date` instance to an ISO 8601 formatted string.
///
/// Uses a helper block that computes the ISO string from the unix timestamp.
/// Emits a block expression that formats the date as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
#[allow(clippy::needless_pass_by_value)] // Must match StringMethodLowering fn pointer type
fn lower_date_to_iso_string(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    // Build a raw expression that computes ISO string from SystemTime
    let receiver_code = emit_receiver(&receiver);
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ \
            let __d = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
            let __secs = __d.as_secs(); \
            let __millis = __d.subsec_millis(); \
            let __days = __secs / 86400; \
            let __time_of_day = __secs % 86400; \
            let __hours = __time_of_day / 3600; \
            let __minutes = (__time_of_day % 3600) / 60; \
            let __seconds = __time_of_day % 60; \
            let mut __y = 1970i64; \
            let mut __remaining = __days as i64; \
            loop {{ \
                let __days_in_year = if __y % 4 == 0 && (__y % 100 != 0 || __y % 400 == 0) {{ 366 }} else {{ 365 }}; \
                if __remaining < __days_in_year {{ break; }} \
                __remaining -= __days_in_year; \
                __y += 1; \
            }} \
            let __leap = __y % 4 == 0 && (__y % 100 != 0 || __y % 400 == 0); \
            let __mdays: [i64; 12] = [31, if __leap {{ 29 }} else {{ 28 }}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]; \
            let mut __m = 0usize; \
            while __m < 12 && __remaining >= __mdays[__m] {{ \
                __remaining -= __mdays[__m]; \
                __m += 1; \
            }} \
            format!(\"{{:04}}-{{:02}}-{{:02}}T{{:02}}:{{:02}}:{{:02}}.{{:03}}Z\", __y, __m + 1, __remaining + 1, __hours, __minutes, __seconds, __millis) \
            }}"
        )),
        span,
    )
}

/// Lower `.toString()` on a `Date` instance to a debug format string.
///
/// Emits: `format!("{:?}", receiver)`
fn lower_date_to_string(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::Macro {
            name: "format".to_owned(),
            args: vec![
                RustExpr::synthetic(RustExprKind::StringLit("{:?}".to_owned())),
                receiver,
            ],
        },
        span,
    )
}

/// Emit a receiver expression as a Rust code string for use in raw blocks.
fn emit_receiver(expr: &RustExpr) -> String {
    match &expr.kind {
        RustExprKind::Ident(name) => name.clone(),
        RustExprKind::FieldAccess { object, field } => {
            format!("{}.{}", emit_receiver(object), field)
        }
        _ => format!("{:?}", expr.kind),
    }
}

/// Check whether a type represents a `Date` (`SystemTime`) value.
///
/// Used for type-aware dispatch of Date instance methods like `.getTime()`.
pub(crate) fn is_date_type(ty: &RustType) -> bool {
    matches!(ty, RustType::Named(n) if n == "Date" || n == "SystemTime")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 12: lookup_method("console", "log") returns Some
    #[test]
    fn test_builtin_registry_lookup_console_log_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "log").is_some());
    }

    // Test 13: lookup_method("foo", "bar") returns None
    #[test]
    fn test_builtin_registry_lookup_unknown_returns_none() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("foo", "bar").is_none());
    }

    // Test 14: is_ref_args("console", "log") returns true
    #[test]
    fn test_builtin_registry_is_ref_args_console_log() {
        let registry = BuiltinRegistry::new();
        assert!(registry.is_ref_args("console", "log"));
    }

    #[test]
    fn test_builtin_registry_is_ref_args_unknown_returns_false() {
        let registry = BuiltinRegistry::new();
        assert!(!registry.is_ref_args("foo", "bar"));
    }

    #[test]
    fn test_lower_console_log_single_arg() {
        let args = vec![RustExpr::new(
            RustExprKind::StringLit("hello".to_owned()),
            Span::new(0, 7),
        )];
        let result = lower_console_log(args, Span::new(0, 20));
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "println");
                assert_eq!(args.len(), 2); // format string + 1 arg
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{}"),
                    other => panic!("expected StringLit for format, got {other:?}"),
                }
            }
            other => panic!("expected Macro, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_log_multiple_args() {
        let args = vec![
            RustExpr::new(RustExprKind::Ident("x".to_owned()), Span::new(0, 1)),
            RustExpr::new(RustExprKind::Ident("y".to_owned()), Span::new(3, 4)),
        ];
        let result = lower_console_log(args, Span::new(0, 10));
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "println");
                assert_eq!(args.len(), 3); // format string + 2 args
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{} {}"),
                    other => panic!("expected StringLit for format, got {other:?}"),
                }
            }
            other => panic!("expected Macro, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 032: String method registry and lowering tests
    // ---------------------------------------------------------------

    fn span() -> Span {
        Span::new(0, 10)
    }

    fn string_receiver() -> RustExpr {
        RustExpr::new(RustExprKind::Ident("s".to_owned()), span())
    }

    fn string_arg(val: &str) -> RustExpr {
        RustExpr::new(RustExprKind::StringLit(val.to_owned()), span())
    }

    // Test 11: Registry lookup for known string method returns Some
    #[test]
    fn test_builtin_registry_lookup_string_method_to_upper_case_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_string_method("toUpperCase").is_some());
    }

    // Test 12: Registry lookup for unknown string method returns None
    #[test]
    fn test_builtin_registry_lookup_string_method_unknown_returns_none() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_string_method("foo").is_none());
    }

    // Test 1: toUpperCase
    #[test]
    fn test_lower_to_upper_case_produces_to_uppercase() {
        let result = lower_to_upper_case(string_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "to_uppercase");
                assert!(args.is_empty());
            }
            other => panic!("expected MethodCall, got {other:?}"),
        }
    }

    // Test 2: toLowerCase
    #[test]
    fn test_lower_to_lower_case_produces_to_lowercase() {
        let result = lower_to_lower_case(string_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "to_lowercase");
                assert!(args.is_empty());
            }
            other => panic!("expected MethodCall, got {other:?}"),
        }
    }

    // Test 3: startsWith
    #[test]
    fn test_lower_starts_with_produces_starts_with() {
        let result = lower_starts_with(string_receiver(), vec![string_arg("A")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "starts_with");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall, got {other:?}"),
        }
    }

    // Test 4: endsWith
    #[test]
    fn test_lower_ends_with_produces_ends_with() {
        let result = lower_ends_with(string_receiver(), vec![string_arg("z")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "ends_with");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall, got {other:?}"),
        }
    }

    // Test 7: includes
    #[test]
    fn test_lower_includes_produces_contains() {
        let result = lower_includes(string_receiver(), vec![string_arg("lic")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "contains");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall, got {other:?}"),
        }
    }

    // Test 6: trim produces chained trim().to_string()
    #[test]
    fn test_lower_trim_produces_trim_to_string_chain() {
        let result = lower_trim(string_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "to_string");
                match &receiver.kind {
                    RustExprKind::MethodCall { method, .. } => {
                        assert_eq!(method, "trim");
                    }
                    other => panic!("expected inner MethodCall(trim), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(to_string), got {other:?}"),
        }
    }

    // Test 8: replace
    #[test]
    fn test_lower_replace_produces_replace() {
        let result = lower_replace(
            string_receiver(),
            vec![string_arg("old"), string_arg("new")],
            span(),
        );
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "replace");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected MethodCall, got {other:?}"),
        }
    }

    // Test 5: split produces chained split().map().collect::<Vec<String>>()
    #[test]
    fn test_lower_split_produces_split_map_collect_chain() {
        let result = lower_split(string_receiver(), vec![string_arg(",")], span());
        // Outermost should be .collect::<Vec<String>>()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                type_args,
                ..
            } => {
                assert_eq!(method, "collect");
                assert_eq!(type_args.len(), 1);
                assert_eq!(
                    type_args[0],
                    RustType::Generic(
                        Box::new(RustType::Named("Vec".into())),
                        vec![RustType::String],
                    )
                );
                // Inner should be .map(|s| s.to_string())
                match &receiver.kind {
                    RustExprKind::MethodCall {
                        receiver: inner_recv,
                        method,
                        ..
                    } => {
                        assert_eq!(method, "map");
                        // Innermost should be .split(sep)
                        match &inner_recv.kind {
                            RustExprKind::MethodCall { method, .. } => {
                                assert_eq!(method, "split");
                            }
                            other => panic!("expected MethodCall(split), got {other:?}"),
                        }
                    }
                    other => panic!("expected MethodCall(map), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    // Test: all registered string methods are present
    #[test]
    fn test_builtin_registry_all_string_methods_registered() {
        let registry = BuiltinRegistry::new();
        let methods = [
            "toUpperCase",
            "toLowerCase",
            "startsWith",
            "endsWith",
            "split",
            "trim",
            "includes",
            "replace",
            // Phase 5 additions
            "charAt",
            "charCodeAt",
            "indexOf",
            "lastIndexOf",
            "slice",
            "substring",
            "padStart",
            "padEnd",
            "repeat",
            "concat",
            "at",
            "trimStart",
            "trimEnd",
            "replaceAll",
            // Array mutating methods (registered as string methods)
            "push",
            "pop",
            "shift",
            "unshift",
            "reverse",
            "sort",
            "join",
            "fill",
        ];
        for method in methods {
            assert!(
                registry.lookup_string_method(method).is_some(),
                "expected string method '{method}' to be registered"
            );
        }
    }

    // Test: all Map/Set methods are registered in the map_set registry
    #[test]
    fn test_builtin_registry_all_map_set_methods_registered() {
        let registry = BuiltinRegistry::new();
        let methods = [
            "get", "set", "has", "delete", "clear", "keys", "values", "entries", "add",
        ];
        for method in methods {
            assert!(
                registry.lookup_map_set_method(method).is_some(),
                "expected map/set method '{method}' to be registered"
            );
        }
    }

    // ---------------------------------------------------------------
    // Task 033: Collection method registry and lowering tests
    // ---------------------------------------------------------------

    fn make_closure(param_name: &str, body: RustExpr) -> RustExpr {
        RustExpr::synthetic(RustExprKind::Closure {
            is_async: false,
            is_move: false,
            params: vec![RustClosureParam {
                name: param_name.into(),
                ty: None,
            }],
            return_type: None,
            body: RustClosureBody::Expr(Box::new(body)),
        })
    }

    fn make_two_param_closure(p1: &str, p2: &str, body: RustExpr) -> RustExpr {
        RustExpr::synthetic(RustExprKind::Closure {
            is_async: false,
            is_move: false,
            params: vec![
                RustClosureParam {
                    name: p1.into(),
                    ty: None,
                },
                RustClosureParam {
                    name: p2.into(),
                    ty: None,
                },
            ],
            return_type: None,
            body: RustClosureBody::Expr(Box::new(body)),
        })
    }

    fn ident_expr(name: &str) -> RustExpr {
        RustExpr::new(RustExprKind::Ident(name.to_owned()), span())
    }

    // Test 10: Registry lookup for collection method "map" returns Some
    #[test]
    fn test_builtin_registry_lookup_collection_method_map_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_collection_method("map").is_some());
    }

    // Test 11: Non-collection method falls through
    #[test]
    fn test_builtin_registry_lookup_collection_method_unknown_returns_none() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_collection_method("customMethod").is_none());
    }

    // Test: all collection methods are registered
    #[test]
    fn test_builtin_registry_all_collection_methods_registered() {
        let registry = BuiltinRegistry::new();
        let methods = [
            "map",
            "filter",
            "reduce",
            "find",
            "forEach",
            "some",
            "every",
            // Phase 5 additions
            "flat",
            "flatMap",
            "findIndex",
            "findLast",
            "findLastIndex",
        ];
        for method in methods {
            assert!(
                registry.lookup_collection_method(method).is_some(),
                "expected collection method '{method}' to be registered"
            );
        }
    }

    // Test 1: map produces IteratorChain with Map op and CollectVec terminal
    #[test]
    fn test_lower_array_map_produces_iterator_chain() {
        let receiver = ident_expr("arr");
        let closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Mul,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(2))),
            }),
        );
        let result = lower_array_map(receiver, vec![closure], span());
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert_eq!(ops.len(), 1);
                assert!(matches!(&ops[0], IteratorOp::Map(p, _) if p.name == "x"));
                assert!(matches!(terminal, IteratorTerminal::CollectVec));
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test 2: filter produces IteratorChain with Filter + Cloned ops
    #[test]
    fn test_lower_array_filter_produces_iterator_chain_with_cloned() {
        let receiver = ident_expr("items");
        let closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(0))),
            }),
        );
        let result = lower_array_filter(receiver, vec![closure], span());
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert_eq!(ops.len(), 2);
                assert!(matches!(&ops[0], IteratorOp::Filter(p, _) if p.name == "x"));
                assert!(matches!(&ops[1], IteratorOp::Cloned));
                assert!(matches!(terminal, IteratorTerminal::CollectVec));
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test 3: reduce produces IteratorChain with Fold terminal, args reordered
    #[test]
    fn test_lower_array_reduce_produces_fold_with_reordered_args() {
        let receiver = ident_expr("arr");
        let closure = make_two_param_closure(
            "acc",
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Add,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("acc".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
            }),
        );
        let init = RustExpr::synthetic(RustExprKind::IntLit(0));
        let result = lower_array_reduce(receiver, vec![closure, init], span());
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert!(ops.is_empty());
                match terminal {
                    IteratorTerminal::Fold {
                        init,
                        acc_param,
                        item_param,
                        ..
                    } => {
                        assert_eq!(acc_param, "acc");
                        assert_eq!(item_param, "x");
                        assert!(matches!(init.kind, RustExprKind::IntLit(0)));
                    }
                    other => panic!("expected Fold terminal, got {other:?}"),
                }
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test 4: find produces IteratorChain with Find terminal
    #[test]
    fn test_lower_array_find_produces_find_terminal() {
        let receiver = ident_expr("items");
        let closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(3))),
            }),
        );
        let result = lower_array_find(receiver, vec![closure], span());
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert!(ops.is_empty());
                assert!(matches!(terminal, IteratorTerminal::Find(p, _) if p.name == "x"));
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test 5: forEach produces IteratorChain with ForEach terminal
    #[test]
    fn test_lower_array_for_each_produces_for_each_terminal() {
        let receiver = ident_expr("items");
        let closure = make_closure("x", RustExpr::synthetic(RustExprKind::Ident("x".into())));
        let result = lower_array_for_each(receiver, vec![closure], span());
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert!(ops.is_empty());
                assert!(matches!(terminal, IteratorTerminal::ForEach(p, _) if p.name == "x"));
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test 6: some produces IteratorChain with Any terminal
    #[test]
    fn test_lower_array_some_produces_any_terminal() {
        let receiver = ident_expr("items");
        let closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(5))),
            }),
        );
        let result = lower_array_some(receiver, vec![closure], span());
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert!(ops.is_empty());
                assert!(matches!(terminal, IteratorTerminal::Any(p, _) if p.name == "x"));
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test 7: every produces IteratorChain with All terminal
    #[test]
    fn test_lower_array_every_produces_all_terminal() {
        let receiver = ident_expr("items");
        let closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(0))),
            }),
        );
        let result = lower_array_every(receiver, vec![closure], span());
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert!(ops.is_empty());
                assert!(matches!(terminal, IteratorTerminal::All(p, _) if p.name == "x"));
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test 8: chain map+filter merges into single IteratorChain
    #[test]
    fn test_merge_map_then_filter_into_single_chain() {
        let receiver = ident_expr("arr");
        let map_closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Mul,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(2))),
            }),
        );
        // Create the inner map chain
        let inner = lower_array_map(receiver, vec![map_closure], span());
        // Now merge a filter onto it
        let filter_closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(5))),
            }),
        );
        let result = merge_into_chain(inner, "filter", &[filter_closure], span())
            .expect("merge should succeed");
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert_eq!(ops.len(), 3); // Map, Filter, Cloned
                assert!(matches!(&ops[0], IteratorOp::Map(p, _) if p.name == "x"));
                assert!(matches!(&ops[1], IteratorOp::Filter(p, _) if p.name == "x"));
                assert!(matches!(&ops[2], IteratorOp::Cloned));
                assert!(matches!(terminal, IteratorTerminal::CollectVec));
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test 9: chain map+filter+reduce merges into single chain
    #[test]
    fn test_merge_map_filter_reduce_into_single_chain() {
        let receiver = ident_expr("arr");
        let map_closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Mul,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(2))),
            }),
        );
        let inner = lower_array_map(receiver, vec![map_closure], span());

        let filter_closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(0))),
            }),
        );
        let mid = merge_into_chain(inner, "filter", &[filter_closure], span())
            .expect("merge filter should succeed");

        let reduce_closure = make_two_param_closure(
            "acc",
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Add,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("acc".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
            }),
        );
        let init = RustExpr::synthetic(RustExprKind::IntLit(0));
        let result = merge_into_chain(mid, "reduce", &[reduce_closure, init], span())
            .expect("merge reduce should succeed");
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                // Map, Filter, Cloned from the filter merge
                assert!(ops.len() >= 2);
                assert!(matches!(terminal, IteratorTerminal::Fold { .. }));
            }
            other => panic!("expected IteratorChain, got {other:?}"),
        }
    }

    // Test: lookup_function("spawn") returns Some
    #[test]
    fn test_builtin_registry_lookup_function_spawn_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("spawn").is_some(),
            "spawn should be registered as a builtin free function"
        );
    }

    // Test: lookup_function for unknown returns None
    #[test]
    fn test_builtin_registry_lookup_function_unknown_returns_none() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("unknown_func").is_none(),
            "unknown function should not be found"
        );
    }

    // Test: lower_spawn produces tokio::spawn(async move { body })
    #[test]
    fn test_lower_spawn_produces_tokio_spawn_async_move_block() {
        let work_call = RustExpr::synthetic(RustExprKind::Call {
            func: "work".into(),
            args: vec![],
        });
        let closure = RustExpr::synthetic(RustExprKind::Closure {
            is_async: true,
            is_move: false,
            params: vec![],
            return_type: None,
            body: RustClosureBody::Block(RustBlock {
                stmts: vec![rsc_syntax::rust_ir::RustStmt::Semi(work_call)],
                expr: None,
            }),
        });

        let result = lower_spawn(vec![closure], span());
        match &result.kind {
            RustExprKind::Call { func, args } => {
                assert_eq!(func, "tokio::spawn");
                assert_eq!(args.len(), 1);
                match &args[0].kind {
                    RustExprKind::AsyncBlock { is_move, body } => {
                        assert!(is_move, "spawn should add move to async block");
                        assert_eq!(body.stmts.len(), 1);
                    }
                    other => panic!("expected AsyncBlock, got {other:?}"),
                }
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 060: Phase 5 string method tests
    // ---------------------------------------------------------------

    fn int_arg(val: i64) -> RustExpr {
        RustExpr::new(RustExprKind::IntLit(val), span())
    }

    #[test]
    fn test_lower_char_at_produces_chars_nth_chain() {
        let result = lower_char_at(string_receiver(), vec![int_arg(0)], span());
        // Outermost: .unwrap_or_default()
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "unwrap_or_default");
            }
            other => panic!("expected MethodCall(unwrap_or_default), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_char_code_at_produces_chars_nth_chain() {
        let result = lower_char_code_at(string_receiver(), vec![int_arg(0)], span());
        // Outermost: .unwrap_or(-1)
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "unwrap_or");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0].kind, RustExprKind::IntLit(-1)));
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_index_of_produces_find_chain() {
        let result = lower_string_index_of(string_receiver(), vec![string_arg("x")], span());
        // Outermost: .unwrap_or(-1)
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "unwrap_or");
                assert!(matches!(args[0].kind, RustExprKind::IntLit(-1)));
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_last_index_of_produces_rfind_chain() {
        let result = lower_string_last_index_of(string_receiver(), vec![string_arg("x")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "unwrap_or");
                assert!(matches!(args[0].kind, RustExprKind::IntLit(-1)));
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_slice_one_arg_produces_index() {
        let result = lower_string_slice(string_receiver(), vec![int_arg(2)], span());
        // Outermost: .to_string()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "to_string");
                assert!(matches!(receiver.kind, RustExprKind::Index { .. }));
            }
            other => panic!("expected MethodCall(to_string), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_slice_two_args_produces_range_index() {
        let result = lower_string_slice(string_receiver(), vec![int_arg(2), int_arg(5)], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "to_string");
                assert!(matches!(receiver.kind, RustExprKind::Index { .. }));
            }
            other => panic!("expected MethodCall(to_string), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_substring_delegates_to_slice() {
        let result = lower_string_substring(string_receiver(), vec![int_arg(1)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "to_string");
            }
            other => panic!("expected MethodCall(to_string), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_pad_start_produces_format_macro() {
        let result = lower_pad_start(string_receiver(), vec![int_arg(10)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "{:>width$}"));
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_pad_end_produces_format_macro() {
        let result = lower_pad_end(string_receiver(), vec![int_arg(10)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "{:<width$}"));
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_repeat_produces_repeat_with_cast() {
        let result = lower_repeat(string_receiver(), vec![int_arg(3)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "repeat");
                assert_eq!(args.len(), 1);
                assert!(matches!(
                    args[0].kind,
                    RustExprKind::Cast(_, RustType::Named(_))
                ));
            }
            other => panic!("expected MethodCall(repeat), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_concat_produces_format_macro() {
        let result = lower_string_concat(string_receiver(), vec![string_arg("world")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "{}{}"));
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_at_produces_chars_nth_map() {
        let result = lower_string_at(string_receiver(), vec![int_arg(0)], span());
        // Outermost: .map(|c| c.to_string())
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "map");
            }
            other => panic!("expected MethodCall(map), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_trim_start_produces_trim_start_to_string() {
        let result = lower_trim_start(string_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "to_string");
                match &receiver.kind {
                    RustExprKind::MethodCall { method, .. } => {
                        assert_eq!(method, "trim_start");
                    }
                    other => panic!("expected inner MethodCall(trim_start), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(to_string), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_trim_end_produces_trim_end_to_string() {
        let result = lower_trim_end(string_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "to_string");
                match &receiver.kind {
                    RustExprKind::MethodCall { method, .. } => {
                        assert_eq!(method, "trim_end");
                    }
                    other => panic!("expected inner MethodCall(trim_end), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(to_string), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_replace_all_delegates_to_replace() {
        let result = lower_replace_all(
            string_receiver(),
            vec![string_arg("old"), string_arg("new")],
            span(),
        );
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "replace");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected MethodCall(replace), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 060: Array mutating method tests
    // ---------------------------------------------------------------

    fn array_receiver() -> RustExpr {
        RustExpr::new(RustExprKind::Ident("arr".to_owned()), span())
    }

    #[test]
    fn test_lower_array_push_produces_push() {
        let result = lower_array_push(array_receiver(), vec![int_arg(42)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "push");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(push), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_pop_produces_pop() {
        let result = lower_array_pop(array_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "pop");
                assert!(args.is_empty());
            }
            other => panic!("expected MethodCall(pop), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_shift_produces_remove_0() {
        let result = lower_array_shift(array_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "remove");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0].kind, RustExprKind::IntLit(0)));
            }
            other => panic!("expected MethodCall(remove), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_unshift_produces_insert_0() {
        let result = lower_array_unshift(array_receiver(), vec![int_arg(99)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "insert");
                assert_eq!(args.len(), 2);
                assert!(matches!(args[0].kind, RustExprKind::IntLit(0)));
            }
            other => panic!("expected MethodCall(insert), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_reverse_produces_reverse() {
        let result = lower_array_reverse(array_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "reverse");
                assert!(args.is_empty());
            }
            other => panic!("expected MethodCall(reverse), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_sort_produces_sort() {
        let result = lower_array_sort(array_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "sort");
                assert!(args.is_empty());
            }
            other => panic!("expected MethodCall(sort), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_join_produces_iter_map_collect_join() {
        let result = lower_array_join(array_receiver(), vec![string_arg(",")], span());
        // Outermost: .join(sep)
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "join");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(join), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_fill_produces_fill() {
        let result = lower_array_fill(array_receiver(), vec![int_arg(0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "fill");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(fill), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 060: Array iterator-based method tests
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_array_flat_produces_into_iter_flatten_collect() {
        let result = lower_array_flat(array_receiver(), vec![], span());
        // Outermost: .collect::<Vec<_>>()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                type_args,
                ..
            } => {
                assert_eq!(method, "collect");
                assert_eq!(type_args.len(), 1);
                // Inner: .flatten()
                match &receiver.kind {
                    RustExprKind::MethodCall {
                        receiver: inner,
                        method,
                        ..
                    } => {
                        assert_eq!(method, "flatten");
                        // Innermost: .into_iter()
                        match &inner.kind {
                            RustExprKind::MethodCall { method, .. } => {
                                assert_eq!(method, "into_iter");
                            }
                            other => panic!("expected MethodCall(into_iter), got {other:?}"),
                        }
                    }
                    other => panic!("expected MethodCall(flatten), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_flat_map_produces_iter_flat_map_collect() {
        let closure = make_closure("x", RustExpr::synthetic(RustExprKind::Ident("x".into())));
        let result = lower_array_flat_map(array_receiver(), vec![closure], span());
        // Outermost: .collect::<Vec<_>>()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "collect");
                match &receiver.kind {
                    RustExprKind::MethodCall { method, .. } => {
                        assert_eq!(method, "flat_map");
                    }
                    other => panic!("expected MethodCall(flat_map), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_find_index_produces_position_chain() {
        let closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(3))),
            }),
        );
        let result = lower_array_find_index(array_receiver(), vec![closure], span());
        // Outermost: .unwrap_or(-1)
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "unwrap_or");
                assert!(matches!(args[0].kind, RustExprKind::IntLit(-1)));
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_find_last_produces_rev_find_cloned() {
        let closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(3))),
            }),
        );
        let result = lower_array_find_last(array_receiver(), vec![closure], span());
        // Outermost: .cloned()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "cloned");
                // Inner: .find(|x| ...)
                match &receiver.kind {
                    RustExprKind::MethodCall {
                        receiver: inner,
                        method,
                        ..
                    } => {
                        assert_eq!(method, "find");
                        // Inner: .rev()
                        match &inner.kind {
                            RustExprKind::MethodCall { method, .. } => {
                                assert_eq!(method, "rev");
                            }
                            other => panic!("expected MethodCall(rev), got {other:?}"),
                        }
                    }
                    other => panic!("expected MethodCall(find), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(cloned), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_find_last_index_produces_rposition_chain() {
        let closure = make_closure(
            "x",
            RustExpr::synthetic(RustExprKind::Binary {
                op: RustBinaryOp::Gt,
                left: Box::new(RustExpr::synthetic(RustExprKind::Ident("x".into()))),
                right: Box::new(RustExpr::synthetic(RustExprKind::IntLit(3))),
            }),
        );
        let result = lower_array_find_last_index(array_receiver(), vec![closure], span());
        // Outermost: .unwrap_or(-1)
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "unwrap_or");
                assert!(matches!(args[0].kind, RustExprKind::IntLit(-1)));
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 060: Map method tests
    // ---------------------------------------------------------------

    fn map_receiver() -> RustExpr {
        RustExpr::new(RustExprKind::Ident("m".to_owned()), span())
    }

    #[test]
    fn test_lower_map_get_produces_get_cloned() {
        let result = lower_map_get(map_receiver(), vec![string_arg("key")], span());
        // Outermost: .cloned()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "cloned");
                match &receiver.kind {
                    RustExprKind::MethodCall { method, args, .. } => {
                        assert_eq!(method, "get");
                        assert_eq!(args.len(), 1);
                        assert!(matches!(args[0].kind, RustExprKind::Borrow(_)));
                    }
                    other => panic!("expected MethodCall(get), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(cloned), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_map_set_produces_insert() {
        let result = lower_map_set(map_receiver(), vec![string_arg("key"), int_arg(42)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "insert");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected MethodCall(insert), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_map_has_produces_contains_key() {
        let result = lower_map_has(map_receiver(), vec![string_arg("key")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "contains_key");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0].kind, RustExprKind::Borrow(_)));
            }
            other => panic!("expected MethodCall(contains_key), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_set_has_produces_contains() {
        let result = lower_set_has(set_receiver(), vec![string_arg("hello")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "contains");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0].kind, RustExprKind::Borrow(_)));
            }
            other => panic!("expected MethodCall(contains), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_map_delete_produces_remove() {
        let result = lower_map_delete(map_receiver(), vec![string_arg("key")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "remove");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0].kind, RustExprKind::Borrow(_)));
            }
            other => panic!("expected MethodCall(remove), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_clear_produces_clear() {
        let result = lower_clear(map_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "clear");
                assert!(args.is_empty());
            }
            other => panic!("expected MethodCall(clear), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_keys_produces_keys_cloned_collect() {
        let result = lower_keys(map_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "collect");
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_values_produces_values_cloned_collect() {
        let result = lower_values(map_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "collect");
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_entries_produces_iter_map_collect() {
        let result = lower_entries(map_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "collect");
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 060: Set method tests
    // ---------------------------------------------------------------

    fn set_receiver() -> RustExpr {
        RustExpr::new(RustExprKind::Ident("s".to_owned()), span())
    }

    #[test]
    fn test_lower_set_add_produces_insert() {
        let result = lower_set_add(set_receiver(), vec![int_arg(42)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "insert");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(insert), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task: Standard library builtins — Math methods
    // ---------------------------------------------------------------

    fn float_arg(val: f64) -> RustExpr {
        RustExpr::new(RustExprKind::FloatLit(val), span())
    }

    #[test]
    fn test_builtin_registry_lookup_math_floor_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Math", "floor").is_some());
    }

    #[test]
    fn test_lower_math_floor_produces_method_call() {
        let result = lower_math_floor(vec![float_arg(3.7)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "floor");
                assert!(args.is_empty());
            }
            other => panic!("expected MethodCall(floor), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_ceil_produces_method_call() {
        let result = lower_math_ceil(vec![float_arg(3.2)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "ceil"),
            other => panic!("expected MethodCall(ceil), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_round_produces_method_call() {
        let result = lower_math_round(vec![float_arg(3.5)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "round"),
            other => panic!("expected MethodCall(round), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_abs_produces_method_call() {
        let result = lower_math_abs(vec![float_arg(-5.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "abs"),
            other => panic!("expected MethodCall(abs), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_sqrt_produces_method_call() {
        let result = lower_math_sqrt(vec![float_arg(16.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "sqrt"),
            other => panic!("expected MethodCall(sqrt), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_min_produces_method_call() {
        let result = lower_math_min(vec![float_arg(1.0), float_arg(2.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "min");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(min), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_max_produces_method_call() {
        let result = lower_math_max(vec![float_arg(1.0), float_arg(2.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "max");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(max), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_random_produces_rand_call() {
        let result = lower_math_random(vec![], span());
        match &result.kind {
            RustExprKind::Call { func, args } => {
                assert_eq!(func, "rand::random::<f64>");
                assert!(args.is_empty());
            }
            other => panic!("expected Call(rand::random), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_pow_produces_powf() {
        let result = lower_math_pow(vec![float_arg(2.0), float_arg(3.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "powf");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(powf), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_log_produces_ln() {
        let result = lower_math_log(vec![float_arg(10.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "ln"),
            other => panic!("expected MethodCall(ln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_sin_produces_sin() {
        let result = lower_math_sin(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "sin"),
            other => panic!("expected MethodCall(sin), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_cos_produces_cos() {
        let result = lower_math_cos(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "cos"),
            other => panic!("expected MethodCall(cos), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_tan_produces_tan() {
        let result = lower_math_tan(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "tan"),
            other => panic!("expected MethodCall(tan), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task: Standard library builtins — console extensions
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_console_error_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "error").is_some());
    }

    #[test]
    fn test_lower_console_error_produces_eprintln() {
        let result = lower_console_error(vec![string_arg("oops")], span());
        match &result.kind {
            RustExprKind::Macro { name, .. } => assert_eq!(name, "eprintln"),
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_warn_produces_eprintln_with_prefix() {
        let result = lower_console_warn(vec![string_arg("caution")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(
                    matches!(&args[0].kind, RustExprKind::StringLit(s) if s.starts_with("warning:"))
                );
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_debug_produces_eprintln_with_prefix() {
        let result = lower_console_debug(vec![string_arg("info")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(
                    matches!(&args[0].kind, RustExprKind::StringLit(s) if s.starts_with("debug:"))
                );
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task: Standard library builtins — Number functions
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_number_parse_int_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Number", "parseInt").is_some());
    }

    #[test]
    fn test_lower_number_parse_int_produces_parse_chain() {
        let result = lower_number_parse_int(vec![string_arg("42")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "unwrap_or"),
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_parse_float_produces_parse_chain() {
        let result = lower_number_parse_float(vec![string_arg("3.14")], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "unwrap_or"),
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_is_nan_produces_method_call() {
        let result = lower_number_is_nan(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "is_nan"),
            other => panic!("expected MethodCall(is_nan), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_is_finite_produces_method_call() {
        let result = lower_number_is_finite(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "is_finite"),
            other => panic!("expected MethodCall(is_finite), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_is_integer_produces_eq_comparison() {
        let result = lower_number_is_integer(vec![float_arg(5.0)], span());
        match &result.kind {
            RustExprKind::Binary {
                op: RustBinaryOp::Eq,
                ..
            } => {} // correct
            other => panic!("expected Binary(Eq), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task: Standard library builtins — Object utilities
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_object_keys_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "keys").is_some());
    }

    #[test]
    fn test_lower_object_keys_produces_collect_chain() {
        let result = lower_object_keys(vec![map_receiver()], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "collect"),
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_object_values_produces_collect_chain() {
        let result = lower_object_values(vec![map_receiver()], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "collect"),
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_object_entries_produces_collect_chain() {
        let result = lower_object_entries(vec![map_receiver()], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "collect"),
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task: Standard library builtins — JSON methods
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_json_stringify_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("JSON", "stringify").is_some());
    }

    #[test]
    fn test_lower_json_stringify_produces_serde_json_call() {
        let result = lower_json_stringify(vec![map_receiver()], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "unwrap_or_default");
                assert!(
                    matches!(&receiver.kind, RustExprKind::Call { func, .. } if func == "serde_json::to_string")
                );
            }
            other => panic!("expected MethodCall(unwrap_or_default), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_json_parse_produces_serde_json_call() {
        let result = lower_json_parse(vec![string_arg("{}")], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "unwrap_or_default");
                assert!(
                    matches!(&receiver.kind, RustExprKind::Call { func, .. } if func == "serde_json::from_str")
                );
            }
            other => panic!("expected MethodCall(unwrap_or_default), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task: Standard library builtins — Math constants
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_math_constant_pi() {
        let result = lower_math_constant("Math", "PI");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(matches!(&expr.kind, RustExprKind::Ident(name) if name == "std::f64::consts::PI"));
    }

    #[test]
    fn test_lower_math_constant_e() {
        let result = lower_math_constant("Math", "E");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(matches!(&expr.kind, RustExprKind::Ident(name) if name == "std::f64::consts::E"));
    }

    #[test]
    fn test_lower_math_constant_unknown_returns_none() {
        assert!(lower_math_constant("Math", "LN2").is_none());
    }

    #[test]
    fn test_lower_math_constant_non_math_returns_none() {
        assert!(lower_math_constant("console", "PI").is_none());
    }

    // ---------------------------------------------------------------
    // Task: Standard library builtins — needs_serde_json / needs_rand
    // ---------------------------------------------------------------

    #[test]
    fn test_needs_serde_json_true_for_json_stringify() {
        assert!(needs_serde_json("JSON", "stringify"));
    }

    #[test]
    fn test_needs_serde_json_true_for_json_parse() {
        assert!(needs_serde_json("JSON", "parse"));
    }

    #[test]
    fn test_needs_serde_json_false_for_console_log() {
        assert!(!needs_serde_json("console", "log"));
    }

    #[test]
    fn test_needs_rand_crate_true_for_math_random() {
        assert!(needs_rand_crate("Math", "random"));
    }

    #[test]
    fn test_needs_rand_crate_false_for_math_floor() {
        assert!(!needs_rand_crate("Math", "floor"));
    }

    // ---------------------------------------------------------------
    // Task 129: Date class lowering
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_date_now() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Date", "now").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_time() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getTime").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_to_iso_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toISOString").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_to_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toString").is_some());
    }

    #[test]
    fn test_lower_date_now() {
        let result = lower_date_now(vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("SystemTime::now()"));
                assert!(code.contains("UNIX_EPOCH"));
                assert!(code.contains("as_millis"));
                assert!(code.contains("as i64"));
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_time() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_time(receiver, vec![], span());
        // The outermost call should be .as_millis()
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "as_millis");
            }
            other => panic!("expected MethodCall(as_millis), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_to_string() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_string(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                assert_eq!(args.len(), 2);
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{:?}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_to_iso_string() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_iso_string(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("UNIX_EPOCH"));
                assert!(code.contains("format!"));
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_is_date_type_named_date() {
        assert!(is_date_type(&RustType::Named("Date".to_owned())));
    }

    #[test]
    fn test_is_date_type_named_system_time() {
        assert!(is_date_type(&RustType::Named("SystemTime".to_owned())));
    }

    #[test]
    fn test_is_date_type_named_other() {
        assert!(!is_date_type(&RustType::Named("String".to_owned())));
    }

    #[test]
    fn test_is_date_type_non_named() {
        assert!(!is_date_type(&RustType::I64));
    }
}
