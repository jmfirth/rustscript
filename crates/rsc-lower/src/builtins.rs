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
    RustExprKind, RustLetStmt, RustLoopStmt, RustStmt, RustType, RustUnaryOp,
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
    /// Array-specific methods that require type-aware dispatch.
    /// These are only consulted when the receiver is known to be a Vec.
    /// Methods like `indexOf`, `at`, `concat`, `slice` have different semantics
    /// on arrays vs strings, so they need type-aware dispatch.
    array_methods: HashMap<String, StringMethodLowering>,
    /// Date-specific instance methods that require type-aware dispatch.
    /// These are only consulted when the receiver is known to be a `Date` (`SystemTime`).
    date_methods: HashMap<String, StringMethodLowering>,
    /// Regex-specific methods that require type-aware dispatch.
    /// These are only consulted when the receiver is known to be a `Regex`.
    regex_methods: HashMap<String, StringMethodLowering>,
    /// Number-specific instance methods that require type-aware dispatch.
    /// These are only consulted when the receiver is known to be a numeric type
    /// (i32, i64, f64, etc.). Methods like `toFixed`, `toPrecision`, `toString`
    /// would conflict with user-defined methods if registered as string methods.
    number_methods: HashMap<String, StringMethodLowering>,
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
            array_methods: HashMap::new(),
            date_methods: HashMap::new(),
            regex_methods: HashMap::new(),
            number_methods: HashMap::new(),
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

    /// Register an array-specific method that requires type-aware dispatch.
    fn register_array_method(&mut self, name: &str, lowering: StringMethodLowering) {
        self.array_methods.insert(name.to_owned(), lowering);
    }

    /// Look up an array-specific method by name.
    ///
    /// Returns the lowering function if the method is a known array method.
    /// Only consulted when the receiver is known to be a `Vec`.
    pub fn lookup_array_method(&self, method: &str) -> Option<&StringMethodLowering> {
        self.array_methods.get(method)
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

    /// Register a Regex-specific method that requires type-aware dispatch.
    fn register_regex_method(&mut self, name: &str, lowering: StringMethodLowering) {
        self.regex_methods.insert(name.to_owned(), lowering);
    }

    /// Look up a Regex method by name.
    ///
    /// Returns the lowering function if the method is a known Regex method.
    /// Only consulted when the receiver is known to be a `Regex`.
    pub fn lookup_regex_method(&self, method: &str) -> Option<&StringMethodLowering> {
        self.regex_methods.get(method)
    }

    /// Register a Number-specific instance method that requires type-aware dispatch.
    fn register_number_method(&mut self, name: &str, lowering: StringMethodLowering) {
        self.number_methods.insert(name.to_owned(), lowering);
    }

    /// Look up a Number instance method by name.
    ///
    /// Returns the lowering function if the method is a known Number instance method.
    /// Only consulted when the receiver is known to be a numeric type (i32, i64, f64, etc.).
    pub fn lookup_number_method(&self, method: &str) -> Option<&StringMethodLowering> {
        self.number_methods.get(method)
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

    // Phase 6: Global parseInt / parseFloat
    registry.register_function("parseInt", lower_parse_int_global);
    registry.register_function("parseFloat", lower_parse_float_global);

    // Timer functions: setTimeout, setInterval, clearTimeout, clearInterval
    registry.register_function("setTimeout", lower_set_timeout);
    registry.register_function("setInterval", lower_set_interval);
    registry.register_function("clearTimeout", lower_clear_timeout);
    registry.register_function("clearInterval", lower_clear_interval);

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

    // String methods that take a regex argument
    registry.register_string_method("match", lower_string_match);
    registry.register_string_method("search", lower_string_search);

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
    registry.register_collection_method("reduceRight", lower_array_reduce_right);

    // Phase 5: Array non-iterator methods (registered as string methods for dispatch)
    registry.register_string_method("push", lower_array_push);
    registry.register_string_method("pop", lower_array_pop);
    registry.register_string_method("shift", lower_array_shift);
    registry.register_string_method("unshift", lower_array_unshift);
    registry.register_string_method("reverse", lower_array_reverse);
    registry.register_string_method("sort", lower_array_sort);
    registry.register_string_method("join", lower_array_join);
    registry.register_string_method("fill", lower_array_fill);
    registry.register_string_method("splice", lower_array_splice);
    registry.register_string_method("copyWithin", lower_array_copy_within);

    // Array-specific methods that need type-aware dispatch.
    // These method names also exist on strings with different semantics,
    // so they are only consulted when the receiver is known to be a Vec.
    registry.register_array_method("indexOf", lower_array_index_of);
    registry.register_array_method("lastIndexOf", lower_array_last_index_of);
    registry.register_array_method("includes", lower_array_includes);
    registry.register_array_method("at", lower_array_at);
    registry.register_array_method("concat", lower_array_concat);
    registry.register_array_method("slice", lower_array_slice);
    registry.register_array_method("keys", lower_array_keys);
    registry.register_array_method("values", lower_array_values);
    registry.register_array_method("entries", lower_array_entries);

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
    registry.register_map_set_method("forEach", lower_map_for_each);

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
    // Task 169: 21 missing Math methods
    registry.register_method("Math", "acos", lower_math_acos, false);
    registry.register_method("Math", "acosh", lower_math_acosh, false);
    registry.register_method("Math", "asin", lower_math_asin, false);
    registry.register_method("Math", "asinh", lower_math_asinh, false);
    registry.register_method("Math", "atan", lower_math_atan, false);
    registry.register_method("Math", "atan2", lower_math_atan2, false);
    registry.register_method("Math", "atanh", lower_math_atanh, false);
    registry.register_method("Math", "cbrt", lower_math_cbrt, false);
    registry.register_method("Math", "cosh", lower_math_cosh, false);
    registry.register_method("Math", "exp", lower_math_exp, false);
    registry.register_method("Math", "expm1", lower_math_expm1, false);
    registry.register_method("Math", "fround", lower_math_fround, false);
    registry.register_method("Math", "hypot", lower_math_hypot, false);
    registry.register_method("Math", "log10", lower_math_log10, false);
    registry.register_method("Math", "log1p", lower_math_log1p, false);
    registry.register_method("Math", "log2", lower_math_log2, false);
    registry.register_method("Math", "sign", lower_math_sign, false);
    registry.register_method("Math", "sinh", lower_math_sinh, false);
    registry.register_method("Math", "tanh", lower_math_tanh, false);
    registry.register_method("Math", "trunc", lower_math_trunc, false);
    registry.register_method("Math", "clz32", lower_math_clz32, false);

    // Phase 5: console extensions
    registry.register_method("console", "error", lower_console_error, true);
    registry.register_method("console", "warn", lower_console_warn, true);
    registry.register_method("console", "debug", lower_console_debug, true);

    // Task 141: Additional console extensions
    registry.register_method("console", "table", lower_console_table, true);
    registry.register_method("console", "dir", lower_console_dir, true);
    registry.register_method("console", "assert", lower_console_assert, true);
    registry.register_method("console", "time", lower_console_time, true);
    registry.register_method("console", "timeEnd", lower_console_time_end, true);
    registry.register_method("console", "timeLog", lower_console_time_log, true);
    registry.register_method("console", "count", lower_console_count, true);
    registry.register_method("console", "countReset", lower_console_count_reset, true);
    registry.register_method("console", "group", lower_console_group, true);
    registry.register_method("console", "groupEnd", lower_console_group_end, true);
    registry.register_method("console", "clear", lower_console_clear, true);
    registry.register_method("console", "trace", lower_console_trace, true);

    // Phase 5: Number functions
    registry.register_method("Number", "parseInt", lower_number_parse_int, false);
    registry.register_method("Number", "parseFloat", lower_number_parse_float, false);
    registry.register_method("Number", "isNaN", lower_number_is_nan, false);
    registry.register_method("Number", "isFinite", lower_number_is_finite, false);
    registry.register_method("Number", "isInteger", lower_number_is_integer, false);
    registry.register_method(
        "Number",
        "isSafeInteger",
        lower_number_is_safe_integer,
        false,
    );

    // Global isNaN / isFinite (bare function calls without Number. prefix)
    registry.register_function("isNaN", lower_number_is_nan);
    registry.register_function("isFinite", lower_number_is_finite);

    // Phase 5: Object utilities
    registry.register_method("Object", "keys", lower_object_keys, false);
    registry.register_method("Object", "values", lower_object_values, false);
    registry.register_method("Object", "entries", lower_object_entries, false);
    registry.register_method("Object", "assign", lower_object_assign, false);
    registry.register_method("Object", "fromEntries", lower_object_from_entries, false);
    registry.register_method("Object", "freeze", lower_object_freeze, false);
    registry.register_method("Object", "create", lower_object_create, false);
    registry.register_method("Object", "hasOwn", lower_object_has_own, false);
    registry.register_method("Object", "is", lower_object_is, false);
    registry.register_method("Object", "isFrozen", lower_object_is_frozen, false);
    registry.register_method(
        "Object",
        "getOwnPropertyNames",
        lower_object_get_own_property_names,
        false,
    );
    registry.register_method(
        "Object",
        "getPrototypeOf",
        lower_object_get_prototype_of,
        false,
    );
    registry.register_method(
        "Object",
        "defineProperty",
        lower_object_define_property,
        false,
    );

    // Phase 5: JSON methods
    registry.register_method("JSON", "stringify", lower_json_stringify, false);
    registry.register_method("JSON", "parse", lower_json_parse, false);

    // Phase 6: Static Array methods
    registry.register_method("Array", "from", lower_array_from, false);
    registry.register_method("Array", "isArray", lower_array_is_array, false);
    registry.register_method("Array", "of", lower_array_of, false);

    // Phase 6: Static String methods
    registry.register_method("String", "fromCharCode", lower_string_from_char_code, false);
    registry.register_method(
        "String",
        "fromCodePoint",
        lower_string_from_code_point,
        false,
    );

    // Task 178: Additional String instance methods
    registry.register_string_method("codePointAt", lower_code_point_at);
    registry.register_string_method("matchAll", lower_match_all);
    registry.register_string_method("normalize", lower_normalize);
    registry.register_string_method("localeCompare", lower_locale_compare);

    // Task 178: String.raw static method
    registry.register_method("String", "raw", lower_string_raw, false);

    // Promise utility methods (bare, non-awaited usage)
    registry.register_method("Promise", "resolve", lower_promise_resolve, false);
    registry.register_method("Promise", "reject", lower_promise_reject, false);

    // Task 129: Date class — static methods
    registry.register_method("Date", "now", lower_date_now, false);
    // Task 173: Date.parse and Date.UTC static methods
    registry.register_method("Date", "parse", lower_date_parse, false);
    registry.register_method("Date", "UTC", lower_date_utc, false);

    // Task 129: Date class — instance methods (type-aware dispatch)
    registry.register_date_method("getTime", lower_date_get_time);
    registry.register_date_method("toISOString", lower_date_to_iso_string);
    registry.register_date_method("toString", lower_date_to_string);

    // Task 170: Date getter methods — calendar component extraction
    registry.register_date_method("getFullYear", lower_date_get_full_year);
    registry.register_date_method("getMonth", lower_date_get_month);
    registry.register_date_method("getDate", lower_date_get_date);
    registry.register_date_method("getDay", lower_date_get_day);
    registry.register_date_method("getHours", lower_date_get_hours);
    registry.register_date_method("getMinutes", lower_date_get_minutes);
    registry.register_date_method("getSeconds", lower_date_get_seconds);
    registry.register_date_method("getMilliseconds", lower_date_get_milliseconds);
    registry.register_date_method("getTimezoneOffset", lower_date_get_timezone_offset);

    // Task 171: Date setter methods — reconstruct SystemTime with modified components
    registry.register_date_method("setTime", lower_date_set_time);
    registry.register_date_method("setFullYear", lower_date_set_full_year);
    registry.register_date_method("setMonth", lower_date_set_month);
    registry.register_date_method("setDate", lower_date_set_date);
    registry.register_date_method("setHours", lower_date_set_hours);
    registry.register_date_method("setMinutes", lower_date_set_minutes);
    registry.register_date_method("setSeconds", lower_date_set_seconds);
    registry.register_date_method("setMilliseconds", lower_date_set_milliseconds);

    // Task 172: Date formatting methods
    registry.register_date_method("toDateString", lower_date_to_date_string);
    registry.register_date_method("toTimeString", lower_date_to_time_string);
    registry.register_date_method("toUTCString", lower_date_to_utc_string);
    registry.register_date_method("toJSON", lower_date_to_json);
    registry.register_date_method("toLocaleDateString", lower_date_to_locale_date_string);
    registry.register_date_method("toLocaleString", lower_date_to_locale_string);
    registry.register_date_method("toLocaleTimeString", lower_date_to_locale_time_string);
    registry.register_date_method("valueOf", lower_date_value_of);

    // RegExp methods — registered in regex_methods for type-aware dispatch.
    // Method names like `test` and `exec` are common and would conflict
    // with user-defined class methods if registered as string methods.
    registry.register_regex_method("test", lower_regexp_test);
    registry.register_regex_method("exec", lower_regexp_exec);

    // Task 160: Number instance methods — registered in number_methods for
    // type-aware dispatch. Method names like `toString` would conflict with
    // other types if registered as string methods.
    registry.register_number_method("toFixed", lower_number_to_fixed);
    registry.register_number_method("toPrecision", lower_number_to_precision);
    registry.register_number_method("toExponential", lower_number_to_exponential);
    registry.register_number_method("toString", lower_number_to_string);

    // Task 176: Global structuredClone and queueMicrotask
    registry.register_function("structuredClone", lower_structured_clone);
    registry.register_function("queueMicrotask", lower_queue_microtask);

    // Task 175: Encoding/decoding global functions
    registry.register_function("btoa", lower_btoa);
    registry.register_function("atob", lower_atob);
    registry.register_function("encodeURIComponent", lower_encode_uri_component);
    registry.register_function("decodeURIComponent", lower_decode_uri_component);
    registry.register_function("encodeURI", lower_encode_uri);
    registry.register_function("decodeURI", lower_decode_uri);
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
// Timer function lowering
// ---------------------------------------------------------------------------

/// Build `tokio::time::sleep(std::time::Duration::from_millis(ms)).await`.
///
/// Creates the Rust IR for the sleep-then-await expression used in both
/// `setTimeout` and `setInterval` lowering.
fn build_sleep_await(delay_expr: RustExpr, span: Span) -> RustExpr {
    let duration = RustExpr::new(
        RustExprKind::StaticCall {
            type_name: "std::time::Duration".into(),
            method: "from_millis".into(),
            args: vec![delay_expr],
        },
        span,
    );
    let sleep_call = RustExpr::new(
        RustExprKind::Call {
            func: "tokio::time::sleep".into(),
            args: vec![duration],
        },
        span,
    );
    RustExpr::new(RustExprKind::Await(Box::new(sleep_call)), span)
}

/// Extract callback body statements from a lowered callback argument.
///
/// If the argument is a `Closure`, extracts the body statements and inlines
/// them directly. If the argument is an `Ident` (function reference), generates
/// a `name()` call expression. For other expressions, wraps them in a
/// statement as-is.
fn callback_body_stmts(callback: RustExpr) -> Vec<RustStmt> {
    match callback.kind {
        RustExprKind::Closure { body, .. } => match body {
            RustClosureBody::Block(b) => {
                let mut stmts = b.stmts;
                if let Some(trailing) = b.expr {
                    stmts.push(RustStmt::Semi(*trailing));
                }
                stmts
            }
            RustClosureBody::Expr(e) => vec![RustStmt::Semi(*e)],
        },
        // Function reference — generate `name()`
        RustExprKind::Ident(name) => {
            vec![RustStmt::Semi(RustExpr::synthetic(RustExprKind::Call {
                func: name,
                args: vec![],
            }))]
        }
        // Other expression — emit as-is (e.g., already a call)
        other => vec![RustStmt::Semi(RustExpr::synthetic(other))],
    }
}

/// Lower `setTimeout(callback, delay)` to:
/// ```text
/// tokio::spawn(async move {
///     tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
///     <callback body>
/// })
/// ```
///
/// Extracts the closure body from the callback argument and inlines it after
/// the sleep. The result is a `tokio::spawn(...)` call that returns a `JoinHandle`.
fn lower_set_timeout(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    let mut args_iter = args.into_iter();
    let callback = args_iter.next().unwrap_or_else(|| {
        RustExpr::synthetic(RustExprKind::Closure {
            is_async: false,
            is_move: false,
            params: vec![],
            return_type: None,
            body: RustClosureBody::Block(RustBlock {
                stmts: vec![],
                expr: None,
            }),
        })
    });
    let delay = args_iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));

    let sleep_await = build_sleep_await(delay, arg_span);
    let body_stmts = callback_body_stmts(callback);

    let mut stmts = vec![RustStmt::Semi(sleep_await)];
    stmts.extend(body_stmts);

    let async_block = RustExpr::synthetic(RustExprKind::AsyncBlock {
        is_move: true,
        body: RustBlock { stmts, expr: None },
    });

    RustExpr::new(
        RustExprKind::Call {
            func: "tokio::spawn".into(),
            args: vec![async_block],
        },
        arg_span,
    )
}

/// Lower `setInterval(callback, delay)` to:
/// ```text
/// tokio::spawn(async move {
///     loop {
///         tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
///         <callback body>
///     }
/// })
/// ```
///
/// Wraps the sleep + callback in an infinite `loop` inside the spawned task.
/// Returns a `JoinHandle` that can be aborted with `clearInterval`.
fn lower_set_interval(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    let mut args_iter = args.into_iter();
    let callback = args_iter.next().unwrap_or_else(|| {
        RustExpr::synthetic(RustExprKind::Closure {
            is_async: false,
            is_move: false,
            params: vec![],
            return_type: None,
            body: RustClosureBody::Block(RustBlock {
                stmts: vec![],
                expr: None,
            }),
        })
    });
    let delay = args_iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));

    let sleep_await = build_sleep_await(delay, arg_span);
    let body_stmts = callback_body_stmts(callback);

    let mut loop_stmts = vec![RustStmt::Semi(sleep_await)];
    loop_stmts.extend(body_stmts);

    let loop_stmt = RustStmt::Loop(RustLoopStmt {
        label: None,
        body: RustBlock {
            stmts: loop_stmts,
            expr: None,
        },
        span: Some(arg_span),
    });

    let async_block = RustExpr::synthetic(RustExprKind::AsyncBlock {
        is_move: true,
        body: RustBlock {
            stmts: vec![loop_stmt],
            expr: None,
        },
    });

    RustExpr::new(
        RustExprKind::Call {
            func: "tokio::spawn".into(),
            args: vec![async_block],
        },
        arg_span,
    )
}

/// Lower `clearTimeout(handle)` to `handle.abort()`.
///
/// Cancels a spawned timeout task by calling `.abort()` on the `JoinHandle`.
fn lower_clear_timeout(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    let handle = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_handle".into())));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(handle),
            method: "abort".into(),
            type_args: vec![],
            args: vec![],
        },
        arg_span,
    )
}

/// Lower `clearInterval(handle)` to `handle.abort()`.
///
/// Cancels a spawned interval task by calling `.abort()` on the `JoinHandle`.
fn lower_clear_interval(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    let handle = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_handle".into())));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(handle),
            method: "abort".into(),
            type_args: vec![],
            args: vec![],
        },
        arg_span,
    )
}

// ---------------------------------------------------------------------------
// Task 176: structuredClone and queueMicrotask
// ---------------------------------------------------------------------------

/// Lower `structuredClone(obj)` to `obj.clone()`.
///
/// JavaScript's `structuredClone` performs a deep clone of its argument.
/// Rust's `Clone` trait provides the same semantics when `#[derive(Clone)]`
/// is present, which the lowering pass adds to all user-defined types.
fn lower_structured_clone(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    let obj = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_obj".into())));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(obj),
            method: "clone".into(),
            type_args: vec![],
            args: vec![],
        },
        arg_span,
    )
}

