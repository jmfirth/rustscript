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
use rsc_syntax::diagnostic::{ColorMode, Severity, render_diagnostics_colored};
use rsc_syntax::rust_ir::RustModDecl;

use crate::error::{DriverError, Result};
use crate::error_translation::{
    parse_rustc_json_diagnostics, render_rustc_json_diagnostics, translate_rustc_errors_colored,
};
use crate::pipeline::CompileResult;
use crate::rustdoc_cache;
use crate::rustdoc_convert;

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

/// Represents a WASM compilation target and its specific behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WasmTarget {
    /// `wasm32-unknown-unknown` — browser WASM (no WASI). Produces a cdylib.
    Unknown,
    /// `wasm32-wasip1` — WASI preview 1 (server-side WASM). Can be a binary.
    Wasip1,
}

impl WasmTarget {
    /// The full target triple string.
    #[must_use]
    pub fn triple(&self) -> &'static str {
        match self {
            Self::Unknown => "wasm32-unknown-unknown",
            Self::Wasip1 => "wasm32-wasip1",
        }
    }
}

/// Parse a `--target` value and determine if it is a WASM target.
///
/// Returns `Some(WasmTarget)` for recognized WASM targets, `None` for non-WASM targets.
/// Non-WASM targets are passed through to Cargo unmodified.
#[must_use]
pub fn parse_wasm_target(target: &str) -> Option<WasmTarget> {
    match target {
        "wasm32-unknown-unknown" => Some(WasmTarget::Unknown),
        "wasm32-wasip1" => Some(WasmTarget::Wasip1),
        _ if target.starts_with("wasm32") => {
            // Other wasm32 targets — treat as unknown-unknown behavior.
            Some(WasmTarget::Unknown)
        }
        _ => None,
    }
}

/// The kind of Cargo project to generate.
#[derive(Debug, Clone, Default)]
enum CrateKind {
    /// A binary with `src/main.rs` (the default).
    #[default]
    Binary,
    /// A library with `src/lib.rs` and a specified `crate-type`.
    Library { crate_types: Vec<String> },
}

/// Structured builder for generating `Cargo.toml` content.
///
/// Uses `BTreeMap` for deterministic alphabetical ordering of dependencies.
struct CargoTomlBuilder {
    name: String,
    edition: String,
    dependencies: BTreeMap<String, DependencySpec>,
    crate_kind: CrateKind,
}

impl CargoTomlBuilder {
    /// Create a new builder with the given project name and edition.
    fn new(name: &str, edition: &str) -> Self {
        Self {
            name: name.to_owned(),
            edition: edition.to_owned(),
            dependencies: BTreeMap::new(),
            crate_kind: CrateKind::default(),
        }
    }

    /// Set the crate kind to a library with the given crate types.
    fn set_library(&mut self, crate_types: Vec<String>) {
        self.crate_kind = CrateKind::Library { crate_types };
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

        // Crate kind section ([lib] or [[bin]])
        match &self.crate_kind {
            CrateKind::Binary => {
                // Default — Cargo infers [[bin]] from src/main.rs
            }
            CrateKind::Library { crate_types } => {
                let _ = writeln!(out);
                let _ = writeln!(out, "[lib]");
                let types_str = crate_types
                    .iter()
                    .map(|t| format!("\"{t}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(out, "crate-type = [{types_str}]");
            }
        }

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
    /// Compilation options (e.g., `--no-borrow-inference`).
    pub compile_options: crate::pipeline::CompileOptions,
    /// Color mode for diagnostic rendering.
    pub color_mode: ColorMode,
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
                    compile_options: crate::pipeline::CompileOptions::default(),
                    color_mode: ColorMode::default(),
                });
            }

