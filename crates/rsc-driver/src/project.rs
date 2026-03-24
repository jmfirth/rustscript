//! `RustScript` project management — initialization, discovery, build, and run.
//!
//! Handles the on-disk project structure: locating source files, managing the
//! `.rsc-build/` build directory, and invoking Cargo for compilation and execution.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rsc_syntax::diagnostic::{Severity, render_diagnostics};

use crate::error::{DriverError, Result};
use crate::pipeline::{CompileResult, compile_source};

/// Name of the `RustScript` project manifest (lowercase — `RustScript` convention).
const PROJECT_MANIFEST: &str = "cargo.toml";

/// Name of the build output directory.
const BUILD_DIR: &str = ".rsc-build";

/// Default hello-world source for new projects.
const HELLO_WORLD_SOURCE: &str = r#"function main() {
  console.log("Hello, World!");
}
"#;

/// A `RustScript` project rooted at a directory.
#[derive(Debug)]
pub struct Project {
    /// Project root directory.
    pub root: PathBuf,
    /// Project name (from directory name or `cargo.toml`).
    pub name: String,
}

impl Project {
    /// Open an existing project from a directory.
    ///
    /// Walks up from `dir` looking for a directory containing `cargo.toml`
    /// (or a `src/` directory with `.rts` files).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::ProjectNotFound`] if no project root is found.
    pub fn open(dir: &Path) -> Result<Self> {
        let start_dir = dir.to_path_buf();
        let mut current = dir.to_path_buf();

        loop {
            // Check for cargo.toml (RustScript manifest)
            if current.join(PROJECT_MANIFEST).is_file() {
                let name = project_name_from_dir(&current);
                return Ok(Self {
                    root: current,
                    name,
                });
            }

            // Check for src/ directory with .rts files
            let src_dir = current.join("src");
            if src_dir.is_dir() && has_rts_files(&src_dir) {
                let name = project_name_from_dir(&current);
                return Ok(Self {
                    root: current,
                    name,
                });
            }

            // Walk up
            if !current.pop() {
                return Err(DriverError::ProjectNotFound(start_dir));
            }
        }
    }

    /// Find the main source file.
    ///
    /// Looks for `src/index.rts` first, then `src/main.rts`.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::MainSourceNotFound`] if neither file exists.
    pub fn main_source(&self) -> Result<PathBuf> {
        let index = self.root.join("src/index.rts");
        if index.is_file() {
            return Ok(index);
        }

        let main = self.root.join("src/main.rts");
        if main.is_file() {
            return Ok(main);
        }

        Err(DriverError::MainSourceNotFound)
    }

    /// Compile the project: read `.rts` source, run pipeline, write `.rs` output.
    ///
    /// Returns the [`CompileResult`] and the path to the build directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the main source file cannot be found or read, or if
    /// the build directory cannot be created.
    pub fn compile(&self) -> Result<(CompileResult, PathBuf)> {
        let source_path = self.main_source()?;
        let source = fs::read_to_string(&source_path)?;

        let file_name = source_path
            .file_name()
            .map_or("unknown.rts", |n| n.to_str().unwrap_or("unknown.rts"));

        let result = compile_source(&source, file_name);

        let build_dir = self.root.join(BUILD_DIR);
        let src_dir = build_dir.join("src");
        fs::create_dir_all(&src_dir)?;

        // Write Cargo.toml (always overwrite)
        let cargo_toml = format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            self.name
        );
        fs::write(build_dir.join("Cargo.toml"), cargo_toml)?;

        // Write src/main.rs (always overwrite)
        fs::write(src_dir.join("main.rs"), &result.rust_source)?;

