//! Translation of `rustc` error messages into `RustScript`-friendly terminology.
//!
//! When cargo build/run fails on the generated `.rs` code, `rustc` emits error
//! messages referencing Rust types, lifetimes, and concepts. This module translates
//! those messages into `RustScript` equivalents so the developer sees familiar terms.

use regex::Regex;
use rsc_syntax::diagnostic::ColorMode;
use rsc_syntax::span::Span;
use std::sync::LazyLock;

/// Header prepended to translated rustc error output.
const TRANSLATED_HEADER: &str = "RustScript compilation error (from rustc):";

/// Header prepended to raw (untranslated) rustc error output.
const RAW_HEADER: &str = "rustc error (in generated code):";

/// Compiled regex patterns for type name translations.
///
/// Each pattern targets a specific Rust type in "type position" contexts:
/// after colons, inside angle brackets, in error message descriptions.
struct TranslationPatterns {
    /// Matches `Vec<...>` with balanced angle brackets.
    vec_type: Regex,
    /// Matches `HashMap<...>` with balanced angle brackets.
    hashmap_type: Regex,
    /// Matches `HashSet<...>` with balanced angle brackets.
    hashset_type: Regex,
    /// Matches `Option<...>` with balanced angle brackets.
    option_type: Regex,
    /// Matches `Result<...>` with balanced angle brackets.
    result_type: Regex,
    /// Matches `String` as a standalone type (not part of another word).
    string_type: Regex,
    /// Matches `&str` as a type reference.
    str_ref_type: Regex,
    /// Matches `&String` as a type reference.
    string_ref_type: Regex,
    /// Matches `impl Fn(...)` / `impl FnMut(...)` / `impl FnOnce(...)` patterns.
    impl_fn_type: Regex,
    /// Matches `fn(...)` type syntax.
    fn_type: Regex,
    /// Matches `impl Trait` (not Fn-family).
    impl_trait: Regex,
    /// Matches `'static` lifetime annotations.
    static_lifetime: Regex,
    /// Matches file reference patterns like `src/main.rs:LINE:COL` in rustc output.
    file_reference: Regex,
    /// Matches `Arc<Mutex<...>>` patterns (`shared<T>` in `RustScript`).
    arc_mutex_type: Regex,
    /// Matches `Box<dyn ...>` patterns (abstract class / interface in `RustScript`).
    box_dyn_type: Regex,
    /// Matches generated union enum names like `I32OrString`, `BoolOrI32OrString`.
    /// These are `PascalCase` type names joined by `Or`.
    union_enum_name: Regex,
}

/// Compiled regex patterns for error message enrichment.
///
/// Each pattern matches a common `rustc` error category and maps to a
/// `RustScript`-specific hint that helps TypeScript developers understand the issue.
struct EnrichmentPatterns {
    /// Matches "use of moved value" or "value used here after move".
    moved_value: Regex,
    /// Matches "cannot move out of" or "borrow of moved value".
    cannot_move_out: Regex,
    /// Matches "cannot borrow .* as mutable" or "cannot borrow .* mutably".
    cannot_borrow_mut: Regex,
    /// Matches immutable/mutable borrow conflict errors.
    borrow_conflict: Regex,
    /// Matches type mismatch errors.
    type_mismatch: Regex,
    /// Matches trait-not-implemented errors.
    trait_not_impl: Regex,
    /// Matches lifetime errors.
    lifetime_error: Regex,
    /// Matches "cannot find value" / "not found in this scope".
    value_not_found: Regex,
    /// Matches "cannot find type" errors.
    type_not_found: Regex,
}

// SAFETY: All regex literals below are compile-time constants; these never fail.
static PATTERNS: LazyLock<TranslationPatterns> = LazyLock::new(|| TranslationPatterns {
    vec_type: Regex::new(r"\bVec<").expect("valid regex"),
    hashmap_type: Regex::new(r"\bHashMap<").expect("valid regex"),
    hashset_type: Regex::new(r"\bHashSet<").expect("valid regex"),
    option_type: Regex::new(r"\bOption<").expect("valid regex"),
    result_type: Regex::new(r"\bResult<").expect("valid regex"),
    string_type: Regex::new(r"\bString\b").expect("valid regex"),
    str_ref_type: Regex::new(r"&str\b").expect("valid regex"),
    string_ref_type: Regex::new(r"&String\b").expect("valid regex"),
    impl_fn_type: Regex::new(r"\bimpl\s+Fn(Mut|Once)?\(").expect("valid regex"),
    fn_type: Regex::new(r"\bfn\(").expect("valid regex"),
    impl_trait: Regex::new(r"\bimpl\s+([A-Z]\w+)\b").expect("valid regex"),
    static_lifetime: Regex::new(r"'static\s*").expect("valid regex"),
    file_reference: Regex::new(r"(src/\w+)\.rs:(\d+):(\d+)").expect("valid regex"),
    arc_mutex_type: Regex::new(r"\bArc<Mutex<").expect("valid regex"),
    box_dyn_type: Regex::new(r"\bBox<dyn\s+").expect("valid regex"),
    // Matches generated union enum names: two or more PascalCase type names joined by "Or".
    // Examples: I32OrString, BoolOrI32OrString, F64OrString
    union_enum_name: Regex::new(r"\b([A-Z]\w*(?:Or[A-Z]\w*)+)\b").expect("valid regex"),
});

// SAFETY: All regex literals below are compile-time constants; these never fail.
static ENRICHMENT_PATTERNS: LazyLock<EnrichmentPatterns> = LazyLock::new(|| EnrichmentPatterns {
    moved_value: Regex::new(r"(?i)use of moved value|value used here after move")
        .expect("valid regex"),
    cannot_move_out: Regex::new(r"(?i)cannot move out of|borrow of moved value")
        .expect("valid regex"),
    cannot_borrow_mut: Regex::new(r"(?i)cannot borrow .* as mutable|cannot borrow .* mutably")
        .expect("valid regex"),
    borrow_conflict: Regex::new(
        r"(?i)cannot borrow .* as immutable because it is also borrowed as mutable",
    )
    .expect("valid regex"),
    type_mismatch: Regex::new(r"(?i)mismatched types|expected .*, found .*").expect("valid regex"),
    trait_not_impl: Regex::new(r"(?i)the trait .* is not implemented").expect("valid regex"),
    lifetime_error: Regex::new(r"(?i)lifetime|does not live long enough").expect("valid regex"),
    value_not_found: Regex::new(r"(?i)cannot find value|not found in this scope")
        .expect("valid regex"),
    type_not_found: Regex::new(r"(?i)cannot find type").expect("valid regex"),
});

/// Translate `rustc` error output into `RustScript`-friendly terms.
///
/// Performs three translation passes:
/// 1. Translates Rust type names to `RustScript` equivalents (e.g., `String` → `string`).
/// 2. Remaps `.rs` line numbers to `.rts` line numbers using the source map (if provided).
/// 3. Replaces `.rs` file references with `.rts` file references (if source map provided).
///
/// When `source_map` is `None`, only type name translation is performed (Phase 2 behavior).
/// The `rts_source` parameter is the original `.rts` source text, used to convert byte
/// offsets in spans to line numbers. The `rts_filename` is the display name for the `.rts`
/// file (e.g., `"src/index.rts"`).
#[must_use]
pub fn translate_rustc_errors(
    stderr: &str,
    source_map: Option<&[Option<Span>]>,
    rts_source: Option<&str>,
    rts_filename: Option<&str>,
) -> String {
    translate_rustc_errors_colored(
        stderr,
        source_map,
        rts_source,
        rts_filename,
        ColorMode::Never,
    )
}

