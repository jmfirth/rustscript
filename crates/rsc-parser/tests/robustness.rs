//! Parser robustness tests — adversarial, malformed, and edge-case input.
//!
//! Every test in this module calls `rsc_parser::parse` with pathological input
//! and asserts that it returns without panicking or hanging. The content of the
//! returned AST and diagnostics is deliberately ignored; the sole contract
//! under test is "the parser never crashes."

use std::panic;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rsc_syntax::source::FileId;

/// Maximum time a single parse call is allowed to take before we consider it
/// a hang. Two seconds is generous — normal parses complete in microseconds.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Dummy file ID used for all robustness tests.
const FILE_ID: FileId = FileId(0);

/// Run `rsc_parser::parse` on `input` and assert it neither panics nor hangs.
///
/// Uses `catch_unwind` for panic detection and a channel + timeout for hang
/// detection. Returns `Ok(())` on success, panics with a descriptive message
/// on failure.
fn assert_parse_survives(input: &str) {
    let input = input.to_owned();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _ = rsc_parser::parse(&input, FILE_ID);
        }));
        // Ignore send errors — the receiver may have timed out and dropped.
        let _ = tx.send(result);
    });

    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(())) => {} // parse returned normally
        Ok(Err(panic_info)) => {
            let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                (*s).to_owned()
            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic payload".to_owned()
            };
            panic!("parser panicked on input: {msg}");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("parser hung (exceeded {TIMEOUT:?}) on input");
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("parser thread terminated unexpectedly");
        }
    }
}

// ===========================================================================
// 1. Empty and minimal inputs
// ===========================================================================

#[test]
fn empty_string() {
    assert_parse_survives("");
}

#[test]
fn single_char_open_brace() {
    assert_parse_survives("{");
}

#[test]
fn single_char_close_brace() {
    assert_parse_survives("}");
}

#[test]
fn single_char_open_paren() {
    assert_parse_survives("(");
}

#[test]
fn single_char_close_paren() {
    assert_parse_survives(")");
}

#[test]
fn single_char_semicolon() {
    assert_parse_survives(";");
}

#[test]
fn single_char_equals() {
    assert_parse_survives("=");
}

#[test]
fn just_whitespace() {
    assert_parse_survives("   \t\t\n\n\r\n   ");
}

#[test]
fn just_line_comments() {
    assert_parse_survives("// comment one\n// comment two\n// comment three");
}

#[test]
fn just_block_comments() {
    assert_parse_survives("/* block */ /* another block */");
}

// ===========================================================================
// 2. Unterminated constructs
// ===========================================================================

#[test]
fn unterminated_function_paren() {
    assert_parse_survives("function foo(");
}

#[test]
fn unterminated_function_brace() {
    assert_parse_survives("function foo() {");
}

#[test]
fn unterminated_if_paren() {
    assert_parse_survives("if (");
}

#[test]
fn unterminated_while_paren() {
    assert_parse_survives("while (true");
}

#[test]
fn unterminated_const_initializer() {
    assert_parse_survives("const x =");
}

#[test]
fn unterminated_type_brace() {
    assert_parse_survives("type Foo = {");
}

#[test]
fn unterminated_class_brace() {
    assert_parse_survives("class Bar {");
}

#[test]
fn unterminated_string_literal() {
    assert_parse_survives("\"unterminated string");
}

#[test]
fn unterminated_template_literal() {
    assert_parse_survives("`unterminated template");
}

#[test]
fn unterminated_import_brace() {
    assert_parse_survives("import {");
}

#[test]
fn unterminated_array_bracket() {
    assert_parse_survives("[");
}

#[test]
fn unterminated_nested_blocks() {
    assert_parse_survives("{ { { {");
}

// ===========================================================================
// 3. Deeply nested constructs
// ===========================================================================

#[test]
fn deeply_nested_parentheses() {
    let depth = 100;
    let input = "(".repeat(depth) + &")".repeat(depth);
    assert_parse_survives(&input);
}

#[test]
fn deeply_nested_parentheses_unmatched() {
    let input = "(".repeat(100);
    assert_parse_survives(&input);
}

