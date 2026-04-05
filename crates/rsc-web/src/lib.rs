#![warn(clippy::pedantic)]
//! WASM bindings for the `RustScript` compiler.
//!
//! Exposes the compiler's pure functions via `wasm-bindgen` for use in the
//! playground (live compilation), crate docs (rustdoc translation), and
//! Monaco LSP features — all client-side in the browser.

pub mod translator;

use rsc_driver::rustdoc_parser::RustdocItemKind;
use rsc_syntax::diagnostic::Severity;
use rsc_syntax::source::compute_line_starts;
use rsc_syntax::span::BytePos;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Shared output types
// ---------------------------------------------------------------------------

/// JSON-serializable compilation result returned by [`compile`].
#[derive(Serialize)]
struct CompileOutput {
    /// The generated Rust source code (empty on error).
    rust_source: String,
    /// All diagnostics from all compiler passes.
    diagnostics: Vec<DiagnosticOutput>,
    /// Whether any error-level diagnostics were emitted.
    has_errors: bool,
}

/// A single diagnostic suitable for JSON serialization.
#[derive(Serialize)]
struct DiagnosticOutput {
    /// The human-readable message.
    message: String,
    /// `"error"`, `"warning"`, or `"info"`.
    severity: String,
    /// 1-based line number (if available).
    line: Option<u32>,
    /// 0-based column offset (if available).
    column: Option<u32>,
}

/// A translated rustdoc item for the docs viewer.
#[derive(Serialize)]
struct TranslatedItem {
    /// The item's name (e.g. `Router`).
    name: String,
    /// The item kind: `"function"`, `"struct"`, `"trait"`, or `"enum"`.
    kind: String,
    /// The `RustScript`-syntax signature.
    signature: String,
    /// Documentation string, if any.
    docs: Option<String>,
    /// Module path (reserved for future use).
    module: Option<String>,
}

// ---------------------------------------------------------------------------
// Helper: convert byte position to (line, column) using source text
// ---------------------------------------------------------------------------

/// Convert a `BytePos` to a 1-based line and 0-based column using
/// precomputed line starts from the source text.
fn byte_pos_to_line_col(pos: BytePos, line_starts: &[u32]) -> (u32, u32) {
    let offset = pos.0;
    let line_idx = line_starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1);
    let line_start = line_starts.get(line_idx).copied().unwrap_or(0);
    let col = offset.saturating_sub(line_start);

    #[allow(clippy::cast_possible_truncation)]
    // Line numbers and columns within a single source file will never overflow u32.
    let line_1based = (line_idx as u32) + 1;
    (line_1based, col)
}

