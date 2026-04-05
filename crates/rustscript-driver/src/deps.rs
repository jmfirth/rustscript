//! Dependency management for `RustScript` projects.
//!
//! Handles reading and writing dependencies via `rustscript.json`, adding/removing
//! dependencies, and providing import suggestions for common crates.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{DriverError, Result};
use crate::manifest::{self, DepSpec, DetailedDep};

/// A dependency entry (used for the public API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepEntry {
    /// The crate version (e.g., `"1"`, `"0.8"`).
    pub version: String,
    /// Enabled features (may be empty).
    pub features: Vec<String>,
    /// Whether this is a dev-only dependency.
    pub dev: bool,
}

/// Result of adding a dependency, for display purposes.
#[derive(Debug)]
pub struct AddResult {
    /// The crate name that was added.
    pub crate_name: String,
    /// The version that was recorded.
    pub version: String,
    /// Features that were enabled.
    pub features: Vec<String>,
    /// Whether it was added as a dev dependency.
    pub dev: bool,
    /// Optional import suggestion.
    pub import_suggestion: Option<String>,
}

/// Read all dependencies from `rustscript.json` at the given project root.
///
/// Returns two maps: regular dependencies and dev dependencies.
/// If `rustscript.json` does not exist, returns empty maps (not an error).
///
/// # Errors
///
/// Returns [`DriverError::ManifestParseFailed`] if `rustscript.json` exists but is malformed,
/// or an I/O error if the file cannot be read.
pub fn read_config(
    project_root: &Path,
) -> Result<(BTreeMap<String, DepEntry>, BTreeMap<String, DepEntry>)> {
    let Some(manifest) = manifest::try_read_manifest(project_root)? else {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    };

    let deps = manifest
        .dependencies
        .iter()
        .map(|(name, spec)| {
            (
                name.clone(),
                DepEntry {
                    version: spec.version().to_owned(),
                    features: spec.features().to_vec(),
                    dev: false,
                },
            )
        })
        .collect();

    let dev_deps = manifest
        .dev_dependencies
        .iter()
        .map(|(name, spec)| {
            (
                name.clone(),
                DepEntry {
                    version: spec.version().to_owned(),
                    features: spec.features().to_vec(),
                    dev: true,
                },
            )
        })
        .collect();

    Ok((deps, dev_deps))
}

/// Add a dependency to `rustscript.json` at the given project root.
///
/// Creates `rustscript.json` if it does not exist. Updates an existing entry
/// if the crate is already present.
///
/// # Errors
///
/// Returns [`DriverError::ManifestParseFailed`] if the existing `rustscript.json` is malformed,
/// or an I/O error if the file cannot be read or written.
pub fn add_dependency(
    project_root: &Path,
    crate_name: &str,
    version: Option<&str>,
    features: &[String],
    dev: bool,
) -> Result<AddResult> {
    let mut manifest_data = if let Some(m) = manifest::try_read_manifest(project_root)? {
        m
    } else {
        // Create a new manifest using the directory name as the project name
        let name = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_owned();
        manifest::new_manifest(&name)
    };

    let resolved_version = version.unwrap_or("*").to_owned();

    let spec = if features.is_empty() {
        DepSpec::Simple(resolved_version.clone())
    } else {
        DepSpec::Detailed(DetailedDep {
            version: resolved_version.clone(),
            features: features.to_vec(),
        })
    };

    if dev {
        manifest_data
            .dev_dependencies
            .insert(crate_name.to_owned(), spec);
    } else {
        manifest_data
            .dependencies
            .insert(crate_name.to_owned(), spec);
    }

    manifest::write_manifest(project_root, &manifest_data)?;

    Ok(AddResult {
        crate_name: crate_name.to_owned(),
        version: resolved_version,
        features: features.to_vec(),
        dev,
        import_suggestion: import_suggestion(crate_name),
    })
}