#[test]
fn deeply_nested_if_statements() {
    let depth = 50;
    let mut input = String::new();
    for _ in 0..depth {
        input.push_str("if (true) { ");
    }
    for _ in 0..depth {
        input.push_str("} ");
    }
    assert_parse_survives(&input);
}

#[test]
fn deeply_nested_functions() {
    let depth = 50;
    let mut input = String::new();
    for i in 0..depth {
        input.push_str(&format!("function f{i}() {{ "));
    }
    for _ in 0..depth {
        input.push_str("} ");
    }
    assert_parse_survives(&input);
}

#[test]
fn very_long_single_line_addition() {
    // 10,000 characters of x + x + x + ...
    let count = 2500;
    let mut input = String::with_capacity(count * 4);
    input.push('x');
    for _ in 1..count {
        input.push_str(" + x");
    }
    assert_parse_survives(&input);
}

#[test]
fn very_many_statements() {
    let count = 1000;
    let input = "const x = 1;\n".repeat(count);
    assert_parse_survives(&input);
}

#[test]
fn deeply_nested_arrays() {
    let depth = 100;
    let input = "[".repeat(depth) + &"]".repeat(depth);
    assert_parse_survives(&input);
}

// ===========================================================================
// 4. Random / adversarial token sequences
// ===========================================================================

#[test]
fn unmatched_closing_braces() {
    assert_parse_survives("} } } } }");
}

#[test]
fn consecutive_operators() {
    assert_parse_survives("+ + + + +");
}

#[test]
fn repeated_function_keyword() {
    assert_parse_survives("function function function");
}

#[test]
fn repeated_const_keyword() {
    assert_parse_survives("const const const");
}

#[test]
fn mixed_keywords_random_order() {
    assert_parse_survives(
        "if else while for const let function return type class import export break continue",
    );
}

#[test]
fn all_operators() {
    assert_parse_survives("+ - * / % = == != < > <= >= && || ! & | ^ ~ << >> += -= *= /=");
}

#[test]
fn alternating_braces_and_parens() {
    assert_parse_survives("( { ) } ( { ) } ( { ) }");
}

#[test]
fn only_commas() {
    assert_parse_survives(",,,,,,,,,,,,,,,,,,,");
}

#[test]
fn only_dots() {
    assert_parse_survives(".....................");
}

#[test]
fn arrow_fragments() {
    assert_parse_survives("=> => => => =>");
}

// ===========================================================================
// 5. Valid-looking but structurally wrong
// ===========================================================================

#[test]
fn const_missing_name() {
    assert_parse_survives("const = 42;");
}

#[test]
fn function_missing_name() {
    assert_parse_survives("function (x) {}");
}

#[test]
fn type_missing_name() {
    assert_parse_survives("type = { x: i32 }");
}

#[test]
fn class_missing_name() {
    assert_parse_survives("class { }");
}

#[test]
fn for_missing_variable() {
    assert_parse_survives("for (const of items) { }");
}

#[test]
fn return_outside_function() {
    assert_parse_survives("return 42;");
}

#[test]
fn break_outside_loop() {
    assert_parse_survives("break;");
}

#[test]
fn continue_outside_loop() {
    assert_parse_survives("continue;");
}

#[test]
fn double_semicolons() {
    assert_parse_survives("const x = 1;; const y = 2;;");
}

#[test]
fn empty_function_body_with_return_type() {
    assert_parse_survives("function foo(): i32 {}");
}

// ===========================================================================
// 6. Pathological string / template content
// ===========================================================================

#[test]
fn string_with_many_escapes() {
    let input = format!("\"{}\"", "\\n\\t\\r\\\\\\\"".repeat(500));
    assert_parse_survives(&input);
}

#[test]
fn template_with_many_interpolations() {
    let mut input = String::from("`");
    for i in 0..200 {
        input.push_str(&format!("text${{x{i}}}"));
    }
    input.push('`');
    assert_parse_survives(&input);
}

#[test]
fn template_with_nested_template() {
    assert_parse_survives("`outer ${`inner ${x}`}`");
}

#[test]
fn empty_template() {
    assert_parse_survives("``");
}

