//! `rustscript.json` project manifest parsing and writing.
//!
//! The `rustscript.json` file is the single project manifest for `RustScript`
//! projects. It replaces the previous `rsc.toml` configuration format with
//! a JSON-based manifest that stores project metadata and dependency information.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{DriverError, Result};

/// Name of the `RustScript` project manifest file.
pub const MANIFEST_FILE: &str = "rustscript.json";

/// A `RustScript` project manifest (`rustscript.json`).
///
/// Contains project metadata and dependency specifications. Dependencies
/// can be a simple version string or an object with version and features.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Project name (required).
    pub name: String,

    /// Project version (defaults to `"0.1.0"`).
    #[serde(default = "default_version")]
    pub version: String,

    /// Rust edition (defaults to `"2024"`).
    #[serde(default = "default_edition")]
    pub edition: String,

    /// Regular dependencies.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, DepSpec>,

    /// Dev-only dependencies.
    #[serde(
        default,
        rename = "devDependencies",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub dev_dependencies: BTreeMap<String, DepSpec>,
}

/// A dependency specification in `rustscript.json`.
///
/// Can be deserialized from either a simple version string (`"1"`) or a
/// detailed object (`{ "version": "1", "features": ["derive"] }`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DepSpec {
    /// A simple version string, e.g. `"1"`.
    Simple(String),
    /// A version with additional metadata.
    Detailed(DetailedDep),
}

/// A detailed dependency specification with version and optional features.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetailedDep {
    /// The crate version (e.g., `"1"`, `"0.8"`).
    pub version: String,
    /// Optional features to enable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

impl DepSpec {
    /// Return the version string for this dependency.
    #[must_use]
    pub fn version(&self) -> &str {
        match self {
            Self::Simple(v) => v,
            Self::Detailed(d) => &d.version,
        }
    }

    /// Return the features for this dependency (empty if simple).
    #[must_use]
    pub fn features(&self) -> &[String] {
        match self {
            Self::Simple(_) => &[],
            Self::Detailed(d) => &d.features,
        }
    }
}

/// Default version for new projects.
fn default_version() -> String {
    "0.1.0".to_owned()
}

/// Default Rust edition for new projects.
fn default_edition() -> String {
    "2024".to_owned()
}

/// Read and parse `rustscript.json` from the given project root.
///
/// # Errors
///
/// Returns [`DriverError::ManifestNotFound`] if the file does not exist,
/// [`DriverError::ManifestParseFailed`] if the JSON is malformed.
pub fn read_manifest(project_root: &Path) -> Result<Manifest> {
    let path = project_root.join(MANIFEST_FILE);

    if !path.is_file() {
        return Err(DriverError::ManifestNotFound(project_root.to_path_buf()));
    }

    let content = std::fs::read_to_string(&path)?;
    parse_manifest(&content)
}

/// Try to read `rustscript.json` from the given project root.
///
/// Returns `Ok(None)` if the file does not exist, `Ok(Some(manifest))`
/// if it was parsed successfully.
///
/// # Errors
///
/// Returns [`DriverError::ManifestParseFailed`] if the file exists but is malformed.
pub fn try_read_manifest(project_root: &Path) -> Result<Option<Manifest>> {
    let path = project_root.join(MANIFEST_FILE);

    if !path.is_file() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)?;
    parse_manifest(&content).map(Some)
}

/// Parse a `rustscript.json` content string into a [`Manifest`].
///
/// # Errors
///
/// Returns [`DriverError::ManifestParseFailed`] if the JSON is malformed.
pub fn parse_manifest(content: &str) -> Result<Manifest> {
    serde_json::from_str(content).map_err(|e| DriverError::ManifestParseFailed(e.to_string()))
}

/// Write a [`Manifest`] to `rustscript.json` in the given project root.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be written.
pub fn write_manifest(project_root: &Path, manifest: &Manifest) -> Result<()> {
    let content = serde_json::to_string_pretty(manifest)
        .map_err(|e| DriverError::ManifestParseFailed(e.to_string()))?;
    let path = project_root.join(MANIFEST_FILE);
    std::fs::write(path, format!("{content}\n"))?;
    Ok(())
}