/// Lower `queueMicrotask(fn)` to `tokio::spawn(async move { fn() })`.
///
/// JavaScript's `queueMicrotask` schedules a callback to run as soon as
/// the current task completes, without any additional delay. The closest
/// Rust equivalent is spawning an async task that runs the callback
/// immediately inside an async block — analogous to `setTimeout(fn, 0)`
/// but without the sleep.
fn lower_queue_microtask(args: Vec<RustExpr>, arg_span: Span) -> RustExpr {
    let callback = args.into_iter().next().unwrap_or_else(|| {
        RustExpr::synthetic(RustExprKind::Closure {
            is_async: false,
            is_move: false,
            params: vec![],
            return_type: None,
            body: RustClosureBody::Block(RustBlock {
                stmts: vec![],
                expr: None,
            }),
        })
    });

    let body_stmts = callback_body_stmts(callback);

    let async_block = RustExpr::synthetic(RustExprKind::AsyncBlock {
        is_move: true,
        body: RustBlock {
            stmts: body_stmts,
            expr: None,
        },
    });

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
// String methods taking a regex argument
// ---------------------------------------------------------------------------

/// Lower `str.match(regex)` to `regex.find_iter(&str).map(|m| m.as_str().to_string()).collect::<Vec<String>>()`.
///
/// `String.prototype.match()` returns an array of all matches. The Rust `regex`
/// crate equivalent chains `find_iter` → `map` → `collect`. The receiver (the
/// string) becomes the argument, and the first argument (the regex) becomes the
/// method receiver.
fn lower_string_match(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let regex_arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("regex".into())));

    // Step 1: regex.find_iter(&str)
    let borrow_receiver = RustExpr::synthetic(RustExprKind::Borrow(Box::new(receiver)));
    let find_iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(regex_arg),
            method: "find_iter".into(),
            type_args: vec![],
            args: vec![borrow_receiver],
        },
        span,
    );

    // Step 2: .map(|m| m.as_str().to_string())
    let closure_param = RustClosureParam {
        name: "m".into(),
        ty: None,
    };
    let as_str_call = RustExpr::synthetic(RustExprKind::MethodCall {
        receiver: Box::new(RustExpr::synthetic(RustExprKind::Ident("m".into()))),
        method: "as_str".into(),
        type_args: vec![],
        args: vec![],
    });
    let to_string_call = RustExpr::synthetic(RustExprKind::MethodCall {
        receiver: Box::new(as_str_call),
        method: "to_string".into(),
        type_args: vec![],
        args: vec![],
    });
    let map_closure = RustExpr::synthetic(RustExprKind::Closure {
        is_async: false,
        is_move: false,
        params: vec![closure_param],
        return_type: None,
        body: RustClosureBody::Expr(Box::new(to_string_call)),
    });
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(find_iter_call),
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

/// Lower `str.search(regex)` to `regex.find(&str).map(|m| m.start() as i64).unwrap_or(-1)`.
///
/// `String.prototype.search()` returns the index of the first match, or -1 if
/// not found. The Rust `regex` crate equivalent chains `find` → `map` →
/// `unwrap_or`. The receiver (the string) becomes the argument, and the first
/// argument (the regex) becomes the method receiver.
fn lower_string_search(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let regex_arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("regex".into())));

    // Step 1: regex.find(&str)
    let borrow_receiver = RustExpr::synthetic(RustExprKind::Borrow(Box::new(receiver)));
    let find_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(regex_arg),
            method: "find".into(),
            type_args: vec![],
            args: vec![borrow_receiver],
        },
        span,
    );

    // Step 2: .map(|m| m.start() as i64)
    let closure_param = RustClosureParam {
        name: "m".into(),
        ty: None,
    };
    let start_call = RustExpr::synthetic(RustExprKind::MethodCall {
        receiver: Box::new(RustExpr::synthetic(RustExprKind::Ident("m".into()))),
        method: "start".into(),
        type_args: vec![],
        args: vec![],
    });
    let cast_expr = RustExpr::synthetic(RustExprKind::Cast(Box::new(start_call), RustType::I64));
    let map_closure = RustExpr::synthetic(RustExprKind::Closure {
        is_async: false,
        is_move: false,
        params: vec![closure_param],
        return_type: None,
        body: RustClosureBody::Expr(Box::new(cast_expr)),
    });
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(find_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![map_closure],
        },
        span,
    );

    // Step 3: .unwrap_or(-1)
    let neg_one = RustExpr::synthetic(RustExprKind::Unary {
        op: RustUnaryOp::Neg,
        operand: Box::new(RustExpr::synthetic(RustExprKind::IntLit(1))),
    });
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(map_call),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![neg_one],
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Task 178: String gap methods
// ---------------------------------------------------------------------------

/// Lower `str.codePointAt(index)` to `str.chars().nth(index as usize).map(|c| c as i64)`.
///
/// Returns the Unicode code point (as `Option<i64>`) at the given character
/// position. Mirrors `charCodeAt` but named for the code-point API.
fn lower_code_point_at(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
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
    let cast_index = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(index),
        RustType::Named("usize".into()),
    ));
    let nth_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(chars_call),
            method: "nth".into(),
            type_args: vec![],
            args: vec![cast_index],
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
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Cast(
                    Box::new(RustExpr::synthetic(RustExprKind::Ident("c".into()))),
                    RustType::I64,
                )))),
            })],
        },
        span,
    )
}

/// Lower `str.matchAll(regex)` to `regex.find_iter(&str).map(|m| m.as_str().to_string()).collect::<Vec<String>>()`.
///
/// Identical Rust lowering to `match` — `find_iter` always collects all matches.
fn lower_match_all(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    lower_string_match(receiver, args, span)
}

/// Lower `str.normalize(form)` to `str.clone()`.
///
/// MVP: full Unicode normalization requires ICU. Rust `String` is already
/// valid UTF-8. Pass through unchanged; the `form` argument is ignored.
fn lower_normalize(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "clone".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `str.localeCompare(other)` to a simplified `cmp`-based comparison.
///
/// Returns -1, 0, or 1 matching JS semantics. Not locale-aware — uses
/// bytewise Unicode ordering. Emits a block expression with a `match` on
/// `std::cmp::Ordering` because `Ordering` cannot be directly cast to `i32`.
fn lower_locale_compare(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let other = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));

    // Bind both sides to locals then emit a raw match on the ordering.
    // Generated Rust:
    //   { let __lhs = <receiver>; let __rhs = <other>;
    //     match __lhs.as_str().cmp(__rhs.as_str()) {
    //       std::cmp::Ordering::Less => -1_i32,
    //       std::cmp::Ordering::Equal => 0_i32,
    //       std::cmp::Ordering::Greater => 1_i32,
    //     }
    //   }
    let block = RustBlock {
        stmts: vec![
            RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "__lhs".into(),
                ty: None,
                init: receiver,
                span: None,
            }),
            RustStmt::Let(RustLetStmt {
                mutable: false,
                name: "__rhs".into(),
                ty: None,
                init: other,
                span: None,
            }),
            RustStmt::Expr(RustExpr::new(
                RustExprKind::Raw(
                    "match __lhs.as_str().cmp(__rhs.as_str()) { \
                     std::cmp::Ordering::Less => -1_i32, \
                     std::cmp::Ordering::Equal => 0_i32, \
                     std::cmp::Ordering::Greater => 1_i32 }"
                        .into(),
                ),
                span,
            )),
        ],
        expr: None,
    };
    RustExpr::new(RustExprKind::BlockExpr(block), span)
}

/// Lower `String.raw(strings, ...values)` — tagged template literal static helper.
///
/// For MVP, lower to `format!` concatenating all provided arguments. This
/// covers the common case where template parts and interpolated values are
/// already flattened by the parser into a flat argument list.
fn lower_string_raw(args: Vec<RustExpr>, span: Span) -> RustExpr {
    if args.is_empty() {
        return RustExpr::new(RustExprKind::StringLit(String::new()), span);
    }
    let fmt = args.iter().map(|_| "{}").collect::<Vec<_>>().join("");
    let mut macro_args = vec![RustExpr::synthetic(RustExprKind::StringLit(fmt))];
    macro_args.extend(args.into_iter().map(strip_to_string));
    RustExpr::new(
        RustExprKind::Macro {
            name: "format".into(),
            args: macro_args,
        },
        span,
    )
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

/// Lower `.splice(start, deleteCount)` to `arr.drain(start..start+deleteCount).collect::<Vec<_>>()`.
///
/// `arr.splice(1, 2)` → `arr.drain(1_usize..(1 + 2) as usize).collect::<Vec<_>>()`
/// Returns the removed elements as a Vec.
fn lower_array_splice(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let start = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let delete_count = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));

    let start_cast = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(start.clone()),
        RustType::Named("usize".into()),
    ));
    // end = (start + deleteCount) as usize
    let sum = RustExpr::synthetic(RustExprKind::Binary {
        op: rsc_syntax::rust_ir::RustBinaryOp::Add,
        left: Box::new(start),
        right: Box::new(delete_count),
    });
    let end_cast = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(sum),
        RustType::Named("usize".into()),
    ));

    let range_expr = RustExpr::synthetic(RustExprKind::Ident(format!(
        "{}..{}",
        emit_inline(&start_cast),
        emit_inline(&end_cast)
    )));

    let drain_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "drain".into(),
            type_args: vec![],
            args: vec![range_expr],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(drain_call),
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

/// Lower `.copyWithin(target, start, end)` to `arr.copy_within(start..end, target)`.
///
/// `arr.copyWithin(0, 2, 4)` → `arr.copy_within(2_usize..4_usize, 0_usize)`
/// Note: argument order differs — JS is `(target, start, end)`, Rust is `(start..end, target)`.
fn lower_array_copy_within(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let target = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let start = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let end = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));

    let start_cast = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(start),
        RustType::Named("usize".into()),
    ));
    let end_cast = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(end),
        RustType::Named("usize".into()),
    ));
    let target_cast = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(target),
        RustType::Named("usize".into()),
    ));

    let range_expr = RustExpr::synthetic(RustExprKind::Ident(format!(
        "{}..{}",
        emit_inline(&start_cast),
        emit_inline(&end_cast)
    )));

    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "copy_within".into(),
            type_args: vec![],
            args: vec![range_expr, target_cast],
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Array-specific methods (type-aware dispatch — only for Vec receivers)
// ---------------------------------------------------------------------------

/// Lower `.indexOf(item)` on arrays to `.iter().position(|x| *x == item).map(|i| i as i64).unwrap_or(-1)`.
///
/// Unlike string `indexOf` which uses `.find()` for byte-position lookup,
/// array `indexOf` compares elements for equality.
fn lower_array_index_of(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let item = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));

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
                params: vec![RustClosureParam {
                    name: "x".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Binary {
                    op: rsc_syntax::rust_ir::RustBinaryOp::Eq,
                    left: Box::new(RustExpr::synthetic(RustExprKind::Ident("*x".into()))),
                    right: Box::new(item),
                }))),
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

/// Lower `.lastIndexOf(item)` on arrays to `.iter().rposition(|x| *x == item).map(|i| i as i64).unwrap_or(-1)`.
fn lower_array_last_index_of(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let item = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));

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
                params: vec![RustClosureParam {
                    name: "x".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Binary {
                    op: rsc_syntax::rust_ir::RustBinaryOp::Eq,
                    left: Box::new(RustExpr::synthetic(RustExprKind::Ident("*x".into()))),
                    right: Box::new(item),
                }))),
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

/// Lower `.includes(item)` on arrays to `.contains(&item)`.
///
/// Array `includes` uses `Vec::contains` with a borrow, while string `includes`
/// uses `str::contains` with pattern matching. The borrow semantics differ.
fn lower_array_includes(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let item = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "contains".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Borrow(Box::new(item)))],
        },
        span,
    )
}

/// Lower `.at(index)` on arrays to index access with negative index support.
///
/// `arr.at(i)` → if i >= 0: `arr.get(i as usize).cloned()`
///               if i < 0: `arr.get(arr.len().wrapping_add(i as usize)).cloned()`
///
/// Since the sign is not known at compile time, we emit a helper expression:
/// `{ let i = index; if i >= 0 { arr.get(i as usize).cloned() } else { arr.get(arr.len().wrapping_sub((-i) as usize)).cloned() } }`
///
/// For simplicity, we emit the common positive case: `arr.get(index as usize).cloned()`
fn lower_array_at(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let index = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let cast_expr = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(index),
        RustType::Named("usize".into()),
    ));
    let get_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "get".into(),
            type_args: vec![],
            args: vec![cast_expr],
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

/// Lower `.concat(other)` on arrays to extending a cloned vec.
///
/// `arr.concat(other)` → `{ let mut v = arr.clone(); v.extend(other.iter().cloned()); v }`
///
/// We emit this as a method chain for simplicity:
/// `[arr.as_slice(), other.as_slice()].concat()`
fn lower_array_concat(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let other = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::VecLit(vec![])));

    // Build: [receiver.as_slice(), other.as_slice()].concat()
    let recv_slice = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "as_slice".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let other_slice = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(other),
            method: "as_slice".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let array_of_slices = RustExpr::synthetic(RustExprKind::VecLit(vec![recv_slice, other_slice]));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(array_of_slices),
            method: "concat".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.slice(start, end?)` on arrays to cloned sub-slice.
///
/// One arg:  `arr[start as usize..].to_vec()`
/// Two args: `arr[start as usize..end as usize].to_vec()`
fn lower_array_slice(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
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
            method: "to_vec".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Array iterator methods: keys, values, entries
// ---------------------------------------------------------------------------

/// Lower `.keys()` to `(0..arr.len()).collect::<Vec<_>>()`.
///
/// Returns an array of indices: `[0, 1, 2, ...]`.
fn lower_array_keys(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let len_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "len".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let range = RustExpr::new(
        RustExprKind::Range {
            start: Box::new(RustExpr::synthetic(RustExprKind::IntLit(0))),
            end: Box::new(len_call),
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(range),
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
                    RustType::Named("i64".into()),
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

/// Lower `.values()` to `arr.clone()`.
///
/// The values of an array are just its elements, so cloning the array suffices.
fn lower_array_values(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "clone".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `.entries()` to `arr.iter().cloned().enumerate().map(|(i, v)| (i as i64, v)).collect::<Vec<_>>()`.
///
/// Returns an array of `(index, value)` tuples.
fn lower_array_entries(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let cloned_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "cloned".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let enumerate_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(cloned_call),
            method: "enumerate".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let map_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(enumerate_call),
            method: "map".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "(i, v)".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Tuple(
                    vec![
                        RustExpr::synthetic(RustExprKind::Cast(
                            Box::new(RustExpr::synthetic(RustExprKind::Ident("i".into()))),
                            RustType::Named("i64".into()),
                        )),
                        RustExpr::synthetic(RustExprKind::Ident("v".into())),
                    ],
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

/// Lower `map.forEach((value, key) => { ... })` to `map.iter().for_each(|(key, value)| { ... })`.
///
/// In TypeScript, `Map.forEach` passes `(value, key, map)` to the callback.
/// In Rust, `HashMap::iter()` yields `(&key, &value)` tuples, so we swap
/// the first two closure parameters into a tuple pattern `(key, value)`.
///
/// Instead of using `IteratorChain`/`ForEach` (which only supports single-expression
/// bodies), this emits `receiver.iter().for_each(closure)` as a method chain,
/// preserving multi-statement closure bodies.
fn lower_map_for_each(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let closure = args.into_iter().next().map_or_else(
        || {
            RustExpr::synthetic(RustExprKind::Closure {
                is_async: false,
                is_move: false,
                params: vec![RustClosureParam {
                    name: "_".into(),
                    ty: None,
                }],
                return_type: None,
                body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Ident(
                    "()".into(),
                )))),
            })
        },
        rewrite_map_foreach_closure,
    );

    let iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "for_each".into(),
            type_args: vec![],
            args: vec![closure],
        },
        span,
    )
}

/// Lower `set.forEach((value) => { ... })` to `set.iter().for_each(|value| { ... })`.
///
/// Set forEach is identical to array forEach — single-parameter callback.
/// Uses direct method chain to preserve multi-statement closure bodies.
pub(crate) fn lower_set_for_each(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let closure = args.into_iter().next().unwrap_or_else(|| {
        RustExpr::synthetic(RustExprKind::Closure {
            is_async: false,
            is_move: false,
            params: vec![RustClosureParam {
                name: "_".into(),
                ty: None,
            }],
            return_type: None,
            body: RustClosureBody::Expr(Box::new(RustExpr::synthetic(RustExprKind::Ident(
                "()".into(),
            )))),
        })
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
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(iter_call),
            method: "for_each".into(),
            type_args: vec![],
            args: vec![closure],
        },
        span,
    )
}

/// Rewrite a Map `forEach` closure to swap `(value, key)` params to `(key, value)` tuple.
///
/// TypeScript's `Map.forEach((value, key) => ...)` passes value first, key second.
/// Rust's `HashMap::iter()` yields `(key, value)` tuples. This rewrites the closure
/// params into a single tuple parameter `(key, value)` with the names swapped.
fn rewrite_map_foreach_closure(arg: RustExpr) -> RustExpr {
    if let RustExprKind::Closure {
        is_async,
        is_move,
        params,
        return_type,
        body,
    } = arg.kind
    {
        let tuple_param = make_map_foreach_param(&params);
        RustExpr::synthetic(RustExprKind::Closure {
            is_async,
            is_move,
            params: vec![tuple_param],
            return_type,
            body,
        })
    } else {
        arg
    }
}

/// Build a tuple closure param `(key, value)` from the TS forEach params `[value, key, ...]`.
///
/// Swaps the first two params (TS: value, key → Rust: key, value) and wraps them
/// in a tuple pattern for destructuring.
fn make_map_foreach_param(params: &[RustClosureParam]) -> RustClosureParam {
    let value_name = params.first().map_or("_v", |p| p.name.as_str());
    let key_name = params.get(1).map_or("_k", |p| p.name.as_str());
    RustClosureParam {
        name: format!("({key_name}, {value_name})"),
        ty: None,
    }
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

// ---------------------------------------------------------------------------
// Task 169: 21 missing Math methods
// ---------------------------------------------------------------------------

/// Lower `Math.acos(x)` to `x.acos()`.
fn lower_math_acos(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "acos".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.acosh(x)` to `x.acosh()`.
fn lower_math_acosh(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "acosh".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.asin(x)` to `x.asin()`.
fn lower_math_asin(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "asin".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.asinh(x)` to `x.asinh()`.
fn lower_math_asinh(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "asinh".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.atan(x)` to `x.atan()`.
fn lower_math_atan(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "atan".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.atan2(y, x)` to `y.atan2(x)`.
fn lower_math_atan2(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let y = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    let x = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(y),
            method: "atan2".into(),
            type_args: vec![],
            args: vec![x],
        },
        span,
    )
}

