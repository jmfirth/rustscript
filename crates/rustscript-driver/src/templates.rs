//! Built-in project templates for `rsc init --template`.
//!
//! Templates are embedded as static strings in the binary. Each template
//! provides a `rustscript.json` manifest, a starter `src/main.rts`, and
//! optionally a `.gitignore`.

use crate::manifest::{DepSpec, DetailedDep, Manifest};

/// A project template definition.
///
/// All fields are static — templates provide starter source code, dependency
/// lists, and optional gitignore content.
pub struct ProjectTemplate {
    /// The `src/main.rts` starter source code.
    pub main_rts: &'static str,
    /// Optional `.gitignore` content.
    pub gitignore: Option<&'static str>,
    /// Function to build the manifest with template-specific dependencies.
    pub build_manifest: fn(name: &str) -> Manifest,
}

/// Shared `.gitignore` content for templates that include one.
const GITIGNORE: &str = "/target\n/src/*.rs\n";

/// WASM-specific `.gitignore` content (includes `pkg/` directory).
const WASM_GITIGNORE: &str = "/target\n/src/*.rs\n/pkg\n";

/// Look up a template by name.
///
/// Returns `None` if the template name is not recognized.
pub fn get_template(name: &str) -> Option<ProjectTemplate> {
    match name {
        "cli" => Some(cli_template()),
        "web-server" => Some(web_server_template()),
        "wasm" => Some(wasm_template()),
        _ => None,
    }
}

/// CLI application template with clap dependency.
fn cli_template() -> ProjectTemplate {
    ProjectTemplate {
        main_rts: concat!(
            "// A CLI application built with RustScript\n",
            "// Run: rsc run -- --name World\n",
            "\n",
            "import { Parser, command, Arg } from \"clap\";\n",
            "\n",
            "function main() {\n",
            "  const name = \"World\";\n",
            "  console.log(`Hello, ${name}! Welcome to RustScript.`);\n",
            "  console.log(\"Edit src/main.rts to get started.\");\n",
            "}\n",
        ),
        gitignore: Some(GITIGNORE),
        build_manifest: |name| {
            let mut manifest = crate::manifest::new_manifest(name);
            manifest.dependencies.insert(
                "clap".to_owned(),
                DepSpec::Detailed(DetailedDep {
                    version: "4".to_owned(),
                    features: vec!["derive".to_owned()],
                }),
            );
            manifest
        },
    }
}

/// Web server template with axum, tokio, and serde dependencies.
fn web_server_template() -> ProjectTemplate {
    ProjectTemplate {
        main_rts: concat!(
            "// A web server built with RustScript\n",
            "// Run: rsc run\n",
            "// Then visit: http://localhost:3000\n",
            "\n",
            "async function main() {\n",
            "  console.log(\"Server starting on http://localhost:3000\");\n",
            "  console.log(\"Edit src/main.rts to add routes.\");\n",
            "}\n",
        ),
        gitignore: Some(GITIGNORE),
        build_manifest: |name| {
            let mut manifest = crate::manifest::new_manifest(name);
            manifest
                .dependencies
                .insert("axum".to_owned(), DepSpec::Simple("0.8".to_owned()));
            manifest.dependencies.insert(
                "tokio".to_owned(),
                DepSpec::Detailed(DetailedDep {
                    version: "1".to_owned(),
                    features: vec!["full".to_owned()],
                }),
            );
            manifest.dependencies.insert(
                "serde".to_owned(),
                DepSpec::Detailed(DetailedDep {
                    version: "1".to_owned(),
                    features: vec!["derive".to_owned()],
                }),
            );
            manifest
                .dependencies
                .insert("serde_json".to_owned(), DepSpec::Simple("1".to_owned()));
            manifest
        },
    }
}

