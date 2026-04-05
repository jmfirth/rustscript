# rustscript-lower

RustScript AST to Rust IR lowering with ownership inference.

Transforms parsed RustScript into idiomatic Rust IR: type mappings (`Array<T>` → `Vec<T>`), builtin method expansion (330+ methods), ownership analysis with clone insertion, class/trait generation, decorator → attribute lowering, and more.

Part of the [RustScript](https://rustscript.dev) compiler. See the [GitHub repository](https://github.com/jmfirth/rustscript) for the full project.

## License

Apache 2.0
