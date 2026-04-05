#![warn(clippy::pedantic)]
//! `RustScript` source code formatter.
//!
//! Parses `.rts` source into the `RustScript` AST, then pretty-prints it back
//! to source text with canonical formatting. Operates on the input language,
//! not generated Rust. Designed to be idempotent: formatting already-formatted
//! code produces identical output.

pub mod error;
mod printer;

use error::Result;
use printer::Printer;
use rustscript_syntax::source::FileId;

/// Format `RustScript` source code.
///
/// Parses the input, then pretty-prints the AST with canonical formatting.
/// Returns the formatted source code, or the original source unchanged
/// if the source contains comments (which the parser would discard).
///
/// # Errors
///
/// Returns an error if the parser produces no usable AST (catastrophic parse failure).
pub fn format_source(source: &str) -> Result<String> {
    // Detect comments — the parser discards them, so formatting would lose them.
    // Return the original source unchanged in this case.
    if source_contains_comments(source) {
        return Ok(source.to_owned());
    }

    let (module, diagnostics) = rustscript_parser::parse(source, FileId(0));

    // If there were parse errors, return the original source unchanged
    // to avoid destroying code.
    if !diagnostics.is_empty() {
        return Ok(source.to_owned());
    }

    let mut p = Printer::new();
    p.print_module(&module);
    let formatted = p.into_output();

    Ok(formatted)
}

/// Check if source is already formatted.
///
/// Returns `true` if formatting would produce identical output.
/// Returns `true` for sources with comments (they are returned unchanged).
#[must_use]
pub fn is_formatted(source: &str) -> bool {
    match format_source(source) {
        Ok(formatted) => formatted == source,
        Err(_) => false,
    }
}

