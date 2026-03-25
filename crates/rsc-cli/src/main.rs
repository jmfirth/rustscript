//! CLI entry point for the `RustScript` compiler.
//!
//! Parses command-line arguments and delegates to `rsc-driver` for all
//! compilation, build, and project management logic.
#![warn(clippy::pedantic)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use rsc_driver::error::DriverError;
use rsc_driver::{Project, compile_source, init_project};

/// Exit code for user-facing errors (compilation failures, build failures).
const EXIT_USER_ERROR: i32 = 1;
/// Exit code for internal errors (compiler bugs, I/O failures, missing project).
const EXIT_INTERNAL_ERROR: i32 = 2;

#[derive(Parser)]
#[command(name = "rsc", about = "The RustScript compiler", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Create a new `RustScript` project
    Init {
        /// Project name (creates a directory with this name)
        name: String,
    },
    /// Compile the project to a native binary
    Build {
        /// Build in release mode
        #[arg(long)]
        release: bool,
    },
    /// Compile and run the project
    Run {
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

/// Run the CLI command, returning the appropriate exit code.
///
/// User-facing errors (compilation/build failures) are handled inline and return
/// an exit code directly. Internal errors propagate as `anyhow::Error`.
fn run(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::Init { name } => cmd_init(&name),
        Command::Build { release } => cmd_build(release),
        Command::Run { args } => cmd_run(&args),
        Command::Test { release, args } => cmd_test(release, &args),
        Command::Check => cmd_check(),
        Command::Fmt { check, files } => cmd_fmt(check, &files),
    }
}

/// Create a new `RustScript` project.
fn cmd_init(name: &str) -> Result<i32> {
    let cwd = std::env::current_dir().context("failed to determine current directory")?;
    match init_project(name, &cwd) {
        Ok(_) => {
            println!("Created project '{name}'");
            Ok(0)
        }
        Err(DriverError::ProjectExists(path)) => {
            eprintln!(
                "error: project directory already exists: {}",
                path.display()
            );
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => Err(e).context("failed to create project"),
    }
}

/// Compile the project to a native binary.
fn cmd_build(release: bool) -> Result<i32> {
    let project = open_project()?;
    match project.build(release) {
        Ok(()) => {
            println!("Build complete");
            Ok(0)
        }
        Err(DriverError::CompilationFailed(_) | DriverError::CargoBuildFailed) => {
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => Err(e).context("build failed"),
    }
}

/// Compile and run the project.
fn cmd_run(args: &[String]) -> Result<i32> {
    let project = open_project()?;
    match project.run(args) {
        Ok(status) => Ok(status.code().unwrap_or(EXIT_INTERNAL_ERROR)),
        Err(DriverError::CompilationFailed(_)) => Ok(EXIT_USER_ERROR),
        Err(e) => Err(e).context("run failed"),
    }
}

/// Compile and run tests for the project.
fn cmd_test(release: bool, args: &[String]) -> Result<i32> {
    let project = open_project()?;
    match project.test(release, args) {
        Ok(status) => Ok(status.code().unwrap_or(EXIT_INTERNAL_ERROR)),
        Err(DriverError::CompilationFailed(_) | DriverError::CargoBuildFailed) => {
            Ok(EXIT_USER_ERROR)
        }
        Err(e) => Err(e).context("test failed"),
    }
}

/// Check the project for errors without building.
fn cmd_check() -> Result<i32> {
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
        let _ = rsc_syntax::diagnostic::render_diagnostics(
            &result.diagnostics,
            &result.source_map,
            &mut stderr,
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
                "no RustScript project found (looked for cargo.toml or src/ starting from {})",
                path.display()
            )
        }
        other => anyhow::anyhow!(other),
    })
}