/// Create a new default [`Manifest`] with the given project name.
#[must_use]
pub fn new_manifest(name: &str) -> Manifest {
    Manifest {
        name: name.to_owned(),
        version: default_version(),
        edition: default_edition(),
        dependencies: BTreeMap::new(),
        dev_dependencies: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // --- Parsing tests ---

    #[test]
    fn test_parse_manifest_minimal() {
        let json = r#"{"name": "my-project"}"#;
        let manifest = parse_manifest(json).unwrap();
        assert_eq!(manifest.name, "my-project");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.edition, "2024");
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.dev_dependencies.is_empty());
    }

    #[test]
    fn test_parse_manifest_full() {
        let json = r#"{
            "name": "my-project",
            "version": "1.0.0",
            "edition": "2021",
            "dependencies": {
                "serde": { "version": "1", "features": ["derive"] },
                "tokio": "1"
            },
            "devDependencies": {
                "tempfile": "3"
            }
        }"#;
        let manifest = parse_manifest(json).unwrap();
        assert_eq!(manifest.name, "my-project");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.edition, "2021");
        assert_eq!(manifest.dependencies.len(), 2);
        assert_eq!(manifest.dev_dependencies.len(), 1);
    }

    #[test]
    fn test_parse_manifest_simple_dep_version() {
        let json = r#"{"name": "test", "dependencies": {"tokio": "1"}}"#;
        let manifest = parse_manifest(json).unwrap();
        let dep = &manifest.dependencies["tokio"];
        assert_eq!(dep.version(), "1");
        assert!(dep.features().is_empty());
    }

    #[test]
    fn test_parse_manifest_detailed_dep_with_features() {
        let json = r#"{"name": "test", "dependencies": {"serde": {"version": "1", "features": ["derive"]}}}"#;
        let manifest = parse_manifest(json).unwrap();
        let dep = &manifest.dependencies["serde"];
        assert_eq!(dep.version(), "1");
        assert_eq!(dep.features(), &["derive"]);
    }

    #[test]
    fn test_parse_manifest_invalid_json_returns_error() {
        let result = parse_manifest("not json");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, DriverError::ManifestParseFailed(_)),
            "expected ManifestParseFailed, got: {err:?}"
        );
    }

    #[test]
    fn test_parse_manifest_missing_name_returns_error() {
        let result = parse_manifest(r#"{"version": "1.0.0"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_manifest_empty_object_returns_error() {
        let result = parse_manifest("{}");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_manifest_defaults_applied() {
        let json = r#"{"name": "defaults-test"}"#;
        let manifest = parse_manifest(json).unwrap();
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.edition, "2024");
    }

    #[test]
    fn test_parse_manifest_explicit_values_override_defaults() {
        let json = r#"{"name": "test", "version": "2.0.0", "edition": "2021"}"#;
        let manifest = parse_manifest(json).unwrap();
        assert_eq!(manifest.version, "2.0.0");
        assert_eq!(manifest.edition, "2021");
    }

    // --- Read/write tests ---

    #[test]
    fn test_read_manifest_from_file() {
        let tmp = TempDir::new().unwrap();
        let content = r#"{"name": "file-test"}"#;
        std::fs::write(tmp.path().join(MANIFEST_FILE), content).unwrap();

        let manifest = read_manifest(tmp.path()).unwrap();
        assert_eq!(manifest.name, "file-test");
    }

    #[test]
    fn test_read_manifest_not_found() {
        let tmp = TempDir::new().unwrap();
        let err = read_manifest(tmp.path()).unwrap_err();
        assert!(
            matches!(err, DriverError::ManifestNotFound(_)),
            "expected ManifestNotFound, got: {err:?}"
        );
    }

    #[test]
    fn test_try_read_manifest_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        let result = try_read_manifest(tmp.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_try_read_manifest_returns_some_when_present() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(MANIFEST_FILE), r#"{"name": "found"}"#).unwrap();

        let result = try_read_manifest(tmp.path()).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "found");
    }

    #[test]
    fn test_write_manifest_creates_file() {
        let tmp = TempDir::new().unwrap();
        let manifest = new_manifest("write-test");
        write_manifest(tmp.path(), &manifest).unwrap();

        let path = tmp.path().join(MANIFEST_FILE);
        assert!(path.is_file());

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("write-test"));
    }

    #[test]
    fn test_write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let mut manifest = new_manifest("roundtrip");
        manifest
            .dependencies
            .insert("serde".to_owned(), DepSpec::Simple("1".to_owned()));
        manifest.dependencies.insert(
            "tokio".to_owned(),
            DepSpec::Detailed(DetailedDep {
                version: "1".to_owned(),
                features: vec!["full".to_owned()],
            }),
        );

        write_manifest(tmp.path(), &manifest).unwrap();
        let read_back = read_manifest(tmp.path()).unwrap();

        assert_eq!(read_back.name, "roundtrip");
        assert_eq!(read_back.dependencies.len(), 2);
        assert_eq!(read_back.dependencies["serde"].version(), "1");
        assert_eq!(read_back.dependencies["tokio"].version(), "1");
        assert_eq!(read_back.dependencies["tokio"].features(), &["full"]);
    }

    // --- Helper tests ---

    #[test]
    fn test_new_manifest_has_defaults() {
        let manifest = new_manifest("test-project");
        assert_eq!(manifest.name, "test-project");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.edition, "2024");
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.dev_dependencies.is_empty());
    }

    #[test]
    fn test_dep_spec_version_simple() {
        let spec = DepSpec::Simple("1.0".to_owned());
        assert_eq!(spec.version(), "1.0");
    }

    #[test]
    fn test_dep_spec_version_detailed() {
        let spec = DepSpec::Detailed(DetailedDep {
            version: "2.0".to_owned(),
            features: vec!["full".to_owned()],
        });
        assert_eq!(spec.version(), "2.0");
        assert_eq!(spec.features(), &["full"]);
    }

    #[test]
    fn test_dep_spec_features_empty_for_simple() {
        let spec = DepSpec::Simple("1".to_owned());
        assert!(spec.features().is_empty());
    }

    #[test]
    fn test_write_manifest_pretty_printed() {
        let tmp = TempDir::new().unwrap();
        let manifest = new_manifest("pretty");
        write_manifest(tmp.path(), &manifest).unwrap();

        let content = std::fs::read_to_string(tmp.path().join(MANIFEST_FILE)).unwrap();
        // Pretty-printed JSON should have newlines
        assert!(content.contains('\n'), "should be pretty-printed");
        // Should end with a newline
        assert!(content.ends_with('\n'), "should end with newline");
    }
}
