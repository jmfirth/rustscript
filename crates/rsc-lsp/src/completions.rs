//! Completion and signature help logic for the `RustScript` LSP.
//!
//! Provides keyword completions with snippets, builtin object member
//! completions (e.g., `Math.`, `console.`), type-aware member completions
//! for variables whose types are known from the compile cache, import
//! completions from the rustdoc cache, and signature help for function calls.

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind, ParameterInformation, ParameterLabel, SignatureHelp,
    SignatureInformation,
};

use crate::server::CachedCompileInfo;
use rsc_driver::rustdoc_cache::RustdocCache;
use rsc_driver::rustdoc_parser::RustdocItemKind;

// ---------------------------------------------------------------------------
// 1. Keyword completions
// ---------------------------------------------------------------------------

/// A keyword completion template.
struct KeywordSnippet {
    /// The keyword label shown in the completion list.
    label: &'static str,
    /// The snippet body inserted on accept (uses `${}` placeholders).
    snippet: &'static str,
    /// Brief detail text shown alongside the label.
    detail: &'static str,
}

/// All `RustScript` keyword snippet templates.
const KEYWORD_SNIPPETS: &[KeywordSnippet] = &[
    KeywordSnippet {
        label: "function",
        snippet: "function ${1:name}(${2:params}): ${3:void} {\n\t$0\n}",
        detail: "Function declaration",
    },
    KeywordSnippet {
        label: "const",
        snippet: "const ${1:name} = $0;",
        detail: "Immutable variable",
    },
    KeywordSnippet {
        label: "let",
        snippet: "let ${1:name} = $0;",
        detail: "Mutable variable",
    },
    KeywordSnippet {
        label: "if",
        snippet: "if (${1:condition}) {\n\t$0\n}",
        detail: "If statement",
    },
    KeywordSnippet {
        label: "else",
        snippet: "else {\n\t$0\n}",
        detail: "Else clause",
    },
    KeywordSnippet {
        label: "while",
        snippet: "while (${1:condition}) {\n\t$0\n}",
        detail: "While loop",
    },
    KeywordSnippet {
        label: "for",
        snippet: "for (const ${1:item} of ${2:items}) {\n\t$0\n}",
        detail: "For-of loop",
    },
    KeywordSnippet {
        label: "class",
        snippet: "class ${1:Name} {\n\tconstructor(${2:params}) {\n\t\t$0\n\t}\n}",
        detail: "Class declaration",
    },
    KeywordSnippet {
        label: "interface",
        snippet: "interface ${1:Name} {\n\t$0\n}",
        detail: "Interface declaration",
    },
    KeywordSnippet {
        label: "type",
        snippet: "type ${1:Name} = {\n\t$0\n}",
        detail: "Type alias",
    },
    KeywordSnippet {
        label: "import",
        snippet: "import { $1 } from \"${2:module}\";",
        detail: "Import declaration",
    },
    KeywordSnippet {
        label: "export",
        snippet: "export $0",
        detail: "Export declaration",
    },
    KeywordSnippet {
        label: "async",
        snippet: "async function ${1:name}(${2:params}) {\n\t$0\n}",
        detail: "Async function declaration",
    },
    KeywordSnippet {
        label: "try",
        snippet: "try {\n\t$0\n} catch (${1:err}: ${2:string}) {\n\t\n}",
        detail: "Try-catch block",
    },
    KeywordSnippet {
        label: "switch",
        snippet: "switch (${1:expr}) {\n\tcase \"${2:value}\":\n\t\t$0\n}",
        detail: "Switch statement",
    },
    KeywordSnippet {
        label: "test",
        snippet: "test(\"${1:description}\", () => {\n\t$0\n});",
        detail: "Test case",
    },
    KeywordSnippet {
        label: "describe",
        snippet: "describe(\"${1:suite}\", () => {\n\t$0\n});",
        detail: "Test suite",
    },
    KeywordSnippet {
        label: "return",
        snippet: "return $0;",
        detail: "Return statement",
    },
    KeywordSnippet {
        label: "throw",
        snippet: "throw $0;",
        detail: "Throw expression",
    },
];

