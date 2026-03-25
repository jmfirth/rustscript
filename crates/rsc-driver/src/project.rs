//! `RustScript` project management — initialization, discovery, build, and run.
//!
//! Handles the on-disk project structure: locating source files, managing the
//! `.rsc-build/` build directory, and invoking Cargo for compilation and execution.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rsc_lower::CrateDependency;
use rsc_syntax::diagnostic::{Severity, render_diagnostics};
use rsc_syntax::rust_ir::RustModDecl;

use crate::error::{DriverError, Result};
use crate::error_translation::translate_rustc_errors;
use crate::pipeline::{CompileResult, compile_source, compile_source_with_mods};

/// Name of the `RustScript` project manifest (lowercase — `RustScript` convention).
const PROJECT_MANIFEST: &str = "cargo.toml";

/// Name of the build output directory.
const BUILD_DIR: &str = ".rsc-build";

/// Default hello-world source for new projects.
const HELLO_WORLD_SOURCE: &str = r#"function main() {
  console.log("Hello, World!");
}
"#;

/// A dependency version specification for `Cargo.toml`.
#[derive(Debug, Clone)]
enum DependencySpec {
    /// A simple version string, e.g. `"*"` or `"1"`.
    Simple(String),
    /// A version with additional features, e.g. `{ version = "1", features = ["full"] }`.
    Detailed {
        version: String,
        features: Vec<String>,
    },
}

/// Structured builder for generating `Cargo.toml` content.
///
/// Uses `BTreeMap` for deterministic alphabetical ordering of dependencies.
struct CargoTomlBuilder {
    name: String,
    edition: String,
    dependencies: BTreeMap<String, DependencySpec>,
}

impl CargoTomlBuilder {
    /// Create a new builder with the given project name and edition.
    fn new(name: &str, edition: &str) -> Self {
        Self {
            name: name.to_owned(),
            edition: edition.to_owned(),
            dependencies: BTreeMap::new(),
        }
    }

    /// Add a dependency. If the dependency already exists, the existing entry
    /// is kept (first-wins deduplication).
    fn add_dependency(&mut self, name: &str, spec: DependencySpec) {
        self.dependencies.entry(name.to_owned()).or_insert(spec);
    }

    /// Add tokio with the async runtime configuration.
    fn add_tokio_runtime(&mut self) {
        // Tokio always gets the detailed spec with version + features.
        // Override any existing `"*"` version if tokio was also imported explicitly.
        self.dependencies.insert(
            "tokio".to_owned(),
            DependencySpec::Detailed {
                version: "1".to_owned(),
                features: vec!["full".to_owned()],
            },
        );
    }

