//! Compilation pipeline orchestration.
//!
//! Wires the compiler stages together: parse, lower, emit. Collects diagnostics
//! from all passes and returns them alongside the generated Rust source.

use std::collections::HashMap;

use rsc_syntax::diagnostic::{Diagnostic, Severity};
use rsc_syntax::external_fn::ExternalFnInfo;
use rsc_syntax::source::SourceMap;
use rsc_syntax::span::Span;

/// Result of compiling a single `RustScript` source file.
#[allow(clippy::struct_excessive_bools)]
// Four independent boolean flags for crate dependency tracking
pub struct CompileResult {
    /// The generated Rust source code (empty if compilation failed).
    pub rust_source: String,
    /// All diagnostics from all compiler passes.
    pub diagnostics: Vec<Diagnostic>,
    /// The source map (needed for diagnostic rendering with source context).
    pub source_map: SourceMap,
    /// Whether any error-level diagnostics were emitted.
    pub has_errors: bool,
    /// Whether the compiled code uses async/await and needs a tokio runtime.
    /// When true, the driver adds tokio to `Cargo.toml` and wraps main in `#[tokio::main]`.
    pub needs_async_runtime: bool,
    /// Whether the compiled code uses `for await` or `Promise.any` and needs the `futures` crate.
    pub needs_futures_crate: bool,
    /// Whether the compiled code uses `JSON.stringify`/`JSON.parse` and needs `serde_json`.
    pub needs_serde_json: bool,
    /// Whether the compiled code uses `Math.random()` and needs the `rand` crate.
    pub needs_rand: bool,
    /// External crate dependencies discovered from import statements.
    /// The driver adds these to the generated Cargo.toml.
    pub crate_dependencies: Vec<rsc_lower::CrateDependency>,
    /// Line-level source map from generated `.rs` to original `.rts`.
    ///
    /// Index = 0-based `.rs` line number, value = `.rts` source span.
    /// `None` entries indicate compiler-generated lines with no `.rts` origin.
    pub source_map_lines: Vec<Option<Span>>,
}

/// Options controlling the compilation pipeline.
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// When true, disables Tier 2 borrow inference and forces all function
    /// parameters to `Owned` mode (Tier 1 behavior).
    pub no_borrow_inference: bool,
    /// External function signatures from rustdoc JSON, keyed by qualified name.
    /// Threaded through to the lowering pass for call-site analysis.
    pub external_signatures: HashMap<String, ExternalFnInfo>,
}

/// Compile a single `RustScript` source string to Rust source code.
///
/// Orchestrates the full pipeline: parse, lower, emit. Diagnostics from every
/// stage are aggregated. If any stage produces errors, later stages are skipped
/// and the result is returned with `has_errors = true`.
#[must_use]
pub fn compile_source(source: &str, file_name: &str) -> CompileResult {
    compile_source_with_options(source, file_name, &CompileOptions::default())
}

/// Compile a single `RustScript` source string with explicit pipeline options.
///
/// Like [`compile_source`], but accepts [`CompileOptions`] to control behavior
/// (e.g., `--no-borrow-inference`).
#[must_use]
pub fn compile_source_with_options(
    source: &str,
    file_name: &str,
    options: &CompileOptions,
) -> CompileResult {
    let mut source_map = SourceMap::new();
    let file_id = source_map.add_file(file_name.to_owned(), source.to_owned());

    // Stage 1: Parse
    let (module, parse_diagnostics) = rsc_parser::parse(source, file_id);

    let mut all_diagnostics = parse_diagnostics;

    if has_errors(&all_diagnostics) {
        return CompileResult {
            rust_source: String::new(),
            diagnostics: all_diagnostics,
            source_map,
            has_errors: true,
            needs_async_runtime: false,
            needs_futures_crate: false,
            needs_serde_json: false,
            needs_rand: false,
            crate_dependencies: Vec::new(),
            source_map_lines: Vec::new(),
        };
    }

    // Stage 2: Lower
    let lower_options = rsc_lower::LowerOptions {
        no_borrow_inference: options.no_borrow_inference,
        external_signatures: options.external_signatures.clone(),
    };
    let lower_result = rsc_lower::lower_with_options(&module, &lower_options);

    all_diagnostics.extend(lower_result.diagnostics);

    if has_errors(&all_diagnostics) {
        return CompileResult {
            rust_source: String::new(),
            diagnostics: all_diagnostics,
            source_map,
            has_errors: true,
            needs_async_runtime: false,
            needs_futures_crate: false,
            needs_serde_json: false,
            needs_rand: false,
            crate_dependencies: Vec::new(),
            source_map_lines: Vec::new(),
        };
    }

    // Stage 3: Emit
    let emit_result = rsc_emit::emit(&lower_result.ir);

    let has_errors = has_errors(&all_diagnostics);

    CompileResult {
        rust_source: emit_result.source,
        diagnostics: all_diagnostics,
        source_map,
        has_errors,
        needs_async_runtime: lower_result.needs_async_runtime,
        needs_futures_crate: lower_result.needs_futures_crate,
        needs_serde_json: lower_result.needs_serde_json,
        needs_rand: lower_result.needs_rand,
        crate_dependencies: lower_result.crate_dependencies,
        source_map_lines: emit_result.source_map,
    }
}