        Ok((result, build_dir))
    }

    /// Build the project: compile, then invoke `cargo build` on the output.
    ///
    /// If `release` is true, passes `--release` to Cargo.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::CompilationFailed`] if the `RustScript` compilation
    /// produces errors, or [`DriverError::CargoBuildFailed`] if `cargo build` fails.
    pub fn build(&self, release: bool) -> Result<()> {
        let (result, build_dir) = self.compile()?;

        if result.has_errors {
            render_errors(&result);
            let error_count = result
                .diagnostics
                .iter()
                .filter(|d| matches!(d.severity, Severity::Error))
                .count();
            return Err(DriverError::CompilationFailed(error_count));
        }

        let mut cmd = Command::new("cargo");
        cmd.arg("build").current_dir(&build_dir);

        if release {
            cmd.arg("--release");
        }

        let status = cmd.status()?;

        if !status.success() {
            return Err(DriverError::CargoBuildFailed);
        }

        Ok(())
    }

    /// Run the project: compile, then invoke `cargo run` on the output.
    ///
    /// Forwards `args` to the compiled program.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::CompilationFailed`] if the `RustScript` compilation
    /// produces errors, or an I/O error if `cargo run` cannot be spawned.
    pub fn run(&self, args: &[String]) -> Result<std::process::ExitStatus> {
        let (result, build_dir) = self.compile()?;

        if result.has_errors {
            render_errors(&result);
            let error_count = result
                .diagnostics
                .iter()
                .filter(|d| matches!(d.severity, Severity::Error))
                .count();
            return Err(DriverError::CompilationFailed(error_count));
        }

        let mut cmd = Command::new("cargo");
        cmd.arg("run").current_dir(&build_dir);

        if !args.is_empty() {
            cmd.arg("--");
            cmd.args(args);
        }

        let status = cmd.status()?;
        Ok(status)
    }
}

/// Initialize a new `RustScript` project.
///
/// Creates the directory structure:
/// ```text
/// {name}/
///   src/
///     index.rts
///   cargo.toml
/// ```
///
/// # Errors
///
/// Returns [`DriverError::ProjectExists`] if `parent_dir/{name}` already exists,
/// or an I/O error if directory/file creation fails.
pub fn init_project(name: &str, parent_dir: &Path) -> Result<PathBuf> {
    let project_dir = parent_dir.join(name);

    if project_dir.exists() {
        return Err(DriverError::ProjectExists(project_dir));
    }

    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    // Write cargo.toml (lowercase — RustScript convention)
    let cargo_toml =
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    fs::write(project_dir.join(PROJECT_MANIFEST), cargo_toml)?;

    // Write src/index.rts
    fs::write(src_dir.join("index.rts"), HELLO_WORLD_SOURCE)?;

    Ok(project_dir)
}

/// Extract a project name from its directory name.
fn project_name_from_dir(dir: &Path) -> String {
    dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed")
        .to_owned()
}

/// Check whether a directory contains any `.rts` files (non-recursive).
fn has_rts_files(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries
        .filter_map(std::result::Result::ok)
        .any(|e| e.path().extension().is_some_and(|ext| ext == "rts"))
}

