//! Dependency management for `RustScript` projects.
//!
//! Handles reading and writing `rsc.toml` configuration, adding/removing
//! dependencies, and providing import suggestions for common crates.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{DriverError, Result};

/// Name of the `RustScript` project config file.
const RSC_CONFIG: &str = "rsc.toml";

/// A dependency entry in `rsc.toml`.
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

/// Read all dependencies from `rsc.toml` at the given project root.
///
/// Returns two maps: regular dependencies and dev dependencies.
/// If `rsc.toml` does not exist, returns empty maps (not an error).
///
/// # Errors
///
/// Returns [`DriverError::ConfigParseFailed`] if `rsc.toml` exists but is malformed,
/// or an I/O error if the file cannot be read.
pub fn read_config(
    project_root: &Path,
) -> Result<(BTreeMap<String, DepEntry>, BTreeMap<String, DepEntry>)> {
    let config_path = project_root.join(RSC_CONFIG);

    if !config_path.is_file() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }

    let content = std::fs::read_to_string(&config_path)?;
    parse_config(&content)
}

/// Parse `rsc.toml` content into dependency maps.
fn parse_config(content: &str) -> Result<(BTreeMap<String, DepEntry>, BTreeMap<String, DepEntry>)> {
    let table: toml::Table = content
        .parse()
        .map_err(|e: toml::de::Error| DriverError::ConfigParseFailed(e.to_string()))?;

    let deps = parse_dep_section(table.get("dependencies"), false)?;
    let dev_deps = parse_dep_section(table.get("dev-dependencies"), true)?;

    Ok((deps, dev_deps))
}

