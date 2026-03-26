//! REPL (Read-Eval-Print Loop) scratch pad for `RustScript`.
//!
//! Provides an interactive compile-and-run loop: each entry is appended to a
//! growing program buffer, compiled through the full pipeline, and executed.
//! State accumulates across entries within a session.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

/// Keywords that indicate the input is a statement (not an auto-printable expression).
const STATEMENT_KEYWORDS: &[&str] = &[
    "const", "let", "function", "class", "type", "import", "if", "while", "for", "return",
    "switch", "{",
];

/// The primary prompt shown when ready for input.
const PROMPT: &str = "rsc> ";

/// The continuation prompt shown during multi-line input.
const CONTINUATION_PROMPT: &str = "...> ";

/// History file name stored in the user's home directory.
const HISTORY_FILE: &str = ".rsc_history";

/// Result of compiling and running a REPL entry.
enum EvalResult {
    /// Compilation and execution succeeded with optional output.
    Success(Option<String>),
    /// Compilation or execution failed with an error message.
    Error(String),
}

/// Persistent state for the REPL session.
pub struct ReplState {
    /// All successfully compiled statements so far.
    history: Vec<String>,
    /// Temp directory for the scratch project (kept alive by the `TempDir` handle).
    _temp_dir: tempfile::TempDir,
    /// Path to the scratch project root.
    project_dir: PathBuf,
    /// The last successfully generated Rust source.
    last_rust_source: Option<String>,
}

impl ReplState {
    /// Create a new REPL state with a fresh temp project.
    ///
    /// # Errors
    ///
    /// Returns an error if the temp directory or Cargo project cannot be created.
    pub fn new() -> Result<Self> {
        let temp_dir = tempfile::Builder::new()
            .prefix("rsc-repl-")
            .tempdir()
            .context("failed to create temp directory for REPL")?;

        let project_dir = temp_dir.path().to_path_buf();
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).context("failed to create src/ in temp project")?;

        // Write initial Cargo.toml
        let cargo_toml = "\
[package]\n\
name = \"rsc-repl-scratch\"\n\
version = \"0.1.0\"\n\
edition = \"2024\"\n\
\n\
[workspace]\n";
        fs::write(project_dir.join("Cargo.toml"), cargo_toml)
            .context("failed to write Cargo.toml")?;

        // Write an initial empty main.rs so Cargo can initialize
        fs::write(src_dir.join("main.rs"), "fn main() {}\n")
            .context("failed to write initial main.rs")?;

