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
    m.insert(
        "string.replace",
        BuiltinHover {
            markdown: "```rustscript\nstring.replace(search: string, replacement: string): string\n```\n\nReplaces the first occurrence of a substring.\n\n**Rust:** `.replacen(search, replacement, 1)`",
        },
    );
    m.insert(
        "string.replaceAll",
        BuiltinHover {
            markdown: "```rustscript\nstring.replaceAll(search: string, replacement: string): string\n```\n\nReplaces all occurrences of a substring.\n\n**Rust:** `.replace(search, replacement)`",
        },
    );
    m.insert(
        "string.charAt",
        BuiltinHover {
            markdown: "```rustscript\nstring.charAt(index: i32): string\n```\n\nReturns the character at the given index.\n\n**Rust:** `.chars().nth(index)`",
        },
    );
    m.insert(
        "string.charCodeAt",
        BuiltinHover {
            markdown: "```rustscript\nstring.charCodeAt(index: i32): i32\n```\n\nReturns the Unicode code point at the given index.\n\n**Rust:** `.chars().nth(index).map(|c| c as u32)`",
        },
    );
    m.insert(
        "string.indexOf",
        BuiltinHover {
            markdown: "```rustscript\nstring.indexOf(search: string): i32\n```\n\nReturns the index of the first occurrence of a substring, or -1.\n\n**Rust:** `.find(search).map_or(-1, |i| i as i64)`",
        },
    );
    m.insert(
        "string.lastIndexOf",
        BuiltinHover {
            markdown: "```rustscript\nstring.lastIndexOf(search: string): i32\n```\n\nReturns the index of the last occurrence of a substring, or -1.\n\n**Rust:** `.rfind(search).map_or(-1, |i| i as i64)`",
        },
    );
    m.insert(
        "string.slice",
        BuiltinHover {
            markdown: "```rustscript\nstring.slice(start: i32, end?: i32): string\n```\n\nExtracts a section of the string.\n\n**Rust:** `s[start..end].to_owned()`",
        },
    );
    m.insert(
        "string.substring",
        BuiltinHover {
            markdown: "```rustscript\nstring.substring(start: i32, end?: i32): string\n```\n\nExtracts characters between two indices.\n\n**Rust:** `s[start..end].to_owned()`",
        },
    );
    m.insert(
        "string.padStart",
        BuiltinHover {
            markdown: "```rustscript\nstring.padStart(length: i32, fill?: string): string\n```\n\nPads the string from the start to the target length.\n\n**Rust:** `format!(\"{:>width$}\", s, width = length)`",
        },
    );
    m.insert(
        "string.padEnd",
        BuiltinHover {
            markdown: "```rustscript\nstring.padEnd(length: i32, fill?: string): string\n```\n\nPads the string from the end to the target length.\n\n**Rust:** `format!(\"{:<width$}\", s, width = length)`",
        },
    );
    m.insert(
        "string.repeat",
        BuiltinHover {
            markdown: "```rustscript\nstring.repeat(count: i32): string\n```\n\nRepeats the string the given number of times.\n\n**Rust:** `.repeat(count)`",
        },
    );
    m.insert(
        "string.concat",
        BuiltinHover {
            markdown: "```rustscript\nstring.concat(other: string): string\n```\n\nConcatenates two strings.\n\n**Rust:** `format!(\"{}{}\", s, other)`",
        },
    );
    m.insert(
        "string.at",
        BuiltinHover {
            markdown: "```rustscript\nstring.at(index: i32): string | null\n```\n\nReturns the character at the given index (supports negative indices).\n\n**Rust:** `.chars().nth(index)`",
        },
    );
    m.insert(
        "string.trimStart",
        BuiltinHover {
            markdown: "```rustscript\nstring.trimStart(): string\n```\n\nRemoves leading whitespace.\n\n**Rust:** `.trim_start().to_owned()`",
        },
    );
    m.insert(
        "string.trimEnd",
        BuiltinHover {
            markdown: "```rustscript\nstring.trimEnd(): string\n```\n\nRemoves trailing whitespace.\n\n**Rust:** `.trim_end().to_owned()`",
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
    m.insert(
        "array.reduce",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.reduce(fn: (acc: U, item: T) => U, initial: U): U\n```\n\nReduces the array to a single value.\n\n**Rust:** `.iter().fold(initial, fn)`",
        },
    );
    m.insert(
        "array.find",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.find(fn: (T) => boolean): T | null\n```\n\nReturns the first element matching the predicate.\n\n**Rust:** `.iter().find(fn).cloned()`",
        },
    );
    m.insert(
        "array.findIndex",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.findIndex(fn: (T) => boolean): i32\n```\n\nReturns the index of the first element matching the predicate, or -1.\n\n**Rust:** `.iter().position(fn).map_or(-1, |i| i as i64)`",
        },
    );
    m.insert(
        "array.findLast",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.findLast(fn: (T) => boolean): T | null\n```\n\nReturns the last element matching the predicate.\n\n**Rust:** `.iter().rev().find(fn).cloned()`",
        },
    );
    m.insert(
        "array.findLastIndex",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.findLastIndex(fn: (T) => boolean): i32\n```\n\nReturns the index of the last element matching the predicate, or -1.\n\n**Rust:** `.iter().rposition(fn).map_or(-1, |i| i as i64)`",
        },
    );
    m.insert(
        "array.some",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.some(fn: (T) => boolean): boolean\n```\n\nReturns true if any element matches the predicate.\n\n**Rust:** `.iter().any(fn)`",
        },
    );
    m.insert(
        "array.every",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.every(fn: (T) => boolean): boolean\n```\n\nReturns true if all elements match the predicate.\n\n**Rust:** `.iter().all(fn)`",
        },
    );
    m.insert(
        "array.flat",
        BuiltinHover {
            markdown: "```rustscript\nArray<Array<T>>.flat(): Array<T>\n```\n\nFlattens one level of nesting.\n\n**Rust:** `.into_iter().flatten().collect()`",
        },
    );
    m.insert(
        "array.flatMap",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.flatMap(fn: (T) => Array<U>): Array<U>\n```\n\nMaps then flattens one level.\n\n**Rust:** `.iter().flat_map(fn).collect()`",
        },
    );
    m.insert(
        "array.shift",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.shift(): T | null\n```\n\nRemoves and returns the first element.\n\n**Rust:** `if arr.is_empty() { None } else { Some(arr.remove(0)) }`",
        },
    );
    m.insert(
        "array.unshift",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.unshift(value: T): void\n```\n\nInserts an element at the beginning.\n\n**Rust:** `.insert(0, value)`",
        },
    );
    m.insert(
        "array.reverse",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.reverse(): void\n```\n\nReverses the array in place.\n\n**Rust:** `.reverse()`",
        },
    );
    m.insert(
        "array.sort",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.sort(): void\n```\n\nSorts the array in place.\n\n**Rust:** `.sort()` for `Ord` types",
        },
    );
    m.insert(
        "array.fill",
        BuiltinHover {
            markdown: "```rustscript\nArray<T>.fill(value: T): void\n```\n\nFills the array with the given value.\n\n**Rust:** `.fill(value)`",
        },
    );

    // Map/Set methods
    m.insert(
        "map.get",
        BuiltinHover {
            markdown: "```rustscript\nMap<K, V>.get(key: K): V | null\n```\n\nReturns the value for the given key, or null.\n\n**Rust:** `.get(&key).cloned()`",
        },
    );
    m.insert(
        "map.set",
        BuiltinHover {
            markdown: "```rustscript\nMap<K, V>.set(key: K, value: V): void\n```\n\nSets a key-value pair.\n\n**Rust:** `.insert(key, value)`",
        },
    );
    m.insert(
        "map.has",
        BuiltinHover {
            markdown: "```rustscript\nMap<K, V>.has(key: K): boolean\n```\n\nChecks if the map contains the key.\n\n**Rust:** `.contains_key(&key)`",
        },
    );
    m.insert(
        "map.delete",
        BuiltinHover {
            markdown: "```rustscript\nMap<K, V>.delete(key: K): boolean\n```\n\nRemoves the entry for the given key.\n\n**Rust:** `.remove(&key).is_some()`",
        },
    );
    m.insert(
        "map.clear",
        BuiltinHover {
            markdown: "```rustscript\nMap<K, V>.clear(): void\n```\n\nRemoves all entries.\n\n**Rust:** `.clear()`",
        },
    );
    m.insert(
        "map.keys",
        BuiltinHover {
            markdown: "```rustscript\nMap<K, V>.keys(): Array<K>\n```\n\nReturns the keys as an array.\n\n**Rust:** `.keys().cloned().collect()`",
        },
    );
    m.insert(
        "map.values",
        BuiltinHover {
            markdown: "```rustscript\nMap<K, V>.values(): Array<V>\n```\n\nReturns the values as an array.\n\n**Rust:** `.values().cloned().collect()`",
        },
    );
    m.insert(
        "map.entries",
        BuiltinHover {
            markdown: "```rustscript\nMap<K, V>.entries(): Array<[K, V]>\n```\n\nReturns the entries as an array of key-value pairs.\n\n**Rust:** `.iter().map(|(k, v)| (k.clone(), v.clone())).collect()`",
        },
    );
    m.insert(
        "set.add",
        BuiltinHover {
            markdown: "```rustscript\nSet<T>.add(value: T): void\n```\n\nAdds a value to the set.\n\n**Rust:** `.insert(value)`",
        },
    );
    m.insert(
        "set.has",
        BuiltinHover {
            markdown: "```rustscript\nSet<T>.has(value: T): boolean\n```\n\nChecks if the set contains the value.\n\n**Rust:** `.contains(&value)`",
        },
    );
    m.insert(
        "set.delete",
        BuiltinHover {
            markdown: "```rustscript\nSet<T>.delete(value: T): boolean\n```\n\nRemoves the value from the set.\n\n**Rust:** `.remove(&value)`",
        },
    );
    m.insert(
        "set.clear",
        BuiltinHover {
            markdown: "```rustscript\nSet<T>.clear(): void\n```\n\nRemoves all values.\n\n**Rust:** `.clear()`",
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
        "Promise.race",
        BuiltinHover {
            markdown: "```rustscript\nPromise.race(promises: Array<Promise<T>>): Promise<T>\n```\n\nRuns all promises concurrently, returns the first to complete.\n\n**Rust:** `tokio::select! { ... }`",
        },
    );
    m.insert(
        "Promise.any",
        BuiltinHover {
            markdown: "```rustscript\nPromise.any(promises: Array<Promise<T>>): Promise<T>\n```\n\nRuns all promises concurrently, returns the first to succeed.\n\n**Rust:** `futures::future::select_ok(...)`",
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
    m.insert(
        "readonly",
        BuiltinHover {
            markdown: "```\nreadonly\n```\n\nMarks a field as immutable after construction.\n\n**Rust:** The field is only set in the constructor; no setter is generated.",
        },
    );
    m.insert(
        "static",
        BuiltinHover {
            markdown: "```\nstatic\n```\n\nDeclares a class member that belongs to the class itself, not instances.\n\n**Rust:** Associated function or `const`/`static` on the `impl` block.",
        },
    );
    m.insert(
        "get",
        BuiltinHover {
            markdown: "```\nget\n```\n\nProperty getter accessor. Accessed as `obj.prop`.\n\n**Rust:** Getter method `fn prop(&self) -> T`.",
        },
    );
    m.insert(
        "set",
        BuiltinHover {
            markdown: "```\nset\n```\n\nProperty setter accessor. Assigned as `obj.prop = value`.\n\n**Rust:** Setter method `fn set_prop(&mut self, value: T)`.",
        },
    );
    m.insert(
        "as",
        BuiltinHover {
            markdown: "```\nas\n```\n\nType cast operator.\n\n**Rust:** `as` operator for numeric casts.",
        },
    );
    m.insert(
        "typeof",
        BuiltinHover {
            markdown: "```\ntypeof\n```\n\nReturns the type name of a value as a string. Resolved statically at compile time.",
        },
    );
    m.insert(
        "finally",
        BuiltinHover {
            markdown: "```\nfinally\n```\n\nExecutes after try/catch regardless of outcome. Cleanup block.",
        },
    );
    m.insert(
        "for await",
        BuiltinHover {
            markdown: "```\nfor await\n```\n\nAsync iteration --- iterates over an async stream.\n\n**Rust:** `while let Some(item) = stream.next().await { ... }`",
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

    // -----------------------------------------------------------------------
    // Task 062: Phase 5 keyword hover tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_builtin_hover_keyword_readonly() {
        let hover = lookup_keyword("readonly");
        assert!(hover.is_some(), "readonly should have hover info");
        assert!(hover.unwrap().contains("immutable"));
    }

    #[test]
    fn test_builtin_hover_keyword_static() {
        let hover = lookup_keyword("static");
        assert!(hover.is_some(), "static should have hover info");
        assert!(hover.unwrap().contains("class itself"));
    }

    #[test]
    fn test_builtin_hover_keyword_get() {
        let hover = lookup_keyword("get");
        assert!(hover.is_some(), "get should have hover info");
        assert!(hover.unwrap().contains("getter"));
    }

    #[test]
    fn test_builtin_hover_keyword_set() {
        let hover = lookup_keyword("set");
        assert!(hover.is_some(), "set should have hover info");
        assert!(hover.unwrap().contains("setter"));
    }

    #[test]
    fn test_builtin_hover_keyword_as() {
        let hover = lookup_keyword("as");
        assert!(hover.is_some(), "as should have hover info");
        assert!(hover.unwrap().contains("cast"));
    }

    #[test]
    fn test_builtin_hover_keyword_typeof() {
        let hover = lookup_keyword("typeof");
        assert!(hover.is_some(), "typeof should have hover info");
        assert!(hover.unwrap().contains("type name"));
    }

    #[test]
    fn test_builtin_hover_keyword_finally() {
        let hover = lookup_keyword("finally");
        assert!(hover.is_some(), "finally should have hover info");
        assert!(hover.unwrap().contains("regardless"));
    }

    // ---------------------------------------------------------------
    // Task 066: Async iteration and Promise methods
    // ---------------------------------------------------------------

    #[test]
    fn test_builtin_hover_promise_race() {
        let hover = lookup_method("Promise", "race");
        assert!(hover.is_some(), "Promise.race should have hover info");
        let text = hover.unwrap();
        assert!(
            text.contains("tokio::select!"),
            "should mention tokio::select!: {text}"
        );
    }

    #[test]
    fn test_builtin_hover_promise_any() {
        let hover = lookup_method("Promise", "any");
        assert!(hover.is_some(), "Promise.any should have hover info");
        let text = hover.unwrap();
        assert!(
            text.contains("select_ok"),
            "should mention select_ok: {text}"
        );
    }

    #[test]
    fn test_builtin_hover_keyword_for_await() {
        let hover = lookup_keyword("for await");
        assert!(hover.is_some(), "for await should have hover info");
        let text = hover.unwrap();
        assert!(
            text.contains("stream.next().await"),
            "should mention stream.next().await: {text}"
        );
    }
}