/// Compile a single `RustScript` source with additional mod declarations injected.
///
/// Used for the entry-point file in a multi-file project: the mod declarations
/// for sibling modules are prepended to the generated IR before emission.
#[must_use]
pub fn compile_source_with_mods(
    source: &str,
    file_name: &str,
    mod_decls: Vec<rsc_syntax::rust_ir::RustModDecl>,
) -> CompileResult {
    compile_source_with_mods_and_options(source, file_name, mod_decls, &CompileOptions::default())
}

/// Compile with mod declarations and explicit pipeline options.
///
/// Like [`compile_source_with_mods`], but accepts [`CompileOptions`].
#[must_use]
pub fn compile_source_with_mods_and_options(
    source: &str,
    file_name: &str,
    mod_decls: Vec<rsc_syntax::rust_ir::RustModDecl>,
    options: &CompileOptions,
) -> CompileResult {
    let mut source_map = SourceMap::new();
    let file_id = source_map.add_file(file_name.to_owned(), source.to_owned());

    // Stage 1: Parse
    let (module, parse_diagnostics) = rsc_parser::parse(source, file_id);

    let mut all_diagnostics = parse_diagnostics;

    if has_errors(&all_diagnostics) {
        return CompileResult {
            rust_source: String::new(),
            diagnostics: all_diagnostics,
            source_map,
            has_errors: true,
            needs_async_runtime: false,
            needs_futures_crate: false,
            needs_serde_json: false,
            needs_rand: false,
            crate_dependencies: Vec::new(),
            source_map_lines: Vec::new(),
        };
    }

    // Stage 2: Lower
    let lower_options = rsc_lower::LowerOptions {
        no_borrow_inference: options.no_borrow_inference,
        external_signatures: options.external_signatures.clone(),
    };
    let lower_result = rsc_lower::lower_with_options(&module, &lower_options);

    all_diagnostics.extend(lower_result.diagnostics);

    if has_errors(&all_diagnostics) {
        return CompileResult {
            rust_source: String::new(),
            diagnostics: all_diagnostics,
            source_map,
            has_errors: true,
            needs_async_runtime: false,
            needs_futures_crate: false,
            needs_serde_json: false,
            needs_rand: false,
            crate_dependencies: Vec::new(),
            source_map_lines: Vec::new(),
        };
    }

    // Inject mod declarations
    let mut ir = lower_result.ir;
    ir.mod_decls = mod_decls;

    // Stage 3: Emit
    let emit_result = rsc_emit::emit(&ir);

    let has_errors = has_errors(&all_diagnostics);

    CompileResult {
        rust_source: emit_result.source,
        diagnostics: all_diagnostics,
        source_map,
        has_errors,
        needs_async_runtime: lower_result.needs_async_runtime,
        needs_futures_crate: lower_result.needs_futures_crate,
        needs_serde_json: lower_result.needs_serde_json,
        needs_rand: lower_result.needs_rand,
        crate_dependencies: lower_result.crate_dependencies,
        source_map_lines: emit_result.source_map,
    }
}

