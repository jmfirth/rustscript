//! CLI entry point for the `RustScript` compiler.
//!
//! Parses command-line arguments and delegates to `rsc-driver` for all
//! compilation, build, and project management logic.
#![warn(clippy::pedantic)]

mod repl;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use notify::{RecursiveMode, Watcher};

use rsc_driver::ColorMode;
use rsc_driver::error::DriverError;
use rsc_driver::{Project, compile_source, init_project};

/// Exit code for user-facing errors (compilation failures, build failures).
const EXIT_USER_ERROR: i32 = 1;
/// Exit code for internal errors (compiler bugs, I/O failures, missing project).
const EXIT_INTERNAL_ERROR: i32 = 2;

/// Color output mode for CLI diagnostics.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default)]
enum CliColorMode {
    /// Detect whether stderr is a terminal and enable colors if so.
    #[default]
    Auto,
    /// Always emit ANSI color codes.
    Always,
    /// Never emit ANSI color codes (plain text).
    Never,
}

#[derive(Parser)]
#[command(name = "rsc", about = "The RustScript compiler", version)]
struct Cli {
    /// Color output mode
    #[arg(long, global = true, default_value = "auto")]
    color: CliColorMode,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Create a new `RustScript` project
    Init {
        /// Project name (creates a directory with this name)
        name: String,
        /// Project template: cli, web-server, wasm (default: none — bare project)
        #[arg(long, short = 't')]
        template: Option<String>,
    },
    /// Compile the project to a native binary
    Build {
        /// Build in release mode
        #[arg(long)]
        release: bool,
        /// Compilation target (e.g., wasm32-unknown-unknown, wasm32-wasip1)
        #[arg(long)]
        target: Option<String>,
        /// Disable Tier 2 borrow inference (all params stay owned)
        #[arg(long)]
        no_borrow_inference: bool,
        /// Also generate TypeScript type definitions into the specified directory
        #[arg(long)]
        emit_types: Option<PathBuf>,
    },
    /// Compile and run the project
    Run {
        /// Compilation target (e.g., native targets only — WASM cannot be run directly)
        #[arg(long)]
        target: Option<String>,
        /// Disable Tier 2 borrow inference (all params stay owned)
        #[arg(long)]
        no_borrow_inference: bool,
        /// Arguments to pass to the compiled program
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run tests for the project
    Test {
        /// Build and run tests in release mode
        #[arg(long)]
        release: bool,
        /// Additional arguments passed to `cargo test` (e.g., test name filter)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Check the project for errors without building
    Check,
    /// Format `RustScript` source files
    Fmt {
        /// Check formatting without modifying files (exit 1 if unformatted)
        #[arg(long)]
        check: bool,
        /// Specific files to format (default: all .rts in src/)
        files: Vec<PathBuf>,
    },
    /// Add a crate dependency to the project
    Add {
        /// Crate name (e.g., "serde", "tokio")
        crate_name: String,

        /// Features to enable (e.g., --features derive,full)
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,

        /// Specific version (e.g., --version "1.0")
        #[arg(long)]
        version: Option<String>,

        /// Add as dev dependency
        #[arg(long)]
        dev: bool,
    },
    /// Remove a crate dependency from the project
    Remove {
        /// Crate name to remove (e.g., "serde")
        crate_name: String,
    },
    /// Start the LSP server (for editor integration)
    Lsp,
    /// Start interactive scratch pad
    Repl,
    /// Start watch mode: recompile on `.rts` file changes
    Dev {
        /// Build in release mode
        #[arg(long)]
        release: bool,
    },
    /// Generate TypeScript type definitions from `RustScript` source
    Types {
        /// Output directory for `.d.ts` files
        #[arg(long, short = 'o', default_value = "types")]
        out: PathBuf,
    },
    /// Eject: convert this `RustScript` project into a pure Rust project
    Eject {
        /// Skip the interactive confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("internal error: {e:#}");
            std::process::exit(EXIT_INTERNAL_ERROR);
        }
    }
}

/// Resolve `CliColorMode` into a concrete `ColorMode` by checking terminal state.
fn resolve_color_mode(cli_mode: CliColorMode) -> ColorMode {
    match cli_mode {
        CliColorMode::Always => ColorMode::Always,
        CliColorMode::Never => ColorMode::Never,
        CliColorMode::Auto => {
            if std::io::stderr().is_terminal() {
                ColorMode::Always
            } else {
                ColorMode::Never
            }
        }
    }
}

/// Run the CLI command, returning the appropriate exit code.
///
/// User-facing errors (compilation/build failures) are handled inline and return
/// an exit code directly. Internal errors propagate as `anyhow::Error`.
fn run(cli: Cli) -> Result<i32> {
    let color = resolve_color_mode(cli.color);

    match cli.command {
        Command::Init { name, template } => cmd_init(&name, template.as_deref()),
        Command::Build {
            release,
            target,
            no_borrow_inference,
            emit_types,
        } => cmd_build(
            release,
            target.as_deref(),
            no_borrow_inference,
            color,
            emit_types.as_deref(),
        ),
        Command::Run {
            args,
            target,
            no_borrow_inference,
        } => cmd_run(&args, target.as_deref(), no_borrow_inference, color),
        Command::Test { release, args } => cmd_test(release, &args, color),
        Command::Check => cmd_check(color),
        Command::Fmt { check, files } => cmd_fmt(check, &files),
        Command::Add {
            crate_name,
            features,
            version,
            dev,
        } => cmd_add(&crate_name, version.as_deref(), &features, dev),
        Command::Remove { crate_name } => cmd_remove(&crate_name),
        Command::Lsp => cmd_lsp(),
        Command::Repl => repl::run_repl(),
        Command::Dev { release } => cmd_dev(release),
        Command::Types { out } => cmd_types(&out),
        Command::Eject { force } => cmd_eject(force),
    }
}

/// Create a new `RustScript` project.
fn cmd_init(name: &str, template: Option<&str>) -> Result<i32> {
    let cwd = std::env::current_dir().context("failed to determine current directory")?;
    match init_project(name, &cwd, template) {
        Ok(_) => {
            if let Some(t) = template {
                println!("Created project '{name}' from template '{t}'");
            } else {
                println!("Created project '{name}'");
            }
            Ok(0)
        }
        Err(DriverError::ProjectExists(path)) => {
            eprintln!(
                "error: project directory already exists: {}",
                path.display()
            );
            Ok(EXIT_USER_ERROR)
        }
        Err(DriverError::InvalidTemplate(t)) => {
            eprintln!("error: unknown template '{t}'. Available: cli, web-server, wasm");
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => Err(e).context("failed to create project"),
    }
}

/// Compile the project to a native binary (or WASM module with `--target`).
fn cmd_build(
    release: bool,
    target: Option<&str>,
    no_borrow_inference: bool,
    color: ColorMode,
    emit_types: Option<&Path>,
) -> Result<i32> {
    let mut project = open_project()?;
    project.compile_options.no_borrow_inference = no_borrow_inference;
    project.color_mode = color;
    match project.build(release, target) {
        Ok(()) => {
            println!("Build complete");
            if let Some(types_dir) = emit_types {
                emit_types_to_dir(&project.root, types_dir)?;
            }
            Ok(0)
        }
        Err(DriverError::CompilationFailed(_) | DriverError::CargoBuildFailed) => {
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => Err(e).context("build failed"),
    }
}

/// Compile and run the project.
fn cmd_run(
    args: &[String],
    target: Option<&str>,
    no_borrow_inference: bool,
    color: ColorMode,
) -> Result<i32> {
    let mut project = open_project()?;
    project.compile_options.no_borrow_inference = no_borrow_inference;
    project.color_mode = color;
    match project.run(args, target) {
        Ok(status) => Ok(status.code().unwrap_or(EXIT_INTERNAL_ERROR)),
        Err(DriverError::CompilationFailed(_)) => Ok(EXIT_USER_ERROR),
        Err(DriverError::WasmRunUnsupported) => {
            eprintln!("error: {}", DriverError::WasmRunUnsupported);
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => Err(e).context("run failed"),
    }
}

/// Compile and run tests for the project.
fn cmd_test(release: bool, args: &[String], color: ColorMode) -> Result<i32> {
    let mut project = open_project()?;
    project.color_mode = color;
    match project.test(release, args) {
        Ok(status) => Ok(status.code().unwrap_or(EXIT_INTERNAL_ERROR)),
        Err(DriverError::CompilationFailed(_) | DriverError::CargoBuildFailed) => {
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => Err(e).context("test failed"),
    }
}

/// Check the project for errors without building.
fn cmd_check(color: ColorMode) -> Result<i32> {
    let project = open_project()?;
    let source_path = project
        .main_source()
        .context("failed to find main source file")?;
    let source = std::fs::read_to_string(&source_path)
        .with_context(|| format!("failed to read {}", source_path.display()))?;

    let file_name = source_path
        .file_name()
        .map_or("unknown.rts", |n| n.to_str().unwrap_or("unknown.rts"));

    let result = compile_source(&source, file_name);

    if result.has_errors {
        let mut stderr = std::io::stderr();
        let _ = rsc_syntax::diagnostic::render_diagnostics_colored(
            &result.diagnostics,
            &result.source_map,
            &mut stderr,
            color,
        );
        return Ok(EXIT_USER_ERROR);
    }

    Ok(0)
}

/// Format `RustScript` source files.
fn cmd_fmt(check: bool, files: &[PathBuf]) -> Result<i32> {
    let sources = if files.is_empty() {
        discover_rts_files()?
    } else {
        files.to_vec()
    };

    if sources.is_empty() {
        println!("No .rts files found");
        return Ok(0);
    }

    let mut unformatted_count = 0;
    for path in &sources {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let formatted = rsc_fmt::format_source(&source)
            .with_context(|| format!("failed to format {}", path.display()))?;

        if source != formatted {
            if check {
                eprintln!("not formatted: {}", path.display());
                unformatted_count += 1;
            } else {
                std::fs::write(path, &formatted)
                    .with_context(|| format!("failed to write {}", path.display()))?;
                println!("formatted: {}", path.display());
            }
        }
    }

    if check && unformatted_count > 0 {
        eprintln!("{unformatted_count} file(s) need formatting");
        Ok(EXIT_USER_ERROR)
    } else {
        Ok(0)
    }
}

/// Add a crate dependency to the project's `rustscript.json`.
fn cmd_add(crate_name: &str, version: Option<&str>, features: &[String], dev: bool) -> Result<i32> {
    let project = open_project()?;
    match rsc_driver::deps::add_dependency(&project.root, crate_name, version, features, dev) {
        Ok(result) => {
            let section = if result.dev {
                "devDependencies"
            } else {
                "dependencies"
            };
            if result.features.is_empty() {
                println!(
                    "Added {} v{} to {section}",
                    result.crate_name, result.version
                );
            } else {
                println!(
                    "Added {} v{} to {section}\n  Features: {}",
                    result.crate_name,
                    result.version,
                    result.features.join(", ")
                );
            }

            if let Some(suggestion) = &result.import_suggestion {
                println!("\n  Suggested import:\n    {suggestion}");
            } else {
                println!(
                    "\n  Suggested import:\n    import {{ ... }} from \"{}\";",
                    result.crate_name
                );
            }

            Ok(0)
        }
        Err(e) => {
            eprintln!("error: {e}");
            Ok(EXIT_USER_ERROR)
        }
    }
}

/// Remove a crate dependency from the project's `rustscript.json`.
fn cmd_remove(crate_name: &str) -> Result<i32> {
    let project = open_project()?;
    match rsc_driver::deps::remove_dependency(&project.root, crate_name) {
        Ok(()) => {
            println!("Removed {crate_name} from rustscript.json");
            Ok(0)
        }
        Err(rsc_driver::error::DriverError::DependencyNotFound(_)) => {
            eprintln!("error: dependency '{crate_name}' not found in rustscript.json");
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => {
            eprintln!("error: {e}");
            Ok(EXIT_USER_ERROR)
        }
    }
}

/// Start the LSP server for editor integration.
///
/// The server communicates over stdin/stdout using the Language Server Protocol.
fn cmd_lsp() -> Result<i32> {
    rsc_lsp::run_server().map_err(|e| anyhow::anyhow!("LSP server failed: {e}"))?;
    Ok(0)
}

/// Generate TypeScript type definitions for the project.
///
/// Walks `src/` for `.rts` files, generates `.d.ts` files for types with
/// `Serialize` or `Deserialize` derives, and writes them to the output directory.
fn cmd_types(out: &Path) -> Result<i32> {
    let project = open_project()?;
    emit_types_to_dir(&project.root, out)?;
    Ok(0)
}

/// Eject the project: convert from `RustScript` to pure Rust.
///
/// Prints a warning and prompts for confirmation unless `--force` is passed.
/// On confirmation, removes `rustscript.json` and un-ignores `.rs` files in `.gitignore`.
fn cmd_eject(force: bool) -> Result<i32> {
    let project = open_project()?;

    if !force {
        eprint!(
            "\
\u{26a0} Eject converts this RustScript project into a pure Rust project.

This will:
  - Remove rustscript.json (the RustScript manifest)
  - Un-ignore generated .rs files in .gitignore
  - Your .rts source files will be left in place for reference

This action cannot be undone. The rsc compiler will no longer recognize
this directory as a RustScript project.

To confirm, type \"eject\": "
        );

        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .context("failed to read confirmation input")?;

        if input.trim() != "eject" {
            eprintln!("Aborted.");
            return Ok(EXIT_USER_ERROR);
        }
    }

    match project.eject() {
        Ok(()) => {
            println!(
                "Ejected successfully. Your project is now a pure Rust project. \
                 You can delete the .rts source files when ready."
            );
            Ok(0)
        }
        Err(DriverError::EjectNotBuilt) => {
            eprintln!("error: {}", DriverError::EjectNotBuilt);
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => Err(e).context("eject failed"),
    }
}

/// Emit TypeScript type definitions from a project into the given output directory.
///
/// Shared between `rsc types` and `rsc build --emit-types`.
fn emit_types_to_dir(project_root: &Path, out: &Path) -> Result<()> {
    let results = rsc_driver::typegen::generate_types_for_project(project_root)
        .context("failed to generate type definitions")?;

    if results.is_empty() {
        println!("No serializable types found");
        return Ok(());
    }

    let mut count = 0;
    for (relative_path, content) in &results {
        let output_path = out.join(relative_path);
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        std::fs::write(&output_path, content)
            .with_context(|| format!("failed to write {}", output_path.display()))?;
        count += 1;
    }

    println!("Generated {count} type file(s) in {}/", out.display());
    Ok(())
}

/// Start watch mode: compile on file changes with debouncing.
///
/// Performs an initial build, then watches the project's source directory
/// for `.rts` file changes. On each change, clears the screen, recompiles,
/// and displays the result. Gracefully shuts down on `Ctrl+C`.
fn cmd_dev(release: bool) -> Result<i32> {
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    ctrlc::set_handler(move || {
        eprintln!("\nStopping watch mode...");
        r.store(false, Ordering::SeqCst);
    })
    .context("failed to set Ctrl+C handler")?;

    // Initial build
    clear_screen();
    print_timestamp("Compiling...");
    let project = open_project()?;
    let src_dir = project.source_dir();
    match project.build(release, None) {
        Ok(()) => print_build_success(),
        Err(DriverError::CompilationFailed(_) | DriverError::CargoBuildFailed) => {
            print_build_failure();
        }
        Err(e) => {
            print_build_failure();
            eprintln!("  {e}");
        }
    }

    // Set up file watcher — only watch src/ for .rts changes
    // (in-place compilation means passthrough files are already in place)
    let (tx, rx) = mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(tx).context("failed to create filesystem watcher")?;
    watcher
        .watch(&src_dir, RecursiveMode::Recursive)
        .with_context(|| format!("failed to watch {}", src_dir.display()))?;

    // Event loop with debounce
    while running.load(Ordering::SeqCst) {
        // Block until an event arrives (with a timeout so we can check the running flag)
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(event) => {
                if !is_relevant_change(&event) {
                    continue;
                }
                // Debounce: drain events for 200ms after the first relevant change
                debounce_events(&rx, Duration::from_millis(200));

                clear_screen();
                print_timestamp("Compiling...");
                match Project::open(&project.root) {
                    Ok(p) => match p.build(release, None) {
                        Ok(()) => print_build_success(),
                        Err(DriverError::CompilationFailed(_) | DriverError::CargoBuildFailed) => {
                            print_build_failure();
                        }
                        Err(e) => {
                            print_build_failure();
                            eprintln!("  {e}");
                        }
                    },
                    Err(e) => {
                        print_build_failure();
                        eprintln!("  {e}");
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(0)
}

/// Check whether a notify event concerns a `.rts` file change.
///
/// In-place compilation only needs to watch for `.rts` source file changes.
fn is_relevant_change(event: &std::result::Result<notify::Event, notify::Error>) -> bool {
    match event {
        Ok(ev) => {
            use notify::EventKind;
            match ev.kind {
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => ev
                    .paths
                    .iter()
                    .any(|p| p.extension().is_some_and(|ext| ext == "rts")),
                _ => false,
            }
        }
        Err(_) => false,
    }
}

/// Drain the event channel for `duration` to debounce rapid-fire events.
fn debounce_events(
    rx: &mpsc::Receiver<std::result::Result<notify::Event, notify::Error>>,
    duration: Duration,
) {
    let deadline = std::time::Instant::now() + duration;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Clear the terminal screen using ANSI escape codes.
fn clear_screen() {
    print!("\x1b[2J\x1b[H");
}

/// Print a timestamped message for build progress.
fn print_timestamp(msg: &str) {
    let now = chrono_free_timestamp();
    println!("\x1b[2m[{now}]\x1b[0m {msg}");
}

/// Print a build success message.
fn print_build_success() {
    let now = chrono_free_timestamp();
    println!("\x1b[2m[{now}]\x1b[0m \x1b[32m\u{2713} Build succeeded\x1b[0m");
}

/// Print a build failure message.
fn print_build_failure() {
    let now = chrono_free_timestamp();
    println!("\x1b[2m[{now}]\x1b[0m \x1b[31m\u{2717} Build failed\x1b[0m");
}

/// Return a `HH:MM:SS` timestamp without pulling in the `chrono` crate.
fn chrono_free_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Local-time approximation: just use UTC — good enough for a dev-mode timestamp.
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// Discover all `.rts` files in the project's `src/` directory.
fn discover_rts_files() -> Result<Vec<PathBuf>> {
    let cwd = std::env::current_dir().context("failed to determine current directory")?;
    let src_dir = cwd.join("src");

    if !src_dir.is_dir() {
        anyhow::bail!("no src/ directory found — are you in a RustScript project?");
    }

    let mut files = Vec::new();
    collect_rts_files(&src_dir, &mut files)?;
    files.sort();
    Ok(files)
}

/// Recursively collect all `.rts` files from a directory.
fn collect_rts_files(dir: &std::path::Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rts_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "rts") {
            files.push(path);
        }
    }

    Ok(())
}

/// Open a project from the current directory.
fn open_project() -> Result<Project> {
    let cwd = std::env::current_dir().context("failed to determine current directory")?;
    Project::open(&cwd).map_err(|e| match e {
        DriverError::ProjectNotFound(path) => {
            anyhow::anyhow!(
                "no RustScript project found (looked for rustscript.json or src/ starting from {})",
                path.display()
            )
        }
        other => anyhow::anyhow!(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(
        kind: notify::EventKind,
        paths: Vec<PathBuf>,
    ) -> std::result::Result<notify::Event, notify::Error> {
        Ok(notify::Event {
            kind,
            paths,
            attrs: notify::event::EventAttributes::default(),
        })
    }

    #[test]
    fn test_is_relevant_change_modify_rts_returns_true() {
        let event = make_event(
            notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            vec![PathBuf::from("src/main.rts")],
        );
        assert!(is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_create_rts_returns_true() {
        let event = make_event(
            notify::EventKind::Create(notify::event::CreateKind::File),
            vec![PathBuf::from("src/new_module.rts")],
        );
        assert!(is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_remove_rts_returns_true() {
        let event = make_event(
            notify::EventKind::Remove(notify::event::RemoveKind::File),
            vec![PathBuf::from("src/old.rts")],
        );
        assert!(is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_modify_non_rts_returns_false() {
        let event = make_event(
            notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            vec![PathBuf::from("src/readme.md")],
        );
        assert!(!is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_rs_file_returns_false() {
        // In-place compilation: .rs files are generated output, not watched
        let event = make_event(
            notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            vec![PathBuf::from("src/main.rs")],
        );
        assert!(!is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_json_file_returns_false() {
        let event = make_event(
            notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            vec![PathBuf::from("rustscript.json")],
        );
        assert!(!is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_toml_file_returns_false() {
        let event = make_event(
            notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            vec![PathBuf::from("Cargo.toml")],
        );
        assert!(!is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_access_event_returns_false() {
        let event = make_event(
            notify::EventKind::Access(notify::event::AccessKind::Read),
            vec![PathBuf::from("src/main.rts")],
        );
        assert!(!is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_error_returns_false() {
        let event: std::result::Result<notify::Event, notify::Error> =
            Err(notify::Error::generic("test error"));
        assert!(!is_relevant_change(&event));
    }

    #[test]
    fn test_is_relevant_change_empty_paths_returns_false() {
        let event = make_event(
            notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            vec![],
        );
        assert!(!is_relevant_change(&event));
    }

    #[test]
    fn test_debounce_events_drains_channel() {
        let (tx, rx) = mpsc::channel();

        // Send several events rapidly
        for _ in 0..5 {
            tx.send(make_event(
                notify::EventKind::Modify(notify::event::ModifyKind::Data(
                    notify::event::DataChange::Any,
                )),
                vec![PathBuf::from("src/main.rts")],
            ))
            .unwrap();
        }

        // Debounce should drain all events
        debounce_events(&rx, Duration::from_millis(50));

        // Channel should be empty (recv_timeout should time out)
        assert!(rx.recv_timeout(Duration::from_millis(10)).is_err());
    }

    #[test]
    fn test_chrono_free_timestamp_format() {
        let ts = chrono_free_timestamp();
        // Should be HH:MM:SS format
        assert_eq!(ts.len(), 8);
        assert_eq!(ts.as_bytes()[2], b':');
        assert_eq!(ts.as_bytes()[5], b':');
    }
}