/// Remove a dependency from `rustscript.json` at the given project root.
///
/// Removes from both `dependencies` and `devDependencies`.
///
/// # Errors
///
/// Returns [`DriverError::DependencyNotFound`] if the crate is not in `rustscript.json`.
pub fn remove_dependency(project_root: &Path, crate_name: &str) -> Result<()> {
    let Some(mut manifest_data) = manifest::try_read_manifest(project_root)? else {
        return Err(DriverError::DependencyNotFound(crate_name.to_owned()));
    };

    let found_in_deps = manifest_data.dependencies.remove(crate_name).is_some();
    let found_in_dev_deps = manifest_data.dev_dependencies.remove(crate_name).is_some();

    if !found_in_deps && !found_in_dev_deps {
        return Err(DriverError::DependencyNotFound(crate_name.to_owned()));
    }

    manifest::write_manifest(project_root, &manifest_data)?;
    Ok(())
}

/// Convert a `DepEntry` to a manifest `DepSpec`.
#[must_use]
pub fn entry_to_spec(entry: &DepEntry) -> DepSpec {
    if entry.features.is_empty() {
        DepSpec::Simple(entry.version.clone())
    } else {
        DepSpec::Detailed(DetailedDep {
            version: entry.version.clone(),
            features: entry.features.clone(),
        })
    }
}