/// Translate `rustc` error output into `RustScript`-friendly terms, with optional color.
///
/// Same as [`translate_rustc_errors`] but applies ANSI color codes to the header
/// when `color` is [`ColorMode::Always`].
#[must_use]
pub fn translate_rustc_errors_colored(
    stderr: &str,
    source_map: Option<&[Option<Span>]>,
    rts_source: Option<&str>,
    rts_filename: Option<&str>,
    color: ColorMode,
) -> String {
    if stderr.trim().is_empty() {
        return String::new();
    }

    let mut translated = translate_type_names(stderr);
    let mut did_translate = translated != stderr;

    // Apply line number and file reference remapping if we have a source map
    if let Some(map) = source_map {
        let remapped = remap_file_references(&translated, map, rts_source, rts_filename);
        if remapped != translated {
            did_translate = true;
            translated = remapped;
        }
    }

    // Enrich with RustScript-specific hints and synthesized code annotations
    let enriched = enrich_translated_output(&translated, source_map, rts_source);
    if enriched != translated {
        did_translate = true;
        translated = enriched;
    }

    // If we actually changed something, use the translated header.
    // If nothing changed, use the raw header as fallback.
    let header = if did_translate {
        color_header(TRANSLATED_HEADER, color)
    } else {
        color_header(RAW_HEADER, color)
    };
    let body = if did_translate { &translated } else { stderr };
    format!("{header}\n{body}")
}

/// Apply ANSI bold red to a header string when colors are enabled.
fn color_header(header: &str, color: ColorMode) -> String {
    match color {
        ColorMode::Always => format!("\x1b[1;31m{header}\x1b[0m"),
        ColorMode::Never => header.to_owned(),
    }
}

/// Process the translated output to append enrichment hints and synthesized code annotations.
///
/// Scans each line of the translated `rustc` output for:
/// 1. Error messages that match known patterns → appends a "hint:" annotation.
/// 2. File references pointing to unmapped `.rs` lines → prepends a synthesized
///    code context annotation.
fn enrich_translated_output(
    input: &str,
    source_map: Option<&[Option<Span>]>,
    rts_source: Option<&str>,
) -> String {
    let mut result = String::with_capacity(input.len());
    let mut hint_appended = false;

    for line in input.lines() {
        result.push_str(line);
        result.push('\n');

        // Check for synthesized code annotation on file reference lines
        // that still point to .rs files (unmapped lines)
        if let Some(map) = source_map
            && let Some(rs_line) = extract_rs_line_from_reference(line)
            && let Some(annotation) = synthesized_code_annotation(map, rs_line, rts_source)
        {
            result.push_str("  |\n");
            result.push_str("  = note: ");
            result.push_str(&annotation);
            result.push('\n');
            hint_appended = true;
        }

        // Check for error message enrichment
        if let Some(hint) = enrich_error_message(line) {
            result.push_str("  |\n");
            result.push_str("  hint: ");
            result.push_str(&hint);
            result.push('\n');
            hint_appended = true;
        }
    }

    // Remove trailing newline to match input if it didn't end with one
    if !input.ends_with('\n') && result.ends_with('\n') && !hint_appended {
        result.pop();
    }

    result
}

/// Extract the `.rs` line number from a file reference line like ` --> src/main.rs:5:10`.
///
/// Returns `Some(line)` if the line references a `.rs` file, or `None` otherwise.
fn extract_rs_line_from_reference(line: &str) -> Option<usize> {
    let pattern = &PATTERNS.file_reference;
    let caps = pattern.captures(line)?;
    let filename = &caps[1];
    // Only annotate lines still pointing at .rs files (not already remapped to .rts)
    let has_rs_ext = std::path::Path::new(filename)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));
    if !has_rs_ext && !line.contains(".rs:") {
        return None;
    }
    caps[2].parse::<usize>().ok()
}