/// WASM module template with wasm-bindgen dependency.
fn wasm_template() -> ProjectTemplate {
    ProjectTemplate {
        main_rts: concat!(
            "// A WASM module built with RustScript\n",
            "// Build: rsc build --target wasm32-unknown-unknown\n",
            "// Note: WASM target support is coming in a future release.\n",
            "\n",
            "function greet(name: string): string {\n",
            "  return `Hello, ${name}!`;\n",
            "}\n",
            "\n",
            "function main() {\n",
            "  console.log(greet(\"World\"));\n",
            "}\n",
        ),
        gitignore: Some(WASM_GITIGNORE),
        build_manifest: |name| {
            let mut manifest = crate::manifest::new_manifest(name);
            manifest
                .dependencies
                .insert("wasm-bindgen".to_owned(), DepSpec::Simple("0.2".to_owned()));
            manifest
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_template_cli_returns_some() {
        assert!(get_template("cli").is_some());
    }

    #[test]
    fn test_get_template_web_server_returns_some() {
        assert!(get_template("web-server").is_some());
    }

    #[test]
    fn test_get_template_wasm_returns_some() {
        assert!(get_template("wasm").is_some());
    }

    #[test]
    fn test_get_template_invalid_returns_none() {
        assert!(get_template("invalid").is_none());
        assert!(get_template("").is_none());
        assert!(get_template("react").is_none());
    }

    #[test]
    fn test_cli_template_manifest_has_clap() {
        let t = get_template("cli").unwrap();
        let manifest = (t.build_manifest)("test-cli");
        assert!(manifest.dependencies.contains_key("clap"));
        let clap = &manifest.dependencies["clap"];
        assert_eq!(clap.version(), "4");
        assert!(clap.features().contains(&"derive".to_owned()));
    }

    #[test]
    fn test_web_server_template_manifest_has_deps() {
        let t = get_template("web-server").unwrap();
        let manifest = (t.build_manifest)("test-web");
        assert!(manifest.dependencies.contains_key("axum"));
        assert!(manifest.dependencies.contains_key("tokio"));
        assert!(manifest.dependencies.contains_key("serde"));
        assert!(manifest.dependencies.contains_key("serde_json"));
    }

    #[test]
    fn test_wasm_template_manifest_has_wasm_bindgen() {
        let t = get_template("wasm").unwrap();
        let manifest = (t.build_manifest)("test-wasm");
        assert!(manifest.dependencies.contains_key("wasm-bindgen"));
    }

    #[test]
    fn test_cli_template_has_gitignore() {
        let t = get_template("cli").unwrap();
        assert!(t.gitignore.is_some());
        let gi = t.gitignore.unwrap();
        assert!(gi.contains("/src/*.rs"));
        assert!(gi.contains("/target"));
    }

    #[test]
    fn test_web_server_template_has_gitignore() {
        let t = get_template("web-server").unwrap();
        assert!(t.gitignore.is_some());
    }

    #[test]
    fn test_wasm_template_has_gitignore_with_pkg() {
        let t = get_template("wasm").unwrap();
        assert!(t.gitignore.is_some());
        let gi = t.gitignore.unwrap();
        assert!(gi.contains("/pkg"));
    }

    #[test]
    fn test_web_server_template_has_async_main() {
        let t = get_template("web-server").unwrap();
        assert!(t.main_rts.contains("async function main()"));
    }

    #[test]
    fn test_cli_template_has_main() {
        let t = get_template("cli").unwrap();
        assert!(t.main_rts.contains("function main()"));
    }

    #[test]
    fn test_wasm_template_has_greet_and_main() {
        let t = get_template("wasm").unwrap();
        assert!(t.main_rts.contains("function greet("));
        assert!(t.main_rts.contains("function main()"));
    }

    #[test]
    fn test_gitignore_does_not_contain_rsc_build() {
        // New in-place compilation: gitignore should NOT reference .rsc-build
        assert!(!GITIGNORE.contains(".rsc-build"));
        assert!(!WASM_GITIGNORE.contains(".rsc-build"));
    }

    #[test]
    fn test_gitignore_contains_src_rs() {
        assert!(GITIGNORE.contains("/src/*.rs"));
        assert!(WASM_GITIGNORE.contains("/src/*.rs"));
    }

    #[test]
    fn test_cli_template_uses_main_rts() {
        let t = get_template("cli").unwrap();
        assert!(t.main_rts.contains("main.rts"));
    }
}