            // Check for src/ directory with .rts files
            let src_dir = current.join("src");
            if src_dir.is_dir() && has_rts_files(&src_dir) {
                let name = project_name_from_dir(&current);
                return Ok(Self {
                    root: current,
                    name,
                    compile_options: crate::pipeline::CompileOptions::default(),
                    color_mode: ColorMode::default(),
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

    /// Return the project's source directory (`<root>/src`).
    #[must_use]
    pub fn source_dir(&self) -> PathBuf {
        self.root.join("src")
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
    /// Returns the [`CompileResult`] for the main file, the path to the build directory,
    /// the original `.rts` source text, and the `.rts` display filename.
    ///
    /// # Errors
    ///
    /// Returns an error if the main source file cannot be found or read, or if
    /// the build directory cannot be created.
    #[allow(clippy::too_many_lines)]
    // Multi-file project compilation orchestrates many steps; splitting would obscure the flow
    pub fn compile(&self) -> Result<(CompileResult, PathBuf, String, String)> {
        let source_path = self.main_source()?;
        let source = fs::read_to_string(&source_path)?;
        let module_files = self.discover_modules()?;

        let build_dir = self.root.join(BUILD_DIR);
        let src_dir = build_dir.join("src");
        fs::create_dir_all(&src_dir)?;

        // External signatures will be loaded after Cargo.toml is written and
        // rustdoc JSON is generated (below). Initialized empty for now.
        let compile_options = crate::pipeline::CompileOptions {
            no_borrow_inference: self.compile_options.no_borrow_inference,
            ..crate::pipeline::CompileOptions::default()
        };

        // Collect all crate dependencies from all compiled modules
        let mut all_crate_deps: Vec<CrateDependency> = Vec::new();
        // Track whether any module uses async (OR across all modules)
        let mut any_needs_async_runtime = false;
        // Track whether any module needs the futures crate
        let mut any_needs_futures_crate = false;
        // Track whether any module uses JSON.stringify/parse (needs serde_json)
        let mut any_needs_serde_json = false;
        // Track whether any module uses Math.random() (needs rand)
        let mut any_needs_rand = false;
        // Track whether any module uses derives Serialize/Deserialize (needs serde)
        let mut any_needs_serde = false;
        // Track whether any module uses new RegExp() (needs regex)
        let mut any_needs_regex = false;

        // Compile each module file and collect mod declarations
        let mut mod_decls = Vec::new();
        let mut has_module_errors = false;

        for module_path in &module_files {
            let module_source = fs::read_to_string(module_path)?;
            let module_file_name = module_path
                .file_name()
                .map_or("unknown.rts", |n| n.to_str().unwrap_or("unknown.rts"));

            let module_result = crate::pipeline::compile_source_with_options(
                &module_source,
                module_file_name,
                &compile_options,
            );

            // Collect dependencies even from modules with errors (the imports are still valid)
            all_crate_deps.extend(module_result.crate_dependencies.iter().cloned());
            any_needs_async_runtime |= module_result.needs_async_runtime;
            any_needs_futures_crate |= module_result.needs_futures_crate;
            any_needs_serde_json |= module_result.needs_serde_json;
            any_needs_rand |= module_result.needs_rand;
            any_needs_serde |= module_result.needs_serde;
            any_needs_regex |= module_result.needs_regex;

            if module_result.has_errors {
                has_module_errors = true;
                render_errors(&module_result, self.color_mode);
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
            crate::pipeline::compile_source_with_options(&source, file_name, &compile_options)
        } else {
            crate::pipeline::compile_source_with_mods_and_options(
                &source,
                file_name,
                mod_decls.clone(),
                &compile_options,
            )
        };

        // Collect main file dependencies
        all_crate_deps.extend(result.crate_dependencies.iter().cloned());
        any_needs_async_runtime |= result.needs_async_runtime;
        any_needs_futures_crate |= result.needs_futures_crate;
        any_needs_serde_json |= result.needs_serde_json;
        any_needs_rand |= result.needs_rand;
        any_needs_serde |= result.needs_serde;
        any_needs_regex |= result.needs_regex;

        // Build Cargo.toml with collected dependencies
        let mut cargo_builder = CargoTomlBuilder::new(&self.name, "2024");

        // Add tokio if async runtime is needed (from any module) — but NOT for WASM targets
        if any_needs_async_runtime {
            cargo_builder.add_tokio_runtime();
        }

        // Add futures crate if for-await or Promise.any is used
        if any_needs_futures_crate {
            cargo_builder.add_dependency("futures", DependencySpec::Simple("0.3".to_owned()));
        }

        // Add serde_json if JSON.stringify/parse is used
        if any_needs_serde_json {
            cargo_builder.add_dependency("serde_json", DependencySpec::Simple("1".to_owned()));
        }

        // Add rand crate if Math.random() is used
        if any_needs_rand {
            cargo_builder.add_dependency("rand", DependencySpec::Simple("0.8".to_owned()));
        }

        // Add regex crate if new RegExp() is used
        if any_needs_regex {
            cargo_builder.add_dependency("regex", DependencySpec::Simple("1".to_owned()));
        }

        // Add serde with derive feature if derives Serialize/Deserialize is used
        if any_needs_serde {
            cargo_builder.add_dependency(
                "serde",
                DependencySpec::Detailed {
                    version: "1".to_owned(),
                    features: vec!["derive".to_owned()],
                },
            );
        }

        // Add all external crate dependencies (deduplicated by BTreeMap)
        for dep in &all_crate_deps {
            cargo_builder.add_dependency(&dep.name, DependencySpec::Simple("*".to_owned()));
        }

        // Add explicit dependencies from rsc.toml (these take priority via insert)
        if let Ok((explicit_deps, _dev_deps)) = crate::deps::read_config(&self.root) {
            for (name, entry) in &explicit_deps {
                let spec = if entry.features.is_empty() {
                    DependencySpec::Simple(entry.version.clone())
                } else {
                    DependencySpec::Detailed {
                        version: entry.version.clone(),
                        features: entry.features.clone(),
                    }
                };
                // Use insert to override auto-detected `"*"` versions with explicit ones
                cargo_builder.dependencies.insert(name.clone(), spec);
            }
        }

        // If tokio was also imported explicitly, the runtime spec takes priority
        // (add_tokio_runtime uses insert which overwrites)
        if any_needs_async_runtime {
            cargo_builder.add_tokio_runtime();
        }

        fs::write(build_dir.join("Cargo.toml"), cargo_builder.build())?;

        // Write src/main.rs (always overwrite)
        fs::write(src_dir.join("main.rs"), &result.rust_source)?;

        // Generate rustdoc JSON for dependencies and re-compile with external
        // signatures. This enables proper param types, throws detection, and
        // async handling for external crate functions.
        if !all_crate_deps.is_empty() {
            Self::maybe_generate_rustdoc(&build_dir);
            let enriched_options = self.load_compile_options_with_rustdoc(&build_dir);
            if !enriched_options.external_signatures.is_empty() {
                // Re-compile with external signatures for better output.
                let enriched_result = if mod_decls.is_empty() {
                    crate::pipeline::compile_source_with_options(
                        &source,
                        file_name,
                        &enriched_options,
                    )
                } else {
                    crate::pipeline::compile_source_with_mods_and_options(
                        &source,
                        file_name,
                        mod_decls.clone(),
                        &enriched_options,
                    )
                };
                if !enriched_result.has_errors {
                    let _ = fs::write(src_dir.join("main.rs"), &enriched_result.rust_source);
                }
            }
        }

        // If any module had errors, propagate that
        let result = if has_module_errors && !result.has_errors {
            CompileResult {
                has_errors: true,
                ..result
            }
        } else {
            result
        };

        let rts_filename = format!("src/{file_name}");
        Ok((result, build_dir, source, rts_filename))
    }

    /// Build `CompileOptions` with external function signatures loaded from
    /// cached rustdoc JSON (if available from a previous compilation).
    fn load_compile_options_with_rustdoc(
        &self,
        build_dir: &Path,
    ) -> crate::pipeline::CompileOptions {
        let mut options = crate::pipeline::CompileOptions {
            no_borrow_inference: self.compile_options.no_borrow_inference,
            ..crate::pipeline::CompileOptions::default()
        };

        // Check if any cached rustdoc JSON exists from a prior build.
        let doc_dir = build_dir.join("target").join("doc");
        if !doc_dir.is_dir() {
            return options;
        }

        // Scan for .json files in the doc directory.
        let json_files: Vec<_> = fs::read_dir(&doc_dir)
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect();

        if json_files.is_empty() {
            return options;
        }

        // Load and convert each cached crate's rustdoc data.
        let mut cache = crate::rustdoc_cache::RustdocCache::new();
        for entry in &json_files {
            let crate_name = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(std::borrow::ToOwned::to_owned);
            if let Some(name) = crate_name
                && let Some(crate_data) = cache.get_crate_docs(&name, build_dir)
            {
                let fns = rustdoc_convert::convert_crate_to_external_fns(&name, &crate_data);
                options.external_signatures.extend(fns);
            }
        }

        options
    }

    /// Generate rustdoc JSON for dependencies if not already cached.
    ///
    /// This is non-fatal: if rustdoc generation fails (e.g., nightly not
    /// installed), compilation continues without external signature info.
    fn maybe_generate_rustdoc(build_dir: &Path) {
        // Simple caching: skip if the doc directory already has .json files.
        let doc_dir = build_dir.join("target").join("doc");
        if doc_dir.is_dir() {
            let has_json = fs::read_dir(&doc_dir)
                .into_iter()
                .flatten()
                .filter_map(std::result::Result::ok)
                .any(|e| e.path().extension().is_some_and(|ext| ext == "json"));
            if has_json {
                return;
            }
        }

        eprintln!("Generating dependency docs...");
        let _ = rustdoc_cache::generate_rustdoc_json(build_dir);
    }

    /// Compile the project with WASM-specific adjustments.
    ///
    /// Delegates to [`compile`](Self::compile) and then patches the generated
    /// `Cargo.toml` and source files for WASM targets:
    /// - Removes `tokio` dependency (async runtime not available for WASM)
    /// - Sets `crate-type = ["cdylib"]` for `wasm32-unknown-unknown`
    /// - Writes `lib.rs` instead of `main.rs` for library WASM targets
    fn compile_for_target(
        &self,
        wasm_target: Option<&WasmTarget>,
    ) -> Result<(CompileResult, PathBuf, String, String)> {
        let (result, build_dir, source, rts_filename) = self.compile()?;

        let Some(wt) = wasm_target else {
            return Ok((result, build_dir, source, rts_filename));
        };

        // Rebuild the Cargo.toml with WASM-specific settings
        let src_dir = build_dir.join("src");
        let mut cargo_builder = CargoTomlBuilder::new(&self.name, "2024");

        // For wasm32-unknown-unknown: library (cdylib), no main
        // For wasm32-wasip1: binary if there's a main() function
        let is_library = matches!(wt, WasmTarget::Unknown);

        if is_library {
            cargo_builder.set_library(vec!["cdylib".to_owned()]);
        }

        // Add external crate dependencies from the compile result — but NOT tokio
        for dep in &result.crate_dependencies {
            if dep.name != "tokio" {
                cargo_builder.add_dependency(&dep.name, DependencySpec::Simple("*".to_owned()));
            }
        }

        // Add explicit dependencies from rsc.toml — but NOT tokio for WASM
        if let Ok((explicit_deps, _dev_deps)) = crate::deps::read_config(&self.root) {
            for (name, entry) in &explicit_deps {
                if name == "tokio" {
                    continue;
                }
                let spec = if entry.features.is_empty() {
                    DependencySpec::Simple(entry.version.clone())
                } else {
                    DependencySpec::Detailed {
                        version: entry.version.clone(),
                        features: entry.features.clone(),
                    }
                };
                cargo_builder.dependencies.insert(name.clone(), spec);
            }
        }

        // Do NOT add tokio for WASM targets — it is not supported

        fs::write(build_dir.join("Cargo.toml"), cargo_builder.build())?;

        // For library targets, write lib.rs instead of main.rs
        if is_library {
            let lib_path = src_dir.join("lib.rs");
            let main_path = src_dir.join("main.rs");
            // Copy main.rs content to lib.rs (the Rust source is the same)
            if main_path.is_file() {
                let content = fs::read_to_string(&main_path)?;
                fs::write(&lib_path, content)?;
                // Remove main.rs to avoid ambiguity
                let _ = fs::remove_file(&main_path);
            }
        }

        Ok((result, build_dir, source, rts_filename))
    }

    /// Build the project: compile, then invoke `cargo build` on the output.
    ///
    /// If `release` is true, passes `--release` to Cargo. If `target` is provided,
    /// passes `--target <triple>` to Cargo (e.g., for WASM cross-compilation).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::CompilationFailed`] if the `RustScript` compilation
    /// produces errors, or [`DriverError::CargoBuildFailed`] if `cargo build` fails.
    pub fn build(&self, release: bool, target: Option<&str>) -> Result<()> {
        let wasm_target = target.and_then(parse_wasm_target);
        let (result, build_dir, rts_source, rts_filename) =
            self.compile_for_target(wasm_target.as_ref())?;

        if result.has_errors {
            render_errors(&result, self.color_mode);
            let error_count = result
                .diagnostics
                .iter()
                .filter(|d| matches!(d.severity, Severity::Error))
                .count();
            return Err(DriverError::CompilationFailed(error_count));
        }

        // Emit warning if async + WASM
        if wasm_target.is_some() && result.needs_async_runtime {
            eprintln!(
                "{}",
                format_status(
                    "warning:",
                    "async runtime (tokio) is not available for WASM targets",
                    StatusStyle::Warning,
                    self.color_mode,
                )
            );
        }

        eprintln!(
            "{}",
            format_status(
                "Compiling",
                &format!("{} v0.1.0", self.name),
                StatusStyle::Success,
                self.color_mode,
            )
        );

        let mut cmd = Command::new("cargo");
        cmd.arg("build")
            .arg("--message-format=json")
            .current_dir(&build_dir);
        if release {
            cmd.arg("--release");
        }
        if let Some(t) = target {
            cmd.arg("--target").arg(t);
        }
        let output = cmd.output()?;

        if !output.status.success() {
            self.handle_build_failure(
                &output,
                &result,
                wasm_target.as_ref(),
                &rts_source,
                &rts_filename,
            );
            return Err(DriverError::CargoBuildFailed);
        }

        let profile_label = if release {
            "release [optimized]"
        } else {
            "dev [unoptimized + debuginfo]"
        };
        eprintln!(
            "{}",
            format_status(
                "Finished",
                &format!("`{profile_label}` target"),
                StatusStyle::Success,
                self.color_mode,
            )
        );

        // Report the output path for WASM targets
        if let Some(ref wt) = wasm_target {
            let profile = if release { "release" } else { "debug" };
            let triple = wt.triple();
            let ext = "wasm";
            let artifact_path = build_dir
                .join("target")
                .join(triple)
                .join(profile)
                .join(format!("{}.{ext}", self.name));
            println!("WASM output: {}", artifact_path.display());
        }

        Ok(())
    }

    /// Handle a failed `cargo build` by translating and printing error messages.
    ///
    /// Attempts structured JSON parsing from stdout first (when `--message-format=json`
    /// is used), then falls back to regex-based stderr translation if no JSON
    /// diagnostics are found.
    fn handle_build_failure(
        &self,
        output: &std::process::Output,
        result: &CompileResult,
        wasm_target: Option<&WasmTarget>,
        rts_source: &str,
        rts_filename: &str,
    ) {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Check for "target may not be installed" hint from Cargo
        if let Some(wt) = wasm_target
            && (stderr.contains("target may not be installed")
                || stderr.contains("can't find crate")
                || stderr.contains("no matching package"))
        {
            let triple = wt.triple();
            eprintln!(
                "{}",
                format_status(
                    "error:",
                    &format!("WASM target not installed. Run: rustup target add {triple}"),
                    StatusStyle::Error,
                    self.color_mode,
                )
            );
            return;
        }

        let source_map = if result.source_map_lines.is_empty() {
            None
        } else {
            Some(result.source_map_lines.as_slice())
        };

        // Try structured JSON parsing from stdout first
        let stdout = String::from_utf8_lossy(&output.stdout);
        let diagnostics = parse_rustc_json_diagnostics(&stdout);

        if diagnostics.is_empty() {
            // Fall back to regex-based stderr translation
            let translated = translate_rustc_errors_colored(
                &stderr,
                source_map,
                Some(rts_source),
                Some(rts_filename),
                self.color_mode,
            );
            eprint!("{translated}");
        } else {
            let translated = render_rustc_json_diagnostics(
                &diagnostics,
                source_map,
                Some(rts_source),
                Some(rts_filename),
                self.color_mode,
            );
            eprint!("{translated}");
        }
    }

    /// Run the project: compile, then invoke `cargo run` on the output.
    ///
    /// Forwards `args` to the compiled program. If `target` is a WASM target,
    /// returns [`DriverError::WasmRunUnsupported`] since WASM cannot be run directly.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::WasmRunUnsupported`] if the target is a WASM triple,
    /// [`DriverError::CompilationFailed`] if the `RustScript` compilation
    /// produces errors, or an I/O error if `cargo run` cannot be spawned.
    pub fn run(&self, args: &[String], target: Option<&str>) -> Result<std::process::ExitStatus> {
        // Reject WASM targets — can't run WASM directly
        if let Some(t) = target
            && parse_wasm_target(t).is_some()
        {
            return Err(DriverError::WasmRunUnsupported);
        }

        let (result, build_dir, rts_source, rts_filename) = self.compile()?;

        if result.has_errors {
            render_errors(&result, self.color_mode);
            let error_count = result
                .diagnostics
                .iter()
                .filter(|d| matches!(d.severity, Severity::Error))
                .count();
            return Err(DriverError::CompilationFailed(error_count));
        }

        let mut cmd = Command::new("cargo");
        cmd.arg("run")
            .arg("--message-format=json")
            .current_dir(&build_dir);

        if !args.is_empty() {
            cmd.arg("--");
            cmd.args(args);
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let source_map = if result.source_map_lines.is_empty() {
                None
            } else {
                Some(result.source_map_lines.as_slice())
            };

            // Try structured JSON parsing from stdout first
            let stdout = String::from_utf8_lossy(&output.stdout);
            let diagnostics = parse_rustc_json_diagnostics(&stdout);

            if diagnostics.is_empty() {
                // Fall back to regex-based stderr translation
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.is_empty() {
                    let translated = translate_rustc_errors_colored(
                        &stderr,
                        source_map,
                        Some(&rts_source),
                        Some(&rts_filename),
                        self.color_mode,
                    );
                    eprint!("{translated}");
                }
            } else {
                let translated = render_rustc_json_diagnostics(
                    &diagnostics,
                    source_map,
                    Some(&rts_source),
                    Some(&rts_filename),
                    self.color_mode,
                );
                eprint!("{translated}");
            }
        }

        Ok(output.status)
    }

    /// Run tests: compile the project, then invoke `cargo test` in the build directory.
    ///
    /// Forwards `args` to `cargo test` (after `--` separator). If `release` is true,
    /// passes `--release` to cargo.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::CompilationFailed`] if `RustScript` compilation fails,
    /// or [`DriverError::CargoBuildFailed`] if `cargo test` fails.
    pub fn test(&self, release: bool, args: &[String]) -> Result<std::process::ExitStatus> {
        let (result, build_dir, rts_source, rts_filename) = self.compile()?;

        if result.has_errors {
            render_errors(&result, self.color_mode);
            let error_count = result
                .diagnostics
                .iter()
                .filter(|d| matches!(d.severity, Severity::Error))
                .count();
            return Err(DriverError::CompilationFailed(error_count));
        }

        let mut cmd = Command::new("cargo");
        cmd.arg("test")
            .arg("--message-format=json")
            .current_dir(&build_dir);

        if release {
            cmd.arg("--release");
        }

        if !args.is_empty() {
            cmd.arg("--");
            cmd.args(args);
        }

        let output = cmd.output()?;

        // Forward stderr: translate on failure, pass through on success.
        if output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                // On success, forward stderr as-is (cargo progress messages).
                eprint!("{stderr}");
            }
        } else {
            let source_map = if result.source_map_lines.is_empty() {
                None
            } else {
                Some(result.source_map_lines.as_slice())
            };

            // Try structured JSON parsing from stdout first
            let stdout = String::from_utf8_lossy(&output.stdout);
            let diagnostics = parse_rustc_json_diagnostics(&stdout);

            if diagnostics.is_empty() {
                // Fall back to regex-based stderr translation
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.is_empty() {
                    let translated = translate_rustc_errors_colored(
                        &stderr,
                        source_map,
                        Some(&rts_source),
                        Some(&rts_filename),
                        self.color_mode,
                    );
                    eprint!("{translated}");
                }
            } else {
                let translated = render_rustc_json_diagnostics(
                    &diagnostics,
                    source_map,
                    Some(&rts_source),
                    Some(&rts_filename),
                    self.color_mode,
                );
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
///   .gitignore    (if template provides one)
/// ```
///
/// When `template` is `None`, creates a bare hello-world project (backward
/// compatible with the original behavior). When a template name is provided,
/// scaffolds the project with pre-configured dependencies and starter code.
///
/// # Errors
///
/// Returns [`DriverError::ProjectExists`] if `parent_dir/{name}` already exists,
/// [`DriverError::InvalidTemplate`] if the template name is not recognized,
/// or an I/O error if directory/file creation fails.
pub fn init_project(name: &str, parent_dir: &Path, template: Option<&str>) -> Result<PathBuf> {
    // Validate the template name before creating any directories.
    if let Some(t) = template
        && crate::templates::get_template(t).is_none()
    {
        return Err(DriverError::InvalidTemplate(t.to_owned()));
    }

    let project_dir = parent_dir.join(name);

    if project_dir.exists() {
        return Err(DriverError::ProjectExists(project_dir));
    }

    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    if let Some(tmpl) = template.and_then(crate::templates::get_template) {
        // Template-based initialization
        let cargo_toml = tmpl.cargo_toml.replace("{name}", name);
        fs::write(project_dir.join(PROJECT_MANIFEST), cargo_toml)?;
        fs::write(src_dir.join("index.rts"), tmpl.index_rts)?;
        if let Some(gitignore) = tmpl.gitignore {
            fs::write(project_dir.join(".gitignore"), gitignore)?;
        }
    } else {
        // Default (bare) project — backward compatible
        let cargo_toml =
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
        fs::write(project_dir.join(PROJECT_MANIFEST), cargo_toml)?;
        fs::write(src_dir.join("index.rts"), HELLO_WORLD_SOURCE)?;
    }

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

/// Render error diagnostics to stderr with the specified color mode.
fn render_errors(result: &CompileResult, color: ColorMode) {
    let mut stderr = std::io::stderr();
    // Ignore rendering errors — we're already in an error path.
    let _ = render_diagnostics_colored(&result.diagnostics, &result.source_map, &mut stderr, color);
}

/// Format a status label with ANSI color when enabled.
///
/// Labels are right-aligned to 12 characters (matching Cargo's formatting).
/// Green is used for progress labels, red for errors, yellow for warnings.
fn format_status(label: &str, message: &str, style: StatusStyle, color: ColorMode) -> String {
    match color {
        ColorMode::Always => {
            let code = match style {
                StatusStyle::Success => "\x1b[1;32m", // bold green
                StatusStyle::Error => "\x1b[1;31m",   // bold red
                StatusStyle::Warning => "\x1b[1;33m", // bold yellow
                StatusStyle::Note => "\x1b[1;36m",    // bold cyan
            };
            format!("{code}{label:>12}\x1b[0m {message}")
        }
        ColorMode::Never => {
            format!("{label:>12} {message}")
        }
    }
}

/// Style for status labels in build output.
#[derive(Debug, Clone, Copy)]
enum StatusStyle {
    /// Green — compiling, finished, success labels.
    Success,
    /// Red — error labels.
    Error,
    /// Yellow — warning labels.
    Warning,
    /// Cyan — note/help labels.
    #[allow(dead_code)]
    Note,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Test 4: init_project creates correct directory structure
    #[test]
    fn test_init_project_creates_directory_structure() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        assert!(project_dir.join("src").is_dir());
        assert!(project_dir.join("src/index.rts").is_file());
        assert!(project_dir.join("cargo.toml").is_file());
    }

    // Test 5: init_project creates cargo.toml with correct package name
    #[test]
    fn test_init_project_cargo_toml_has_correct_name() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

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
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

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
        init_project("test-app", tmp.path(), None).unwrap();

        let err = init_project("test-app", tmp.path(), None).unwrap_err();
        assert!(
            matches!(err, DriverError::ProjectExists(_)),
            "expected ProjectExists, got: {err:?}"
        );
    }

    // Test 8: Project::open finds project in current directory
    #[test]
    fn test_project_open_finds_project_in_directory() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        assert_eq!(project.name, "test-app");
        assert_eq!(project.root, project_dir);
    }

    // Test 9: Project::main_source returns src/index.rts when it exists
    #[test]
    fn test_project_main_source_returns_index_rts() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

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
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, build_dir, _, _) = project.compile().unwrap();

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
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project.compile().unwrap();

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
        let project_dir = init_project("hello-project", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, build_dir, _, _) = project.compile().unwrap();

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
        let project_dir = init_project("walkup", tmp.path(), None).unwrap();

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
        let project_dir = init_project("preserve-test", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();

        // First compile
        let (_, build_dir, _, _) = project.compile().unwrap();

        // Simulate a previous cargo build by creating a target/ directory
        let target_dir = build_dir.join("target");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("marker"), "should survive").unwrap();

        // Also create a Cargo.lock
        fs::write(build_dir.join("Cargo.lock"), "# lock file").unwrap();

        // Recompile
        let (_, build_dir2, _, _) = project.compile().unwrap();
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
        let (result, build_dir, _, _) = project.compile().unwrap();

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
            utils_rs.contains("pub fn greet(name: &str)"),
            "expected `pub fn greet(name: &str)` in utils.rs:\n{utils_rs}"
        );
    }

    // Test: Single-file projects still work (regression)
    #[test]
    fn test_project_compile_single_file_still_works() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("single-file", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, _, _, _) = project.compile().unwrap();

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
        let (_, build_dir, _, _) = project.compile().unwrap();

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
        let project_dir = init_project("local-only", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project.compile().unwrap();

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
        let (_, build_dir, _, _) = project.compile().unwrap();

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
        let project_dir = init_project("builder-test", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project.compile().unwrap();

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
        let (result, build_dir, _, _) = project.compile().unwrap();

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
        let project_dir = init_project("sync-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project.compile().unwrap();

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

    // ---------------------------------------------------------------
    // Task 037: Project::test
    // ---------------------------------------------------------------

    // Test: Project::test compiles first — compilation error returns CompilationFailed
    #[test]
    fn test_project_test_compilation_error_returns_compilation_failed() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("test-compile-err");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"test-compile-err\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        // Write invalid source
        fs::write(src_dir.join("index.rts"), "function {").unwrap();

        let project = Project::open(&project_dir).unwrap();
        let err = project.test(false, &[]).unwrap_err();
        assert!(
            matches!(err, DriverError::CompilationFailed(_)),
            "expected CompilationFailed, got: {err:?}"
        );
    }

    // Test: Project has a public test method (compile check)
    #[test]
    fn test_project_test_method_exists() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-exists", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        // Verify the method signature compiles — this is a type-level assertion
        let _: fn(&Project, bool, &[String]) -> Result<std::process::ExitStatus> = Project::test;
        drop(project);
    }

    // ---------------------------------------------------------------
    // Task 039: Project templates
    // ---------------------------------------------------------------

    // Test 1: Default init unchanged — produces same output as before
    #[test]
    fn test_init_project_default_template_unchanged() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-default", tmp.path(), None).unwrap();

        assert!(project_dir.join("src/index.rts").is_file());
        assert!(project_dir.join("cargo.toml").is_file());
        assert!(!project_dir.join(".gitignore").exists());

        let cargo = fs::read_to_string(project_dir.join("cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"test-default\""));
        assert!(cargo.contains("edition = \"2024\""));

        let source = fs::read_to_string(project_dir.join("src/index.rts")).unwrap();
        assert!(source.contains("Hello, World!"));
    }

    // Test 2: CLI template creates cargo.toml with clap dependency
    #[test]
    fn test_init_project_cli_template_has_clap() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-cli", tmp.path(), Some("cli")).unwrap();

        let cargo = fs::read_to_string(project_dir.join("cargo.toml")).unwrap();
        assert!(
            cargo.contains("clap = { version = \"4\", features = [\"derive\"] }"),
            "expected clap dependency in cargo.toml:\n{cargo}",
        );
        assert!(cargo.contains("name = \"test-cli\""));
    }

    // Test 3: Web server template creates cargo.toml with axum, tokio, serde
    #[test]
    fn test_init_project_web_server_template_has_deps() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-web", tmp.path(), Some("web-server")).unwrap();

        let cargo = fs::read_to_string(project_dir.join("cargo.toml")).unwrap();
        assert!(
            cargo.contains("axum"),
            "expected axum in cargo.toml:\n{cargo}"
        );
        assert!(
            cargo.contains("tokio"),
            "expected tokio in cargo.toml:\n{cargo}"
        );
        assert!(
            cargo.contains("serde"),
            "expected serde in cargo.toml:\n{cargo}"
        );
        assert!(
            cargo.contains("serde_json"),
            "expected serde_json in cargo.toml:\n{cargo}"
        );
        assert!(cargo.contains("name = \"test-web\""));
    }