/// Convert a byte offset in source text to a 1-based line number.
///
/// Counts newlines before the byte offset to determine the line number.
fn byte_offset_to_line(source: &str, byte_offset: u32) -> usize {
    let offset = byte_offset as usize;
    let clamped = offset.min(source.len());
    source[..clamped].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Remap `.rs` file references and line numbers to `.rts` equivalents.
///
/// Finds patterns like `src/main.rs:LINE:COL` in rustc stderr output and replaces
/// them with the corresponding `.rts` file and line number using the source map.
/// Lines without a source map entry keep the original `.rs` reference.
fn remap_file_references(
    input: &str,
    source_map: &[Option<Span>],
    rts_source: Option<&str>,
    rts_filename: Option<&str>,
) -> String {
    let rts_file = rts_filename.unwrap_or("src/index.rts");
    let pattern = &PATTERNS.file_reference;

    pattern
        .replace_all(input, |caps: &regex::Captures| {
            let rs_line_str = &caps[2];
            let col = &caps[3];

            // Parse the 1-based line number from the .rs reference
            if let Ok(rs_line) = rs_line_str.parse::<usize>() {
                // Source map is 0-based, rustc lines are 1-based
                let map_index = rs_line.saturating_sub(1);
                if let Some(Some(span)) = source_map.get(map_index) {
                    // Convert the span's byte offset to a 1-based .rts line number
                    if let Some(source) = rts_source {
                        let rts_line = byte_offset_to_line(source, span.start.0);
                        format!("{rts_file}:{rts_line}:{col}")
                    } else {
                        // No source text to resolve byte offsets — just fix the filename
                        format!("{rts_file}:{rs_line}:{col}")
                    }
                } else {
                    // No mapping for this line — keep original .rs reference
                    caps[0].to_owned()
                }
            } else {
                caps[0].to_owned()
            }
        })
        .into_owned()
}

/// Examine a `rustc` error message and return a `RustScript`-specific hint annotation.
///
/// Returns `Some(hint)` if the message matches a known error pattern, or `None`
/// if no enrichment applies. The hint is intended to be appended as an indented
/// "hint:" block after the original error output.
///
/// Patterns are checked in specificity order — more specific patterns (e.g. borrow
/// conflict) are tested before broader ones (e.g. generic type mismatch).
#[must_use]
fn enrich_error_message(message: &str) -> Option<String> {
    let p = &*ENRICHMENT_PATTERNS;

    // Ownership — moved value
    if p.moved_value.is_match(message) {
        return Some(
            "In RustScript, passing a value to a function moves it by default. \
             To use it again, either clone it before passing (e.g., `myVar.clone()`) \
             or restructure your code to avoid reusing the value."
                .to_owned(),
        );
    }

    // Ownership — cannot move out / borrow of moved
    if p.cannot_move_out.is_match(message) {
        return Some(
            "A value was moved (transferred ownership) and then accessed. \
             Consider cloning the value before the move, or restructuring to \
             avoid the double use."
                .to_owned(),
        );
    }

    // Borrow conflict (immutable while mutable) — check before generic mutable borrow
    if p.borrow_conflict.is_match(message) {
        return Some(
            "You have a mutable reference and an immutable reference to the same \
             value at the same time. Rust requires exclusive access for mutation. \
             Restructure so the mutable use completes before the immutable read."
                .to_owned(),
        );
    }

    // Cannot borrow as mutable
    if p.cannot_borrow_mut.is_match(message) {
        return Some(
            "You're trying to modify a value while it's being read elsewhere. \
             Finish reading before modifying, or use a separate variable."
                .to_owned(),
        );
    }

    // Trait not implemented — check before type mismatch (more specific)
    if p.trait_not_impl.is_match(message) {
        return Some(
            "This type doesn't support the required operation. If this is a custom \
             type, you may need to add a `derives` clause (e.g., `derives Clone, Debug`)."
                .to_owned(),
        );
    }

    // Lifetime errors
    if p.lifetime_error.is_match(message) {
        return Some(
            "A value is being used after its scope has ended. This often happens \
             when returning a reference to a local variable. Try returning an owned \
             value instead."
                .to_owned(),
        );
    }

    // Value not found
    if p.value_not_found.is_match(message) {
        return Some(
            "This name isn't defined in the current scope. Check for typos, or \
             make sure the import is correct."
                .to_owned(),
        );
    }

    // Type not found
    if p.type_not_found.is_match(message) {
        return Some(
            "This type isn't defined. Check the type name for typos or add the \
             appropriate import."
                .to_owned(),
        );
    }

    // Type mismatch (broad — checked last among type-related patterns)
    if p.type_mismatch.is_match(message) {
        return Some(
            "Type mismatch — check that your function's return type matches what \
             you're returning, or that the argument types match the function signature."
                .to_owned(),
        );
    }

    None
}

/// Produce a context annotation for errors on compiler-synthesized (unmapped) lines.
///
/// When a `rustc` error points at a generated `.rs` line that has no corresponding
/// `.rts` source (i.e. `None` in the source map), this function walks backward
/// through the source map to find the nearest mapped `.rts` line and returns a
/// message like "This error is in code generated by the `RustScript` compiler near
/// line N of your .rts file."
///
/// Returns `None` if the `rs_line` *does* have a mapping (no annotation needed),
/// or if no nearby mapped line can be found.
#[must_use]
fn synthesized_code_annotation(
    source_map: &[Option<Span>],
    rs_line: usize,
    rts_source: Option<&str>,
) -> Option<String> {
    let map_index = rs_line.saturating_sub(1);

    // If this line *has* a mapping, no annotation is needed.
    if let Some(Some(_)) = source_map.get(map_index) {
        return None;
    }

    // Walk backward to find the nearest non-null entry.
    let nearest = (0..map_index).rev().find_map(|i| {
        source_map
            .get(i)
            .and_then(|entry| entry.as_ref())
            .map(|span| (i, span))
    });

    if let Some((_idx, span)) = nearest {
        if let Some(source) = rts_source {
            let nearest_line = byte_offset_to_line(source, span.start.0);
            Some(format!(
                "This error is in code generated by the RustScript compiler \
                 near line {nearest_line} of your .rts file."
            ))
        } else {
            Some(
                "This error is in code generated by the RustScript compiler.".to_owned(),
            )
        }
    } else {
        // No mapped lines found at all — generic message
        Some(
            "This error is in code generated by the RustScript compiler.".to_owned(),
        )
    }
}

/// Apply all type name translations to the given text.
///
/// Handles nested generics by processing from the inside out: first translates
/// the innermost type names, then works outward through generic wrappers.
fn translate_type_names(input: &str) -> String {
    let mut output = input.to_owned();

    // Order matters: translate inner types before outer wrappers.
    // 1. Simple leaf types first (String, &str, &String)
    output = translate_string_ref_type(&output);
    output = translate_str_ref_type(&output);
    output = translate_string_type(&output);

    // 2. Lifetime annotations
    output = translate_static_lifetime(&output);

    // 3. Function types
    output = translate_impl_fn_types(&output);
    output = translate_fn_types(&output);

    // 4. Generic wrapper types (inside-out: nested ones get translated first
    //    because we translate the inner type name, then the wrapper)
    output = translate_option_type(&output);
    output = translate_result_type(&output);
    output = translate_hashset_type(&output);
    output = translate_hashmap_type(&output);
    output = translate_vec_type(&output);

    // 4b. Shared and trait-object types
    output = translate_arc_mutex_type(&output);
    output = translate_box_dyn_type(&output);

    // 4c. Tuple types: `(String, i32)` → `[string, i32]`
    output = translate_tuple_type(&output);

    // 4d. Generated union enum names: `I32OrString` → `i32 | string`
    output = translate_union_enum_names(&output);

    // 5. impl Trait (after Fn translations to avoid double-matching)
    output = translate_impl_trait(&output);

    // 6. Async pattern names — translate Rust macro/function names back to RustScript
    output = translate_async_patterns(&output);

    // 7. Abstract class / trait error messages
    output = translate_trait_errors(&output);

    output
}

/// Translate `String` → `string` (only standalone occurrences, not inside other words).
fn translate_string_type(input: &str) -> String {
    PATTERNS
        .string_type
        .replace_all(input, "string")
        .into_owned()
}

/// Translate `&str` → `string (reference)`.
fn translate_str_ref_type(input: &str) -> String {
    PATTERNS
        .str_ref_type
        .replace_all(input, "string (reference)")
        .into_owned()
}

/// Translate `&String` → `string (reference)`.
fn translate_string_ref_type(input: &str) -> String {
    PATTERNS
        .string_ref_type
        .replace_all(input, "string (reference)")
        .into_owned()
}

/// Translate `'static` → empty (remove lifetime noise).
fn translate_static_lifetime(input: &str) -> String {
    PATTERNS.static_lifetime.replace_all(input, "").into_owned()
}

/// Translate `Vec<T>` → `Array<T>`.
///
/// Finds each `Vec<` and extracts the balanced generic content, then wraps
/// it as `Array<...>`.
fn translate_vec_type(input: &str) -> String {
    replace_generic_type(input, &PATTERNS.vec_type, "Vec<", "Array<")
}

/// Translate `HashMap<K, V>` → `Map<K, V>`.
fn translate_hashmap_type(input: &str) -> String {
    replace_generic_type(input, &PATTERNS.hashmap_type, "HashMap<", "Map<")
}

/// Translate `HashSet<T>` → `Set<T>`.
fn translate_hashset_type(input: &str) -> String {
    replace_generic_type(input, &PATTERNS.hashset_type, "HashSet<", "Set<")
}

/// Translate `Option<T>` → `T | null`.
///
/// This is special: `Option<i32>` becomes `i32 | null`, not `Option<i32>` with
/// a different wrapper name.
fn translate_option_type(input: &str) -> String {
    replace_option_type(input)
}

/// Translate `Result<T, E>` → mention of throws.
///
/// `Result<i32, E>` becomes `i32 (throws E)`.
fn translate_result_type(input: &str) -> String {
    replace_result_type(input)
}

/// Translate `impl Fn(T) -> U` / `impl FnMut(T) -> U` / `impl FnOnce(T) -> U`
/// into `(T) => U`.
fn translate_impl_fn_types(input: &str) -> String {
    replace_impl_fn_type(input)
}

/// Translate bare `fn(T) -> U` into `(T) => U`.
fn translate_fn_types(input: &str) -> String {
    replace_bare_fn_type(input)
}

/// Translate generated union enum names back to `RustScript` union syntax.
///
/// `I32OrString` → `i32 | string`, `BoolOrI32OrString` → `bool | i32 | string`.
/// Only matches names that follow the `XOrY` pattern where X and Y are
/// recognized type names.
fn translate_union_enum_names(input: &str) -> String {
    let re = &PATTERNS.union_enum_name;
    re.replace_all(input, |caps: &regex::Captures| {
        let name = &caps[1];
        // Split on "Or" boundaries (between PascalCase segments)
        let parts: Vec<&str> = name.split("Or").collect();
        if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
            return name.to_owned();
        }
        // Convert each PascalCase segment to its RustScript equivalent
        let rts_parts: Vec<String> = parts.iter().map(|p| pascal_type_to_rts(p)).collect();
        rts_parts.join(" | ")
    })
    .into_owned()
}