/// Build the full list of keyword completion items.
#[must_use]
pub fn keyword_completions() -> Vec<CompletionItem> {
    KEYWORD_SNIPPETS
        .iter()
        .map(|ks| CompletionItem {
            label: ks.label.to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(ks.detail.to_owned()),
            insert_text: Some(ks.snippet.to_owned()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 2. Builtin object completions
// ---------------------------------------------------------------------------

/// A member of a builtin object (method or constant).
struct BuiltinMember {
    /// Label shown in the completion list.
    label: &'static str,
    /// Brief type signature shown as detail.
    detail: &'static str,
    /// Documentation string.
    doc: &'static str,
    /// The completion item kind (method, property, constant, etc.).
    kind: CompletionItemKind,
}

/// `Math` object members.
const MATH_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "floor",
        detail: "(x: number): number",
        doc: "Rounds down to the nearest integer.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "ceil",
        detail: "(x: number): number",
        doc: "Rounds up to the nearest integer.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "round",
        detail: "(x: number): number",
        doc: "Rounds to the nearest integer.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "abs",
        detail: "(x: number): number",
        doc: "Returns the absolute value.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "sqrt",
        detail: "(x: number): number",
        doc: "Returns the square root.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "min",
        detail: "(a: number, b: number): number",
        doc: "Returns the smaller of two values.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "max",
        detail: "(a: number, b: number): number",
        doc: "Returns the larger of two values.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "random",
        detail: "(): number",
        doc: "Returns a random f64 in [0, 1).",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "sin",
        detail: "(x: number): number",
        doc: "Returns the sine.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "cos",
        detail: "(x: number): number",
        doc: "Returns the cosine.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "tan",
        detail: "(x: number): number",
        doc: "Returns the tangent.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "log",
        detail: "(x: number): number",
        doc: "Returns the natural logarithm.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "pow",
        detail: "(base: number, exp: number): number",
        doc: "Returns base raised to the power of exp.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "PI",
        detail: "number",
        doc: "The ratio of a circle's circumference to its diameter (~3.14159).",
        kind: CompletionItemKind::CONSTANT,
    },
    BuiltinMember {
        label: "E",
        detail: "number",
        doc: "Euler's number (~2.71828).",
        kind: CompletionItemKind::CONSTANT,
    },
];

/// `console` object members.
const CONSOLE_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "log",
        detail: "(...args): void",
        doc: "Prints arguments to stdout.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "error",
        detail: "(...args): void",
        doc: "Prints arguments to stderr.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "warn",
        detail: "(...args): void",
        doc: "Prints a warning to stderr.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "debug",
        detail: "(...args): void",
        doc: "Prints debug output to stderr.",
        kind: CompletionItemKind::METHOD,
    },
];

/// `JSON` object members.
const JSON_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "parse",
        detail: "(str: string): any",
        doc: "Deserializes a JSON string to a value.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "stringify",
        detail: "(obj: any): string",
        doc: "Serializes a value to a JSON string.",
        kind: CompletionItemKind::METHOD,
    },
];

/// `Number` object members.
const NUMBER_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "parseInt",
        detail: "(str: string): number",
        doc: "Parses a string as an integer.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "parseFloat",
        detail: "(str: string): number",
        doc: "Parses a string as a float.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "isNaN",
        detail: "(x: number): boolean",
        doc: "Checks if the value is NaN.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "isFinite",
        detail: "(x: number): boolean",
        doc: "Checks if the value is finite.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "isInteger",
        detail: "(x: number): boolean",
        doc: "Checks if the value is an integer.",
        kind: CompletionItemKind::METHOD,
    },
];

/// `Object` utility members.
const OBJECT_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "keys",
        detail: "(map: Map<K, V>): Array<K>",
        doc: "Returns the keys of a map as an array.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "values",
        detail: "(map: Map<K, V>): Array<V>",
        doc: "Returns the values of a map as an array.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "entries",
        detail: "(map: Map<K, V>): Array<[K, V]>",
        doc: "Returns the entries of a map as key-value pairs.",
        kind: CompletionItemKind::METHOD,
    },
];

/// Convert a slice of [`BuiltinMember`] to completion items.
fn builtin_members_to_completions(members: &[BuiltinMember]) -> Vec<CompletionItem> {
    members
        .iter()
        .map(|m| CompletionItem {
            label: m.label.to_owned(),
            kind: Some(m.kind),
            detail: Some(m.detail.to_owned()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: m.doc.to_owned(),
            })),
            ..Default::default()
        })
        .collect()
}

/// Return completions for a builtin object's members.
///
/// Given a receiver like `"Math"`, `"console"`, `"JSON"`, etc., returns
/// the appropriate member list.
#[must_use]
pub fn builtin_object_completions(object_name: &str) -> Option<Vec<CompletionItem>> {
    let members = match object_name {
        "Math" => MATH_MEMBERS,
        "console" => CONSOLE_MEMBERS,
        "JSON" => JSON_MEMBERS,
        "Number" => NUMBER_MEMBERS,
        "Object" => OBJECT_MEMBERS,
        _ => return None,
    };
    Some(builtin_members_to_completions(members))
}

// ---------------------------------------------------------------------------
// 3. Type-aware member completions
// ---------------------------------------------------------------------------