/// Lower `Math.atanh(x)` to `x.atanh()`.
fn lower_math_atanh(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "atanh".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.cbrt(x)` to `x.cbrt()`.
fn lower_math_cbrt(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "cbrt".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.cosh(x)` to `x.cosh()`.
fn lower_math_cosh(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "cosh".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.exp(x)` to `x.exp()`.
fn lower_math_exp(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "exp".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.expm1(x)` to `x.exp_m1()`.
fn lower_math_expm1(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "exp_m1".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.fround(x)` to `(x as f32) as f64`.
fn lower_math_fround(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    let as_f32 = RustExpr::new(RustExprKind::Cast(Box::new(arg), RustType::F32), span);
    RustExpr::new(RustExprKind::Cast(Box::new(as_f32), RustType::F64), span)
}

/// Lower `Math.hypot(x, y)` to `x.hypot(y)`.
fn lower_math_hypot(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let x = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    let y = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::FloatLit(0.0)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(x),
            method: "hypot".into(),
            type_args: vec![],
            args: vec![y],
        },
        span,
    )
}

/// Lower `Math.log10(x)` to `x.log10()`.
fn lower_math_log10(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "log10".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.log1p(x)` to `x.ln_1p()`.
fn lower_math_log1p(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "ln_1p".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.log2(x)` to `x.log2()`.
fn lower_math_log2(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "log2".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.sign(x)` to `x.signum()`.
fn lower_math_sign(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "signum".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.sinh(x)` to `x.sinh()`.
fn lower_math_sinh(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "sinh".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.tanh(x)` to `x.tanh()`.
fn lower_math_tanh(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "tanh".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.trunc(x)` to `x.trunc()`.
fn lower_math_trunc(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "trunc".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    )
}

/// Lower `Math.clz32(x)` to `(x as i32).leading_zeros() as f64`.
fn lower_math_clz32(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = first_arg_or_zero(args);
    let as_i32 = RustExpr::new(RustExprKind::Cast(Box::new(arg), RustType::I32), span);
    let leading_zeros = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(as_i32),
            method: "leading_zeros".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::Cast(Box::new(leading_zeros), RustType::F64),
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
// Task 141: Additional console extension lowering functions
// ---------------------------------------------------------------------------

/// Lower `console.table(data)` to `println!("{:#?}", data)` (debug pretty-print).
fn lower_console_table(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let macro_args = vec![
        RustExpr::synthetic(RustExprKind::StringLit("{:#?}".into())),
        strip_to_string(arg),
    ];
    RustExpr::new(
        RustExprKind::Macro {
            name: "println".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.dir(obj)` to `println!("{:#?}", obj)` (debug pretty-print).
///
/// Same output as `console.table` — both use Rust's `Debug` pretty-print.
fn lower_console_dir(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let macro_args = vec![
        RustExpr::synthetic(RustExprKind::StringLit("{:#?}".into())),
        strip_to_string(arg),
    ];
    RustExpr::new(
        RustExprKind::Macro {
            name: "println".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.assert(cond, msg?)` to `assert!(cond, "{}", msg)`.
///
/// If no message is provided, emits `assert!(cond)` without a format string.
fn lower_console_assert(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut args_iter = args.into_iter();
    let cond = args_iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::BoolLit(false)));
    let macro_args = if let Some(msg) = args_iter.next() {
        vec![
            cond,
            RustExpr::synthetic(RustExprKind::StringLit("{}".into())),
            strip_to_string(msg),
        ]
    } else {
        vec![cond]
    };
    RustExpr::new(
        RustExprKind::Macro {
            name: "assert".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.time(label)` to `eprintln!("{}: timer started", label)`.
///
/// True timing requires runtime state tracking. This simplified version
/// prints the label to stderr to indicate the timer was started.
fn lower_console_time(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let label = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit("default".into())));
    let macro_args = vec![
        RustExpr::synthetic(RustExprKind::StringLit("{}: timer started".into())),
        strip_to_string(label),
    ];
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.timeEnd(label)` to `eprintln!("{}: timer ended", label)`.
///
/// True timing requires runtime state tracking. This simplified version
/// prints the label to stderr to indicate the timer was ended.
fn lower_console_time_end(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let label = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit("default".into())));
    let macro_args = vec![
        RustExpr::synthetic(RustExprKind::StringLit("{}: timer ended".into())),
        strip_to_string(label),
    ];
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.timeLog(label)` to `eprintln!("{}: <time>", label)`.
///
/// True timing requires runtime state tracking. This simplified version
/// prints the label to stderr to indicate a timer checkpoint was logged.
fn lower_console_time_log(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let label = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit("default".into())));
    let macro_args = vec![
        RustExpr::synthetic(RustExprKind::StringLit("{}: <time>".into())),
        strip_to_string(label),
    ];
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.count(label?)` to `eprintln!("{}: count", label)`.
///
/// True counting requires runtime state. This simplified version
/// prints the label to stderr.
fn lower_console_count(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let label = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit("default".into())));
    let macro_args = vec![
        RustExpr::synthetic(RustExprKind::StringLit("{}: count".into())),
        strip_to_string(label),
    ];
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.countReset(label?)` to `eprintln!("{}: count reset", label)`.
///
/// Stub — prints a message to stderr indicating the counter was reset.
fn lower_console_count_reset(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let label = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit("default".into())));
    let macro_args = vec![
        RustExpr::synthetic(RustExprKind::StringLit("{}: count reset".into())),
        strip_to_string(label),
    ];
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.group(label?)` to `eprintln!("{}", label)`.
///
/// Indentation-based grouping is not easily representable in compiled output.
/// This simplified version prints the label to stderr.
fn lower_console_group(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let label = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let macro_args = vec![
        RustExpr::synthetic(RustExprKind::StringLit("{}".into())),
        strip_to_string(label),
    ];
    RustExpr::new(
        RustExprKind::Macro {
            name: "eprintln".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.groupEnd()` to a no-op (empty block expression).
///
/// Group tracking is not supported in compiled output.
fn lower_console_group_end(_args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(
        RustExprKind::BlockExpr(RustBlock {
            stmts: vec![],
            expr: None,
        }),
        span,
    )
}

/// Lower `console.clear()` to `print!("\x1B[2J\x1B[H")` (ANSI terminal clear).
fn lower_console_clear(_args: Vec<RustExpr>, span: Span) -> RustExpr {
    let macro_args = vec![RustExpr::synthetic(RustExprKind::StringLit(
        "\x1B[2J\x1B[H".into(),
    ))];
    RustExpr::new(
        RustExprKind::Macro {
            name: "print".into(),
            args: macro_args,
        },
        span,
    )
}

/// Lower `console.trace(args...)` to `eprintln!("Trace: {} {} ...", args...)`.
///
/// A simplified trace that prints "Trace:" followed by arguments.
/// Does not produce actual stack traces in compiled output.
fn lower_console_trace(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let format_placeholders = args.iter().map(|_| "{}").collect::<Vec<_>>().join(" ");
    let format_str = if format_placeholders.is_empty() {
        "Trace".to_owned()
    } else {
        format!("Trace: {format_placeholders}")
    };
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

// ---------------------------------------------------------------------------
// Phase 6: Global parseInt / parseFloat
// ---------------------------------------------------------------------------

/// Lower global `parseInt(str)` to `str.parse::<i64>().unwrap_or(0)`.
/// Lower global `parseInt(str, radix)` to `i64::from_str_radix(str, radix).unwrap_or(0)`.
fn lower_parse_int_global(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let str_arg = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let radix_arg = iter.next();

    let parse_expr = if let Some(radix) = radix_arg {
        // i64::from_str_radix(str, radix)
        RustExpr::new(
            RustExprKind::StaticCall {
                type_name: "i64".into(),
                method: "from_str_radix".into(),
                args: vec![str_arg, radix],
            },
            span,
        )
    } else {
        // str.parse::<i64>()
        RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(str_arg),
                method: "parse".into(),
                type_args: vec![RustType::I64],
                args: vec![],
            },
            span,
        )
    };

    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(parse_expr),
            method: "unwrap_or".into(),
            type_args: vec![],
            args: vec![RustExpr::synthetic(RustExprKind::IntLit(0))],
        },
        span,
    )
}

/// Lower global `parseFloat(str)` to `str.parse::<f64>().unwrap_or(0.0)`.
fn lower_parse_float_global(args: Vec<RustExpr>, span: Span) -> RustExpr {
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

/// Lower `Number.isSafeInteger(x)` to `x.is_finite() && (x as i64 as f64) == x && x.abs() <= 9007199254740991.0_f64`.
fn lower_number_is_safe_integer(args: Vec<RustExpr>, span: Span) -> RustExpr {
    use rsc_syntax::rust_ir::RustBinaryOp;

    let arg = first_arg_or_zero(args);

    // Part 1: x.is_finite()
    let is_finite = RustExpr::synthetic(RustExprKind::MethodCall {
        receiver: Box::new(arg.clone()),
        method: "is_finite".into(),
        type_args: vec![],
        args: vec![],
    });

    // Part 2: (x as i64 as f64) == x
    let int_cast = RustExpr::synthetic(RustExprKind::Cast(Box::new(arg.clone()), RustType::I64));
    let float_cast = RustExpr::synthetic(RustExprKind::Cast(Box::new(int_cast), RustType::F64));
    let is_integer = RustExpr::synthetic(RustExprKind::Binary {
        op: RustBinaryOp::Eq,
        left: Box::new(float_cast),
        right: Box::new(arg.clone()),
    });

    // Part 3: x.abs() <= 9007199254740991.0_f64
    let abs_call = RustExpr::synthetic(RustExprKind::MethodCall {
        receiver: Box::new(arg),
        method: "abs".into(),
        type_args: vec![],
        args: vec![],
    });
    let max_safe = RustExpr::synthetic(RustExprKind::FloatLit(9_007_199_254_740_991.0));
    let in_range = RustExpr::synthetic(RustExprKind::Binary {
        op: RustBinaryOp::Le,
        left: Box::new(abs_call),
        right: Box::new(max_safe),
    });

    // Combine: is_finite && is_integer && in_range
    let left = RustExpr::synthetic(RustExprKind::Binary {
        op: RustBinaryOp::And,
        left: Box::new(is_finite),
        right: Box::new(is_integer),
    });

    RustExpr::new(
        RustExprKind::Binary {
            op: RustBinaryOp::And,
            left: Box::new(left),
            right: Box::new(in_range),
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

/// Lower `Object.assign(target, source)` to a block that extends `target` with `source`.
///
/// Produces: `{ let mut tmp = target.clone(); tmp.extend(source.clone()); tmp }`
///
/// For `HashMap`-based objects, `extend` merges key-value pairs from the source
/// into the target, matching the semantics of `Object.assign()`.
fn lower_object_assign(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let target = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    let source = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));

    // Clone target, extend with source clone, return result
    let target_clone = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(target),
            method: "clone".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    let source_clone = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(source),
            method: "clone".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );

    // Build: { let mut tmp = target.clone(); tmp.extend(source.clone()); tmp }
    let tmp_ident = RustExpr::synthetic(RustExprKind::Ident("__rsc_tmp".into()));
    let extend_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(tmp_ident.clone()),
            method: "extend".into(),
            type_args: vec![],
            args: vec![source_clone],
        },
        span,
    );

    RustExpr::new(
        RustExprKind::ClosureCall {
            is_async: false,
            body: RustBlock {
                stmts: vec![
                    RustStmt::Let(RustLetStmt {
                        mutable: true,
                        name: "__rsc_tmp".into(),
                        ty: None,
                        init: target_clone,
                        span: None,
                    }),
                    RustStmt::Semi(extend_call),
                ],
                expr: Some(Box::new(tmp_ident)),
            },
            return_type: RustType::Infer,
        },
        span,
    )
}

/// Lower `Object.fromEntries(entries)` to `entries.into_iter().collect::<HashMap<_, _>>()`.
///
/// Converts a list of key-value tuples into a `HashMap`, matching the semantics
/// of `Object.fromEntries()`.
fn lower_object_from_entries(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    let into_iter_call = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "into_iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(into_iter_call),
            method: "collect".into(),
            type_args: vec![RustType::Generic(
                Box::new(RustType::Named("HashMap".into())),
                vec![RustType::Infer, RustType::Infer],
            )],
            args: vec![],
        },
        span,
    )
}

/// Lower `Object.freeze(obj)` — no-op in Rust.
///
/// Rust's `let` bindings are already immutable. `Object.freeze()` simply returns
/// the argument unchanged. A comment is emitted as documentation.
fn lower_object_freeze(args: Vec<RustExpr>, _span: Span) -> RustExpr {
    // Just pass through the argument — Rust let-bindings are immutable by default.
    args.into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())))
}

/// Lower `Object.create(proto)` — identity pass-through (stub).
///
/// `Object.create()` has no direct Rust equivalent. For now, return the argument
/// unchanged. Full prototype-based patterns are not supported.
fn lower_object_create(args: Vec<RustExpr>, _span: Span) -> RustExpr {
    // Stub: return the argument as-is.
    args.into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())))
}

/// Lower `Object.hasOwn(obj, key)` to `obj.contains_key(&key)`.
///
/// For `HashMap`-based objects this checks key membership. RustScript does not
/// have a prototype chain, so "own" is the only kind of property that exists.
fn lower_object_has_own(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let obj = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    let key = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    // contains_key expects &key
    let key_ref = RustExpr::synthetic(RustExprKind::Borrow(Box::new(key)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(obj),
            method: "contains_key".into(),
            type_args: vec![],
            args: vec![key_ref],
        },
        span,
    )
}

/// Lower `Object.is(a, b)` to `a == b`.
///
/// JavaScript's `Object.is` is identical to `===` for all values except NaN
/// and ±0. In RustScript, NaN comparisons rely on the underlying `f64` `==`,
/// which follows IEEE 754 (NaN ≠ NaN). This MVP lowers to plain `==`, which
/// is the correct behavior for all integer and string types used in RustScript.
fn lower_object_is(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut iter = args.into_iter();
    let a = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    let b = iter
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())));
    RustExpr::new(
        RustExprKind::Binary {
            op: rsc_syntax::rust_ir::RustBinaryOp::Eq,
            left: Box::new(a),
            right: Box::new(b),
        },
        span,
    )
}

/// Lower `Object.isFrozen(obj)` to `true`.
///
/// In Rust, `let` bindings are immutable by default. Every RustScript value is
/// effectively frozen unless declared `let mut`. This call always returns `true`.
fn lower_object_is_frozen(_args: Vec<RustExpr>, _span: Span) -> RustExpr {
    // Always frozen in Rust — immutability is the default.
    RustExpr::synthetic(RustExprKind::BoolLit(true))
}

/// Lower `Object.getOwnPropertyNames(obj)` to `obj.keys().cloned().collect::<Vec<_>>()`.
///
/// RustScript objects are structs or `HashMap`s with no prototype chain, so
/// every key is an own property. This is semantically identical to
/// `Object.keys()`.
fn lower_object_get_own_property_names(args: Vec<RustExpr>, span: Span) -> RustExpr {
    // Identical to Object.keys for RustScript (no prototype chain).
    lower_object_keys(args, span)
}

/// Lower `Object.getPrototypeOf(obj)` to `None`.
///
/// Rust structs have no prototype chain. This stub returns `None`, which is the
/// closest Rust equivalent to JavaScript's `null` prototype.
fn lower_object_get_prototype_of(_args: Vec<RustExpr>, _span: Span) -> RustExpr {
    // No prototype chain in Rust — return None.
    RustExpr::synthetic(RustExprKind::EnumVariant {
        enum_name: "Option".into(),
        variant_name: "None".into(),
    })
}

/// Lower `Object.defineProperty(obj, prop, descriptor)` — no-op stub.
///
/// Rust does not support runtime property definition. This call is lowered to
/// the first argument (the object), preserving the expression for type-checking
/// while emitting no side-effects.
fn lower_object_define_property(args: Vec<RustExpr>, _span: Span) -> RustExpr {
    // Runtime property definition is not supported in Rust.
    // Return the object argument unchanged (no-op).
    args.into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("_".into())))
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

// ---------------------------------------------------------------------------
// Phase 6: Static Array method lowering functions
// ---------------------------------------------------------------------------

/// Lower `Array.from(iterable)` to `iterable.into_iter().collect::<Vec<_>>()`.
///
/// Converts any iterable argument into a `Vec` by chaining `.into_iter()` and
/// `.collect::<Vec<_>>()`.
fn lower_array_from(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::VecLit(vec![])));
    let into_iter = RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(arg),
            method: "into_iter".into(),
            type_args: vec![],
            args: vec![],
        },
        span,
    );
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(into_iter),
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

/// Lower `Array.isArray(x)` to `true`.
///
/// In RustScript, arrays are always `Vec`, so this is statically known to be
/// `true` for any array argument. We emit the literal `true` directly.
fn lower_array_is_array(_args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(RustExprKind::BoolLit(true), span)
}

/// Lower `Array.of(a, b, c)` to `vec![a, b, c]`.
///
/// Collects all arguments into a `VecLit` IR node that emits as `vec![...]`.
fn lower_array_of(args: Vec<RustExpr>, span: Span) -> RustExpr {
    RustExpr::new(RustExprKind::VecLit(args), span)
}

// ---------------------------------------------------------------------------
// Phase 6: Static String method lowering functions
// ---------------------------------------------------------------------------

/// Lower `String.fromCharCode(code)` to
/// `char::from_u32(code as u32).map(|c| c.to_string()).unwrap_or_default()`.
///
/// For multiple arguments, lowers to
/// `vec![c1, c2, ...].into_iter().filter_map(|c| char::from_u32(c as u32)).collect::<String>()`.
fn lower_string_from_char_code(args: Vec<RustExpr>, span: Span) -> RustExpr {
    lower_from_char_code_impl(args, span)
}

/// Lower `String.fromCodePoint(code)` — same as `fromCharCode` in Rust,
/// since `char` is a Unicode scalar value.
fn lower_string_from_code_point(args: Vec<RustExpr>, span: Span) -> RustExpr {
    lower_from_char_code_impl(args, span)
}

/// Shared implementation for `fromCharCode` and `fromCodePoint`.
///
/// Single arg: `char::from_u32(code as u32).map(|c| c.to_string()).unwrap_or_default()`
/// Multiple args: `vec![c1, c2, ...].into_iter().filter_map(|c| char::from_u32(c as u32)).collect::<String>()`
fn lower_from_char_code_impl(args: Vec<RustExpr>, span: Span) -> RustExpr {
    if args.len() <= 1 {
        // Single argument (or zero — use 0 as default)
        let arg = args
            .into_iter()
            .next()
            .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
        let cast = RustExpr::synthetic(RustExprKind::Cast(Box::new(arg), RustType::U32));
        let from_u32 = RustExpr::new(
            RustExprKind::Call {
                func: "char::from_u32".into(),
                args: vec![cast],
            },
            span,
        );
        let map_call = RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(from_u32),
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
                            receiver: Box::new(RustExpr::synthetic(RustExprKind::Ident(
                                "c".into(),
                            ))),
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
    } else {
        // Multiple arguments: vec![c1, c2, ...].into_iter().filter_map(|c| char::from_u32(c as u32)).collect::<String>()
        let vec_lit = RustExpr::new(RustExprKind::VecLit(args), span);
        let into_iter = RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(vec_lit),
                method: "into_iter".into(),
                type_args: vec![],
                args: vec![],
            },
            span,
        );
        let filter_map = RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(into_iter),
                method: "filter_map".into(),
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
                        RustExprKind::Call {
                            func: "char::from_u32".into(),
                            args: vec![RustExpr::synthetic(RustExprKind::Cast(
                                Box::new(RustExpr::synthetic(RustExprKind::Ident("c".into()))),
                                RustType::U32,
                            ))],
                        },
                    ))),
                })],
            },
            span,
        );
        RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(filter_map),
                method: "collect".into(),
                type_args: vec![RustType::String],
                args: vec![],
            },
            span,
        )
    }
}

/// Check whether an expression uses `Math.PI` or `Math.E` properties.
///
/// This is called from the `FieldAccess` lowering in `expr_lower.rs` to
/// intercept `Math.PI`, `Math.E`, `Math.LN2`, `Math.LN10`, `Math.LOG2E`,
/// `Math.LOG10E`, `Math.SQRT2`, and `Math.SQRT1_2` before they are lowered
/// as normal field access expressions.
pub(crate) fn lower_math_constant(object_name: &str, field_name: &str) -> Option<RustExpr> {
    if object_name != "Math" {
        return None;
    }
    let rust_const = match field_name {
        "PI" => "std::f64::consts::PI",
        "E" => "std::f64::consts::E",
        "LN2" => "std::f64::consts::LN_2",
        "LN10" => "std::f64::consts::LN_10",
        "LOG2E" => "std::f64::consts::LOG2_E",
        "LOG10E" => "std::f64::consts::LOG10_E",
        "SQRT2" => "std::f64::consts::SQRT_2",
        "SQRT1_2" => "std::f64::consts::FRAC_1_SQRT_2",
        _ => return None,
    };
    Some(RustExpr::synthetic(RustExprKind::Ident(rust_const.into())))
}