/// Convert a `PascalCase` type variant name to its `RustScript` equivalent.
///
/// Maps union enum variant names like `"String"` → `"string"`,
/// `"I32"` → `"i32"`, `"Bool"` → `"bool"`.
fn pascal_type_to_rts(name: &str) -> String {
    match name {
        "String" => "string".to_owned(),
        "Bool" => "bool".to_owned(),
        "I8" => "i8".to_owned(),
        "I16" => "i16".to_owned(),
        "I32" => "i32".to_owned(),
        "I64" => "i64".to_owned(),
        "U8" => "u8".to_owned(),
        "U16" => "u16".to_owned(),
        "U32" => "u32".to_owned(),
        "U64" => "u64".to_owned(),
        "F32" => "f32".to_owned(),
        "F64" => "f64".to_owned(),
        other => other.to_owned(),
    }
}

/// Translate `impl Trait` → `extends Trait` (but not `impl Fn*`).
fn translate_impl_trait(input: &str) -> String {
    // Only match impl followed by a non-Fn trait name.
    let re = &PATTERNS.impl_trait;
    re.replace_all(input, |caps: &regex::Captures| {
        let trait_name = &caps[1];
        // Skip Fn/FnMut/FnOnce — those were already handled.
        if trait_name.starts_with("Fn") {
            caps[0].to_owned()
        } else {
            format!("extends {trait_name}")
        }
    })
    .into_owned()
}