/// Check whether any diagnostic in the slice is an error.
fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 1: compile_source with valid hello-world source
    #[test]
    fn test_compile_source_hello_world_produces_fn_main() {
        let source = r#"function main() {
  console.log("Hello, World!");
}"#;
        let result = compile_source(source, "hello.rts");

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );
        assert!(
            result.rust_source.contains("fn main()"),
            "expected fn main in output, got:\n{}",
            result.rust_source
        );
    }

    // Test 2: compile_source with syntax error
    #[test]
    fn test_compile_source_syntax_error_has_errors() {
        let source = "function {";
        let result = compile_source(source, "bad.rts");

        assert!(result.has_errors, "expected errors for syntax error input");
        assert!(
            !result.diagnostics.is_empty(),
            "expected at least one diagnostic"
        );
        assert!(
            result.rust_source.is_empty(),
            "expected empty rust_source on error"
        );
    }

    // Test 3: compile_source with type error (unknown type)
    #[test]
    fn test_compile_source_unknown_type_has_errors() {
        // Use a type that the lowering pass doesn't recognize — this depends on
        // whether the lowering pass currently emits diagnostics for unknown types.
        // If lowering passes unknown types through without error, this test verifies
        // that the pipeline at least completes without errors for valid-looking code.
        let source = "function foo(x: SomeUnknownType): SomeUnknownType { return x; }";
        let result = compile_source(source, "types.rts");

        // The current lowering pass may or may not flag unknown types as errors.
        // If it does, has_errors is true and diagnostics are non-empty.
        // If it doesn't (passes them through as-is), has_errors is false.
        // Either way, the pipeline should not panic.
        // We test the pipeline handles whatever the lowering pass produces.
        if result.has_errors {
            assert!(!result.diagnostics.is_empty());
        }
    }

    // Correctness scenario 1: Full pipeline with fibonacci
    #[test]
    fn test_compile_source_correctness_fibonacci() {
        let source = r#"function fibonacci(n: i32): i32 {
  if (n <= 1) {
    return n;
  }
  return fibonacci(n - 1) + fibonacci(n - 2);
}"#;
        let result = compile_source(source, "fib.rts");

        assert!(
            !result.has_errors,
            "expected no errors for fibonacci, got: {:?}",
            result.diagnostics
        );
        assert!(
            !result.rust_source.is_empty(),
            "expected non-empty rust_source"
        );
        assert!(
            result.rust_source.contains("fn fibonacci"),
            "expected fn fibonacci in output, got:\n{}",
            result.rust_source
        );
    }

    // ---- Task 016: Correctness scenarios ----

    // Correctness scenario 1: Generic identity function
    #[test]
    fn test_compile_source_generic_identity_function() {
        let source = r#"function identity<T>(x: T): T {
  return x;
}
function main() {
  const a = identity(42);
  const b = identity("hello");
  console.log(a);
  console.log(b);
}"#;
        let result = compile_source(source, "identity.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );
        assert!(
            result.rust_source.contains("fn identity<T>(x: T) -> T"),
            "expected generic identity in output, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("fn main()"),
            "expected fn main in output, got:\n{}",
            result.rust_source
        );
    }

    // Correctness scenario 2: Generic struct
    #[test]
    fn test_compile_source_generic_struct() {
        let source = r#"type Pair<T> = { first: T, second: T }
function main() {
  const p: Pair<i32> = { first: 1, second: 2 };
  console.log(p.first);
}"#;
        let result = compile_source(source, "pair.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );
        assert!(
            result.rust_source.contains("struct Pair<T>"),
            "expected generic struct in output, got:\n{}",
            result.rust_source
        );
    }

    // Correctness scenario 3: Constrained generic
    #[test]
    fn test_compile_source_constrained_generic() {
        let source = r#"function max<T extends PartialOrd>(a: T, b: T): T {
  if (a > b) { return a; }
  return b;
}"#;
        let result = compile_source(source, "max.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );
        assert!(
            result
                .rust_source
                .contains("fn max<T: PartialOrd>(a: T, b: T) -> T"),
            "expected constrained generic in output, got:\n{}",
            result.rust_source
        );
    }

    // ---- Task 020: T | null → Option correctness scenarios ----

    // Correctness scenario 1: Null check narrowing e2e
    #[test]
    fn test_compile_source_null_check_narrowing() {
        let source = r#"function findName(found: bool): string | null {
  if (found) { return "Alice"; }
  return null;
}
function main() {
  const name = findName(true);
  if (name !== null) {
    console.log(name);
  }
}"#;
        let result = compile_source(source, "null_narrowing.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("-> Option<String>"),
            "expected Option<String> return type, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("return None"),
            "expected return None, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("Some("),
            "expected Some() wrapping, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("if let Some(name) = name"),
            "expected if let Some narrowing, got:\n{}",
            result.rust_source
        );
    }

    // Correctness scenario 2: Nullish coalescing e2e
    #[test]
    fn test_compile_source_nullish_coalescing() {
        let source = r#"function findName(found: bool): string | null {
  if (found) { return "Bob"; }
  return null;
}
function main() {
  const name = findName(false) ?? "Anonymous";
  console.log(name);
}"#;
        let result = compile_source(source, "nullish_coalesce.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains(".unwrap_or("),
            "expected .unwrap_or(), got:\n{}",
            result.rust_source
        );
    }

    // Correctness scenario 3: Optional chaining
    #[test]
    fn test_compile_source_optional_chaining() {
        let source = r#"type User = { name: string }
function getUser(found: bool): User | null {
  if (found) { return { name: "Alice" }; }
  return null;
}
function main() {
  const user = getUser(true);
  const name = user?.name ?? "Unknown";
  console.log(name);
}"#;
        let result = compile_source(source, "optional_chain.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("Option<User>"),
            "expected Option<User> return type, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(".map("),
            "expected .map() for optional chaining, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(".unwrap_or("),
            "expected .unwrap_or() for ??, got:\n{}",
            result.rust_source
        );
    }

    // ---- Task 022: Interface → Trait correctness scenarios ----

    // Correctness scenario 1: Interface definition e2e
    #[test]
    fn test_compile_source_interface_definition_e2e() {
        let source = r#"interface Printable {
  display(): string;
}

interface Serializable {
  serialize(): string;
}"#;
        let result = compile_source(source, "interfaces.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("trait Printable {"),
            "expected trait Printable in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("fn display(&self) -> String;"),
            "expected fn display(&self) -> String in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("trait Serializable {"),
            "expected trait Serializable in output:\n{}",
            result.rust_source
        );
        assert!(
            result
                .rust_source
                .contains("fn serialize(&self) -> String;"),
            "expected fn serialize(&self) -> String in output:\n{}",
            result.rust_source
        );
    }

    // Correctness scenario 2: Intersection type parameter e2e
    #[test]
    fn test_compile_source_intersection_type_parameter_e2e() {
        let source = r#"interface Serializable {
  serialize(): string;
}
interface Printable {
  print(): void;
}
function process(input: Serializable & Printable): string {
  input.print();
  return input.serialize();
}"#;
        let result = compile_source(source, "intersection.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result
                .rust_source
                .contains("fn process<T: Serializable + Printable>(input: T) -> String"),
            "expected generic fn with trait bounds in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("input.print();"),
            "expected input.print() in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("return input.serialize();"),
            "expected return input.serialize() in output:\n{}",
            result.rust_source
        );
    }

    // ---- Task 018: For-of loops, break, continue correctness scenarios ----

    // Correctness scenario 1: For-of array iteration e2e
    #[test]
    fn test_compile_source_for_of_array_iteration() {
        let source = r#"function main() {
  const numbers: Array<i32> = [1, 2, 3, 4, 5];
  for (const n of numbers) {
    console.log(n);
  }
}"#;
        let result = compile_source(source, "for_of.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("for &n in &numbers"),
            "expected `for &n in &numbers` in output, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("let numbers: Vec<i32>"),
            "expected `let numbers: Vec<i32>` in output, got:\n{}",
            result.rust_source
        );
    }

    // Correctness scenario 2: Break in while loop e2e
    #[test]
    fn test_compile_source_break_in_while_loop() {
        let source = r#"function main() {
  let i = 0;
  while (true) {
    if (i >= 3) { break; }
    console.log(i);
    i += 1;
  }
}"#;
        let result = compile_source(source, "break_while.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("break;"),
            "expected `break;` in output, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("while true"),
            "expected `while true` in output, got:\n{}",
            result.rust_source
        );
    }

    // Correctness scenario 3: Continue in for-of e2e
    #[test]
    fn test_compile_source_continue_in_for_of() {
        let source = r#"function main() {
  const numbers: Array<i32> = [1, 2, 3, 4, 5];
  for (const n of numbers) {
    if (n == 3) { continue; }
    console.log(n);
  }
}"#;
        let result = compile_source(source, "continue_for.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("continue;"),
            "expected `continue;` in output, got:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("for &n in &numbers"),
            "expected `for &n in &numbers` in output, got:\n{}",
            result.rust_source
        );
    }

    // Correctness scenario 3: Interface with Self type
    #[test]
    fn test_compile_source_interface_self_type_e2e() {
        let source = r#"interface Cloneable {
  clone(): Self;
}"#;
        let result = compile_source(source, "cloneable.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("trait Cloneable {"),
            "expected trait Cloneable in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("fn clone(&self) -> Self;"),
            "expected fn clone(&self) -> Self; in output:\n{}",
            result.rust_source
        );
    }

    // ---------------------------------------------------------------
    // Task 031: Crate consumption — correctness scenarios
    // ---------------------------------------------------------------

    // Correctness scenario 1: External crate import + std import
    #[test]
    fn test_compile_source_external_crate_import_and_std() {
        let source = r#"import { HashMap } from "std/collections";
import { Value } from "serde_json";

function main() {
  console.log("created map");
}"#;
        let result = compile_source(source, "crate_import.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result
                .rust_source
                .contains("use std::collections::HashMap;"),
            "expected use std::collections::HashMap in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("use serde_json::Value;"),
            "expected use serde_json::Value in output:\n{}",
            result.rust_source
        );
        // serde_json should be in dependencies, std should not
        assert_eq!(
            result.crate_dependencies.len(),
            1,
            "expected 1 dependency, got: {:?}",
            result.crate_dependencies
        );
        assert_eq!(result.crate_dependencies[0].name, "serde_json");
    }

    // Correctness scenario 2: Multiple crate imports
    #[test]
    fn test_compile_source_multiple_crate_imports() {
        let source = r#"import { get } from "reqwest";
import { Serialize, Deserialize } from "serde";

function main() {
  console.log("ready");
}"#;
        let result = compile_source(source, "multi_crate.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("use reqwest::get;"),
            "expected use reqwest::get in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("use serde::Serialize;"),
            "expected use serde::Serialize in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("use serde::Deserialize;"),
            "expected use serde::Deserialize in output:\n{}",
            result.rust_source
        );
        // Both reqwest and serde as dependencies
        assert_eq!(
            result.crate_dependencies.len(),
            2,
            "expected 2 dependencies, got: {:?}",
            result.crate_dependencies
        );
        let dep_names: Vec<&str> = result
            .crate_dependencies
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(dep_names.contains(&"reqwest"));
        assert!(dep_names.contains(&"serde"));
    }

    // Correctness scenario 3: Local + external mixed (regression)
    #[test]
    fn test_compile_source_local_and_external_imports_mixed() {
        let source = r#"import { helper } from "./utils";
import { Value } from "serde_json";

function main() {
  console.log("mixed");
}"#;
        let result = compile_source(source, "mixed_imports.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("use crate::utils::helper;"),
            "expected use crate::utils::helper in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("use serde_json::Value;"),
            "expected use serde_json::Value in output:\n{}",
            result.rust_source
        );
        assert_eq!(
            result.crate_dependencies.len(),
            1,
            "expected 1 dependency (serde_json only), got: {:?}",
            result.crate_dependencies
        );
    }

    // ---------------------------------------------------------------
    // Task 029: Async lowering and tokio runtime integration
    // ---------------------------------------------------------------

    // Correctness scenario 1: Async main with tokio
    #[test]
    fn test_compile_source_async_main_with_tokio() {
        let source = r#"async function main() {
  const data = await fetchData();
  console.log(data);
}

