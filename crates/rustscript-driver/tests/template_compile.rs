//! Template-to-compilation pipeline tests (Phase 3 integration).
//!
//! Verifies that each project template's generated `main.rts` compiles
//! successfully through the RustScript pipeline and, where applicable,
//! builds with `cargo build`.
//!
//! These tests are slow (invoke cargo) and marked `#[ignore]`.

use rustscript_driver::{Project, init_project};

// ---------------------------------------------------------------------------
// 1. Default template compiles
// ---------------------------------------------------------------------------

#[test]
#[ignore] // Slow: compiles with cargo
fn test_template_default_compiles() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    init_project("test-default", tmp.path(), None).expect("init_project failed");

    let project = Project::open(&tmp.path().join("test-default")).expect("open project failed");
    project
        .build(false, None)
        .expect("default template should compile");
}

// ---------------------------------------------------------------------------
// 2. CLI template compiles
// ---------------------------------------------------------------------------

#[test]
#[ignore] // Slow: compiles with cargo
fn test_template_cli_compiles() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    init_project("test-cli", tmp.path(), Some("cli")).expect("init_project failed");

    let project = Project::open(&tmp.path().join("test-cli")).expect("open project failed");
    project
        .build(false, None)
        .expect("cli template should compile");
}

// ---------------------------------------------------------------------------
// 3. Web server template compiles (async main with tokio)
// ---------------------------------------------------------------------------

#[test]
#[ignore] // Slow: compiles with cargo
fn test_template_web_server_compiles() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    init_project("test-web", tmp.path(), Some("web-server")).expect("init_project failed");

    let project = Project::open(&tmp.path().join("test-web")).expect("open project failed");
    project
        .build(false, None)
        .expect("web-server template should compile");
}

// ---------------------------------------------------------------------------
// 4. WASM template compiles as a regular binary
//
// The WASM template normally targets wasm32-unknown-unknown (Phase 4),
// but the source code should compile as a normal binary too.
// ---------------------------------------------------------------------------

#[test]
#[ignore] // Slow: compiles with cargo
fn test_template_wasm_compiles_as_binary() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    init_project("test-wasm", tmp.path(), Some("wasm")).expect("init_project failed");

    let project = Project::open(&tmp.path().join("test-wasm")).expect("open project failed");
    project
        .build(false, None)
        .expect("wasm template should compile as binary");
}
