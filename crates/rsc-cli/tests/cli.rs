//! Integration tests for the `rsc` CLI binary.
//!
//! These tests invoke the compiled binary via `std::process::Command` and verify
//! argument parsing, exit codes, and end-to-end behavior.

use std::process::Command;

fn rsc_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rsc"))
}

// Test 1: rsc --help produces help text mentioning init, build, run, test, check
#[test]
fn test_cli_help_mentions_all_commands() {
    let output = rsc_bin().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "rsc --help should succeed");
    assert!(stdout.contains("init"), "help should mention init");
    assert!(stdout.contains("build"), "help should mention build");
    assert!(stdout.contains("run"), "help should mention run");
    assert!(stdout.contains("test"), "help should mention test");
    assert!(stdout.contains("check"), "help should mention check");
}

// Test 2: rsc --version produces version output
#[test]
fn test_cli_version_output() {
    let output = rsc_bin().arg("--version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "rsc --version should succeed");
    assert!(
        stdout.contains("rsc"),
        "version output should contain 'rsc'"
    );
}

// Test 3: rsc init test-project creates directory with src/index.rts and cargo.toml
#[test]
#[ignore] // Slow: invokes binary, creates filesystem artifacts
fn test_cli_init_creates_project() {
    let tmp = tempfile::TempDir::new().unwrap();

    let output = rsc_bin()
        .args(["init", "test-project"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "rsc init should succeed");
    assert!(
        stdout.contains("Created project"),
        "should print creation message, got: {stdout}"
    );

    let project_dir = tmp.path().join("test-project");
    assert!(project_dir.join("src/index.rts").is_file());
    assert!(project_dir.join("cargo.toml").is_file());
}

// Test 4: rsc init without name shows error about missing argument
#[test]
fn test_cli_init_missing_name_shows_error() {
    let output = rsc_bin().arg("init").output().unwrap();

    assert!(
        !output.status.success(),
        "rsc init without name should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("name") || stderr.contains("required"),
        "error should mention missing argument, got: {stderr}"
    );
}

// Test 5: rsc check in a valid project exits with code 0
#[test]
#[ignore] // Slow: invokes binary, creates filesystem artifacts
fn test_cli_check_valid_project_exits_zero() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Init project first
    rsc_bin()
        .args(["init", "check-ok"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let output = rsc_bin()
        .arg("check")
        .current_dir(tmp.path().join("check-ok"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "rsc check in valid project should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Test 6: rsc check in a project with syntax errors exits with code 1
#[test]
#[ignore] // Slow: invokes binary, creates filesystem artifacts
fn test_cli_check_syntax_error_exits_one() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Init project first
    rsc_bin()
        .args(["init", "check-err"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // Overwrite source with invalid code
    std::fs::write(tmp.path().join("check-err/src/index.rts"), "function {").unwrap();

    let output = rsc_bin()
        .arg("check")
        .current_dir(tmp.path().join("check-err"))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "rsc check with syntax errors should exit 1, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Test 7: rsc test is a valid command (help text parses)
#[test]
fn test_cli_test_help_is_valid() {
    let output = rsc_bin().args(["test", "--help"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "rsc test --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("release"),
        "test help should mention --release flag, got: {stdout}"
    );
}

// Correctness scenario 1: rsc test on valid project compiles and runs cargo test
#[test]
#[ignore] // Slow: invokes binary, compiles with cargo
fn test_cli_test_valid_project_exits_zero() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Init project first
    rsc_bin()
        .args(["init", "test-ok"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let output = rsc_bin()
        .arg("test")
        .current_dir(tmp.path().join("test-ok"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "rsc test in valid project should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // cargo test should report test results (even if 0 tests)
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("running") || stdout.contains("test result"),
        "rsc test should show cargo test output, got stdout: {stdout}"
    );
}

// Correctness scenario 2: rsc test on project with compilation error exits 1
#[test]
#[ignore] // Slow: invokes binary, creates filesystem artifacts
fn test_cli_test_compilation_error_exits_one() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Init project first
    rsc_bin()
        .args(["init", "test-err"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // Overwrite source with invalid code
    std::fs::write(tmp.path().join("test-err/src/index.rts"), "function {").unwrap();

    let output = rsc_bin()
        .arg("test")
        .current_dir(tmp.path().join("test-err"))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "rsc test with compilation errors should exit 1, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Correctness scenario: full workflow — init, build, run
#[test]
#[ignore] // Slow: invokes binary, compiles with cargo
fn test_cli_correctness_full_workflow() {
    let tmp = tempfile::TempDir::new().unwrap();

    // rsc init hello
    let output = rsc_bin()
        .args(["init", "hello"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "init should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project_dir = tmp.path().join("hello");

    // rsc build
    let output = rsc_bin()
        .arg("build")
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "build should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify binary exists
    let binary_path = project_dir.join(".rsc-build/target/debug/hello");
    assert!(binary_path.is_file(), "compiled binary should exist");

    // rsc run
    let output = rsc_bin()
        .arg("run")
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "run should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Hello, World!"),
        "run should print Hello, World!, got: {stdout}"
    );
}