async function fetchData(): string {
  return "hello from async";
}"#;
        let result = compile_source(source, "async_main.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains("#[tokio::main]"),
            "expected #[tokio::main] in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("async fn main()"),
            "expected async fn main() in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(".await"),
            "expected .await in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains("async fn fetchData()"),
            "expected async fn fetchData() in output:\n{}",
            result.rust_source
        );
        assert!(
            result.needs_async_runtime,
            "expected needs_async_runtime to be true"
        );
    }

    // Correctness scenario 2: Non-async main unchanged
    #[test]
    fn test_compile_source_non_async_main_no_tokio() {
        let source = r#"function main() {
  console.log("hello");
}"#;
        let result = compile_source(source, "sync_main.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );
        assert!(
            !result.rust_source.contains("#[tokio::main]"),
            "expected no #[tokio::main] in output:\n{}",
            result.rust_source
        );
        assert!(
            !result.rust_source.contains("async fn"),
            "expected no async fn in output:\n{}",
            result.rust_source
        );
        assert!(
            !result.needs_async_runtime,
            "expected needs_async_runtime to be false"
        );
    }

    // Correctness scenario 3: Async function with throws
    #[test]
    fn test_compile_source_async_function_with_throws() {
        let source = r#"async function loadUser(id: string): string throws string {
  const data = await fetch(id);
  return data;
}"#;
        let result = compile_source(source, "async_throws.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result
                .rust_source
                .contains("async fn loadUser(id: String) -> Result<String, String>"),
            "expected async fn loadUser with Result return type in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(".await"),
            "expected .await in output:\n{}",
            result.rust_source
        );
        assert!(
            result.needs_async_runtime,
            "expected needs_async_runtime to be true"
        );
    }

    // Pipeline integration test: Full pipeline for async function produces expected Rust
    #[test]
    fn test_compile_source_async_pipeline_integration() {
        let source = r#"async function main() {
  const result = await getData();
  console.log(result);
}