    // Test 4: WASM template creates cargo.toml with wasm-bindgen and [lib] section
    #[test]
    fn test_init_project_wasm_template_has_lib_and_wasm_bindgen() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-wasm", tmp.path(), Some("wasm")).unwrap();

        let cargo = fs::read_to_string(project_dir.join("cargo.toml")).unwrap();
        assert!(
            cargo.contains("wasm-bindgen"),
            "expected wasm-bindgen in cargo.toml:\n{cargo}",
        );
        assert!(
            cargo.contains("[lib]"),
            "expected [lib] section in cargo.toml:\n{cargo}",
        );
        assert!(
            cargo.contains("crate-type = [\"cdylib\"]"),
            "expected cdylib crate-type in cargo.toml:\n{cargo}",
        );
    }

    // Test 5: Invalid template returns InvalidTemplate error
    #[test]
    fn test_init_project_invalid_template_returns_error() {
        let tmp = TempDir::new().unwrap();
        let err = init_project("test-bad", tmp.path(), Some("invalid")).unwrap_err();
        assert!(
            matches!(err, DriverError::InvalidTemplate(ref t) if t == "invalid"),
            "expected InvalidTemplate, got: {err:?}",
        );
    }

    // Test 6: CLI, web-server, and wasm templates create .gitignore
    #[test]
    fn test_init_project_templates_create_gitignore() {
        let tmp = TempDir::new().unwrap();

        let cli_dir = init_project("t-cli", tmp.path(), Some("cli")).unwrap();
        assert!(cli_dir.join(".gitignore").is_file());

        let web_dir = init_project("t-web", tmp.path(), Some("web-server")).unwrap();
        assert!(web_dir.join(".gitignore").is_file());

        let wasm_dir = init_project("t-wasm", tmp.path(), Some("wasm")).unwrap();
        assert!(wasm_dir.join(".gitignore").is_file());
    }

    // Test 7: Default template does not create .gitignore
    #[test]
    fn test_init_project_default_no_gitignore() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("t-default", tmp.path(), None).unwrap();
        assert!(!project_dir.join(".gitignore").exists());
    }

    // Test 8: Template starter code contains expected patterns
    #[test]
    fn test_init_project_cli_template_starter_has_main() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("s-cli", tmp.path(), Some("cli")).unwrap();

        let source = fs::read_to_string(project_dir.join("src/index.rts")).unwrap();
        assert!(
            source.contains("function main()"),
            "expected function main() in CLI starter:\n{source}",
        );
    }

    // Test: Web server template starter has async main
    #[test]
    fn test_init_project_web_server_template_starter_has_async_main() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("s-web", tmp.path(), Some("web-server")).unwrap();

        let source = fs::read_to_string(project_dir.join("src/index.rts")).unwrap();
        assert!(
            source.contains("async function main()"),
            "expected async function main() in web-server starter:\n{source}",
        );
    }

    // Test: WASM template starter has greet function and main
    #[test]
    fn test_init_project_wasm_template_starter_has_greet() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("s-wasm", tmp.path(), Some("wasm")).unwrap();

        let source = fs::read_to_string(project_dir.join("src/index.rts")).unwrap();
        assert!(
            source.contains("function greet("),
            "expected function greet() in WASM starter:\n{source}",
        );
        assert!(
            source.contains("function main()"),
            "expected function main() in WASM starter:\n{source}",
        );
    }

    // Test: gitignore content contains .rsc-build
    #[test]
    fn test_init_project_gitignore_has_rsc_build() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("gi-cli", tmp.path(), Some("cli")).unwrap();

        let gitignore = fs::read_to_string(project_dir.join(".gitignore")).unwrap();
        assert!(
            gitignore.contains(".rsc-build"),
            "expected .rsc-build in .gitignore:\n{gitignore}",
        );
        assert!(
            gitignore.contains("/target"),
            "expected /target in .gitignore:\n{gitignore}",
        );
    }

    // Test: WASM gitignore also includes /pkg
    #[test]
    fn test_init_project_wasm_gitignore_has_pkg() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("gi-wasm", tmp.path(), Some("wasm")).unwrap();

        let gitignore = fs::read_to_string(project_dir.join(".gitignore")).unwrap();
        assert!(
            gitignore.contains("/pkg"),
            "expected /pkg in WASM .gitignore:\n{gitignore}",
        );
    }

    // Test: Invalid template does not create project directory
    #[test]
    fn test_init_project_invalid_template_no_directory_created() {
        let tmp = TempDir::new().unwrap();
        let _ = init_project("no-dir", tmp.path(), Some("nonexistent"));
        assert!(!tmp.path().join("no-dir").exists());
    }

    // Correctness scenario 1: CLI template structure
    #[test]
    fn test_correctness_cli_template_structure() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("my-cli", tmp.path(), Some("cli")).unwrap();

        let cargo = fs::read_to_string(project_dir.join("cargo.toml")).unwrap();
        assert!(cargo.contains("clap = { version = \"4\", features = [\"derive\"] }"));

        let source = fs::read_to_string(project_dir.join("src/index.rts")).unwrap();
        assert!(source.contains("function main()"));

        let gitignore = fs::read_to_string(project_dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains(".rsc-build"));
    }

    // Correctness scenario 2: Web server template structure
    #[test]
    fn test_correctness_web_server_template_structure() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("my-api", tmp.path(), Some("web-server")).unwrap();

        let cargo = fs::read_to_string(project_dir.join("cargo.toml")).unwrap();
        assert!(cargo.contains("axum"));
        assert!(cargo.contains("tokio"));
        assert!(cargo.contains("serde"));
        assert!(cargo.contains("serde_json"));

        let source = fs::read_to_string(project_dir.join("src/index.rts")).unwrap();
        assert!(source.contains("async function main()"));

        assert!(project_dir.join(".gitignore").is_file());
    }

    // Correctness scenario 3: Default template backward compatibility
    #[test]
    fn test_correctness_default_template_backward_compat() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("my-app", tmp.path(), None).unwrap();

        let cargo = fs::read_to_string(project_dir.join("cargo.toml")).unwrap();
        let expected_cargo =
            "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        assert_eq!(
            cargo, expected_cargo,
            "default cargo.toml should be identical to original behavior"
        );

        let source = fs::read_to_string(project_dir.join("src/index.rts")).unwrap();
        assert_eq!(
            source, HELLO_WORLD_SOURCE,
            "default index.rts should be identical to original hello world"
        );

        assert!(!project_dir.join(".gitignore").exists());
    }

    // ---------------------------------------------------------------
    // Task 050: WASM compilation target
    // ---------------------------------------------------------------

    // Test: parse_wasm_target recognizes wasm32-unknown-unknown
    #[test]
    fn test_parse_wasm_target_unknown_unknown() {
        let result = parse_wasm_target("wasm32-unknown-unknown");
        assert_eq!(result, Some(WasmTarget::Unknown));
    }

    // Test: parse_wasm_target recognizes wasm32-wasip1
    #[test]
    fn test_parse_wasm_target_wasip1() {
        let result = parse_wasm_target("wasm32-wasip1");
        assert_eq!(result, Some(WasmTarget::Wasip1));
    }

    // Test: parse_wasm_target returns None for non-WASM targets
    #[test]
    fn test_parse_wasm_target_native_returns_none() {
        assert!(parse_wasm_target("x86_64-unknown-linux-gnu").is_none());
        assert!(parse_wasm_target("aarch64-apple-darwin").is_none());
    }

    // Test: parse_wasm_target treats other wasm32-* as Unknown
    #[test]
    fn test_parse_wasm_target_other_wasm32_treated_as_unknown() {
        let result = parse_wasm_target("wasm32-wasi");
        assert!(result.is_some());
    }

    // Test: WasmTarget::triple returns correct strings
    #[test]
    fn test_wasm_target_triple_values() {
        assert_eq!(WasmTarget::Unknown.triple(), "wasm32-unknown-unknown");
        assert_eq!(WasmTarget::Wasip1.triple(), "wasm32-wasip1");
    }

    // Test: CargoTomlBuilder with library crate-type produces [lib] section
    #[test]
    fn test_cargo_toml_builder_library_cdylib() {
        let mut builder = CargoTomlBuilder::new("wasm-app", "2024");
        builder.set_library(vec!["cdylib".to_owned()]);
        let output = builder.build();

        assert!(
            output.contains("[lib]"),
            "expected [lib] section, got:\n{output}"
        );
        assert!(
            output.contains("crate-type = [\"cdylib\"]"),
            "expected crate-type cdylib, got:\n{output}"
        );
    }

    // Test: Default CargoTomlBuilder (binary) does NOT produce [lib] section
    #[test]
    fn test_cargo_toml_builder_binary_has_no_lib_section() {
        let builder = CargoTomlBuilder::new("my-app", "2024");
        let output = builder.build();

        assert!(
            !output.contains("[lib]"),
            "binary crate should not have [lib] section, got:\n{output}"
        );
    }

    // Test: WASM target excludes tokio dependency from Cargo.toml
    #[test]
    fn test_compile_for_target_wasm_excludes_tokio() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("async-wasm");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"async-wasm\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        // Write an async main — the compiler will set needs_async_runtime = true
        fs::write(
            src_dir.join("index.rts"),
            "async function main() {\n  console.log(\"hi\");\n}\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project
            .compile_for_target(Some(&WasmTarget::Unknown))
            .unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            !cargo_toml.contains("tokio"),
            "WASM Cargo.toml should NOT contain tokio, got:\n{cargo_toml}"
        );
    }

    // Test: WASM unknown-unknown target generates lib.rs instead of main.rs
    #[test]
    fn test_compile_for_target_wasm_unknown_generates_lib_rs() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("wasm-lib", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project
            .compile_for_target(Some(&WasmTarget::Unknown))
            .unwrap();

        let lib_rs = build_dir.join("src/lib.rs");
        let main_rs = build_dir.join("src/main.rs");

        assert!(
            lib_rs.is_file(),
            "expected lib.rs for wasm32-unknown-unknown"
        );
        assert!(
            !main_rs.is_file(),
            "main.rs should be removed for wasm32-unknown-unknown"
        );
    }

    // Test: WASM wasip1 target keeps main.rs (binary)
    #[test]
    fn test_compile_for_target_wasm_wasip1_keeps_main_rs() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("wasi-bin", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project
            .compile_for_target(Some(&WasmTarget::Wasip1))
            .unwrap();

        let main_rs = build_dir.join("src/main.rs");
        assert!(main_rs.is_file(), "expected main.rs for wasm32-wasip1");

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            !cargo_toml.contains("[lib]"),
            "wasip1 binary should not have [lib] section, got:\n{cargo_toml}"
        );
    }

    // Test: WASM unknown-unknown Cargo.toml has cdylib crate-type
    #[test]
    fn test_compile_for_target_wasm_unknown_cargo_toml_has_cdylib() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("wasm-cdylib", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project
            .compile_for_target(Some(&WasmTarget::Unknown))
            .unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("crate-type = [\"cdylib\"]"),
            "expected crate-type cdylib for wasm32-unknown-unknown, got:\n{cargo_toml}"
        );
    }

    // Test: compile_for_target with None produces same result as compile
    #[test]
    fn test_compile_for_target_none_matches_compile() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("no-target", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result1, _, _, _) = project.compile().unwrap();
        let (result2, _, _, _) = project.compile_for_target(None).unwrap();

        assert_eq!(result1.rust_source, result2.rust_source);
    }

    // Test: async function + WASM target sets needs_async_runtime flag
    #[test]
    fn test_compile_for_target_wasm_async_needs_runtime_flag() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("async-check");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"async-check\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            src_dir.join("index.rts"),
            "async function main() {\n  console.log(\"hello\");\n}\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, _, _, _) = project
            .compile_for_target(Some(&WasmTarget::Unknown))
            .unwrap();

        // The compile result should still indicate async was used
        // (the driver uses this to emit a warning)
        assert!(
            result.needs_async_runtime,
            "expected needs_async_runtime to be true for async function"
        );
    }

    // Test: run with WASM target returns WasmRunUnsupported
    #[test]
    fn test_run_wasm_target_returns_unsupported() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("run-wasm", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let err = project
            .run(&[], Some("wasm32-unknown-unknown"))
            .unwrap_err();
        assert!(
            matches!(err, DriverError::WasmRunUnsupported),
            "expected WasmRunUnsupported, got: {err:?}"
        );
    }

    // Test: run with wasm32-wasip1 target also returns WasmRunUnsupported
    #[test]
    fn test_run_wasm_wasip1_target_returns_unsupported() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("run-wasi", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let err = project.run(&[], Some("wasm32-wasip1")).unwrap_err();
        assert!(
            matches!(err, DriverError::WasmRunUnsupported),
            "expected WasmRunUnsupported, got: {err:?}"
        );
    }

    // ---------------------------------------------------------------
    // Task 070: Dependency management — rsc.toml integration
    // ---------------------------------------------------------------

    // Test: compile includes explicit deps from rsc.toml in generated Cargo.toml
    #[test]
    fn test_compile_includes_rsc_toml_dependencies() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("deps-test", tmp.path(), None).unwrap();

        // Add a dependency via rsc.toml
        crate::deps::add_dependency(&project_dir, "serde", Some("1"), &[], false).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, build_dir, _, _) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("serde"),
            "expected serde in generated Cargo.toml, got:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("\"1\""),
            "expected version \"1\" in generated Cargo.toml, got:\n{cargo_toml}"
        );
    }

    // Test: compile includes deps with features from rsc.toml
    #[test]
    fn test_compile_includes_rsc_toml_deps_with_features() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("deps-features", tmp.path(), None).unwrap();

        let features = vec!["derive".to_owned()];
        crate::deps::add_dependency(&project_dir, "serde", Some("1"), &features, false).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("serde"),
            "expected serde in generated Cargo.toml, got:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("derive"),
            "expected features in generated Cargo.toml, got:\n{cargo_toml}"
        );
    }

    // Test: explicit rsc.toml deps override auto-detected wildcard versions
    #[test]
    fn test_compile_rsc_toml_overrides_autodetected_deps() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("override-test");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("cargo.toml"),
            "[package]\nname = \"override-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        // Write source that imports rand (will be auto-detected as "*")
        fs::write(
            src_dir.join("index.rts"),
            "import { thread_rng } from \"rand\";\n\nfunction main() {\n  console.log(\"hi\");\n}\n",
        )
        .unwrap();

        // Add explicit version via rsc.toml
        crate::deps::add_dependency(&project_dir, "rand", Some("0.8"), &[], false).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, build_dir, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(build_dir.join("Cargo.toml")).unwrap();
        // The explicit version "0.8" should override the auto-detected "*"
        assert!(
            cargo_toml.contains("\"0.8\""),
            "expected version \"0.8\" to override wildcard, got:\n{cargo_toml}"
        );
    }

    // --- Color output tests ---
    #[test]
    fn test_format_status_never_produces_plain_text() {
        let result = format_status(
            "Compiling",
            "myproject v0.1.0",
            StatusStyle::Success,
            ColorMode::Never,
        );
        assert_eq!(result, "   Compiling myproject v0.1.0");
        assert!(!result.contains("\x1b["));
    }

    #[test]
    fn test_format_status_always_produces_ansi_green_for_success() {
        let result = format_status(
            "Compiling",
            "myproject v0.1.0",
            StatusStyle::Success,
            ColorMode::Always,
        );
        assert!(result.contains("\x1b[1;32m"), "should contain bold green");
        assert!(result.contains("\x1b[0m"), "should contain reset");
        assert!(result.contains("Compiling"), "should contain the label");
        assert!(
            result.contains("myproject v0.1.0"),
            "should contain the message"
        );
    }

    #[test]
    fn test_format_status_always_produces_ansi_red_for_error() {
        let result = format_status(
            "error:",
            "something broke",
            StatusStyle::Error,
            ColorMode::Always,
        );
        assert!(result.contains("\x1b[1;31m"), "should contain bold red");
    }

    #[test]
    fn test_format_status_always_produces_ansi_yellow_for_warning() {
        let result = format_status(
            "warning:",
            "something off",
            StatusStyle::Warning,
            ColorMode::Always,
        );
        assert!(result.contains("\x1b[1;33m"), "should contain bold yellow");
    }

    #[test]
    fn test_project_open_defaults_to_no_color() {
        let tmp = TempDir::new().unwrap();
        let _project_dir = init_project("color-test", tmp.path(), None).unwrap();

        let project = Project::open(&tmp.path().join("color-test")).unwrap();
        assert_eq!(
            project.color_mode,
            ColorMode::Never,
            "default should be Never"
        );
    }

    #[test]
    fn test_load_compile_options_with_rustdoc_no_docs_returns_empty_sigs() {
        let tmp = TempDir::new().unwrap();
        let build_dir = tmp.path().join(".rsc-build");
        fs::create_dir_all(&build_dir).unwrap();

        let project = Project {
            root: tmp.path().to_path_buf(),
            name: "test".to_owned(),
            compile_options: crate::pipeline::CompileOptions::default(),
            color_mode: ColorMode::Never,
        };

        let options = project.load_compile_options_with_rustdoc(&build_dir);
        assert!(
            options.external_signatures.is_empty(),
            "no doc dir should yield empty external signatures"
        );
    }

    #[test]
    fn test_load_compile_options_with_rustdoc_empty_doc_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let build_dir = tmp.path().join(".rsc-build");
        let doc_dir = build_dir.join("target").join("doc");
        fs::create_dir_all(&doc_dir).unwrap();

        let project = Project {
            root: tmp.path().to_path_buf(),
            name: "test".to_owned(),
            compile_options: crate::pipeline::CompileOptions::default(),
            color_mode: ColorMode::Never,
        };

        let options = project.load_compile_options_with_rustdoc(&build_dir);
        assert!(
            options.external_signatures.is_empty(),
            "empty doc dir should yield empty external signatures"
        );
    }

    #[test]
    fn test_maybe_generate_rustdoc_skips_when_json_exists() {
        let tmp = TempDir::new().unwrap();
        let build_dir = tmp.path().join(".rsc-build");
        let doc_dir = build_dir.join("target").join("doc");
        fs::create_dir_all(&doc_dir).unwrap();

        // Write a dummy .json file to simulate cached docs
        fs::write(doc_dir.join("serde.json"), "{}").unwrap();

        // This should return early without calling cargo doc.
        // If it didn't skip, it would try to run cargo doc and potentially
        // do something noisy, but since we just check the caching logic
        // we verify it doesn't panic and returns cleanly.
        Project::maybe_generate_rustdoc(&build_dir);
    }
}
