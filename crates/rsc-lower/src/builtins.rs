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
    IteratorOp, IteratorTerminal, RustClosureBody, RustClosureParam, RustExpr, RustExprKind,
    RustType,
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
/// Phase 0 registers `console.log` -> `println!`.
/// Phase 2 adds string method mappings (e.g., `.toUpperCase()` -> `.to_uppercase()`)
/// and collection method mappings (e.g., `.map()` -> `.iter().map().collect()`).
pub(crate) struct BuiltinRegistry {
    methods: HashMap<String, HashMap<String, BuiltinEntry>>,
    string_methods: HashMap<String, StringMethodLowering>,
    collection_methods: HashMap<String, CollectionMethodLowering>,
}

impl BuiltinRegistry {
    /// Create a new registry with the default builtins registered.
    pub fn new() -> Self {
        let mut registry = Self {
            methods: HashMap::new(),
            string_methods: HashMap::new(),
            collection_methods: HashMap::new(),
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

    // Phase 2: collection methods (array iterator chains)
    registry.register_collection_method("map", lower_array_map);
    registry.register_collection_method("filter", lower_array_filter);
    registry.register_collection_method("reduce", lower_array_reduce);
    registry.register_collection_method("find", lower_array_find);
    registry.register_collection_method("forEach", lower_array_for_each);
    registry.register_collection_method("some", lower_array_some);
    registry.register_collection_method("every", lower_array_every);
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
                RustExpr::synthetic(RustExprKind::Ident("_".into())),
            )
        });
    RustExpr::new(
        RustExprKind::IteratorChain {
            source: Box::new(receiver),
            ops: vec![IteratorOp::Map(param, Box::new(body))],
            terminal: IteratorTerminal::CollectVec,
        },
        span,
    )
}

/// Lower `.filter(fn)` to an `IteratorChain` with `.cloned().collect()`.
///
/// `arr.filter(x => x > 0)` → `arr.iter().filter(|x| *x > 0).cloned().collect::<Vec<_>>()`
fn lower_array_filter(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
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
            ops: vec![
                IteratorOp::Filter(param, Box::new(body)),
                IteratorOp::Cloned,
            ],
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
        let body_expr = match body {
            RustClosureBody::Expr(e) => *e,
            RustClosureBody::Block(block) => {
                if let Some(expr) = block.expr {
                    *expr
                } else {
                    RustExpr::synthetic(RustExprKind::Ident("_".into()))
                }
            }
        };
        (acc, item, body_expr)
    } else {
        (
            "acc".into(),
            "item".into(),
            RustExpr::synthetic(RustExprKind::Ident("_".into())),
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
                body: Box::new(body),
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
                let (param, body) = outer_args.first().and_then(extract_closure_ref)?;
                ops.push(IteratorOp::Map(param, Box::new(body)));
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
                let (param, body) = outer_args.first().and_then(extract_closure_ref)?;
                ops.push(IteratorOp::Filter(param, Box::new(body)));
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
        let body_expr = match body {
            RustClosureBody::Expr(e) => (**e).clone(),
            RustClosureBody::Block(block) => {
                if let Some(expr) = &block.expr {
                    (**expr).clone()
                } else {
                    return None;
                }
            }
        };
        Some(RustExpr::new(
            RustExprKind::IteratorChain {
                source,
                ops,
                terminal: IteratorTerminal::Fold {
                    init: Box::new(init_arg),
                    acc_param: acc,
                    item_param: item,
                    body: Box::new(body_expr),
                },
            },
            span,
        ))
    } else {
        None
    }
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

    // Test: all 7 collection methods are registered
    #[test]
    fn test_builtin_registry_all_collection_methods_registered() {
        let registry = BuiltinRegistry::new();
        let methods = [
            "map", "filter", "reduce", "find", "forEach", "some", "every",
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
}
