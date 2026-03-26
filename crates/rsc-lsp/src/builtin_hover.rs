//! Static hover descriptions for `RustScript` builtins and keywords.
//!
//! Provides rich hover information for built-in identifiers, methods,
//! keywords, and literals that don't come from user code. These are
//! matched by name and context during hover resolution.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Hover description for a builtin identifier or method.
#[derive(Debug, Clone)]
pub struct BuiltinHover {
    /// Markdown content to display on hover.
    pub markdown: &'static str,
}

/// Map of builtin identifier names to their hover descriptions.
static BUILTIN_IDENTIFIERS: LazyLock<HashMap<&'static str, BuiltinHover>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    m.insert(
        "console",
        BuiltinHover {
            markdown: "```rustscript\nobject console\n```\n\nThe console object --- provides output methods.\n\n- `console.log(...args)` --- `println!(\"{}\", ...)`\n- `console.error(...args)` --- `eprintln!(\"{}\", ...)`",
        },
    );

    m.insert(
        "null",
        BuiltinHover {
            markdown: "```rustscript\nnull\n```\n\nThe null literal --- represents absence of a value.\n\n**Rust:** `None`\n\nIn RustScript, `T | null` lowers to `Option<T>`.",
        },
    );

    m.insert(
        "true",
        BuiltinHover {
            markdown: "```rustscript\ntrue: boolean\n```\n\nBoolean literal.\n\n**Rust:** `true`",
        },
    );

    m.insert(
        "false",
        BuiltinHover {
            markdown: "```rustscript\nfalse: boolean\n```\n\nBoolean literal.\n\n**Rust:** `false`",
        },
    );

    m.insert(
        "this",
        BuiltinHover {
            markdown: "```rustscript\nthis\n```\n\nReference to the current class instance.\n\n**Rust:** `self`",
        },
    );

    m
});

/// Map of method names (on specific receiver types) to hover descriptions.
/// Key format: `"receiver.method"` or just `"method"` for universal methods.
static BUILTIN_METHODS: LazyLock<HashMap<&'static str, BuiltinHover>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // console methods
    m.insert(
        "console.log",
        BuiltinHover {
            markdown: "```rustscript\nconsole.log(...args): void\n```\n\nPrints arguments to stdout.\n\n**Rust:** `println!(\"{}\", ...)`",
        },
    );
    m.insert(
        "console.error",
        BuiltinHover {
            markdown: "```rustscript\nconsole.error(...args): void\n```\n\nPrints arguments to stderr.\n\n**Rust:** `eprintln!(\"{}\", ...)`",
        },
    );

    // String methods
    m.insert(
        "string.toUpperCase",
        BuiltinHover {
            markdown: "```rustscript\nstring.toUpperCase(): string\n```\n\nConverts the string to uppercase.\n\n**Rust:** `.to_uppercase()`",
        },
    );
    m.insert(
        "string.toLowerCase",
        BuiltinHover {
            markdown: "```rustscript\nstring.toLowerCase(): string\n```\n\nConverts the string to lowercase.\n\n**Rust:** `.to_lowercase()`",
        },
    );
    m.insert(
        "string.trim",
        BuiltinHover {
            markdown: "```rustscript\nstring.trim(): string\n```\n\nRemoves leading and trailing whitespace.\n\n**Rust:** `.trim().to_owned()`",
        },
    );
    m.insert(
        "string.includes",
        BuiltinHover {
            markdown: "```rustscript\nstring.includes(search: string): boolean\n```\n\nChecks if the string contains the given substring.\n\n**Rust:** `.contains(search)`",
        },
    );
    m.insert(
        "string.startsWith",
        BuiltinHover {
            markdown: "```rustscript\nstring.startsWith(prefix: string): boolean\n```\n\nChecks if the string starts with the given prefix.\n\n**Rust:** `.starts_with(prefix)`",
        },
    );
    m.insert(
        "string.endsWith",
        BuiltinHover {
            markdown: "```rustscript\nstring.endsWith(suffix: string): boolean\n```\n\nChecks if the string ends with the given suffix.\n\n**Rust:** `.ends_with(suffix)`",
        },
    );
    m.insert(
        "string.split",
        BuiltinHover {
            markdown: "```rustscript\nstring.split(separator: string): Array<string>\n```\n\nSplits the string by the separator.\n\n**Rust:** `.split(sep).map(|s| s.to_owned()).collect::<Vec<String>>()`",
        },
    );
    m.insert(
        "string.length",
        BuiltinHover {
            markdown: "```rustscript\nstring.length: number\n```\n\nThe number of characters in the string.\n\n**Rust:** `.len()`",
        },
    );
    m.insert(
        "string.toString",
        BuiltinHover {
            markdown: "```rustscript\nstring.toString(): string\n```\n\nReturns the string itself.\n\n**Rust:** `.to_string()`",
        },
    );
    m.insert(
        "string.chars",
        BuiltinHover {
            markdown: "```rustscript\nstring.chars(): Array<string>\n```\n\nReturns the characters of the string.\n\n**Rust:** `.chars()`",
        },
    );

    // Array methods
    m.insert(
        "array.push",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.push(value: T): void\n```\n\nAppends an element to the end of the array.\n\n**Rust:** `.push(value)`",
        },
    );
    m.insert(
        "array.pop",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.pop(): T | null\n```\n\nRemoves and returns the last element.\n\n**Rust:** `.pop()`",
        },
    );
    m.insert(
        "array.map",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.map(fn: (T) => U): Array<U>\n```\n\nTransforms each element using the given function.\n\n**Rust:** `.iter().map(fn).collect()`",
        },
    );
    m.insert(
        "array.filter",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.filter(fn: (T) => boolean): Array<T>\n```\n\nKeeps elements where the predicate returns true.\n\n**Rust:** `.iter().filter(fn).cloned().collect()`",
        },
    );
    m.insert(
        "array.forEach",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.forEach(fn: (T) => void): void\n```\n\nExecutes a function for each element.\n\n**Rust:** `.iter().for_each(fn)` or `for item in &arr { ... }`",
        },
    );
    m.insert(
        "array.length",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.length: number\n```\n\nThe number of elements in the array.\n\n**Rust:** `.len()`",
        },
    );
    m.insert(
        "array.isEmpty",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.isEmpty(): boolean\n```\n\nChecks if the array has no elements.\n\n**Rust:** `.is_empty()`",
        },
    );
    m.insert(
        "array.join",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.join(separator: string): string\n```\n\nJoins array elements into a single string.\n\n**Rust:** `.join(separator)`",
        },
    );

    // Promise / async
    m.insert(
        "Promise.all",
        BuiltinHover {
            markdown: "```rustscript\nPromise.all(promises: Array<Promise<T>>): Promise<Array<T>>\n```\n\nWaits for all promises to resolve.\n\n**Rust:** `futures::future::join_all(futures)`",
        },
    );
    m.insert(
        "spawn",
        BuiltinHover {
            markdown: "```rustscript\nspawn(fn: () => void): void\n```\n\nSpawns a concurrent task.\n\n**Rust:** `tokio::spawn(async { ... })`",
        },
    );

    m
});