/// Translate `Arc<Mutex<T>>` → `shared<T>`.
///
/// Peels off both layers of generics and wraps the inner type as `shared<T>`.
fn translate_arc_mutex_type(input: &str) -> String {
    let pattern = &PATTERNS.arc_mutex_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        // After `Arc<Mutex<`, find the balanced closing `>>`.
        let after_open = &remaining[m.end()..];
        if let Some(inner_close) = find_balanced_close(after_open) {
            let inner = &after_open[..inner_close];
            let translated_inner = translate_type_names(inner);
            // After the inner `>`, we expect another `>` for the outer Arc.
            let after_inner = &after_open[inner_close + 1..];
            if let Some(rest) = after_inner.strip_prefix('>') {
                result.push_str("shared<");
                result.push_str(&translated_inner);
                result.push('>');
                remaining = rest;
            } else {
                // Malformed — leave original text.
                result.push_str("Arc<Mutex<");
                remaining = after_open;
            }
        } else {
            result.push_str("Arc<Mutex<");
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Translate `Box<dyn Trait>` → `Trait` (just the trait name).
///
/// Strips the `Box<dyn ...>` wrapper, keeping only the trait name.
fn translate_box_dyn_type(input: &str) -> String {
    let pattern = &PATTERNS.box_dyn_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        // After `Box<dyn `, find the balanced closing `>`.
        let after_open = &remaining[m.end()..];
        if let Some(close_pos) = find_balanced_close(after_open) {
            let inner = after_open[..close_pos].trim();
            result.push_str(inner);
            remaining = &after_open[close_pos + 1..];
        } else {
            result.push_str("Box<dyn ");
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Translate Rust tuple types `(String, i32)` → `[string, i32]`.
///
/// Matches tuples that appear in type contexts — parenthesized comma-separated types.
/// Each element is recursively translated via [`translate_type_names`].
fn translate_tuple_type(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    let mut last_end = 0;

    while let Some(&(i, ch)) = chars.peek() {
        if ch == '(' {
            // Check if this looks like a tuple type (has a comma inside balanced parens)
            // but NOT a function type (preceded by `Fn`, `FnMut`, `FnOnce`, or `=>`)
            let before = &input[..i];
            let is_fn_type = before.ends_with("Fn")
                || before.ends_with("FnMut")
                || before.ends_with("FnOnce")
                || before.ends_with("fn");

            if !is_fn_type && let Some(close) = find_balanced_paren(&input[i + 1..]) {
                let inner = &input[i + 1..i + 1 + close];
                // Only convert if there's at least one comma (multi-element tuple)
                if inner.contains(',') {
                    result.push_str(&input[last_end..i]);
                    // Split by commas (respecting nesting) and translate each element
                    let elements = split_type_list(inner);
                    result.push('[');
                    for (idx, elem) in elements.iter().enumerate() {
                        if idx > 0 {
                            result.push_str(", ");
                        }
                        result.push_str(&translate_type_names(elem.trim()));
                    }
                    result.push(']');
                    last_end = i + 1 + close + 1;
                    // Advance past the close paren
                    for _ in 0..=close {
                        chars.next();
                    }
                    continue;
                }
            }
        }
        chars.next();
    }

    result.push_str(&input[last_end..]);
    result
}

/// Find the matching closing `)` for a `(`, respecting nesting.
///
/// Returns the index of the closing `)` relative to the start of `input`.
fn find_balanced_paren(input: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in input.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Split a comma-separated type list, respecting nested brackets and parens.
fn split_type_list(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut last = 0;
    for (i, ch) in input.char_indices() {
        match ch {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&input[last..i]);
                last = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[last..]);
    parts
}

/// Translate async-pattern-related terms back to `RustScript` equivalents.
///
/// Maps Rust macro/function names from generated code to the `RustScript` constructs
/// that produced them, making error messages more understandable.
fn translate_async_patterns(input: &str) -> String {
    let mut output = input.to_owned();
    // tokio::select! errors → mention Promise.race
    output = output.replace("tokio::select!", "Promise.race (tokio::select!)");
    // futures::future::select_ok errors → mention Promise.any
    output = output.replace(
        "futures::future::select_ok",
        "Promise.any (futures::future::select_ok)",
    );
    // StreamExt trait errors → explain for await
    output = output.replace("StreamExt", "StreamExt (required by `for await`)");
    // futures::Stream trait errors → explain for await
    if output.contains("futures::Stream") && !output.contains("StreamExt") {
        output = output.replace(
            "futures::Stream",
            "futures::Stream (the iterable in `for await` must implement the Stream trait)",
        );
    }
    output
}

/// Replace a generic type like `Vec<T>` → `Array<T>` with balanced bracket matching.
///
/// The inner content is recursively translated via [`translate_type_names`] so that
/// nested generics like `Vec<Vec<String>>` become `Array<Array<string>>`.
fn replace_generic_type(input: &str, pattern: &Regex, prefix: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        // Add everything before the match.
        result.push_str(&remaining[..m.start()]);

        // Find the balanced closing `>` after the opening `<`.
        let after_open = &remaining[m.end()..];
        if let Some(close_pos) = find_balanced_close(after_open) {
            let inner = &after_open[..close_pos];
            let translated_inner = translate_type_names(inner);
            result.push_str(replacement);
            result.push_str(&translated_inner);
            result.push('>');
            remaining = &after_open[close_pos + 1..];
        } else {
            // No balanced close found — leave the original text.
            result.push_str(prefix);
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Replace `Option<T>` with `T | null`.
///
/// The inner content is recursively translated so that `Option<Vec<String>>`
/// becomes `Array<string> | null`.
fn replace_option_type(input: &str) -> String {
    let pattern = &PATTERNS.option_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        let after_open = &remaining[m.end()..];
        if let Some(close_pos) = find_balanced_close(after_open) {
            let inner = &after_open[..close_pos];
            let translated_inner = translate_type_names(inner);
            result.push_str(&translated_inner);
            result.push_str(" | null");
            remaining = &after_open[close_pos + 1..];
        } else {
            result.push_str("Option<");
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Replace `Result<T, E>` with `T (throws E)`.
///
/// The inner content is recursively translated so that `Result<Vec<String>, MyError>`
/// becomes `Array<string> (throws MyError)`.
fn replace_result_type(input: &str) -> String {
    let pattern = &PATTERNS.result_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        let after_open = &remaining[m.end()..];
        if let Some(close_pos) = find_balanced_close(after_open) {
            let inner = &after_open[..close_pos];
            // Split on the first top-level comma to separate T from E.
            if let Some(comma_pos) = find_top_level_comma(inner) {
                let ok_type = translate_type_names(inner[..comma_pos].trim());
                let err_type = translate_type_names(inner[comma_pos + 1..].trim());
                result.push_str(&ok_type);
                result.push_str(" (throws ");
                result.push_str(&err_type);
                result.push(')');
            } else {
                // Single type arg — just show it with throws
                let translated = translate_type_names(inner.trim());
                result.push_str(&translated);
                result.push_str(" (throws)");
            }
            remaining = &after_open[close_pos + 1..];
        } else {
            result.push_str("Result<");
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Replace `impl Fn(T) -> U` or `impl FnMut(T) -> U` or `impl FnOnce(T) -> U`
/// with `(T) => U`.
fn replace_impl_fn_type(input: &str) -> String {
    let pattern = &PATTERNS.impl_fn_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        // After the opening `(`, find the matching `)`.
        let after_paren = &remaining[m.end()..];
        if let Some(close_paren) = find_balanced_paren_close(after_paren) {
            let params = &after_paren[..close_paren];
            let after_close = &after_paren[close_paren + 1..];

            // Check for `-> ReturnType`.
            let trimmed = after_close.trim_start();
            if let Some(rest) = trimmed.strip_prefix("->") {
                let ret = rest.trim_start();
                // Consume the return type (up to a comma, closing bracket, or newline).
                let ret_end = ret.find([',', ')', '\n', ';']).unwrap_or(ret.len());
                let ret_type = ret[..ret_end].trim();
                result.push('(');
                result.push_str(params);
                result.push_str(") => ");
                result.push_str(ret_type);
                remaining = &ret[ret_end..];
            } else {
                // No return type
                result.push('(');
                result.push_str(params);
                result.push_str(") => void");
                remaining = after_close;
            }
        } else {
            // No balanced paren — leave original.
            result.push_str(&remaining[m.start()..m.end()]);
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Replace bare `fn(T) -> U` with `(T) => U`.
fn replace_bare_fn_type(input: &str) -> String {
    let pattern = &PATTERNS.fn_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        let after_paren = &remaining[m.end()..];
        if let Some(close_paren) = find_balanced_paren_close(after_paren) {
            let params = &after_paren[..close_paren];
            let after_close = &after_paren[close_paren + 1..];

            let trimmed = after_close.trim_start();
            if let Some(rest) = trimmed.strip_prefix("->") {
                let ret = rest.trim_start();
                let ret_end = ret.find([',', ')', '\n', ';']).unwrap_or(ret.len());
                let ret_type = ret[..ret_end].trim();
                result.push('(');
                result.push_str(params);
                result.push_str(") => ");
                result.push_str(ret_type);
                remaining = &ret[ret_end..];
            } else {
                result.push('(');
                result.push_str(params);
                result.push_str(") => void");
                remaining = after_close;
            }
        } else {
            result.push_str(&remaining[m.start()..m.end()]);
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Find the position of the closing `>` that balances the first `<` we are past.
///
/// Returns the byte offset of `>` relative to the start of the input slice,
/// or `None` if no balanced closing bracket is found.
fn find_balanced_close(input: &str) -> Option<usize> {
    let mut depth = 1;
    for (i, c) in input.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the position of the closing `)` that balances an already-opened `(`.
fn find_balanced_paren_close(input: &str) -> Option<usize> {
    let mut depth = 1;
    for (i, c) in input.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the position of the first comma at depth 0 (not inside nested `<>`).
fn find_top_level_comma(input: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in input.char_indices() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Translate trait-related rustc errors into abstract class terminology.
///
/// Maps "trait `X` is not implemented for" → "abstract class `X` is not implemented for",
/// and "required method" → "abstract method".
fn translate_trait_errors(input: &str) -> String {
    let output = input
        .replace("the trait `", "the abstract class `")
        .replace("trait `", "abstract class `")
        .replace("required method", "abstract method");
    // Only translate "field `X` of struct `Y` is private" to mention #field
    output.replace(
        "is private",
        "is private (use `#field` syntax for private fields)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Test 1: String type translation ---
    #[test]
    fn test_translate_string_type_in_error_message() {
        let input = "expected String, found i32";
        let result = translate_type_names(input);
        assert_eq!(result, "expected string, found i32");
    }

    // --- Test 2: Vec<T> translation ---
    #[test]
    fn test_translate_vec_to_array() {
        let input = "expected Vec<String>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Array<string>");
    }

    // --- Test 3: Option<T> translation ---
    #[test]
    fn test_translate_option_to_union_null() {
        let input = "expected Option<i32>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected i32 | null");
    }

    // --- Test 4: Error passthrough with header ---
    #[test]
    fn test_translate_unrecognized_error_shows_raw_header() {
        let input = "error[E9999]: some future error\n --> src/main.rs:3:1\n";
        let result = translate_rustc_errors(input, None, None, None);
        assert!(
            result.starts_with(RAW_HEADER),
            "expected raw header, got:\n{result}"
        );
        assert!(
            result.contains("error[E9999]"),
            "original error should be preserved"
        );
    }

    // --- Test 5: Empty stderr produces empty output ---
    #[test]
    fn test_translate_empty_stderr_returns_empty() {
        let result = translate_rustc_errors("", None, None, None);
        assert!(result.is_empty());
    }

    // --- Test 6: HashMap translation ---
    #[test]
    fn test_translate_hashmap_to_map() {
        let input = "expected HashMap<String, i32>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Map<string, i32>");
    }

    // --- Test 7: HashSet translation ---
    #[test]
    fn test_translate_hashset_to_set() {
        let input = "expected HashSet<String>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Set<string>");
    }

    // --- Test 8: Result<T, E> translation ---
    #[test]
    fn test_translate_result_to_throws() {
        let input = "expected Result<i32, MyError>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected i32 (throws MyError)");
    }

    // --- Test 9: &str translation ---
    #[test]
    fn test_translate_str_ref_to_string_reference() {
        let input = "expected &str, found i32";
        let result = translate_type_names(input);
        assert_eq!(result, "expected string (reference), found i32");
    }

    // --- Test 10: &String translation ---
    #[test]
    fn test_translate_string_ref_to_string_reference() {
        let input = "expected &String";
        let result = translate_type_names(input);
        assert_eq!(result, "expected string (reference)");
    }

    // --- Test 11: impl Fn translation ---
    #[test]
    fn test_translate_impl_fn_to_arrow() {
        let input = "expected impl Fn(i32) -> bool";
        let result = translate_type_names(input);
        assert_eq!(result, "expected (i32) => bool");
    }

    // --- Test 12: impl Trait translation ---
    #[test]
    fn test_translate_impl_trait_to_extends() {
        let input = "impl Display";
        let result = translate_type_names(input);
        assert_eq!(result, "extends Display");
    }

    // --- Test 13: 'static lifetime removal ---
    #[test]
    fn test_translate_static_lifetime_removed() {
        let input = "expected &'static str";
        let result = translate_type_names(input);
        assert_eq!(result, "expected &str");
        // Note: &str itself would be further translated in the full pipeline,
        // but we translate simple types first, then this catches leftovers.
    }

    // --- Test 14: Nested generics ---
    #[test]
    fn test_translate_nested_vec_of_string() {
        let input = "expected Vec<Vec<String>>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Array<Array<string>>");
    }

    // --- Test 15: Full rustc error message translation ---
    #[test]
    fn test_translate_full_rustc_error() {
        let input = r#"error[E0308]: mismatched types
 --> src/main.rs:5:10
  |
5 |     let x: String = 42;
  |                      ^^ expected String, found integer
"#;
        let result = translate_rustc_errors(input, None, None, None);
        assert!(
            result.starts_with(TRANSLATED_HEADER),
            "expected translated header, got:\n{result}"
        );
        assert!(
            result.contains("expected string, found integer"),
            "String should be translated to string in:\n{result}"
        );
    }

    // --- Test 16: Successful build (no error) ---
    #[test]
    fn test_translate_whitespace_only_returns_empty() {
        let result = translate_rustc_errors("   \n  ", None, None, None);
        assert!(result.is_empty());
    }

    // --- Test 17: fn type translation ---
    #[test]
    fn test_translate_fn_type_to_arrow() {
        let input = "expected fn(i32) -> bool";
        let result = translate_type_names(input);
        assert_eq!(result, "expected (i32) => bool");
    }

    // --- Test 18: Complex nested type ---
    #[test]
    fn test_translate_complex_nested_type() {
        let input = "expected HashMap<String, Vec<Option<i32>>>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Map<string, Array<i32 | null>>");
    }

    // --- Test 19: Multiple types in one message ---
    #[test]
    fn test_translate_multiple_types_in_message() {
        let input = "expected Vec<String>, found Option<i32>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Array<string>, found i32 | null");
    }

    // --- Test 20: impl FnMut translation ---
    #[test]
    fn test_translate_impl_fn_mut_to_arrow() {
        let input = "expected impl FnMut(i32) -> bool";
        let result = translate_type_names(input);
        assert_eq!(result, "expected (i32) => bool");
    }

    // --- Test 21: Result with single type argument ---
    #[test]
    fn test_translate_result_single_arg() {
        let input = "expected Result<i32>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected i32 (throws)");
    }

    // --- Correctness Scenario 1: Type mismatch translation ---
    #[test]
    fn test_correctness_type_mismatch_translation() {
        let input = r#"error[E0308]: mismatched types
 --> src/main.rs:5:10
  |
5 |     let x: String = 42;
  |                      ^^ expected String, found integer
"#;
        let result = translate_rustc_errors(input, None, None, None);
        assert!(
            result.contains("expected string, found integer"),
            "correctness scenario 1 failed: {result}"
        );
    }

    // --- Correctness Scenario 2: Fallback on unknown error ---
    #[test]
    fn test_correctness_fallback_unknown_error() {
        let input = "error[E9999]: some future error\n --> src/main.rs:3:1\n";
        let result = translate_rustc_errors(input, None, None, None);
        assert!(
            result.starts_with(RAW_HEADER),
            "correctness scenario 2 failed: expected raw header, got:\n{result}"
        );
        assert!(
            result.contains("error[E9999]: some future error"),
            "original error should be preserved in:\n{result}"
        );
    }

    // =========================================================================
    // Task 040: Enhanced error message tests
    // =========================================================================

    // Task 040 Test 4: Line number translation —
    // src/main.rs:5:10 in stderr is remapped to the .rts line.
    #[test]
    fn test_translate_line_number_remapped_via_source_map() {
        use rsc_syntax::span::Span;
        // Source map: 5 entries (lines 0-4). Line 4 (1-based: line 5) maps to .rts span at byte 20.
        // The .rts source has line 3 starting at byte 20 ("line1\nline2\nline3\n...")
        let source_map: Vec<Option<Span>> = vec![
            None,
            Some(Span::new(0, 5)),
            Some(Span::new(6, 11)),
            Some(Span::new(12, 19)),
            Some(Span::new(20, 30)),
        ];
        let rts_source = "line1\nline2\nline3\nline4\nline5\n";
        // Line 5 in .rs → source_map[4] → Span(20, 30) → byte 20 in rts_source → line 4
        let stderr = "error: something at src/main.rs:5:10\n";
        let result = translate_rustc_errors(
            stderr,
            Some(&source_map),
            Some(rts_source),
            Some("src/index.rts"),
        );
        assert!(
            result.contains("src/index.rts:4:10"),
            "expected src/index.rts:4:10 in output, got:\n{result}"
        );
    }

    // Task 040 Test 5: File name translation —
    // src/main.rs is replaced with src/index.rts.
    #[test]
    fn test_translate_file_name_remapped_to_rts() {
        use rsc_syntax::span::Span;
        let source_map: Vec<Option<Span>> = vec![None, Some(Span::new(0, 5))];
        let rts_source = "line1\n";
        let stderr = " --> src/main.rs:2:5\n";
        let result = translate_rustc_errors(
            stderr,
            Some(&source_map),
            Some(rts_source),
            Some("src/index.rts"),
        );
        assert!(
            result.contains("src/index.rts"),
            "expected src/index.rts in output, got:\n{result}"
        );
        assert!(
            !result.contains("src/main.rs"),
            "src/main.rs should be replaced, got:\n{result}"
        );
    }

    // Task 040 Test 6: Type name translation preserved —
    // Existing type translations still work with the new signature.
    #[test]
    fn test_translate_type_names_preserved_with_source_map() {
        use rsc_syntax::span::Span;
        let source_map: Vec<Option<Span>> = vec![None, Some(Span::new(0, 5))];
        let stderr = "error: expected String at src/main.rs:2:5\n";
        let result = translate_rustc_errors(
            stderr,
            Some(&source_map),
            Some("x\n"),
            Some("src/index.rts"),
        );
        assert!(
            result.contains("expected string"),
            "String should be translated to string, got:\n{result}"
        );
    }

    // Task 040 Test 7: Fallback on no source map —
    // When no source map is provided, behavior matches Task 034.
    #[test]
    fn test_translate_fallback_no_source_map() {
        let stderr = "error: expected String\n";
        let result = translate_rustc_errors(stderr, None, None, None);
        assert!(
            result.contains("expected string"),
            "type translation should work without source map, got:\n{result}"
        );
        assert!(
            result.starts_with(TRANSLATED_HEADER),
            "should use translated header, got:\n{result}"
        );
    }

    // Task 040 Test 8: Fallback on unmapped line —
    // Lines without a source map entry keep the .rs reference.
    #[test]
    fn test_translate_fallback_unmapped_line_keeps_rs_reference() {
        use rsc_syntax::span::Span;
        // Source map has 2 entries, but .rs line 5 is out of bounds.
        let source_map: Vec<Option<Span>> = vec![None, Some(Span::new(0, 5))];
        let stderr = " --> src/main.rs:5:10\n";
        let result = translate_rustc_errors(
            stderr,
            Some(&source_map),
            Some("x\n"),
            Some("src/index.rts"),
        );
        assert!(
            result.contains("src/main.rs:5:10"),
            "unmapped line should keep .rs reference, got:\n{result}"
        );
    }

    // Task 040 Test 10: Pipeline integration — verified at the pipeline level in pipeline.rs.
    // (See compile_source test below that checks source_map_lines is populated.)

    // =========================================================================
    // Task 040: Correctness scenarios
    // =========================================================================

    // Correctness Scenario 1: Line number remapping
    #[test]
    fn test_correctness_line_number_remapping() {
        use rsc_syntax::span::Span;
        // source map: [None, Some(Span{line:1}), Some(Span{line:3})]
        // The spec says "line 1" and "line 3" which we interpret as byte offsets
        // that resolve to those line numbers in the .rts source.
        // rts_source: "a\nb\nc\nd\n" — line 1 starts at byte 0, line 2 at byte 2, line 3 at byte 4.
        let source_map: Vec<Option<Span>> = vec![
            None,
            Some(Span::new(0, 1)), // .rts byte 0 = line 1
            Some(Span::new(4, 5)), // .rts byte 4 = line 3
        ];
        let rts_source = "a\nb\nc\nd\n";
        let stderr = "error at src/main.rs:2:5\n";
        let result = translate_rustc_errors(
            stderr,
            Some(&source_map),
            Some(rts_source),
            Some("src/index.rts"),
        );
        assert!(
            result.contains("src/index.rts:1:5"),
            "correctness scenario 1: expected line 1, got:\n{result}"
        );
    }

    // Correctness Scenario 2: Type and line translation combined
    #[test]
    fn test_correctness_type_and_line_translation_combined() {
        use rsc_syntax::span::Span;
        // .rts source: "x\ny\nz\n" — line 1 byte 0, line 2 byte 2, line 3 byte 4
        // source map: line 5 (index 4) maps to byte 4 in rts = line 3
        let source_map: Vec<Option<Span>> = vec![
            None,
            None,
            None,
            None,
            Some(Span::new(4, 5)), // .rs line 5 → .rts byte 4 → line 3
        ];
        let rts_source = "x\ny\nz\n";
        let stderr = "error: expected String at src/main.rs:5:10\n";
        let result = translate_rustc_errors(
            stderr,
            Some(&source_map),
            Some(rts_source),
            Some("src/index.rts"),
        );
        assert!(
            result.contains("expected string"),
            "String should be translated to string, got:\n{result}"
        );
        assert!(
            result.contains("src/index.rts:3:10"),
            "expected src/index.rts:3:10, got:\n{result}"
        );
    }

    // Correctness Scenario 3: No source map fallback
    // Existing Task 034 tests continue to pass when source_map is None.
    #[test]
    fn test_correctness_no_source_map_fallback() {
        // This is the same as the Task 034 tests — just verifying they still work.
        let input = r#"error[E0308]: mismatched types
 --> src/main.rs:5:10
  |
5 |     let x: String = 42;
  |                      ^^ expected String, found integer
"#;
        let result = translate_rustc_errors(input, None, None, None);
        assert!(
            result.contains("expected string, found integer"),
            "correctness scenario 3: type translation should work without source map, got:\n{result}"
        );
        assert!(
            result.starts_with(TRANSLATED_HEADER),
            "correctness scenario 3: expected translated header, got:\n{result}"
        );
    }

    // Task 040: byte_offset_to_line utility tests
    #[test]
    fn test_byte_offset_to_line_first_line() {
        let source = "hello\nworld\n";
        assert_eq!(byte_offset_to_line(source, 0), 1);
        assert_eq!(byte_offset_to_line(source, 3), 1);
    }

    #[test]
    fn test_byte_offset_to_line_second_line() {
        let source = "hello\nworld\n";
        assert_eq!(byte_offset_to_line(source, 6), 2);
        assert_eq!(byte_offset_to_line(source, 10), 2);
    }

    #[test]
    fn test_byte_offset_to_line_third_line() {
        let source = "a\nb\nc\n";
        assert_eq!(byte_offset_to_line(source, 4), 3);
    }

    #[test]
    fn test_byte_offset_to_line_beyond_end() {
        let source = "a\nb\n";
        // Beyond end is clamped to source length. "a\nb\n" has 2 newlines,
        // so the clamped range covers all of them → line 3 (1-based).
        assert_eq!(byte_offset_to_line(source, 100), 3);
    }

    // =========================================================================
    // Task 062: Phase 5 tooling catch-up — new type translations
    // =========================================================================

    // --- Arc<Mutex<T>> → shared<T> ---
    #[test]
    fn test_translate_arc_mutex_to_shared() {
        let input = "expected Arc<Mutex<i32>>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected shared<i32>");
    }

    #[test]
    fn test_translate_arc_mutex_nested_to_shared() {
        let input = "expected Arc<Mutex<Vec<String>>>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected shared<Array<string>>");
    }

    // --- Box<dyn Trait> → Trait ---
    #[test]
    fn test_translate_box_dyn_to_trait_name() {
        let input = "expected Box<dyn Serializable>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Serializable");
    }

    #[test]
    fn test_translate_box_dyn_in_context() {
        let input = "found Box<dyn Display>, expected String";
        let result = translate_type_names(input);
        assert_eq!(result, "found Display, expected string");
    }

    #[test]
    fn test_translate_tuple_type_to_bracket_syntax() {
        let input = "expected (String, i32)";
        let result = translate_type_names(input);
        assert_eq!(result, "expected [string, i32]");
    }

    #[test]
    fn test_translate_tuple_type_three_elements() {
        let input = "found (String, i32, bool)";
        let result = translate_type_names(input);
        assert_eq!(result, "found [string, i32, bool]");
    }

    // ---------------------------------------------------------------
    // Task 066: Async pattern error translations
    // ---------------------------------------------------------------

    #[test]
    fn test_translate_tokio_select_mentions_promise_race() {
        let input = "error in tokio::select! macro expansion";
        let result = translate_type_names(input);
        assert!(
            result.contains("Promise.race"),
            "should mention Promise.race: {result}"
        );
    }

    #[test]
    fn test_translate_futures_select_ok_mentions_promise_any() {
        let input = "error in futures::future::select_ok call";
        let result = translate_type_names(input);
        assert!(
            result.contains("Promise.any"),
            "should mention Promise.any: {result}"
        );
    }

    #[test]
    fn test_translate_stream_ext_mentions_for_await() {
        let input = "the trait `StreamExt` is not implemented";
        let result = translate_type_names(input);
        assert!(
            result.contains("for await"),
            "should mention for await: {result}"
        );
    }

    // --- Color header tests ---
    #[test]
    fn test_color_header_never_returns_plain_text() {
        let result = color_header("error header", ColorMode::Never);
        assert_eq!(result, "error header");
        assert!(!result.contains("\x1b["));
    }

    #[test]
    fn test_color_header_always_returns_ansi_codes() {
        let result = color_header("error header", ColorMode::Always);
        assert!(result.contains("\x1b[1;31m"), "should contain bold red");
        assert!(result.contains("\x1b[0m"), "should contain reset");
        assert!(result.contains("error header"), "should contain the text");
    }

    #[test]
    fn test_translate_rustc_errors_colored_never_matches_plain() {
        let input = "error[E0308]: expected String, found i32";
        let plain = translate_rustc_errors(input, None, None, None);
        let colored_never =
            translate_rustc_errors_colored(input, None, None, None, ColorMode::Never);
        assert_eq!(plain, colored_never);
    }

    #[test]
    fn test_translate_rustc_errors_colored_always_adds_ansi() {
        let input = "error[E0308]: expected String, found i32";
        let colored = translate_rustc_errors_colored(input, None, None, None, ColorMode::Always);
        assert!(
            colored.contains("\x1b["),
            "colored output should contain ANSI codes"
        );
        assert!(colored.contains("string"), "should still translate types");
    }

    // =========================================================================
    // Task 164b: Error enrichment annotations
    // =========================================================================

    #[test]
    fn test_enrich_moved_value() {
        let msg = "error[E0382]: use of moved value: `x`";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match moved value pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("moves it by default"),
            "hint should explain move semantics: {hint}"
        );
        assert!(
            hint.contains("clone"),
            "hint should suggest clone: {hint}"
        );
    }

    #[test]
    fn test_enrich_moved_value_variant() {
        let msg = "value used here after move";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match 'value used here after move'");
    }

    #[test]
    fn test_enrich_cannot_move_out() {
        let msg = "cannot move out of `self.field`";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match cannot-move-out pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("cloning"),
            "hint should suggest cloning: {hint}"
        );
    }

    #[test]
    fn test_enrich_borrow_of_moved() {
        let msg = "borrow of moved value: `x`";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match borrow-of-moved pattern");
    }

    #[test]
    fn test_enrich_borrow_conflict() {
        let msg = "cannot borrow `x` as immutable because it is also borrowed as mutable";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match borrow conflict pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("exclusive access"),
            "hint should explain exclusive access: {hint}"
        );
        assert!(
            hint.contains("Restructure"),
            "hint should suggest restructuring: {hint}"
        );
    }

    #[test]
    fn test_enrich_cannot_borrow_mutable() {
        let msg = "cannot borrow `data` as mutable";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match cannot-borrow-mutable pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("modify"),
            "hint should mention modifying: {hint}"
        );
    }

    #[test]
    fn test_enrich_type_mismatch() {
        let msg = "error[E0308]: mismatched types";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match type mismatch pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("return type"),
            "hint should mention return type: {hint}"
        );
    }

    #[test]
    fn test_enrich_expected_found() {
        let msg = "expected `i32`, found `String`";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match expected/found pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("Type mismatch"),
            "hint should say type mismatch: {hint}"
        );
    }

    #[test]
    fn test_enrich_trait_not_implemented() {
        let msg = "the trait `Clone` is not implemented for `MyStruct`";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match trait-not-implemented pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("derives"),
            "hint should suggest derives: {hint}"
        );
    }

    #[test]
    fn test_enrich_lifetime() {
        let msg = "error: `x` does not live long enough";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match lifetime pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("scope"),
            "hint should mention scope: {hint}"
        );
        assert!(
            hint.contains("owned value"),
            "hint should suggest owned value: {hint}"
        );
    }

    #[test]
    fn test_enrich_lifetime_keyword() {
        let msg = "lifetime mismatch in function signature";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match 'lifetime' keyword");
    }

    #[test]
    fn test_enrich_not_found() {
        let msg = "error[E0425]: cannot find value `foo` in this scope";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match not-found pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("typos"),
            "hint should mention typos: {hint}"
        );
        assert!(
            hint.contains("import"),
            "hint should mention imports: {hint}"
        );
    }

    #[test]
    fn test_enrich_not_found_in_scope() {
        let msg = "`bar` not found in this scope";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match 'not found in this scope'");
    }

    #[test]
    fn test_enrich_type_not_found() {
        let msg = "error[E0412]: cannot find type `Foo` in this scope";
        let hint = enrich_error_message(msg);
        assert!(hint.is_some(), "should match cannot-find-type pattern");
        let hint = hint.unwrap();
        assert!(
            hint.contains("type name"),
            "hint should mention type name: {hint}"
        );
    }

    #[test]
    fn test_enrich_no_match_returns_none() {
        let msg = "Compiling myproject v0.1.0";
        let hint = enrich_error_message(msg);
        assert!(hint.is_none(), "non-error message should return None");

        let msg2 = "warning: unused variable: `x`";
        let hint2 = enrich_error_message(msg2);
        assert!(hint2.is_none(), "warning should return None");
    }

    #[test]
    fn test_synthesized_code_nearest_span() {
        use rsc_syntax::span::Span;
        // Source map: [Some(0..5), Some(6..11), None, None, None]
        // Error on rs line 4 (index 3 = None) → walks back to index 1 → byte 6 → line 2
        let source_map: Vec<Option<Span>> = vec![
            Some(Span::new(0, 5)),
            Some(Span::new(6, 11)),
            None,
            None,
            None,
        ];
        let rts_source = "line1\nline2\nline3\n";
        let annotation = synthesized_code_annotation(&source_map, 4, Some(rts_source));
        assert!(
            annotation.is_some(),
            "should produce annotation for unmapped line"
        );
        let annotation = annotation.unwrap();
        assert!(
            annotation.contains("near line 2"),
            "should reference nearest .rts line 2: {annotation}"
        );
        assert!(
            annotation.contains("generated by the RustScript compiler"),
            "should mention generated code: {annotation}"
        );
    }

    #[test]
    fn test_synthesized_code_all_null() {
        let source_map: Vec<Option<Span>> = vec![None, None, None];
        let annotation = synthesized_code_annotation(&source_map, 2, Some("a\nb\n"));
        assert!(
            annotation.is_some(),
            "should produce annotation even with all-null map"
        );
        let annotation = annotation.unwrap();
        assert!(
            annotation.contains("generated by the RustScript compiler"),
            "should produce generic message: {annotation}"
        );
        // Should NOT contain "near line" since no mapped line was found
        assert!(
            !annotation.contains("near line"),
            "should not reference a line when all null: {annotation}"
        );
    }

    #[test]
    fn test_synthesized_code_mapped_line_returns_none() {
        use rsc_syntax::span::Span;
        let source_map: Vec<Option<Span>> = vec![Some(Span::new(0, 5)), Some(Span::new(6, 11))];
        let annotation = synthesized_code_annotation(&source_map, 1, Some("line1\nline2\n"));
        assert!(
            annotation.is_none(),
            "mapped line should not produce annotation"
        );
    }

    #[test]
    fn test_enrichment_wired_into_output() {
        // Full pipeline: an error with "use of moved value" should get a hint in the output
        let stderr = "error[E0382]: use of moved value: `x`\n --> src/main.rs:5:10\n";
        let result = translate_rustc_errors(stderr, None, None, None);
        assert!(
            result.contains("hint:"),
            "output should contain hint annotation: {result}"
        );
        assert!(
            result.contains("moves it by default"),
            "output should contain the enrichment text: {result}"
        );
    }

    #[test]
    fn test_enrichment_synthesized_annotation_in_output() {
        use rsc_syntax::span::Span;
        // Source map where line 3 is unmapped, line 1 is mapped
        let source_map: Vec<Option<Span>> = vec![
            Some(Span::new(0, 5)),
            None,
            None,
        ];
        let rts_source = "line1\nline2\nline3\n";
        // Error pointing at .rs line 3 (unmapped)
        let stderr = "error: something bad\n --> src/main.rs:3:5\n";
        let result = translate_rustc_errors(
            stderr,
            Some(&source_map),
            Some(rts_source),
            Some("src/index.rts"),
        );
        assert!(
            result.contains("generated by the RustScript compiler"),
            "output should contain synthesized annotation: {result}"
        );
    }
}