/// Build [`DiagnosticOutput`] entries from compiler diagnostics, resolving
/// byte positions to line/column using the raw source text.
fn build_diagnostics(
    diagnostics: &[rsc_syntax::diagnostic::Diagnostic],
    source: &str,
) -> Vec<DiagnosticOutput> {
    let line_starts = compute_line_starts(source);

    diagnostics
        .iter()
        .map(|d| {
            let (line, column) = d.labels.first().map_or((None, None), |label| {
                let (l, c) = byte_pos_to_line_col(label.span.start, &line_starts);
                (Some(l), Some(c))
            });

            DiagnosticOutput {
                message: d.message.clone(),
                severity: match d.severity {
                    Severity::Error => "error".to_owned(),
                    Severity::Warning => "warning".to_owned(),
                    Severity::Note => "info".to_owned(),
                },
                line,
                column,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. compile(source) -> { rust_source, diagnostics, has_errors }
// ---------------------------------------------------------------------------

/// Compile `RustScript` source to Rust.
///
/// Returns a JSON object with `rust_source`, `diagnostics`, and `has_errors`.
#[wasm_bindgen]
#[allow(clippy::must_use_candidate)]
pub fn compile(source: &str) -> JsValue {
    let result = rsc_driver::compile_source(source, "playground.rts");

    let output = CompileOutput {
        rust_source: result.rust_source,
        diagnostics: build_diagnostics(&result.diagnostics, source),
        has_errors: result.has_errors,
    };

    serde_wasm_bindgen::to_value(&output).unwrap_or(JsValue::NULL)
}

// ---------------------------------------------------------------------------
// 2. get_diagnostics(source) -> [{ message, severity, line, column }]
// ---------------------------------------------------------------------------

/// Parse and lower a `RustScript` source, returning only diagnostics.
///
/// Faster than [`compile`] because it skips the emit stage.
#[wasm_bindgen]
#[allow(clippy::must_use_candidate)]
pub fn get_diagnostics(source: &str) -> JsValue {
    let line_starts = compute_line_starts(source);

    // Stage 1: Parse
    let file_id = rsc_syntax::source::FileId(0);
    let (module, parse_diagnostics) = rsc_parser::parse(source, file_id);

    let mut all_diagnostics = parse_diagnostics;

    // Stage 2: Lower (only if parsing succeeded without errors)
    let has_parse_errors = all_diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));

    if !has_parse_errors {
        let lower_result = rsc_lower::lower(&module);
        all_diagnostics.extend(lower_result.diagnostics);
    }

    let output: Vec<DiagnosticOutput> = all_diagnostics
        .iter()
        .map(|d| {
            let (line, column) = d.labels.first().map_or((None, None), |label| {
                let (l, c) = byte_pos_to_line_col(label.span.start, &line_starts);
                (Some(l), Some(c))
            });

            DiagnosticOutput {
                message: d.message.clone(),
                severity: match d.severity {
                    Severity::Error => "error".to_owned(),
                    Severity::Warning => "warning".to_owned(),
                    Severity::Note => "info".to_owned(),
                },
                line,
                column,
            }
        })
        .collect();

    serde_wasm_bindgen::to_value(&output).unwrap_or(JsValue::NULL)
}

// ---------------------------------------------------------------------------
// 3. hover(source, line, column) -> String
// ---------------------------------------------------------------------------

/// Builtin descriptions for well-known `RustScript` APIs.
fn builtin_hover(token: &str) -> Option<&'static str> {
    Some(match token {
        "console" => {
            "The console object provides access to the debugging console.\n\n```rustscript\nconst console: Console\n```"
        }
        "log" => {
            "Outputs a message to the console.\n\n```rustscript\nfunction console.log(...args: any[]): void\n```"
        }
        "error" => {
            "Outputs an error message to the console.\n\n```rustscript\nfunction console.error(...args: any[]): void\n```"
        }
        "warn" => {
            "Outputs a warning message to the console.\n\n```rustscript\nfunction console.warn(...args: any[]): void\n```"
        }
        "push" => {
            "Appends an element to the end of an array.\n\n```rustscript\nfunction Array<T>.push(value: T): void\n```"
        }
        "pop" => {
            "Removes the last element from an array and returns it.\n\n```rustscript\nfunction Array<T>.pop(): T | null\n```"
        }
        "map" => {
            "Creates a new array by applying a function to each element.\n\n```rustscript\nfunction Array<T>.map<U>(f: (item: T) => U): Array<U>\n```"
        }
        "filter" => {
            "Creates a new array with elements that pass a test.\n\n```rustscript\nfunction Array<T>.filter(f: (item: T) => boolean): Array<T>\n```"
        }
        "forEach" => {
            "Calls a function for each element in an array.\n\n```rustscript\nfunction Array<T>.forEach(f: (item: T) => void): void\n```"
        }
        "length" => {
            "The number of elements in an array or characters in a string.\n\n```rustscript\nreadonly length: number\n```"
        }
        "includes" => {
            "Determines whether an array or string contains a specified value.\n\n```rustscript\nfunction Array<T>.includes(value: T): boolean\n```"
        }
        "keys" => {
            "Returns an array of a Map's keys.\n\n```rustscript\nfunction Map<K, V>.keys(): Array<K>\n```"
        }
        "values" => {
            "Returns an array of a Map's values.\n\n```rustscript\nfunction Map<K, V>.values(): Array<V>\n```"
        }
        "has" => {
            "Returns whether a key exists in a Map or Set.\n\n```rustscript\nfunction Map<K, V>.has(key: K): boolean\n```"
        }
        "get" => {
            "Returns the value for a key in a Map.\n\n```rustscript\nfunction Map<K, V>.get(key: K): V | null\n```"
        }
        "set" => {
            "Sets a key-value pair in a Map.\n\n```rustscript\nfunction Map<K, V>.set(key: K, value: V): void\n```"
        }
        "delete" => {
            "Removes a key from a Map or Set.\n\n```rustscript\nfunction Map<K, V>.delete(key: K): boolean\n```"
        }
        "parseInt" => {
            "Parses a string and returns an integer.\n\n```rustscript\nfunction parseInt(s: string): number\n```"
        }
        "parseFloat" => {
            "Parses a string and returns a floating-point number.\n\n```rustscript\nfunction parseFloat(s: string): number\n```"
        }
        "toString" => {
            "Returns a string representation of a value.\n\n```rustscript\nfunction toString(): string\n```"
        }
        "JSON" => {
            "The JSON object provides methods for parsing and stringifying JSON.\n\n```rustscript\nconst JSON: JSON\n```"
        }
        "stringify" => {
            "Converts a value to a JSON string.\n\n```rustscript\nfunction JSON.stringify(value: any): string\n```"
        }
        "parse" => {
            "Parses a JSON string into a value.\n\n```rustscript\nfunction JSON.parse(text: string): any\n```"
        }
        "Math" => {
            "The Math object provides mathematical constants and functions.\n\n```rustscript\nconst Math: Math\n```"
        }
        "floor" => {
            "Returns the largest integer less than or equal to a number.\n\n```rustscript\nfunction Math.floor(x: number): number\n```"
        }
        "ceil" => {
            "Returns the smallest integer greater than or equal to a number.\n\n```rustscript\nfunction Math.ceil(x: number): number\n```"
        }
        "abs" => {
            "Returns the absolute value of a number.\n\n```rustscript\nfunction Math.abs(x: number): number\n```"
        }
        "random" => {
            "Returns a pseudo-random number between 0 and 1.\n\n```rustscript\nfunction Math.random(): number\n```"
        }
        "max" => {
            "Returns the largest of the given numbers.\n\n```rustscript\nfunction Math.max(...values: number[]): number\n```"
        }
        "min" => {
            "Returns the smallest of the given numbers.\n\n```rustscript\nfunction Math.min(...values: number[]): number\n```"
        }
        _ => return None,
    })
}

/// Return hover information for the symbol at the given position.
///
/// For the MVP, this identifies the token at (line, column) and returns
/// descriptions for known builtins. For user-defined symbols, it returns
/// the parsed signature when possible.
#[wasm_bindgen]
#[allow(clippy::must_use_candidate)]
pub fn hover(source: &str, line: u32, column: u32) -> String {
    // Convert 1-based line/column (Monaco convention) to 0-based indices.
    let line_0 = line.saturating_sub(1) as usize;
    let col = column.saturating_sub(1) as usize;

    // Find the token at the given position.
    let lines: Vec<&str> = source.lines().collect();
    let Some(line_text) = lines.get(line_0) else {
        return String::new();
    };

    // Extract the identifier at/around the column.
    let bytes = line_text.as_bytes();
    if col >= bytes.len() {
        return String::new();
    }

    // Find word boundaries around the cursor position.
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    if !is_ident_char(bytes[col]) {
        return String::new();
    }

    let start = (0..=col)
        .rev()
        .take_while(|&i| is_ident_char(bytes[i]))
        .last()
        .unwrap_or(col);
    let end = (col..bytes.len())
        .take_while(|&i| is_ident_char(bytes[i]))
        .last()
        .map_or(col + 1, |i| i + 1);

    let token = &line_text[start..end];

    // Check builtins first.
    if let Some(desc) = builtin_hover(token) {
        return desc.to_owned();
    }

    // For user-defined symbols, try to find a definition in the parsed AST.
    // Parse the source and look for function/type/variable declarations matching
    // the token name.
    let file_id = rsc_syntax::source::FileId(0);
    let (module, _diagnostics) = rsc_parser::parse(source, file_id);

    // Search top-level declarations for a matching name.
    for item in &module.items {
        if let Some(sig) = extract_declaration_signature(item, token) {
            return sig;
        }
    }

    // Fallback: just return the token.
    format!("`{token}`")
}

/// Try to extract a hover signature from a top-level item if it declares
/// something with the given name.
fn extract_declaration_signature(item: &rsc_syntax::ast::Item, name: &str) -> Option<String> {
    use rsc_syntax::ast::ItemKind;

    match &item.kind {
        ItemKind::Function(f) if f.name.name == name => {
            let params: Vec<String> = f
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name.name, format_type_ann(&p.type_ann)))
                .collect();

            let ret = f
                .return_type
                .as_ref()
                .and_then(|rta| rta.type_ann.as_ref())
                .map_or_else(|| "void".to_owned(), format_type_ann);

            let async_prefix = if f.is_async { "async " } else { "" };
            Some(format!(
                "```rustscript\n{async_prefix}function {name}({}): {ret}\n```",
                params.join(", ")
            ))
        }
        ItemKind::TypeDef(td) if td.name.name == name => {
            Some(format!("```rustscript\ntype {name} = ...\n```"))
        }
        ItemKind::Interface(iface) if iface.name.name == name => {
            Some(format!("```rustscript\ninterface {name}\n```"))
        }
        ItemKind::EnumDef(e) if e.name.name == name => {
            let variants: Vec<String> = e
                .variants
                .iter()
                .map(|v| match v {
                    rsc_syntax::ast::EnumVariant::Simple(ident, _) => ident.name.clone(),
                    rsc_syntax::ast::EnumVariant::Data { name: n, .. } => n.name.clone(),
                })
                .collect();
            Some(format!(
                "```rustscript\nenum {name} {{ {} }}\n```",
                variants.join(", ")
            ))
        }
        ItemKind::Class(c) if c.name.name == name => {
            Some(format!("```rustscript\nclass {name}\n```"))
        }
        _ => None,
    }
}