/// Check whether an expression uses `Number.MAX_SAFE_INTEGER`,
/// `Number.MIN_SAFE_INTEGER`, `Number.EPSILON`, `Number.MAX_VALUE`,
/// `Number.MIN_VALUE`, `Number.NaN`, `Number.NEGATIVE_INFINITY`, or
/// `Number.POSITIVE_INFINITY` properties.
///
/// Called from the `FieldAccess` lowering in `expr_lower.rs` to intercept
/// these constants before they are lowered as normal field access expressions.
pub(crate) fn lower_number_constant(object_name: &str, field_name: &str) -> Option<RustExpr> {
    if object_name != "Number" {
        return None;
    }
    match field_name {
        "MAX_SAFE_INTEGER" => Some(RustExpr::synthetic(RustExprKind::IntLit(
            9_007_199_254_740_991,
        ))),
        "MIN_SAFE_INTEGER" => Some(RustExpr::synthetic(RustExprKind::IntLit(
            -9_007_199_254_740_991,
        ))),
        "EPSILON" => Some(RustExpr::synthetic(RustExprKind::Ident(
            "f64::EPSILON".into(),
        ))),
        "MAX_VALUE" => Some(RustExpr::synthetic(RustExprKind::Ident("f64::MAX".into()))),
        "MIN_VALUE" => Some(RustExpr::synthetic(RustExprKind::Ident(
            "f64::MIN_POSITIVE".into(),
        ))),
        "NaN" => Some(RustExpr::synthetic(RustExprKind::Ident("f64::NAN".into()))),
        "NEGATIVE_INFINITY" => Some(RustExpr::synthetic(RustExprKind::Ident(
            "f64::NEG_INFINITY".into(),
        ))),
        "POSITIVE_INFINITY" => Some(RustExpr::synthetic(RustExprKind::Ident(
            "f64::INFINITY".into(),
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

/// Check whether a `new` expression constructs a `RegExp`, requiring the `regex` crate.
pub(crate) fn needs_regex_crate(type_name: &str) -> bool {
    type_name == "RegExp"
}

// ---------------------------------------------------------------------------
// RegExp method lowering functions
// ---------------------------------------------------------------------------

/// Lower `regex.test(str)` to `regex.is_match(&str)`.
///
/// `RegExp.prototype.test()` returns a boolean indicating whether the pattern
/// matches the given string. The Rust `regex` crate equivalent is `is_match`.
fn lower_regexp_test(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let borrow_arg = RustExpr::synthetic(RustExprKind::Borrow(Box::new(arg)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "is_match".into(),
            type_args: vec![],
            args: vec![borrow_arg],
        },
        span,
    )
}

/// Lower `regex.exec(str)` to `regex.captures(&str)`.
///
/// `RegExp.prototype.exec()` returns an array of capture groups or null.
/// The Rust `regex` crate equivalent is `captures`, which returns `Option<Captures>`.
fn lower_regexp_exec(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let borrow_arg = RustExpr::synthetic(RustExprKind::Borrow(Box::new(arg)));
    RustExpr::new(
        RustExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: "captures".into(),
            type_args: vec![],
            args: vec![borrow_arg],
        },
        span,
    )
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

/// Lower `.reduceRight(fn, init)` to an `IteratorChain` with `Rev` op and `Fold` terminal.
///
/// `arr.reduceRight((acc, x) => acc + x, 0)` → `arr.iter().rev().fold(0, |acc, x| acc + x)`
/// Note: argument order is swapped — JS `reduceRight(fn, init)` → Rust `fold(init, fn)`.
fn lower_array_reduce_right(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
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
            ops: vec![IteratorOp::Rev],
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
            "reduceRight" => merge_reduce_right_into_chain(source, ops, outer_args, span),
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

/// Helper for merging a `reduceRight` call into an existing iterator chain.
///
/// Like `merge_reduce_into_chain` but inserts a `Rev` op before the fold.
fn merge_reduce_right_into_chain(
    source: Box<RustExpr>,
    mut ops: Vec<IteratorOp>,
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
        ops.push(IteratorOp::Rev);
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

// ---------------------------------------------------------------
// Promise utility methods
// ---------------------------------------------------------------

/// Lower `Promise.resolve(value)` to `async { value }`.
///
/// When used without `await`, creates an immediately-resolved future.
/// The `await` handler in `expr_lower.rs` optimizes `await Promise.resolve(x)` to just `x`.
fn lower_promise_resolve(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let value = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::Ident("()".into())));
    RustExpr::new(RustExprKind::PromiseResolve(Box::new(value)), span)
}

/// Lower `Promise.reject(msg)` to `async { panic!("rejected: {}", msg) }`.
///
/// When used without `await`, creates an immediately-rejected future.
/// The `await` handler in `expr_lower.rs` optimizes `await Promise.reject(msg)`
/// to `panic!("rejected: {}", msg)`.
fn lower_promise_reject(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let msg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit("rejected".into())));
    RustExpr::new(RustExprKind::PromiseReject(Box::new(msg)), span)
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

/// Lower `Date.parse(dateString)` to epoch milliseconds as `i64`.
///
/// Parses ISO 8601 strings: `"YYYY-MM-DD"`, `"YYYY-MM-DDTHH:MM:SS"`,
/// `"YYYY-MM-DDTHH:MM:SSZ"`, `"YYYY-MM-DDTHH:MM:SS.mmmZ"`.
///
/// Emits a raw block that splits and parses the components, then converts
/// to milliseconds since the Unix epoch using a civil-to-days algorithm.
fn lower_date_parse(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg_code = args
        .first()
        .map(|a| emit_expr_raw(a))
        .unwrap_or_else(|| r#""""#.to_owned());
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ \
            let __ds: &str = &{arg_code}; \
            let __ds = __ds.trim_end_matches('Z'); \
            let (__date_part, __time_part) = if let Some(__i) = __ds.find('T') {{ \
                (__ds[..__i].to_string(), __ds[__i + 1..].to_string()) \
            }} else {{ \
                (__ds.to_string(), String::new()) \
            }}; \
            let __dp: Vec<&str> = __date_part.split('-').collect(); \
            let __yr: i64 = __dp[0].parse().unwrap_or(1970); \
            let __mo: i64 = if __dp.len() > 1 {{ __dp[1].parse().unwrap_or(1) }} else {{ 1 }}; \
            let __dy: i64 = if __dp.len() > 2 {{ __dp[2].parse().unwrap_or(1) }} else {{ 1 }}; \
            let (__hr, __mi, __sc, __ms): (i64, i64, i64, i64) = if !__time_part.is_empty() {{ \
                let (__main, __frac) = if let Some(__dot) = __time_part.find('.') {{ \
                    (__time_part[..__dot].to_string(), __time_part[__dot + 1..].to_string()) \
                }} else {{ \
                    (__time_part.clone(), String::new()) \
                }}; \
                let __tp: Vec<&str> = __main.split(':').collect(); \
                let __h: i64 = __tp.first().and_then(|s| s.parse().ok()).unwrap_or(0); \
                let __m: i64 = __tp.get(1).and_then(|s| s.parse().ok()).unwrap_or(0); \
                let __s: i64 = __tp.get(2).and_then(|s| s.parse().ok()).unwrap_or(0); \
                let __f: i64 = if !__frac.is_empty() {{ \
                    let mut __fv: i64 = __frac[..std::cmp::min(__frac.len(), 3)].parse().unwrap_or(0); \
                    if __frac.len() == 1 {{ __fv *= 100; }} \
                    else if __frac.len() == 2 {{ __fv *= 10; }} \
                    __fv \
                }} else {{ 0 }}; \
                (__h, __m, __s, __f) \
            }} else {{ \
                (0, 0, 0, 0) \
            }}; \
            let __y = __yr - (if __mo <= 2 {{ 1 }} else {{ 0 }}); \
            let __em = if __mo > 2 {{ __mo - 3 }} else {{ __mo + 9 }}; \
            let __era = (if __y >= 0 {{ __y }} else {{ __y - 399 }}) / 400; \
            let __yoe = (__y - __era * 400) as u64; \
            let __doy = (153 * __em as u64 + 2) / 5 + __dy as u64 - 1; \
            let __doe = __yoe * 365 + __yoe / 4 - __yoe / 100 + __doy; \
            let __total_days = __era * 146097 + __doe as i64 - 719468; \
            let __total_secs = __total_days * 86400 + __hr * 3600 + __mi * 60 + __sc; \
            __total_secs * 1000 + __ms \
            }}"
        )),
        span,
    )
}

/// Lower `Date.UTC(year, month, ...)` to epoch milliseconds as `i64`.
///
/// Arguments: `year`, `month` (0-based), optional `day`, `hours`, `minutes`,
/// `seconds`, `milliseconds`. Uses the civil-to-days algorithm.
fn lower_date_utc(args: Vec<RustExpr>, span: Span) -> RustExpr {
    // Extract argument expressions. We need to emit them in-line.
    let yr_code = args
        .first()
        .map(|a| emit_expr_raw(a))
        .unwrap_or_else(|| "1970".to_owned());
    let mo_code = args
        .get(1)
        .map(|a| emit_expr_raw(a))
        .unwrap_or_else(|| "0".to_owned());
    let dy_code = args
        .get(2)
        .map(|a| emit_expr_raw(a))
        .unwrap_or_else(|| "1".to_owned());
    let hr_code = args
        .get(3)
        .map(|a| emit_expr_raw(a))
        .unwrap_or_else(|| "0".to_owned());
    let mi_code = args
        .get(4)
        .map(|a| emit_expr_raw(a))
        .unwrap_or_else(|| "0".to_owned());
    let sc_code = args
        .get(5)
        .map(|a| emit_expr_raw(a))
        .unwrap_or_else(|| "0".to_owned());
    let ms_code = args
        .get(6)
        .map(|a| emit_expr_raw(a))
        .unwrap_or_else(|| "0".to_owned());

    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ \
            let __yr: i64 = {yr_code} as i64; \
            let __mo: i64 = ({mo_code} as i64) + 1; \
            let __dy: i64 = {dy_code} as i64; \
            let __hr: i64 = {hr_code} as i64; \
            let __mi: i64 = {mi_code} as i64; \
            let __sc: i64 = {sc_code} as i64; \
            let __ms: i64 = {ms_code} as i64; \
            let __y = __yr - (if __mo <= 2 {{ 1 }} else {{ 0 }}); \
            let __em = if __mo > 2 {{ __mo - 3 }} else {{ __mo + 9 }}; \
            let __era = (if __y >= 0 {{ __y }} else {{ __y - 399 }}) / 400; \
            let __yoe = (__y - __era * 400) as u64; \
            let __doy = (153 * __em as u64 + 2) / 5 + __dy as u64 - 1; \
            let __doe = __yoe * 365 + __yoe / 4 - __yoe / 100 + __doy; \
            let __total_days = __era * 146097 + __doe as i64 - 719468; \
            let __total_secs = __total_days * 86400 + __hr * 3600 + __mi * 60 + __sc; \
            __total_secs * 1000 + __ms \
            }}"
        )),
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

// ---------------------------------------------------------------------------
// Task 170: Date getter methods — calendar component extraction
// ---------------------------------------------------------------------------

/// Shared helper: emit the Hinnant civil-from-days calendar computation preamble.
///
/// Given a receiver code string, emits the block prefix that computes
/// `__year`, `__m` (1-based month), `__d` (1-based day), `__dow` (0=Sun..6=Sat),
/// and time-of-day components.
fn date_civil_preamble(receiver_code: &str) -> String {
    format!(
        "{{ \
        let __dur = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
        let __total_secs = __dur.as_secs() as i64; \
        let __days = __total_secs / 86400; \
        let __time_of_day = (__total_secs % 86400) as u64; \
        let __z = __days + 719468; \
        let __era = (if __z >= 0 {{ __z }} else {{ __z - 146096 }}) / 146097; \
        let __doe = (__z - __era * 146097) as u64; \
        let __yoe = (__doe - __doe / 1460 + __doe / 36524 - __doe / 146096) / 365; \
        let __y = __yoe as i64 + __era * 400; \
        let __doy = __doe - (365 * __yoe + __yoe / 4 - __yoe / 100); \
        let __mp = (5 * __doy + 2) / 153; \
        let __d = __doy - (153 * __mp + 2) / 5 + 1; \
        let __m = if __mp < 10 {{ __mp + 3 }} else {{ __mp - 9 }}; \
        let __year = if __m <= 2 {{ __y + 1 }} else {{ __y }}; \
        let __dow = ((__days % 7 + 4) % 7 + 7) % 7; "
    )
}

/// Lower `.getFullYear()` on a `Date` instance to the 4-digit year (e.g. 2026).
///
/// Uses the Hinnant civil_from_days algorithm to extract the year from a `SystemTime`.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_full_year(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let preamble = date_civil_preamble(&receiver_code);
    RustExpr::new(
        RustExprKind::Raw(format!("{preamble}__year as i64 }}")),
        span,
    )
}

/// Lower `.getMonth()` on a `Date` instance to 0-based month (0=Jan, 11=Dec).
///
/// Uses the Hinnant civil_from_days algorithm.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_month(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let preamble = date_civil_preamble(&receiver_code);
    RustExpr::new(
        RustExprKind::Raw(format!("{preamble}(__m as i64) - 1 }}")),
        span,
    )
}

/// Lower `.getDate()` on a `Date` instance to day of month (1-31).
///
/// Uses the Hinnant civil_from_days algorithm.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_date(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let preamble = date_civil_preamble(&receiver_code);
    RustExpr::new(RustExprKind::Raw(format!("{preamble}__d as i64 }}")), span)
}

/// Lower `.getDay()` on a `Date` instance to day of week (0=Sun, 6=Sat).
///
/// Unix epoch (1970-01-01) was a Thursday (day 4). We compute
/// `(days_since_epoch % 7 + 4) % 7` to get 0=Sun..6=Sat.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_day(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let preamble = date_civil_preamble(&receiver_code);
    RustExpr::new(
        RustExprKind::Raw(format!("{preamble}__dow as i64 }}")),
        span,
    )
}

/// Lower `.getHours()` on a `Date` instance to hours (0-23).
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_hours(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ \
            let __dur = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
            let __secs = __dur.as_secs(); \
            let __time_of_day = __secs % 86400; \
            (__time_of_day / 3600) as i64 }}"
        )),
        span,
    )
}

/// Lower `.getMinutes()` on a `Date` instance to minutes (0-59).
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_minutes(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ \
            let __dur = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
            let __secs = __dur.as_secs(); \
            let __time_of_day = __secs % 86400; \
            ((__time_of_day % 3600) / 60) as i64 }}"
        )),
        span,
    )
}

/// Lower `.getSeconds()` on a `Date` instance to seconds (0-59).
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_seconds(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ \
            let __dur = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
            let __secs = __dur.as_secs(); \
            (__secs % 60) as i64 }}"
        )),
        span,
    )
}

/// Lower `.getMilliseconds()` on a `Date` instance to milliseconds (0-999).
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_milliseconds(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ \
            let __dur = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
            __dur.subsec_millis() as i64 }}"
        )),
        span,
    )
}

/// Lower `.getTimezoneOffset()` on a `Date` instance to 0 (UTC).
///
/// RustScript operates in UTC only; the offset is always 0 minutes.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_get_timezone_offset(
    _receiver: RustExpr,
    _args: Vec<RustExpr>,
    span: Span,
) -> RustExpr {
    RustExpr::new(RustExprKind::Raw("0i64".to_owned()), span)
}

// ---------------------------------------------------------------------------
// Task 171: Date setter lowering functions
// ---------------------------------------------------------------------------

/// Lower `.setTime(ms)` — reconstruct `SystemTime` from epoch milliseconds.
///
/// Emits: `std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64)`
fn lower_date_set_time(_receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .first()
        .map_or_else(|| "0".to_owned(), |a| emit_expr_raw(a));
    RustExpr::new(
        RustExprKind::Raw(format!(
            "std::time::UNIX_EPOCH + std::time::Duration::from_millis({arg} as u64)"
        )),
        span,
    )
}

/// Shared helper: decompose a `SystemTime` receiver into calendar components,
/// replace one component, and reconstruct via the civil-to-days algorithm.
///
/// `component` is which variable to override (`__year`, `__month`, `__day`,
/// `__hours`, `__minutes`, `__seconds`, `__millis`).
/// `replacement` is the Rust expression string to assign to that variable.
fn date_setter_raw(receiver_code: &str, component: &str, replacement: &str) -> String {
    // Decomposition: extract all calendar components from SystemTime
    let decompose = format!(
        "let __dur = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
        let __total_secs = __dur.as_secs() as i64; \
        let __millis_part = __dur.subsec_millis() as i64; \
        let __day_secs = __total_secs % 86400; \
        let __civil_days = (__total_secs - __day_secs) / 86400; \
        let mut __hours = __day_secs / 3600; \
        let mut __minutes = (__day_secs % 3600) / 60; \
        let mut __seconds = __day_secs % 60; \
        let mut __millis = __millis_part; \
        let __z = __civil_days + 719468; \
        let __era = (if __z >= 0 {{ __z }} else {{ __z - 146096 }}) / 146097; \
        let __doe = (__z - __era * 146097) as u64; \
        let __yoe = (__doe - __doe / 1460 + __doe / 36524 - __doe / 146096) / 365; \
        let mut __year = __yoe as i64 + __era * 400; \
        let __doy = __doe - (365 * __yoe + __yoe / 4 - __yoe / 100); \
        let __mp = (5 * __doy + 2) / 153; \
        let mut __day = (__doy - (153 * __mp + 2) / 5 + 1) as i64; \
        let mut __month = (if __mp < 10 {{ __mp + 3 }} else {{ __mp - 9 }}) as i64; \
        if __month <= 2 {{ __year += 1; }}"
    );

    // Reconstruction: civil_to_days inverse algorithm
    let reconstruct = "\
        let __rm = if __month <= 2 { __month + 9 } else { __month - 3 }; \
        let __ry = if __month <= 2 { __year - 1 } else { __year }; \
        let __rera = (if __ry >= 0 { __ry } else { __ry - 399 }) / 400; \
        let __ryoe = (__ry - __rera * 400) as u64; \
        let __rdoy = (153 * __rm as u64 + 2) / 5 + __day as u64 - 1; \
        let __rdoe = __ryoe * 365 + __ryoe / 4 - __ryoe / 100 + __rdoy; \
        let __rdays = __rera * 146097 + __rdoe as i64 - 719468; \
        let __new_secs = __rdays * 86400 + __hours * 3600 + __minutes * 60 + __seconds; \
        std::time::UNIX_EPOCH + std::time::Duration::from_millis((__new_secs * 1000 + __millis) as u64)";

    // Override the target component
    let override_line = [component, " = ", replacement, ";"].concat();

    // Use string concatenation (not format!) to avoid brace-escaping issues
    // in the `reconstruct` block which contains Rust if-else braces.
    [
        "{ ",
        &decompose,
        " ",
        &override_line,
        " ",
        reconstruct,
        " }",
    ]
    .concat()
}

/// Lower `.setFullYear(year)` — reconstruct `SystemTime` with a new year.
fn lower_date_set_full_year(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let arg = args
        .first()
        .map_or_else(|| "1970".to_owned(), |a| emit_expr_raw(a));
    RustExpr::new(
        RustExprKind::Raw(date_setter_raw(
            &receiver_code,
            "__year",
            &format!("{arg} as i64"),
        )),
        span,
    )
}

/// Lower `.setMonth(month)` — reconstruct `SystemTime` with a new month.
///
/// JavaScript months are 0-based (0=Jan, 11=Dec), so we add 1 for the
/// 1-based internal representation.
fn lower_date_set_month(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let arg = args
        .first()
        .map_or_else(|| "0".to_owned(), |a| emit_expr_raw(a));
    RustExpr::new(
        RustExprKind::Raw(date_setter_raw(
            &receiver_code,
            "__month",
            &format!("{arg} as i64 + 1"),
        )),
        span,
    )
}

/// Lower `.setDate(day)` — reconstruct `SystemTime` with a new day-of-month.
fn lower_date_set_date(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let arg = args
        .first()
        .map_or_else(|| "1".to_owned(), |a| emit_expr_raw(a));
    RustExpr::new(
        RustExprKind::Raw(date_setter_raw(
            &receiver_code,
            "__day",
            &format!("{arg} as i64"),
        )),
        span,
    )
}

/// Lower `.setHours(hours)` — reconstruct `SystemTime` with new hours.
fn lower_date_set_hours(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let arg = args
        .first()
        .map_or_else(|| "0".to_owned(), |a| emit_expr_raw(a));
    RustExpr::new(
        RustExprKind::Raw(date_setter_raw(
            &receiver_code,
            "__hours",
            &format!("{arg} as i64"),
        )),
        span,
    )
}

/// Lower `.setMinutes(minutes)` — reconstruct `SystemTime` with new minutes.
fn lower_date_set_minutes(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let arg = args
        .first()
        .map_or_else(|| "0".to_owned(), |a| emit_expr_raw(a));
    RustExpr::new(
        RustExprKind::Raw(date_setter_raw(
            &receiver_code,
            "__minutes",
            &format!("{arg} as i64"),
        )),
        span,
    )
}