        Ok(Self {
            history: Vec::new(),
            _temp_dir: temp_dir,
            project_dir,
            last_rust_source: None,
        })
    }

    /// Evaluate a line of `RustScript` input.
    ///
    /// Appends the input to the accumulated program, compiles, and runs it.
    /// If compilation fails, the input is discarded (not added to history).
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures during compilation or execution.
    pub fn eval(&mut self, input: &str) -> Result<Option<String>> {
        // Build the candidate program with the new input
        let mut candidate = self.history.clone();
        let is_expression = is_expression_input(input);

        if is_expression {
            // Wrap standalone expressions in console.log() for auto-print
            let mut wrapped = String::from("console.log(");
            wrapped.push_str(input);
            wrapped.push_str(");");
            candidate.push(wrapped);
        } else {
            candidate.push(input.to_string());
        }

        let rts_source = build_program(&candidate);

        // Compile through the pipeline
        let result = rsc_driver::compile_source(&rts_source, "repl.rts");

        if result.has_errors {
            // Render diagnostics to a string
            let mut buf = Vec::new();
            let _ = rsc_syntax::diagnostic::render_diagnostics(
                &result.diagnostics,
                &result.source_map,
                &mut buf,
            );
            let msg = String::from_utf8_lossy(&buf).to_string();
            return Err(anyhow!("{msg}"));
        }

        // Store the generated Rust source for :rust command
        self.last_rust_source = Some(result.rust_source.clone());

        // Write the generated Rust to the temp project
        let src_dir = self.project_dir.join("src");
        fs::write(src_dir.join("main.rs"), &result.rust_source)
            .context("failed to write main.rs")?;

        // Build and run
        match self.compile_and_run()? {
            EvalResult::Success(output) => {
                // Commit the input to history (store original, not wrapped)
                if is_expression {
                    let mut wrapped = String::from("console.log(");
                    wrapped.push_str(input);
                    wrapped.push_str(");");
                    self.history.push(wrapped);
                } else {
                    self.history.push(input.to_string());
                }
                Ok(output)
            }
            EvalResult::Error(err) => Err(anyhow!("{err}")),
        }
    }

    /// Clear all accumulated state, starting fresh.
    pub fn clear(&mut self) {
        self.history.clear();
        self.last_rust_source = None;
    }

    /// Return the accumulated statement history.
    #[must_use]
    pub fn history_entries(&self) -> &[String] {
        &self.history
    }

    /// Return the last generated Rust source, if any.
    #[must_use]
    pub fn last_rust(&self) -> Option<&str> {
        self.last_rust_source.as_deref()
    }

    /// Compile and run the scratch project, capturing stdout.
    fn compile_and_run(&self) -> Result<EvalResult> {
        // cargo build first
        let build_output = Command::new("cargo")
            .arg("build")
            .current_dir(&self.project_dir)
            .output()
            .context("failed to invoke cargo build")?;

        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            return Ok(EvalResult::Error(stderr.to_string()));
        }

        // cargo run
        let run_output = Command::new("cargo")
            .arg("run")
            .arg("--quiet")
            .current_dir(&self.project_dir)
            .output()
            .context("failed to invoke cargo run")?;

        let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();

        if !run_output.status.success() {
            return Ok(EvalResult::Error(format!("{stdout}{stderr}")));
        }

        // Trim trailing newline from output for cleaner display
        let output = stdout.trim_end().to_string();
        Ok(EvalResult::Success(if output.is_empty() {
            None
        } else {
            Some(output)
        }))
    }
}

/// Build a complete `RustScript` program from accumulated statements.
///
/// Wraps the statements in a `function main(): void { ... }` block.
#[must_use]
pub fn build_program(statements: &[String]) -> String {
    let mut source = String::new();
    source.push_str("function main(): void {\n");
    for stmt in statements {
        source.push_str("    ");
        source.push_str(stmt);
        source.push('\n');
    }
    source.push_str("}\n");
    source
}

/// Determine whether input looks like a standalone expression (not a statement).
///
/// Uses a simple heuristic: if the input starts with a known statement keyword,
/// it's a statement. Otherwise, it's an expression that should be auto-printed.
#[must_use]
pub fn is_expression_input(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    for keyword in STATEMENT_KEYWORDS {
        if let Some(rest) = trimmed.strip_prefix(keyword) {
            // Make sure it's a keyword boundary (followed by whitespace, paren, etc.)
            if rest.is_empty() || rest.starts_with(|c: char| !c.is_alphanumeric() && c != '_') {
                return false;
            }
        }
    }

    true
}

/// Count unmatched opening delimiters to detect incomplete multi-line input.
///
/// Returns the nesting depth (positive means unclosed delimiters remain).
#[must_use]
pub fn brace_depth(input: &str) -> i32 {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut string_char: char = '"';

    for ch in input.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }

        if in_string {
            if ch == '\\' {
                escape_next = true;
            } else if ch == string_char {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                string_char = ch;
            }
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            _ => {}
        }
    }

    depth
}

/// Handle a special command (lines starting with `:`)
///
/// Returns `Some(response)` if the command was handled, or `None` if it should
/// be treated as regular input (not a recognized command).
pub fn handle_command(input: &str, state: &mut ReplState) -> Option<String> {
    let trimmed = input.trim();

    match trimmed {
        ":help" => Some(help_text()),
        ":quit" | ":q" | ":exit" => None, // Handled by the caller
        ":clear" => {
            state.clear();
            Some("State cleared.".to_string())
        }
        ":history" => {
            let entries = state.history_entries();
            if entries.is_empty() {
                Some("(no history)".to_string())
            } else {
                let mut out = String::new();
                for (i, entry) in entries.iter().enumerate() {
                    let _ = writeln!(out, "  {}: {entry}", i + 1);
                }
                Some(out.trim_end().to_string())
            }
        }
        ":rust" => match state.last_rust() {
            Some(src) => Some(src.to_string()),
            None => Some("(no generated Rust yet)".to_string()),
        },
        _ if trimmed.starts_with(":type ") => {
            Some("type queries are not yet implemented".to_string())
        }
        _ if trimmed.starts_with(':') => Some(format!(
            "unknown command: {trimmed}. Type :help for available commands."
        )),
        _ => None,
    }
}