    /// Build the `Cargo.toml` content string.
    fn build(&self) -> String {
        let mut out = String::new();

        // [package] section
        let _ = writeln!(out, "[package]");
        let _ = writeln!(out, "name = \"{}\"", self.name);
        let _ = writeln!(out, "version = \"0.1.0\"");
        let _ = writeln!(out, "edition = \"{}\"", self.edition);

        // [dependencies] section (only if there are dependencies)
        if !self.dependencies.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "[dependencies]");
            for (name, spec) in &self.dependencies {
                match spec {
                    DependencySpec::Simple(version) => {
                        let _ = writeln!(out, "{name} = \"{version}\"");
                    }
                    DependencySpec::Detailed { version, features } => {
                        let features_str = features
                            .iter()
                            .map(|f| format!("\"{f}\""))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let _ = writeln!(
                            out,
                            "{name} = {{ version = \"{version}\", features = [{features_str}] }}"
                        );
                    }
                }
            }
        }

        // [workspace] section to prevent Cargo from walking up
        let _ = writeln!(out);
        let _ = writeln!(out, "[workspace]");

        out
    }
}

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

    /// Discover all `.rts` source files in the `src/` directory (non-recursive for Phase 1).
    ///
    /// Returns a list of paths to `.rts` files, excluding the main entry point.
    fn discover_modules(&self) -> Result<Vec<PathBuf>> {
        let src_dir = self.root.join("src");
        let main_source = self.main_source()?;
        let mut modules = Vec::new();

        if let Ok(entries) = fs::read_dir(&src_dir) {
            for entry in entries.filter_map(std::result::Result::ok) {
                let path = entry.path();
                if path.is_file()
                    && path.extension().is_some_and(|ext| ext == "rts")
                    && path != main_source
                {
                    modules.push(path);
                }
            }
        }

        // Sort for deterministic output
        modules.sort();
        Ok(modules)
    }

    /// Compile the project: read `.rts` source, run pipeline, write `.rs` output.
    ///
    /// Discovers all `.rts` files in `src/`, compiles each independently, generates
    /// `mod` declarations for the main file, and writes all output to `.rsc-build/src/`.
    ///
    /// Returns the [`CompileResult`] for the main file and the path to the build directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the main source file cannot be found or read, or if
    /// the build directory cannot be created.
    pub fn compile(&self) -> Result<(CompileResult, PathBuf)> {
        let source_path = self.main_source()?;
        let source = fs::read_to_string(&source_path)?;
        let module_files = self.discover_modules()?;

        let build_dir = self.root.join(BUILD_DIR);
        let src_dir = build_dir.join("src");
        fs::create_dir_all(&src_dir)?;

        // Collect all crate dependencies from all compiled modules
        let mut all_crate_deps: Vec<CrateDependency> = Vec::new();
        // Track whether any module uses async (OR across all modules)
        let mut any_needs_async_runtime = false;

        // Compile each module file and collect mod declarations
        let mut mod_decls = Vec::new();
        let mut has_module_errors = false;

        for module_path in &module_files {
            let module_source = fs::read_to_string(module_path)?;
            let module_file_name = module_path
                .file_name()
                .map_or("unknown.rts", |n| n.to_str().unwrap_or("unknown.rts"));

            let module_result = compile_source(&module_source, module_file_name);

            // Collect dependencies even from modules with errors (the imports are still valid)
            all_crate_deps.extend(module_result.crate_dependencies.iter().cloned());
            any_needs_async_runtime |= module_result.needs_async_runtime;

            if module_result.has_errors {
                has_module_errors = true;
                render_errors(&module_result);
                continue;
            }

            // Derive module name from filename: "utils.rts" → "utils"
            let module_name = module_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_owned();

            // Write the module's .rs file
            fs::write(
                src_dir.join(format!("{module_name}.rs")),
                &module_result.rust_source,
            )?;

            mod_decls.push(RustModDecl {
                name: module_name,
                public: false,
                span: None,
            });
        }

        // Compile the main file with mod declarations for discovered modules
        let file_name = source_path
            .file_name()
            .map_or("unknown.rts", |n| n.to_str().unwrap_or("unknown.rts"));

        let result = if mod_decls.is_empty() {
            compile_source(&source, file_name)
        } else {
            compile_source_with_mods(&source, file_name, mod_decls)
        };

        // Collect main file dependencies
        all_crate_deps.extend(result.crate_dependencies.iter().cloned());
        any_needs_async_runtime |= result.needs_async_runtime;

        // Build Cargo.toml with collected dependencies
        let mut cargo_builder = CargoTomlBuilder::new(&self.name, "2024");

        // Add tokio if async runtime is needed (from any module)
        if any_needs_async_runtime {
            cargo_builder.add_tokio_runtime();
        }

        // Add all external crate dependencies (deduplicated by BTreeMap)
        for dep in &all_crate_deps {
            cargo_builder.add_dependency(&dep.name, DependencySpec::Simple("*".to_owned()));
        }

        // If tokio was also imported explicitly, the runtime spec takes priority
        // (add_tokio_runtime uses insert which overwrites)
        if any_needs_async_runtime {
            cargo_builder.add_tokio_runtime();
        }

        fs::write(build_dir.join("Cargo.toml"), cargo_builder.build())?;

        // Write src/main.rs (always overwrite)
        fs::write(src_dir.join("main.rs"), &result.rust_source)?;

        // If any module had errors, propagate that
        let result = if has_module_errors && !result.has_errors {
            CompileResult {
                has_errors: true,
                ..result
            }
        } else {
            result
        };

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

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let translated = translate_rustc_errors(&stderr);
            eprint!("{translated}");
            return Err(DriverError::CargoBuildFailed);
        }

        // On success, forward any stdout (e.g. "Compiling ..." messages).
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            print!("{stdout}");
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

        let output = cmd.output()?;

        // Forward stdout (program output + cargo messages).
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            print!("{stdout}");
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                let translated = translate_rustc_errors(&stderr);
                eprint!("{translated}");
            }
        }

        Ok(output.status)
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

    // ---------------------------------------------------------------
    // Task 024: Multi-file compilation
    // ---------------------------------------------------------------

    // Test 13: Driver compiles a two-file project (main + one module)
    #[test]
    fn test_project_compile_two_file_project() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("multi-file");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // Write cargo.toml
        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"multi-file\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        // Write main file with import
        fs::write(
            src_dir.join("index.rts"),
            "import { greet } from \"./utils\";\n\nfunction main() {\n  greet(\"World\");\n}\n",
        )
        .unwrap();

        // Write module file with export
        fs::write(
            src_dir.join("utils.rts"),
            "export function greet(name: string): void {\n  console.log(name);\n}\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, build_dir) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        // Check main.rs was generated with mod and use declarations
        let main_rs = fs::read_to_string(build_dir.join("src/main.rs")).unwrap();
        assert!(
            main_rs.contains("mod utils;"),
            "expected `mod utils;` in main.rs:\n{main_rs}"
        );
        assert!(
            main_rs.contains("use crate::utils::greet;"),
            "expected `use crate::utils::greet;` in main.rs:\n{main_rs}"
        );
        assert!(
            main_rs.contains("fn main()"),
            "expected `fn main()` in main.rs:\n{main_rs}"
        );

        // Check utils.rs was generated with pub fn
        let utils_rs = fs::read_to_string(build_dir.join("src/utils.rs")).unwrap();
        assert!(
            utils_rs.contains("pub fn greet(name: String)"),
            "expected `pub fn greet(name: String)` in utils.rs:\n{utils_rs}"
        );
    }

    // Test: Single-file projects still work (regression)
    #[test]
    fn test_project_compile_single_file_still_works() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("single-file", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, _) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors for single-file project, got: {:?}",
            result.diagnostics
        );
    }

    // ---------------------------------------------------------------
    // Task 031: Crate consumption — driver / CargoTomlBuilder tests
    // ---------------------------------------------------------------

    // Test 9: Generated Cargo.toml contains [dependencies] with crate entries
    #[test]
    fn test_project_compile_external_crate_cargo_toml_has_dependency() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("crate-dep");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"crate-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            src_dir.join("index.rts"),
            "import { get } from \"reqwest\";\nfunction main() { console.log(\"hi\"); }\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("[dependencies]"),
            "expected [dependencies] section in Cargo.toml:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("reqwest = \"*\""),
            "expected reqwest = \"*\" in Cargo.toml:\n{cargo_toml}"
        );
    }

    // Test 10: Project with only local imports has no [dependencies] section
    #[test]
    fn test_project_compile_local_imports_no_dependencies_section() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("local-only", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            !cargo_toml.contains("[dependencies]"),
            "expected no [dependencies] section for local-only project:\n{cargo_toml}"
        );
    }

    // Test 11: Two modules importing from same crate produce one dependency
    #[test]
    fn test_project_compile_multi_file_dedup_dependencies() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("multi-dep");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"multi-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        // Main file imports from reqwest
        fs::write(
            src_dir.join("index.rts"),
            "import { get } from \"reqwest\";\nimport { helper } from \"./utils\";\nfunction main() { console.log(\"hi\"); }\n",
        )
        .unwrap();

        // Module also imports from reqwest
        fs::write(
            src_dir.join("utils.rts"),
            "import { Client } from \"reqwest\";\nexport function helper(): void { return; }\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        // reqwest should appear exactly once
        let reqwest_count = cargo_toml.matches("reqwest").count();
        assert_eq!(
            reqwest_count, 1,
            "expected reqwest to appear once in Cargo.toml, got {reqwest_count}:\n{cargo_toml}"
        );
    }

    // Test 12: Dependencies are sorted alphabetically in output
    #[test]
    fn test_cargo_toml_builder_deps_sorted_alphabetically() {
        let mut builder = CargoTomlBuilder::new("test-app", "2024");
        builder.add_dependency("serde", DependencySpec::Simple("*".to_owned()));
        builder.add_dependency("axum", DependencySpec::Simple("*".to_owned()));
        builder.add_dependency("reqwest", DependencySpec::Simple("*".to_owned()));
        let output = builder.build();

        let axum_pos = output.find("axum").unwrap();
        let reqwest_pos = output.find("reqwest").unwrap();
        let serde_pos = output.find("serde").unwrap();
        assert!(
            axum_pos < reqwest_pos && reqwest_pos < serde_pos,
            "dependencies should be sorted alphabetically:\n{output}"
        );
    }

    // Test: CargoTomlBuilder with detailed dependency (tokio with features)
    #[test]
    fn test_cargo_toml_builder_detailed_dependency() {
        let mut builder = CargoTomlBuilder::new("test-app", "2024");
        builder.add_tokio_runtime();
        let output = builder.build();

        assert!(
            output.contains("tokio = { version = \"1\", features = [\"full\"] }"),
            "expected tokio with features in Cargo.toml:\n{output}"
        );
    }

    // Test: CargoTomlBuilder without dependencies omits [dependencies] section
    #[test]
    fn test_cargo_toml_builder_no_deps_no_section() {
        let builder = CargoTomlBuilder::new("empty-app", "2024");
        let output = builder.build();

        assert!(
            !output.contains("[dependencies]"),
            "expected no [dependencies] section:\n{output}"
        );
        assert!(
            output.contains("[workspace]"),
            "expected [workspace] section:\n{output}"
        );
    }

    // Test: Existing Cargo.toml test still works with CargoTomlBuilder
    #[test]
    fn test_project_compile_cargo_toml_still_has_package_info() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("builder-test", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("name = \"builder-test\""),
            "expected name in Cargo.toml:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("edition = \"2024\""),
            "expected edition in Cargo.toml:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("[workspace]"),
            "expected [workspace] section in Cargo.toml:\n{cargo_toml}"
        );
    }

    // ---------------------------------------------------------------
    // Task 029: Async lowering and tokio runtime integration — driver tests
    // ---------------------------------------------------------------

    // Test 1: Driver — async Cargo.toml includes tokio
    #[test]
    fn test_project_compile_async_main_cargo_toml_has_tokio() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("async-app");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"async-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            src_dir.join("index.rts"),
            r#"async function main() {
  const data = await fetchData();
  console.log(data);
}

async function fetchData(): string {
  return "hello";
}
"#,
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, build_dir) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("[dependencies]"),
            "expected [dependencies] section in Cargo.toml:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("tokio = { version = \"1\", features = [\"full\"] }"),
            "expected tokio dependency in Cargo.toml:\n{cargo_toml}"
        );
    }

    // Test 2: Driver — non-async Cargo.toml does NOT include tokio
    #[test]
    fn test_project_compile_non_async_cargo_toml_no_tokio() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("sync-app", tmp.path()).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            !cargo_toml.contains("tokio"),
            "expected no tokio in Cargo.toml for non-async project:\n{cargo_toml}"
        );
        assert!(
            !cargo_toml.contains("[dependencies]"),
            "expected no [dependencies] section for non-async project:\n{cargo_toml}"
        );
    }
}