// ===========================================================================
// 7. Type annotation edge cases
// ===========================================================================

#[test]
fn deeply_nested_generic_types() {
    // Array<Array<Array<Array<...>>>>
    let depth = 50;
    let mut input = String::from("const x: ");
    for _ in 0..depth {
        input.push_str("Array<");
    }
    input.push_str("i32");
    for _ in 0..depth {
        input.push('>');
    }
    input.push_str(" = x;");
    assert_parse_survives(&input);
}

#[test]
fn union_type_many_arms() {
    let mut input = String::from("const x: ");
    let types: Vec<&str> = vec!["i32", "string", "bool", "f64", "u8", "i64", "u32", "f32"];
    input.push_str(&types.join(" | "));
    input.push_str(" = x;");
    assert_parse_survives(&input);
}

#[test]
fn tuple_type_many_elements() {
    let mut input = String::from("const x: [");
    let elements: Vec<String> = (0..50).map(|_| "i32".to_owned()).collect();
    input.push_str(&elements.join(", "));
    input.push_str("] = x;");
    assert_parse_survives(&input);
}

#[test]
fn function_type_annotation() {
    assert_parse_survives("const x: (a: i32, b: string) => bool = f;");
}

// ===========================================================================
// 8. Boundary / NULL-byte / unicode edge cases
// ===========================================================================

#[test]
fn null_byte_in_source() {
    assert_parse_survives("const x = \0;");
}

#[test]
fn null_bytes_everywhere() {
    assert_parse_survives("\0\0\0\0\0");
}

#[test]
fn unicode_identifiers() {
    assert_parse_survives("const caf\u{00e9} = 42;");
}

#[test]
fn emoji_in_source() {
    assert_parse_survives("const x = \"\u{1F600}\";");
}

#[test]
fn bom_at_start() {
    assert_parse_survives("\u{FEFF}const x = 1;");
}

#[test]
fn mixed_line_endings() {
    assert_parse_survives("const x = 1;\r\nconst y = 2;\rconst z = 3;\n");
}

#[test]
fn very_long_identifier() {
    let ident = "a".repeat(10_000);
    let input = format!("const {ident} = 1;");
    assert_parse_survives(&input);
}

#[test]
fn very_long_number_literal() {
    let num = "9".repeat(10_000);
    let input = format!("const x = {num};");
    assert_parse_survives(&input);
}

// ===========================================================================
// 9. Combined stress — multiple malformed constructs
// ===========================================================================

#[test]
fn multiple_unterminated_constructs() {
    assert_parse_survives(
        r#"
        function foo(
        if (
        class Bar {
        const x =
        type Foo = {
        "#,
    );
}

#[test]
fn valid_then_garbage() {
    assert_parse_survives(
        r#"
        function valid() { return 1; }
        }{}{}{) ( ) = + - * / !@#$%
        "#,
    );
}

#[test]
fn garbage_then_valid() {
    assert_parse_survives(
        r#"
        }{}{}{) ( ) = + - * / !@#$%
        function valid() { return 1; }
        "#,
    );
}

#[test]
fn interleaved_valid_and_garbage() {
    assert_parse_survives(
        r#"
        const a = 1;
        }}}
        function b() { return 2; }
        ((((
        const c = 3;
        ====
        "#,
    );
}

#[test]
fn empty_blocks_and_parens() {
    assert_parse_survives("() {} [] () {} [] () {} []");
}

#[test]
fn switch_without_body() {
    assert_parse_survives("switch (x)");
}

#[test]
fn switch_with_empty_cases() {
    assert_parse_survives("switch (x) { case 1: case 2: case 3: }");
}

#[test]
fn try_without_catch() {
    assert_parse_survives("try { }");
}

#[test]
fn catch_without_try() {
    assert_parse_survives("catch (e) { }");
}

#[test]
fn export_without_declaration() {
    assert_parse_survives("export");
}

#[test]
fn import_malformed_path() {
    assert_parse_survives("import { a, b } from ;");
}

#[test]
fn class_with_malformed_members() {
    assert_parse_survives(
        r#"
        class Foo {
            + - *
            const = ;
            function {
        }
        "#,
    );
}