/// String method members for type-aware completion.
const STRING_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "toUpperCase",
        detail: "(): string",
        doc: "Converts the string to uppercase.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "toLowerCase",
        detail: "(): string",
        doc: "Converts the string to lowercase.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "trim",
        detail: "(): string",
        doc: "Removes leading and trailing whitespace.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "trimStart",
        detail: "(): string",
        doc: "Removes leading whitespace.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "trimEnd",
        detail: "(): string",
        doc: "Removes trailing whitespace.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "startsWith",
        detail: "(prefix: string): boolean",
        doc: "Checks if the string starts with the given prefix.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "endsWith",
        detail: "(suffix: string): boolean",
        doc: "Checks if the string ends with the given suffix.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "includes",
        detail: "(search: string): boolean",
        doc: "Checks if the string contains the given substring.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "split",
        detail: "(separator: string): Array<string>",
        doc: "Splits the string by the separator.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "replace",
        detail: "(search: string, replacement: string): string",
        doc: "Replaces the first occurrence of a substring.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "replaceAll",
        detail: "(search: string, replacement: string): string",
        doc: "Replaces all occurrences of a substring.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "charAt",
        detail: "(index: i32): string",
        doc: "Returns the character at the given index.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "indexOf",
        detail: "(search: string): i32",
        doc: "Returns the index of the first occurrence, or -1.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "lastIndexOf",
        detail: "(search: string): i32",
        doc: "Returns the index of the last occurrence, or -1.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "slice",
        detail: "(start: i32, end?: i32): string",
        doc: "Extracts a section of the string.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "substring",
        detail: "(start: i32, end?: i32): string",
        doc: "Extracts characters between two indices.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "repeat",
        detail: "(count: i32): string",
        doc: "Repeats the string the given number of times.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "padStart",
        detail: "(length: i32, fill?: string): string",
        doc: "Pads the string from the start.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "padEnd",
        detail: "(length: i32, fill?: string): string",
        doc: "Pads the string from the end.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "concat",
        detail: "(other: string): string",
        doc: "Concatenates two strings.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "length",
        detail: "number",
        doc: "The number of characters in the string.",
        kind: CompletionItemKind::PROPERTY,
    },
];

/// Array method members for type-aware completion.
const ARRAY_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "map",
        detail: "(fn: (T) => U): Array<U>",
        doc: "Transforms each element using the given function.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "filter",
        detail: "(fn: (T) => boolean): Array<T>",
        doc: "Keeps elements where the predicate returns true.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "reduce",
        detail: "(fn: (acc: U, item: T) => U, initial: U): U",
        doc: "Reduces the array to a single value.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "forEach",
        detail: "(fn: (T) => void): void",
        doc: "Executes a function for each element.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "find",
        detail: "(fn: (T) => boolean): T | null",
        doc: "Returns the first element matching the predicate.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "findIndex",
        detail: "(fn: (T) => boolean): i32",
        doc: "Returns the index of the first matching element, or -1.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "some",
        detail: "(fn: (T) => boolean): boolean",
        doc: "Returns true if any element matches.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "every",
        detail: "(fn: (T) => boolean): boolean",
        doc: "Returns true if all elements match.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "push",
        detail: "(value: T): void",
        doc: "Appends an element to the end.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "pop",
        detail: "(): T | null",
        doc: "Removes and returns the last element.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "shift",
        detail: "(): T | null",
        doc: "Removes and returns the first element.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "unshift",
        detail: "(value: T): void",
        doc: "Inserts an element at the beginning.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "join",
        detail: "(separator: string): string",
        doc: "Joins array elements into a string.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "flat",
        detail: "(): Array<T>",
        doc: "Flattens one level of nesting.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "flatMap",
        detail: "(fn: (T) => Array<U>): Array<U>",
        doc: "Maps then flattens one level.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "reverse",
        detail: "(): void",
        doc: "Reverses the array in place.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "sort",
        detail: "(): void",
        doc: "Sorts the array in place.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "fill",
        detail: "(value: T): void",
        doc: "Fills the array with the given value.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "length",
        detail: "number",
        doc: "The number of elements in the array.",
        kind: CompletionItemKind::PROPERTY,
    },
];

/// Map method members for type-aware completion.
const MAP_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "get",
        detail: "(key: K): V | null",
        doc: "Returns the value for the given key, or null.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "set",
        detail: "(key: K, value: V): void",
        doc: "Sets a key-value pair.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "has",
        detail: "(key: K): boolean",
        doc: "Checks if the map contains the key.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "delete",
        detail: "(key: K): boolean",
        doc: "Removes the entry for the given key.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "keys",
        detail: "(): Array<K>",
        doc: "Returns the keys as an array.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "values",
        detail: "(): Array<V>",
        doc: "Returns the values as an array.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "entries",
        detail: "(): Array<[K, V]>",
        doc: "Returns entries as key-value pairs.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "clear",
        detail: "(): void",
        doc: "Removes all entries.",
        kind: CompletionItemKind::METHOD,
    },
];

/// Set method members for type-aware completion.
const SET_MEMBERS: &[BuiltinMember] = &[
    BuiltinMember {
        label: "add",
        detail: "(value: T): void",
        doc: "Adds a value to the set.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "has",
        detail: "(value: T): boolean",
        doc: "Checks if the set contains the value.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "delete",
        detail: "(value: T): boolean",
        doc: "Removes the value from the set.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "clear",
        detail: "(): void",
        doc: "Removes all values.",
        kind: CompletionItemKind::METHOD,
    },
    BuiltinMember {
        label: "values",
        detail: "(): Array<T>",
        doc: "Returns all values as an array.",
        kind: CompletionItemKind::METHOD,
    },
];

/// Return type-aware member completions for a variable whose type is known.
///
/// Given a `RustScript` type string (e.g., `"string"`, `"Array<i32>"`, `"Map<K, V>"`),
/// returns appropriate member completions. Returns `None` if the type is not
/// a recognized builtin category.
#[must_use]
pub fn type_member_completions(type_str: &str) -> Option<Vec<CompletionItem>> {
    let members = if type_str == "String" || type_str == "string" {
        STRING_MEMBERS
    } else if type_str.starts_with("Vec<") || type_str.starts_with("Array<") {
        ARRAY_MEMBERS
    } else if type_str.starts_with("HashMap<") || type_str.starts_with("Map<") {
        MAP_MEMBERS
    } else if type_str.starts_with("HashSet<") || type_str.starts_with("Set<") {
        SET_MEMBERS
    } else {
        return None;
    };
    Some(builtin_members_to_completions(members))
}

