# rustscript-driver

Compilation pipeline orchestration and Cargo integration for RustScript.

Coordinates the full compilation pipeline (parse → typecheck → lower → emit), manages project structure (`rustscript.json` manifest, Cargo.toml merge), invokes Cargo for building, and translates rustc errors back to RustScript source positions.

Part of the [RustScript](https://rustscript.dev) compiler. See the [GitHub repository](https://github.com/jmfirth/rustscript) for the full project.

## License

Apache 2.0