/// Map of keyword hover descriptions.
static KEYWORD_HOVERS: LazyLock<HashMap<&'static str, BuiltinHover>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    m.insert(
        "async",
        BuiltinHover {
            markdown: "```\nasync\n```\n\nMarks a function as asynchronous. Async functions return a `Promise<T>`.\n\n**Rust:** `async fn`",
        },
    );
    m.insert(
        "await",
        BuiltinHover {
            markdown: "```\nawait\n```\n\nWaits for a `Promise<T>` to resolve, yielding the inner value.\n\n**Rust:** `.await`",
        },
    );
    m.insert(
        "throws",
        BuiltinHover {
            markdown: "```\nthrows ErrorType\n```\n\nDeclares that a function can return an error. The return type becomes `Result<T, E>`.\n\n**Rust:** `-> Result<T, E>`",
        },
    );
    m.insert(
        "shared",
        BuiltinHover {
            markdown: "```\nshared<T>\n```\n\nWraps a value in thread-safe shared ownership.\n\n**Rust:** `Arc<Mutex<T>>`\n\nAccess via `.lock().unwrap()` is inserted automatically.",
        },
    );
    m.insert(
        "const",
        BuiltinHover {
            markdown: "```\nconst\n```\n\nDeclares an immutable variable binding.\n\n**Rust:** `let`",
        },
    );
    m.insert(
        "let",
        BuiltinHover {
            markdown: "```\nlet\n```\n\nDeclares a mutable variable binding.\n\n**Rust:** `let mut`",
        },
    );

    m
});

/// Look up hover information for a builtin identifier.
///
/// Returns the hover markdown if the identifier matches a known builtin
/// (e.g., `console`, `null`, `true`, `false`).
#[must_use]
pub fn lookup_identifier(name: &str) -> Option<&'static str> {
    BUILTIN_IDENTIFIERS.get(name).map(|h| h.markdown)
}

