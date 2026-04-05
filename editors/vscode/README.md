# RustScript for Visual Studio Code

Language support for [RustScript](https://github.com/rustscript/rsc) — Write TypeScript, Ship Rust.

## Features

- **Syntax highlighting** for `.rts` files with full coverage of RustScript keywords, types, and expressions
- **LSP integration** via the `rustscript lsp` language server for diagnostics, completions, and go-to-definition
- **Bracket matching** and auto-closing for all bracket types including angle brackets for generics
- **Comment toggling** with `//` line comments and `/* */` block comments
- **Smart indentation** following brace-based block structure

## Requirements

- Visual Studio Code 1.75.0 or later
- The `rustscript` compiler installed and available on `$PATH` (for LSP features)

Install `rustscript`:

```bash
cargo install rustscript
```

Or build from source:

```bash
git clone https://github.com/rustscript/rsc.git
cd rsc
cargo install --path crates/rustscript-cli
```

## Extension Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `rustscript.serverPath` | `"rustscript"` | Path to the `rustscript` binary (or `rsc` alias) |
| `rustscript.lsp.enable` | `true` | Enable the language server |
| `rustscript.lsp.args` | `["lsp"]` | Arguments passed to start the language server |
| `rustscript.trace.server` | `"off"` | Trace LSP communication for debugging |

## Development

```bash
cd editors/vscode
npm install
npm run compile
```

To test the extension, open this directory in VS Code and press `F5` to launch an Extension Development Host.

## Packaging

```bash
npm run package
```

This produces a `.vsix` file installable via `code --install-extension rustscript-0.1.0.vsix`.

## License

Apache-2.0
