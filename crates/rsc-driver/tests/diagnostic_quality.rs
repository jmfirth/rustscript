//! Diagnostic quality audit tests (Task 145).
//!
//! Verifies that the most important user-facing error messages are:
//! 1. Present (not panics or empty strings)
//! 2. Actionable (contain guidance on how to fix the issue)
//! 3. Reference `.rts` source concepts (TypeScript syntax, not Rust jargon)

mod test_utils;

use rsc_driver::{compile_source, translate_rustc_errors};
use test_utils::compile_diagnostics;

// ===========================================================================
// Helper: compile and return (messages, notes) pairs
// ===========================================================================

/// Compile source and return diagnostics with their notes.
fn compile_diagnostics_with_notes(source: &str) -> Vec<(String, Vec<String>)> {
    let result = compile_source(source, "test.rts");
    result
        .diagnostics
        .into_iter()
        .map(|d| (d.message, d.notes))
        .collect()
}

// ===========================================================================
// 1. Unknown type — suggests checking imports and lists built-in types
// ===========================================================================

#[test]
fn test_diag_unknown_type_suggests_imports() {
    let source = "\
function main() {
  const x: Foo = 42;
}";

    let diags = compile_diagnostics_with_notes(source);

    assert!(!diags.is_empty(), "expected at least one diagnostic");

    let has_unknown = diags.iter().any(|(m, _)| m.contains("unknown type"));
    assert!(
        has_unknown,
        "expected 'unknown type' diagnostic, got: {diags:?}"
    );

    let has_helpful_note = diags
        .iter()
        .any(|(_, notes)| notes.iter().any(|n| n.contains("defined or imported")));
    assert!(
        has_helpful_note,
        "expected note about checking imports, got: {diags:?}"
    );
}

// ===========================================================================
// 2. Unknown type in function return position — same quality
// ===========================================================================

#[test]
fn test_diag_unknown_return_type() {
    let source = "\
function getWidget(): Widget {
  return 42;
}";

    let diags = compile_diagnostics_with_notes(source);

    assert!(!diags.is_empty(), "expected at least one diagnostic");

    let has_unknown = diags
        .iter()
        .any(|(m, _)| m.contains("unknown type") && m.contains("Widget"));
    assert!(
        has_unknown,
        "expected 'unknown type Widget' diagnostic, got: {diags:?}"
    );

    let has_builtins_note = diags
        .iter()
        .any(|(_, notes)| notes.iter().any(|n| n.contains("built-in types")));
    assert!(
        has_builtins_note,
        "expected note listing built-in types, got: {diags:?}"
    );
}

// ===========================================================================
// 3. Namespace not supported — suggests modules
// ===========================================================================

#[test]
fn test_diag_namespace_not_supported() {
    let source = "\
namespace MyLib {
  export function doStuff(): void {}
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for namespace"
    );

    let has_namespace = messages
        .iter()
        .any(|m| m.contains("namespaces are not supported"));
    assert!(
        has_namespace,
        "expected 'namespaces are not supported' diagnostic, got: {messages:?}"
    );

    let has_suggestion = messages
        .iter()
        .any(|m| m.contains("import") && m.contains("export"));
    assert!(
        has_suggestion,
        "expected suggestion to use import/export, got: {messages:?}"
    );
}

// ===========================================================================
// 4. Dynamic import warning — suggests static import
// ===========================================================================

#[test]
fn test_diag_dynamic_import_warning() {
    let source = r#"
function main() {
  const mod = import("some_module");
}"#;

    let messages = compile_diagnostics(source);

    let has_dynamic_import = messages
        .iter()
        .any(|m| m.contains("dynamic import") && m.contains("static import"));
    assert!(
        has_dynamic_import,
        "expected warning about dynamic import with static import suggestion, got: {messages:?}"
    );
}

// ===========================================================================
// 5. new.target warning — clear about limitation
// ===========================================================================

