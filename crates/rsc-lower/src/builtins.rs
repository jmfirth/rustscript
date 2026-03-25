//! Builtin function and method registry for the lowering pass.
//!
//! Data-driven mapping of known method calls (e.g., `console.log`) to their
//! Rust equivalents (e.g., `println!`). New builtins are added by writing
//! a lowering function and registering it — no changes to the transform module.
//!
//! String methods (e.g., `.toUpperCase()`, `.split()`) are registered
//! separately from object-based builtins because their receiver is any
//! expression of type `String`, not a named object like `"console"`.

use std::collections::HashMap;

use rsc_syntax::rust_ir::{RustClosureBody, RustClosureParam, RustExpr, RustExprKind, RustType};
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

/// Registry of builtin functions and methods.
///
/// Maps `(object_name, method_name)` pairs to their lowering functions.
/// Uses a nested `HashMap` to avoid allocating on every lookup.
/// Phase 0 registers `console.log` -> `println!`.
/// Phase 2 adds string method mappings (e.g., `.toUpperCase()` -> `.to_uppercase()`).
pub(crate) struct BuiltinRegistry {
    methods: HashMap<String, HashMap<String, BuiltinEntry>>,
    string_methods: HashMap<String, StringMethodLowering>,
}

impl BuiltinRegistry {
    /// Create a new registry with the default builtins registered.
    pub fn new() -> Self {
        let mut registry = Self {
            methods: HashMap::new(),
            string_methods: HashMap::new(),
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
    // Phase 0: console.log
    registry.register_method("console", "log", lower_console_log, true);

    // Phase 2: string methods
    registry.register_string_method("toUpperCase", lower_to_upper_case);
    registry.register_string_method("toLowerCase", lower_to_lower_case);
    registry.register_string_method("startsWith", lower_starts_with);
    registry.register_string_method("endsWith", lower_ends_with);
    registry.register_string_method("split", lower_split);
    registry.register_string_method("trim", lower_trim);
    registry.register_string_method("includes", lower_includes);
    registry.register_string_method("replace", lower_replace);
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
fn strip_to_string_args(args: Vec<RustExpr>) -> Vec<RustExpr> {
    args.into_iter().map(strip_to_string).collect()
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
        ];
        for method in methods {
            assert!(
                registry.lookup_string_method(method).is_some(),
                "expected string method '{method}' to be registered"
            );
        }
    }
}
