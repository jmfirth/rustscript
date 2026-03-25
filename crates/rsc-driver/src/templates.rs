//! Built-in project templates for `rsc init --template`.
//!
//! Templates are embedded as static strings in the binary. Each template
//! provides a `cargo.toml`, a starter `src/index.rts`, and optionally
//! a `.gitignore`.

/// A project template definition.
///
/// All fields are static strings compiled into the binary.
pub struct ProjectTemplate {
    /// The `cargo.toml` content (with `{name}` placeholder for the project name).
    pub cargo_toml: &'static str,
    /// The `src/index.rts` starter source code.
    pub index_rts: &'static str,
    /// Optional `.gitignore` content.
    pub gitignore: Option<&'static str>,
}

/// Shared `.gitignore` content for templates that include one.
const GITIGNORE: &str = "/target\n/.rsc-build\n";

/// WASM-specific `.gitignore` content (includes `pkg/` directory).
const WASM_GITIGNORE: &str = "/target\n/.rsc-build\n/pkg\n";

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
        cargo_toml: concat!(
            "[package]\n",
            "name = \"{name}\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2024\"\n",
            "\n",
            "[dependencies]\n",
            "clap = { version = \"4\", features = [\"derive\"] }\n",
            "\n",
            "[workspace]\n",
        ),
        index_rts: concat!(
            "// A CLI application built with RustScript\n",
            "// Run: rsc run -- --name World\n",
            "\n",
            "import { Parser, command, Arg } from \"clap\";\n",
            "\n",
            "function main() {\n",
            "  const name = \"World\";\n",
            "  console.log(`Hello, ${name}! Welcome to RustScript.`);\n",
            "  console.log(\"Edit src/index.rts to get started.\");\n",
            "}\n",
        ),
        gitignore: Some(GITIGNORE),
    }
}

/// Web server template with axum, tokio, and serde dependencies.
fn web_server_template() -> ProjectTemplate {
    ProjectTemplate {
        cargo_toml: concat!(
            "[package]\n",
            "name = \"{name}\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2024\"\n",
            "\n",
            "[dependencies]\n",
            "axum = \"0.8\"\n",
            "tokio = { version = \"1\", features = [\"full\"] }\n",
            "serde = { version = \"1\", features = [\"derive\"] }\n",
            "serde_json = \"1\"\n",
            "\n",
            "[workspace]\n",
        ),
        index_rts: concat!(
            "// A web server built with RustScript\n",
            "// Run: rsc run\n",
            "// Then visit: http://localhost:3000\n",
            "\n",
            "async function main() {\n",
            "  console.log(\"Server starting on http://localhost:3000\");\n",
            "  console.log(\"Edit src/index.rts to add routes.\");\n",
            "}\n",
        ),
        gitignore: Some(GITIGNORE),
    }
}

/// WASM module template with wasm-bindgen dependency.
fn wasm_template() -> ProjectTemplate {
    ProjectTemplate {
        cargo_toml: concat!(
            "[package]\n",
            "name = \"{name}\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2024\"\n",
            "\n",
            "[lib]\n",
            "crate-type = [\"cdylib\"]\n",
            "\n",
            "[dependencies]\n",
            "wasm-bindgen = \"0.2\"\n",
            "\n",
            "[workspace]\n",
        ),
        index_rts: concat!(
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
    fn test_cli_template_cargo_toml_has_placeholder() {
        let t = get_template("cli").unwrap();
        assert!(t.cargo_toml.contains("{name}"));
    }

    #[test]
    fn test_cli_template_cargo_toml_has_clap() {
        let t = get_template("cli").unwrap();
        assert!(
            t.cargo_toml
                .contains("clap = { version = \"4\", features = [\"derive\"] }")
        );
    }

    #[test]
    fn test_web_server_template_cargo_toml_has_deps() {
        let t = get_template("web-server").unwrap();
        assert!(t.cargo_toml.contains("axum"));
        assert!(t.cargo_toml.contains("tokio"));
        assert!(t.cargo_toml.contains("serde"));
        assert!(t.cargo_toml.contains("serde_json"));
    }

    #[test]
    fn test_wasm_template_cargo_toml_has_lib_section() {
        let t = get_template("wasm").unwrap();
        assert!(t.cargo_toml.contains("[lib]"));
        assert!(t.cargo_toml.contains("crate-type = [\"cdylib\"]"));
        assert!(t.cargo_toml.contains("wasm-bindgen"));
    }

    #[test]
    fn test_cli_template_has_gitignore() {
        let t = get_template("cli").unwrap();
        assert!(t.gitignore.is_some());
        let gi = t.gitignore.unwrap();
        assert!(gi.contains(".rsc-build"));
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
        assert!(t.index_rts.contains("async function main()"));
    }

    #[test]
    fn test_cli_template_has_main() {
        let t = get_template("cli").unwrap();
        assert!(t.index_rts.contains("function main()"));
    }

    #[test]
    fn test_wasm_template_has_greet_and_main() {
        let t = get_template("wasm").unwrap();
        assert!(t.index_rts.contains("function greet("));
        assert!(t.index_rts.contains("function main()"));
    }
}
