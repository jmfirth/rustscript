//! Decorator tests — `@decorator` syntax lowering to Rust attributes (`#[...]`).
//!
//! Snapshot tests verify the generated `.rs` output matches expectations.
//! Compilation tests (marked `#[ignore]`) verify the output compiles with `rustc`.

mod test_utils;

use test_utils::compile_to_rust;

/// Assert that `actual` matches `expected`, printing a diff on failure.
fn assert_snapshot(name: &str, actual: &str, expected: &str) {
    if actual != expected {
        panic!(
            "snapshot mismatch for `{name}`.\n\n\
             === expected ===\n{expected}\n\
             === actual ===\n{actual}\n\
             === end ===\n"
        );
    }
}

// ---------------------------------------------------------------------------
// 1. @test decorator on function → #[test]
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_decorator_test_function() {
    let source = "\
@test
function test_add() {
  const result: i32 = 1 + 1;
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("#[test]"),
        "expected #[test] attribute in output:\n{actual}"
    );
    assert!(
        actual.contains("fn test_add()"),
        "expected fn test_add() in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 2. @derive(Clone, Debug) on type → #[derive(Clone, Debug)]
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_decorator_derive_on_type() {
    let source = "\
@derive(Clone, Debug)
type Point = {
  x: f64,
  y: f64,
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("Clone"),
        "expected Clone derive in output:\n{actual}"
    );
    assert!(
        actual.contains("Debug"),
        "expected Debug derive in output:\n{actual}"
    );
    assert!(
        actual.contains("struct Point"),
        "expected struct Point in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 3. @cfg decorator with string args → #[cfg(...)]
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_decorator_cfg_on_function() {
    let source = r#"
@cfg(test)
function helper(): i32 {
  return 42;
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("#[cfg(test)]"),
        "expected #[cfg(test)] attribute in output:\n{actual}"
    );
    assert!(
        actual.contains("fn helper()"),
        "expected fn helper() in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 4. Multiple decorators stack: @a @b function → #[a] #[b] fn
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_multiple_decorators() {
    let source = "\
@inline
@must_use
function compute(): i32 {
  return 42;
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("#[inline]"),
        "expected #[inline] attribute in output:\n{actual}"
    );
    assert!(
        actual.contains("#[must_use]"),
        "expected #[must_use] attribute in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 5. @tokio_test → #[tokio::test] special mapping
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_decorator_tokio_test() {
    let source = "\
@tokio_test
async function test_fetch() {
  const x: i32 = 1;
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("#[tokio::test]"),
        "expected #[tokio::test] attribute in output:\n{actual}"
    );
    assert!(
        actual.contains("async fn test_fetch()"),
        "expected async fn test_fetch() in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 6. @derive on enum → merged into derive list
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_decorator_derive_on_enum() {
    let source = r#"
@derive(Serialize, Deserialize)
type Color = "red" | "green" | "blue"
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("Serialize"),
        "expected Serialize derive in output:\n{actual}"
    );
    assert!(
        actual.contains("Deserialize"),
        "expected Deserialize derive in output:\n{actual}"
    );
    assert!(
        actual.contains("enum Color"),
        "expected enum Color in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7. @serde attribute on struct → #[serde(...)] + #[derive(...)]
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_decorator_serde_attr_on_struct() {
    let source = r#"
@derive(Serialize)
@serde(rename_all = "camelCase")
type Config = {
  host_name: string,
  port_number: u32,
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("#[serde(rename_all = \"camelCase\")]"),
        "expected #[serde(rename_all = \"camelCase\")] attribute in output:\n{actual}"
    );
    assert!(
        actual.contains("Serialize"),
        "expected Serialize derive in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 8. Compilation test: @test function compiles as a Rust test
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_compile_decorator_test_function() {
    use std::fs;
    use std::process::Command;

    let source = "\
@test
function test_addition() {
  const result: i32 = 1 + 1;
}";

    let rust_source = compile_to_rust(source);

    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let src_dir = tmp_dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("failed to create src dir");

    let cargo_toml = "[package]\nname = \"rsc-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n[[bin]]\nname = \"rsc-test\"\npath = \"src/main.rs\"\n\n[lib]\nname = \"rsc_test_lib\"\npath = \"src/lib.rs\"\n";
    fs::write(tmp_dir.path().join("Cargo.toml"), cargo_toml).expect("failed to write Cargo.toml");
    fs::write(src_dir.join("lib.rs"), &rust_source).expect("failed to write lib.rs");
    fs::write(src_dir.join("main.rs"), "fn main() {}").expect("failed to write main.rs");

    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .current_dir(tmp_dir.path())
        .output()
        .expect("failed to run cargo test");

    assert!(
        output.status.success(),
        "cargo test failed.\nstdout: {}\nstderr: {}\ngenerated Rust:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        rust_source,
    );
}

// ---------------------------------------------------------------------------
// 9. Compilation test: @derive(Debug) struct compiles with Debug
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_compile_decorator_derive_debug() {
    use std::fs;
    use std::process::Command;

    let source = "\
@derive(Debug)
type Point = {
  x: f64,
  y: f64,
}

function main() {
  const p: Point = Point { x: 1.0, y: 2.0 };
  console.log(p);
}";

    let rust_source = compile_to_rust(source);

    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let src_dir = tmp_dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("failed to create src dir");

    let cargo_toml =
        "[package]\nname = \"rsc-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n";
    fs::write(tmp_dir.path().join("Cargo.toml"), cargo_toml).expect("failed to write Cargo.toml");
    fs::write(src_dir.join("main.rs"), &rust_source).expect("failed to write main.rs");

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
}