/// Render error diagnostics to stderr.
fn render_errors(result: &CompileResult) {
    let mut stderr = std::io::stderr();
    // Ignore rendering errors — we're already in an error path.
    let _ = render_diagnostics(&result.diagnostics, &result.source_map, &mut stderr);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Test 4: init_project creates correct directory structure
    #[test]
    fn test_init_project_creates_directory_structure() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path()).unwrap();

        assert!(project_dir.join("src").is_dir());
        assert!(project_dir.join("src/index.rts").is_file());
        assert!(project_dir.join("cargo.toml").is_file());
    }

    // Test 5: init_project creates cargo.toml with correct package name
    #[test]
    fn test_init_project_cargo_toml_has_correct_name() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path()).unwrap();

        let content = fs::read_to_string(project_dir.join("cargo.toml")).unwrap();
        assert!(
            content.contains("name = \"test-app\""),
            "cargo.toml should contain project name, got:\n{content}"
        );
        assert!(
            content.contains("edition = \"2024\""),
            "cargo.toml should specify edition 2024, got:\n{content}"
        );
    }

    // Test 6: init_project creates index.rts with hello-world content
    #[test]
    fn test_init_project_index_rts_has_hello_world() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path()).unwrap();

        let content = fs::read_to_string(project_dir.join("src/index.rts")).unwrap();
        assert!(
            content.contains("console.log"),
            "index.rts should contain console.log, got:\n{content}"
        );
        assert!(
            content.contains("Hello, World!"),
            "index.rts should contain Hello, World!, got:\n{content}"
        );
    }

    // Test 7: init_project on existing directory returns ProjectExists
    #[test]
    fn test_init_project_existing_dir_returns_error() {
        let tmp = TempDir::new().unwrap();
        init_project("test-app", tmp.path()).unwrap();

        let err = init_project("test-app", tmp.path()).unwrap_err();
        assert!(
            matches!(err, DriverError::ProjectExists(_)),
            "expected ProjectExists, got: {err:?}"
        );
    }

    // Test 8: Project::open finds project in current directory
    #[test]
    fn test_project_open_finds_project_in_directory() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        assert_eq!(project.name, "test-app");
        assert_eq!(project.root, project_dir);
    }

    // Test 9: Project::main_source returns src/index.rts when it exists
    #[test]
    fn test_project_main_source_returns_index_rts() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let main = project.main_source().unwrap();
        assert_eq!(main, project_dir.join("src/index.rts"));
    }

    // Test: Project::main_source falls back to src/main.rts
    #[test]
    fn test_project_main_source_falls_back_to_main_rts() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("fallback-app");
        fs::create_dir_all(project_dir.join("src")).unwrap();

        // Write cargo.toml but only main.rts (no index.rts)
        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"fallback-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            project_dir.join("src/main.rts"),
            "function main() { console.log(\"hi\"); }\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let main = project.main_source().unwrap();
        assert_eq!(main, project_dir.join("src/main.rts"));
    }

    // Test: Project::main_source returns error when neither exists
    #[test]
    fn test_project_main_source_returns_error_when_missing() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("no-source");
        fs::create_dir_all(project_dir.join("src")).unwrap();
        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"no-source\"\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let err = project.main_source().unwrap_err();
        assert!(
            matches!(err, DriverError::MainSourceNotFound),
            "expected MainSourceNotFound, got: {err:?}"
        );
    }

    // Test 10: Project::compile produces .rsc-build/src/main.rs with valid content
    #[test]
    fn test_project_compile_produces_build_directory() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, build_dir) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        let main_rs = build_dir.join("src/main.rs");
        assert!(main_rs.is_file(), "expected src/main.rs in build dir");

        let content = fs::read_to_string(main_rs).unwrap();
        assert!(
            content.contains("fn main()"),
            "expected fn main in generated Rust, got:\n{content}"
        );
        assert!(
            content.contains("println!"),
            "expected println! in generated Rust, got:\n{content}"
        );
    }

    // Test 11: Generated .rsc-build/Cargo.toml has correct name and edition
    #[test]
    fn test_project_compile_generates_correct_cargo_toml() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("name = \"test-app\""),
            "expected project name in Cargo.toml, got:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("edition = \"2024\""),
            "expected edition 2024 in Cargo.toml, got:\n{cargo_toml}"
        );
    }

    // Correctness scenario 2: Init + compile
    #[test]
    fn test_correctness_init_then_compile() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("hello-project", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, build_dir) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        let main_rs_path = build_dir.join("src/main.rs");
        assert!(main_rs_path.is_file(), "expected main.rs in build dir");

        let content = fs::read_to_string(main_rs_path).unwrap();
        assert!(
            content.contains("fn main"),
            "expected fn main in generated Rust, got:\n{content}"
        );
        assert!(
            content.contains("println!"),
            "expected println! in generated Rust, got:\n{content}"
        );
    }

    // Test: Project::open walks up to find project root
    #[test]
    fn test_project_open_walks_up_directories() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("walkup", tmp.path()).unwrap();

        // Open from a subdirectory
        let sub_dir = project_dir.join("src");
        let project = Project::open(&sub_dir).unwrap();
        assert_eq!(project.root, project_dir);
    }

    // Test: Project::open returns error when no project found
    #[test]
    fn test_project_open_returns_error_when_not_found() {
        let tmp = TempDir::new().unwrap();
        let err = Project::open(tmp.path()).unwrap_err();
        assert!(
            matches!(err, DriverError::ProjectNotFound(_)),
            "expected ProjectNotFound, got: {err:?}"
        );
    }

    // Test: Build directory preserves target/ across recompilations
    #[test]
    fn test_compile_preserves_build_dir_target() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("preserve-test", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();

        // First compile
        let (_, build_dir) = project.compile().unwrap();

        // Simulate a previous cargo build by creating a target/ directory
        let target_dir = build_dir.join("target");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("marker"), "should survive").unwrap();

        // Also create a Cargo.lock
        fs::write(build_dir.join("Cargo.lock"), "# lock file").unwrap();

        // Recompile
        let (_, build_dir2) = project.compile().unwrap();
        assert_eq!(build_dir, build_dir2);

        // target/ and Cargo.lock should still be there
        assert!(
            target_dir.join("marker").is_file(),
            "target/ should be preserved across recompilations"
        );
        assert!(
            build_dir.join("Cargo.lock").is_file(),
            "Cargo.lock should be preserved across recompilations"
        );
    }
}