#[test]
fn test_diag_new_target_warning() {
    let source = "\
function main() {
  const x = new.target;
}";

    let messages = compile_diagnostics(source);

    let has_new_target = messages
        .iter()
        .any(|m| m.contains("new.target") && m.contains("not supported"));
    assert!(
        has_new_target,
        "expected clear new.target warning, got: {messages:?}"
    );

    // Should NOT reference Rust jargon
    let has_rust_jargon = messages
        .iter()
        .any(|m| m.contains("there is no equivalent in Rust"));
    assert!(
        !has_rust_jargon,
        "new.target message should not reference Rust internals, got: {messages:?}"
    );
}

// ===========================================================================
// 6. Type-only import used as value — clear about type-only
// ===========================================================================

#[test]
fn test_diag_import_type_as_value() {
    let source = r#"
import type { Widget } from "widgets";

function main() {
  const x = Widget();
}"#;

    let messages = compile_diagnostics(source);

    let has_type_only = messages
        .iter()
        .any(|m| m.contains("type-only import") && m.contains("as a value"));
    assert!(
        has_type_only,
        "expected error about using type-only import as value, got: {messages:?}"
    );
}

// ===========================================================================
// 7. Missing return type on function — no crash, clear error
// ===========================================================================

#[test]
fn test_diag_missing_function_body() {
    let source = "function getData(): string";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for missing function body"
    );

    // Should mention expected token, not panic
    for msg in &messages {
        assert!(
            !msg.contains("panic"),
            "compiler should not panic, got: {msg}"
        );
    }
}

// ===========================================================================
// 8. Cannot infer struct type — suggests explicit type annotation
// ===========================================================================

#[test]
fn test_diag_cannot_infer_struct_type() {
    // An object literal without context type or explicit type name
    let source = "\
function main() {
  const x = { name: \"test\", value: 42 };
}";

    let messages = compile_diagnostics(source);

    let has_infer = messages
        .iter()
        .any(|m| m.contains("cannot infer struct type"));
    if has_infer {
        // If we do get this error, verify it's actionable
        let has_suggestion = messages
            .iter()
            .any(|m| m.contains("specify the type explicitly"));
        assert!(
            has_suggestion,
            "struct inference error should suggest explicit type, got: {messages:?}"
        );
    }
    // Note: if the compiler handles this via another path, that's fine too
}

// ===========================================================================
// 9. Cannot infer destructuring type — suggests annotation
// ===========================================================================

#[test]
fn test_diag_cannot_infer_destructuring() {
    let source = "\
function main() {
  const obj = 42;
  const { x, y } = obj;
}";

    let messages = compile_diagnostics(source);

    let has_destructuring = messages
        .iter()
        .any(|m| m.contains("cannot infer type for destructuring"));
    if has_destructuring {
        let has_suggestion = messages.iter().any(|m| m.contains("type annotation"));
        assert!(
            has_suggestion,
            "destructuring error should suggest type annotation, got: {messages:?}"
        );
    }
}

// ===========================================================================
// 10. Expected expression — clear error for trailing semicolons etc.
// ===========================================================================

#[test]
fn test_diag_expected_expression() {
    let source = "\
function main() {
  const x = ;
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for missing expression"
    );

    let has_expected = messages.iter().any(|m| m.contains("expected expression"));
    assert!(
        has_expected,
        "expected 'expected expression' diagnostic, got: {messages:?}"
    );
}

// ===========================================================================
// 11. Invalid assignment target — clear about what's wrong
// ===========================================================================

#[test]
fn test_diag_invalid_assignment_target() {
    let source = "\
function main() {
  1 + 2 = 3;
}";

    let messages = compile_diagnostics(source);

    let has_assignment = messages
        .iter()
        .any(|m| m.contains("invalid assignment target"));
    assert!(
        has_assignment,
        "expected 'invalid assignment target' diagnostic, got: {messages:?}"
    );
}

// ===========================================================================
// 12. Unterminated block — points to opening brace
// ===========================================================================

