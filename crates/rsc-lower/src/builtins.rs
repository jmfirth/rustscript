//! Builtin function and method registry for the lowering pass.
//!
//! Data-driven mapping of known method calls (e.g., `console.log`) to their
//! Rust equivalents (e.g., `println!`). New builtins are added by writing
//! a lowering function and registering it — no changes to the transform module.

use std::collections::HashMap;

use rsc_syntax::rust_ir::{RustExpr, RustExprKind};
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

/// Registry of builtin functions and methods.
///
/// Maps `(object_name, method_name)` pairs to their lowering functions.
/// Phase 0 registers `console.log` -> `println!`.
pub(crate) struct BuiltinRegistry {
    methods: HashMap<(String, String), BuiltinEntry>,
}

impl BuiltinRegistry {
    /// Create a new registry with the default builtins registered.
    pub fn new() -> Self {
        let mut registry = Self {
            methods: HashMap::new(),
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
        self.methods.insert(
            (object.to_owned(), method.to_owned()),
            BuiltinEntry { lowering, ref_args },
        );
    }

    /// Check if a method call is a builtin and return its lowering function.
    pub fn lookup_method(&self, object: &str, method: &str) -> Option<&BuiltinLowering> {
        self.methods
            .get(&(object.to_owned(), method.to_owned()))
            .map(|entry| &entry.lowering)
    }

    /// Check if a method call's arguments are passed by reference.
    ///
    /// Builtins like `println!` take references, so their args are NOT move positions.
    /// Returns `false` for unknown methods (they are assumed to move).
    pub fn is_ref_args(&self, object: &str, method: &str) -> bool {
        self.methods
            .get(&(object.to_owned(), method.to_owned()))
            .is_some_and(|entry| entry.ref_args)
    }
}

/// Register Phase 0 default builtins.
fn register_defaults(registry: &mut BuiltinRegistry) {
    registry.register_method("console", "log", lower_console_log, true);
}

/// Lower `console.log(args...)` to `println!("{} {} ...", arg1, arg2, ...)`.
///
/// Builds a format string with one `{}` per argument, space-separated.
fn lower_console_log(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    let format_str = args.iter().map(|_| "{}").collect::<Vec<_>>().join(" ");
    let mut macro_args = vec![RustExpr::synthetic(RustExprKind::StringLit(format_str))];
    macro_args.extend(args);
    RustExpr::new(
        RustExprKind::Macro {
            name: "println".into(),
            args: macro_args,
        },
        arg_span,
    )
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
}