/// Lower `.setSeconds(seconds)` — reconstruct `SystemTime` with new seconds.
fn lower_date_set_seconds(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let arg = args
        .first()
        .map_or_else(|| "0".to_owned(), |a| emit_expr_raw(a));
    RustExpr::new(
        RustExprKind::Raw(date_setter_raw(
            &receiver_code,
            "__seconds",
            &format!("{arg} as i64"),
        )),
        span,
    )
}

/// Lower `.setMilliseconds(ms)` — reconstruct `SystemTime` with new milliseconds.
fn lower_date_set_milliseconds(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let arg = args
        .first()
        .map_or_else(|| "0".to_owned(), |a| emit_expr_raw(a));
    RustExpr::new(
        RustExprKind::Raw(date_setter_raw(
            &receiver_code,
            "__millis",
            &format!("{arg} as i64"),
        )),
        span,
    )
}

// ---------------------------------------------------------------------------
// Task 172: Date formatting methods
// ---------------------------------------------------------------------------

/// Shared raw block preamble that extracts calendar components from a `SystemTime`.
///
/// Given a receiver code string, returns a Rust code prefix that computes:
/// `__secs`, `__days`, `__hours`, `__minutes`, `__seconds`, `__y` (year),
/// `__m` (0-based month), `__day` (1-based day-of-month), `__dow` (0=Sun..6=Sat).
fn date_calendar_preamble(receiver_code: &str) -> String {
    format!(
        "let __d = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
        let __secs = __d.as_secs(); \
        let __days = __secs / 86400; \
        let __time_of_day = __secs % 86400; \
        let __hours = __time_of_day / 3600; \
        let __minutes = (__time_of_day % 3600) / 60; \
        let __seconds = __time_of_day % 60; \
        let __dow = ((__days + 4) % 7) as usize; \
        let mut __y = 1970i64; \
        let mut __remaining = __days as i64; \
        loop {{ \
            let __diy = if __y % 4 == 0 && (__y % 100 != 0 || __y % 400 == 0) {{ 366 }} else {{ 365 }}; \
            if __remaining < __diy {{ break; }} \
            __remaining -= __diy; \
            __y += 1; \
        }} \
        let __leap = __y % 4 == 0 && (__y % 100 != 0 || __y % 400 == 0); \
        let __mdays: [i64; 12] = [31, if __leap {{ 29 }} else {{ 28 }}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]; \
        let mut __m = 0usize; \
        while __m < 12 && __remaining >= __mdays[__m] {{ \
            __remaining -= __mdays[__m]; \
            __m += 1; \
        }} \
        let __day = __remaining + 1; \
        const __DAYS: &[&str] = &[\"Sun\", \"Mon\", \"Tue\", \"Wed\", \"Thu\", \"Fri\", \"Sat\"]; \
        const __MONTHS: &[&str] = &[\"Jan\", \"Feb\", \"Mar\", \"Apr\", \"May\", \"Jun\", \"Jul\", \"Aug\", \"Sep\", \"Oct\", \"Nov\", \"Dec\"]; "
    )
}

/// Lower `.toDateString()` on a `Date` instance.
///
/// Formats as `"Day Mon DD YYYY"` (e.g., `"Thu Jan 01 1970"`).
#[allow(clippy::needless_pass_by_value)]
fn lower_date_to_date_string(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let preamble = date_calendar_preamble(&receiver_code);
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ {preamble}\
            format!(\"{{}} {{}} {{:02}} {{}}\", __DAYS[__dow], __MONTHS[__m], __day, __y) \
            }}"
        )),
        span,
    )
}

/// Lower `.toTimeString()` on a `Date` instance.
///
/// Formats as `"HH:MM:SS GMT+0000"` (time portion only).
#[allow(clippy::needless_pass_by_value)]
fn lower_date_to_time_string(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ \
            let __d = {receiver_code}.duration_since(std::time::UNIX_EPOCH).unwrap(); \
            let __secs = __d.as_secs(); \
            let __time_of_day = __secs % 86400; \
            let __hours = __time_of_day / 3600; \
            let __minutes = (__time_of_day % 3600) / 60; \
            let __seconds = __time_of_day % 60; \
            format!(\"{{:02}}:{{:02}}:{{:02}} GMT+0000\", __hours, __minutes, __seconds) \
            }}"
        )),
        span,
    )
}

/// Lower `.toUTCString()` on a `Date` instance.
///
/// Formats as `"Day, DD Mon YYYY HH:MM:SS GMT"` (RFC 7231).
#[allow(clippy::needless_pass_by_value)]
fn lower_date_to_utc_string(receiver: RustExpr, _args: Vec<RustExpr>, span: Span) -> RustExpr {
    let receiver_code = emit_receiver(&receiver);
    let preamble = date_calendar_preamble(&receiver_code);
    RustExpr::new(
        RustExprKind::Raw(format!(
            "{{ {preamble}\
            format!(\"{{}}, {{:02}} {{}} {{}} {{:02}}:{{:02}}:{{:02}} GMT\", \
            __DAYS[__dow], __day, __MONTHS[__m], __y, __hours, __minutes, __seconds) \
            }}"
        )),
        span,
    )
}

/// Lower `.toJSON()` on a `Date` instance — delegates to `.toISOString()`.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_to_json(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    lower_date_to_iso_string(receiver, args, span)
}

/// Lower `.toLocaleDateString()` on a `Date` instance.
///
/// MVP: locale-independent, same as `.toDateString()`.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_to_locale_date_string(
    receiver: RustExpr,
    args: Vec<RustExpr>,
    span: Span,
) -> RustExpr {
    lower_date_to_date_string(receiver, args, span)
}

/// Lower `.toLocaleString()` on a `Date` instance.
///
/// MVP: locale-independent, same as `.toString()`.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_to_locale_string(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    lower_date_to_string(receiver, args, span)
}

/// Lower `.toLocaleTimeString()` on a `Date` instance.
///
/// MVP: locale-independent, same as `.toTimeString()`.
#[allow(clippy::needless_pass_by_value)]
fn lower_date_to_locale_time_string(
    receiver: RustExpr,
    args: Vec<RustExpr>,
    span: Span,
) -> RustExpr {
    lower_date_to_time_string(receiver, args, span)
}

/// Lower `.valueOf()` on a `Date` instance — same as `.getTime()` (millis since epoch).
#[allow(clippy::needless_pass_by_value)]
fn lower_date_value_of(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    lower_date_get_time(receiver, args, span)
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

// ---------------------------------------------------------------------------
// Task 160: Number instance method lowering functions
// ---------------------------------------------------------------------------

/// Check whether a type represents a numeric value.
///
/// Used for type-aware dispatch of Number instance methods like `.toFixed()`.
pub(crate) fn is_number_type(ty: &RustType) -> bool {
    matches!(
        ty,
        RustType::I8
            | RustType::I16
            | RustType::I32
            | RustType::I64
            | RustType::U8
            | RustType::U16
            | RustType::U32
            | RustType::U64
            | RustType::F32
            | RustType::F64
    )
}

/// Lower `.toFixed(digits)` on a number to `format!("{:.prec$}", num, prec = digits as usize)`.
///
/// Emits: `format!("{:.prec$}", receiver, prec = digits as usize)`
/// If no argument is provided, defaults to 0 decimal places.
fn lower_number_to_fixed(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let digits = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(0)));
    let cast_digits = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(digits),
        RustType::Named("usize".into()),
    ));
    RustExpr::new(
        RustExprKind::Macro {
            name: "format".into(),
            args: vec![
                RustExpr::synthetic(RustExprKind::StringLit("{:.prec$}".into())),
                receiver,
                RustExpr::synthetic(RustExprKind::Ident(format!(
                    "prec = {}",
                    emit_inline(&cast_digits)
                ))),
            ],
        },
        span,
    )
}

/// Lower `.toExponential(digits)` on a number to `format!("{:.prec$e}", num, prec = digits as usize)`.
///
/// Emits: `format!("{:.prec$e}", receiver, prec = digits as usize)`
/// If no argument is provided, defaults to 6 decimal places.
fn lower_number_to_exponential(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let digits = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::IntLit(6)));
    let cast_digits = RustExpr::synthetic(RustExprKind::Cast(
        Box::new(digits),
        RustType::Named("usize".into()),
    ));
    RustExpr::new(
        RustExprKind::Macro {
            name: "format".into(),
            args: vec![
                RustExpr::synthetic(RustExprKind::StringLit("{:.prec$e}".into())),
                receiver,
                RustExpr::synthetic(RustExprKind::Ident(format!(
                    "prec = {}",
                    emit_inline(&cast_digits)
                ))),
            ],
        },
        span,
    )
}

/// Lower `.toPrecision(digits)` on a number to `format!("{:.prec$e}", num, prec = digits as usize)`.
///
/// Emits: `format!("{:.prec$e}", receiver, prec = digits as usize)`
/// If no argument is provided, falls back to `receiver.to_string()`.
fn lower_number_to_precision(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut args_iter = args.into_iter();
    if let Some(digits) = args_iter.next() {
        let cast_digits = RustExpr::synthetic(RustExprKind::Cast(
            Box::new(digits),
            RustType::Named("usize".into()),
        ));
        RustExpr::new(
            RustExprKind::Macro {
                name: "format".into(),
                args: vec![
                    RustExpr::synthetic(RustExprKind::StringLit("{:.prec$e}".into())),
                    receiver,
                    RustExpr::synthetic(RustExprKind::Ident(format!(
                        "prec = {}",
                        emit_inline(&cast_digits)
                    ))),
                ],
            },
            span,
        )
    } else {
        // No argument: just convert to string
        RustExpr::new(RustExprKind::ToString(Box::new(receiver)), span)
    }
}

/// Lower `.toString(radix?)` on a number.
///
/// Without radix: `receiver.to_string()`
/// With radix 16: `format!("{:x}", receiver)`
/// With radix 8: `format!("{:o}", receiver)`
/// With radix 2: `format!("{:b}", receiver)`
/// With other radix: falls back to `format!("{}", receiver)` (Rust lacks
/// arbitrary-radix formatting in `format!`, so we support the common cases).
fn lower_number_to_string(receiver: RustExpr, args: Vec<RustExpr>, span: Span) -> RustExpr {
    let mut args_iter = args.into_iter();
    if let Some(radix_arg) = args_iter.next() {
        // Check for literal radix values to pick the right format specifier
        let fmt_spec = match &radix_arg.kind {
            RustExprKind::IntLit(2) => Some("{:b}"),
            RustExprKind::IntLit(8) => Some("{:o}"),
            RustExprKind::IntLit(16) => Some("{:x}"),
            RustExprKind::IntLit(10) => Some("{}"),
            _ => None,
        };

        if let Some(fmt) = fmt_spec {
            RustExpr::new(
                RustExprKind::Macro {
                    name: "format".into(),
                    args: vec![
                        RustExpr::synthetic(RustExprKind::StringLit(fmt.to_owned())),
                        receiver,
                    ],
                },
                span,
            )
        } else {
            // Non-literal or unsupported radix: fall back to decimal
            RustExpr::new(
                RustExprKind::Macro {
                    name: "format".into(),
                    args: vec![
                        RustExpr::synthetic(RustExprKind::StringLit("{}".to_owned())),
                        receiver,
                    ],
                },
                span,
            )
        }
    } else {
        // No radix: just .to_string()
        RustExpr::new(RustExprKind::ToString(Box::new(receiver)), span)
    }
}

/// Check whether a type represents a `Date` (`SystemTime`) value.
///
/// Used for type-aware dispatch of Date instance methods like `.getTime()`.
pub(crate) fn is_date_type(ty: &RustType) -> bool {
    matches!(ty, RustType::Named(n) if n == "Date" || n == "SystemTime" || n == "std::time::SystemTime")
}

// ---------------------------------------------------------------------------
// Task 175: Encoding/decoding global functions
// ---------------------------------------------------------------------------

/// Emit a simple expression as a Rust code string for use in raw expression blocks.
fn emit_expr_raw(expr: &RustExpr) -> String {
    match &expr.kind {
        RustExprKind::Ident(name) => name.clone(),
        RustExprKind::StringLit(s) => format!(r#""{s}""#),
        RustExprKind::IntLit(n) => n.to_string(),
        RustExprKind::FloatLit(f) => format!("{f}"),
        RustExprKind::FieldAccess { object, field } => {
            format!("{}.{}", emit_expr_raw(object), field)
        }
        RustExprKind::ToString(inner) => {
            // String literals get wrapped in .to_string() during lowering.
            // For raw block embedding, emit the inner string literal directly
            // (since we typically take &str anyway, the .to_string() is redundant).
            let inner_code = emit_expr_raw(inner);
            format!("{inner_code}.to_string()")
        }
        RustExprKind::Raw(code) => code.clone(),
        _ => "__enc_in".to_owned(),
    }
}

/// Public wrapper around `emit_expr_raw` for use in `expr_lower.rs`.
///
/// Emits a `RustExpr` as inline Rust source text for embedding in raw blocks.
pub(crate) fn emit_expr_raw_pub(expr: &RustExpr) -> String {
    emit_expr_raw(expr)
}

/// Lower `btoa(str)` to an inline base64 encoding block.
///
/// Emits a self-contained Rust block that base64-encodes the input string using
/// the standard alphabet. No external crate dependencies.
///
/// JavaScript: `btoa("hello")` -> `"aGVsbG8="`
fn lower_btoa(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let arg_code = emit_expr_raw(&arg);
    RustExpr::new(
        RustExprKind::Raw(format!(
            r#"{{
    let __b64_in: &str = &({arg_code});
    let __b64_bytes = __b64_in.as_bytes();
    const __B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut __b64_out = String::with_capacity((__b64_bytes.len() + 2) / 3 * 4);
    let mut __b64_i = 0usize;
    while __b64_i + 2 < __b64_bytes.len() {{
        let b0 = __b64_bytes[__b64_i] as usize;
        let b1 = __b64_bytes[__b64_i + 1] as usize;
        let b2 = __b64_bytes[__b64_i + 2] as usize;
        __b64_out.push(__B64_CHARS[b0 >> 2] as char);
        __b64_out.push(__B64_CHARS[((b0 & 3) << 4) | (b1 >> 4)] as char);
        __b64_out.push(__B64_CHARS[((b1 & 15) << 2) | (b2 >> 6)] as char);
        __b64_out.push(__B64_CHARS[b2 & 63] as char);
        __b64_i += 3;
    }}
    let __b64_rem = __b64_bytes.len() - __b64_i;
    if __b64_rem == 1 {{
        let b0 = __b64_bytes[__b64_i] as usize;
        __b64_out.push(__B64_CHARS[b0 >> 2] as char);
        __b64_out.push(__B64_CHARS[(b0 & 3) << 4] as char);
        __b64_out.push('=');
        __b64_out.push('=');
    }} else if __b64_rem == 2 {{
        let b0 = __b64_bytes[__b64_i] as usize;
        let b1 = __b64_bytes[__b64_i + 1] as usize;
        __b64_out.push(__B64_CHARS[b0 >> 2] as char);
        __b64_out.push(__B64_CHARS[((b0 & 3) << 4) | (b1 >> 4)] as char);
        __b64_out.push(__B64_CHARS[(b1 & 15) << 2] as char);
        __b64_out.push('=');
    }}
    __b64_out
}}"#
        )),
        span,
    )
}

/// Lower `atob(str)` to an inline base64 decoding block.
///
/// Emits a self-contained Rust block that base64-decodes the input string.
/// No external crate dependencies. Returns an empty string on invalid input.
///
/// JavaScript: `atob("aGVsbG8=")` -> `"hello"`
fn lower_atob(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let arg_code = emit_expr_raw(&arg);
    RustExpr::new(
        RustExprKind::Raw(format!(
            r#"{{
    let __b64d_in: &str = &({arg_code});
    let __b64d_bytes = __b64d_in.as_bytes();
    let b64d_val = |c: u8| -> u8 {{
        match c {{
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }}
    }};
    let mut __b64d_out: Vec<u8> = Vec::with_capacity(__b64d_bytes.len() / 4 * 3);
    let mut __b64d_i = 0usize;
    while __b64d_i + 3 < __b64d_bytes.len() {{
        let v0 = b64d_val(__b64d_bytes[__b64d_i]) as u32;
        let v1 = b64d_val(__b64d_bytes[__b64d_i + 1]) as u32;
        let v2 = b64d_val(__b64d_bytes[__b64d_i + 2]) as u32;
        let v3 = b64d_val(__b64d_bytes[__b64d_i + 3]) as u32;
        __b64d_out.push(((v0 << 2) | (v1 >> 4)) as u8);
        if __b64d_bytes[__b64d_i + 2] != b'=' {{
            __b64d_out.push(((v1 << 4) | (v2 >> 2)) as u8);
        }}
        if __b64d_bytes[__b64d_i + 3] != b'=' {{
            __b64d_out.push(((v2 << 6) | v3) as u8);
        }}
        __b64d_i += 4;
    }}
    String::from_utf8(__b64d_out).unwrap_or_default()
}}"#
        )),
        span,
    )
}

/// Lower `encodeURIComponent(str)` to an inline percent-encoding block.
///
/// Preserves `A-Z a-z 0-9 - _ . ! ~ * ' ( )` unchanged; percent-encodes everything else.
/// Matches the JavaScript `encodeURIComponent` specification.
///
/// JavaScript: `encodeURIComponent("hello world")` -> `"hello%20world"`
fn lower_encode_uri_component(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let arg_code = emit_expr_raw(&arg);
    RustExpr::new(
        RustExprKind::Raw(format!(
            r#"{{
    let __enc_in: &str = &({arg_code});
    let mut __enc_out = String::with_capacity(__enc_in.len());
    for __enc_b in __enc_in.bytes() {{
        match __enc_b {{
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {{
                __enc_out.push(__enc_b as char);
            }}
            _ => {{
                __enc_out.push_str(&format!("%{{:02X}}", __enc_b));
            }}
        }}
    }}
    __enc_out
}}"#
        )),
        span,
    )
}

/// Lower `decodeURIComponent(str)` to an inline percent-decoding block.
///
/// Decodes `%XX` sequences back to their UTF-8 byte values.
/// Matches the JavaScript `decodeURIComponent` specification.
fn lower_decode_uri_component(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let arg_code = emit_expr_raw(&arg);
    RustExpr::new(
        RustExprKind::Raw(format!(
            r#"{{
    let __dec_in: &str = &({arg_code});
    let mut __dec_bytes: Vec<u8> = Vec::with_capacity(__dec_in.len());
    let mut __dec_chars = __dec_in.chars().peekable();
    while let Some(__dec_c) = __dec_chars.next() {{
        if __dec_c == '%' {{
            let h1 = __dec_chars.next().unwrap_or('0');
            let h2 = __dec_chars.next().unwrap_or('0');
            let __dec_hex = format!("{{}}{{}}", h1, h2);
            let __dec_byte = u8::from_str_radix(&__dec_hex, 16).unwrap_or(0);
            __dec_bytes.push(__dec_byte);
        }} else {{
            let mut __dec_buf = [0u8; 4];
            for &b in __dec_c.encode_utf8(&mut __dec_buf).as_bytes() {{
                __dec_bytes.push(b);
            }}
        }}
    }}
    String::from_utf8(__dec_bytes).unwrap_or_default()
}}"#
        )),
        span,
    )
}

/// Lower `encodeURI(str)` to an inline percent-encoding block.
///
/// Preserves URI structural characters (`:/?#[]@!$&'()*+,;=`) in addition to
/// unreserved characters. Matches the JavaScript `encodeURI` specification.
fn lower_encode_uri(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let arg_code = emit_expr_raw(&arg);
    RustExpr::new(
        RustExprKind::Raw(format!(
            r#"{{
    let __euri_in: &str = &({arg_code});
    let mut __euri_out = String::with_capacity(__euri_in.len());
    for __euri_b in __euri_in.bytes() {{
        match __euri_b {{
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~'
            | b':' | b'/' | b'?' | b'#' | b'[' | b']' | b'@'
            | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*'
            | b'+' | b',' | b';' | b'=' => {{
                __euri_out.push(__euri_b as char);
            }}
            _ => {{
                __euri_out.push_str(&format!("%{{:02X}}", __euri_b));
            }}
        }}
    }}
    __euri_out
}}"#
        )),
        span,
    )
}

