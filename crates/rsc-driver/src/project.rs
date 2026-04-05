//! `RustScript` project management — initialization, discovery, build, and run.
//!
//! Handles the on-disk project structure: locating source files, generating
//! `Cargo.toml` via merge strategy, and invoking Cargo for compilation and execution.
//! Projects compile in-place — there is no `.rsc-build/` copy step.

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
use crate::manifest::{self, DepSpec, MANIFEST_FILE, Manifest};
use crate::pipeline::CompileResult;
use crate::rustdoc_cache;
use crate::rustdoc_convert;

/// Default hello-world source for new projects.
const HELLO_WORLD_SOURCE: &str = r#"function main() {
  console.log("Hello, World!");
}
"#;

/// Default `.gitignore` content for new projects.
const DEFAULT_GITIGNORE: &str = "/target\n/src/*.rs\n";

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

/// Structured builder for generating `Cargo.toml` content from scratch.
///
/// Uses `BTreeMap` for deterministic alphabetical ordering of dependencies.
struct CargoTomlBuilder {
    name: String,
    edition: String,
    version: String,
    dependencies: BTreeMap<String, DependencySpec>,
    crate_kind: CrateKind,
}

impl CargoTomlBuilder {
    /// Create a new builder with the given project name, edition, and version.
    fn new(name: &str, edition: &str, version: &str) -> Self {
        Self {
            name: name.to_owned(),
            edition: edition.to_owned(),
            version: version.to_owned(),
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
        let _ = writeln!(out, "version = \"{}\"", self.version);
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

/// Merge auto-detected and explicit dependencies into an existing `Cargo.toml`.
///
/// Reads the existing TOML, adds new dependencies without overwriting existing
/// entries (except explicit `rustscript.json` deps which always win), ensures
/// `[workspace]` exists, and writes back.
///
/// # Errors
///
/// Returns a `DriverError` if the existing `Cargo.toml` cannot be parsed.
fn merge_cargo_toml(
    cargo_toml_path: &Path,
    auto_deps: &BTreeMap<String, DependencySpec>,
    explicit_deps: &BTreeMap<String, DepSpec>,
) -> Result<()> {
    let content = fs::read_to_string(cargo_toml_path)?;
    let mut doc: toml::Table = content
        .parse()
        .map_err(|e: toml::de::Error| DriverError::ManifestParseFailed(e.to_string()))?;

    // Ensure [dependencies] section exists
    if !doc.contains_key("dependencies") {
        doc.insert(
            "dependencies".to_owned(),
            toml::Value::Table(toml::Table::new()),
        );
    }

    let deps_table = doc
        .get_mut("dependencies")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| {
            DriverError::ManifestParseFailed("[dependencies] is not a table".to_owned())
        })?;

    // Add auto-detected deps only if NOT already present
    for (name, spec) in auto_deps {
        if !deps_table.contains_key(name) {
            deps_table.insert(name.clone(), dep_spec_to_toml_value(spec));
        }
    }

    // Explicit deps from rustscript.json ALWAYS win (override existing)
    for (name, spec) in explicit_deps {
        let value = manifest_dep_to_toml_value(spec);
        deps_table.insert(name.clone(), value);
    }

    // Remove empty [dependencies] if no deps
    if deps_table.is_empty() {
        doc.remove("dependencies");
    }

    // Ensure [workspace] exists
    if !doc.contains_key("workspace") {
        doc.insert(
            "workspace".to_owned(),
            toml::Value::Table(toml::Table::new()),
        );
    }

    let output = toml::to_string_pretty(&doc)
        .map_err(|e| DriverError::ManifestParseFailed(e.to_string()))?;
    fs::write(cargo_toml_path, output)?;

    Ok(())
}

/// Convert a `DependencySpec` to a TOML value.
fn dep_spec_to_toml_value(spec: &DependencySpec) -> toml::Value {
    match spec {
        DependencySpec::Simple(version) => toml::Value::String(version.clone()),
        DependencySpec::Detailed { version, features } => {
            let mut table = toml::Table::new();
            table.insert("version".to_owned(), toml::Value::String(version.clone()));
            table.insert(
                "features".to_owned(),
                toml::Value::Array(
                    features
                        .iter()
                        .map(|f| toml::Value::String(f.clone()))
                        .collect(),
                ),
            );
            toml::Value::Table(table)
        }
    }
}

/// Convert a manifest `DepSpec` to a TOML value.
fn manifest_dep_to_toml_value(spec: &DepSpec) -> toml::Value {
    match spec {
        DepSpec::Simple(version) => toml::Value::String(version.clone()),
        DepSpec::Detailed(detail) => {
            let mut table = toml::Table::new();
            table.insert(
                "version".to_owned(),
                toml::Value::String(detail.version.clone()),
            );
            if !detail.features.is_empty() {
                table.insert(
                    "features".to_owned(),
                    toml::Value::Array(
                        detail
                            .features
                            .iter()
                            .map(|f| toml::Value::String(f.clone()))
                            .collect(),
                    ),
                );
            }
            toml::Value::Table(table)
        }
    }
}

/// A `RustScript` project rooted at a directory.
#[derive(Debug)]
pub struct Project {
    /// Project root directory.
    pub root: PathBuf,
    /// Project name (from `rustscript.json` or directory name).
    pub name: String,
    /// Compilation options (e.g., `--no-borrow-inference`).
    pub compile_options: crate::pipeline::CompileOptions,
    /// Color mode for diagnostic rendering.
    pub color_mode: ColorMode,
}

impl Project {
    /// Open an existing project from a directory.
    ///
    /// Walks up from `dir` looking for a directory containing `rustscript.json`
    /// (or a `src/` directory with `.rts` files).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::ProjectNotFound`] if no project root is found.
    pub fn open(dir: &Path) -> Result<Self> {
        let start_dir = dir.to_path_buf();
        let mut current = dir.to_path_buf();

        loop {
            // Check for rustscript.json (preferred manifest)
            if current.join(MANIFEST_FILE).is_file() {
                let name = manifest::try_read_manifest(&current)
                    .ok()
                    .flatten()
                    .map_or_else(|| project_name_from_dir(&current), |m| m.name);
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
    /// Looks for `src/main.rts` only. The legacy `index.rts` entry point is
    /// not recognized.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::MainSourceNotFound`] if `src/main.rts` does not exist.
    pub fn main_source(&self) -> Result<PathBuf> {
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

    /// Eject the project: convert from a `RustScript` project to a pure Rust project.
    ///
    /// This removes `rustscript.json` and updates `.gitignore` to un-ignore
    /// generated `.rs` files. After ejecting, the generated `.rs` files become
    /// the source of truth and the `rsc` compiler will no longer recognize
    /// this directory as a `RustScript` project.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::EjectNotBuilt`] if `src/main.rs` does not exist
    /// (the project must be built at least once before ejecting).
    /// Returns [`DriverError::Io`] if file operations fail.
    pub fn eject(&self) -> Result<()> {
        // Ensure the project has been built
        let main_rs = self.root.join("src/main.rs");
        if !main_rs.is_file() {
            return Err(DriverError::EjectNotBuilt);
        }

        // Remove rustscript.json
        let manifest_path = self.root.join(MANIFEST_FILE);
        if manifest_path.is_file() {
            fs::remove_file(&manifest_path)?;
        }

        // Update .gitignore: remove the /src/*.rs line
        let gitignore_path = self.root.join(".gitignore");
        if gitignore_path.is_file() {
            let content = fs::read_to_string(&gitignore_path)?;
            let updated = remove_gitignore_rs_line(&content);
            fs::write(&gitignore_path, updated)?;
        }

        Ok(())
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

    /// Read the project manifest, or return a default with the project name.
    fn manifest_or_default(&self) -> Manifest {
        manifest::try_read_manifest(&self.root)
            .ok()
            .flatten()
            .unwrap_or_else(|| manifest::new_manifest(&self.name))
    }

    /// Compile the project: read `.rts` source, run pipeline, write `.rs` output in-place.
    ///
    /// Discovers all `.rts` files in `src/`, compiles each independently, generates
    /// `mod` declarations for the main file, and writes all output to `src/`.
    /// Generates or merges `Cargo.toml` in the project root.
    ///
    /// Returns the [`CompileResult`] for the main file, the project root path,
    /// the original `.rts` source text, and the `.rts` display filename.
    ///
    /// # Errors
    ///
    /// Returns an error if the main source file cannot be found or read, or if
    /// the output directory cannot be created.
    #[allow(clippy::too_many_lines)]
    // Multi-file project compilation orchestrates many steps; splitting would obscure the flow
    pub fn compile(&self) -> Result<(CompileResult, PathBuf, String, String)> {
        let source_path = self.main_source()?;
        let source = fs::read_to_string(&source_path)?;
        let module_files = self.discover_modules()?;

        let src_dir = self.root.join("src");
        fs::create_dir_all(&src_dir)?;

        let manifest_data = self.manifest_or_default();

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

            // Derive module name from filename: "utils.rts" -> "utils"
            let module_name = module_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_owned();

            // Write the module's .rs file in-place
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

        // Build auto-detected dependencies map
        let mut auto_deps = BTreeMap::new();

        // Add tokio if async runtime is needed
        if any_needs_async_runtime {
            auto_deps.insert(
                "tokio".to_owned(),
                DependencySpec::Detailed {
                    version: "1".to_owned(),
                    features: vec!["full".to_owned()],
                },
            );
        }

        if any_needs_futures_crate {
            auto_deps.insert(
                "futures".to_owned(),
                DependencySpec::Simple("0.3".to_owned()),
            );
        }

        if any_needs_serde_json {
            auto_deps.insert(
                "serde_json".to_owned(),
                DependencySpec::Simple("1".to_owned()),
            );
        }

        if any_needs_rand {
            auto_deps.insert("rand".to_owned(), DependencySpec::Simple("0.8".to_owned()));
        }

        if any_needs_regex {
            auto_deps.insert("regex".to_owned(), DependencySpec::Simple("1".to_owned()));
        }

        if any_needs_serde {
            auto_deps.insert(
                "serde".to_owned(),
                DependencySpec::Detailed {
                    version: "1".to_owned(),
                    features: vec!["derive".to_owned()],
                },
            );
        }

        // Add all external crate dependencies
        for dep in &all_crate_deps {
            auto_deps
                .entry(dep.name.clone())
                .or_insert_with(|| DependencySpec::Simple("*".to_owned()));
        }

        // Collect explicit dependencies from rustscript.json
        let explicit_deps = &manifest_data.dependencies;

        let cargo_toml_path = self.root.join("Cargo.toml");

        if cargo_toml_path.is_file() {
            // Merge strategy: read existing, add new, preserve user edits
            merge_cargo_toml(&cargo_toml_path, &auto_deps, explicit_deps)?;
        } else {
            // Generate from scratch
            let mut cargo_builder = CargoTomlBuilder::new(
                &manifest_data.name,
                &manifest_data.edition,
                &manifest_data.version,
            );

            // Add auto-detected deps
            for (name, spec) in &auto_deps {
                cargo_builder.add_dependency(name, spec.clone());
            }

            // Explicit deps from rustscript.json override auto-detected
            for (name, spec) in explicit_deps {
                let dep_spec = match spec {
                    DepSpec::Simple(v) => DependencySpec::Simple(v.clone()),
                    DepSpec::Detailed(d) => DependencySpec::Detailed {
                        version: d.version.clone(),
                        features: d.features.clone(),
                    },
                };
                cargo_builder.dependencies.insert(name.clone(), dep_spec);
            }

            // If tokio was also imported explicitly, the runtime spec takes priority
            if any_needs_async_runtime {
                cargo_builder.add_tokio_runtime();
            }

            fs::write(&cargo_toml_path, cargo_builder.build())?;
        }

        // Write src/main.rs (always overwrite)
        fs::write(src_dir.join("main.rs"), &result.rust_source)?;

        // Generate rustdoc JSON for dependencies and re-compile with external
        // signatures. This enables proper param types, throws detection, and
        // async handling for external crate functions.
        if !all_crate_deps.is_empty() {
            Self::maybe_generate_rustdoc(&self.root);
            let enriched_options = self.load_compile_options_with_rustdoc(&self.root);
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
        Ok((result, self.root.clone(), source, rts_filename))
    }

    /// Build `CompileOptions` with external function signatures loaded from
    /// cached rustdoc JSON (if available from a previous compilation).
    fn load_compile_options_with_rustdoc(
        &self,
        project_dir: &Path,
    ) -> crate::pipeline::CompileOptions {
        let mut options = crate::pipeline::CompileOptions {
            no_borrow_inference: self.compile_options.no_borrow_inference,
            ..crate::pipeline::CompileOptions::default()
        };

        // Check if any cached rustdoc JSON exists from a prior build.
        let doc_dir = project_dir.join("target").join("doc");
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
                && let Some(crate_data) = cache.get_crate_docs(&name, project_dir)
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
    fn maybe_generate_rustdoc(project_dir: &Path) {
        // Simple caching: skip if the doc directory already has .json files.
        let doc_dir = project_dir.join("target").join("doc");
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
        let _ = rustdoc_cache::generate_rustdoc_json(project_dir);
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
        let (result, project_dir, source, rts_filename) = self.compile()?;

        let Some(wt) = wasm_target else {
            return Ok((result, project_dir, source, rts_filename));
        };

        let manifest_data = self.manifest_or_default();

        // Rebuild the Cargo.toml with WASM-specific settings
        let src_dir = project_dir.join("src");
        let mut cargo_builder = CargoTomlBuilder::new(
            &manifest_data.name,
            &manifest_data.edition,
            &manifest_data.version,
        );

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

        // Add explicit dependencies from rustscript.json — but NOT tokio for WASM
        for (name, spec) in &manifest_data.dependencies {
            if name == "tokio" {
                continue;
            }
            let dep_spec = match spec {
                DepSpec::Simple(v) => DependencySpec::Simple(v.clone()),
                DepSpec::Detailed(d) => DependencySpec::Detailed {
                    version: d.version.clone(),
                    features: d.features.clone(),
                },
            };
            cargo_builder.dependencies.insert(name.clone(), dep_spec);
        }

        // Do NOT add tokio for WASM targets — it is not supported

        fs::write(project_dir.join("Cargo.toml"), cargo_builder.build())?;

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

        Ok((result, project_dir, source, rts_filename))
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
        let (result, project_dir, rts_source, rts_filename) =
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
            .current_dir(&project_dir);
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
            let artifact_path = project_dir
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

        let (result, project_dir, rts_source, rts_filename) = self.compile()?;

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
            .current_dir(&project_dir);

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

    /// Run tests: compile the project, then invoke `cargo test` in the project directory.
    ///
    /// Forwards `args` to `cargo test` (after `--` separator). If `release` is true,
    /// passes `--release` to cargo.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::CompilationFailed`] if `RustScript` compilation fails,
    /// or [`DriverError::CargoBuildFailed`] if `cargo test` fails.
    pub fn test(&self, release: bool, args: &[String]) -> Result<std::process::ExitStatus> {
        let (result, project_dir, rts_source, rts_filename) = self.compile()?;

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
            .current_dir(&project_dir);

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
///     main.rts
///   rustscript.json
///   Cargo.toml
///   .gitignore
/// ```
///
/// When `template` is `None`, creates a bare hello-world project.
/// When a template name is provided, scaffolds the project with
/// pre-configured dependencies and starter code.
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
        let manifest_data = (tmpl.build_manifest)(name);
        manifest::write_manifest(&project_dir, &manifest_data)?;

        // Generate initial Cargo.toml from the manifest
        let mut cargo_builder =
            CargoTomlBuilder::new(name, &manifest_data.edition, &manifest_data.version);
        for (dep_name, spec) in &manifest_data.dependencies {
            let dep_spec = match spec {
                DepSpec::Simple(v) => DependencySpec::Simple(v.clone()),
                DepSpec::Detailed(d) => DependencySpec::Detailed {
                    version: d.version.clone(),
                    features: d.features.clone(),
                },
            };
            cargo_builder
                .dependencies
                .insert(dep_name.clone(), dep_spec);
        }
        fs::write(project_dir.join("Cargo.toml"), cargo_builder.build())?;

        fs::write(src_dir.join("main.rts"), tmpl.main_rts)?;
        if let Some(gitignore) = tmpl.gitignore {
            fs::write(project_dir.join(".gitignore"), gitignore)?;
        }
    } else {
        // Default (bare) project
        let manifest_data = manifest::new_manifest(name);
        manifest::write_manifest(&project_dir, &manifest_data)?;

        let cargo_toml = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n"
        );
        fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;

        fs::write(src_dir.join("main.rts"), HELLO_WORLD_SOURCE)?;
        fs::write(project_dir.join(".gitignore"), DEFAULT_GITIGNORE)?;
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

/// Remove the `/src/*.rs` line from `.gitignore` content.
///
/// Preserves all other lines and avoids introducing trailing whitespace or
/// duplicate blank lines. Returns the updated content.
fn remove_gitignore_rs_line(content: &str) -> String {
    content
        .lines()
        .filter(|line| line.trim() != "/src/*.rs")
        .collect::<Vec<_>>()
        .join("\n")
        + if content.ends_with('\n') { "\n" } else { "" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // --- Init tests ---

    #[test]
    fn test_init_project_creates_directory_structure() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        assert!(project_dir.join("src").is_dir());
        assert!(project_dir.join("src/main.rts").is_file());
        assert!(project_dir.join("rustscript.json").is_file());
        assert!(project_dir.join("Cargo.toml").is_file());
        assert!(project_dir.join(".gitignore").is_file());
    }

    #[test]
    fn test_init_project_rustscript_json_has_correct_name() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let manifest_data = manifest::read_manifest(&project_dir).unwrap();
        assert_eq!(manifest_data.name, "test-app");
        assert_eq!(manifest_data.version, "0.1.0");
        assert_eq!(manifest_data.edition, "2024");
    }

    #[test]
    fn test_init_project_cargo_toml_has_correct_name() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let content = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            content.contains("name = \"test-app\""),
            "Cargo.toml should contain project name, got:\n{content}"
        );
        assert!(
            content.contains("edition = \"2024\""),
            "Cargo.toml should specify edition 2024, got:\n{content}"
        );
        assert!(
            content.contains("[workspace]"),
            "Cargo.toml should have [workspace] section, got:\n{content}"
        );
    }

    #[test]
    fn test_init_project_main_rts_has_hello_world() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let content = fs::read_to_string(project_dir.join("src/main.rts")).unwrap();
        assert!(
            content.contains("console.log"),
            "main.rts should contain console.log, got:\n{content}"
        );
        assert!(
            content.contains("Hello, World!"),
            "main.rts should contain Hello, World!, got:\n{content}"
        );
    }

    #[test]
    fn test_init_project_gitignore_content() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let gitignore = fs::read_to_string(project_dir.join(".gitignore")).unwrap();
        assert!(
            gitignore.contains("/target"),
            "gitignore should contain /target"
        );
        assert!(
            gitignore.contains("/src/*.rs"),
            "gitignore should contain /src/*.rs"
        );
        assert!(
            !gitignore.contains(".rsc-build"),
            "gitignore should NOT contain .rsc-build"
        );
    }

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

    #[test]
    fn test_init_project_does_not_create_index_rts() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        assert!(
            !project_dir.join("src/index.rts").exists(),
            "index.rts should NOT be created"
        );
    }

    // --- Project::open tests ---

    #[test]
    fn test_project_open_finds_project_in_directory() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        assert_eq!(project.name, "test-app");
        assert_eq!(project.root, project_dir);
    }

    #[test]
    fn test_project_open_reads_name_from_manifest() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("manifest-name", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        assert_eq!(project.name, "manifest-name");
    }

    #[test]
    fn test_project_open_walks_up_directories() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("walkup", tmp.path(), None).unwrap();

        // Open from a subdirectory
        let sub_dir = project_dir.join("src");
        let project = Project::open(&sub_dir).unwrap();
        assert_eq!(project.root, project_dir);
    }

    #[test]
    fn test_project_open_returns_error_when_not_found() {
        let tmp = TempDir::new().unwrap();
        let err = Project::open(tmp.path()).unwrap_err();
        assert!(
            matches!(err, DriverError::ProjectNotFound(_)),
            "expected ProjectNotFound, got: {err:?}"
        );
    }

    #[test]
    fn test_project_open_finds_src_dir_project() {
        // Project with src/ and .rts files but no rustscript.json
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("bare-project");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("main.rts"), "function main() {}\n").unwrap();

        let project = Project::open(&project_dir).unwrap();
        assert_eq!(project.root, project_dir);
    }

    // --- main_source tests ---

    #[test]
    fn test_project_main_source_returns_main_rts() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let main = project.main_source().unwrap();
        assert_eq!(main, project_dir.join("src/main.rts"));
    }

    #[test]
    fn test_project_main_source_ignores_index_rts() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("no-main");
        fs::create_dir_all(project_dir.join("src")).unwrap();
        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "no-main"}"#,
        )
        .unwrap();
        // Only index.rts exists, no main.rts
        fs::write(project_dir.join("src/index.rts"), "function main() {}\n").unwrap();

        let project = Project::open(&project_dir).unwrap();
        let err = project.main_source().unwrap_err();
        assert!(
            matches!(err, DriverError::MainSourceNotFound),
            "expected MainSourceNotFound when only index.rts exists, got: {err:?}"
        );
    }

    #[test]
    fn test_project_main_source_returns_error_when_missing() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("no-source");
        fs::create_dir_all(project_dir.join("src")).unwrap();
        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "no-source"}"#,
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let err = project.main_source().unwrap_err();
        assert!(
            matches!(err, DriverError::MainSourceNotFound),
            "expected MainSourceNotFound, got: {err:?}"
        );
    }

    // --- compile tests ---

    #[test]
    fn test_project_compile_writes_main_rs_in_place() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, returned_dir, _, _) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        // compile() returns the project root, not .rsc-build
        assert_eq!(returned_dir, project_dir);

        // main.rs should be written directly in src/
        let main_rs = project_dir.join("src/main.rs");
        assert!(main_rs.is_file(), "expected src/main.rs in project dir");

        let content = fs::read_to_string(main_rs).unwrap();
        assert!(
            content.contains("fn main()"),
            "expected fn main in generated Rust, got:\n{content}"
        );
    }

    #[test]
    fn test_project_compile_generates_cargo_toml_in_place() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("name = \"test-app\""),
            "expected project name in Cargo.toml, got:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("[workspace]"),
            "expected [workspace] section in Cargo.toml, got:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_project_compile_no_rsc_build_directory() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("no-build-dir", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        assert!(
            !project_dir.join(".rsc-build").exists(),
            ".rsc-build should not exist with in-place compilation"
        );
    }

    // --- Cargo.toml merge strategy tests ---

    #[test]
    fn test_cargo_toml_merge_preserves_user_edits() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("merge-test", tmp.path(), None).unwrap();

        // Simulate user manually editing Cargo.toml to add a [profile] section
        let cargo_path = project_dir.join("Cargo.toml");
        let mut content = fs::read_to_string(&cargo_path).unwrap();
        content.push_str("\n[profile.release]\nopt-level = 3\nlto = true\n");
        fs::write(&cargo_path, &content).unwrap();

        // Compile — should merge, not overwrite
        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let new_content = fs::read_to_string(&cargo_path).unwrap();
        assert!(
            new_content.contains("opt-level"),
            "user's [profile.release] should be preserved, got:\n{new_content}"
        );
        assert!(
            new_content.contains("lto"),
            "user's lto setting should be preserved, got:\n{new_content}"
        );
    }

    #[test]
    fn test_cargo_toml_merge_adds_new_deps() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("merge-add");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // Write rustscript.json and initial Cargo.toml
        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "merge-add"}"#,
        )
        .unwrap();
        fs::write(
            project_dir.join("Cargo.toml"),
            "[package]\nname = \"merge-add\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .unwrap();

        // Source that imports reqwest (auto-detected dep)
        fs::write(
            src_dir.join("main.rts"),
            "import { get } from \"reqwest\";\nfunction main() { console.log(\"hi\"); }\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("reqwest"),
            "auto-detected dep should be added, got:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_cargo_toml_merge_preserves_existing_deps() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("merge-keep");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "merge-keep"}"#,
        )
        .unwrap();
        // Cargo.toml with a user-pinned dependency version
        fs::write(
            project_dir.join("Cargo.toml"),
            "[package]\nname = \"merge-keep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nreqwest = \"0.11\"\n\n[workspace]\n",
        )
        .unwrap();

        // Source also imports reqwest — auto-detect would add it as "*"
        fs::write(
            src_dir.join("main.rts"),
            "import { get } from \"reqwest\";\nfunction main() { console.log(\"hi\"); }\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        // User's pinned version should be preserved (not overwritten with "*")
        assert!(
            cargo_toml.contains("\"0.11\""),
            "user's pinned version should be preserved, got:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_cargo_toml_merge_explicit_deps_override() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("merge-override");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // rustscript.json with explicit dep
        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "merge-override", "dependencies": {"rand": "0.8"}}"#,
        )
        .unwrap();
        fs::write(
            project_dir.join("Cargo.toml"),
            "[package]\nname = \"merge-override\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nrand = \"0.7\"\n\n[workspace]\n",
        )
        .unwrap();

        fs::write(
            src_dir.join("main.rts"),
            "function main() { console.log(\"hi\"); }\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        // Explicit dep from rustscript.json should override the existing version
        assert!(
            cargo_toml.contains("\"0.8\""),
            "explicit rustscript.json dep should override, got:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_cargo_toml_merge_ensures_workspace() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("merge-ws");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "merge-ws"}"#,
        )
        .unwrap();
        // Cargo.toml WITHOUT [workspace]
        fs::write(
            project_dir.join("Cargo.toml"),
            "[package]\nname = \"merge-ws\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        fs::write(
            src_dir.join("main.rts"),
            "function main() { console.log(\"hi\"); }\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("[workspace]"),
            "merge should add [workspace] if missing, got:\n{cargo_toml}"
        );
    }

    // --- Multi-file compilation tests ---

    #[test]
    fn test_project_compile_two_file_project() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("multi-file");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "multi-file"}"#,
        )
        .unwrap();
        fs::write(
            project_dir.join("Cargo.toml"),
            "[package]\nname = \"multi-file\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .unwrap();

        fs::write(
            src_dir.join("main.rts"),
            "import { greet } from \"./utils\";\n\nfunction main() {\n  greet(\"World\");\n}\n",
        )
        .unwrap();

        fs::write(
            src_dir.join("utils.rts"),
            "export function greet(name: string): void {\n  console.log(name);\n}\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, returned_dir, _, _) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        // Check main.rs was generated in-place
        let main_rs = fs::read_to_string(returned_dir.join("src/main.rs")).unwrap();
        assert!(
            main_rs.contains("mod utils;"),
            "expected `mod utils;` in main.rs:\n{main_rs}"
        );

        // Check utils.rs was generated in-place
        let utils_rs = fs::read_to_string(returned_dir.join("src/utils.rs")).unwrap();
        assert!(
            utils_rs.contains("pub fn greet(name: &str)"),
            "expected `pub fn greet(name: &str)` in utils.rs:\n{utils_rs}"
        );
    }

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

    // --- External crate dependency tests ---

    #[test]
    fn test_project_compile_external_crate_cargo_toml_has_dependency() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("crate-dep");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "crate-dep"}"#,
        )
        .unwrap();

        fs::write(
            src_dir.join("main.rts"),
            "import { get } from \"reqwest\";\nfunction main() { console.log(\"hi\"); }\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("[dependencies]"),
            "expected [dependencies] section in Cargo.toml:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("reqwest"),
            "expected reqwest in Cargo.toml:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_project_compile_local_imports_no_new_dependencies() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("local-only", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            !cargo_toml.contains("[dependencies]"),
            "expected no [dependencies] section for local-only project:\n{cargo_toml}"
        );
    }

    // --- Async runtime tests ---

    #[test]
    fn test_project_compile_async_main_cargo_toml_has_tokio() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("async-app");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "async-app"}"#,
        )
        .unwrap();

        fs::write(
            src_dir.join("main.rts"),
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
        let (result, _, _, _) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("tokio"),
            "expected tokio dependency in Cargo.toml:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_project_compile_non_async_cargo_toml_no_tokio() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("sync-app", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            !cargo_toml.contains("tokio"),
            "expected no tokio in Cargo.toml for non-async project:\n{cargo_toml}"
        );
    }

    // --- rustscript.json integration tests ---

    #[test]
    fn test_compile_includes_rustscript_json_dependencies() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("deps-test", tmp.path(), None).unwrap();

        // Add a dependency via rustscript.json
        crate::deps::add_dependency(&project_dir, "serde", Some("1"), &[], false).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (result, _, _, _) = project.compile().unwrap();

        assert!(
            !result.has_errors,
            "expected no errors, got: {:?}",
            result.diagnostics
        );

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("serde"),
            "expected serde in generated Cargo.toml, got:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_compile_includes_deps_with_features_from_rustscript_json() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("deps-features", tmp.path(), None).unwrap();

        let features = vec!["derive".to_owned()];
        crate::deps::add_dependency(&project_dir, "serde", Some("1"), &features, false).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("serde"),
            "expected serde in generated Cargo.toml, got:\n{cargo_toml}"
        );
        assert!(
            cargo_toml.contains("derive"),
            "expected features in generated Cargo.toml, got:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_compile_rustscript_json_overrides_autodetected_deps() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("override-test");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "override-test"}"#,
        )
        .unwrap();
        fs::write(
            project_dir.join("Cargo.toml"),
            "[package]\nname = \"override-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .unwrap();

        // Write source that imports rand (will be auto-detected as "*")
        fs::write(
            src_dir.join("main.rts"),
            "import { thread_rng } from \"rand\";\n\nfunction main() {\n  console.log(\"hi\");\n}\n",
        )
        .unwrap();

        // Add explicit version via rustscript.json
        crate::deps::add_dependency(&project_dir, "rand", Some("0.8"), &[], false).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, _, _, _) = project.compile().unwrap();

        let cargo_toml = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        // The explicit version "0.8" should override the auto-detected "*"
        assert!(
            cargo_toml.contains("\"0.8\""),
            "expected version \"0.8\" to override wildcard, got:\n{cargo_toml}"
        );
    }

    // --- Template tests ---

    #[test]
    fn test_init_project_cli_template_has_clap() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-cli", tmp.path(), Some("cli")).unwrap();

        let cargo = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains("clap"),
            "expected clap dependency in Cargo.toml:\n{cargo}",
        );
        assert!(cargo.contains("name = \"test-cli\""));

        let manifest_data = manifest::read_manifest(&project_dir).unwrap();
        assert!(manifest_data.dependencies.contains_key("clap"));
    }

    #[test]
    fn test_init_project_web_server_template_has_deps() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-web", tmp.path(), Some("web-server")).unwrap();

        let cargo = fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains("axum"),
            "expected axum in Cargo.toml:\n{cargo}"
        );
        assert!(
            cargo.contains("tokio"),
            "expected tokio in Cargo.toml:\n{cargo}"
        );
        assert!(
            cargo.contains("serde"),
            "expected serde in Cargo.toml:\n{cargo}"
        );
    }

    #[test]
    fn test_init_project_wasm_template_has_lib_and_wasm_bindgen() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("test-wasm", tmp.path(), Some("wasm")).unwrap();

        let manifest_data = manifest::read_manifest(&project_dir).unwrap();
        assert!(manifest_data.dependencies.contains_key("wasm-bindgen"));
    }

    #[test]
    fn test_init_project_invalid_template_returns_error() {
        let tmp = TempDir::new().unwrap();
        let err = init_project("test-bad", tmp.path(), Some("invalid")).unwrap_err();
        assert!(
            matches!(err, DriverError::InvalidTemplate(ref t) if t == "invalid"),
            "expected InvalidTemplate, got: {err:?}",
        );
    }

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

    #[test]
    fn test_init_project_default_creates_gitignore() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("t-default", tmp.path(), None).unwrap();
        assert!(project_dir.join(".gitignore").is_file());
    }

    #[test]
    fn test_init_project_templates_use_main_rts() {
        let tmp = TempDir::new().unwrap();

        let cli_dir = init_project("u-cli", tmp.path(), Some("cli")).unwrap();
        assert!(cli_dir.join("src/main.rts").is_file());
        assert!(!cli_dir.join("src/index.rts").exists());

        let web_dir = init_project("u-web", tmp.path(), Some("web-server")).unwrap();
        assert!(web_dir.join("src/main.rts").is_file());
        assert!(!web_dir.join("src/index.rts").exists());
    }

    #[test]
    fn test_init_project_invalid_template_no_directory_created() {
        let tmp = TempDir::new().unwrap();
        let _ = init_project("no-dir", tmp.path(), Some("nonexistent"));
        assert!(!tmp.path().join("no-dir").exists());
    }

    // --- WASM tests ---

    #[test]
    fn test_parse_wasm_target_unknown_unknown() {
        let result = parse_wasm_target("wasm32-unknown-unknown");
        assert_eq!(result, Some(WasmTarget::Unknown));
    }

    #[test]
    fn test_parse_wasm_target_wasip1() {
        let result = parse_wasm_target("wasm32-wasip1");
        assert_eq!(result, Some(WasmTarget::Wasip1));
    }

    #[test]
    fn test_parse_wasm_target_native_returns_none() {
        assert!(parse_wasm_target("x86_64-unknown-linux-gnu").is_none());
        assert!(parse_wasm_target("aarch64-apple-darwin").is_none());
    }

    #[test]
    fn test_wasm_target_triple_values() {
        assert_eq!(WasmTarget::Unknown.triple(), "wasm32-unknown-unknown");
        assert_eq!(WasmTarget::Wasip1.triple(), "wasm32-wasip1");
    }

    #[test]
    fn test_compile_for_target_wasm_excludes_tokio() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("async-wasm");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "async-wasm"}"#,
        )
        .unwrap();
        fs::write(
            src_dir.join("main.rts"),
            "async function main() {\n  console.log(\"hi\");\n}\n",
        )
        .unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, project_root, _, _) = project
            .compile_for_target(Some(&WasmTarget::Unknown))
            .unwrap();

        let cargo_toml = fs::read_to_string(project_root.join("Cargo.toml")).unwrap();
        assert!(
            !cargo_toml.contains("tokio"),
            "WASM Cargo.toml should NOT contain tokio, got:\n{cargo_toml}"
        );
    }

    #[test]
    fn test_compile_for_target_wasm_unknown_generates_lib_rs() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("wasm-lib", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, project_root, _, _) = project
            .compile_for_target(Some(&WasmTarget::Unknown))
            .unwrap();

        let lib_rs = project_root.join("src/lib.rs");
        let main_rs = project_root.join("src/main.rs");

        assert!(
            lib_rs.is_file(),
            "expected lib.rs for wasm32-unknown-unknown"
        );
        assert!(
            !main_rs.is_file(),
            "main.rs should be removed for wasm32-unknown-unknown"
        );
    }

    #[test]
    fn test_compile_for_target_wasm_wasip1_keeps_main_rs() {
        let tmp = TempDir::new().unwrap();
        let project_dir = init_project("wasi-bin", tmp.path(), None).unwrap();

        let project = Project::open(&project_dir).unwrap();
        let (_, project_root, _, _) = project
            .compile_for_target(Some(&WasmTarget::Wasip1))
            .unwrap();

        let main_rs = project_root.join("src/main.rs");
        assert!(main_rs.is_file(), "expected main.rs for wasm32-wasip1");
    }

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

    // --- CargoTomlBuilder tests ---

    #[test]
    fn test_cargo_toml_builder_deps_sorted_alphabetically() {
        let mut builder = CargoTomlBuilder::new("test-app", "2024", "0.1.0");
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

    #[test]
    fn test_cargo_toml_builder_detailed_dependency() {
        let mut builder = CargoTomlBuilder::new("test-app", "2024", "0.1.0");
        builder.add_tokio_runtime();
        let output = builder.build();

        assert!(
            output.contains("tokio = { version = \"1\", features = [\"full\"] }"),
            "expected tokio with features in Cargo.toml:\n{output}"
        );
    }

    #[test]
    fn test_cargo_toml_builder_no_deps_no_section() {
        let builder = CargoTomlBuilder::new("empty-app", "2024", "0.1.0");
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

    #[test]
    fn test_cargo_toml_builder_library_cdylib() {
        let mut builder = CargoTomlBuilder::new("wasm-app", "2024", "0.1.0");
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

    // --- Test compilation tests ---

    #[test]
    fn test_project_test_compilation_error_returns_compilation_failed() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("test-compile-err");
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            project_dir.join("rustscript.json"),
            r#"{"name": "test-compile-err"}"#,
        )
        .unwrap();
        fs::write(src_dir.join("main.rts"), "function {").unwrap();

        let project = Project::open(&project_dir).unwrap();
        let err = project.test(false, &[]).unwrap_err();
        assert!(
            matches!(err, DriverError::CompilationFailed(_)),
            "expected CompilationFailed, got: {err:?}"
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
        let project_dir = tmp.path().join("doc-test");
        fs::create_dir_all(&project_dir).unwrap();

        let project = Project {
            root: project_dir.clone(),
            name: "test".to_owned(),
            compile_options: crate::pipeline::CompileOptions::default(),
            color_mode: ColorMode::Never,
        };

        let options = project.load_compile_options_with_rustdoc(&project_dir);
        assert!(
            options.external_signatures.is_empty(),
            "no doc dir should yield empty external signatures"
        );
    }

    #[test]
    fn test_load_compile_options_with_rustdoc_empty_doc_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("doc-test2");
        let doc_dir = project_dir.join("target").join("doc");
        fs::create_dir_all(&doc_dir).unwrap();

        let project = Project {
            root: project_dir.clone(),
            name: "test".to_owned(),
            compile_options: crate::pipeline::CompileOptions::default(),
            color_mode: ColorMode::Never,
        };

        let options = project.load_compile_options_with_rustdoc(&project_dir);
        assert!(
            options.external_signatures.is_empty(),
            "empty doc dir should yield empty external signatures"
        );
    }

    #[test]
    fn test_maybe_generate_rustdoc_skips_when_json_exists() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("cached-docs");
        let doc_dir = project_dir.join("target").join("doc");
        fs::create_dir_all(&doc_dir).unwrap();

        // Write a dummy .json file to simulate cached docs
        fs::write(doc_dir.join("serde.json"), "{}").unwrap();

        // This should return early without calling cargo doc.
        Project::maybe_generate_rustdoc(&project_dir);
    }

    // --- Merge function unit tests ---

    #[test]
    fn test_merge_cargo_toml_adds_deps_to_existing() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("Cargo.toml");
        fs::write(
            &path,
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .unwrap();

        let mut auto = BTreeMap::new();
        auto.insert("serde".to_owned(), DependencySpec::Simple("1".to_owned()));

        merge_cargo_toml(&path, &auto, &BTreeMap::new()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("serde"));
    }

    #[test]
    fn test_merge_cargo_toml_does_not_overwrite_existing_deps() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("Cargo.toml");
        fs::write(
            &path,
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1.0.200\"\n\n[workspace]\n",
        )
        .unwrap();

        let mut auto = BTreeMap::new();
        auto.insert("serde".to_owned(), DependencySpec::Simple("*".to_owned()));

        merge_cargo_toml(&path, &auto, &BTreeMap::new()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("1.0.200"),
            "existing pinned version should be preserved, got:\n{content}"
        );
    }

    #[test]
    fn test_merge_cargo_toml_explicit_overrides() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("Cargo.toml");
        fs::write(
            &path,
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nrand = \"0.7\"\n\n[workspace]\n",
        )
        .unwrap();

        let mut explicit = BTreeMap::new();
        explicit.insert("rand".to_owned(), DepSpec::Simple("0.8".to_owned()));

        merge_cargo_toml(&path, &BTreeMap::new(), &explicit).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("0.8"),
            "explicit dep should override, got:\n{content}"
        );
    }

    // --- Eject tests ---

    /// Helper: create a minimal RustScript project directory for eject tests.
    fn create_eject_project(tmp: &TempDir, built: bool) -> Project {
        let root = tmp.path().to_path_buf();
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // Write rustscript.json
        fs::write(
            root.join("rustscript.json"),
            r#"{"name":"test-eject","version":"0.1.0","edition":"2024"}"#,
        )
        .unwrap();

        // Write .gitignore with /src/*.rs and /target
        fs::write(root.join(".gitignore"), "/target\n/src/*.rs\n").unwrap();

        // Write Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"test-eject\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        // Write Cargo.lock
        fs::write(root.join("Cargo.lock"), "# placeholder lock").unwrap();

        // Write .rts source
        fs::write(
            src_dir.join("main.rts"),
            "function main() {\n  console.log(\"hello\");\n}\n",
        )
        .unwrap();

        // If built, write the generated .rs file
        if built {
            fs::write(
                src_dir.join("main.rs"),
                "fn main() {\n    println!(\"hello\");\n}\n",
            )
            .unwrap();
        }

        Project {
            root,
            name: "test-eject".to_owned(),
            compile_options: crate::pipeline::CompileOptions::default(),
            color_mode: ColorMode::Never,
        }
    }

    #[test]
    fn test_remove_gitignore_rs_line_basic() {
        let input = "/target\n/src/*.rs\n";
        let result = remove_gitignore_rs_line(input);
        assert_eq!(result, "/target\n");
    }

    #[test]
    fn test_remove_gitignore_rs_line_preserves_other_lines() {
        let input = "/target\n/src/*.rs\n.env\nbuild/\n";
        let result = remove_gitignore_rs_line(input);
        assert_eq!(result, "/target\n.env\nbuild/\n");
    }

    #[test]
    fn test_remove_gitignore_rs_line_no_match() {
        let input = "/target\n.env\n";
        let result = remove_gitignore_rs_line(input);
        assert_eq!(result, "/target\n.env\n");
    }

    #[test]
    fn test_remove_gitignore_rs_line_only_rs_line() {
        let input = "/src/*.rs\n";
        let result = remove_gitignore_rs_line(input);
        assert_eq!(result, "\n");
    }

    #[test]
    fn test_eject_removes_rustscript_json() {
        let tmp = TempDir::new().unwrap();
        let project = create_eject_project(&tmp, true);

        assert!(project.root.join("rustscript.json").is_file());
        project.eject().unwrap();
        assert!(!project.root.join("rustscript.json").exists());
    }

    #[test]
    fn test_eject_updates_gitignore() {
        let tmp = TempDir::new().unwrap();
        let project = create_eject_project(&tmp, true);

        project.eject().unwrap();

        let gitignore = fs::read_to_string(project.root.join(".gitignore")).unwrap();
        assert!(
            !gitignore.contains("/src/*.rs"),
            ".gitignore should not contain /src/*.rs after eject, got:\n{gitignore}"
        );
        assert!(
            gitignore.contains("/target"),
            ".gitignore should still contain /target, got:\n{gitignore}"
        );
    }

    #[test]
    fn test_eject_fails_if_not_built() {
        let tmp = TempDir::new().unwrap();
        let project = create_eject_project(&tmp, false);

        let result = project.eject();
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), DriverError::EjectNotBuilt),
            "expected EjectNotBuilt error"
        );
    }

    #[test]
    fn test_eject_preserves_cargo_toml_and_rts_files() {
        let tmp = TempDir::new().unwrap();
        let project = create_eject_project(&tmp, true);

        let cargo_before = fs::read_to_string(project.root.join("Cargo.toml")).unwrap();
        let lock_before = fs::read_to_string(project.root.join("Cargo.lock")).unwrap();

        project.eject().unwrap();

        // Cargo.toml preserved
        let cargo_after = fs::read_to_string(project.root.join("Cargo.toml")).unwrap();
        assert_eq!(cargo_before, cargo_after, "Cargo.toml should be unchanged");

        // Cargo.lock preserved
        let lock_after = fs::read_to_string(project.root.join("Cargo.lock")).unwrap();
        assert_eq!(lock_before, lock_after, "Cargo.lock should be unchanged");

        // .rts file preserved
        assert!(
            project.root.join("src/main.rts").is_file(),
            ".rts files should be preserved"
        );

        // .rs file preserved
        assert!(
            project.root.join("src/main.rs").is_file(),
            ".rs files should be preserved"
        );
    }
}
