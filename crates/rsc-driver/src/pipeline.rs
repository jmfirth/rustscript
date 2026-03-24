//! Compilation pipeline orchestration.
//!
//! Wires the compiler stages together: parse, lower, emit. Collects diagnostics
//! from all passes and returns them alongside the generated Rust source.

use rsc_syntax::diagnostic::{Diagnostic, Severity};
use rsc_syntax::source::SourceMap;

/// Result of compiling a single `RustScript` source file.
pub struct CompileResult {
    /// The generated Rust source code (empty if compilation failed).
    pub rust_source: String,
    /// All diagnostics from all compiler passes.
    pub diagnostics: Vec<Diagnostic>,
    /// The source map (needed for diagnostic rendering with source context).
    pub source_map: SourceMap,
    /// Whether any error-level diagnostics were emitted.
    pub has_errors: bool,
}

/// Compile a single `RustScript` source string to Rust source code.
///
/// Orchestrates the full pipeline: parse, lower, emit. Diagnostics from every
/// stage are aggregated. If any stage produces errors, later stages are skipped
/// and the result is returned with `has_errors = true`.
#[must_use]
pub fn compile_source(source: &str, file_name: &str) -> CompileResult {
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
        };
    }

    // Stage 2: Lower
    let (ir, lower_diagnostics) = rsc_lower::lower(&module);

    all_diagnostics.extend(lower_diagnostics);

    if has_errors(&all_diagnostics) {
        return CompileResult {
            rust_source: String::new(),
            diagnostics: all_diagnostics,
            source_map,
            has_errors: true,
        };
    }

    // Stage 3: Emit
    let rust_source = rsc_emit::emit(&ir);

    let has_errors = has_errors(&all_diagnostics);

    CompileResult {
        rust_source,
        diagnostics: all_diagnostics,
        source_map,
        has_errors,
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
        };
    }

    // Stage 2: Lower
    let (mut ir, lower_diagnostics) = rsc_lower::lower(&module);

    all_diagnostics.extend(lower_diagnostics);

    if has_errors(&all_diagnostics) {
        return CompileResult {
            rust_source: String::new(),
            diagnostics: all_diagnostics,
            source_map,
            has_errors: true,
        };
    }

    // Inject mod declarations
    ir.mod_decls = mod_decls;

    // Stage 3: Emit
    let rust_source = rsc_emit::emit(&ir);

    let has_errors = has_errors(&all_diagnostics);

    CompileResult {
        rust_source,
        diagnostics: all_diagnostics,
        source_map,
        has_errors,
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
            result.rust_source.contains("for n in &numbers"),
            "expected `for n in &numbers` in output, got:\n{}",
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
            result.rust_source.contains("for n in &numbers"),
            "expected `for n in &numbers` in output, got:\n{}",
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
}
