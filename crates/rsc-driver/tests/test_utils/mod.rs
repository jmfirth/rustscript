//! Shared test helpers for RustScript integration tests.
//!
//! Provides three core operations:
//! - [`compile_to_rust`]: fast — compiles `.rts` source to generated `.rs` string
//! - [`compile_and_run`]: slow — compiles, builds with cargo, runs, returns stdout
//! - [`compile_diagnostics`]: fast — compiles and returns diagnostic messages

use std::fs;
use std::process::Command;

use rsc_driver::compile_source;

/// Compile a `.rts` source string and return the generated `.rs` output.
///
/// Panics if compilation produces errors.
pub fn compile_to_rust(rts_source: &str) -> String {
    let result = compile_source(rts_source, "test.rts");

    assert!(
        !result.has_errors,
        "compilation failed with errors: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    result.rust_source
}

/// Compile a `.rts` source string, write to a temp project, build with cargo,
/// run, and return stdout.
///
/// This is necessarily slow (invokes cargo) — tests using it should be `#[ignore]`.
///
/// Panics if compilation, building, or running fails.
pub fn compile_and_run(rts_source: &str) -> String {
    let rust_source = compile_to_rust(rts_source);
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");

    let src_dir = tmp_dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("failed to create src dir");

    // Write Cargo.toml — [workspace] prevents Cargo from walking up to
    // the rsc workspace root, which is virtual and would confuse cargo.
    let cargo_toml =
        "[package]\nname = \"rsc-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n";
    fs::write(tmp_dir.path().join("Cargo.toml"), cargo_toml).expect("failed to write Cargo.toml");

    // Write the generated Rust source.
    fs::write(src_dir.join("main.rs"), &rust_source).expect("failed to write main.rs");

    // Build and run with cargo.
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .current_dir(tmp_dir.path())
        .output()
        .expect("failed to run cargo");

    assert!(
        output.status.success(),
        "cargo run failed.\nstdout: {}\nstderr: {}\ngenerated Rust:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        rust_source,
    );

    String::from_utf8(output.stdout).expect("stdout is not valid utf-8")
}

/// Compile a `.rts` source string and return the full [`CompileResult`].
///
/// Useful for tests that need to inspect `needs_async_runtime`, `crate_dependencies`,
/// diagnostics, or other fields beyond just the Rust source.
///
/// Panics if compilation produces errors.
pub fn compile_result(rts_source: &str) -> rsc_driver::CompileResult {
    let result = compile_source(rts_source, "test.rts");

    assert!(
        !result.has_errors,
        "compilation failed with errors: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    result
}

/// Compile a `.rts` source string, write to a temp project with async runtime
/// support (tokio), build with cargo, run, and return stdout.
///
/// Use this for async e2e tests. The generated Cargo.toml includes
/// `tokio = { version = "1", features = ["full"] }`.
///
/// This is necessarily slow (invokes cargo) — tests using it should be `#[ignore]`.
///
/// Panics if compilation, building, or running fails.
pub fn compile_and_run_async(rts_source: &str) -> String {
    let rust_source = compile_to_rust(rts_source);
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");

    let src_dir = tmp_dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("failed to create src dir");

    // Write Cargo.toml with tokio dependency for async runtime.
    let cargo_toml = "\
[package]\n\
name = \"rsc-test\"\n\
version = \"0.1.0\"\n\
edition = \"2024\"\n\
\n\
[dependencies]\n\
tokio = { version = \"1\", features = [\"full\"] }\n\
\n\
[workspace]\n";
    fs::write(tmp_dir.path().join("Cargo.toml"), cargo_toml).expect("failed to write Cargo.toml");

    // Write the generated Rust source.
    fs::write(src_dir.join("main.rs"), &rust_source).expect("failed to write main.rs");

    // Build and run with cargo.
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .current_dir(tmp_dir.path())
        .output()
        .expect("failed to run cargo");

    assert!(
        output.status.success(),
        "cargo run failed.\nstdout: {}\nstderr: {}\ngenerated Rust:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        rust_source,
    );

    String::from_utf8(output.stdout).expect("stdout is not valid utf-8")
}

/// Compile a `.rts` source string and return diagnostic messages.
///
/// Useful for testing error cases. Returns the message string from each
/// diagnostic, preserving order.
pub fn compile_diagnostics(rts_source: &str) -> Vec<String> {
    let result = compile_source(rts_source, "test.rts");

    result.diagnostics.into_iter().map(|d| d.message).collect()
}

/// Compile a multi-file RustScript project, build with cargo, run, and return stdout.
///
/// Takes a list of `(filename, source)` pairs. The first file is treated as the
/// entry point (main.rts). All other files are modules.
///
/// This is necessarily slow (invokes cargo) — tests using it should be `#[ignore]`.
///
/// Panics if compilation, building, or running fails.
pub fn compile_multi_file_and_run(files: &[(&str, &str)]) -> String {
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let project_dir = tmp_dir.path().join("project");
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).expect("failed to create src dir");

    // Write rustscript.json for the RustScript project
    let manifest = r#"{"name": "rsc-multi-test"}"#;
    fs::write(project_dir.join("rustscript.json"), manifest)
        .expect("failed to write rustscript.json");

    // Write all .rts source files
    for (filename, source) in files {
        fs::write(src_dir.join(filename), source).expect("failed to write .rts file");
    }

    // Open and compile the project using the driver
    let project = rsc_driver::Project::open(&project_dir).expect("failed to open project");
    let (result, project_root, _, _) = project.compile().expect("failed to compile project");

    assert!(
        !result.has_errors,
        "compilation failed with errors: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Build and run with cargo in the project directory (in-place compilation)
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .current_dir(&project_root)
        .output()
        .expect("failed to run cargo");

    assert!(
        output.status.success(),
        "cargo run failed.\nstdout: {}\nstderr: {}\nproject dir: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        project_root.display(),
    );

    String::from_utf8(output.stdout).expect("stdout is not valid utf-8")
}