/// Return struct/class field and method completions from the compile cache.
///
/// Checks `variable_types` for struct-like type info (fields embedded in the
/// type string as `type X = { field: T, ... }`), and also checks for class
/// methods stored in `function_signatures` with a matching receiver prefix.
#[must_use]
pub fn struct_member_completions(
    variable_name: &str,
    cache: &CachedCompileInfo,
) -> Option<Vec<CompletionItem>> {
    let type_str = cache.variable_types.get(variable_name)?;

    // Check if this is a known struct type by looking for its definition
    // in the variable_types map (struct definitions are stored as
    // `type Name = { field: T, ... }`).
    let struct_def = cache
        .variable_types
        .iter()
        .find(|(_, v)| v.starts_with("type ") && v.contains(&format!("type {type_str} =")));

    let mut items = Vec::new();

    if let Some((_, def)) = struct_def {
        // Parse `type Name = { field1: T1, field2: T2 }` to extract fields.
        if let Some(fields_str) = def
            .strip_prefix(&format!("type {type_str} = {{ "))
            .and_then(|s| s.strip_suffix(" }"))
        {
            for field in fields_str.split(", ") {
                if let Some((name, ty)) = field.split_once(": ") {
                    items.push(CompletionItem {
                        label: name.to_owned(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(ty.to_owned()),
                        ..Default::default()
                    });
                }
            }
        }
    }

    // Look for class methods: function_signatures keys like `ClassName.methodName`.
    for (sig_name, sig) in &cache.function_signatures {
        if let Some(method_name) = sig_name.strip_prefix(&format!("{type_str}.")) {
            items.push(CompletionItem {
                label: method_name.to_owned(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(sig.clone()),
                ..Default::default()
            });
        }
    }

    if items.is_empty() { None } else { Some(items) }
}

// ---------------------------------------------------------------------------
// 4. Signature help
// ---------------------------------------------------------------------------

/// Build a [`SignatureHelp`] response for a function call at the cursor.
///
/// Parses the function name from the source text, looks it up in the compile
/// cache's `function_signatures`, and returns parameter labels.
#[must_use]
pub fn signature_help_for_function(
    function_name: &str,
    active_param: u32,
    cache: &CachedCompileInfo,
) -> Option<SignatureHelp> {
    let sig_str = cache.function_signatures.get(function_name)?;

    // Parse the signature: `function name(p1: T1, p2: T2): ReturnType`
    // Extract the parameters between `(` and `)`.
    let open_paren = sig_str.find('(')?;
    let close_paren = sig_str.rfind(')')?;
    let params_str = &sig_str[open_paren + 1..close_paren];

    let params: Vec<ParameterInformation> = if params_str.is_empty() {
        Vec::new()
    } else {
        params_str
            .split(", ")
            .map(|p| ParameterInformation {
                label: ParameterLabel::Simple(p.to_owned()),
                documentation: None,
            })
            .collect()
    };

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: sig_str.clone(),
            documentation: None,
            parameters: Some(params),
            active_parameter: Some(active_param),
        }],
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}

// ---------------------------------------------------------------------------
// 5. Import completions
// ---------------------------------------------------------------------------

/// Return completions for items available from a crate via the rustdoc cache.
///
/// Used inside `import { | } from "crate"` to suggest available exports.
#[must_use]
pub fn import_completions(crate_name: &str, rustdoc: &RustdocCache) -> Option<Vec<CompletionItem>> {
    let crate_data = rustdoc.get_crate(crate_name)?;

    let mut items: Vec<CompletionItem> = crate_data
        .name_index
        .keys()
        .filter_map(|name| {
            let ids = crate_data.name_index.get(name)?;
            let first_id = ids.first()?;
            let item = crate_data.items.get(first_id)?;

            let kind = match &item.kind {
                RustdocItemKind::Function(_) => CompletionItemKind::FUNCTION,
                RustdocItemKind::Struct(_) => CompletionItemKind::STRUCT,
                RustdocItemKind::Trait(_) => CompletionItemKind::INTERFACE,
                RustdocItemKind::Enum(_) => CompletionItemKind::ENUM,
            };

            Some(CompletionItem {
                label: name.clone(),
                kind: Some(kind),
                detail: item.docs.as_ref().map(|d| {
                    // Truncate long doc strings for detail view.
                    if d.len() > 80 {
                        format!("{}...", &d[..77])
                    } else {
                        d.clone()
                    }
                }),
                ..Default::default()
            })
        })
        .collect();

    items.sort_by(|a, b| a.label.cmp(&b.label));

    if items.is_empty() { None } else { Some(items) }
}

// ---------------------------------------------------------------------------
// High-level completion resolution
// ---------------------------------------------------------------------------