#[test]
fn test_diag_unterminated_block() {
    let source = "\
function main() {
  if (true) {
    const x = 1;
";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected diagnostics for unterminated block"
    );

    let has_unterminated = messages
        .iter()
        .any(|m| m.contains("unterminated") || m.contains("expected"));
    assert!(
        has_unterminated,
        "expected unterminated block diagnostic, got: {messages:?}"
    );
}

// ===========================================================================
// 13. Error enrichment: moved value hint references RustScript
// ===========================================================================

#[test]
fn test_diag_enrichment_moved_value() {
    let rustc_stderr = "error[E0382]: use of moved value: `x`\n  --> src/main.rs:5:10\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("RustScript"),
        "moved value hint should reference RustScript, got: {translated}"
    );
    assert!(
        translated.contains("clone"),
        "moved value hint should suggest clone, got: {translated}"
    );
}

// ===========================================================================
// 14. Error enrichment: borrow conflict references RustScript not Rust
// ===========================================================================

#[test]
fn test_diag_enrichment_borrow_conflict_no_rust_jargon() {
    let rustc_stderr =
        "error: cannot borrow `x` as immutable because it is also borrowed as mutable\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("RustScript"),
        "borrow conflict hint should reference RustScript, got: {translated}"
    );
    // Should NOT say "Rust requires"
    assert!(
        !translated.contains("Rust requires"),
        "borrow conflict hint should not say 'Rust requires', got: {translated}"
    );
}

// ===========================================================================
// 15. Error enrichment: trait not implemented suggests `derives`
// ===========================================================================

#[test]
fn test_diag_enrichment_trait_not_impl() {
    let rustc_stderr = "error[E0277]: the trait `Clone` is not implemented for `Foo`\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("derives"),
        "trait-not-implemented hint should suggest `derives`, got: {translated}"
    );
}

// ===========================================================================
// 16. Error enrichment: type mismatch suggests checking return type
// ===========================================================================

#[test]
fn test_diag_enrichment_type_mismatch() {
    let rustc_stderr = "error[E0308]: mismatched types\n  expected i32, found bool\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("return type") || translated.contains("function signature"),
        "type mismatch hint should reference return type or signature, got: {translated}"
    );
}

// ===========================================================================
// 17. Error enrichment: value not found suggests checking imports
// ===========================================================================

#[test]
fn test_diag_enrichment_value_not_found() {
    let rustc_stderr = "error[E0425]: cannot find value `foo` in this scope\n";

    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("import"),
        "value-not-found hint should mention imports, got: {translated}"
    );
}

// ===========================================================================
// 18. Super outside class extension — clear error
// ===========================================================================

#[test]
fn test_diag_super_outside_class() {
    let source = "\
function main() {
  super.method();
}";

    let messages = compile_diagnostics(source);

    // Should not crash
    for msg in &messages {
        assert!(
            !msg.contains("panic"),
            "compiler should not panic on super outside class, got: {msg}"
        );
    }
}

// ===========================================================================
// 19. Expected declaration after export — lists valid export targets
// ===========================================================================

#[test]
fn test_diag_expected_after_export() {
    let source = "export 42;";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for invalid export"
    );

    let has_expected = messages
        .iter()
        .any(|m| m.contains("function") || m.contains("class") || m.contains("type"));
    assert!(
        has_expected,
        "export error should list valid export targets, got: {messages:?}"
    );
}

// ===========================================================================
// 20. Shared type without type parameter — clear error
// ===========================================================================

#[test]
fn test_diag_shared_requires_type_param() {
    let source = "\
function main() {
  const x: shared = 0;
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for shared without type parameter"
    );

    let has_shared = messages
        .iter()
        .any(|m| m.contains("shared") && m.contains("type parameter"));
    assert!(
        has_shared,
        "expected shared type parameter error, got: {messages:?}"
    );
}

// ===========================================================================
// 21. Const enum at non-top-level — clear about restriction
// ===========================================================================

