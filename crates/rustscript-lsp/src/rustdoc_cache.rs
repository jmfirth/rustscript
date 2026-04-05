//! Cache for parsed and translated rustdoc documentation.
//!
//! Caches the parsed rustdoc JSON per crate so that repeated hover requests
//! don't re-parse the JSON files. The cache is populated lazily on first
//! hover over an import from a given crate.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::rustdoc_parser::{self, RustdocCrate};
use crate::rustdoc_translator;

/// Cache of parsed rustdoc data per crate name.
///
/// Thread-safe via `RwLock` in the caller; this struct itself is
/// designed to be held behind a lock or in a concurrent map.
#[derive(Debug, Default)]
pub struct RustdocCache {
    /// Parsed crate data keyed by crate name.
    crates: HashMap<String, Arc<RustdocCrate>>,
    /// Crates that failed to load (avoid retrying).
    failed: HashSet<String>,
}

impl RustdocCache {
    /// Create a new empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a symbol in a crate's rustdoc and return the translated hover.
    ///
    /// If the crate hasn't been loaded yet, attempts to load it from the
    /// build directory. Returns `None` if the crate docs aren't available
    /// or the symbol isn't found.
    pub fn lookup_hover(
        &mut self,
        crate_name: &str,
        symbol_name: &str,
        build_dir: &Path,
    ) -> Option<String> {
        // Check if we already know this crate can't be loaded.
        if self.failed.contains(crate_name) {
            return None;
        }

        // Load the crate if not cached.
        if !self.crates.contains_key(crate_name) {
            if let Some(crate_data) = load_crate_docs(crate_name, build_dir) {
                self.crates
                    .insert(crate_name.to_owned(), Arc::new(crate_data));
            } else {
                self.failed.insert(crate_name.to_owned());
                return None;
            }
        }

        let crate_data = self.crates.get(crate_name)?;
        let item = rustdoc_parser::lookup_item(crate_data, symbol_name)?;
        Some(rustdoc_translator::translate_item_to_hover(item))
    }

    /// Check if a crate's docs are cached.
    #[must_use]
    pub fn is_cached(&self, crate_name: &str) -> bool {
        self.crates.contains_key(crate_name)
    }

    /// Get the cached crate data for direct access.
    #[must_use]
    pub fn get_crate(&self, crate_name: &str) -> Option<&Arc<RustdocCrate>> {
        self.crates.get(crate_name)
    }

    /// Clear all cached data.
    pub fn clear(&mut self) {
        self.crates.clear();
        self.failed.clear();
    }

    /// Insert pre-parsed crate data (useful for testing).
    pub fn insert(&mut self, crate_name: String, data: RustdocCrate) {
        self.crates.insert(crate_name, Arc::new(data));
    }
}