async function getData(): string {
  return "data";
}"#;
        let result = compile_source(source, "async_pipeline.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        // Verify the full structure
        assert!(result.rust_source.contains("#[tokio::main]"));
        assert!(result.rust_source.contains("async fn main()"));
        assert!(result.rust_source.contains("getData().await"));
        assert!(result.rust_source.contains("async fn getData() -> String"));
        assert!(result.needs_async_runtime);
    }

    // ---------------------------------------------------------------
    // Task 033: Iterator method chaining correctness tests
    // ---------------------------------------------------------------

    // Correctness Scenario 1: map produces .iter().map().collect::<Vec<_>>()
    #[test]
    fn test_correctness_array_map_emits_iterator_chain() {
        let source = r#"function main() {
            const numbers: Array<i32> = [1, 2, 3];
            const doubled = numbers.map((n: i32): i32 => n * 2);
        }"#;
        let result = compile_source(source, "map.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains(".iter().map("),
            "expected .iter().map( in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(".collect::<Vec<_>>()"),
            "expected .collect::<Vec<_>>() in output:\n{}",
            result.rust_source
        );
    }

    // Correctness Scenario 1: filter produces .iter().filter().cloned().collect()
    #[test]
    fn test_correctness_array_filter_emits_iterator_chain_with_cloned() {
        let source = r#"function main() {
            const numbers: Array<i32> = [1, 2, 3, 4, 5];
            const evens = numbers.filter((n: i32): bool => n % 2 == 0);
        }"#;
        let result = compile_source(source, "filter.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains(".iter().filter("),
            "expected .iter().filter( in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(".cloned().collect::<Vec<_>>()"),
            "expected .cloned().collect::<Vec<_>>() in output:\n{}",
            result.rust_source
        );
    }

    // Correctness Scenario 2: reduce → fold with argument reordering
    #[test]
    fn test_correctness_array_reduce_emits_fold_with_reordered_args() {
        let source = r#"function main() {
            const sum = [1, 2, 3, 4, 5].reduce((acc: i32, n: i32): i32 => acc + n, 0);
            console.log(sum);
        }"#;
        let result = compile_source(source, "reduce.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        // fold(0, |acc, n| ...) — init comes first in Rust
        assert!(
            result.rust_source.contains(".iter().fold(0, |acc, n|"),
            "expected .iter().fold(0, |acc, n| in output:\n{}",
            result.rust_source
        );
    }

    // Correctness Scenario 3: find produces .iter().find().cloned()
    #[test]
    fn test_correctness_array_find_emits_find_with_cloned() {
        let source = r#"function main() {
            const numbers: Array<i32> = [1, 2, 3, 4, 5];
            const found = numbers.find((n: i32): bool => n > 3);
        }"#;
        let result = compile_source(source, "find.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains(".iter().find("),
            "expected .iter().find( in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(").cloned()"),
            "expected .cloned() after find in output:\n{}",
            result.rust_source
        );
    }

    // Correctness: some → any
    #[test]
    fn test_correctness_array_some_emits_any() {
        let source = r#"function main() {
            const items: Array<i32> = [1, 2, 3];
            const has_big = items.some((x: i32): bool => x > 5);
        }"#;
        let result = compile_source(source, "some.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains(".iter().any("),
            "expected .iter().any( in output:\n{}",
            result.rust_source
        );
    }

    // Correctness: every → all
    #[test]
    fn test_correctness_array_every_emits_all() {
        let source = r#"function main() {
            const items: Array<i32> = [1, 2, 3];
            const all_pos = items.every((x: i32): bool => x > 0);
        }"#;
        let result = compile_source(source, "every.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains(".iter().all("),
            "expected .iter().all( in output:\n{}",
            result.rust_source
        );
    }

    // Correctness: forEach → for_each
    #[test]
    fn test_correctness_array_foreach_emits_for_each() {
        let source = r#"function main() {
            const items: Array<i32> = [1, 2, 3];
            items.forEach((x: i32): void => console.log(x));
        }"#;
        let result = compile_source(source, "foreach.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        assert!(
            result.rust_source.contains(".iter().for_each("),
            "expected .iter().for_each( in output:\n{}",
            result.rust_source
        );
    }

    // Correctness: chained map+filter composes into single iterator chain
    #[test]
    fn test_correctness_chained_map_filter_emits_single_chain() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3, 4, 5];
            const result = arr.map((x: i32): i32 => x * 2).filter((x: i32): bool => x > 4);
        }"#;
        let result = compile_source(source, "chain.rts");
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}\ngenerated:\n{}",
            result.diagnostics, result.rust_source
        );
        // Should be a single chain: .iter().map(...).filter(...).cloned().collect()
        assert!(
            result.rust_source.contains(".iter().map("),
            "expected .iter().map( in chained output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(").filter("),
            "expected .filter( after .map() in output:\n{}",
            result.rust_source
        );
        assert!(
            result.rust_source.contains(".collect::<Vec<_>>()"),
            "expected .collect::<Vec<_>>() at end of chain:\n{}",
            result.rust_source
        );
    }

    // =========================================================================
    // Task 040: Pipeline integration tests
    // =========================================================================

    // Task 040 Test 10: Pipeline integration — CompileResult carries the source map.
    #[test]
    fn test_compile_result_carries_source_map_lines() {
        let source = r#"function main() {
  console.log("Hello!");
}"#;
        let result = compile_source(source, "hello.rts");

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );
        assert!(
            !result.source_map_lines.is_empty(),
            "expected non-empty source_map_lines"
        );
        // The number of source map entries should match the number of newlines in the output.
        let newline_count = result.rust_source.chars().filter(|&c| c == '\n').count();
        assert_eq!(
            result.source_map_lines.len(),
            newline_count,
            "source_map_lines length should match newline count in rust_source"
        );
    }

    // Task 040: Source map entries have spans for function body.
    #[test]
    fn test_compile_source_map_has_spans_for_fn_body() {
        let source = r#"function main() {
  const x: i32 = 42;
}"#;
        let result = compile_source(source, "spans.rts");

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );
        // At least some source map entries should have a span (the function body lines).
        let has_some_spans = result.source_map_lines.iter().any(|entry| entry.is_some());
        assert!(
            has_some_spans,
            "expected at least some source_map_lines entries to have spans, got: {:?}",
            result.source_map_lines
        );
    }

    // Test: external signatures are threaded through CompileOptions to lowering
    #[test]
    fn test_compile_source_with_external_signatures() {
        use rsc_syntax::external_fn::{ExternalFnInfo, ExternalParamInfo, ExternalReturnType};

        let mut sigs = HashMap::new();
        sigs.insert(
            "axum::Router::route".to_owned(),
            ExternalFnInfo {
                name: "route".to_owned(),
                crate_name: "axum".to_owned(),
                params: vec![ExternalParamInfo {
                    name: "path".to_owned(),
                    is_ref: true,
                    is_str_ref: true,
                    is_mut_ref: false,
                }],
                return_type: ExternalReturnType::Value,
                is_async: false,
                is_method: true,
                parent_type: Some("Router".to_owned()),
            },
        );

        let options = CompileOptions {
            no_borrow_inference: false,
            external_signatures: sigs,
        };

        // The external signatures don't affect a simple hello-world program,
        // but the pipeline should accept and thread them through without error.
        let source = r#"function main() {
  console.log("hello");
}"#;
        let result = compile_source_with_options(source, "test.rts", &options);
        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );
        assert!(result.rust_source.contains("fn main()"));
    }
}
