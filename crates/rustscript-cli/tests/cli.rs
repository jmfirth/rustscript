//! Integration tests for the `rustscript` CLI binary.
//!
//! These tests invoke the compiled binary via `std::process::Command` and verify
//! argument parsing, exit codes, and end-to-end behavior.

use std::process::Command;

fn rsc_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustscript"))
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

// ---------------------------------------------------------------------------
// Phase 3 integration: CLI surface completeness (all 7 commands)
// ---------------------------------------------------------------------------

// Test P3-1: rsc --help lists all 7 Phase 3 commands
#[test]
fn test_cli_help_lists_all_seven_commands() {
    let output = rsc_bin().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "rsc --help should succeed");

    let required_commands = ["init", "build", "run", "check", "test", "fmt", "lsp"];
    for cmd in &required_commands {
        assert!(
            stdout.contains(cmd),
            "help output should mention '{cmd}', got: {stdout}"
        );
    }
}

// Test P3-2: rsc fmt --help is a valid command
#[test]
fn test_cli_fmt_help_is_valid() {
    let output = rsc_bin().args(["fmt", "--help"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "rsc fmt --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("check"),
        "fmt help should mention --check flag, got: {stdout}"
    );
}

// Test P3-3: rsc lsp --help is a valid command
#[test]
fn test_cli_lsp_help_is_valid() {
    let output = rsc_bin().args(["lsp", "--help"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "rsc lsp --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("LSP") || stdout.contains("lsp") || stdout.contains("editor"),
        "lsp help should describe LSP functionality, got: {stdout}"
    );
}

// Test P3-4: rsc init with --template cli creates a CLI project
#[test]
#[ignore] // Slow: invokes binary, creates filesystem artifacts
fn test_cli_init_template_cli_creates_project() {
    let tmp = tempfile::TempDir::new().unwrap();

    let output = rsc_bin()
        .args(["init", "my-cli", "--template", "cli"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "rsc init --template cli should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("Created project"),
        "should print creation message, got: {stdout}"
    );

    let project_dir = tmp.path().join("my-cli");
    assert!(project_dir.join("src/main.rts").is_file());
    assert!(project_dir.join("Cargo.toml").is_file());
}

// Test P3-5: rsc init with invalid template shows error
#[test]
fn test_cli_init_invalid_template_shows_error() {
    let tmp = tempfile::TempDir::new().unwrap();

    let output = rsc_bin()
        .args(["init", "bad-proj", "--template", "invalid-template"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "rsc init with invalid template should exit 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown template"),
        "should mention unknown template, got: {stderr}"
    );
}

// Test P3-6: rsc fmt on an unformatted file exits 1 with --check
#[test]
#[ignore] // Slow: creates filesystem artifacts
fn test_cli_fmt_check_unformatted_exits_one() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Create a minimal project structure with an unformatted file
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("main.rts"), "function foo() { const x = 1; }").unwrap();

    let output = rsc_bin()
        .args(["fmt", "--check"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "rsc fmt --check on unformatted file should exit 1, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Test P3-7: rsc fmt formats a file in place
#[test]
#[ignore] // Slow: creates filesystem artifacts
fn test_cli_fmt_formats_file_in_place() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Create a minimal project structure with an unformatted file
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let source_path = src_dir.join("main.rts");
    std::fs::write(&source_path, "function foo() { const x = 1; }").unwrap();

    let output = rsc_bin()
        .arg("fmt")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "rsc fmt should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The file should now be formatted (multi-line)
    let formatted = std::fs::read_to_string(&source_path).unwrap();
    assert!(
        formatted.contains('\n'),
        "formatted output should be multi-line, got: {formatted}"
    );
    assert!(
        formatted.contains("  const x = 1;"),
        "should have indented body, got: {formatted}"
    );
}

// Test 2: rsc --version produces version output
#[test]
fn test_cli_version_output() {
    let output = rsc_bin().arg("--version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "rsc --version should succeed");
    assert!(
        stdout.contains("rustscript"),
        "version output should contain 'rustscript'"
    );
}

// Test 3: rsc init test-project creates directory with src/main.rts and Cargo.toml
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
    assert!(project_dir.join("src/main.rts").is_file());
    assert!(project_dir.join("rustscript.json").is_file());
    assert!(project_dir.join("Cargo.toml").is_file());
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
    std::fs::write(tmp.path().join("check-err/src/main.rts"), "function {").unwrap();

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
    std::fs::write(tmp.path().join("test-err/src/main.rts"), "function {").unwrap();

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

    // Verify binary exists (in-place compilation: binary is in target/debug/)
    let binary_path = project_dir.join("target/debug/hello");
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