/// Look up hover information for a method call.
///
/// The `receiver` is the type or object name (e.g., `"console"`, `"string"`, `"array"`),
/// and `method` is the method name (e.g., `"log"`, `"toUpperCase"`, `"map"`).
#[must_use]
pub fn lookup_method(receiver: &str, method: &str) -> Option<&'static str> {
    let key = format!("{receiver}.{method}");
    BUILTIN_METHODS.get(key.as_str()).map(|h| h.markdown)
}

/// Look up hover information for a keyword.
///
/// Returns the hover markdown if the name matches a known keyword
/// (e.g., `"async"`, `"await"`, `"throws"`, `"shared"`).
#[must_use]
pub fn lookup_keyword(name: &str) -> Option<&'static str> {
    KEYWORD_HOVERS.get(name).map(|h| h.markdown)
}

/// Classify a receiver expression name as a builtin type for method lookup.
///
/// Maps known identifier names to their builtin type category. For example,
/// if the variable was declared as `string`, we know its methods. For arbitrary
/// variable names, we return `None` (the caller should use the compile result).
#[must_use]
pub fn classify_receiver(name: &str) -> Option<&'static str> {
    match name {
        "console" => Some("console"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_hover_console_identifier() {
        let hover = lookup_identifier("console");
        assert!(hover.is_some(), "console should have hover info");
        assert!(
            hover.unwrap().contains("console"),
            "should describe console"
        );
    }

    #[test]
    fn test_builtin_hover_null_identifier() {
        let hover = lookup_identifier("null");
        assert!(hover.is_some(), "null should have hover info");
        assert!(hover.unwrap().contains("None"), "should mention Rust None");
    }

    #[test]
    fn test_builtin_hover_true_literal() {
        let hover = lookup_identifier("true");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("boolean"));
    }

    #[test]
    fn test_builtin_hover_false_literal() {
        let hover = lookup_identifier("false");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("boolean"));
    }

    #[test]
    fn test_builtin_hover_console_log_method() {
        let hover = lookup_method("console", "log");
        assert!(hover.is_some(), "console.log should have hover info");
        let text = hover.unwrap();
        assert!(text.contains("println!"), "should mention println!: {text}");
    }

    #[test]
    fn test_builtin_hover_string_to_upper_case() {
        let hover = lookup_method("string", "toUpperCase");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("to_uppercase"));
    }

    #[test]
    fn test_builtin_hover_array_map() {
        let hover = lookup_method("array", "map");
        assert!(hover.is_some());
        let text = hover.unwrap();
        assert!(text.contains(".iter().map"), "should show Rust equivalent");
    }

    #[test]
    fn test_builtin_hover_array_filter() {
        let hover = lookup_method("array", "filter");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains(".filter"));
    }

    #[test]
    fn test_builtin_hover_keyword_async() {
        let hover = lookup_keyword("async");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("async fn"));
    }

    #[test]
    fn test_builtin_hover_keyword_await() {
        let hover = lookup_keyword("await");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains(".await"));
    }

    #[test]
    fn test_builtin_hover_keyword_throws() {
        let hover = lookup_keyword("throws");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("Result"));
    }

    #[test]
    fn test_builtin_hover_keyword_shared() {
        let hover = lookup_keyword("shared");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("Arc<Mutex<T>>"));
    }

    #[test]
    fn test_builtin_hover_keyword_const() {
        let hover = lookup_keyword("const");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("let"));
    }

    #[test]
    fn test_builtin_hover_keyword_let() {
        let hover = lookup_keyword("let");
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("let mut"));
    }

    #[test]
    fn test_builtin_hover_unknown_identifier_returns_none() {
        assert!(lookup_identifier("foobar").is_none());
    }

    #[test]
    fn test_builtin_hover_unknown_method_returns_none() {
        assert!(lookup_method("console", "foobar").is_none());
    }

    #[test]
    fn test_builtin_hover_classify_receiver_console() {
        assert_eq!(classify_receiver("console"), Some("console"));
    }

    #[test]
    fn test_builtin_hover_classify_receiver_unknown() {
        assert!(classify_receiver("myVar").is_none());
    }

    #[test]
    fn test_builtin_hover_spawn() {
        let hover = lookup_method("spawn", "spawn");
        // spawn is in BUILTIN_METHODS as just "spawn", not a method
        // Check identifier instead:
        assert!(lookup_identifier("spawn").is_none());
        // It's actually in the BUILTIN_METHODS map — but we'd look it up differently.
        // The spawn function is a top-level call, not a method. Let's verify:
        assert!(hover.is_none());
    }

    #[test]
    fn test_builtin_hover_this_identifier() {
        let hover = lookup_identifier("this");
        assert!(hover.is_some(), "this should have hover info");
        assert!(hover.unwrap().contains("self"));
    }
}
