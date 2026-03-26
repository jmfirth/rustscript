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
        Command::Init { name, template } => cmd_init(&name, template.as_deref()),
        Command::Build {
            release,
            target,
            no_borrow_inference,
        } => cmd_build(release, target.as_deref(), no_borrow_inference),
        Command::Run {
            args,
            target,
            no_borrow_inference,
        } => cmd_run(&args, target.as_deref(), no_borrow_inference),
        Command::Test { release, args } => cmd_test(release, &args),
        Command::Check => cmd_check(),
        Command::Fmt { check, files } => cmd_fmt(check, &files),
        Command::Add {
            crate_name,
            features,
            version,
            dev,
        } => cmd_add(&crate_name, version.as_deref(), &features, dev),
        Command::Remove { crate_name } => cmd_remove(&crate_name),
        Command::Lsp => cmd_lsp(),
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
fn cmd_build(release: bool, target: Option<&str>, no_borrow_inference: bool) -> Result<i32> {
    let mut project = open_project()?;
    project.compile_options.no_borrow_inference = no_borrow_inference;
    match project.build(release, target) {
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
fn cmd_run(args: &[String], target: Option<&str>, no_borrow_inference: bool) -> Result<i32> {
    let mut project = open_project()?;
    project.compile_options.no_borrow_inference = no_borrow_inference;
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

/// Add a crate dependency to the project's `rsc.toml`.
fn cmd_add(crate_name: &str, version: Option<&str>, features: &[String], dev: bool) -> Result<i32> {
    let project = open_project()?;
    match rsc_driver::deps::add_dependency(&project.root, crate_name, version, features, dev) {
        Ok(result) => {
            let section = if result.dev {
                "dev-dependencies"
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

/// Remove a crate dependency from the project's `rsc.toml`.
fn cmd_remove(crate_name: &str) -> Result<i32> {
    let project = open_project()?;
    match rsc_driver::deps::remove_dependency(&project.root, crate_name) {
        Ok(()) => {
            println!("Removed {crate_name} from rsc.toml");
            Ok(0)
        }
        Err(rsc_driver::error::DriverError::DependencyNotFound(_)) => {
            eprintln!("error: dependency '{crate_name}' not found in rsc.toml");
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