/// Return an import suggestion for well-known crates.
#[must_use]
pub fn import_suggestion(crate_name: &str) -> Option<String> {
    let suggestion = match crate_name {
        "serde" => "import { Serialize, Deserialize } from \"serde\";",
        "tokio" => "import { sleep, spawn } from \"tokio\";",
        "axum" => "import { Router, get, post } from \"axum\";",
        "reqwest" => "import { Client } from \"reqwest\";",
        "clap" => "import { Parser } from \"clap\";",
        "anyhow" => "import { Result, Context } from \"anyhow\";",
        "thiserror" => "import { Error } from \"thiserror\";",
        "rand" => "import { Rng, thread_rng } from \"rand\";",
        "serde_json" => "import { Value, from_str, to_string } from \"serde_json\";",
        "tracing" => "import { info, warn, error, debug } from \"tracing\";",
        "uuid" => "import { Uuid } from \"uuid\";",
        "chrono" => "import { DateTime, Utc } from \"chrono\";",
        "regex" => "import { Regex } from \"regex\";",
        _ => return None,
    };
    Some(suggestion.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_add_dependency_creates_rustscript_json() {
        let tmp = TempDir::new().unwrap();
        let result = add_dependency(tmp.path(), "serde", Some("1"), &[], false).unwrap();

        assert_eq!(result.crate_name, "serde");
        assert_eq!(result.version, "1");
        assert!(!result.dev);

        let config_path = tmp.path().join("rustscript.json");
        assert!(config_path.is_file());

        let content = std::fs::read_to_string(config_path).unwrap();
        assert!(
            content.contains("serde"),
            "rustscript.json should contain serde"
        );
    }

    #[test]
    fn test_add_dependency_with_features() {
        let tmp = TempDir::new().unwrap();
        let features = vec!["derive".to_owned(), "full".to_owned()];
        let result = add_dependency(tmp.path(), "serde", Some("1"), &features, false).unwrap();

        assert_eq!(result.features, features);

        let (deps, _) = read_config(tmp.path()).unwrap();
        let entry = deps.get("serde").unwrap();
        assert_eq!(entry.features, features);
    }

    #[test]
    fn test_add_dev_dependency() {
        let tmp = TempDir::new().unwrap();
        add_dependency(tmp.path(), "tempfile", Some("3"), &[], true).unwrap();

        let (deps, dev_deps) = read_config(tmp.path()).unwrap();
        assert!(deps.is_empty());
        assert!(dev_deps.contains_key("tempfile"));
        assert!(dev_deps["tempfile"].dev);
    }

    #[test]
    fn test_add_dependency_default_version() {
        let tmp = TempDir::new().unwrap();
        let result = add_dependency(tmp.path(), "rand", None, &[], false).unwrap();

        assert_eq!(result.version, "*");
    }

    #[test]
    fn test_add_dependency_updates_existing() {
        let tmp = TempDir::new().unwrap();
        add_dependency(tmp.path(), "serde", Some("1"), &[], false).unwrap();

        // Add again with features - should update
        let features = vec!["derive".to_owned()];
        add_dependency(tmp.path(), "serde", Some("1"), &features, false).unwrap();

        let (deps, _) = read_config(tmp.path()).unwrap();
        let entry = deps.get("serde").unwrap();
        assert_eq!(entry.features, vec!["derive".to_owned()]);
    }

    #[test]
    fn test_remove_dependency() {
        let tmp = TempDir::new().unwrap();
        add_dependency(tmp.path(), "serde", Some("1"), &[], false).unwrap();
        add_dependency(tmp.path(), "tokio", Some("1"), &[], false).unwrap();

        remove_dependency(tmp.path(), "serde").unwrap();

        let (deps, _) = read_config(tmp.path()).unwrap();
        assert!(!deps.contains_key("serde"));
        assert!(deps.contains_key("tokio"));
    }

    #[test]
    fn test_remove_dependency_not_found() {
        let tmp = TempDir::new().unwrap();
        let err = remove_dependency(tmp.path(), "nonexistent").unwrap_err();
        assert!(
            matches!(err, DriverError::DependencyNotFound(ref name) if name == "nonexistent"),
            "expected DependencyNotFound, got: {err:?}"
        );
    }

    #[test]
    fn test_remove_dev_dependency() {
        let tmp = TempDir::new().unwrap();
        add_dependency(tmp.path(), "tempfile", Some("3"), &[], true).unwrap();

        remove_dependency(tmp.path(), "tempfile").unwrap();

        let (_, dev_deps) = read_config(tmp.path()).unwrap();
        assert!(!dev_deps.contains_key("tempfile"));
    }

    #[test]
    fn test_read_config_missing_file() {
        let tmp = TempDir::new().unwrap();
        let (deps, dev_deps) = read_config(tmp.path()).unwrap();
        assert!(deps.is_empty());
        assert!(dev_deps.is_empty());
    }

    #[test]
    fn test_import_suggestion_known_crate() {
        let suggestion = import_suggestion("serde");
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("Serialize"));
    }

    #[test]
    fn test_import_suggestion_unknown_crate() {
        let suggestion = import_suggestion("my-obscure-crate");
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_add_dependency_import_suggestion_returned() {
        let tmp = TempDir::new().unwrap();
        let result = add_dependency(tmp.path(), "serde", Some("1"), &[], false).unwrap();
        assert!(result.import_suggestion.is_some());

        let result2 = add_dependency(tmp.path(), "unknown-crate", None, &[], false).unwrap();
        assert!(result2.import_suggestion.is_none());
    }

    #[test]
    fn test_multiple_dependencies_ordering() {
        let tmp = TempDir::new().unwrap();
        add_dependency(tmp.path(), "tokio", Some("1"), &[], false).unwrap();
        add_dependency(tmp.path(), "serde", Some("1"), &[], false).unwrap();
        add_dependency(tmp.path(), "anyhow", Some("1"), &[], false).unwrap();

        let (deps, _) = read_config(tmp.path()).unwrap();
        let keys: Vec<&String> = deps.keys().collect();
        // BTreeMap gives alphabetical order
        assert_eq!(keys, vec!["anyhow", "serde", "tokio"]);
    }

    #[test]
    fn test_read_config_with_rustscript_json() {
        let tmp = TempDir::new().unwrap();
        let json = r#"{
            "name": "test",
            "dependencies": {
                "serde": { "version": "1", "features": ["derive"] },
                "tokio": "1"
            },
            "devDependencies": {
                "tempfile": "3"
            }
        }"#;
        std::fs::write(tmp.path().join("rustscript.json"), json).unwrap();

        let (deps, dev_deps) = read_config(tmp.path()).unwrap();
        assert_eq!(deps["serde"].version, "1");
        assert_eq!(deps["serde"].features, vec!["derive"]);
        assert_eq!(deps["tokio"].version, "1");
        assert!(deps["tokio"].features.is_empty());
        assert_eq!(dev_deps["tempfile"].version, "3");
    }

    #[test]
    fn test_entry_to_spec_simple() {
        let entry = DepEntry {
            version: "1".to_owned(),
            features: vec![],
            dev: false,
        };
        let spec = entry_to_spec(&entry);
        assert!(matches!(spec, DepSpec::Simple(ref v) if v == "1"));
    }

    #[test]
    fn test_entry_to_spec_detailed() {
        let entry = DepEntry {
            version: "1".to_owned(),
            features: vec!["derive".to_owned()],
            dev: false,
        };
        let spec = entry_to_spec(&entry);
        assert!(matches!(spec, DepSpec::Detailed(_)));
    }
}