/// Lower `decodeURI(str)` to an inline percent-decoding block.
///
/// Decodes `%XX` sequences but preserves sequences that represent URI structural
/// characters (`:/?#[]@!$&'()*+,;=`). Matches the JavaScript `decodeURI` spec.
fn lower_decode_uri(args: Vec<RustExpr>, span: Span) -> RustExpr {
    let arg = args
        .into_iter()
        .next()
        .unwrap_or_else(|| RustExpr::synthetic(RustExprKind::StringLit(String::new())));
    let arg_code = emit_expr_raw(&arg);
    RustExpr::new(
        RustExprKind::Raw(format!(
            r#"{{
    let __duri_in: &str = &({arg_code});
    let mut __duri_bytes: Vec<u8> = Vec::with_capacity(__duri_in.len());
    const __DURI_RESERVED: &[u8] = b":/?#[]@!$&'()*+,;=";
    let mut __duri_chars = __duri_in.chars().peekable();
    while let Some(__duri_c) = __duri_chars.next() {{
        if __duri_c == '%' {{
            let h1 = __duri_chars.next().unwrap_or('0');
            let h2 = __duri_chars.next().unwrap_or('0');
            let __duri_hex = format!("{{}}{{}}", h1, h2);
            let __duri_byte = u8::from_str_radix(&__duri_hex, 16).unwrap_or(0);
            if __DURI_RESERVED.contains(&__duri_byte) {{
                __duri_bytes.push(b'%');
                for b in __duri_hex.bytes() {{
                    __duri_bytes.push(b);
                }}
            }} else {{
                __duri_bytes.push(__duri_byte);
            }}
        }} else {{
            let mut __duri_buf = [0u8; 4];
            for &b in __duri_c.encode_utf8(&mut __duri_buf).as_bytes() {{
                __duri_bytes.push(b);
            }}
        }}
    }}
    String::from_utf8(__duri_bytes).unwrap_or_default()
}}"#
        )),
        span,
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
            // Regex-argument string methods
            "match",
            "search",
            // Array mutating methods (registered as string methods)
            "push",
            "pop",
            "shift",
            "unshift",
            "reverse",
            "sort",
            "join",
            "fill",
            "splice",
            "copyWithin",
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
            "get", "set", "has", "delete", "clear", "keys", "values", "entries", "add", "forEach",
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
            "reduceRight",
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
    // Task 136: Timer function tests
    // ---------------------------------------------------------------

    // Test: lookup_function("setTimeout") returns Some
    #[test]
    fn test_builtin_registry_lookup_function_set_timeout_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("setTimeout").is_some(),
            "setTimeout should be registered as a builtin free function"
        );
    }

    // Test: lookup_function("setInterval") returns Some
    #[test]
    fn test_builtin_registry_lookup_function_set_interval_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("setInterval").is_some(),
            "setInterval should be registered as a builtin free function"
        );
    }

    // Test: lookup_function("clearTimeout") returns Some
    #[test]
    fn test_builtin_registry_lookup_function_clear_timeout_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("clearTimeout").is_some(),
            "clearTimeout should be registered as a builtin free function"
        );
    }

    // Test: lookup_function("clearInterval") returns Some
    #[test]
    fn test_builtin_registry_lookup_function_clear_interval_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("clearInterval").is_some(),
            "clearInterval should be registered as a builtin free function"
        );
    }

    // Test: lower_set_timeout produces tokio::spawn(async move { sleep.await; body })
    #[test]
    fn test_lower_set_timeout_produces_tokio_spawn_with_sleep() {
        let callback_body = RustExpr::synthetic(RustExprKind::Call {
            func: "work".into(),
            args: vec![],
        });
        let callback = RustExpr::synthetic(RustExprKind::Closure {
            is_async: false,
            is_move: false,
            params: vec![],
            return_type: None,
            body: RustClosureBody::Block(RustBlock {
                stmts: vec![RustStmt::Semi(callback_body)],
                expr: None,
            }),
        });
        let delay = RustExpr::synthetic(RustExprKind::IntLit(1000));

        let result = lower_set_timeout(vec![callback, delay], span());
        match &result.kind {
            RustExprKind::Call { func, args } => {
                assert_eq!(func, "tokio::spawn");
                assert_eq!(args.len(), 1);
                match &args[0].kind {
                    RustExprKind::AsyncBlock { is_move, body } => {
                        assert!(is_move, "setTimeout should produce async move block");
                        // First stmt: sleep.await, second: the callback body
                        assert_eq!(
                            body.stmts.len(),
                            2,
                            "expected 2 stmts (sleep + callback body), got {}",
                            body.stmts.len()
                        );
                        // Verify first statement is the sleep await
                        match &body.stmts[0] {
                            RustStmt::Semi(expr) => {
                                assert!(
                                    matches!(&expr.kind, RustExprKind::Await(_)),
                                    "first stmt should be Await(sleep), got {:?}",
                                    expr.kind
                                );
                            }
                            other => panic!("expected Semi(Await), got {other:?}"),
                        }
                        // Verify second statement is the callback call
                        match &body.stmts[1] {
                            RustStmt::Semi(expr) => {
                                assert!(
                                    matches!(&expr.kind, RustExprKind::Call { func, .. } if func == "work"),
                                    "second stmt should be work() call, got {:?}",
                                    expr.kind
                                );
                            }
                            other => panic!("expected Semi(Call), got {other:?}"),
                        }
                    }
                    other => panic!("expected AsyncBlock, got {other:?}"),
                }
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    // Test: lower_set_interval produces tokio::spawn(async move { loop { sleep.await; body } })
    #[test]
    fn test_lower_set_interval_produces_tokio_spawn_with_loop() {
        let callback_body = RustExpr::synthetic(RustExprKind::Call {
            func: "tick".into(),
            args: vec![],
        });
        let callback = RustExpr::synthetic(RustExprKind::Closure {
            is_async: false,
            is_move: false,
            params: vec![],
            return_type: None,
            body: RustClosureBody::Block(RustBlock {
                stmts: vec![RustStmt::Semi(callback_body)],
                expr: None,
            }),
        });
        let delay = RustExpr::synthetic(RustExprKind::IntLit(500));

        let result = lower_set_interval(vec![callback, delay], span());
        match &result.kind {
            RustExprKind::Call { func, args } => {
                assert_eq!(func, "tokio::spawn");
                assert_eq!(args.len(), 1);
                match &args[0].kind {
                    RustExprKind::AsyncBlock { is_move, body } => {
                        assert!(is_move, "setInterval should produce async move block");
                        // Single stmt: a Loop
                        assert_eq!(
                            body.stmts.len(),
                            1,
                            "expected 1 stmt (loop), got {}",
                            body.stmts.len()
                        );
                        match &body.stmts[0] {
                            RustStmt::Loop(loop_stmt) => {
                                assert!(loop_stmt.label.is_none());
                                // Loop body: sleep + callback
                                assert_eq!(
                                    loop_stmt.body.stmts.len(),
                                    2,
                                    "loop body should have 2 stmts (sleep + callback)"
                                );
                                // Verify sleep await
                                match &loop_stmt.body.stmts[0] {
                                    RustStmt::Semi(expr) => {
                                        assert!(
                                            matches!(&expr.kind, RustExprKind::Await(_)),
                                            "first loop stmt should be Await(sleep)"
                                        );
                                    }
                                    other => panic!("expected Semi(Await), got {other:?}"),
                                }
                                // Verify callback call
                                match &loop_stmt.body.stmts[1] {
                                    RustStmt::Semi(expr) => {
                                        assert!(
                                            matches!(&expr.kind, RustExprKind::Call { func, .. } if func == "tick"),
                                            "second loop stmt should be tick() call"
                                        );
                                    }
                                    other => panic!("expected Semi(Call), got {other:?}"),
                                }
                            }
                            other => panic!("expected Loop, got {other:?}"),
                        }
                    }
                    other => panic!("expected AsyncBlock, got {other:?}"),
                }
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    // Test: lower_clear_timeout produces handle.abort()
    #[test]
    fn test_lower_clear_timeout_produces_abort() {
        let handle = RustExpr::synthetic(RustExprKind::Ident("timer_handle".into()));
        let result = lower_clear_timeout(vec![handle], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                assert_eq!(method, "abort");
                assert!(args.is_empty());
                assert!(
                    matches!(&receiver.kind, RustExprKind::Ident(name) if name == "timer_handle"),
                    "receiver should be the handle ident"
                );
            }
            other => panic!("expected MethodCall(abort), got {other:?}"),
        }
    }

    // Test: lower_clear_interval produces handle.abort()
    #[test]
    fn test_lower_clear_interval_produces_abort() {
        let handle = RustExpr::synthetic(RustExprKind::Ident("interval_handle".into()));
        let result = lower_clear_interval(vec![handle], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                assert_eq!(method, "abort");
                assert!(args.is_empty());
                assert!(
                    matches!(&receiver.kind, RustExprKind::Ident(name) if name == "interval_handle"),
                    "receiver should be the handle ident"
                );
            }
            other => panic!("expected MethodCall(abort), got {other:?}"),
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
    // Task 162: String .match() and .search() tests
    // ---------------------------------------------------------------

    fn regex_arg() -> RustExpr {
        RustExpr::new(RustExprKind::Ident("re".to_owned()), span())
    }

    #[test]
    fn test_lower_string_match_produces_find_iter_map_collect() {
        let result = lower_string_match(string_receiver(), vec![regex_arg()], span());
        // Outermost: .collect::<Vec<String>>()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver: collect_recv,
                method,
                type_args,
                ..
            } => {
                assert_eq!(method, "collect");
                assert_eq!(type_args.len(), 1);
                // Inner: .map(|m| m.as_str().to_string())
                match &collect_recv.kind {
                    RustExprKind::MethodCall {
                        receiver: map_recv,
                        method: map_method,
                        args: map_args,
                        ..
                    } => {
                        assert_eq!(map_method, "map");
                        assert_eq!(map_args.len(), 1);
                        // Innermost: regex.find_iter(&str)
                        match &map_recv.kind {
                            RustExprKind::MethodCall {
                                receiver: find_recv,
                                method: find_method,
                                args: find_args,
                                ..
                            } => {
                                assert_eq!(find_method, "find_iter");
                                // Receiver is the regex
                                assert!(
                                    matches!(&find_recv.kind, RustExprKind::Ident(name) if name == "re")
                                );
                                // Arg is &str (borrowed string)
                                assert_eq!(find_args.len(), 1);
                                assert!(matches!(&find_args[0].kind, RustExprKind::Borrow(_)));
                            }
                            other => panic!("expected MethodCall(find_iter), got {other:?}"),
                        }
                    }
                    other => panic!("expected MethodCall(map), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_search_produces_find_map_unwrap_or() {
        let result = lower_string_search(string_receiver(), vec![regex_arg()], span());
        // Outermost: .unwrap_or(-1)
        match &result.kind {
            RustExprKind::MethodCall {
                receiver: unwrap_recv,
                method,
                args,
                ..
            } => {
                assert_eq!(method, "unwrap_or");
                assert_eq!(args.len(), 1);
                // The -1 arg
                assert!(matches!(
                    &args[0].kind,
                    RustExprKind::Unary {
                        op: RustUnaryOp::Neg,
                        ..
                    }
                ));
                // Inner: .map(|m| m.start() as i64)
                match &unwrap_recv.kind {
                    RustExprKind::MethodCall {
                        receiver: map_recv,
                        method: map_method,
                        args: map_args,
                        ..
                    } => {
                        assert_eq!(map_method, "map");
                        assert_eq!(map_args.len(), 1);
                        // Innermost: regex.find(&str)
                        match &map_recv.kind {
                            RustExprKind::MethodCall {
                                receiver: find_recv,
                                method: find_method,
                                args: find_args,
                                ..
                            } => {
                                assert_eq!(find_method, "find");
                                // Receiver is the regex
                                assert!(
                                    matches!(&find_recv.kind, RustExprKind::Ident(name) if name == "re")
                                );
                                // Arg is &str (borrowed string)
                                assert_eq!(find_args.len(), 1);
                                assert!(matches!(&find_args[0].kind, RustExprKind::Borrow(_)));
                            }
                            other => panic!("expected MethodCall(find), got {other:?}"),
                        }
                    }
                    other => panic!("expected MethodCall(map), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_match_registry_lookup() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_string_method("match").is_some());
    }

    #[test]
    fn test_lower_string_search_registry_lookup() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_string_method("search").is_some());
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
    // Task 161: Map/Set forEach and size
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_map_for_each_produces_iter_for_each_with_swapped_tuple_param() {
        let closure = make_two_param_closure(
            "value",
            "key",
            RustExpr::synthetic(RustExprKind::Ident("value".into())),
        );
        let result = lower_map_for_each(map_receiver(), vec![closure], span());
        // Outermost: .for_each(closure)
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                assert_eq!(method, "for_each");
                assert_eq!(args.len(), 1);
                // Check closure has swapped tuple param (key, value)
                match &args[0].kind {
                    RustExprKind::Closure { params, .. } => {
                        assert_eq!(params.len(), 1);
                        assert_eq!(params[0].name, "(key, value)");
                    }
                    other => panic!("expected Closure, got {other:?}"),
                }
                // Inner: .iter()
                match &receiver.kind {
                    RustExprKind::MethodCall { method, .. } => {
                        assert_eq!(method, "iter");
                    }
                    other => panic!("expected MethodCall(iter), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(for_each), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_map_for_each_single_param_uses_default_key() {
        let closure = make_closure(
            "value",
            RustExpr::synthetic(RustExprKind::Ident("value".into())),
        );
        let result = lower_map_for_each(map_receiver(), vec![closure], span());
        match &result.kind {
            RustExprKind::MethodCall { args, .. } => {
                match &args[0].kind {
                    RustExprKind::Closure { params, .. } => {
                        // Only one param → key defaults to _k
                        assert_eq!(params[0].name, "(_k, value)");
                    }
                    other => panic!("expected Closure, got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(for_each), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_set_for_each_produces_iter_for_each() {
        let closure = make_closure(
            "value",
            RustExpr::synthetic(RustExprKind::Ident("value".into())),
        );
        let result = lower_set_for_each(set_receiver(), vec![closure], span());
        // Outermost: .for_each(closure)
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                assert_eq!(method, "for_each");
                assert_eq!(args.len(), 1);
                // Check closure has the original param
                match &args[0].kind {
                    RustExprKind::Closure { params, .. } => {
                        assert_eq!(params.len(), 1);
                        assert_eq!(params[0].name, "value");
                    }
                    other => panic!("expected Closure, got {other:?}"),
                }
                // Inner: .iter()
                match &receiver.kind {
                    RustExprKind::MethodCall { method, .. } => {
                        assert_eq!(method, "iter");
                    }
                    other => panic!("expected MethodCall(iter), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(for_each), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_for_each_registered_as_map_set_method() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_map_set_method("forEach").is_some(),
            "expected forEach to be registered as map/set method"
        );
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
    // Task 169: 21 missing Math methods — unit tests
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_math_acos_produces_acos() {
        let result = lower_math_acos(vec![float_arg(1.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "acos"),
            other => panic!("expected MethodCall(acos), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_acosh_produces_acosh() {
        let result = lower_math_acosh(vec![float_arg(1.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "acosh"),
            other => panic!("expected MethodCall(acosh), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_asin_produces_asin() {
        let result = lower_math_asin(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "asin"),
            other => panic!("expected MethodCall(asin), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_asinh_produces_asinh() {
        let result = lower_math_asinh(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "asinh"),
            other => panic!("expected MethodCall(asinh), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_atan_produces_atan() {
        let result = lower_math_atan(vec![float_arg(1.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "atan"),
            other => panic!("expected MethodCall(atan), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_atan2_produces_atan2_with_two_args() {
        let result = lower_math_atan2(vec![float_arg(1.0), float_arg(1.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "atan2");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(atan2), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_atanh_produces_atanh() {
        let result = lower_math_atanh(vec![float_arg(0.5)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "atanh"),
            other => panic!("expected MethodCall(atanh), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_cbrt_produces_cbrt() {
        let result = lower_math_cbrt(vec![float_arg(8.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "cbrt"),
            other => panic!("expected MethodCall(cbrt), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_cosh_produces_cosh() {
        let result = lower_math_cosh(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "cosh"),
            other => panic!("expected MethodCall(cosh), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_exp_produces_exp() {
        let result = lower_math_exp(vec![float_arg(1.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "exp"),
            other => panic!("expected MethodCall(exp), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_expm1_produces_exp_m1() {
        let result = lower_math_expm1(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "exp_m1"),
            other => panic!("expected MethodCall(exp_m1), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_fround_produces_double_cast() {
        let result = lower_math_fround(vec![float_arg(1.5)], span());
        match &result.kind {
            RustExprKind::Cast(inner, outer_ty) => {
                assert!(matches!(outer_ty, RustType::F64));
                match &inner.kind {
                    RustExprKind::Cast(_, inner_ty) => {
                        assert!(matches!(inner_ty, RustType::F32));
                    }
                    other => panic!("expected inner Cast to f32, got {other:?}"),
                }
            }
            other => panic!("expected Cast(Cast(x, f32), f64), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_hypot_produces_hypot_with_two_args() {
        let result = lower_math_hypot(vec![float_arg(3.0), float_arg(4.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "hypot");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall(hypot), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_log10_produces_log10() {
        let result = lower_math_log10(vec![float_arg(100.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "log10"),
            other => panic!("expected MethodCall(log10), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_log1p_produces_ln_1p() {
        let result = lower_math_log1p(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "ln_1p"),
            other => panic!("expected MethodCall(ln_1p), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_log2_produces_log2() {
        let result = lower_math_log2(vec![float_arg(8.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "log2"),
            other => panic!("expected MethodCall(log2), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_sign_produces_signum() {
        let result = lower_math_sign(vec![float_arg(-5.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "signum"),
            other => panic!("expected MethodCall(signum), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_sinh_produces_sinh() {
        let result = lower_math_sinh(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "sinh"),
            other => panic!("expected MethodCall(sinh), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_tanh_produces_tanh() {
        let result = lower_math_tanh(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "tanh"),
            other => panic!("expected MethodCall(tanh), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_trunc_produces_trunc() {
        let result = lower_math_trunc(vec![float_arg(4.7)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "trunc"),
            other => panic!("expected MethodCall(trunc), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_math_clz32_produces_leading_zeros_cast() {
        let result = lower_math_clz32(vec![float_arg(1.0)], span());
        match &result.kind {
            RustExprKind::Cast(inner, outer_ty) => {
                assert!(matches!(outer_ty, RustType::F64));
                match &inner.kind {
                    RustExprKind::MethodCall { method, .. } => {
                        assert_eq!(method, "leading_zeros");
                    }
                    other => panic!("expected inner MethodCall(leading_zeros), got {other:?}"),
                }
            }
            other => panic!("expected Cast(MethodCall(leading_zeros), f64), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_all_21_new_math_methods() {
        let registry = BuiltinRegistry::new();
        for method in &[
            "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "cbrt", "cosh", "exp",
            "expm1", "fround", "hypot", "log10", "log1p", "log2", "sign", "sinh", "tanh", "trunc",
            "clz32",
        ] {
            assert!(
                registry.lookup_method("Math", method).is_some(),
                "Math.{method} should be registered"
            );
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
    // Task 141: Additional console extensions
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_console_table_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "table").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_dir_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "dir").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_assert_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "assert").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_time_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "time").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_time_end_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "timeEnd").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_time_log_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "timeLog").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_count_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "count").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_count_reset_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "countReset").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_group_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "group").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_group_end_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "groupEnd").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_clear_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "clear").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_console_trace_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("console", "trace").is_some());
    }

    #[test]
    fn test_lower_console_table_produces_debug_println() {
        let result = lower_console_table(vec![string_arg("data")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "println");
                assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "{:#?}"));
            }
            other => panic!("expected Macro(println), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_dir_produces_debug_println() {
        let result = lower_console_dir(vec![string_arg("obj")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "println");
                assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "{:#?}"));
            }
            other => panic!("expected Macro(println), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_assert_with_message() {
        let cond = RustExpr::new(RustExprKind::BoolLit(true), span());
        let msg = string_arg("should be true");
        let result = lower_console_assert(vec![cond, msg], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "assert");
                assert_eq!(args.len(), 3); // cond + format string + message
            }
            other => panic!("expected Macro(assert), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_assert_without_message() {
        let cond = RustExpr::new(RustExprKind::BoolLit(true), span());
        let result = lower_console_assert(vec![cond], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "assert");
                assert_eq!(args.len(), 1); // only condition
            }
            other => panic!("expected Macro(assert), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_time_produces_eprintln() {
        let result = lower_console_time(vec![string_arg("benchmark")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(
                    matches!(&args[0].kind, RustExprKind::StringLit(s) if s.contains("timer started"))
                );
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_time_end_produces_eprintln() {
        let result = lower_console_time_end(vec![string_arg("benchmark")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(
                    matches!(&args[0].kind, RustExprKind::StringLit(s) if s.contains("timer ended"))
                );
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_time_log_produces_eprintln_with_time_placeholder() {
        let result = lower_console_time_log(vec![string_arg("benchmark")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(
                    matches!(&args[0].kind, RustExprKind::StringLit(s) if s.contains("<time>"))
                );
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_count_produces_eprintln() {
        let result = lower_console_count(vec![string_arg("loop")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s.contains("count")));
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_count_reset_produces_eprintln() {
        let result = lower_console_count_reset(vec![string_arg("loop")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(
                    matches!(&args[0].kind, RustExprKind::StringLit(s) if s.contains("count reset"))
                );
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_group_produces_eprintln() {
        let result = lower_console_group(vec![string_arg("section")], span());
        match &result.kind {
            RustExprKind::Macro { name, .. } => assert_eq!(name, "eprintln"),
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_group_end_produces_block_expr() {
        let result = lower_console_group_end(vec![], span());
        assert!(
            matches!(&result.kind, RustExprKind::BlockExpr(_)),
            "expected BlockExpr, got {:?}",
            result.kind
        );
    }

    #[test]
    fn test_lower_console_clear_produces_print() {
        let result = lower_console_clear(vec![], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "print");
                assert!(
                    matches!(&args[0].kind, RustExprKind::StringLit(s) if s.contains("\x1B[2J"))
                );
            }
            other => panic!("expected Macro(print), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_trace_produces_eprintln() {
        let result = lower_console_trace(vec![string_arg("here")], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(
                    matches!(&args[0].kind, RustExprKind::StringLit(s) if s.starts_with("Trace:"))
                );
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_console_trace_no_args_produces_trace_only() {
        let result = lower_console_trace(vec![], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "eprintln");
                assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "Trace"));
            }
            other => panic!("expected Macro(eprintln), got {other:?}"),
        }
    }

    #[test]
    fn test_console_table_is_ref_args() {
        let registry = BuiltinRegistry::new();
        assert!(registry.is_ref_args("console", "table"));
    }

    #[test]
    fn test_console_assert_is_ref_args() {
        let registry = BuiltinRegistry::new();
        assert!(registry.is_ref_args("console", "assert"));
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
    // Task 137: Global parseInt / parseFloat
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_function_parse_int_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("parseInt").is_some(),
            "parseInt should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_builtin_registry_lookup_function_parse_float_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("parseFloat").is_some(),
            "parseFloat should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_lower_parse_int_global() {
        let result = lower_parse_int_global(vec![string_arg("42")], span());
        match &result.kind {
            RustExprKind::MethodCall {
                method, receiver, ..
            } => {
                assert_eq!(method, "unwrap_or");
                // The receiver should be a parse::<i64>() call
                match &receiver.kind {
                    RustExprKind::MethodCall { method, .. } => assert_eq!(method, "parse"),
                    other => panic!("expected inner MethodCall(parse), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_parse_int_with_radix() {
        let result = lower_parse_int_global(vec![string_arg("ff"), int_arg(16)], span());
        match &result.kind {
            RustExprKind::MethodCall {
                method, receiver, ..
            } => {
                assert_eq!(method, "unwrap_or");
                // The receiver should be i64::from_str_radix(...)
                match &receiver.kind {
                    RustExprKind::StaticCall {
                        type_name, method, ..
                    } => {
                        assert_eq!(type_name, "i64");
                        assert_eq!(method, "from_str_radix");
                    }
                    other => panic!("expected StaticCall(i64::from_str_radix), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_parse_float_global() {
        let result = lower_parse_float_global(vec![string_arg("3.14")], span());
        match &result.kind {
            RustExprKind::MethodCall {
                method, receiver, ..
            } => {
                assert_eq!(method, "unwrap_or");
                // The receiver should be a parse::<f64>() call
                match &receiver.kind {
                    RustExprKind::MethodCall { method, .. } => assert_eq!(method, "parse"),
                    other => panic!("expected inner MethodCall(parse), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap_or), got {other:?}"),
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

    #[test]
    fn test_builtin_registry_lookup_object_assign_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "assign").is_some());
    }

    #[test]
    fn test_lower_object_assign_produces_closure_call() {
        let target = map_receiver();
        let source = map_receiver();
        let result = lower_object_assign(vec![target, source], span());
        match &result.kind {
            RustExprKind::ClosureCall { body, .. } => {
                // Should have a let-mut statement, a semi (extend), and a trailing expr
                assert_eq!(body.stmts.len(), 2);
                assert!(body.expr.is_some());
            }
            other => panic!("expected ClosureCall, got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_object_from_entries_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "fromEntries").is_some());
    }

    #[test]
    fn test_lower_object_from_entries_produces_collect_chain() {
        let result = lower_object_from_entries(vec![map_receiver()], span());
        match &result.kind {
            RustExprKind::MethodCall {
                method, type_args, ..
            } => {
                assert_eq!(method, "collect");
                // Should collect into HashMap<_, _>
                assert!(!type_args.is_empty());
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_object_freeze_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "freeze").is_some());
    }

    #[test]
    fn test_lower_object_freeze_returns_argument_unchanged() {
        let arg = map_receiver();
        let result = lower_object_freeze(vec![arg], span());
        // freeze is a no-op — returns the argument as-is
        match &result.kind {
            RustExprKind::Ident(name) => assert_eq!(name, "m"),
            other => panic!("expected Ident(m), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_object_create_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "create").is_some());
    }

    #[test]
    fn test_lower_object_create_returns_argument_unchanged() {
        let arg = map_receiver();
        let result = lower_object_create(vec![arg], span());
        // create is a stub — returns the argument as-is
        match &result.kind {
            RustExprKind::Ident(name) => assert_eq!(name, "m"),
            other => panic!("expected Ident(m), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 177: Object static methods (hasOwn, is, isFrozen, etc.)
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_object_has_own_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "hasOwn").is_some());
    }

    #[test]
    fn test_lower_object_has_own_produces_contains_key_call() {
        let obj = map_receiver();
        let key = RustExpr::synthetic(RustExprKind::StringLit("k".into()));
        let result = lower_object_has_own(vec![obj, key], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "contains_key");
                assert_eq!(args.len(), 1);
                // Argument should be a borrow of the key
                assert!(
                    matches!(&args[0].kind, RustExprKind::Borrow(_)),
                    "expected Borrow, got {:?}",
                    &args[0].kind
                );
            }
            other => panic!("expected MethodCall(contains_key), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_object_is_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "is").is_some());
    }

    #[test]
    fn test_lower_object_is_produces_eq_binary_op() {
        let a = RustExpr::synthetic(RustExprKind::IntLit(1));
        let b = RustExpr::synthetic(RustExprKind::IntLit(1));
        let result = lower_object_is(vec![a, b], span());
        match &result.kind {
            RustExprKind::Binary { op, .. } => {
                assert_eq!(*op, RustBinaryOp::Eq);
            }
            other => panic!("expected Binary(==), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_object_is_frozen_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "isFrozen").is_some());
    }

    #[test]
    fn test_lower_object_is_frozen_returns_true_literal() {
        let result = lower_object_is_frozen(vec![map_receiver()], span());
        match &result.kind {
            RustExprKind::BoolLit(true) => {}
            other => panic!("expected BoolLit(true), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_object_get_own_property_names_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry
                .lookup_method("Object", "getOwnPropertyNames")
                .is_some()
        );
    }

    #[test]
    fn test_lower_object_get_own_property_names_produces_collect_chain() {
        let result = lower_object_get_own_property_names(vec![map_receiver()], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "collect"),
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_object_get_prototype_of_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "getPrototypeOf").is_some());
    }

    #[test]
    fn test_lower_object_get_prototype_of_returns_none_variant() {
        let result = lower_object_get_prototype_of(vec![map_receiver()], span());
        match &result.kind {
            RustExprKind::EnumVariant {
                enum_name,
                variant_name,
            } => {
                assert_eq!(enum_name, "Option");
                assert_eq!(variant_name, "None");
            }
            other => panic!("expected EnumVariant(Option::None), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_object_define_property_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Object", "defineProperty").is_some());
    }

    #[test]
    fn test_lower_object_define_property_returns_object_argument() {
        let obj = map_receiver();
        let prop = RustExpr::synthetic(RustExprKind::StringLit("x".into()));
        let desc = RustExpr::synthetic(RustExprKind::Ident("desc".into()));
        let result = lower_object_define_property(vec![obj, prop, desc], span());
        // Should return the object argument unchanged (no-op stub)
        match &result.kind {
            RustExprKind::Ident(name) => assert_eq!(name, "m"),
            other => panic!("expected Ident(m), got {other:?}"),
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
    fn test_lower_math_constant_ln2() {
        let result = lower_math_constant("Math", "LN2");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(
            matches!(&expr.kind, RustExprKind::Ident(name) if name == "std::f64::consts::LN_2")
        );
    }

    #[test]
    fn test_lower_math_constant_ln10() {
        let result = lower_math_constant("Math", "LN10");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(
            matches!(&expr.kind, RustExprKind::Ident(name) if name == "std::f64::consts::LN_10")
        );
    }

    #[test]
    fn test_lower_math_constant_log2e() {
        let result = lower_math_constant("Math", "LOG2E");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(
            matches!(&expr.kind, RustExprKind::Ident(name) if name == "std::f64::consts::LOG2_E")
        );
    }

    #[test]
    fn test_lower_math_constant_log10e() {
        let result = lower_math_constant("Math", "LOG10E");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(
            matches!(&expr.kind, RustExprKind::Ident(name) if name == "std::f64::consts::LOG10_E")
        );
    }

    #[test]
    fn test_lower_math_constant_sqrt2() {
        let result = lower_math_constant("Math", "SQRT2");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(
            matches!(&expr.kind, RustExprKind::Ident(name) if name == "std::f64::consts::SQRT_2")
        );
    }

    #[test]
    fn test_lower_math_constant_sqrt1_2() {
        let result = lower_math_constant("Math", "SQRT1_2");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(
            matches!(&expr.kind, RustExprKind::Ident(name) if name == "std::f64::consts::FRAC_1_SQRT_2")
        );
    }

    #[test]
    fn test_lower_math_constant_unknown_returns_none() {
        assert!(lower_math_constant("Math", "TAU").is_none());
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
    // Task 130: RegExp class support
    // ---------------------------------------------------------------

    fn regex_receiver() -> RustExpr {
        RustExpr::new(RustExprKind::Ident("re".to_owned()), span())
    }

    #[test]
    fn test_builtin_registry_lookup_regex_method_test_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_regex_method("test").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_regex_method_exec_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_regex_method("exec").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_regex_method_unknown_returns_none() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_regex_method("foo").is_none());
    }

    #[test]
    fn test_lower_regexp_test() {
        let result = lower_regexp_test(regex_receiver(), vec![string_arg("hello")], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                assert_eq!(method, "is_match");
                assert!(matches!(&receiver.kind, RustExprKind::Ident(name) if name == "re"));
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0].kind, RustExprKind::Borrow(_)));
            }
            other => panic!("expected MethodCall(is_match), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 133: Static Array methods
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_array_from() {
        let args = vec![RustExpr::new(
            RustExprKind::Ident("items".to_owned()),
            span(),
        )];
        let result = lower_array_from(args, span());
        // Should produce: items.into_iter().collect::<Vec<_>>()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                type_args,
                ..
            } => {
                assert_eq!(method, "collect");
                assert_eq!(type_args.len(), 1);
                // Receiver should be .into_iter()
                match &receiver.kind {
                    RustExprKind::MethodCall {
                        method: inner_method,
                        receiver: inner_recv,
                        ..
                    } => {
                        assert_eq!(inner_method, "into_iter");
                        match &inner_recv.kind {
                            RustExprKind::Ident(name) => assert_eq!(name, "items"),
                            other => panic!("expected Ident, got {other:?}"),
                        }
                    }
                    other => panic!("expected MethodCall(into_iter), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 134: Static String methods
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_string_from_char_code_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("String", "fromCharCode").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_string_from_code_point_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("String", "fromCodePoint").is_some());
    }

    // ---------------------------------------------------------------
    // Task 138: Global isNaN / isFinite + Number.isSafeInteger + constants
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_is_nan_global() {
        // Global isNaN(x) reuses lower_number_is_nan, should produce x.is_nan()
        let result = lower_number_is_nan(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "is_nan"),
            other => panic!("expected MethodCall(is_nan), got {other:?}"),
        }
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

    // ---------------------------------------------------------------
    // Task 172: Date formatting method registry lookups
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_date_to_date_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toDateString").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_to_time_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toTimeString").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_to_utc_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toUTCString").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_to_json() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toJSON").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_to_locale_date_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toLocaleDateString").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_to_locale_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toLocaleString").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_to_locale_time_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("toLocaleTimeString").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_value_of() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("valueOf").is_some());
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

    // ---------------------------------------------------------------
    // Task 170: Date getter methods
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_date_get_full_year() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getFullYear").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_month() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getMonth").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_date() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getDate").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_day() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getDay").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_hours() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getHours").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_minutes() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getMinutes").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_seconds() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getSeconds").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_milliseconds() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getMilliseconds").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_get_timezone_offset() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("getTimezoneOffset").is_some());
    }

    #[test]
    fn test_lower_date_get_full_year_emits_hinnant_algorithm() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_full_year(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("UNIX_EPOCH"), "should reference UNIX_EPOCH");
                assert!(code.contains("719468"), "should use Hinnant epoch offset");
                assert!(code.contains("__year"), "should compute __year");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_month_emits_zero_based() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_month(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__m as i64) - 1"),
                    "should return 0-based month"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_date_emits_day_of_month() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_date(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("__d as i64"), "should return day of month");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_day_emits_day_of_week() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_day(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("__dow"), "should compute day of week");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_hours_emits_time_extraction() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_hours(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("3600"), "should divide by 3600 for hours");
                assert!(
                    code.contains("86400"),
                    "should mod by 86400 for time of day"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_minutes_emits_time_extraction() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_minutes(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("3600"), "should use 3600");
                assert!(code.contains("60"), "should divide by 60 for minutes");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_seconds_emits_mod_60() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_seconds(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("% 60"), "should mod by 60 for seconds");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_milliseconds_emits_subsec() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_milliseconds(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("subsec_millis"), "should use subsec_millis()");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_get_timezone_offset_returns_zero() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_get_timezone_offset(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert_eq!(code, "0i64", "UTC offset should be 0");
            }
            other => panic!("expected Raw(0i64), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 173: Date.parse and Date.UTC static method tests
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_date_parse() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_method("Date", "parse").is_some(),
            "Date.parse should be registered"
        );
    }

    #[test]
    fn test_builtin_registry_lookup_date_utc() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_method("Date", "UTC").is_some(),
            "Date.UTC should be registered"
        );
    }

    #[test]
    fn test_lower_date_parse() {
        let result = lower_date_parse(
            vec![RustExpr::new(
                RustExprKind::StringLit("2024-01-15".to_owned()),
                span(),
            )],
            span(),
        );
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("__ds"), "should define __ds variable: {code}");
                assert!(
                    code.contains("split('-')"),
                    "should split date parts: {code}"
                );
                assert!(
                    code.contains("__total_secs"),
                    "should compute total seconds: {code}"
                );
            }
            other => panic!("expected Raw block, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_utc() {
        let result = lower_date_utc(
            vec![
                RustExpr::new(RustExprKind::IntLit(2024), span()),
                RustExpr::new(RustExprKind::IntLit(0), span()),
                RustExpr::new(RustExprKind::IntLit(15), span()),
            ],
            span(),
        );
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("2024"), "should contain year: {code}");
                assert!(
                    code.contains("__total_days"),
                    "should compute total days: {code}"
                );
                assert!(code.contains("+ 1"), "should adjust 0-based month: {code}");
            }
            other => panic!("expected Raw block, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_is_finite_global() {
        // Global isFinite(x) reuses lower_number_is_finite, should produce x.is_finite()
        let result = lower_number_is_finite(vec![float_arg(0.0)], span());
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => assert_eq!(method, "is_finite"),
            other => panic!("expected MethodCall(is_finite), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_function_is_nan_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("isNaN").is_some(),
            "isNaN should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_lower_string_from_char_code_single_arg() {
        let result = lower_string_from_char_code(vec![int_arg(65)], span());
        // Should produce: char::from_u32(65 as u32).map(|c| c.to_string()).unwrap_or_default()
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "unwrap_or_default");
            }
            other => panic!("expected MethodCall(unwrap_or_default), got {other:?}"),
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

    // ---------------------------------------------------------------
    // Task 172: Date formatting method lowering tests
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_date_to_date_string() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_date_string(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("UNIX_EPOCH"), "should reference UNIX_EPOCH");
                assert!(code.contains("__DAYS"), "should have day name array");
                assert!(code.contains("__MONTHS"), "should have month name array");
                assert!(code.contains("__dow"), "should compute day-of-week");
                assert!(code.contains("format!"), "should format output");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_to_time_string() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_time_string(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("UNIX_EPOCH"), "should reference UNIX_EPOCH");
                assert!(code.contains("__hours"), "should compute hours");
                assert!(code.contains("__minutes"), "should compute minutes");
                assert!(code.contains("__seconds"), "should compute seconds");
                assert!(code.contains("GMT+0000"), "should include timezone");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_to_utc_string() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_utc_string(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("UNIX_EPOCH"), "should reference UNIX_EPOCH");
                assert!(code.contains("__DAYS"), "should have day name array");
                assert!(code.contains("__MONTHS"), "should have month name array");
                assert!(code.contains("GMT"), "should include GMT suffix");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_to_json_delegates_to_iso_string() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_json(receiver, vec![], span());
        // toJSON delegates to toISOString, so it should produce a Raw block with ISO format
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("UNIX_EPOCH"), "should reference UNIX_EPOCH");
                assert!(code.contains("format!"), "should format output");
            }
            other => panic!("expected Raw (ISO delegate), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_to_locale_date_string_delegates() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_locale_date_string(receiver, vec![], span());
        // MVP: delegates to toDateString
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("__DAYS"), "should have day name array");
                assert!(code.contains("__MONTHS"), "should have month name array");
            }
            other => panic!("expected Raw (toDateString delegate), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_to_locale_string_delegates() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_locale_string(receiver, vec![], span());
        // MVP: delegates to toString -> format!("{:?}", receiver)
        match &result.kind {
            RustExprKind::Macro { name, .. } => {
                assert_eq!(name, "format");
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_to_locale_time_string_delegates() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_to_locale_time_string(receiver, vec![], span());
        // MVP: delegates to toTimeString
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(code.contains("__hours"), "should compute hours");
                assert!(code.contains("GMT+0000"), "should include timezone");
            }
            other => panic!("expected Raw (toTimeString delegate), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_value_of_delegates_to_get_time() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_value_of(receiver, vec![], span());
        // valueOf delegates to getTime, should produce .as_millis()
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "as_millis");
            }
            other => panic!("expected MethodCall(as_millis), got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Task 171: Date setter registry + lowering tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_date_set_time() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("setTime").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_set_full_year() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("setFullYear").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_set_month() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("setMonth").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_set_date() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("setDate").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_set_hours() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("setHours").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_set_minutes() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("setMinutes").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_set_seconds() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("setSeconds").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_date_set_milliseconds() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_date_method("setMilliseconds").is_some());
    }

    #[test]
    fn test_lower_date_set_time() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_set_time(receiver, vec![int_arg(1_000_000)], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("UNIX_EPOCH"),
                    "setTime should reference UNIX_EPOCH: {code}"
                );
                assert!(
                    code.contains("Duration::from_millis"),
                    "setTime should use Duration::from_millis: {code}"
                );
                assert!(
                    code.contains("1000000"),
                    "setTime should embed the argument: {code}"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_set_full_year() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_set_full_year(receiver, vec![int_arg(2025)], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__year = 2025"),
                    "setFullYear should override __year: {code}"
                );
                assert!(
                    code.contains("UNIX_EPOCH"),
                    "setFullYear should reconstruct from UNIX_EPOCH: {code}"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_set_month() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_set_month(receiver, vec![int_arg(5)], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__month = 5 as i64 + 1"),
                    "setMonth should add 1 for 0-based JS months: {code}"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_set_date() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_set_date(receiver, vec![int_arg(15)], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__day = 15"),
                    "setDate should override __day: {code}"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_set_hours() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_set_hours(receiver, vec![int_arg(12)], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__hours = 12"),
                    "setHours should override __hours: {code}"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_set_minutes() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_set_minutes(receiver, vec![int_arg(30)], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__minutes = 30"),
                    "setMinutes should override __minutes: {code}"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_set_seconds() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_set_seconds(receiver, vec![int_arg(45)], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__seconds = 45"),
                    "setSeconds should override __seconds: {code}"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_date_set_milliseconds() {
        let receiver = RustExpr::new(RustExprKind::Ident("d".to_owned()), span());
        let result = lower_date_set_milliseconds(receiver, vec![int_arg(500)], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__millis = 500"),
                    "setMilliseconds should override __millis: {code}"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_from_char_code_multi_arg() {
        let result =
            lower_string_from_char_code(vec![int_arg(72), int_arg(101), int_arg(108)], span());
        // Should produce: vec![72, 101, 108].into_iter().filter_map(...).collect::<String>()
        match &result.kind {
            RustExprKind::MethodCall {
                method, type_args, ..
            } => {
                assert_eq!(method, "collect");
                assert_eq!(type_args.len(), 1);
                assert_eq!(type_args[0].to_string(), "String");
            }
            other => panic!("expected MethodCall(collect::<String>), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_function_is_finite_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("isFinite").is_some(),
            "isFinite should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_lower_number_is_safe_integer_produces_and_chain() {
        let result = lower_number_is_safe_integer(vec![float_arg(5.0)], span());
        // The top-level should be Binary(And) — the outer &&
        match &result.kind {
            RustExprKind::Binary {
                op: RustBinaryOp::And,
                ..
            } => {} // correct
            other => panic!("expected Binary(And), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_is_array() {
        let args = vec![RustExpr::new(RustExprKind::Ident("x".to_owned()), span())];
        let result = lower_array_is_array(args, span());
        match &result.kind {
            RustExprKind::BoolLit(val) => assert!(val),
            other => panic!("expected BoolLit(true), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_of() {
        let args = vec![
            RustExpr::new(RustExprKind::IntLit(1), span()),
            RustExpr::new(RustExprKind::IntLit(2), span()),
            RustExpr::new(RustExprKind::IntLit(3), span()),
        ];
        let result = lower_array_of(args, span());
        match &result.kind {
            RustExprKind::VecLit(elems) => {
                assert_eq!(elems.len(), 3);
                match &elems[0].kind {
                    RustExprKind::IntLit(n) => assert_eq!(*n, 1),
                    other => panic!("expected IntLit(1), got {other:?}"),
                }
            }
            other => panic!("expected VecLit, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 140: Array mutation methods
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_all_array_methods_registered() {
        let registry = BuiltinRegistry::new();
        let methods = [
            "indexOf",
            "lastIndexOf",
            "includes",
            "at",
            "concat",
            "slice",
            "keys",
            "values",
            "entries",
        ];
        for method in methods {
            assert!(
                registry.lookup_array_method(method).is_some(),
                "expected array method '{method}' to be registered"
            );
        }
    }

    #[test]
    fn test_lower_array_splice_produces_drain_collect() {
        let result = lower_array_splice(array_receiver(), vec![int_arg(1), int_arg(2)], span());
        // Outermost: .collect::<Vec<_>>()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "collect");
                // Inner: .drain(range)
                match &receiver.kind {
                    RustExprKind::MethodCall { method, args, .. } => {
                        assert_eq!(method, "drain");
                        assert_eq!(args.len(), 1);
                    }
                    other => panic!("expected MethodCall(drain), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_copy_within_produces_copy_within() {
        let result = lower_array_copy_within(
            array_receiver(),
            vec![int_arg(0), int_arg(2), int_arg(4)],
            span(),
        );
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "copy_within");
                assert_eq!(args.len(), 2); // range + target
            }
            other => panic!("expected MethodCall(copy_within), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_index_of_produces_position_chain() {
        let result = lower_array_index_of(array_receiver(), vec![int_arg(42)], span());
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
    fn test_lower_array_last_index_of_produces_rposition_chain() {
        let result = lower_array_last_index_of(array_receiver(), vec![int_arg(42)], span());
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
    fn test_lower_array_includes_produces_contains_with_borrow() {
        let result = lower_array_includes(array_receiver(), vec![int_arg(5)], span());
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
    fn test_lower_array_at_produces_get_cloned() {
        let result = lower_array_at(array_receiver(), vec![int_arg(2)], span());
        // Outermost: .cloned()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "cloned");
                // Inner: .get(index as usize)
                match &receiver.kind {
                    RustExprKind::MethodCall { method, args, .. } => {
                        assert_eq!(method, "get");
                        assert_eq!(args.len(), 1);
                    }
                    other => panic!("expected MethodCall(get), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(cloned), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_regexp_exec() {
        let result = lower_regexp_exec(regex_receiver(), vec![string_arg("hello")], span());
        match &result.kind {
            RustExprKind::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                assert_eq!(method, "captures");
                assert!(matches!(&receiver.kind, RustExprKind::Ident(name) if name == "re"));
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0].kind, RustExprKind::Borrow(_)));
            }
            other => panic!("expected MethodCall(captures), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_string_from_code_point_single_arg() {
        let result = lower_string_from_code_point(vec![int_arg(128522)], span());
        // Should produce same shape as fromCharCode
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "unwrap_or_default");
            }
            other => panic!("expected MethodCall(unwrap_or_default), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_concat_produces_concat_chain() {
        let other = RustExpr::new(RustExprKind::Ident("other".to_owned()), span());
        let result = lower_array_concat(array_receiver(), vec![other], span());
        // Outermost: .concat()
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "concat");
            }
            other => panic!("expected MethodCall(concat), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_array_from() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Array", "from").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_array_is_array() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Array", "isArray").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_array_of() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_method("Array", "of").is_some());
    }

    #[test]
    fn test_lower_string_from_char_code_no_args() {
        let result = lower_string_from_char_code(vec![], span());
        // Zero args should still produce a valid expression (defaults to code 0)
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "unwrap_or_default");
            }
            other => panic!("expected MethodCall(unwrap_or_default), got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_registry_lookup_number_is_safe_integer_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_method("Number", "isSafeInteger").is_some(),
            "Number.isSafeInteger should be registered"
        );
    }

    #[test]
    fn test_lower_number_constant_max_safe_integer() {
        let result = lower_number_constant("Number", "MAX_SAFE_INTEGER");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(matches!(
            &expr.kind,
            RustExprKind::IntLit(9_007_199_254_740_991)
        ));
    }

    #[test]
    fn test_lower_number_constant_min_safe_integer() {
        let result = lower_number_constant("Number", "MIN_SAFE_INTEGER");
        assert!(result.is_some());
        let expr = result.unwrap();
        assert!(matches!(
            &expr.kind,
            RustExprKind::IntLit(-9_007_199_254_740_991)
        ));
    }

    #[test]
    fn test_lower_number_constant_unknown_returns_none() {
        assert!(lower_number_constant("Number", "UNKNOWN_CONSTANT").is_none());
    }

    #[test]
    fn test_lower_number_constant_non_number_returns_none() {
        assert!(lower_number_constant("Math", "MAX_SAFE_INTEGER").is_none());
    }

    // -----------------------------------------------------------------------
    // Task 174: Number constants — EPSILON, MAX_VALUE, MIN_VALUE, NaN,
    // NEGATIVE_INFINITY, POSITIVE_INFINITY
    // -----------------------------------------------------------------------

    #[test]
    fn test_lower_number_constant_epsilon() {
        let result = lower_number_constant("Number", "EPSILON");
        assert!(result.is_some(), "EPSILON should produce Some");
        assert!(
            matches!(&result.unwrap().kind, RustExprKind::Ident(s) if s == "f64::EPSILON"),
            "EPSILON should lower to f64::EPSILON"
        );
    }

    #[test]
    fn test_lower_number_constant_max_value() {
        let result = lower_number_constant("Number", "MAX_VALUE");
        assert!(result.is_some(), "MAX_VALUE should produce Some");
        assert!(
            matches!(&result.unwrap().kind, RustExprKind::Ident(s) if s == "f64::MAX"),
            "MAX_VALUE should lower to f64::MAX"
        );
    }

    #[test]
    fn test_lower_number_constant_min_value() {
        let result = lower_number_constant("Number", "MIN_VALUE");
        assert!(result.is_some(), "MIN_VALUE should produce Some");
        assert!(
            matches!(&result.unwrap().kind, RustExprKind::Ident(s) if s == "f64::MIN_POSITIVE"),
            "MIN_VALUE should lower to f64::MIN_POSITIVE"
        );
    }

    #[test]
    fn test_lower_number_constant_nan() {
        let result = lower_number_constant("Number", "NaN");
        assert!(result.is_some(), "NaN should produce Some");
        assert!(
            matches!(&result.unwrap().kind, RustExprKind::Ident(s) if s == "f64::NAN"),
            "NaN should lower to f64::NAN"
        );
    }

    #[test]
    fn test_lower_number_constant_negative_infinity() {
        let result = lower_number_constant("Number", "NEGATIVE_INFINITY");
        assert!(result.is_some(), "NEGATIVE_INFINITY should produce Some");
        assert!(
            matches!(&result.unwrap().kind, RustExprKind::Ident(s) if s == "f64::NEG_INFINITY"),
            "NEGATIVE_INFINITY should lower to f64::NEG_INFINITY"
        );
    }

    #[test]
    fn test_lower_number_constant_positive_infinity() {
        let result = lower_number_constant("Number", "POSITIVE_INFINITY");
        assert!(result.is_some(), "POSITIVE_INFINITY should produce Some");
        assert!(
            matches!(&result.unwrap().kind, RustExprKind::Ident(s) if s == "f64::INFINITY"),
            "POSITIVE_INFINITY should lower to f64::INFINITY"
        );
    }

    // -----------------------------------------------------------------------
    // Task 174: toExponential
    // -----------------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_number_to_exponential() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_number_method("toExponential").is_some());
    }

    #[test]
    fn test_lower_number_to_exponential_produces_format_macro() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_exponential(receiver, vec![int_arg(2)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                assert_eq!(args.len(), 3);
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{:.prec$e}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_to_exponential_no_arg_defaults_to_6() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_exponential(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                assert_eq!(args.len(), 3);
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{:.prec$e}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_slice_one_arg_produces_to_vec() {
        let result = lower_array_slice(array_receiver(), vec![int_arg(1)], span());
        // Outermost: .to_vec()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "to_vec");
                // Inner: index access
                assert!(matches!(&receiver.kind, RustExprKind::Index { .. }));
            }
            other => panic!("expected MethodCall(to_vec), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_slice_two_args_produces_to_vec() {
        let result = lower_array_slice(array_receiver(), vec![int_arg(1), int_arg(3)], span());
        // Outermost: .to_vec()
        match &result.kind {
            RustExprKind::MethodCall { method, .. } => {
                assert_eq!(method, "to_vec");
            }
            other => panic!("expected MethodCall(to_vec), got {other:?}"),
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

    #[test]
    fn test_needs_regex_crate_true_for_regexp() {
        assert!(needs_regex_crate("RegExp"));
    }

    #[test]
    fn test_needs_regex_crate_false_for_map() {
        assert!(!needs_regex_crate("Map"));
    }

    // ---------------------------------------------------------------
    // Task 160: Number instance methods
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_number_to_fixed() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_number_method("toFixed").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_number_to_precision() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_number_method("toPrecision").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_number_to_string() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_number_method("toString").is_some());
    }

    #[test]
    fn test_builtin_registry_lookup_number_unknown_returns_none() {
        let registry = BuiltinRegistry::new();
        assert!(registry.lookup_number_method("foo").is_none());
    }

    #[test]
    fn test_lower_number_to_fixed_produces_format_macro() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_fixed(receiver, vec![int_arg(2)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                assert_eq!(args.len(), 3);
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{:.prec$}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 163: reduceRight, keys, values, entries
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_array_reduce_right_produces_rev_fold() {
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
        let result = lower_array_reduce_right(receiver, vec![closure, init], span());
        match &result.kind {
            RustExprKind::IteratorChain { ops, terminal, .. } => {
                assert_eq!(ops.len(), 1);
                assert!(matches!(&ops[0], IteratorOp::Rev));
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

    #[test]
    fn test_lower_number_to_fixed_default_zero() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_fixed(receiver, vec![], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                // Should still produce format macro with default 0 digits
                assert_eq!(args.len(), 3);
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_to_precision_with_arg() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_precision(receiver, vec![int_arg(4)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                assert_eq!(args.len(), 3);
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{:.prec$e}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_to_precision_no_arg() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_precision(receiver, vec![], span());
        match &result.kind {
            RustExprKind::ToString(_) => {} // good
            other => panic!("expected ToString, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_to_string_no_radix() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_string(receiver, vec![], span());
        match &result.kind {
            RustExprKind::ToString(_) => {} // good
            other => panic!("expected ToString, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_to_string_radix_16() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_string(receiver, vec![int_arg(16)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{:x}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_to_string_radix_8() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_string(receiver, vec![int_arg(8)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{:o}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_to_string_radix_2() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_string(receiver, vec![int_arg(2)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{:b}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_number_to_string_radix_10() {
        let receiver = RustExpr::new(RustExprKind::Ident("num".to_owned()), span());
        let result = lower_number_to_string(receiver, vec![int_arg(10)], span());
        match &result.kind {
            RustExprKind::Macro { name, args } => {
                assert_eq!(name, "format");
                match &args[0].kind {
                    RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{}"),
                    other => panic!("expected StringLit format, got {other:?}"),
                }
            }
            other => panic!("expected Macro(format), got {other:?}"),
        }
    }

    #[test]
    fn test_is_number_type_i32() {
        assert!(is_number_type(&RustType::I32));
    }

    #[test]
    fn test_is_number_type_i64() {
        assert!(is_number_type(&RustType::I64));
    }

    #[test]
    fn test_is_number_type_f64() {
        assert!(is_number_type(&RustType::F64));
    }

    #[test]
    fn test_is_number_type_u8() {
        assert!(is_number_type(&RustType::U8));
    }

    #[test]
    fn test_is_number_type_string_is_not() {
        assert!(!is_number_type(&RustType::String));
    }

    #[test]
    fn test_is_number_type_bool_is_not() {
        assert!(!is_number_type(&RustType::Bool));
    }

    #[test]
    fn test_is_number_type_named_is_not() {
        assert!(!is_number_type(&RustType::Named("Foo".to_owned())));
    }

    #[test]
    fn test_lower_array_keys_produces_range_collect() {
        let result = lower_array_keys(array_receiver(), vec![], span());
        // Outermost: .collect::<Vec<_>>()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "collect");
                // Inner: .map(|i| i as i64)
                match &receiver.kind {
                    RustExprKind::MethodCall {
                        receiver: inner,
                        method,
                        ..
                    } => {
                        assert_eq!(method, "map");
                        // Inner: Range 0..arr.len()
                        match &inner.kind {
                            RustExprKind::Range { .. } => {}
                            other => panic!("expected Range, got {other:?}"),
                        }
                    }
                    other => panic!("expected MethodCall(map), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_values_produces_clone() {
        let result = lower_array_values(array_receiver(), vec![], span());
        match &result.kind {
            RustExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "clone");
                assert!(args.is_empty());
            }
            other => panic!("expected MethodCall(clone), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_array_entries_produces_enumerate_collect() {
        let result = lower_array_entries(array_receiver(), vec![], span());
        // Outermost: .collect::<Vec<_>>()
        match &result.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "collect");
                // Inner: .map(|(i, v)| (i as i64, v))
                match &receiver.kind {
                    RustExprKind::MethodCall {
                        receiver: inner,
                        method,
                        ..
                    } => {
                        assert_eq!(method, "map");
                        // Inner: .enumerate()
                        match &inner.kind {
                            RustExprKind::MethodCall { method, .. } => {
                                assert_eq!(method, "enumerate");
                            }
                            other => panic!("expected MethodCall(enumerate), got {other:?}"),
                        }
                    }
                    other => panic!("expected MethodCall(map), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(collect), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 175: Encoding/decoding global functions
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_registry_lookup_function_btoa_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("btoa").is_some(),
            "btoa should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_builtin_registry_lookup_function_atob_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("atob").is_some(),
            "atob should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_builtin_registry_lookup_function_encode_uri_component_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("encodeURIComponent").is_some(),
            "encodeURIComponent should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_builtin_registry_lookup_function_decode_uri_component_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("decodeURIComponent").is_some(),
            "decodeURIComponent should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_builtin_registry_lookup_function_encode_uri_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("encodeURI").is_some(),
            "encodeURI should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_builtin_registry_lookup_function_decode_uri_returns_some() {
        let registry = BuiltinRegistry::new();
        assert!(
            registry.lookup_function("decodeURI").is_some(),
            "decodeURI should be registered as a builtin free function"
        );
    }

    #[test]
    fn test_lower_btoa_produces_raw_with_base64_chars() {
        let result = lower_btoa(vec![string_arg("hello")], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__B64_CHARS"),
                    "btoa should use __B64_CHARS lookup table"
                );
                assert!(code.contains("__b64_out"), "btoa should emit __b64_out");
                assert!(code.contains("\"hello\""), "btoa should embed the input");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_atob_produces_raw_with_b64d_val() {
        let result = lower_atob(vec![string_arg("aGVsbG8=")], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("b64d_val"),
                    "atob should use b64d_val decoder"
                );
                assert!(code.contains("__b64d_out"), "atob should emit __b64d_out");
                assert!(code.contains("aGVsbG8="), "atob should embed the input");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_encode_uri_component_produces_raw_with_percent_format() {
        let result = lower_encode_uri_component(vec![string_arg("hello world")], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("%{:02X}"),
                    "encodeURIComponent should use percent-hex format"
                );
                assert!(
                    code.contains("__enc_out"),
                    "encodeURIComponent should emit __enc_out"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_decode_uri_component_produces_raw_with_from_str_radix() {
        let result = lower_decode_uri_component(vec![string_arg("hello%20world")], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("from_str_radix"),
                    "decodeURIComponent should use from_str_radix"
                );
                assert!(
                    code.contains("__dec_bytes"),
                    "decodeURIComponent should emit __dec_bytes"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_encode_uri_preserves_structural_chars() {
        let result = lower_encode_uri(vec![string_arg("https://example.com/path?q=1")], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("%{:02X}"),
                    "encodeURI should use percent-hex format"
                );
                assert!(
                    code.contains("b':'"),
                    "encodeURI should preserve structural chars"
                );
                assert!(code.contains("b'/'"), "encodeURI should preserve slash");
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_decode_uri_produces_raw_with_reserved_check() {
        let result = lower_decode_uri(vec![string_arg("hello%20world")], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__DURI_RESERVED"),
                    "decodeURI should check reserved chars"
                );
                assert!(
                    code.contains("from_str_radix"),
                    "decodeURI should use from_str_radix"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_btoa_no_args_uses_empty_string() {
        let result = lower_btoa(vec![], span());
        match &result.kind {
            RustExprKind::Raw(code) => {
                assert!(
                    code.contains("__b64_out"),
                    "btoa with no args should still emit output var"
                );
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }
}