/// Attempt to load rustdoc JSON for a crate from the build directory.
///
/// Looks for the JSON file at `target/doc/{crate_name}.json` in the project root.
/// Returns `None` if the file doesn't exist or can't be parsed.
fn load_crate_docs(crate_name: &str, build_dir: &Path) -> Option<RustdocCrate> {
    let json_path = find_rustdoc_json(crate_name, build_dir)?;
    let contents = std::fs::read_to_string(&json_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    rustdoc_parser::parse_rustdoc_json(&json)
}

/// Find the rustdoc JSON file for a crate in the build directory.
///
/// Searches common locations where `cargo doc --output-format json` places
/// the output files.
fn find_rustdoc_json(crate_name: &str, build_dir: &Path) -> Option<PathBuf> {
    // The crate name in the filesystem uses underscores instead of hyphens.
    let fs_name = crate_name.replace('-', "_");

    // Primary location: target/doc/{crate_name}.json
    let primary = build_dir
        .join("target")
        .join("doc")
        .join(format!("{fs_name}.json"));
    if primary.exists() {
        return Some(primary);
    }

    // Alternative: just doc/{crate_name}.json
    let alt = build_dir.join("doc").join(format!("{fs_name}.json"));
    if alt.exists() {
        return Some(alt);
    }

    None
}

/// Generate rustdoc JSON for the project's dependencies.
///
/// Runs `cargo doc --output-format json` in the build directory.
/// This requires nightly Rust or a recent stable version with unstable options.
///
/// Returns `true` if the command succeeded, `false` otherwise.
#[must_use]
pub fn generate_rustdoc_json(build_dir: &Path) -> bool {
    // Try nightly-style first.
    let result = std::process::Command::new("cargo")
        .args(["+nightly", "doc", "--output-format", "json", "--no-deps"])
        .current_dir(build_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(status) = result
        && status.success()
    {
        return true;
    }

    // Fall back to `-Z unstable-options`.
    let result = std::process::Command::new("cargo")
        .args([
            "doc",
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
            "--no-deps",
        ])
        .current_dir(build_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(status) = result {
        return status.success();
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rustdoc_parser::{RustdocFunction, RustdocItem, RustdocItemKind, RustdocType};

    fn make_test_crate() -> RustdocCrate {
        let mut crate_data = RustdocCrate::default();

        let item = RustdocItem {
            id: "0:1".to_owned(),
            name: "Router".to_owned(),
            docs: Some("An HTTP router.".to_owned()),
            kind: RustdocItemKind::Struct(crate::rustdoc_parser::RustdocStruct {
                generics: vec![],
                fields: vec![],
                is_tuple: false,
                method_ids: vec![],
            }),
        };

        crate_data
            .name_index
            .entry("Router".to_owned())
            .or_default()
            .push("0:1".to_owned());
        crate_data.items.insert("0:1".to_owned(), item);

        let func_item = RustdocItem {
            id: "0:2".to_owned(),
            name: "get".to_owned(),
            docs: Some("Create a GET handler.".to_owned()),
            kind: RustdocItemKind::Function(RustdocFunction {
                generics: vec![],
                params: vec![],
                return_type: Some(RustdocType::ResolvedPath {
                    name: "MethodRouter".to_owned(),
                    args: vec![],
                }),
                is_async: false,
                is_unsafe: false,
                has_self: false,
                parent_type: None, is_trait_impl: false,
            }),
        };

        crate_data
            .name_index
            .entry("get".to_owned())
            .or_default()
            .push("0:2".to_owned());
        crate_data.items.insert("0:2".to_owned(), func_item);

        crate_data
    }

    #[test]
    fn test_rustdoc_cache_new_is_empty() {
        let cache = RustdocCache::new();
        assert!(!cache.is_cached("axum"));
    }

    #[test]
    fn test_rustdoc_cache_insert_and_lookup() {
        let mut cache = RustdocCache::new();
        cache.insert("axum".to_owned(), make_test_crate());
        assert!(cache.is_cached("axum"));
    }

    #[test]
    fn test_rustdoc_cache_lookup_hover_from_inserted() {
        let mut cache = RustdocCache::new();
        cache.insert("axum".to_owned(), make_test_crate());

        // The lookup_hover needs a build_dir, but since the crate is already
        // cached, it won't try to load from disk.
        let hover = cache.lookup_hover("axum", "Router", Path::new("/nonexistent"));
        assert!(hover.is_some());
        let hover_text = hover.unwrap();
        assert!(hover_text.contains("class Router"));
        assert!(hover_text.contains("An HTTP router."));
    }

    #[test]
    fn test_rustdoc_cache_lookup_hover_function() {
        let mut cache = RustdocCache::new();
        cache.insert("axum".to_owned(), make_test_crate());

        let hover = cache.lookup_hover("axum", "get", Path::new("/nonexistent"));
        assert!(hover.is_some());
        let hover_text = hover.unwrap();
        assert!(hover_text.contains("function get(): MethodRouter"));
    }

    #[test]
    fn test_rustdoc_cache_lookup_hover_missing_symbol() {
        let mut cache = RustdocCache::new();
        cache.insert("axum".to_owned(), make_test_crate());

        let hover = cache.lookup_hover("axum", "nonexistent", Path::new("/nonexistent"));
        assert!(hover.is_none());
    }

    #[test]
    fn test_rustdoc_cache_lookup_hover_missing_crate() {
        let mut cache = RustdocCache::new();

        // Will try to load from disk, fail, and mark as failed.
        let hover = cache.lookup_hover("nonexistent_crate", "Foo", Path::new("/nonexistent"));
        assert!(hover.is_none());
        assert!(cache.failed.contains("nonexistent_crate"));
    }

    #[test]
    fn test_rustdoc_cache_failed_crate_not_retried() {
        let mut cache = RustdocCache::new();

        // First attempt fails.
        let _ = cache.lookup_hover("bad_crate", "Foo", Path::new("/nonexistent"));
        assert!(cache.failed.contains("bad_crate"));

        // Second attempt should also return None without trying disk again.
        let hover = cache.lookup_hover("bad_crate", "Foo", Path::new("/nonexistent"));
        assert!(hover.is_none());
    }

    #[test]
    fn test_rustdoc_cache_clear() {
        let mut cache = RustdocCache::new();
        cache.insert("axum".to_owned(), make_test_crate());
        let _ = cache.lookup_hover("bad", "Foo", Path::new("/nonexistent"));

        cache.clear();
        assert!(!cache.is_cached("axum"));
        assert!(!cache.failed.contains("bad"));
    }

    #[test]
    fn test_rustdoc_cache_get_crate() {
        let mut cache = RustdocCache::new();
        cache.insert("axum".to_owned(), make_test_crate());

        let crate_data = cache.get_crate("axum");
        assert!(crate_data.is_some());
        assert!(crate_data.unwrap().name_index.contains_key("Router"));
    }

    #[test]
    fn test_find_rustdoc_json_nonexistent_dir() {
        let result = find_rustdoc_json("axum", Path::new("/nonexistent/path"));
        assert!(result.is_none());
    }
}