/// Parse a single `[dependencies]` or `[dev-dependencies]` section.
fn parse_dep_section(
    section: Option<&toml::Value>,
    dev: bool,
) -> Result<BTreeMap<String, DepEntry>> {
    let mut result = BTreeMap::new();

    let Some(toml::Value::Table(table)) = section else {
        return Ok(result);
    };

    for (name, value) in table {
        let entry = match value {
            toml::Value::String(version) => DepEntry {
                version: version.clone(),
                features: Vec::new(),
                dev,
            },
            toml::Value::Table(t) => {
                let version = t
                    .get("version")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("*")
                    .to_owned();
                let features = t
                    .get("features")
                    .and_then(toml::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(toml::Value::as_str)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                DepEntry {
                    version,
                    features,
                    dev,
                }
            }
            _ => {
                return Err(DriverError::ConfigParseFailed(format!(
                    "invalid dependency format for '{name}'"
                )));
            }
        };
        result.insert(name.clone(), entry);
    }

    Ok(result)
}

/// Add a dependency to `rsc.toml` at the given project root.
///
/// Creates `rsc.toml` if it does not exist. Updates an existing entry
/// if the crate is already present.
///
/// # Errors
///
/// Returns [`DriverError::ConfigParseFailed`] if the existing `rsc.toml` is malformed,
/// or an I/O error if the file cannot be read or written.
pub fn add_dependency(
    project_root: &Path,
    crate_name: &str,
    version: Option<&str>,
    features: &[String],
    dev: bool,
) -> Result<AddResult> {
    let (mut deps, mut dev_deps) = read_config(project_root)?;

    let resolved_version = version.unwrap_or("*").to_owned();

    let entry = DepEntry {
        version: resolved_version.clone(),
        features: features.to_vec(),
        dev,
    };

    if dev {
        dev_deps.insert(crate_name.to_owned(), entry);
    } else {
        deps.insert(crate_name.to_owned(), entry);
    }

    write_config(project_root, &deps, &dev_deps)?;

    Ok(AddResult {
        crate_name: crate_name.to_owned(),
        version: resolved_version,
        features: features.to_vec(),
        dev,
        import_suggestion: import_suggestion(crate_name),
    })
}

/// Remove a dependency from `rsc.toml` at the given project root.
///
/// Removes from both `[dependencies]` and `[dev-dependencies]`.
///
/// # Errors
///
/// Returns [`DriverError::DependencyNotFound`] if the crate is not in `rsc.toml`.
pub fn remove_dependency(project_root: &Path, crate_name: &str) -> Result<()> {
    let (mut deps, mut dev_deps) = read_config(project_root)?;

    let found_in_deps = deps.remove(crate_name).is_some();
    let found_in_dev_deps = dev_deps.remove(crate_name).is_some();

    if !found_in_deps && !found_in_dev_deps {
        return Err(DriverError::DependencyNotFound(crate_name.to_owned()));
    }

    write_config(project_root, &deps, &dev_deps)?;
    Ok(())
}

/// Write dependency maps back to `rsc.toml`.
fn write_config(
    project_root: &Path,
    deps: &BTreeMap<String, DepEntry>,
    dev_deps: &BTreeMap<String, DepEntry>,
) -> Result<()> {
    let mut table = toml::Table::new();

    if !deps.is_empty() {
        table.insert(
            "dependencies".to_owned(),
            toml::Value::Table(deps_to_toml(deps)),
        );
    }

    if !dev_deps.is_empty() {
        table.insert(
            "dev-dependencies".to_owned(),
            toml::Value::Table(deps_to_toml(dev_deps)),
        );
    }

    let content = toml::to_string_pretty(&table)
        .map_err(|e| DriverError::ConfigParseFailed(e.to_string()))?;

    let config_path = project_root.join(RSC_CONFIG);
    std::fs::write(config_path, content)?;

    Ok(())
}

/// Convert a dependency map to a TOML table.
fn deps_to_toml(deps: &BTreeMap<String, DepEntry>) -> toml::Table {
    let mut table = toml::Table::new();

    for (name, entry) in deps {
        if entry.features.is_empty() {
            table.insert(name.clone(), toml::Value::String(entry.version.clone()));
        } else {
            let mut dep_table = toml::Table::new();
            dep_table.insert(
                "version".to_owned(),
                toml::Value::String(entry.version.clone()),
            );
            dep_table.insert(
                "features".to_owned(),
                toml::Value::Array(
                    entry
                        .features
                        .iter()
                        .map(|f| toml::Value::String(f.clone()))
                        .collect(),
                ),
            );
            table.insert(name.clone(), toml::Value::Table(dep_table));
        }
    }

    table
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
    fn test_add_dependency_creates_rsc_toml() {
        let tmp = TempDir::new().unwrap();
        let result = add_dependency(tmp.path(), "serde", Some("1"), &[], false).unwrap();

        assert_eq!(result.crate_name, "serde");
        assert_eq!(result.version, "1");
        assert!(!result.dev);

        let config_path = tmp.path().join("rsc.toml");
        assert!(config_path.is_file());

        let content = std::fs::read_to_string(config_path).unwrap();
        assert!(content.contains("serde"), "rsc.toml should contain serde");
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

        // Add again with features — should update
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
    fn test_parse_config_simple_version() {
        let content = "[dependencies]\nserde = \"1\"\n";
        let (deps, _) = parse_config(content).unwrap();
        assert_eq!(deps["serde"].version, "1");
        assert!(deps["serde"].features.is_empty());
    }

    #[test]
    fn test_parse_config_detailed_version() {
        let content = "[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n";
        let (deps, _) = parse_config(content).unwrap();
        assert_eq!(deps["serde"].version, "1");
        assert_eq!(deps["serde"].features, vec!["derive"]);
    }

    #[test]
    fn test_parse_config_mixed_sections() {
        let content = "[dependencies]\nserde = \"1\"\n\n[dev-dependencies]\ntempfile = \"3\"\n";
        let (deps, dev_deps) = parse_config(content).unwrap();
        assert!(deps.contains_key("serde"));
        assert!(dev_deps.contains_key("tempfile"));
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
}