/// Context needed to resolve completions.
pub struct CompletionContext<'a> {
    /// The full source text of the document.
    pub source: &'a str,
    /// Cursor line (0-based).
    pub line: u32,
    /// Cursor character (0-based).
    pub character: u32,
    /// Compile cache for the document, if available.
    pub cache: Option<&'a CachedCompileInfo>,
    /// Rustdoc cache for import completions.
    pub rustdoc: Option<&'a RustdocCache>,
}

/// Attempt to resolve native `RustScript` completions.
///
/// Tries dot-triggered completions first (builtin objects and type-aware members),
/// then falls back to keyword completions when not in a dot context. Returns
/// `None` only if no native completions apply.
#[must_use]
pub fn resolve_completions(ctx: &CompletionContext<'_>) -> Option<CompletionResponse> {
    let lines: Vec<&str> = ctx.source.lines().collect();
    let current_line = lines.get(ctx.line as usize)?;

    // Get the text up to the cursor on the current line.
    let prefix_end = (ctx.character as usize).min(current_line.len());
    let prefix = &current_line[..prefix_end];

    // Check if we're inside an import statement: `import { | } from "crate"`
    if let Some(items) = try_import_completions(prefix, ctx.source, ctx.rustdoc) {
        return Some(CompletionResponse::Array(items));
    }

    // Check for dot-triggered completions: `receiver.`
    if let Some(dot_pos) = prefix.rfind('.') {
        let before_dot = prefix[..dot_pos].trim();

        // Extract the identifier before the dot.
        let receiver = extract_last_identifier(before_dot);
        if !receiver.is_empty() {
            // Try builtin object completions first.
            if let Some(items) = builtin_object_completions(receiver) {
                return Some(CompletionResponse::Array(items));
            }

            // Try type-aware completions from the compile cache.
            if let Some(cache) = ctx.cache {
                // Look up the variable's type and provide method completions.
                if let Some(type_str) = cache.variable_types.get(receiver) {
                    if let Some(items) = type_member_completions(type_str) {
                        return Some(CompletionResponse::Array(items));
                    }
                    // Try struct/class member completions.
                    if let Some(items) = struct_member_completions(receiver, cache) {
                        return Some(CompletionResponse::Array(items));
                    }
                }
            }
        }

        return None;
    }

    // Not in a dot context — return keyword completions.
    Some(CompletionResponse::Array(keyword_completions()))
}

/// Try to provide import completions based on the current context.
///
/// Detects patterns like `import { | } from "crate"` and returns completions
/// for available items from the crate's rustdoc data.
fn try_import_completions(
    prefix: &str,
    source: &str,
    rustdoc: Option<&RustdocCache>,
) -> Option<Vec<CompletionItem>> {
    let rustdoc = rustdoc?;

    // Look for the import statement pattern in the current source.
    // The cursor should be between `{` and `}` of an import statement.
    let trimmed = prefix.trim();
    if !trimmed.starts_with("import") {
        return None;
    }

    // Find the `from "crate"` part in the full source line or nearby lines.
    // Simple heuristic: search for `from "` in the same line or nearby.
    for line in source.lines() {
        let line_trimmed = line.trim();
        if line_trimmed.starts_with("import")
            && let Some(from_idx) = line_trimmed.find("from \"")
        {
            let after_from = &line_trimmed[from_idx + 6..];
            if let Some(end_quote) = after_from.find('"') {
                let crate_name = &after_from[..end_quote];
                if !crate_name.starts_with('.') && !crate_name.starts_with('/') {
                    return import_completions(crate_name, rustdoc);
                }
            }
        }
    }

    None
}

/// Extract the last identifier from a string (for dot-completion receiver).
///
/// Given `"  someVar"`, returns `"someVar"`. Given `"obj.method()"`, this
/// is called on the text before the final dot.
fn extract_last_identifier(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut end = bytes.len();

    // Skip trailing whitespace.
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    let mut start = end;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }

    &text[start..end]
}

// ---------------------------------------------------------------------------
// Signature help resolution
// ---------------------------------------------------------------------------

/// Context for resolving signature help.
pub struct SignatureHelpContext<'a> {
    /// The full source text of the document.
    pub source: &'a str,
    /// Cursor line (0-based).
    pub line: u32,
    /// Cursor character (0-based).
    pub character: u32,
    /// Compile cache for the document.
    pub cache: Option<&'a CachedCompileInfo>,
}