/// Format a type annotation for display.
fn format_type_ann(ty: &rsc_syntax::ast::TypeAnnotation) -> String {
    use rsc_syntax::ast::TypeKind;

    match &ty.kind {
        TypeKind::Named(ident) => ident.name.clone(),
        TypeKind::Void => "void".to_owned(),
        TypeKind::Never => "never".to_owned(),
        TypeKind::Unknown => "unknown".to_owned(),
        TypeKind::Inferred => "(inferred)".to_owned(),
        TypeKind::Generic(ident, args) => {
            let args_str: Vec<String> = args.iter().map(format_type_ann).collect();
            format!("{}<{}>", ident.name, args_str.join(", "))
        }
        TypeKind::Union(types) => {
            let inner: Vec<String> = types.iter().map(format_type_ann).collect();
            inner.join(" | ")
        }
        TypeKind::Function(params, ret) => {
            let params_str: Vec<String> = params.iter().map(format_type_ann).collect();
            format!("({}) => {}", params_str.join(", "), format_type_ann(ret))
        }
        TypeKind::Intersection(types) => {
            let inner: Vec<String> = types.iter().map(format_type_ann).collect();
            inner.join(" & ")
        }
        TypeKind::Shared(inner) => format!("shared<{}>", format_type_ann(inner)),
        TypeKind::Tuple(types) => {
            let inner: Vec<String> = types.iter().map(format_type_ann).collect();
            format!("[{}]", inner.join(", "))
        }
        TypeKind::StringLiteral(s) => format!("\"{s}\""),
        TypeKind::KeyOf(inner) => format!("keyof {}", format_type_ann(inner)),
        TypeKind::TypeOf(ident) => format!("typeof {}", ident.name),
        TypeKind::IndexSignature(idx) => {
            format!(
                "{{ [{}]: {} }}",
                format_type_ann(&idx.key_type),
                format_type_ann(&idx.value_type)
            )
        }
        TypeKind::IndexAccess(obj, idx) => {
            format!("{}[{}]", format_type_ann(obj), format_type_ann(idx))
        }
        TypeKind::Readonly(inner) => format!("readonly {}", format_type_ann(inner)),
        TypeKind::Conditional { .. } => "...".to_owned(),
        TypeKind::Infer(ident) => format!("infer {}", ident.name),
        TypeKind::TupleSpread(inner) => format!("...{}", format_type_ann(inner)),
        TypeKind::TypeGuard {
            param,
            guarded_type,
        } => {
            format!("{} is {}", param.name, format_type_ann(guarded_type))
        }
        TypeKind::Asserts {
            param,
            guarded_type,
        } => {
            if let Some(gt) = guarded_type {
                format!("asserts {} is {}", param.name, format_type_ann(gt))
            } else {
                format!("asserts {}", param.name)
            }
        }
        TypeKind::TemplateLiteralType { .. } => "string".to_owned(),
        TypeKind::MappedType { .. } => "{ [key: string]: ... }".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// 4. translate_rustdoc(json) -> [{ name, kind, signature, docs, module }]
// ---------------------------------------------------------------------------

/// Translate rustdoc JSON to `RustScript`-syntax item descriptions.
///
/// Takes a rustdoc JSON string (from `cargo doc --output-format json`),
/// parses it, and translates all items to `RustScript` syntax.
#[wasm_bindgen]
#[allow(clippy::must_use_candidate)]
pub fn translate_rustdoc(json: &str) -> JsValue {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json) else {
        return JsValue::NULL;
    };

    let Some(crate_data) = rsc_driver::rustdoc_parser::parse_rustdoc_json(&parsed) else {
        return JsValue::NULL;
    };

    let items: Vec<TranslatedItem> = crate_data
        .items
        .values()
        .map(|item| {
            let kind = match &item.kind {
                RustdocItemKind::Function(_) => "function",
                RustdocItemKind::Struct(_) => "struct",
                RustdocItemKind::Trait(_) => "trait",
                RustdocItemKind::Enum(_) => "enum",
            };
            TranslatedItem {
                name: item.name.clone(),
                kind: kind.to_owned(),
                signature: translator::translate_item_to_hover(item),
                docs: item.docs.clone(),
                module: None,
            }
        })
        .collect();

    serde_wasm_bindgen::to_value(&items).unwrap_or(JsValue::NULL)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_hello_world() {
        let source = "function main() { console.log(\"hello\"); }";
        let result = rsc_driver::compile_source(source, "test.rts");
        assert!(
            !result.has_errors,
            "compilation produced errors: {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
        assert!(result.rust_source.contains("println!"));
    }

    #[test]
    fn test_compile_with_error() {
        // Intentionally bad source to produce a parse error.
        let source = "function { }";
        let result = rsc_driver::compile_source(source, "test.rts");
        assert!(result.has_errors);
        assert!(!result.diagnostics.is_empty());
    }

    #[test]
    fn test_build_diagnostics_line_column() {
        let source = "let x: number = 42;\nlet y: string = \"hi\";";
        let diag = rsc_syntax::diagnostic::Diagnostic::error("test error").with_label(
            rsc_syntax::span::Span::new(20, 21),
            rsc_syntax::source::FileId(0),
            "here",
        );
        let output = build_diagnostics(&[diag], source);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].severity, "error");
        // Byte 20 is the start of line 2 (0-indexed line 1).
        assert_eq!(output[0].line, Some(2));
        assert_eq!(output[0].column, Some(0));
    }

    #[test]
    fn test_hover_builtin_console() {
        let source = "function main() { console.log(\"hello\"); }";
        let result = hover(source, 1, 18);
        assert!(
            result.contains("console"),
            "expected console hover, got: {result}"
        );
    }

    #[test]
    fn test_hover_builtin_log() {
        let source = "function main() { console.log(\"hello\"); }";
        let result = hover(source, 1, 26);
        assert!(result.contains("log"), "expected log hover, got: {result}");
    }

    #[test]
    fn test_hover_user_function() {
        let source = "function greet(name: string): string { return name; }";
        let result = hover(source, 1, 9);
        assert!(
            result.contains("function greet"),
            "expected greet signature, got: {result}"
        );
    }

    #[test]
    fn test_hover_out_of_bounds() {
        let source = "let x = 1;";
        let result = hover(source, 99, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_translate_rustdoc_invalid_json() {
        // Invalid JSON should fail to parse.
        let parsed: Result<serde_json::Value, _> = serde_json::from_str("not valid json");
        assert!(parsed.is_err());
    }

    #[test]
    fn test_translate_rustdoc_empty_index() {
        let json = r#"{"index": {}, "paths": {}}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let crate_data = rsc_driver::rustdoc_parser::parse_rustdoc_json(&parsed);
        // Valid rustdoc JSON with no items — should parse but have empty items.
        assert!(crate_data.is_some());
        assert!(crate_data.unwrap().items.is_empty());
    }

    #[test]
    fn test_byte_pos_to_line_col_basic() {
        let line_starts = vec![0, 10, 20];
        // Byte 0 = line 1, col 0
        assert_eq!(byte_pos_to_line_col(BytePos(0), &line_starts), (1, 0));
        // Byte 5 = line 1, col 5
        assert_eq!(byte_pos_to_line_col(BytePos(5), &line_starts), (1, 5));
        // Byte 10 = line 2, col 0
        assert_eq!(byte_pos_to_line_col(BytePos(10), &line_starts), (2, 0));
        // Byte 15 = line 2, col 5
        assert_eq!(byte_pos_to_line_col(BytePos(15), &line_starts), (2, 5));
        // Byte 20 = line 3, col 0
        assert_eq!(byte_pos_to_line_col(BytePos(20), &line_starts), (3, 0));
    }
}