/// Detect whether source text contains single-line (`//`) or multi-line (`/* */`) comments.
///
/// Scans the source without fully lexing it. Avoids false positives from
/// `//` or `/*` inside string literals by tracking quote state.
fn source_contains_comments(source: &str) -> bool {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Skip string literals (double-quoted)
        if b == b'"' {
            i += 1;
            while i < len && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1; // skip escaped character
                }
                i += 1;
            }
            i += 1; // skip closing quote
            continue;
        }

        // Skip template literals (backtick)
        if b == b'`' {
            i += 1;
            while i < len && bytes[i] != b'`' {
                if bytes[i] == b'\\' {
                    i += 1; // skip escaped character
                }
                i += 1;
            }
            i += 1; // skip closing backtick
            continue;
        }

        // Check for comments
        if b == b'/' && i + 1 < len {
            let next = bytes[i + 1];
            if next == b'/' || next == b'*' {
                return true;
            }
        }

        i += 1;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_source_empty_function_canonical_form() {
        let input = "function foo() {}";
        let result = format_source(input).expect("should format");
        assert_eq!(result, "function foo() {}\n");
    }

    #[test]
    fn test_format_source_indentation_two_spaces() {
        let input = "function foo() { const x = 1; }";
        let result = format_source(input).expect("should format");
        assert!(result.contains("  const x = 1;"), "got: {result}");
    }

    #[test]
    fn test_format_source_operator_spacing() {
        let input = "function foo() { const x = 1 + 2; }";
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("1 + 2"),
            "should have spaces around +: {result}"
        );
    }

    #[test]
    fn test_format_source_blank_lines_between_items() {
        let input = "function foo() {} function bar() {}";
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("}\n\nfunction bar"),
            "should have blank line between functions: {result}"
        );
    }

    #[test]
    fn test_format_source_trailing_newline() {
        let input = "function foo() {}";
        let result = format_source(input).expect("should format");
        assert!(result.ends_with('\n'), "should end with newline");
        assert!(
            !result.ends_with("\n\n"),
            "should not end with double newline"
        );
    }

    #[test]
    fn test_format_source_import_sorting() {
        let input = "import { X } from \"./mod\";\nimport { A } from \"./alpha\";\n";
        let result = format_source(input).expect("should format");
        let alpha_pos = result.find("./alpha").expect("alpha present");
        let mod_pos = result.find("./mod").expect("mod present");
        assert!(alpha_pos < mod_pos, "imports should be sorted: {result}");
    }

    #[test]
    fn test_format_source_idempotent() {
        let input = "function add(a: i32, b: i32): i32 {\n  return a + b;\n}\n";
        let first = format_source(input).expect("should format");
        let second = format_source(&first).expect("should format");
        assert_eq!(first, second, "formatting should be idempotent");
    }

    #[test]
    fn test_format_source_type_annotation_colon_spacing() {
        let input = "function foo(x: i32) {}";
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("x: i32"),
            "should have space after colon: {result}"
        );
    }

    #[test]
    fn test_format_source_comment_detection_single_line() {
        let input = "// this is a comment\nfunction foo() {}";
        let result = format_source(input).expect("should format");
        assert_eq!(
            result, input,
            "source with comments should be returned unchanged"
        );
    }

    #[test]
    fn test_format_source_comment_detection_multi_line() {
        let input = "/* comment */\nfunction foo() {}";
        let result = format_source(input).expect("should format");
        assert_eq!(
            result, input,
            "source with block comments should be returned unchanged"
        );
    }

    #[test]
    fn test_format_source_comment_in_string_not_detected() {
        // A string containing "//" should NOT trigger comment detection
        let input = "function foo() { const x = \"http://example.com\"; }";
        let result = format_source(input).expect("should format");
        // The result should be formatted (not returned unchanged)
        assert!(
            result.contains("  const x"),
            "should format the function body: {result}"
        );
    }

    #[test]
    fn test_is_formatted_returns_true_for_formatted() {
        let input = "function foo() {}\n";
        assert!(is_formatted(input));
    }

    #[test]
    fn test_is_formatted_returns_false_for_unformatted() {
        // Missing trailing newline
        let input = "function foo() {}";
        assert!(!is_formatted(input));
    }

    #[test]
    fn test_format_source_correctness_scenario_1_full_function() {
        let input = "function add(a: i32, b: i32): i32 { return a + b; }";
        let result = format_source(input).expect("should format");
        let expected = "function add(a: i32, b: i32): i32 {\n  return a + b;\n}\n";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_format_source_correctness_scenario_2_multi_item() {
        let input = concat!(
            "import { X } from \"./mod\";\n",
            "import { A } from \"./alpha\";\n",
            "function foo() { const x = 1; }\n",
            "function bar() { const y = 2; }\n",
        );
        let result = format_source(input).expect("should format");
        let expected = concat!(
            "import { A } from \"./alpha\";\n",
            "import { X } from \"./mod\";\n",
            "\n",
            "function foo() {\n",
            "  const x = 1;\n",
            "}\n",
            "\n",
            "function bar() {\n",
            "  const y = 2;\n",
            "}\n",
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn test_format_source_correctness_scenario_3_idempotent() {
        let already_formatted = concat!(
            "import { A } from \"./alpha\";\n",
            "import { X } from \"./mod\";\n",
            "\n",
            "function foo() {\n",
            "  const x = 1;\n",
            "}\n",
            "\n",
            "function bar() {\n",
            "  const y = 2;\n",
            "}\n",
        );
        let result = format_source(already_formatted).expect("should format");
        assert_eq!(
            result, already_formatted,
            "idempotent: second format should be identical"
        );
    }

    #[test]
    fn test_format_source_if_block() {
        let input = "function foo() { if (x) { return 1; } }";
        let result = format_source(input).expect("should format");
        let expected = "function foo() {\n  if (x) {\n    return 1;\n  }\n}\n";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_format_source_while_loop() {
        let input = "function foo() { while (true) { break; } }";
        let result = format_source(input).expect("should format");
        assert!(result.contains("while (true) {"), "got: {result}");
        assert!(result.contains("    break;"), "got: {result}");
    }

    #[test]
    fn test_format_source_for_loop() {
        let input = "function foo() { for (const x of items) { const y = x; } }";
        let result = format_source(input).expect("should format");
        assert!(result.contains("for (const x of items)"), "got: {result}");
    }

    #[test]
    fn test_source_contains_comments_no_comments() {
        assert!(!source_contains_comments("function foo() {}"));
    }

    #[test]
    fn test_source_contains_comments_line_comment() {
        assert!(source_contains_comments("// comment\nfunction foo() {}"));
    }

    #[test]
    fn test_source_contains_comments_block_comment() {
        assert!(source_contains_comments("/* block */ function foo() {}"));
    }

    #[test]
    fn test_source_contains_comments_url_in_string() {
        assert!(!source_contains_comments(
            "const x = \"http://example.com\";"
        ));
    }

    #[test]
    fn test_source_contains_comments_in_template() {
        assert!(!source_contains_comments(
            "const x = `url: http://example.com`;"
        ));
    }

    // ---------------------------------------------------------------
    // Task 062: Destructuring completeness formatting
    // ---------------------------------------------------------------

    #[test]
    fn test_format_destructure_rename() {
        let input = "function foo() { const { name: n, age: a } = user; }";
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("const { name: n, age: a } = user;"),
            "got: {result}"
        );
    }

    #[test]
    fn test_format_destructure_default() {
        let input = r#"function foo() { const { name = "x" } = config; }"#;
        let result = format_source(input).expect("should format");
        assert!(
            result.contains(r#"const { name = "x" } = config;"#),
            "got: {result}"
        );
    }

    #[test]
    fn test_format_destructure_rename_and_default() {
        let input = r#"function foo() { const { name: n = "x" } = config; }"#;
        let result = format_source(input).expect("should format");
        assert!(
            result.contains(r#"const { name: n = "x" } = config;"#),
            "got: {result}"
        );
    }

    #[test]
    fn test_format_array_destructure_rest() {
        let input = "function foo() { const [first, ...rest] = arr; }";
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("const [first, ...rest] = arr;"),
            "got: {result}"
        );
    }

    #[test]
    fn test_format_destructure_roundtrip() {
        let input = "function foo() {\n  const { name: n, age = 0 } = user;\n}\n";
        let result = format_source(input).expect("should format");
        let result2 = format_source(&result).expect("should format again");
        assert_eq!(result, result2, "round-trip should be stable");
    }

    #[test]
    fn test_format_type_def_with_derives() {
        let input = "type Foo = { x: i32 } derives Serialize, Deserialize";
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("} derives Serialize, Deserialize;"),
            "got: {result}"
        );
    }

    #[test]
    fn test_format_simple_enum_with_derives() {
        let input = r#"type Dir = "n" | "s" derives Clone, Serialize"#;
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("derives Clone, Serialize;"),
            "got: {result}"
        );
    }

    #[test]
    fn test_format_class_with_derives() {
        let input = "class Foo derives Debug { name: string; }";
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("class Foo derives Debug {"),
            "got: {result}"
        );
    }

    #[test]
    fn test_format_class_implements_and_derives() {
        let input = "class Foo implements Bar, derives Serialize { name: string; }";
        let result = format_source(input).expect("should format");
        assert!(
            result.contains("implements Bar, derives Serialize {"),
            "got: {result}"
        );
    }
}