/// Attempt to resolve signature help for the current cursor position.
///
/// Looks for the nearest open parenthesis before the cursor, extracts the
/// function name, and counts commas to determine the active parameter.
#[must_use]
pub fn resolve_signature_help(ctx: &SignatureHelpContext<'_>) -> Option<SignatureHelp> {
    let cache = ctx.cache?;

    let lines: Vec<&str> = ctx.source.lines().collect();
    let current_line = lines.get(ctx.line as usize)?;

    let prefix_end = (ctx.character as usize).min(current_line.len());
    let prefix = &current_line[..prefix_end];

    // Walk backwards to find the nearest unmatched `(`.
    let mut depth: i32 = 0;
    let mut comma_count: u32 = 0;
    let mut paren_pos = None;

    for (i, ch) in prefix.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    paren_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
            ',' if depth == 0 => comma_count += 1,
            _ => {}
        }
    }

    let paren_pos = paren_pos?;
    let before_paren = &prefix[..paren_pos];
    let func_name = extract_last_identifier(before_paren);

    if func_name.is_empty() {
        return None;
    }

    signature_help_for_function(func_name, comma_count, cache)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    // -----------------------------------------------------------------------
    // Keyword completions
    // -----------------------------------------------------------------------

    #[test]
    fn test_keyword_completions_returns_all_keywords() {
        let items = keyword_completions();
        assert_eq!(items.len(), KEYWORD_SNIPPETS.len());

        // Check that all expected keywords are present.
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"function"));
        assert!(labels.contains(&"const"));
        assert!(labels.contains(&"let"));
        assert!(labels.contains(&"if"));
        assert!(labels.contains(&"else"));
        assert!(labels.contains(&"while"));
        assert!(labels.contains(&"for"));
        assert!(labels.contains(&"class"));
        assert!(labels.contains(&"interface"));
        assert!(labels.contains(&"type"));
        assert!(labels.contains(&"import"));
        assert!(labels.contains(&"export"));
        assert!(labels.contains(&"async"));
        assert!(labels.contains(&"try"));
        assert!(labels.contains(&"switch"));
        assert!(labels.contains(&"test"));
        assert!(labels.contains(&"describe"));
        assert!(labels.contains(&"return"));
        assert!(labels.contains(&"throw"));
    }

    #[test]
    fn test_keyword_completions_have_snippet_format() {
        let items = keyword_completions();
        for item in &items {
            assert_eq!(
                item.kind,
                Some(CompletionItemKind::KEYWORD),
                "keyword {} should have KEYWORD kind",
                item.label
            );
            assert_eq!(
                item.insert_text_format,
                Some(InsertTextFormat::SNIPPET),
                "keyword {} should have SNIPPET format",
                item.label
            );
            assert!(
                item.insert_text.is_some(),
                "keyword {} should have insert text",
                item.label
            );
        }
    }

    #[test]
    fn test_keyword_function_snippet_contains_placeholders() {
        let items = keyword_completions();
        let func = items.iter().find(|i| i.label == "function");
        assert!(func.is_some());
        let snippet = func.as_ref().and_then(|f| f.insert_text.as_deref());
        assert!(snippet.is_some());
        let text = snippet.unwrap_or_default();
        assert!(text.contains("${1:name}"), "should have name placeholder");
        assert!(
            text.contains("${2:params}"),
            "should have params placeholder"
        );
    }

    // -----------------------------------------------------------------------
    // Builtin object completions
    // -----------------------------------------------------------------------

    #[test]
    fn test_builtin_math_completions() {
        let items = builtin_object_completions("Math");
        assert!(items.is_some(), "Math should have completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"floor"));
        assert!(labels.contains(&"ceil"));
        assert!(labels.contains(&"round"));
        assert!(labels.contains(&"abs"));
        assert!(labels.contains(&"sqrt"));
        assert!(labels.contains(&"min"));
        assert!(labels.contains(&"max"));
        assert!(labels.contains(&"random"));
        assert!(labels.contains(&"PI"));
        assert!(labels.contains(&"E"));
    }

    #[test]
    fn test_builtin_console_completions() {
        let items = builtin_object_completions("console");
        assert!(items.is_some(), "console should have completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"log"));
        assert!(labels.contains(&"error"));
        assert!(labels.contains(&"warn"));
        assert!(labels.contains(&"debug"));
    }

    #[test]
    fn test_builtin_json_completions() {
        let items = builtin_object_completions("JSON");
        assert!(items.is_some(), "JSON should have completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"parse"));
        assert!(labels.contains(&"stringify"));
    }

    #[test]
    fn test_builtin_number_completions() {
        let items = builtin_object_completions("Number");
        assert!(items.is_some(), "Number should have completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"parseInt"));
        assert!(labels.contains(&"parseFloat"));
        assert!(labels.contains(&"isNaN"));
        assert!(labels.contains(&"isFinite"));
        assert!(labels.contains(&"isInteger"));
    }

    #[test]
    fn test_builtin_object_completions() {
        let items = builtin_object_completions("Object");
        assert!(items.is_some(), "Object should have completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"keys"));
        assert!(labels.contains(&"values"));
        assert!(labels.contains(&"entries"));
    }

    #[test]
    fn test_builtin_unknown_object_returns_none() {
        assert!(builtin_object_completions("Unknown").is_none());
    }

    #[test]
    fn test_builtin_completions_have_documentation() {
        let items = builtin_object_completions("Math").unwrap_or_default();
        for item in &items {
            assert!(
                item.documentation.is_some(),
                "builtin member {} should have docs",
                item.label
            );
        }
    }

    // -----------------------------------------------------------------------
    // Type-aware member completions
    // -----------------------------------------------------------------------

    #[test]
    fn test_type_member_completions_string() {
        let items = type_member_completions("string");
        assert!(items.is_some(), "string should have member completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"toUpperCase"));
        assert!(labels.contains(&"toLowerCase"));
        assert!(labels.contains(&"trim"));
        assert!(labels.contains(&"split"));
        assert!(labels.contains(&"startsWith"));
        assert!(labels.contains(&"length"));
    }

    #[test]
    fn test_type_member_completions_string_capital() {
        let items = type_member_completions("String");
        assert!(items.is_some(), "String should also match");
    }

    #[test]
    fn test_type_member_completions_array() {
        let items = type_member_completions("Array<i32>");
        assert!(items.is_some(), "Array should have member completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"map"));
        assert!(labels.contains(&"filter"));
        assert!(labels.contains(&"reduce"));
        assert!(labels.contains(&"push"));
        assert!(labels.contains(&"pop"));
        assert!(labels.contains(&"length"));
    }

    #[test]
    fn test_type_member_completions_vec() {
        let items = type_member_completions("Vec<String>");
        assert!(
            items.is_some(),
            "Vec should also match as array completions"
        );
    }

    #[test]
    fn test_type_member_completions_map() {
        let items = type_member_completions("Map<string, i32>");
        assert!(items.is_some(), "Map should have member completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"get"));
        assert!(labels.contains(&"set"));
        assert!(labels.contains(&"has"));
        assert!(labels.contains(&"delete"));
        assert!(labels.contains(&"keys"));
        assert!(labels.contains(&"values"));
    }

    #[test]
    fn test_type_member_completions_set() {
        let items = type_member_completions("Set<string>");
        assert!(items.is_some(), "Set should have member completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"add"));
        assert!(labels.contains(&"has"));
        assert!(labels.contains(&"delete"));
        assert!(labels.contains(&"clear"));
    }

    #[test]
    fn test_type_member_completions_unknown_returns_none() {
        assert!(type_member_completions("CustomType").is_none());
    }

    #[test]
    fn test_struct_member_completions_from_cache() {
        let mut variable_types = HashMap::new();
        variable_types.insert("user".to_owned(), "User".to_owned());
        variable_types.insert(
            "User".to_owned(),
            "type User = { name: string, age: i64 }".to_owned(),
        );

        let cache = CachedCompileInfo {
            variable_types,
            function_signatures: HashMap::new(),
        };

        let items = struct_member_completions("user", &cache);
        assert!(items.is_some(), "should return struct field completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"name"), "should contain field 'name'");
        assert!(labels.contains(&"age"), "should contain field 'age'");
    }

    // -----------------------------------------------------------------------
    // Signature help
    // -----------------------------------------------------------------------

    #[test]
    fn test_signature_help_for_known_function() {
        let mut function_signatures = HashMap::new();
        function_signatures.insert(
            "greet".to_owned(),
            "function greet(name: string, count: i32): void".to_owned(),
        );
        let cache = CachedCompileInfo {
            variable_types: HashMap::new(),
            function_signatures,
        };

        let help = signature_help_for_function("greet", 0, &cache);
        assert!(help.is_some(), "should return signature help");
        let help = help.unwrap();
        assert_eq!(help.signatures.len(), 1);
        assert_eq!(help.active_parameter, Some(0));

        let sig = &help.signatures[0];
        assert!(sig.label.contains("greet"));
        assert!(sig.parameters.is_some());
        let empty = Vec::new();
        let params = sig.parameters.as_ref().unwrap_or(&empty);
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_signature_help_for_unknown_function() {
        let cache = CachedCompileInfo {
            variable_types: HashMap::new(),
            function_signatures: HashMap::new(),
        };
        assert!(signature_help_for_function("unknown", 0, &cache).is_none());
    }

    #[test]
    fn test_signature_help_active_param() {
        let mut function_signatures = HashMap::new();
        function_signatures.insert(
            "add".to_owned(),
            "function add(a: i32, b: i32): i32".to_owned(),
        );
        let cache = CachedCompileInfo {
            variable_types: HashMap::new(),
            function_signatures,
        };

        let help = signature_help_for_function("add", 1, &cache);
        assert!(help.is_some());
        assert_eq!(help.unwrap().active_parameter, Some(1));
    }

    #[test]
    fn test_signature_help_no_params() {
        let mut function_signatures = HashMap::new();
        function_signatures.insert("init".to_owned(), "function init(): void".to_owned());
        let cache = CachedCompileInfo {
            variable_types: HashMap::new(),
            function_signatures,
        };

        let help = signature_help_for_function("init", 0, &cache);
        assert!(help.is_some());
        let help = help.unwrap();
        let sig = &help.signatures[0];
        let empty = Vec::new();
        let params = sig.parameters.as_ref().unwrap_or(&empty);
        assert!(
            params.is_empty(),
            "zero-param function should have no parameters"
        );
    }

    // -----------------------------------------------------------------------
    // Import completions
    // -----------------------------------------------------------------------

    #[test]
    fn test_import_completions_from_rustdoc_cache() {
        use rsc_driver::rustdoc_parser::{
            RustdocCrate, RustdocFunction, RustdocItem, RustdocItemKind,
        };

        let mut crate_data = RustdocCrate::default();

        let item = RustdocItem {
            id: "0:1".to_owned(),
            name: "Router".to_owned(),
            docs: Some("An HTTP router.".to_owned()),
            kind: RustdocItemKind::Struct(rsc_driver::rustdoc_parser::RustdocStruct {
                generics: vec![],
                fields: vec![],
                is_tuple: false,
                method_ids: vec![],
            }),
        };

        crate_data
            .name_index
            .entry("Router".to_owned())
            .or_default()
            .push("0:1".to_owned());
        crate_data.items.insert("0:1".to_owned(), item);

        let func_item = RustdocItem {
            id: "0:2".to_owned(),
            name: "get".to_owned(),
            docs: Some("Create a GET route.".to_owned()),
            kind: RustdocItemKind::Function(RustdocFunction {
                generics: vec![],
                params: vec![],
                return_type: None,
                is_async: false,
                is_unsafe: false,
                has_self: false,
                parent_type: None,
            }),
        };

        crate_data
            .name_index
            .entry("get".to_owned())
            .or_default()
            .push("0:2".to_owned());
        crate_data.items.insert("0:2".to_owned(), func_item);

        let mut rustdoc = RustdocCache::new();
        rustdoc.insert("axum".to_owned(), crate_data);

        let items = import_completions("axum", &rustdoc);
        assert!(items.is_some(), "should return import completions");
        let items = items.unwrap_or_default();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Router"));
        assert!(labels.contains(&"get"));
    }

    #[test]
    fn test_import_completions_unknown_crate_returns_none() {
        let rustdoc = RustdocCache::new();
        assert!(import_completions("nonexistent", &rustdoc).is_none());
    }

    // -----------------------------------------------------------------------
    // Resolve completions (integration)
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_completions_keyword_context() {
        let source = "fu";
        let ctx = CompletionContext {
            source,
            line: 0,
            character: 2,
            cache: None,
            rustdoc: None,
        };

        let result = resolve_completions(&ctx);
        assert!(result.is_some(), "should return keyword completions");
        if let Some(CompletionResponse::Array(items)) = result {
            assert!(!items.is_empty());
            // Keywords should be in the list.
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(labels.contains(&"function"));
        }
    }

    #[test]
    fn test_resolve_completions_dot_builtin() {
        let source = "Math.";
        let ctx = CompletionContext {
            source,
            line: 0,
            character: 5,
            cache: None,
            rustdoc: None,
        };

        let result = resolve_completions(&ctx);
        assert!(result.is_some(), "should return Math member completions");
        if let Some(CompletionResponse::Array(items)) = result {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(labels.contains(&"floor"));
            assert!(labels.contains(&"PI"));
        }
    }

    #[test]
    fn test_resolve_completions_dot_type_aware() {
        let mut variable_types = HashMap::new();
        variable_types.insert("name".to_owned(), "string".to_owned());
        let cache = CachedCompileInfo {
            variable_types,
            function_signatures: HashMap::new(),
        };

        let source = "name.";
        let ctx = CompletionContext {
            source,
            line: 0,
            character: 5,
            cache: Some(&cache),
            rustdoc: None,
        };

        let result = resolve_completions(&ctx);
        assert!(result.is_some(), "should return string member completions");
        if let Some(CompletionResponse::Array(items)) = result {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(labels.contains(&"toUpperCase"));
            assert!(labels.contains(&"trim"));
        }
    }

    // -----------------------------------------------------------------------
    // Signature help resolution
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_signature_help_basic() {
        let mut function_signatures = HashMap::new();
        function_signatures.insert(
            "greet".to_owned(),
            "function greet(name: string): void".to_owned(),
        );
        let cache = CachedCompileInfo {
            variable_types: HashMap::new(),
            function_signatures,
        };

        let source = "greet(";
        let ctx = SignatureHelpContext {
            source,
            line: 0,
            character: 6,
            cache: Some(&cache),
        };

        let result = resolve_signature_help(&ctx);
        assert!(result.is_some(), "should return signature help");
        let help = result.unwrap();
        assert_eq!(help.signatures.len(), 1);
        assert!(help.signatures[0].label.contains("greet"));
    }

    #[test]
    fn test_resolve_signature_help_active_param_from_commas() {
        let mut function_signatures = HashMap::new();
        function_signatures.insert(
            "add".to_owned(),
            "function add(a: i32, b: i32): i32".to_owned(),
        );
        let cache = CachedCompileInfo {
            variable_types: HashMap::new(),
            function_signatures,
        };

        let source = "add(1, ";
        let ctx = SignatureHelpContext {
            source,
            line: 0,
            character: 7,
            cache: Some(&cache),
        };

        let result = resolve_signature_help(&ctx);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().active_parameter,
            Some(1),
            "second param should be active after one comma"
        );
    }

    #[test]
    fn test_resolve_signature_help_no_cache_returns_none() {
        let source = "foo(";
        let ctx = SignatureHelpContext {
            source,
            line: 0,
            character: 4,
            cache: None,
        };

        assert!(resolve_signature_help(&ctx).is_none());
    }

    // -----------------------------------------------------------------------
    // Helper: extract_last_identifier
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_last_identifier_simple() {
        assert_eq!(extract_last_identifier("someVar"), "someVar");
    }

    #[test]
    fn test_extract_last_identifier_with_whitespace() {
        assert_eq!(extract_last_identifier("  foo  "), "foo");
    }

    #[test]
    fn test_extract_last_identifier_empty() {
        assert_eq!(extract_last_identifier(""), "");
    }

    #[test]
    fn test_extract_last_identifier_after_equals() {
        assert_eq!(extract_last_identifier("const x = myVar"), "myVar");
    }
}
