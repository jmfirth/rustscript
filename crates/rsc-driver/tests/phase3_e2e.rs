//! Phase 3 end-to-end integration tests.
//!
//! Each test exercises multiple Phase 3 features together:
//! templates, formatting, error translation, and CLI commands.
//!
//! Slow tests are marked `#[ignore]`.

mod test_utils;

use rsc_driver::{Project, compile_source, init_project};

// ===========================================================================
// E2E Scenario 1: Template -> format -> build pipeline
//
// Init a CLI template, format its source, verify it still compiles.
// Features: init, fmt, build
// ===========================================================================

#[test]
#[ignore] // Slow: compiles with cargo
fn test_e2e_p3_template_format_build_pipeline() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    init_project("fmt-pipe", tmp.path(), Some("cli")).expect("init_project failed");

    let project_dir = tmp.path().join("fmt-pipe");
    let source_path = project_dir.join("src/index.rts");

    // Read the template source
    let original = std::fs::read_to_string(&source_path).expect("read source");

    // Format it
    let formatted = rsc_fmt::format_source(&original).expect("format should succeed");

    // Write formatted source back
    std::fs::write(&source_path, &formatted).expect("write formatted source");

    // Build — should still compile
    let project = Project::open(&project_dir).expect("open project");
    project
        .build(false)
        .expect("formatted template should still compile");
}

// ===========================================================================
// E2E Scenario 2: Format -> compile roundtrip (Phase 2 async code)
//
// Take async code, format it, compile both versions, verify same output.
// Features: fmt, async compilation, semantic preservation
// ===========================================================================

#[test]
fn test_e2e_p3_format_compile_roundtrip_async() {
    let source = r#"async function greet(): string {
  const name = "world";
  return `Hello, ${name}!`;
}"#;

    // Compile original
    let original_result = compile_source(source, "test.rts");
    assert!(
        !original_result.has_errors,
        "original should compile: {:?}",
        original_result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Format
    let formatted = rsc_fmt::format_source(source).expect("format should succeed");

    // Compile formatted
    let formatted_result = compile_source(&formatted, "test.rts");
    assert!(
        !formatted_result.has_errors,
        "formatted should compile: {:?}",
        formatted_result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Both should produce the same Rust output
    assert_eq!(
        original_result.rust_source, formatted_result.rust_source,
        "formatting should not change compilation output"
    );
}

// ===========================================================================
// E2E Scenario 3: Template init -> check -> error handling
//
// Init a project, break the source, verify rsc check catches it.
// Features: init, check, error diagnostics
// ===========================================================================

#[test]
#[ignore] // Slow: creates filesystem artifacts
fn test_e2e_p3_init_break_check_catches_error() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    init_project("check-test", tmp.path(), None).expect("init_project failed");

    let project_dir = tmp.path().join("check-test");

    // Overwrite source with broken code
    std::fs::write(project_dir.join("src/index.rts"), "function {").expect("write broken source");

    // Open project and compile — should fail
    let source = std::fs::read_to_string(project_dir.join("src/index.rts")).expect("read source");
    let result = compile_source(&source, "index.rts");

    assert!(
        result.has_errors,
        "broken source should produce compilation errors"
    );
    assert!(
        !result.diagnostics.is_empty(),
        "should produce at least one diagnostic"
    );
}

// ===========================================================================
// E2E Scenario 4: Full init -> build -> run pipeline with CLI template
//
// Features: init with template, build, run
// ===========================================================================

#[test]
#[ignore] // Slow: compiles with cargo
fn test_e2e_p3_cli_template_full_pipeline() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    init_project("cli-full", tmp.path(), Some("cli")).expect("init_project failed");

    let project_dir = tmp.path().join("cli-full");
    let project = Project::open(&project_dir).expect("open project");

    // Build
    project.build(false).expect("build should succeed");

    // Run
    let status = project.run(&[]).expect("run should succeed");
    assert!(status.success(), "running CLI template should succeed");
}

// ===========================================================================
// E2E Scenario 5: Source map populated after compile
//
// Compile valid code and verify the source map lines are populated,
// which enables error translation and LSP position mapping.
// Features: compilation pipeline, source map
// ===========================================================================

#[test]
fn test_e2e_p3_source_map_populated_after_compile() {
    let source = "\
function add(a: i32, b: i32): i32 {
  return a + b;
}

function main() {
  console.log(add(1, 2));
}";

    let result = compile_source(source, "test.rts");
    assert!(!result.has_errors, "valid code should compile");

    // Source map should have entries for the generated lines
    assert!(
        !result.source_map_lines.is_empty(),
        "source map should be populated after compilation"
    );

    // At least some entries should map back to .rts spans
    let mapped_count = result
        .source_map_lines
        .iter()
        .filter(|e| e.is_some())
        .count();
    assert!(
        mapped_count > 0,
        "at least some generated lines should map back to .rts spans, got 0 mapped out of {}",
        result.source_map_lines.len()
    );
}

// ===========================================================================
// E2E Scenario 6: Error translation integration
//
// Compile broken code, get diagnostics, verify error messages are
// meaningful and reference the right positions.
// Features: compilation pipeline, error diagnostics, error translation
// ===========================================================================

#[test]
fn test_e2e_p3_error_translation_integration() {
    // rustc-style error mentioning Rust types
    let rustc_stderr = "error[E0308]: mismatched types\n  --> src/main.rs:2:5\n   |\n2  |     let x: String = 42;\n   |            ^^^^^^ expected String, found integer\n";

    // Source map: .rs line 1 (0-indexed) -> .rts byte span
    let source_map = vec![
        Some(rsc_syntax::span::Span::new(0, 10)),
        Some(rsc_syntax::span::Span::new(11, 30)),
    ];
    let rts_source = "function main() {\n  const x: string = 42;\n}\n";

    let translated = rsc_driver::translate_rustc_errors(
        rustc_stderr,
        Some(&source_map),
        Some(rts_source),
        Some("src/index.rts"),
    );

    // Should use RustScript type names
    assert!(
        translated.contains("string"),
        "should translate String to string, got: {translated}"
    );

    // Should reference .rts file
    assert!(
        translated.contains("src/index.rts"),
        "should reference .rts file, got: {translated}"
    );
}
