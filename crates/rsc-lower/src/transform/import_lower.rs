//! Import classification and lowering.
//!
//! Handles the classification of `RustScript` import paths into local, builtin,
//! standard library, and external crate categories, producing appropriate `use`
//! declarations and crate dependency records.

use std::collections::HashSet;

use rsc_syntax::ast;
use rsc_syntax::rust_ir::RustUseDecl;
use rsc_syntax::span::Span;

use crate::CrateDependency;

/// Resolve a `RustScript` local import path to a Rust module path.
///
/// Maps `"./models"` to `crate::models`, `"./utils/helpers"` to `crate::utils::helpers`.
/// Only used for local imports (`"./"` or `"../"`).
fn resolve_import_path(source: &str) -> String {
    let stripped = source.strip_prefix("./").unwrap_or(source);
    let module_path = stripped.replace('/', "::");
    format!("crate::{module_path}")
}

/// Classify an import source path and produce appropriate `use` declarations
/// and crate dependencies.
///
/// Import paths fall into four categories:
/// 1. **Local** (`"./"` or `"../"`) — existing module imports → `use crate::module::Name;`
/// 2. **Builtin** (`"std/concurrent"`) — compiler-handled, no `use` or dependency
/// 3. **Standard library** (`"std/..."`) — `use std::...::Name;`, no Cargo.toml entry
/// 4. **External crate** (everything else) — `use crate_name::Name;` + dependency
pub(super) fn classify_import(
    source: &str,
    names: &[ast::Ident],
    public: bool,
    span: Span,
    import_uses: &mut Vec<RustUseDecl>,
    crate_deps: &mut HashSet<CrateDependency>,
) {
    if source.starts_with("./") || source.starts_with("../") {
        // Local module import (existing behavior)
        let module_path = resolve_import_path(source);
        for name in names {
            import_uses.push(RustUseDecl {
                path: format!("{}::{}", module_path, name.name),
                public,
                span: Some(span),
            });
        }
    } else if source == "std/concurrent" {
        // Builtin module — no use declaration, no dependency (Task 030)
    } else if source.starts_with("std/") {
        // Standard library import
        let rust_path = source.replace('/', "::");
        for name in names {
            import_uses.push(RustUseDecl {
                path: format!("{}::{}", rust_path, name.name),
                public,
                span: Some(span),
            });
        }
    } else {
        // External crate import
        let parts: Vec<&str> = source.split('/').collect();
        let crate_name = parts[0].replace('-', "_");
        let rust_path = parts
            .iter()
            .map(|p| p.replace('-', "_"))
            .collect::<Vec<_>>()
            .join("::");

        for name in names {
            import_uses.push(RustUseDecl {
                path: format!("{}::{}", rust_path, name.name),
                public,
                span: Some(span),
            });
        }

        crate_deps.insert(CrateDependency { name: crate_name });
    }
}

/// Classify a wildcard re-export source path and produce a `pub use path::*;`
/// declaration plus any crate dependency.
///
/// Same path classification as [`classify_import`]:
/// 1. **Local** (`"./"` or `"../"`) — `pub use crate::module::*;`
/// 2. **Standard library** (`"std/..."`) — `pub use std::...::*;`
/// 3. **External crate** (everything else) — `pub use crate_name::*;` + dependency
pub(super) fn classify_wildcard_reexport(
    source: &str,
    span: Span,
    import_uses: &mut Vec<RustUseDecl>,
    crate_deps: &mut HashSet<CrateDependency>,
) {
    if source.starts_with("./") || source.starts_with("../") {
        // Local module wildcard re-export
        let module_path = resolve_import_path(source);
        import_uses.push(RustUseDecl {
            path: format!("{module_path}::*"),
            public: true,
            span: Some(span),
        });
    } else if source.starts_with("std/") {
        // Standard library wildcard re-export
        let rust_path = source.replace('/', "::");
        import_uses.push(RustUseDecl {
            path: format!("{rust_path}::*"),
            public: true,
            span: Some(span),
        });
    } else {
        // External crate wildcard re-export
        let parts: Vec<&str> = source.split('/').collect();
        let crate_name = parts[0].replace('-', "_");
        let rust_path = parts
            .iter()
            .map(|p| p.replace('-', "_"))
            .collect::<Vec<_>>()
            .join("::");

        import_uses.push(RustUseDecl {
            path: format!("{rust_path}::*"),
            public: true,
            span: Some(span),
        });

        crate_deps.insert(CrateDependency { name: crate_name });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_import_path_simple() {
        assert_eq!(resolve_import_path("./models"), "crate::models");
    }

    #[test]
    fn test_resolve_import_path_nested() {
        assert_eq!(
            resolve_import_path("./utils/helpers"),
            "crate::utils::helpers"
        );
    }

    #[test]
    fn test_classify_wildcard_reexport_local() {
        let mut uses = Vec::new();
        let mut deps = HashSet::new();
        let span = Span::new(0, 0);
        classify_wildcard_reexport("./utils", span, &mut uses, &mut deps);
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, "crate::utils::*");
        assert!(uses[0].public);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_classify_wildcard_reexport_external() {
        let mut uses = Vec::new();
        let mut deps = HashSet::new();
        let span = Span::new(0, 0);
        classify_wildcard_reexport("serde", span, &mut uses, &mut deps);
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, "serde::*");
        assert!(uses[0].public);
        assert_eq!(deps.len(), 1);
        assert!(deps.iter().any(|d| d.name == "serde"));
    }

    #[test]
    fn test_classify_wildcard_reexport_std() {
        let mut uses = Vec::new();
        let mut deps = HashSet::new();
        let span = Span::new(0, 0);
        classify_wildcard_reexport("std/collections", span, &mut uses, &mut deps);
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, "std::collections::*");
        assert!(uses[0].public);
        assert!(deps.is_empty());
    }
}