/// Return the help text for REPL commands.
fn help_text() -> String {
    "\
Commands:
  :help      Show this help message
  :clear     Clear history, start fresh
  :history   Show all accumulated statements
  :type <x>  Show the inferred type of x (not yet implemented)
  :rust      Show generated Rust for last input
  :quit      Exit the REPL (also Ctrl+D)"
        .to_string()
}

/// Run the REPL interactive loop.
///
/// # Errors
///
/// Returns an error if readline initialization fails or on I/O errors.
pub fn run_repl() -> Result<i32> {
    println!("RustScript REPL v0.1.0");
    println!("Type :help for available commands, :quit or Ctrl+D to exit.\n");

    let mut state = ReplState::new()?;

    let config = rustyline::Config::builder().auto_add_history(true).build();

    let mut rl =
        rustyline::DefaultEditor::with_config(config).context("failed to initialize readline")?;

    // Load history from file (ignore errors — file may not exist)
    let history_path = history_file_path();
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    loop {
        let input = read_input(&mut rl, &mut state);

        match input {
            ReadResult::Line(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Check for special commands
                if trimmed.starts_with(':') {
                    if trimmed == ":quit" || trimmed == ":q" || trimmed == ":exit" {
                        break;
                    }

                    if let Some(response) = handle_command(trimmed, &mut state) {
                        println!("{response}");
                    }
                    continue;
                }

                // Evaluate the input
                match state.eval(trimmed) {
                    Ok(Some(output)) => println!("{output}"),
                    Ok(None) => {}
                    Err(e) => eprintln!("{e}"),
                }
            }
            ReadResult::Eof => break,
            // Ctrl+C — just show a new prompt
            ReadResult::Interrupted => {}
        }
    }

    // Save history
    if let Some(ref path) = history_path {
        let _ = rl.save_history(path);
    }

    println!("Goodbye!");
    Ok(0)
}

/// Result of reading a line from the editor.
enum ReadResult {
    /// A complete line of input (possibly multi-line if braces were unbalanced).
    Line(String),
    /// End of input (Ctrl+D).
    Eof,
    /// User pressed Ctrl+C.
    Interrupted,
}

/// Read a complete input (handling multi-line for unclosed braces).
fn read_input(rl: &mut rustyline::DefaultEditor, _state: &mut ReplState) -> ReadResult {
    // Read the first line
    let first_line = match rl.readline(PROMPT) {
        Ok(line) => line,
        Err(rustyline::error::ReadlineError::Interrupted) => return ReadResult::Interrupted,
        Err(rustyline::error::ReadlineError::Eof | _) => return ReadResult::Eof,
    };

    let mut buffer = first_line;
    let mut depth = brace_depth(&buffer);

    // Continue reading if there are unclosed delimiters
    while depth > 0 {
        match rl.readline(CONTINUATION_PROMPT) {
            Ok(line) => {
                buffer.push('\n');
                buffer.push_str(&line);
                depth = brace_depth(&buffer);
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                // Close out on Ctrl+D even if unbalanced
                break;
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                return ReadResult::Interrupted;
            }
            Err(_) => return ReadResult::Eof,
        }
    }

    ReadResult::Line(buffer)
}

/// Get the path to the history file, or `None` if the home directory is unavailable.
fn history_file_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(HISTORY_FILE))
}

