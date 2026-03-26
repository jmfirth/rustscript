//! Error message quality tests (Phase 3 integration).
//!
//! Verifies that error messages from the compilation pipeline reference
//! `.rts` file positions and use RustScript type terminology rather than
//! raw Rust types.

use rsc_driver::translate_rustc_errors;
use rsc_syntax::span::Span;

// ---------------------------------------------------------------------------
// 1. Error translation: String → string
// ---------------------------------------------------------------------------

#[test]
fn test_error_translation_string_becomes_string() {
    let rustc_stderr = "error[E0308]: mismatched types\n  --> src/main.rs:3:5\n   |\n3  |     let x: String = 42;\n   |            ^^^^^^ expected String, found integer\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("string"),
        "should translate String to string, got: {translated}"
    );
    assert!(
        !translated.contains("String"),
        "should not contain Rust String type, got: {translated}"
    );
}

// ---------------------------------------------------------------------------
// 2. Error translation: Vec<T> → Array<T>
// ---------------------------------------------------------------------------

#[test]
fn test_error_translation_vec_becomes_array() {
    let rustc_stderr = "error: expected Vec<i32>, found String\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("Array<i32>"),
        "should translate Vec<i32> to Array<i32>, got: {translated}"
    );
}

// ---------------------------------------------------------------------------
// 3. Error translation: HashMap → Map
// ---------------------------------------------------------------------------

#[test]
fn test_error_translation_hashmap_becomes_map() {
    let rustc_stderr = "error: expected HashMap<String, i32>\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("Map<"),
        "should translate HashMap to Map, got: {translated}"
    );
}

// ---------------------------------------------------------------------------
// 4. Error translation: Option<T> → T | null
// ---------------------------------------------------------------------------

#[test]
fn test_error_translation_option_becomes_nullable() {
    let rustc_stderr = "error: expected Option<i32>, found i32\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("| null") || translated.contains("null"),
        "should translate Option<i32> to nullable form, got: {translated}"
    );
}

// ---------------------------------------------------------------------------
// 5. File reference remapping: .rs → .rts with line numbers
// ---------------------------------------------------------------------------

#[test]
fn test_error_references_rts_line_numbers() {
    let rustc_stderr = "error[E0308]: mismatched types\n  --> src/main.rs:3:5\n   |\n3  |     let x: i32 = true;\n   |            ^^^ expected i32, found bool\n";

    // Source map: .rs line 2 (0-indexed) → .rts byte offset 20 (line 2 in a 3-line source)
    let source_map = vec![
        Some(Span::new(0, 10)),
        Some(Span::new(11, 19)),
        Some(Span::new(20, 30)),
    ];
    let rts_source = "function main() {\n  const x = 1;\n  const y = true;\n}\n";

    let translated = translate_rustc_errors(
        rustc_stderr,
        Some(&source_map),
        Some(rts_source),
        Some("src/index.rts"),
    );

    assert!(
        translated.contains("src/index.rts"),
        "should reference .rts file, got: {translated}"
    );
    assert!(
        !translated.contains("src/main.rs:3"),
        "should not contain .rs file reference, got: {translated}"
    );
}

// ---------------------------------------------------------------------------
// 6. Error translation preserves header
// ---------------------------------------------------------------------------

#[test]
fn test_error_translation_adds_rustscript_header() {
    let rustc_stderr = "error: expected String, found i32\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("RustScript compilation error"),
        "translated errors should have RustScript header, got: {translated}"
    );
}

// ---------------------------------------------------------------------------
// 7. Untranslatable errors use raw header
// ---------------------------------------------------------------------------

#[test]
fn test_error_untranslatable_uses_raw_header() {
    let rustc_stderr = "error: some totally unknown error with no Rust types\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("rustc error"),
        "untranslatable errors should use raw header, got: {translated}"
    );
}

// ---------------------------------------------------------------------------
// 8. Compile diagnostics reference .rts positions (parse errors)
// ---------------------------------------------------------------------------

#[test]
fn test_compile_diagnostics_reference_rts_positions() {
    // Compile broken code and verify diagnostics have spans
    let source = "function foo( { }";
    let result = rsc_driver::compile_source(source, "test.rts");

    assert!(result.has_errors, "source with syntax error should fail");
    assert!(!result.diagnostics.is_empty(), "should produce diagnostics");

    // Verify the diagnostic has a label with a span
    let first_diag = &result.diagnostics[0];
    assert!(
        !first_diag.labels.is_empty(),
        "diagnostic should have labels with position information"
    );

    // The span should be within the source bounds
    let label = &first_diag.labels[0];
    let source_len = source.len() as u32;
    assert!(
        label.span.start.0 <= source_len,
        "span start {} should be within source length {}",
        label.span.start.0,
        source_len
    );
}

// ---------------------------------------------------------------------------
// 9. Error translation: &str → string (reference)
// ---------------------------------------------------------------------------

#[test]
fn test_error_translation_str_ref_becomes_string_reference() {
    let rustc_stderr = "error: expected &str, found i32\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("string (reference)"),
        "should translate &str to string (reference), got: {translated}"
    );
}

// ---------------------------------------------------------------------------
// Task 062: Phase 5 tooling catch-up — new type translations
// ---------------------------------------------------------------------------

#[test]
fn test_error_translation_arc_mutex_becomes_shared() {
    let rustc_stderr = "error: expected Arc<Mutex<i32>>, found String\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("shared<i32>"),
        "should translate Arc<Mutex<i32>> to shared<i32>, got: {translated}"
    );
}

#[test]
fn test_error_translation_box_dyn_becomes_trait_name() {
    let rustc_stderr = "error: expected Box<dyn Serializable>, found i32\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("Serializable"),
        "should translate Box<dyn Serializable> to Serializable, got: {translated}"
    );
    assert!(
        !translated.contains("Box<dyn"),
        "should not contain Box<dyn wrapper, got: {translated}"
    );
}