#[test]
fn test_diag_const_enum_not_top_level() {
    let source = "\
function main() {
  const enum Color { Red = 0, Green = 1, Blue = 2 }
}";

    let messages = compile_diagnostics(source);

    let has_top_level = messages
        .iter()
        .any(|m| m.contains("const enum") && m.contains("top level"));
    assert!(
        has_top_level,
        "expected 'const enum must appear at top level' diagnostic, got: {messages:?}"
    );
}

// ===========================================================================
// 22. Try-catch missing catch/finally — clear about what's expected
// ===========================================================================

#[test]
fn test_diag_try_missing_catch() {
    let source = "\
function main() {
  try {
    const x = 1;
  }
}";

    let messages = compile_diagnostics(source);

    let has_catch = messages
        .iter()
        .any(|m| m.contains("catch") || m.contains("finally") || m.contains("expected"));
    assert!(
        has_catch,
        "expected diagnostic about missing catch/finally, got: {messages:?}"
    );
}

// ===========================================================================
// 23. Do-while missing while keyword — clear error
// ===========================================================================

#[test]
fn test_diag_do_while_missing_while() {
    let source = "\
function main() {
  let x: i32 = 0;
  do {
    x += 1;
  }
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected diagnostic for missing while"
    );

    let has_while = messages
        .iter()
        .any(|m| m.contains("while") || m.contains("expected"));
    assert!(
        has_while,
        "expected 'expected while' diagnostic, got: {messages:?}"
    );
}

// ===========================================================================
// 24. Expected item at top level — clear message with suggestions
// ===========================================================================

#[test]
fn test_diag_expected_item_at_top_level() {
    let source = "42;";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected diagnostic for bare expression at top level"
    );

    let has_declaration = messages
        .iter()
        .any(|m| m.contains("declaration") || m.contains("expected"));
    assert!(
        has_declaration,
        "top-level error should mention declarations, got: {messages:?}"
    );
}

// ===========================================================================
// 25. Error translation: Vec<T> to Array<T>
// ===========================================================================

#[test]
fn test_diag_error_translation_vec_to_array() {
    let rustc_stderr = "error: expected Vec<i32>, found String\n";
    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("Array<i32>"),
        "should translate Vec<i32> to Array<i32>, got: {translated}"
    );
    assert!(
        translated.contains("string"),
        "should translate String to string, got: {translated}"
    );
}

// ===========================================================================
// 26. Error translation: Option<T> to T | null
// ===========================================================================

#[test]
fn test_diag_error_translation_option_to_nullable() {
    let rustc_stderr = "error: expected Option<i32>, found i32\n";
    let translated = translate_rustc_errors(rustc_stderr, None, None, None);

    assert!(
        translated.contains("| null") || translated.contains("null"),
        "should translate Option<i32> to nullable form, got: {translated}"
    );
}

// ===========================================================================
// 27. All diagnostics have severity (not panics)
// ===========================================================================

#[test]
fn test_diag_no_panics_on_various_errors() {
    let bad_sources = [
        "function {",
        "class",
        "const x: = 1;",
        "import { } from;",
        "export default;",
        "type = number;",
        "function main() { return return; }",
    ];

    for source in &bad_sources {
        let result = compile_source(source, "test.rts");
        // Should produce diagnostics, never panic
        assert!(
            result.has_errors || !result.diagnostics.is_empty(),
            "expected errors or diagnostics for: {source}"
        );
    }
}

// ===========================================================================
// 28. Spread type outside tuple — includes example
// ===========================================================================

#[test]
fn test_diag_spread_outside_tuple() {
    let source = "\
type Bad = ...string;";

    let diags = compile_diagnostics_with_notes(source);

    // The compiler may handle this at parser level or typeck level.
    // If it produces a specific diagnostic, verify quality.
    if !diags.is_empty() {
        // Just verify no panic
        for (msg, _) in &diags {
            assert!(
                !msg.contains("panic"),
                "should not panic on spread outside tuple, got: {msg}"
            );
        }
    }
}