/// Get the user's home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- build_program tests ----

    #[test]
    fn test_build_program_empty_produces_empty_main() {
        let result = build_program(&[]);
        assert_eq!(result, "function main(): void {\n}\n");
    }

    #[test]
    fn test_build_program_single_statement_wraps_correctly() {
        let result = build_program(&["const x = 5;".to_string()]);
        assert_eq!(result, "function main(): void {\n    const x = 5;\n}\n");
    }

    #[test]
    fn test_build_program_multiple_statements_preserves_order() {
        let stmts = vec![
            "const x = 5;".to_string(),
            "const y = x * 2;".to_string(),
            "console.log(y);".to_string(),
        ];
        let result = build_program(&stmts);
        assert!(result.contains("const x = 5;"));
        assert!(result.contains("const y = x * 2;"));
        assert!(result.contains("console.log(y);"));
        // Verify order
        let x_pos = result.find("const x = 5;").expect("x");
        let y_pos = result.find("const y = x * 2;").expect("y");
        let log_pos = result.find("console.log(y);").expect("log");
        assert!(x_pos < y_pos);
        assert!(y_pos < log_pos);
    }

    // ---- is_expression_input tests ----

    #[test]
    fn test_is_expression_empty_input_returns_false() {
        assert!(!is_expression_input(""));
        assert!(!is_expression_input("   "));
    }

    #[test]
    fn test_is_expression_const_declaration_returns_false() {
        assert!(!is_expression_input("const x = 5;"));
    }

    #[test]
    fn test_is_expression_let_declaration_returns_false() {
        assert!(!is_expression_input("let x = 5;"));
    }

    #[test]
    fn test_is_expression_function_declaration_returns_false() {
        assert!(!is_expression_input("function foo() {}"));
    }

    #[test]
    fn test_is_expression_class_declaration_returns_false() {
        assert!(!is_expression_input("class Foo {}"));
    }

    #[test]
    fn test_is_expression_if_statement_returns_false() {
        assert!(!is_expression_input("if (true) {}"));
    }

    #[test]
    fn test_is_expression_while_statement_returns_false() {
        assert!(!is_expression_input("while (true) {}"));
    }

    #[test]
    fn test_is_expression_for_statement_returns_false() {
        assert!(!is_expression_input("for (const x of arr) {}"));
    }

    #[test]
    fn test_is_expression_return_statement_returns_false() {
        assert!(!is_expression_input("return 42;"));
    }

    #[test]
    fn test_is_expression_switch_statement_returns_false() {
        assert!(!is_expression_input("switch (x) {}"));
    }

    #[test]
    fn test_is_expression_brace_block_returns_false() {
        assert!(!is_expression_input("{ x = 1; }"));
    }

    #[test]
    fn test_is_expression_numeric_literal_returns_true() {
        assert!(is_expression_input("42"));
    }

    #[test]
    fn test_is_expression_arithmetic_returns_true() {
        assert!(is_expression_input("2 + 3"));
    }

    #[test]
    fn test_is_expression_string_literal_returns_true() {
        assert!(is_expression_input("\"hello\""));
    }

    #[test]
    fn test_is_expression_function_call_returns_true() {
        assert!(is_expression_input("foo(42)"));
    }

    #[test]
    fn test_is_expression_method_call_returns_true() {
        assert!(is_expression_input("x.toString()"));
    }

    #[test]
    fn test_is_expression_identifier_with_keyword_prefix_returns_true() {
        // "constant" starts with "const" but is not the keyword
        assert!(is_expression_input("constant_value"));
        // "letter" starts with "let" but is not the keyword
        assert!(is_expression_input("letter"));
        // "fortune" starts with "for" but is not the keyword
        assert!(is_expression_input("fortune"));
    }

    #[test]
    fn test_is_expression_type_annotation_returns_false() {
        assert!(!is_expression_input("type Foo = i32;"));
    }

    #[test]
    fn test_is_expression_import_returns_false() {
        assert!(!is_expression_input("import { foo } from \"bar\";"));
    }

    // ---- brace_depth tests ----

    #[test]
    fn test_brace_depth_balanced_returns_zero() {
        assert_eq!(brace_depth("{ }"), 0);
        assert_eq!(brace_depth("()"), 0);
        assert_eq!(brace_depth("[]"), 0);
        assert_eq!(brace_depth("function foo() { return 1; }"), 0);
    }

    #[test]
    fn test_brace_depth_open_brace_returns_positive() {
        assert_eq!(brace_depth("{"), 1);
        assert_eq!(brace_depth("function foo() {"), 1);
        assert_eq!(brace_depth("{ {"), 2);
    }

    #[test]
    fn test_brace_depth_mixed_delimiters() {
        assert_eq!(brace_depth("{ ("), 2);
        assert_eq!(brace_depth("[ { ("), 3);
        assert_eq!(brace_depth("{ ( ) }"), 0);
    }

    #[test]
    fn test_brace_depth_ignores_delimiters_in_strings() {
        assert_eq!(brace_depth("\"{ }\""), 0);
        assert_eq!(brace_depth("'{'"), 0);
        assert_eq!(brace_depth("`{`"), 0);
    }

    #[test]
    fn test_brace_depth_handles_escape_in_strings() {
        assert_eq!(brace_depth(r#""\""#), 0);
        assert_eq!(brace_depth(r#""\\""#), 0);
    }

    #[test]
    fn test_brace_depth_empty_input() {
        assert_eq!(brace_depth(""), 0);
    }

    #[test]
    fn test_brace_depth_close_without_open() {
        assert_eq!(brace_depth("}"), -1);
    }

    // ---- handle_command tests ----

    #[test]
    fn test_handle_command_help_returns_text() {
        let mut state = ReplState {
            history: Vec::new(),
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: None,
        };
        let result = handle_command(":help", &mut state);
        assert!(result.is_some());
        assert!(result.unwrap().contains(":help"));
    }

    #[test]
    fn test_handle_command_clear_resets_state() {
        let mut state = ReplState {
            history: vec!["const x = 5;".to_string()],
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: Some("fn main() {}".to_string()),
        };
        let result = handle_command(":clear", &mut state);
        assert!(result.is_some());
        assert!(state.history_entries().is_empty());
        assert!(state.last_rust().is_none());
    }

    #[test]
    fn test_handle_command_history_empty_shows_message() {
        let mut state = ReplState {
            history: Vec::new(),
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: None,
        };
        let result = handle_command(":history", &mut state);
        assert_eq!(result.unwrap(), "(no history)");
    }

    #[test]
    fn test_handle_command_history_with_entries_lists_them() {
        let mut state = ReplState {
            history: vec!["const x = 5;".to_string(), "const y = 10;".to_string()],
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: None,
        };
        let result = handle_command(":history", &mut state).unwrap();
        assert!(result.contains("1: const x = 5;"));
        assert!(result.contains("2: const y = 10;"));
    }

    #[test]
    fn test_handle_command_rust_no_source_shows_message() {
        let mut state = ReplState {
            history: Vec::new(),
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: None,
        };
        let result = handle_command(":rust", &mut state);
        assert_eq!(result.unwrap(), "(no generated Rust yet)");
    }

    #[test]
    fn test_handle_command_rust_with_source_shows_it() {
        let mut state = ReplState {
            history: Vec::new(),
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: Some("fn main() { println!(\"42\"); }".to_string()),
        };
        let result = handle_command(":rust", &mut state);
        assert_eq!(result.unwrap(), "fn main() { println!(\"42\"); }");
    }

    #[test]
    fn test_handle_command_type_stub_returns_not_implemented() {
        let mut state = ReplState {
            history: Vec::new(),
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: None,
        };
        let result = handle_command(":type x", &mut state);
        assert!(result.unwrap().contains("not yet implemented"));
    }

    #[test]
    fn test_handle_command_unknown_command_returns_error() {
        let mut state = ReplState {
            history: Vec::new(),
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: None,
        };
        let result = handle_command(":foobar", &mut state);
        assert!(result.unwrap().contains("unknown command"));
    }

    #[test]
    fn test_handle_command_quit_returns_none() {
        let mut state = ReplState {
            history: Vec::new(),
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: None,
        };
        // :quit is handled by the caller, not handle_command
        let result = handle_command(":quit", &mut state);
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_command_non_command_returns_none() {
        let mut state = ReplState {
            history: Vec::new(),
            _temp_dir: tempfile::TempDir::new().unwrap(),
            project_dir: PathBuf::from("/tmp/fake"),
            last_rust_source: None,
        };
        let result = handle_command("42", &mut state);
        assert!(result.is_none());
    }
}
