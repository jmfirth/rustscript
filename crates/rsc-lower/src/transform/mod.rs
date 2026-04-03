//! AST-to-IR transformation.
//!
//! Consumes the `RustScript` AST and produces Rust IR, using the types,
//! ownership, and builtins modules for type resolution, clone insertion,
//! and builtin method lowering respectively.

mod async_lower;
mod class_lower;
mod expr_lower;
mod fn_lower;
mod import_lower;
mod match_lower;
mod stdlib_deps;
mod stmt_lower;
mod test_lower;
mod type_lower;
mod union_lower;
mod use_collector;

// Re-export free functions from sub-modules so sibling modules can import them
// via `use super::{...}` as before the split.
use type_lower::{capitalize_first, collect_generic_param_names, lower_type_params, merge_derives};

use std::collections::{HashMap, HashSet};

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::external_fn::ExternalFnInfo;
use rsc_syntax::rust_ir::{
    ParamMode, RustAttribute, RustBinaryOp, RustCompoundAssignOp, RustEnumDef, RustEnumVariant,
    RustExpr, RustFile, RustItem, RustParam, RustType, RustTypeAlias, RustUnaryOp, RustUseDecl,
};

use crate::CrateDependency;
use crate::builtins::BuiltinRegistry;
use crate::context::LoweringContext;
use crate::derive_inference;
use rsc_typeck::registry::TypeRegistry;

/// Information about a function's signature, collected in a pre-pass.
///
/// Used by call-site lowering to determine whether to insert `?`, fill in
/// default values for omitted arguments, wrap optional params, and collect
/// rest args. Also used by Tier 2 ownership inference for per-parameter
/// borrow modes.
#[derive(Debug, Clone)]
struct FnSignature {
    /// Whether this function has a `throws` annotation.
    throws: bool,
    /// Resolved parameter types for enum variant resolution at call sites.
    param_types: Vec<RustType>,
    /// Inferred parameter modes from Tier 2 borrow analysis.
    /// `None` means analysis hasn't run (e.g., external functions).
    param_modes: Option<Vec<ParamMode>>,
    /// Per-parameter flags: whether each parameter is optional.
    optional_params: Vec<bool>,
    /// Per-parameter default value expressions (lowered to `RustExpr`).
    /// `None` means no default; the caller must supply the argument or it
    /// must be an optional param (in which case `None` is filled in).
    default_values: Vec<Option<RustExpr>>,
    /// Whether the last parameter is a rest parameter (`...args`).
    has_rest_param: bool,
    /// Total parameter count (including the rest param if any).
    param_count: usize,
}

/// Map from function name to its throws signature.
type FunctionSignatureMap = HashMap<String, FnSignature>;

/// Registry of generated union types, keyed by canonical enum name.
///
/// Tracks all union types encountered during lowering to ensure each distinct
/// union produces exactly one enum definition. The canonical name is computed
/// by sorting variant names alphabetically and joining with "Or".
struct UnionRegistry {
    /// Map from canonical enum name to its variant list.
    unions: HashMap<String, Vec<(String, RustType)>>,
}

impl UnionRegistry {
    /// Create an empty union registry.
    fn new() -> Self {
        Self {
            unions: HashMap::new(),
        }
    }

    /// Register a union type if not already known.
    fn register(&mut self, name: &str, variants: &[(String, RustType)]) {
        self.unions
            .entry(name.to_owned())
            .or_insert_with(|| variants.to_vec());
    }

    /// Generate all enum definitions and From impls for registered unions.
    fn generate_items(&self) -> Vec<RustItem> {
        let mut items = Vec::new();
        // Sort by name for deterministic output
        let mut names: Vec<&str> = self.unions.keys().map(String::as_str).collect();
        names.sort_unstable();

        for name in names {
            let variants = &self.unions[name];

            // Generate enum definition with tuple variants
            let enum_variants: Vec<RustEnumVariant> = variants
                .iter()
                .map(|(variant_name, inner_ty)| RustEnumVariant {
                    name: variant_name.clone(),
                    fields: vec![],
                    tuple_types: vec![inner_ty.clone()],
                    span: None,
                })
                .collect();

            let derives = derive_inference::infer_enum_derives(&enum_variants);

            items.push(RustItem::Enum(RustEnumDef {
                public: false,
                name: name.to_owned(),
                variants: enum_variants,
                derives,
                attributes: vec![],
                doc_comment: None,
                span: None,
            }));

            // Generate From impls for each variant as raw Rust
            for (variant_name, inner_ty) in variants {
                let from_code = format!(
                    "impl From<{inner_ty}> for {name} {{\n    \
                     fn from(v: {inner_ty}) -> Self {{\n        \
                     Self::{variant_name}(v)\n    \
                     }}\n}}"
                );
                items.push(RustItem::RawRust(from_code));
            }
        }

        items
    }
}

/// The AST-to-IR transformer.
///
/// Holds the builtin registry and type registry, and drives the lowering of
/// an entire module.
pub(crate) struct Transform {
    builtins: BuiltinRegistry,
    type_registry: TypeRegistry,
    /// Function signature map for `throws` detection during lowering.
    fn_signatures: FunctionSignatureMap,
    /// When true, disables Tier 2 borrow inference (all params stay Owned).
    no_borrow_inference: bool,
    /// Registry of auto-generated union enum types encountered during lowering.
    union_registry: UnionRegistry,
    /// Names imported from other modules/crates.
    /// Used to distinguish `Type.method()` (static call) from `variable.method()`.
    imported_types: HashSet<String>,
    /// Names of classes that are used as base classes (some other class `extends` them).
    /// These classes generate a `{Name}Trait` for polymorphism.
    extended_classes: HashSet<String>,
    /// External function signatures from rustdoc JSON, keyed by
    /// `"crate::function"` or `"crate::Type::method"`.
    external_signatures: HashMap<String, ExternalFnInfo>,
    /// Map from generator function name to its iterator struct name.
    /// Used to rewrite call sites: `range(0, 5)` → `RangeIter::new(0, 5)`.
    generator_structs: HashMap<String, String>,
    /// Resolved types for type aliases, used for variadic tuple spread resolution.
    /// When `type Extended = [...Pair, bool]` is encountered, we look up `Pair`
    /// here to get its resolved `RustType::Tuple(...)` for flattening.
    type_alias_types: HashMap<String, RustType>,
    /// Names imported via `import type { ... }` declarations.
    /// These names are valid in type positions but must not be used as values.
    /// No `use` declarations are emitted for type-only imports.
    type_only_imports: HashSet<String>,
}

/// Convert a `RustScript` decorator to a Rust attribute.
///
/// Handles the special mapping `@tokio_test` → `#[tokio::test]`.
/// All other decorators map directly: `@name(args)` → `#[name(args)]`.
fn lower_decorator(decorator: &ast::Decorator) -> RustAttribute {
    // Special mapping: @tokio_test → #[tokio::test]
    let path = if decorator.name == "tokio_test" {
        "tokio::test".to_owned()
    } else {
        decorator.name.clone()
    };
    RustAttribute {
        path,
        args: decorator.args.clone(),
    }
}

/// Lower a list of decorators to Rust attributes, splitting out `@derive(...)` decorators.
///
/// Returns `(attributes, extra_derives)` where:
/// - `attributes` are non-derive attributes to add to the IR item
/// - `extra_derives` are derive macro names extracted from `@derive(...)` decorators
fn lower_decorators(decorators: &[ast::Decorator]) -> (Vec<RustAttribute>, Vec<String>) {
    let mut attributes = Vec::new();
    let mut extra_derives = Vec::new();
    for decorator in decorators {
        if decorator.name == "derive" {
            // Extract derive names from args
            if let Some(ref args) = decorator.args {
                for name in args.split(',') {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        extra_derives.push(trimmed.to_owned());
                    }
                }
            }
        } else {
            attributes.push(lower_decorator(decorator));
        }
    }
    (attributes, extra_derives)
}

impl Transform {
    /// Create a new transformer with the default builtin registry and an empty
    /// type registry.
    pub fn new(no_borrow_inference: bool) -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
            type_registry: TypeRegistry::new(),
            fn_signatures: FunctionSignatureMap::new(),
            no_borrow_inference,
            union_registry: UnionRegistry::new(),
            imported_types: HashSet::new(),
            extended_classes: HashSet::new(),
            external_signatures: HashMap::new(),
            generator_structs: HashMap::new(),
            type_alias_types: HashMap::new(),
            type_only_imports: HashSet::new(),
        }
    }

    /// Set external function signatures from rustdoc data.
    ///
    /// Called by the driver after loading rustdoc JSON for imported crates.
    /// The keys are qualified names like `"axum::Router::route"` or `"serde_json::to_string"`.
    pub fn set_external_signatures(&mut self, sigs: HashMap<String, ExternalFnInfo>) {
        self.external_signatures = sigs;
    }

    /// Check whether an identifier refers to a type rather than a variable.
    ///
    /// Returns `true` if the name was imported, registered in the type registry,
    /// or starts with an uppercase letter (`PascalCase` convention for Rust types)
    /// and is not declared as a variable in the current scope.
    fn is_type_name(&self, name: &str, ctx: &LoweringContext) -> bool {
        // If it's a known variable, it's not a type
        if ctx.lookup_variable(name).is_some() {
            return false;
        }
        // Check imported names and type registry
        if self.imported_types.contains(name) {
            return true;
        }
        if self.type_registry.has_type(name) {
            return true;
        }
        // PascalCase heuristic: starts with uppercase letter
        name.starts_with(|c: char| c.is_ascii_uppercase())
    }

    /// Check whether an identifier was imported via `import type { ... }`.
    ///
    /// Type-only imports are valid in type positions but must not be used as
    /// values (e.g., in function calls, property access, or variable bindings).
    fn is_type_only_import(&self, name: &str) -> bool {
        self.type_only_imports.contains(name)
    }

    /// Lower a complete `RustScript` module to a Rust file.
    ///
    /// Performs a pre-pass to register all type definitions, then lowers
    /// each item. Returns the Rust IR, diagnostics, and any external crate
    /// dependencies discovered from import statements.
    #[allow(clippy::too_many_lines)]
    // Module lowering orchestrates type registration, item lowering, and use collection
    #[allow(clippy::type_complexity)]
    // Module lowering returns multiple output channels; a named struct would be heavier
    #[allow(clippy::type_complexity)]
    // Returns a tuple of (IR, diagnostics, crate deps, needs_async, needs_futures,
    // needs_serde_json, needs_rand, needs_serde, needs_regex) — complex but each element is
    // independently needed by the driver.
    pub fn lower_module(
        &mut self,
        module: &ast::Module,
    ) -> (
        RustFile,
        Vec<Diagnostic>,
        HashSet<CrateDependency>,
        bool,
        bool,
        bool,
        bool,
        bool,
        bool,
    ) {
        let mut ctx = LoweringContext::new();

        // Pre-pass: identify classes used as base classes for inheritance.
        // These classes will generate a `{Name}Trait` for polymorphism.
        for item in &module.items {
            if let ast::ItemKind::Class(cls) = &item.kind
                && let Some(ref base) = cls.extends
            {
                self.extended_classes.insert(base.name.clone());
            }
            // Also check class expressions inside const bindings
            if let ast::ItemKind::Const(decl) = &item.kind
                && let ast::ExprKind::ClassExpr(cls) = &decl.init.kind
                && let Some(ref base) = cls.extends
            {
                self.extended_classes.insert(base.name.clone());
            }
        }

        // Pre-pass: register all type definitions so they can be resolved
        // during function lowering.
        for item in &module.items {
            match &item.kind {
                ast::ItemKind::TypeDef(td) => self.register_type_def(td, &mut ctx),
                ast::ItemKind::EnumDef(ed) => self.register_enum_def(ed, &mut ctx),
                ast::ItemKind::Interface(iface) => self.register_interface_def(iface, &mut ctx),
                ast::ItemKind::Class(cls) => {
                    if cls.is_abstract {
                        // Register abstract class as an interface (trait)
                        self.register_abstract_class_as_interface(cls);
                    } else {
                        self.register_class_def(cls, &mut ctx);
                    }
                }
                ast::ItemKind::Const(decl) => {
                    // Register class expressions inside const bindings
                    if let ast::ExprKind::ClassExpr(class_def) = &decl.init.kind {
                        let mut cls = class_def.clone();
                        if cls.name.name == "__AnonymousClass" {
                            cls.name = decl.name.clone();
                        }
                        self.register_class_def(&cls, &mut ctx);
                    }
                }
                ast::ItemKind::Function(_)
                | ast::ItemKind::Import(_)
                | ast::ItemKind::ReExport(_)
                | ast::ItemKind::WildcardReExport(_)
                | ast::ItemKind::RustBlock(_)
                | ast::ItemKind::TestBlock(_) => {}
            }
        }

        // Pre-pass: for concrete classes that are extended, register their
        // methods as interface methods so derived classes can generate trait impls.
        for item in &module.items {
            if let ast::ItemKind::Class(cls) = &item.kind
                && !cls.is_abstract
                && self.extended_classes.contains(&cls.name.name)
            {
                self.register_concrete_class_as_interface(cls, &mut ctx);
            }
        }

        // Pre-pass: register generator functions so call sites can be rewritten
        for item in &module.items {
            if let ast::ItemKind::Function(f) = &item.kind
                && f.is_generator
            {
                let struct_name = fn_lower::generator_struct_name(&f.name.name);
                self.generator_structs
                    .insert(f.name.name.clone(), struct_name);
            }
        }

        // Pre-pass: collect function signatures for throws detection
        for item in &module.items {
            if let ast::ItemKind::Function(f) = &item.kind {
                self.register_fn_signature(f, &mut ctx);
            }
        }

        // Register external function signatures from rustdoc data.
        // Converts ExternalFnInfo into FnSignature entries so call-site lowering
        // can use param modes and throws detection for external crate functions.
        self.register_external_signatures();

        // Pre-pass: scan all type annotations and register any general union types
        // so their enum definitions will be generated.
        self.register_union_types_in_module(module);

        let mut items: Vec<RustItem> = Vec::new();
        let mut import_uses: Vec<RustUseDecl> = Vec::new();
        let mut crate_deps: HashSet<CrateDependency> = HashSet::new();
        let mut needs_async_runtime = async_lower::module_needs_async_runtime(module);

        for item in &module.items {
            let exported = item.exported;
            let (decorator_attrs, decorator_derives) = lower_decorators(&item.decorators);
            let items_before = items.len();
            match &item.kind {
                ast::ItemKind::Function(f) => {
                    if f.is_generator {
                        let gen_items = self.lower_generator(f, &mut ctx, exported);
                        items.extend(gen_items);
                    } else {
                        if f.is_async {
                            needs_async_runtime = true;
                        }
                        let mut lowered = self.lower_fn(f, &mut ctx);
                        lowered.public = exported;
                        lowered.attributes.extend(decorator_attrs.iter().cloned());
                        items.push(RustItem::Function(lowered));
                    }
                }
                ast::ItemKind::TypeDef(td) => {
                    // Utility type alias: type X = Partial<Y>, Record<K,V>, etc.
                    if let Some(ref alias) = td.type_alias {
                        if Self::identify_utility_type(alias).is_some() {
                            let mut lowered = self.lower_utility_type(td, alias, &mut ctx);
                            match &mut lowered {
                                RustItem::Struct(s) => s.public = exported,
                                RustItem::TypeAlias(a) => a.public = exported,
                                _ => {}
                            }
                            items.push(lowered);
                        } else if matches!(alias.kind, ast::TypeKind::MappedType { .. }) {
                            // Mapped type: type X = { [K in keyof T]: V }
                            let mut lowered = self.lower_mapped_type(td, alias, &mut ctx);
                            lowered.public = exported;
                            items.push(RustItem::Struct(lowered));
                        } else if let ast::TypeKind::KeyOf(ref inner) = alias.kind {
                            // keyof T — generate a simple enum with field names as variants
                            if let Some(enum_def) =
                                self.lower_keyof_type(td, inner, exported, &mut ctx)
                            {
                                items.push(RustItem::Enum(enum_def));
                            }
                        } else if let ast::TypeKind::TypeOf(ref ident) = alias.kind {
                            // typeof x — resolve to the variable's declared type
                            if let Some(var_info) = ctx.lookup_variable(&ident.name) {
                                let rust_ty = var_info.ty.clone();
                                items.push(RustItem::TypeAlias(RustTypeAlias {
                                    public: exported,
                                    name: td.name.name.clone(),
                                    ty: rust_ty,
                                    span: Some(td.span),
                                }));
                            } else {
                                ctx.emit_diagnostic(Diagnostic::error(format!(
                                    "`typeof` refers to unknown variable `{}`",
                                    ident.name
                                )));
                            }
                        } else if let ast::TypeKind::Intersection(ref members) = alias.kind {
                            // Intersection type: type X = A & B
                            // Merge fields from all constituent struct types into one struct.
                            let merged = self.collect_intersection_fields(members, td, &mut ctx);
                            if !merged.is_empty() {
                                let mut lowered = self.lower_intersection_struct(td, &merged);
                                lowered.public = exported;
                                items.push(RustItem::Struct(lowered));
                            } else {
                                // Fallback: produce a type alias to the first member
                                let rust_ty = self.resolve_type_alias_body(alias, td, &mut ctx);
                                self.type_alias_types
                                    .insert(td.name.name.clone(), rust_ty.clone());
                                items.push(RustItem::TypeAlias(RustTypeAlias {
                                    public: exported,
                                    name: td.name.name.clone(),
                                    ty: rust_ty,
                                    span: Some(td.span),
                                }));
                            }
                        } else {
                            // Non-utility type alias: type X = SomeType
                            let rust_ty = self.resolve_type_alias_body(alias, td, &mut ctx);
                            self.type_alias_types
                                .insert(td.name.name.clone(), rust_ty.clone());
                            items.push(RustItem::TypeAlias(RustTypeAlias {
                                public: exported,
                                name: td.name.name.clone(),
                                ty: rust_ty,
                                span: Some(td.span),
                            }));
                        }
                    } else if td.fields.is_empty() && td.index_signature.is_some() {
                        // Pure index signature (no regular fields) → type alias to HashMap
                        let mut alias = self.lower_index_signature_type_alias(td, &mut ctx);
                        alias.public = exported;
                        items.push(RustItem::TypeAlias(alias));
                    } else {
                        let mut lowered = self.lower_type_def(td, &mut ctx);
                        lowered.public = exported;
                        items.push(RustItem::Struct(lowered));
                    }
                }
                ast::ItemKind::EnumDef(ed) => {
                    let mut lowered = self.lower_enum_def(ed, &mut ctx);
                    lowered.public = exported;
                    items.push(RustItem::Enum(lowered));
                }
                ast::ItemKind::Interface(iface) => {
                    let mut lowered = self.lower_interface_def(iface, &mut ctx);
                    lowered.public = exported;
                    items.push(RustItem::Trait(lowered));
                }
                ast::ItemKind::Class(cls) => {
                    let lowered = self.lower_class_def(cls, exported, &mut ctx);
                    items.extend(lowered);
                }
                ast::ItemKind::Import(import) => {
                    if import.is_type_only {
                        // Type-only imports (`import type { T } from "mod"`) do not
                        // generate `use` declarations. The names are still tracked
                        // so they can be recognized as types in annotations, but
                        // using them as values will produce a diagnostic error.
                        for name in &import.names {
                            self.imported_types.insert(name.name.clone());
                            self.type_only_imports.insert(name.name.clone());
                        }
                    } else {
                        import_lower::classify_import(
                            &import.source.value,
                            &import.names,
                            false,
                            import.span,
                            &mut import_uses,
                            &mut crate_deps,
                        );
                        // Track imported names so method calls on types can be
                        // recognized as static calls (`Type.method()` → `Type::method()`).
                        for name in &import.names {
                            self.imported_types.insert(name.name.clone());
                        }
                    }
                }
                ast::ItemKind::ReExport(reexport) => {
                    import_lower::classify_import(
                        &reexport.source.value,
                        &reexport.names,
                        true,
                        reexport.span,
                        &mut import_uses,
                        &mut crate_deps,
                    );
                    // Track re-exported names for static call recognition.
                    for name in &reexport.names {
                        self.imported_types.insert(name.name.clone());
                    }
                }
                ast::ItemKind::WildcardReExport(reexport) => {
                    import_lower::classify_wildcard_reexport(
                        &reexport.source.value,
                        reexport.span,
                        &mut import_uses,
                        &mut crate_deps,
                    );
                }
                ast::ItemKind::RustBlock(rb) => {
                    items.push(RustItem::RawRust(rb.code.clone()));
                }
                ast::ItemKind::Const(decl) => {
                    // If the initializer is a class expression, hoist it as a
                    // class definition using the variable name as the class name.
                    if let ast::ExprKind::ClassExpr(class_def) = &decl.init.kind {
                        let mut cls = class_def.clone();
                        // Anonymous class expressions get the binding variable name
                        if cls.name.name == "__AnonymousClass" {
                            cls.name = decl.name.clone();
                        }
                        let lowered = self.lower_class_def(&cls, exported, &mut ctx);
                        items.extend(lowered);
                    } else {
                        let lowered = self.lower_top_level_const(decl, exported, &mut ctx);
                        items.push(lowered);
                    }
                }
                // Test blocks are handled separately by collect_test_module
                ast::ItemKind::TestBlock(_) => {}
            }

            // Apply decorator-derived attributes and derives to the first newly-lowered
            // item (the primary lowered item). Function attributes are already applied
            // above in the Function arm.
            if (!decorator_attrs.is_empty() || !decorator_derives.is_empty())
                && let Some(new_item) = items[items_before..].first_mut()
            {
                match new_item {
                    RustItem::Struct(s) => {
                        s.attributes.extend(decorator_attrs);
                        for d in &decorator_derives {
                            if !s.derives.contains(d) {
                                s.derives.push(d.clone());
                            }
                        }
                    }
                    RustItem::Enum(e) => {
                        e.attributes.extend(decorator_attrs);
                        for d in &decorator_derives {
                            if !e.derives.contains(d) {
                                e.derives.push(d.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Generate auto-generated union enum definitions and From impls.
        // These are prepended before user items so they are available to all functions.
        let union_items = self.union_registry.generate_items();
        if !union_items.is_empty() {
            let mut all_items = union_items;
            all_items.append(&mut items);
            items = all_items;
        }

        // Collect use declarations by scanning generated items for HashMap/HashSet usage
        let mut uses = use_collector::collect_use_declarations(&items);
        // Prepend import-derived use declarations
        import_uses.append(&mut uses);
        // Deduplicate use declarations by path to avoid duplicate imports
        // (e.g., when an import and new X() both generate a use for the same type)
        let mut seen_paths = std::collections::HashSet::new();
        import_uses.retain(|u| seen_paths.insert(u.path.clone()));
        let mut uses = import_uses;

        // Detect whether the futures crate is needed (for await, Promise.any).
        // Check if any use declaration references futures::.
        let needs_futures_crate = uses.iter().any(|u| u.path.starts_with("futures::"))
            || async_lower::module_needs_futures_crate(module);

        // Collect test blocks (test(), describe(), it()) from top-level items
        let test_module = self.collect_test_module(module, &mut ctx);

        // Detect whether serde_json or rand crates are needed by scanning the AST
        // for JSON.stringify/parse and Math.random calls.
        let needs_serde_json = stdlib_deps::module_needs_serde_json(module);
        let needs_rand = stdlib_deps::module_needs_rand(module);

        // Detect whether regex crate is needed by scanning the AST for new RegExp().
        let needs_regex = stdlib_deps::module_needs_regex(module);

        // Detect whether serde derive crate is needed by scanning explicit derives
        // for Serialize or Deserialize. If so, add use declarations.
        let needs_serde = module_needs_serde_derives(module);
        if needs_serde {
            let serde_derives = collect_serde_derive_names(module);
            for name in serde_derives {
                uses.push(RustUseDecl {
                    path: format!("serde::{name}"),
                    public: false,
                    span: None,
                });
            }
        }

        let diagnostics = ctx.into_diagnostics();
        (
            RustFile {
                uses,
                mod_decls: Vec::new(),
                items,
                test_module,
            },
            diagnostics,
            crate_deps,
            needs_async_runtime,
            needs_futures_crate,
            needs_serde_json,
            needs_rand,
            needs_serde,
            needs_regex,
        )
    }
}

/// Map a `RustScript` binary operator to a Rust binary operator.
fn lower_binary_op(op: ast::BinaryOp) -> RustBinaryOp {
    match op {
        ast::BinaryOp::Add => RustBinaryOp::Add,
        ast::BinaryOp::Sub => RustBinaryOp::Sub,
        ast::BinaryOp::Mul => RustBinaryOp::Mul,
        ast::BinaryOp::Div => RustBinaryOp::Div,
        ast::BinaryOp::Mod => RustBinaryOp::Rem,
        ast::BinaryOp::Eq => RustBinaryOp::Eq,
        ast::BinaryOp::Ne => RustBinaryOp::Ne,
        ast::BinaryOp::Lt => RustBinaryOp::Lt,
        ast::BinaryOp::Gt => RustBinaryOp::Gt,
        ast::BinaryOp::Le => RustBinaryOp::Le,
        ast::BinaryOp::Ge => RustBinaryOp::Ge,
        ast::BinaryOp::And => RustBinaryOp::And,
        ast::BinaryOp::Or => RustBinaryOp::Or,
        ast::BinaryOp::BitAnd => RustBinaryOp::BitAnd,
        ast::BinaryOp::BitOr => RustBinaryOp::BitOr,
        ast::BinaryOp::BitXor => RustBinaryOp::BitXor,
        ast::BinaryOp::Shl => RustBinaryOp::Shl,
        ast::BinaryOp::Shr => RustBinaryOp::Shr,
        // Pow is handled specially in expr_lower, not via this mapping.
        ast::BinaryOp::Pow => unreachable!("Pow is handled specially in expr_lower"),
        // In is handled specially in expr_lower as a method call, not via this mapping.
        ast::BinaryOp::In => unreachable!("In is handled specially in expr_lower"),
    }
}

/// Map a `RustScript` unary operator to a Rust unary operator.
fn lower_unary_op(op: ast::UnaryOp) -> RustUnaryOp {
    match op {
        ast::UnaryOp::Neg => RustUnaryOp::Neg,
        ast::UnaryOp::Not => RustUnaryOp::Not,
        ast::UnaryOp::BitNot => RustUnaryOp::BitNot,
    }
}

/// Check if the expression is a literal whose default inferred type matches `ty`.
///
/// When this returns `true`, the type annotation can be omitted on the `let`
/// binding because Rust will infer the same type.
/// Check whether a collection type's element type is `Copy`.
///
/// For `Vec<i32>` (represented as `Generic(Named("Vec"), [I32])`), returns true.
/// For `Vec<String>` or non-collection types, returns false.
/// Infer the element type for an `as const` array to determine the static slice type.
///
/// For string arrays: `&str` (string literals are `&'static str` in const context).
/// For integer arrays: `i64` (the default integer type).
/// For float arrays: `f64`.
/// For boolean arrays: `bool`.
/// Falls back to `i64` for empty or mixed arrays.
fn infer_as_const_slice_element_type(array_expr: &ast::Expr) -> RustType {
    if let ast::ExprKind::ArrayLit(elements) = &array_expr.kind
        && let Some(ast::ArrayElement::Expr(first)) = elements.first()
    {
        return match &first.kind {
            ast::ExprKind::StringLit(_) => RustType::StrRef,
            ast::ExprKind::FloatLit(_) => RustType::F64,
            ast::ExprKind::BoolLit(_) => RustType::Bool,
            _ => RustType::I64,
        };
    }
    RustType::I64
}

/// Extract the element type from a generic collection type (e.g., `Vec<i32>` → `i32`).
///
/// Returns the first type argument of a `Generic` type, or `None` if the type
/// is not generic or has no type arguments.
fn extract_element_type(ty: &RustType) -> Option<&RustType> {
    if let RustType::Generic(_, args) = ty {
        args.first()
    } else {
        None
    }
}

fn element_type_is_copy(ty: &RustType) -> bool {
    if let RustType::Generic(_, args) = ty
        && let Some(elem) = args.first()
    {
        return matches!(
            elem,
            RustType::I8
                | RustType::I16
                | RustType::I32
                | RustType::I64
                | RustType::U8
                | RustType::U16
                | RustType::U32
                | RustType::U64
                | RustType::F32
                | RustType::F64
                | RustType::Bool
        );
    }
    false
}

/// Extract the base type name from a `RustType`.
///
/// Returns the name for `Named("Foo")` and `Generic(Named("Foo"), _)`.
fn extract_named_type(ty: &RustType) -> Option<String> {
    match ty {
        RustType::Named(name) => Some(name.clone()),
        RustType::Generic(base, args) => {
            if let RustType::Named(name) = base.as_ref() {
                // For collection types (Vec, HashSet), extract the element type name
                // so struct literal context resolves to the element type, not the container.
                // e.g., Vec<Todo> → "Todo", not "Vec"
                if (name == "Vec" || name == "HashSet") && args.len() == 1 {
                    return extract_named_type(&args[0]);
                }
                Some(name.clone())
            } else {
                None
            }
        }
        RustType::Option(inner) => extract_named_type(inner),
        RustType::Result(ok, _) => extract_named_type(ok),
        _ => None,
    }
}

/// Capitalize the first letter of a string.

/// Check whether any type, enum, or class in the module uses `Serialize` or
/// `Deserialize` in its explicit derives, which requires the serde crate.
fn module_needs_serde_derives(module: &ast::Module) -> bool {
    fn has_serde_derive(derives: &[ast::Ident]) -> bool {
        derives
            .iter()
            .any(|d| d.name == "Serialize" || d.name == "Deserialize")
    }

    module.items.iter().any(|item| match &item.kind {
        ast::ItemKind::TypeDef(td) => has_serde_derive(&td.derives),
        ast::ItemKind::EnumDef(ed) => has_serde_derive(&ed.derives),
        ast::ItemKind::Class(cls) => has_serde_derive(&cls.derives),
        _ => false,
    })
}

/// Collect unique serde derive names (Serialize, Deserialize) from the module.
fn collect_serde_derive_names(module: &ast::Module) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for item in &module.items {
        let derives: &[ast::Ident] = match &item.kind {
            ast::ItemKind::TypeDef(td) => &td.derives,
            ast::ItemKind::EnumDef(ed) => &ed.derives,
            ast::ItemKind::Class(cls) => &cls.derives,
            _ => continue,
        };
        for d in derives {
            if d.name == "Serialize" || d.name == "Deserialize" {
                names.insert(d.name.clone());
            }
        }
    }
    names.into_iter().collect()
}

fn is_default_literal_type(expr: &ast::Expr, ty: &RustType) -> bool {
    match &expr.kind {
        ast::ExprKind::IntLit(_) => *ty == RustType::I64,
        ast::ExprKind::FloatLit(_) => *ty == RustType::F64,
        ast::ExprKind::BoolLit(_) => *ty == RustType::Bool,
        ast::ExprKind::StringLit(_) | ast::ExprKind::TemplateLit(_) => *ty == RustType::String,
        ast::ExprKind::NullLit => matches!(ty, RustType::Option(_)),
        _ => false,
    }
}

/// Detect the compound assignment pattern `x = x op rhs`.
///
/// If the value expression is `Binary(op, Ident(name), rhs)` where `name`
/// matches the assignment target, returns the compound operator and the rhs.
fn detect_compound_assign<'a>(
    target: &str,
    value: &'a ast::Expr,
) -> Option<(RustCompoundAssignOp, &'a ast::Expr)> {
    if let ast::ExprKind::Binary(bin) = &value.kind
        && let ast::ExprKind::Ident(ident) = &bin.left.kind
        && ident.name == target
    {
        let compound_op = match bin.op {
            ast::BinaryOp::Add => RustCompoundAssignOp::AddAssign,
            ast::BinaryOp::Sub => RustCompoundAssignOp::SubAssign,
            ast::BinaryOp::Mul => RustCompoundAssignOp::MulAssign,
            ast::BinaryOp::Div => RustCompoundAssignOp::DivAssign,
            ast::BinaryOp::Mod => RustCompoundAssignOp::RemAssign,
            _ => return None,
        };
        return Some((compound_op, &bin.right));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::ast::*;
    use rsc_syntax::rust_ir::{RustExprKind, RustSelfParam, RustStmt};
    use rsc_syntax::span::Span;

    fn span(start: u32, end: u32) -> Span {
        Span::new(start, end)
    }

    fn ident(name: &str, start: u32, end: u32) -> Ident {
        Ident {
            name: name.to_owned(),
            span: span(start, end),
        }
    }

    fn int_expr(value: i64, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::IntLit(value),
            span: span(start, end),
        }
    }

    fn ident_expr(name: &str, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::Ident(ident(name, start, end)),
            span: span(start, end),
        }
    }

    /// Parse, lower, and emit a RustScript source string to Rust output.
    fn compile_and_emit(source: &str) -> String {
        let file_id = rsc_syntax::source::FileId(0);
        let (module, parse_diags) = rsc_parser::parse(source, file_id);
        assert!(
            parse_diags.is_empty(),
            "unexpected parse diagnostics: {parse_diags:?}"
        );
        let lower_result = crate::lower(&module);
        let ir = lower_result.ir;
        let lower_diags = lower_result.diagnostics;
        assert!(
            lower_diags.is_empty(),
            "unexpected lowering diagnostics: {lower_diags:?}"
        );
        rsc_emit::emit(&ir).source
    }

    /// Parse and lower a RustScript source string, returning the Rust IR.
    fn lower_source(source: &str) -> RustFile {
        let file_id = rsc_syntax::source::FileId(0);
        let (module, parse_diags) = rsc_parser::parse(source, file_id);
        assert!(
            parse_diags.is_empty(),
            "unexpected parse diagnostics: {parse_diags:?}"
        );
        let lower_result = crate::lower(&module);
        let ir = lower_result.ir;
        let lower_diags = lower_result.diagnostics;
        assert!(
            lower_diags.is_empty(),
            "unexpected lowering diagnostics: {lower_diags:?}"
        );
        ir
    }

    fn float_expr(value: f64, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::FloatLit(value),
            span: span(start, end),
        }
    }

    fn string_expr(s: &str, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::StringLit(s.to_owned()),
            span: span(start, end),
        }
    }

    /// Wrap a `TypeAnnotation` in a `ReturnTypeAnnotation` with no throws.
    fn ret_type(ann: TypeAnnotation) -> ReturnTypeAnnotation {
        let s = ann.span;
        ReturnTypeAnnotation {
            type_ann: Some(ann),
            throws: None,
            span: s,
        }
    }

    fn fn_item(f: FnDecl) -> Item {
        let item_span = f.span;
        Item {
            kind: ItemKind::Function(f),
            exported: false,
            decorators: vec![],
            span: item_span,
        }
    }

    fn make_module(items: Vec<Item>) -> Module {
        Module {
            items,
            span: span(0, 100),
        }
    }

    fn make_fn(
        name: &str,
        params: Vec<Param>,
        return_type: Option<TypeAnnotation>,
        body: Vec<Stmt>,
    ) -> FnDecl {
        FnDecl {
            is_async: false,
            is_generator: false,
            name: ident(name, 0, name.len() as u32),
            type_params: None,
            params,
            return_type: return_type.map(|ann| ret_type(ann)),
            body: Block {
                stmts: body,
                span: span(0, 100),
            },
            doc_comment: None,
            span: span(0, 100),
        }
    }

    fn make_param(name: &str, type_name: &str) -> Param {
        Param {
            name: ident(name, 0, name.len() as u32),
            type_ann: TypeAnnotation {
                kind: TypeKind::Named(ident(type_name, 0, type_name.len() as u32)),
                span: span(0, type_name.len() as u32),
            },
            optional: false,
            default_value: None,
            is_rest: false,
            span: span(0, 10),
        }
    }

    // Test 15: Lower empty function main()
    #[test]
    fn test_lower_empty_main_function() {
        let f = make_fn("main", vec![], None, vec![]);
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty());
        assert_eq!(file.items.len(), 1);
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.name, "main");
        assert!(func.params.is_empty());
        assert!(func.return_type.is_none());
        assert!(func.body.stmts.is_empty());
        assert!(func.span.is_some());
    }

    // Test 16: Lower function params (a: i32, b: string): bool
    #[test]
    fn test_lower_function_params_and_return_type() {
        let f = make_fn(
            "test",
            vec![make_param("a", "i32"), make_param("b", "string")],
            Some(TypeAnnotation {
                kind: TypeKind::Named(ident("bool", 0, 4)),
                span: span(0, 4),
            }),
            vec![],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.params[0].name, "a");
        assert_eq!(func.params[0].ty, RustType::I32);
        assert_eq!(func.params[1].name, "b");
        assert_eq!(func.params[1].ty, RustType::String);
        assert_eq!(func.return_type, Some(RustType::Bool));
    }

    // Test 17: Lower const x: i32 = 42
    #[test]
    fn test_lower_const_with_type_annotation() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 1);
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(!let_stmt.mutable);
                assert_eq!(let_stmt.ty, Some(RustType::I32));
                assert_eq!(let_stmt.name, "x");
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test 18: Lower let x = 42; x = 10; → let mut
    #[test]
    fn test_lower_let_with_reassignment_becomes_mut() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![
                Stmt::VarDecl(VarDecl {
                    binding: VarBinding::Let,
                    name: ident("x", 4, 5),
                    type_ann: None,
                    init: int_expr(42, 8, 10),
                    span: span(0, 11),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident("x", 12, 13),
                        value: Box::new(int_expr(10, 16, 18)),
                    }),
                    span: span(12, 19),
                }),
            ],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 2);
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(
                    let_stmt.mutable,
                    "x should be mutable since it's reassigned"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
        // Second stmt should be Semi(Assign)
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Assign { target, .. } => assert_eq!(target, "x"),
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 19: Lower const x = 42; (no type ann) → omit type (inferable)
    #[test]
    fn test_lower_const_no_type_annotation_omits_type() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: None,
                init: int_expr(42, 10, 12),
                span: span(0, 13),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(
                    let_stmt.ty, None,
                    "type should be omitted when inferable from literal"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test 20: Lower console.log("hello") → println! via builtin registry
    #[test]
    fn test_lower_console_log_single_arg_produces_println_macro() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![string_expr("hello", 12, 19)],
                }),
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    assert_eq!(args.len(), 2);
                    match &args[0].kind {
                        RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{}"),
                        other => panic!("expected StringLit format, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 21: Lower console.log(x, y) → println! with format string "{} {}"
    #[test]
    fn test_lower_console_log_two_args_format_string() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![ident_expr("x", 12, 13), ident_expr("y", 15, 16)],
                }),
                span: span(0, 17),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    assert_eq!(args.len(), 3); // format string + 2 args
                    match &args[0].kind {
                        RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{} {}"),
                        other => panic!("expected StringLit format, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 22: Lower if (x > 0) { ... } → RustIfStmt sans parens
    #[test]
    fn test_lower_if_statement_with_condition() {
        let f = make_fn(
            "main",
            vec![make_param("x", "i32")],
            None,
            vec![Stmt::If(IfStmt {
                condition: Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Gt,
                        left: Box::new(ident_expr("x", 4, 5)),
                        right: Box::new(int_expr(0, 8, 9)),
                    }),
                    span: span(4, 9),
                },
                then_block: Block {
                    stmts: vec![],
                    span: span(11, 13),
                },
                else_clause: None,
                span: span(0, 13),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::If(if_stmt) => {
                assert!(if_stmt.span.is_some());
                match &if_stmt.condition.kind {
                    RustExprKind::Binary { op, .. } => {
                        assert_eq!(*op, RustBinaryOp::Gt);
                    }
                    other => panic!("expected Binary, got {other:?}"),
                }
                assert!(if_stmt.else_clause.is_none());
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    // Test 23: Lower binary % → RustBinaryOp::Rem
    #[test]
    fn test_lower_binary_mod_to_rem() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Mod,
                    left: Box::new(int_expr(10, 0, 2)),
                    right: Box::new(int_expr(3, 5, 6)),
                }),
                span: span(0, 6),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Binary { op, .. } => assert_eq!(*op, RustBinaryOp::Rem),
                other => panic!("expected Binary, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 24: Lower return; → RustStmt::Return with value: None
    #[test]
    fn test_lower_bare_return() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Return(ReturnStmt {
                value: None,
                span: span(0, 7),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Return(ret) => {
                assert!(ret.value.is_none());
                assert!(ret.span.is_some());
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // Test 25: Unknown type name → diagnostic emitted
    #[test]
    fn test_lower_unknown_type_emits_diagnostic() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("Foo", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (_, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown type"));
        assert!(diags[0].message.contains("Foo"));
    }

    // Correctness Scenario 1: Fibonacci lowering
    #[test]
    fn test_correctness_fibonacci_lowering() {
        // function fib(n: i32): i32 {
        //   if (n <= 1) { return n; }
        //   return fib(n - 1) + fib(n - 2);
        // }
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("fib", 0, 3),
            type_params: None,
            params: vec![make_param("n", "i32")],
            return_type: Some(ret_type(TypeAnnotation {
                kind: TypeKind::Named(ident("i32", 0, 3)),
                span: span(0, 3),
            })),
            body: Block {
                stmts: vec![
                    Stmt::If(IfStmt {
                        condition: Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Le,
                                left: Box::new(ident_expr("n", 10, 11)),
                                right: Box::new(int_expr(1, 15, 16)),
                            }),
                            span: span(10, 16),
                        },
                        then_block: Block {
                            stmts: vec![Stmt::Return(ReturnStmt {
                                value: Some(ident_expr("n", 20, 21)),
                                span: span(18, 22),
                            })],
                            span: span(17, 23),
                        },
                        else_clause: None,
                        span: span(7, 23),
                    }),
                    Stmt::Return(ReturnStmt {
                        value: Some(Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Add,
                                left: Box::new(Expr {
                                    kind: ExprKind::Call(CallExpr {
                                        callee: ident("fib", 30, 33),
                                        args: vec![Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: BinaryOp::Sub,
                                                left: Box::new(ident_expr("n", 34, 35)),
                                                right: Box::new(int_expr(1, 38, 39)),
                                            }),
                                            span: span(34, 39),
                                        }],
                                    }),
                                    span: span(30, 40),
                                }),
                                right: Box::new(Expr {
                                    kind: ExprKind::Call(CallExpr {
                                        callee: ident("fib", 43, 46),
                                        args: vec![Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: BinaryOp::Sub,
                                                left: Box::new(ident_expr("n", 47, 48)),
                                                right: Box::new(int_expr(2, 51, 52)),
                                            }),
                                            span: span(47, 52),
                                        }],
                                    }),
                                    span: span(43, 53),
                                }),
                            }),
                            span: span(30, 53),
                        }),
                        span: span(24, 54),
                    }),
                ],
                span: span(5, 55),
            },
            doc_comment: None,
            span: span(0, 55),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.name, "fib");
        assert_eq!(func.params.len(), 1);
        assert_eq!(func.params[0].name, "n");
        assert_eq!(func.params[0].ty, RustType::I32);
        assert_eq!(func.return_type, Some(RustType::I32));
        assert_eq!(func.body.stmts.len(), 2);
        assert!(func.span.is_some());

        // Verify all spans are Some
        assert!(func.params[0].span.is_some());
        match &func.body.stmts[0] {
            RustStmt::If(if_stmt) => {
                assert!(if_stmt.span.is_some());
                assert!(if_stmt.condition.span.is_some());
            }
            other => panic!("expected If, got {other:?}"),
        }
        match &func.body.stmts[1] {
            RustStmt::Return(ret) => {
                assert!(ret.span.is_some());
                assert!(ret.value.as_ref().unwrap().span.is_some());
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // Correctness Scenario 2: String - no clones for println! args
    #[test]
    fn test_correctness_no_clones_for_println_args() {
        // function example(name: string): void {
        //   console.log(name);   // stmt 0: NOT a move position
        //   console.log(name);   // stmt 1: NOT a move position
        // }
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("example", 0, 7),
            type_params: None,
            params: vec![make_param("name", "string")],
            return_type: Some(ret_type(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            })),
            body: Block {
                stmts: vec![
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 30, 37)),
                            method: ident("log", 38, 41),
                            args: vec![ident_expr("name", 42, 46)],
                        }),
                        span: span(30, 47),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 50, 57)),
                            method: ident("log", 58, 61),
                            args: vec![ident_expr("name", 62, 66)],
                        }),
                        span: span(50, 67),
                    }),
                ],
                span: span(28, 68),
            },
            doc_comment: None,
            span: span(0, 68),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        // Both statements should be println! macros with NO clones
        for (i, stmt) in func.body.stmts.iter().enumerate() {
            match stmt {
                RustStmt::Semi(expr) => match &expr.kind {
                    RustExprKind::Macro { name, args } => {
                        assert_eq!(name, "println");
                        // The second arg should be an Ident, not a Clone
                        assert!(
                            args.len() >= 2,
                            "stmt {i}: expected at least 2 args in println!"
                        );
                        match &args[1].kind {
                            RustExprKind::Ident(n) => assert_eq!(n, "name"),
                            RustExprKind::Clone(_) => {
                                panic!("stmt {i}: name should NOT be cloned for println!")
                            }
                            other => panic!("stmt {i}: expected Ident, got {other:?}"),
                        }
                    }
                    other => panic!("stmt {i}: expected Macro, got {other:?}"),
                },
                other => panic!("stmt {i}: expected Semi, got {other:?}"),
            }
        }
    }

    // Correctness Scenario 3: String clone when actually needed
    #[test]
    fn test_correctness_string_clone_at_move_point() {
        // function example(name: string): void {
        //   greet(name);          // stmt 0: move position, name used later → clone
        //   console.log(name);    // stmt 1: not a move position, no clone
        // }
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("example", 0, 7),
            type_params: None,
            params: vec![make_param("name", "string")],
            return_type: Some(ret_type(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            })),
            body: Block {
                stmts: vec![
                    Stmt::Expr(Expr {
                        kind: ExprKind::Call(CallExpr {
                            callee: ident("greet", 30, 35),
                            args: vec![ident_expr("name", 36, 40)],
                        }),
                        span: span(30, 41),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 45, 52)),
                            method: ident("log", 53, 56),
                            args: vec![ident_expr("name", 57, 61)],
                        }),
                        span: span(45, 62),
                    }),
                ],
                span: span(28, 63),
            },
            doc_comment: None,
            span: span(0, 63),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 2);

        // stmt 0: greet(name.clone()) — name is in move position and used later
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Call { func: f, args } => {
                    assert_eq!(f, "greet");
                    assert_eq!(args.len(), 1);
                    match &args[0].kind {
                        RustExprKind::Clone(inner) => match &inner.kind {
                            RustExprKind::Ident(n) => assert_eq!(n, "name"),
                            other => panic!("expected Ident inside Clone, got {other:?}"),
                        },
                        other => panic!("expected Clone, got {other:?}"),
                    }
                }
                other => panic!("expected Call, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }

        // stmt 1: println! — name is NOT cloned
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    match &args[1].kind {
                        RustExprKind::Ident(n) => assert_eq!(n, "name"),
                        RustExprKind::Clone(_) => panic!("name should NOT be cloned in println!"),
                        other => panic!("expected Ident, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Correctness Scenario 4: Mutability detection
    #[test]
    fn test_correctness_mutability_detection() {
        // function counter(): void {
        //   let x = 0;
        //   const y = 10;
        //   x = x + 1;
        // }
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("counter", 0, 7),
            type_params: None,
            params: vec![],
            return_type: Some(ret_type(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            })),
            body: Block {
                stmts: vec![
                    Stmt::VarDecl(VarDecl {
                        binding: VarBinding::Let,
                        name: ident("x", 20, 21),
                        type_ann: None,
                        init: int_expr(0, 24, 25),
                        span: span(16, 26),
                    }),
                    Stmt::VarDecl(VarDecl {
                        binding: VarBinding::Const,
                        name: ident("y", 33, 34),
                        type_ann: None,
                        init: int_expr(10, 37, 39),
                        span: span(27, 40),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::Assign(AssignExpr {
                            target: ident("x", 41, 42),
                            value: Box::new(Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Add,
                                    left: Box::new(ident_expr("x", 45, 46)),
                                    right: Box::new(int_expr(1, 49, 50)),
                                }),
                                span: span(45, 50),
                            }),
                        }),
                        span: span(41, 51),
                    }),
                ],
                span: span(14, 52),
            },
            doc_comment: None,
            span: span(0, 52),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 3);

        // x is let mut (reassigned)
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(let_stmt.name, "x");
                assert!(let_stmt.mutable, "x should be let mut (reassigned)");
            }
            other => panic!("expected Let for x, got {other:?}"),
        }

        // y is let (const, not reassigned)
        match &func.body.stmts[1] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(let_stmt.name, "y");
                assert!(!let_stmt.mutable, "y should be let (const)");
            }
            other => panic!("expected Let for y, got {other:?}"),
        }
    }

    // Test: Compound assignment lowering — x = x + 1 becomes CompoundAssign
    #[test]
    fn test_lower_compound_assign_add() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![
                Stmt::VarDecl(VarDecl {
                    binding: VarBinding::Let,
                    name: ident("x", 4, 5),
                    type_ann: None,
                    init: int_expr(0, 8, 9),
                    span: span(0, 10),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident("x", 11, 12),
                        value: Box::new(Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Add,
                                left: Box::new(ident_expr("x", 15, 16)),
                                right: Box::new(int_expr(1, 19, 20)),
                            }),
                            span: span(15, 20),
                        }),
                    }),
                    span: span(11, 21),
                }),
            ],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::CompoundAssign { target, op, .. } => {
                    assert_eq!(target, "x");
                    assert_eq!(*op, RustCompoundAssignOp::AddAssign);
                }
                other => panic!("expected CompoundAssign, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test: x = y + 1 does NOT become compound assign (target != lhs)
    #[test]
    fn test_lower_non_compound_assign_different_ident() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Assign(AssignExpr {
                    target: ident("x", 0, 1),
                    value: Box::new(Expr {
                        kind: ExprKind::Binary(BinaryExpr {
                            op: BinaryOp::Add,
                            left: Box::new(ident_expr("y", 4, 5)),
                            right: Box::new(int_expr(1, 8, 9)),
                        }),
                        span: span(4, 9),
                    }),
                }),
                span: span(0, 10),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Assign { target, .. } => assert_eq!(target, "x"),
                other => panic!("expected Assign (not CompoundAssign), got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test: console.log("hello") strips .to_string() from string arg
    #[test]
    fn test_lower_console_log_string_arg_no_to_string() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![string_expr("hello", 12, 19)],
                }),
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { args, .. } => {
                    // args[1] should be StringLit, NOT ToString(StringLit)
                    match &args[1].kind {
                        RustExprKind::StringLit(s) => assert_eq!(s, "hello"),
                        RustExprKind::ToString(_) => {
                            panic!("string arg in println! should NOT be wrapped in .to_string()")
                        }
                        other => panic!("expected StringLit, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test: console.log(name) where name is a variable — still works
    #[test]
    fn test_lower_console_log_ident_arg_not_stripped() {
        let f = make_fn(
            "main",
            vec![make_param("name", "string")],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![ident_expr("name", 12, 16)],
                }),
                span: span(0, 17),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { args, .. } => {
                    // args[1] should be Ident, not ToString-wrapped
                    match &args[1].kind {
                        RustExprKind::Ident(n) => assert_eq!(n, "name"),
                        other => panic!("expected Ident(name), got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test: Explicit type annotation is preserved even when it matches default
    #[test]
    fn test_lower_explicit_type_annotation_preserved() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i64", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(
                    let_stmt.ty,
                    Some(RustType::I64),
                    "explicit i64 annotation should be preserved"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test: Explicit non-default type annotation is preserved
    #[test]
    fn test_lower_explicit_i32_annotation_preserved() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(
                    let_stmt.ty,
                    Some(RustType::I32),
                    "explicit i32 annotation should be preserved"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 014: Type definitions and struct sugar lowering
    // ---------------------------------------------------------------

    // Test T14-6: Lower TypeDef -> RustStructDef with pub fields
    #[test]
    fn test_lower_type_def_produces_struct_with_pub_fields() {
        let td = ast::TypeDef {
            name: ident("User", 0, 4),
            type_params: None,
            fields: vec![
                ast::FieldDef {
                    name: ident("name", 0, 4),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("string", 0, 6)),
                        span: span(0, 6),
                    },
                    span: span(0, 10),
                },
                ast::FieldDef {
                    name: ident("age", 0, 3),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("u32", 0, 3)),
                        span: span(0, 3),
                    },
                    span: span(0, 6),
                },
            ],
            index_signature: None,
            type_alias: None,
            derives: vec![],
            doc_comment: None,
            span: span(0, 50),
        };
        let module = Module {
            items: vec![Item {
                kind: ItemKind::TypeDef(td),
                exported: false,
                decorators: vec![],
                span: span(0, 50),
            }],
            span: span(0, 50),
        };
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty());
        assert_eq!(file.items.len(), 1);
        let RustItem::Struct(s) = &file.items[0] else {
            panic!("expected Struct item");
        };
        assert_eq!(s.name, "User");
        assert_eq!(s.fields.len(), 2);
        assert!(s.fields[0].public);
        assert_eq!(s.fields[0].name, "name");
        assert_eq!(s.fields[0].ty, RustType::String);
        assert!(s.fields[1].public);
        assert_eq!(s.fields[1].name, "age");
        assert_eq!(s.fields[1].ty, RustType::U32);
    }

    // Test T14-7: Lower struct literal -> RustExprKind::StructLit
    #[test]
    fn test_lower_struct_literal_produces_struct_lit_expr() {
        let td = ast::TypeDef {
            name: ident("Point", 0, 5),
            type_params: None,
            fields: vec![
                ast::FieldDef {
                    name: ident("x", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("f64", 0, 3)),
                        span: span(0, 3),
                    },
                    span: span(0, 4),
                },
                ast::FieldDef {
                    name: ident("y", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("f64", 0, 3)),
                        span: span(0, 3),
                    },
                    span: span(0, 4),
                },
            ],
            index_signature: None,
            type_alias: None,
            derives: vec![],
            doc_comment: None,
            span: span(0, 30),
        };
        let body = vec![Stmt::VarDecl(VarDecl {
            binding: VarBinding::Const,
            name: ident("p", 0, 1),
            type_ann: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("Point", 0, 5)),
                span: span(0, 5),
            }),
            init: Expr {
                kind: ExprKind::StructLit(ast::StructLitExpr {
                    type_name: None,
                    spread: None,
                    fields: vec![
                        ast::FieldInit {
                            name: ident("x", 0, 1),
                            value: Expr {
                                kind: ExprKind::FloatLit(1.0),
                                span: span(0, 3),
                            },
                            computed_key: None,
                            span: span(0, 4),
                        },
                        ast::FieldInit {
                            name: ident("y", 0, 1),
                            value: Expr {
                                kind: ExprKind::FloatLit(2.0),
                                span: span(0, 3),
                            },
                            computed_key: None,
                            span: span(0, 4),
                        },
                    ],
                }),
                span: span(0, 20),
            },
            span: span(0, 25),
        })];
        let module = Module {
            items: vec![
                Item {
                    kind: ItemKind::TypeDef(td),
                    exported: false,
                    decorators: vec![],
                    span: span(0, 30),
                },
                fn_item(make_fn("main", vec![], None, body)),
            ],
            span: span(0, 100),
        };
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty());
        // The function is the second item
        let RustItem::Function(func) = &file.items[1] else {
            panic!("expected Function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                RustExprKind::StructLit { type_name, fields } => {
                    assert_eq!(type_name, "Point");
                    assert_eq!(fields.len(), 2);
                    assert_eq!(fields[0].0, "x");
                    assert_eq!(fields[1].0, "y");
                }
                other => panic!("expected StructLit, got {other:?}"),
            },
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test T14-8: Lower field access -> RustExprKind::FieldAccess
    #[test]
    fn test_lower_field_access_produces_field_access_expr() {
        let body = vec![
            Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 0, 3)),
                    span: span(0, 3),
                }),
                init: int_expr(42, 0, 2),
                span: span(0, 10),
            }),
            Stmt::Expr(Expr {
                kind: ExprKind::FieldAccess(ast::FieldAccessExpr {
                    object: Box::new(ident_expr("obj", 0, 3)),
                    field: ident("name", 4, 8),
                }),
                span: span(0, 8),
            }),
        ];
        let module = make_module(vec![fn_item(make_fn("main", vec![], None, body))]);
        let mut transform = Transform::new(false);
        let (file, _diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected Function item");
        };
        // Second stmt should be the field access
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::FieldAccess { object: _, field } => {
                    assert_eq!(field, "name");
                }
                other => panic!("expected FieldAccess, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // ---- Task 016: Generics lowering ----

    // Test T16-6: Lower generic function → RustFnDecl with type_params
    #[test]
    fn test_lower_generic_fn_produces_type_params() {
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("id", 0, 2),
            type_params: Some(ast::TypeParams {
                params: vec![ast::TypeParam {
                    name: ident("T", 0, 1),
                    constraint: None,
                    span: span(0, 1),
                }],
                span: span(0, 3),
            }),
            params: vec![Param {
                name: ident("x", 0, 1),
                type_ann: TypeAnnotation {
                    kind: TypeKind::Named(ident("T", 0, 1)),
                    span: span(0, 1),
                },
                optional: false,
                default_value: None,
                is_rest: false,
                span: span(0, 3),
            }],
            return_type: Some(ret_type(TypeAnnotation {
                kind: TypeKind::Named(ident("T", 0, 1)),
                span: span(0, 1),
            })),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(ident_expr("x", 0, 1)),
                    span: span(0, 10),
                })],
                span: span(0, 20),
            },
            doc_comment: None,
            span: span(0, 30),
        };
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected Function");
        };
        assert_eq!(func.type_params.len(), 1);
        assert_eq!(func.type_params[0].name, "T");
        assert!(func.type_params[0].bounds.is_empty());
        assert_eq!(func.params[0].ty, RustType::TypeParam("T".to_owned()));
        assert_eq!(func.return_type, Some(RustType::TypeParam("T".to_owned())));
    }

    // Test T16-7: Lower constrained generic → RustTypeParam with bounds
    #[test]
    fn test_lower_constrained_generic_produces_bounds() {
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("merge", 0, 5),
            type_params: Some(ast::TypeParams {
                params: vec![ast::TypeParam {
                    name: ident("T", 0, 1),
                    constraint: Some(TypeAnnotation {
                        kind: TypeKind::Named(ident("Comparable", 0, 10)),
                        span: span(0, 10),
                    }),
                    span: span(0, 20),
                }],
                span: span(0, 22),
            }),
            params: vec![
                Param {
                    name: ident("a", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("T", 0, 1)),
                        span: span(0, 1),
                    },
                    optional: false,
                    default_value: None,
                    is_rest: false,
                    span: span(0, 3),
                },
                Param {
                    name: ident("b", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("T", 0, 1)),
                        span: span(0, 1),
                    },
                    optional: false,
                    default_value: None,
                    is_rest: false,
                    span: span(0, 3),
                },
            ],
            return_type: Some(ret_type(TypeAnnotation {
                kind: TypeKind::Named(ident("T", 0, 1)),
                span: span(0, 1),
            })),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(ident_expr("a", 0, 1)),
                    span: span(0, 10),
                })],
                span: span(0, 20),
            },
            doc_comment: None,
            span: span(0, 50),
        };
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected Function");
        };
        assert_eq!(func.type_params.len(), 1);
        assert_eq!(func.type_params[0].name, "T");
        assert_eq!(func.type_params[0].bounds, vec!["Comparable".to_owned()]);
    }

    // Test T16-8: Lower generic struct → RustStructDef with type_params
    #[test]
    fn test_lower_generic_struct_produces_type_params() {
        let td = ast::TypeDef {
            name: ident("Container", 0, 9),
            type_params: Some(ast::TypeParams {
                params: vec![ast::TypeParam {
                    name: ident("T", 0, 1),
                    constraint: None,
                    span: span(0, 1),
                }],
                span: span(0, 3),
            }),
            fields: vec![ast::FieldDef {
                name: ident("value", 0, 5),
                type_ann: TypeAnnotation {
                    kind: TypeKind::Named(ident("T", 0, 1)),
                    span: span(0, 1),
                },
                span: span(0, 8),
            }],
            index_signature: None,
            type_alias: None,
            derives: vec![],
            doc_comment: None,
            span: span(0, 30),
        };
        let module = make_module(vec![Item {
            kind: ItemKind::TypeDef(td),
            exported: false,
            decorators: vec![],
            span: span(0, 30),
        }]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[0] else {
            panic!("expected Struct");
        };
        assert_eq!(s.name, "Container");
        assert_eq!(s.type_params.len(), 1);
        assert_eq!(s.type_params[0].name, "T");
        assert_eq!(s.fields[0].ty, RustType::TypeParam("T".to_owned()));
    }

    // ---------------------------------------------------------------
    // Template literal lowering tests
    // ---------------------------------------------------------------

    // Test: Lower no-interpolation template → .to_string()
    #[test]
    fn test_lower_template_no_interpolation_produces_to_string() {
        let template_expr = Expr {
            kind: ExprKind::TemplateLit(ast::TemplateLitExpr {
                parts: vec![ast::TemplatePart::String(
                    "hello world".to_owned(),
                    span(0, 11),
                )],
            }),
            span: span(0, 14),
        };
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("msg", 0, 3),
                type_ann: None,
                init: template_expr,
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let stmt");
        };
        // Should be .to_string() wrapping a string literal
        assert!(
            matches!(&let_stmt.init.kind, RustExprKind::ToString(inner) if
                matches!(&inner.kind, RustExprKind::StringLit(s) if s == "hello world")
            ),
            "expected ToString(StringLit(\"hello world\")), got {:?}",
            let_stmt.init.kind
        );
    }

    // Test: Lower single interpolation → format!("Hello, {}!", name)
    #[test]
    fn test_lower_template_single_interpolation_produces_format_macro() {
        let template_expr = Expr {
            kind: ExprKind::TemplateLit(ast::TemplateLitExpr {
                parts: vec![
                    ast::TemplatePart::String("Hello, ".to_owned(), span(0, 7)),
                    ast::TemplatePart::Expr(ident_expr("name", 10, 14)),
                    ast::TemplatePart::String("!".to_owned(), span(15, 16)),
                ],
            }),
            span: span(0, 18),
        };
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("greeting", 0, 8),
                type_ann: None,
                init: template_expr,
                span: span(0, 25),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let stmt");
        };
        let RustExprKind::Macro { name, args } = &let_stmt.init.kind else {
            panic!("expected Macro, got {:?}", let_stmt.init.kind);
        };
        assert_eq!(name, "format");
        assert_eq!(args.len(), 2);
        assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "Hello, {}!"));
        assert!(matches!(&args[1].kind, RustExprKind::Ident(n) if n == "name"));
    }

    // Test: Lower multiple interpolations → format! with multiple args
    #[test]
    fn test_lower_template_multiple_interpolations_produces_format_with_multiple_args() {
        let template_expr = Expr {
            kind: ExprKind::TemplateLit(ast::TemplateLitExpr {
                parts: vec![
                    ast::TemplatePart::String(String::new(), span(0, 0)),
                    ast::TemplatePart::Expr(ident_expr("a", 2, 3)),
                    ast::TemplatePart::String(" + ".to_owned(), span(4, 7)),
                    ast::TemplatePart::Expr(ident_expr("b", 9, 10)),
                    ast::TemplatePart::String(" = ".to_owned(), span(11, 14)),
                    ast::TemplatePart::Expr(ident_expr("c", 16, 17)),
                    ast::TemplatePart::String(String::new(), span(18, 18)),
                ],
            }),
            span: span(0, 20),
        };
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: None,
                init: template_expr,
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let stmt");
        };
        let RustExprKind::Macro { name, args } = &let_stmt.init.kind else {
            panic!("expected Macro, got {:?}", let_stmt.init.kind);
        };
        assert_eq!(name, "format");
        assert_eq!(args.len(), 4); // format string + 3 exprs
        assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "{} + {} = {}"));
    }

    // Test: Lower expression interpolation → format!("Result: {}", x + y)
    #[test]
    fn test_lower_template_expression_interpolation_produces_format_with_binary() {
        let binary_expr = Expr {
            kind: ExprKind::Binary(BinaryExpr {
                op: BinaryOp::Add,
                left: Box::new(ident_expr("x", 10, 11)),
                right: Box::new(ident_expr("y", 14, 15)),
            }),
            span: span(10, 15),
        };
        let template_expr = Expr {
            kind: ExprKind::TemplateLit(ast::TemplateLitExpr {
                parts: vec![
                    ast::TemplatePart::String("Result: ".to_owned(), span(0, 8)),
                    ast::TemplatePart::Expr(binary_expr),
                    ast::TemplatePart::String(String::new(), span(16, 16)),
                ],
            }),
            span: span(0, 18),
        };
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: None,
                init: template_expr,
                span: span(0, 25),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let stmt");
        };
        let RustExprKind::Macro { name, args } = &let_stmt.init.kind else {
            panic!("expected Macro, got {:?}", let_stmt.init.kind);
        };
        assert_eq!(name, "format");
        assert_eq!(args.len(), 2);
        assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "Result: {}"));
        assert!(matches!(&args[1].kind, RustExprKind::Binary { .. }));
    }

    // ---- Task 015: Enum lowering tests ----

    // Test T015-4: Lower simple enum → RustEnumDef with fieldless variants, names capitalized
    #[test]
    fn test_lower_simple_enum_capitalized_variants() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::EnumDef(EnumDef {
                    name: ident("Direction", 0, 9),
                    variants: vec![
                        EnumVariant::Simple(ident("North", 0, 5), span(0, 5)),
                        EnumVariant::Simple(ident("South", 0, 5), span(0, 5)),
                        EnumVariant::Simple(ident("East", 0, 4), span(0, 4)),
                        EnumVariant::Simple(ident("West", 0, 4), span(0, 4)),
                    ],
                    derives: vec![],
                    doc_comment: None,
                    is_const: false,
                    span: span(0, 50),
                }),
                exported: false,
                decorators: vec![],
                span: span(0, 50),
            }],
            span: span(0, 50),
        };

        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert_eq!(file.items.len(), 1);
        match &file.items[0] {
            RustItem::Enum(e) => {
                assert_eq!(e.name, "Direction");
                assert_eq!(e.variants.len(), 4);
                assert_eq!(e.variants[0].name, "North");
                assert!(e.variants[0].fields.is_empty());
                assert_eq!(e.variants[3].name, "West");
            }
            _ => panic!("expected Enum item"),
        }
    }

    // Test T015-5: Lower data enum → RustEnumDef with struct variants, kind field removed
    #[test]
    fn test_lower_data_enum_struct_variants() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::EnumDef(EnumDef {
                    name: ident("Shape", 0, 5),
                    is_const: false,
                    variants: vec![
                        EnumVariant::Data {
                            discriminant_value: "circle".to_owned(),
                            name: ident("Circle", 0, 6),
                            fields: vec![FieldDef {
                                name: ident("radius", 0, 6),
                                type_ann: TypeAnnotation {
                                    kind: TypeKind::Named(ident("f64", 0, 3)),
                                    span: span(0, 3),
                                },
                                span: span(0, 10),
                            }],
                            span: span(0, 30),
                        },
                        EnumVariant::Data {
                            discriminant_value: "rect".to_owned(),
                            name: ident("Rect", 0, 4),
                            fields: vec![
                                FieldDef {
                                    name: ident("width", 0, 5),
                                    type_ann: TypeAnnotation {
                                        kind: TypeKind::Named(ident("f64", 0, 3)),
                                        span: span(0, 3),
                                    },
                                    span: span(0, 10),
                                },
                                FieldDef {
                                    name: ident("height", 0, 6),
                                    type_ann: TypeAnnotation {
                                        kind: TypeKind::Named(ident("f64", 0, 3)),
                                        span: span(0, 3),
                                    },
                                    span: span(0, 10),
                                },
                            ],
                            span: span(0, 50),
                        },
                    ],
                    derives: vec![],
                    doc_comment: None,
                    span: span(0, 80),
                }),
                exported: false,
                decorators: vec![],
                span: span(0, 80),
            }],
            span: span(0, 80),
        };

        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);
        match &file.items[0] {
            RustItem::Enum(e) => {
                assert_eq!(e.name, "Shape");
                assert_eq!(e.variants.len(), 2);
                assert_eq!(e.variants[0].name, "Circle");
                assert_eq!(e.variants[0].fields.len(), 1);
                assert_eq!(e.variants[0].fields[0].name, "radius");
                assert_eq!(e.variants[1].name, "Rect");
                assert_eq!(e.variants[1].fields.len(), 2);
            }
            _ => panic!("expected Enum item"),
        }
    }

    // ---------------------------------------------------------------
    // Task 017: Collection lowering
    // ---------------------------------------------------------------

    // Test T17-7: Lower array literal → RustExprKind::VecLit
    #[test]
    fn test_lower_array_literal_produces_vec_lit() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("nums", 0, 4),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::ArrayLit(vec![
                        ast::ArrayElement::Expr(int_expr(1, 0, 1)),
                        ast::ArrayElement::Expr(int_expr(2, 3, 4)),
                        ast::ArrayElement::Expr(int_expr(3, 6, 7)),
                    ]),
                    span: span(0, 8),
                },
                span: span(0, 10),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        assert!(matches!(let_stmt.init.kind, RustExprKind::VecLit(_)));
        if let RustExprKind::VecLit(elems) = &let_stmt.init.kind {
            assert_eq!(elems.len(), 3);
        }
    }

    // Test T17-8: Lower `new Map()` → StaticCall { type_name: "HashMap", method: "new" }
    #[test]
    fn test_lower_new_map_produces_static_call_hashmap() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("lookup", 0, 6),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Generic(
                        ident("Map", 0, 3),
                        vec![
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("string", 0, 6)),
                                span: span(0, 6),
                            },
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("i32", 0, 3)),
                                span: span(0, 3),
                            },
                        ],
                    ),
                    span: span(0, 20),
                }),
                init: Expr {
                    kind: ExprKind::New(ast::NewExpr {
                        type_name: ident("Map", 0, 3),
                        type_args: vec![],
                        args: vec![],
                    }),
                    span: span(0, 10),
                },
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        match &let_stmt.init.kind {
            RustExprKind::StaticCall {
                type_name, method, ..
            } => {
                assert_eq!(type_name, "HashMap");
                assert_eq!(method, "new");
            }
            _ => panic!("expected StaticCall, got {:?}", let_stmt.init.kind),
        }
    }

    // Test T17-9: Lower index access → RustExprKind::Index
    #[test]
    fn test_lower_index_access_produces_index() {
        let f = make_fn(
            "main",
            vec![make_param("arr", "i32")],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Index(ast::IndexExpr {
                    object: Box::new(ident_expr("arr", 0, 3)),
                    index: Box::new(int_expr(0, 4, 5)),
                }),
                span: span(0, 6),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Semi(expr) = &func.body.stmts[0] else {
            panic!("expected Semi");
        };
        assert!(matches!(expr.kind, RustExprKind::Index { .. }));
    }

    // Test T17-10: Lower `Array<string>` type → Vec<String> in Rust type
    #[test]
    fn test_lower_array_string_type_to_vec_string() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("names", 0, 5),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Generic(
                        ident("Array", 0, 5),
                        vec![TypeAnnotation {
                            kind: TypeKind::Named(ident("string", 0, 6)),
                            span: span(0, 6),
                        }],
                    ),
                    span: span(0, 13),
                }),
                init: Expr {
                    kind: ExprKind::ArrayLit(vec![]),
                    span: span(0, 2),
                },
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        // Type should be Vec<String>
        let expected_ty = RustType::Generic(
            Box::new(RustType::Named("Vec".to_owned())),
            vec![RustType::String],
        );
        assert_eq!(let_stmt.ty, Some(expected_ty));
    }

    // Test T17-14: use declarations generated for HashMap
    #[test]
    fn test_lower_map_generates_use_hashmap() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("lookup", 0, 6),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Generic(
                        ident("Map", 0, 3),
                        vec![
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("string", 0, 6)),
                                span: span(0, 6),
                            },
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("i32", 0, 3)),
                                span: span(0, 3),
                            },
                        ],
                    ),
                    span: span(0, 20),
                }),
                init: Expr {
                    kind: ExprKind::New(ast::NewExpr {
                        type_name: ident("Map", 0, 3),
                        type_args: vec![],
                        args: vec![],
                    }),
                    span: span(0, 10),
                },
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _diags, _, _, _, _, _, _, _) = transform.lower_module(&module);

        assert!(!file.uses.is_empty(), "expected use declarations");
        assert!(
            file.uses
                .iter()
                .any(|u| u.path == "std::collections::HashMap"),
            "expected use std::collections::HashMap"
        );
    }

    // ---- Task 020: T | null → Option lowering tests ----

    // Test 6: Lower `T | null` return type → `Option<T>` in Rust
    #[test]
    fn test_lower_option_return_type() {
        let module = make_module(vec![fn_item(FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("find", 0, 4),
            type_params: None,
            params: vec![],
            return_type: Some(ret_type(TypeAnnotation {
                kind: TypeKind::Union(vec![
                    TypeAnnotation {
                        kind: TypeKind::Named(ident("string", 0, 6)),
                        span: span(0, 6),
                    },
                    TypeAnnotation {
                        kind: TypeKind::Named(ident("null", 9, 13)),
                        span: span(9, 13),
                    },
                ]),
                span: span(0, 13),
            })),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(Expr {
                        kind: ExprKind::NullLit,
                        span: span(20, 24),
                    }),
                    span: span(15, 25),
                })],
                span: span(14, 26),
            },
            doc_comment: None,
            span: span(0, 26),
        })]);

        let mut transform = Transform::new(false);
        let (file, _diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };
        assert_eq!(
            func.return_type,
            Some(RustType::Option(Box::new(RustType::String)))
        );
    }

    // Test 7: Lower `null` literal → `RustExprKind::None`
    #[test]
    fn test_lower_null_literal_to_none() {
        let module = make_module(vec![fn_item(make_fn(
            "test",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::NullLit,
                    span: span(0, 4),
                },
                span: span(0, 5),
            })],
        ))]);

        let mut transform = Transform::new(false);
        let (file, _diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(matches!(let_stmt.init.kind, RustExprKind::None));
            }
            _ => panic!("expected Let"),
        }
    }

    // Test 8: Lower non-null return in Option context → `Some(expr)`
    #[test]
    fn test_lower_return_some_wrapping() {
        let module = make_module(vec![fn_item(FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("find", 0, 4),
            type_params: None,
            params: vec![],
            return_type: Some(ret_type(TypeAnnotation {
                kind: TypeKind::Union(vec![
                    TypeAnnotation {
                        kind: TypeKind::Named(ident("string", 0, 6)),
                        span: span(0, 6),
                    },
                    TypeAnnotation {
                        kind: TypeKind::Named(ident("null", 9, 13)),
                        span: span(9, 13),
                    },
                ]),
                span: span(0, 13),
            })),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(string_expr("hello", 20, 27)),
                    span: span(15, 28),
                })],
                span: span(14, 29),
            },
            doc_comment: None,
            span: span(0, 29),
        })]);

        let mut transform = Transform::new(false);
        let (file, _diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };
        match &func.body.stmts[0] {
            RustStmt::Return(ret) => {
                let val = ret.value.as_ref().expect("expected return value");
                assert!(
                    matches!(val.kind, RustExprKind::Some(_)),
                    "expected Some wrapping, got {:?}",
                    val.kind
                );
            }
            _ => panic!("expected Return"),
        }
    }

    // Test 9: Lower `x !== null` in if-condition → `if let Some(x) = x`
    #[test]
    fn test_lower_null_check_narrowing() {
        let module = make_module(vec![fn_item(make_fn(
            "test",
            vec![],
            None,
            vec![Stmt::If(IfStmt {
                condition: Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Ne,
                        left: Box::new(ident_expr("x", 0, 1)),
                        right: Box::new(Expr {
                            kind: ExprKind::NullLit,
                            span: span(5, 9),
                        }),
                    }),
                    span: span(0, 9),
                },
                then_block: Block {
                    stmts: vec![],
                    span: span(10, 12),
                },
                else_clause: None,
                span: span(0, 12),
            })],
        ))]);

        let mut transform = Transform::new(false);
        let (file, _diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };
        assert!(
            matches!(func.body.stmts[0], RustStmt::IfLet(_)),
            "expected IfLet statement, got {:?}",
            func.body.stmts[0]
        );
        match &func.body.stmts[0] {
            RustStmt::IfLet(if_let) => {
                assert_eq!(if_let.binding, "x");
            }
            _ => panic!("expected IfLet"),
        }
    }

    // Test 10: Lower optional chaining → OptionMap expression
    #[test]
    fn test_lower_optional_chaining() {
        let module = make_module(vec![fn_item(make_fn(
            "test",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::OptionalChain(OptionalChainExpr {
                        object: Box::new(ident_expr("user", 4, 8)),
                        access: OptionalAccess::Field(ident("name", 10, 14)),
                    }),
                    span: span(4, 14),
                },
                span: span(0, 15),
            })],
        ))]);

        let mut transform = Transform::new(false);
        let (file, _diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(
                    matches!(let_stmt.init.kind, RustExprKind::OptionMap { .. }),
                    "expected OptionMap, got {:?}",
                    let_stmt.init.kind
                );
            }
            _ => panic!("expected Let"),
        }
    }

    // Test 11: Lower nullish coalescing → UnwrapOr expression
    #[test]
    fn test_lower_nullish_coalescing() {
        let module = make_module(vec![fn_item(make_fn(
            "test",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 0, 1),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::NullishCoalescing(NullishCoalescingExpr {
                        left: Box::new(ident_expr("name", 4, 8)),
                        right: Box::new(string_expr("Anonymous", 12, 23)),
                    }),
                    span: span(4, 23),
                },
                span: span(0, 24),
            })],
        ))]);

        let mut transform = Transform::new(false);
        let (file, _diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(
                    matches!(let_stmt.init.kind, RustExprKind::UnwrapOr { .. }),
                    "expected UnwrapOr, got {:?}",
                    let_stmt.init.kind
                );
            }
            _ => panic!("expected Let"),
        }
    }

    // --- Task 021: throws → Result with try/catch ---

    // Lower throws function return type to Result<T, E>
    #[test]
    fn test_lower_throws_function_produces_result_return_type() {
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("divide", 0, 6),
            type_params: None,
            params: vec![make_param("a", "f64"), make_param("b", "f64")],
            return_type: Some(ReturnTypeAnnotation {
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("f64", 0, 3)),
                    span: span(0, 3),
                }),
                throws: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }),
                span: span(0, 20),
            }),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(float_expr(1.0, 0, 3)),
                    span: span(0, 10),
                })],
                span: span(0, 20),
            },
            doc_comment: None,
            span: span(0, 50),
        };

        let module = make_module(vec![fn_item(f)]);
        let file = crate::lower(&module).ir;
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };

        assert_eq!(
            func.return_type,
            Some(RustType::Result(
                Box::new(RustType::F64),
                Box::new(RustType::String)
            ))
        );
    }

    // Lower return in throws function to Ok(value)
    #[test]
    fn test_lower_return_in_throws_function_wraps_in_ok() {
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("get", 0, 3),
            type_params: None,
            params: vec![],
            return_type: Some(ReturnTypeAnnotation {
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 0, 3)),
                    span: span(0, 3),
                }),
                throws: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }),
                span: span(0, 20),
            }),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(int_expr(42, 0, 2)),
                    span: span(0, 10),
                })],
                span: span(0, 20),
            },
            doc_comment: None,
            span: span(0, 50),
        };

        let module = make_module(vec![fn_item(f)]);
        let file = crate::lower(&module).ir;
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };

        // The return value should be wrapped in Ok(...)
        match &func.body.stmts[0] {
            RustStmt::Return(ret) => {
                let value = ret.value.as_ref().expect("expected return value");
                assert!(
                    matches!(&value.kind, RustExprKind::Ok(_)),
                    "expected Ok(...), got {:?}",
                    value.kind
                );
            }
            _ => panic!("expected Return"),
        }
    }

    // Lower throw expression to Err
    #[test]
    fn test_lower_throw_expression_produces_return_err() {
        let f = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("fail", 0, 4),
            type_params: None,
            params: vec![],
            return_type: Some(ReturnTypeAnnotation {
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 0, 3)),
                    span: span(0, 3),
                }),
                throws: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }),
                span: span(0, 20),
            }),
            body: Block {
                stmts: vec![Stmt::Expr(Expr {
                    kind: ExprKind::Throw(Box::new(string_expr("oops", 0, 6))),
                    span: span(0, 12),
                })],
                span: span(0, 20),
            },
            doc_comment: None,
            span: span(0, 50),
        };

        let module = make_module(vec![fn_item(f)]);
        let file = crate::lower(&module).ir;
        let func = match &file.items[0] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };

        // throw "oops" → return Err("oops".to_string())
        match &func.body.stmts[0] {
            RustStmt::Return(ret) => {
                let value = ret.value.as_ref().expect("expected return value");
                assert!(
                    matches!(&value.kind, RustExprKind::Err(_)),
                    "expected Err(...), got {:?}",
                    value.kind
                );
            }
            _ => panic!("expected Return, got {:?}", func.body.stmts[0]),
        }
    }

    // Lower call to throws function inside throws function inserts ?
    #[test]
    fn test_lower_call_to_throws_function_inserts_question_mark() {
        let inner_fn = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("inner", 0, 5),
            type_params: None,
            params: vec![],
            return_type: Some(ReturnTypeAnnotation {
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 0, 3)),
                    span: span(0, 3),
                }),
                throws: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }),
                span: span(0, 20),
            }),
            body: Block {
                stmts: vec![Stmt::Return(ReturnStmt {
                    value: Some(int_expr(1, 0, 1)),
                    span: span(0, 5),
                })],
                span: span(0, 20),
            },
            doc_comment: None,
            span: span(0, 50),
        };

        let outer_fn = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("outer", 0, 5),
            type_params: None,
            params: vec![],
            return_type: Some(ReturnTypeAnnotation {
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 0, 3)),
                    span: span(0, 3),
                }),
                throws: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }),
                span: span(0, 20),
            }),
            body: Block {
                stmts: vec![Stmt::VarDecl(VarDecl {
                    binding: VarBinding::Const,
                    name: ident("x", 0, 1),
                    type_ann: None,
                    init: Expr {
                        kind: ExprKind::Call(CallExpr {
                            callee: ident("inner", 0, 5),
                            args: vec![],
                        }),
                        span: span(0, 7),
                    },
                    span: span(0, 10),
                })],
                span: span(0, 20),
            },
            doc_comment: None,
            span: span(0, 50),
        };

        let module = make_module(vec![fn_item(inner_fn), fn_item(outer_fn)]);
        let file = crate::lower(&module).ir;

        // Check outer function
        let func = match &file.items[1] {
            RustItem::Function(f) => f,
            _ => panic!("expected Function"),
        };

        // The var decl init should have ? applied
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(
                    matches!(&let_stmt.init.kind, RustExprKind::QuestionMark(_)),
                    "expected QuestionMark, got {:?}",
                    let_stmt.init.kind
                );
            }
            _ => panic!("expected Let"),
        }
    }

    // Emit Result<T, E> type display
    #[test]
    fn test_rust_type_result_display() {
        let ty = RustType::Result(Box::new(RustType::I32), Box::new(RustType::String));
        assert_eq!(ty.to_string(), "Result<i32, String>");
    }

    // ---------------------------------------------------------------
    // Task 019: Closures and arrow functions
    // ---------------------------------------------------------------

    // Test T19-5: Lower expression-body closure
    #[test]
    fn test_lower_closure_expr_body() {
        let source = "function main() { const double = (x: i32): i32 => x * 2; }";
        let output = compile_and_emit(source);
        assert!(
            output.contains("|x: i32| -> i32"),
            "expected closure in output:\n{output}"
        );
    }

    // Test T19-6: Lower block-body closure
    #[test]
    fn test_lower_closure_block_body() {
        let source = "function main() { const greet = (name: string) => { console.log(name); }; }";
        let output = compile_and_emit(source);
        assert!(
            output.contains("|name: String|"),
            "expected closure params in output:\n{output}"
        );
    }

    // Test T19-7: Lower move closure
    #[test]
    fn test_lower_closure_move() {
        let source = "function main() { const handler = move () => { console.log(\"hi\"); }; }";
        let output = compile_and_emit(source);
        assert!(
            output.contains("move ||"),
            "expected move closure in output:\n{output}"
        );
    }

    // Test T19-11: Function type in parameter lowers to impl Fn
    #[test]
    fn test_lower_function_type_param_to_impl_fn() {
        let source = "function apply(x: i32, f: (i32) => i32): i32 { return f(x); }";
        let output = compile_and_emit(source);
        assert!(
            output.contains("impl Fn(i32) -> i32"),
            "expected impl Fn in output:\n{output}"
        );
    }

    // Test T19-12: Closure captures outer variable — compiles correctly
    #[test]
    fn test_lower_closure_captures_variable() {
        let source = r#"function main() {
            const greeting: string = "Hello";
            const greet = (name: string) => {
                console.log(greeting);
                console.log(name);
            };
            greet("Alice");
        }"#;
        let output = compile_and_emit(source);
        // Should produce a closure that references `greeting`
        assert!(
            output.contains("|name: String|"),
            "expected closure in output:\n{output}"
        );
        assert!(
            output.contains("greeting"),
            "expected greeting reference in output:\n{output}"
        );
    }

    // Test: ImplFn type display
    #[test]
    fn test_rust_type_impl_fn_display() {
        let ty = RustType::ImplFn(vec![RustType::I32, RustType::I32], Box::new(RustType::I32));
        assert_eq!(ty.to_string(), "impl Fn(i32, i32) -> i32");
    }

    // ---- Task 022: Interface lowering tests ----

    #[test]
    fn test_lower_interface_to_trait_with_self_param() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Interface(InterfaceDef {
                    name: ident("Serializable", 0, 12),
                    type_params: None,
                    methods: vec![InterfaceMethod {
                        name: ident("serialize", 15, 24),
                        params: vec![],
                        return_type: Some(ReturnTypeAnnotation {
                            type_ann: Some(TypeAnnotation {
                                kind: TypeKind::Named(ident("string", 28, 34)),
                                span: span(28, 34),
                            }),
                            throws: None,
                            span: span(28, 34),
                        }),
                        span: span(15, 35),
                    }],
                    fields: vec![],
                    doc_comment: None,
                    span: span(0, 37),
                }),
                exported: false,
                decorators: vec![],
                span: span(0, 37),
            }],
            span: span(0, 37),
        };

        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.items.len(), 1);
        match &file.items[0] {
            RustItem::Trait(t) => {
                assert_eq!(t.name, "Serializable");
                assert_eq!(t.methods.len(), 1);
                assert_eq!(t.methods[0].name, "serialize");
                assert!(t.methods[0].has_self);
                assert_eq!(t.methods[0].return_type, Some(RustType::String));
            }
            other => panic!("expected Trait, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_interface_self_return_type_to_self() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Interface(InterfaceDef {
                    name: ident("Cloneable", 0, 9),
                    type_params: None,
                    methods: vec![InterfaceMethod {
                        name: ident("clone", 12, 17),
                        params: vec![],
                        return_type: Some(ReturnTypeAnnotation {
                            type_ann: Some(TypeAnnotation {
                                kind: TypeKind::Named(ident("Self", 21, 25)),
                                span: span(21, 25),
                            }),
                            throws: None,
                            span: span(21, 25),
                        }),
                        span: span(12, 26),
                    }],
                    fields: vec![],
                    doc_comment: None,
                    span: span(0, 28),
                }),
                exported: false,
                decorators: vec![],
                span: span(0, 28),
            }],
            span: span(0, 28),
        };

        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        match &file.items[0] {
            RustItem::Trait(t) => {
                assert_eq!(t.methods[0].return_type, Some(RustType::SelfType));
            }
            other => panic!("expected Trait, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_intersection_type_parameter_to_generic_bounds() {
        let module = Module {
            items: vec![
                Item {
                    kind: ItemKind::Interface(InterfaceDef {
                        name: ident("Serializable", 0, 12),
                        type_params: None,
                        methods: vec![InterfaceMethod {
                            name: ident("serialize", 15, 24),
                            params: vec![],
                            return_type: Some(ReturnTypeAnnotation {
                                type_ann: Some(TypeAnnotation {
                                    kind: TypeKind::Named(ident("string", 28, 34)),
                                    span: span(28, 34),
                                }),
                                throws: None,
                                span: span(28, 34),
                            }),
                            span: span(15, 35),
                        }],
                        fields: vec![],
                        doc_comment: None,
                        span: span(0, 37),
                    }),
                    exported: false,
                    decorators: vec![],
                    span: span(0, 37),
                },
                Item {
                    kind: ItemKind::Interface(InterfaceDef {
                        name: ident("Printable", 40, 49),
                        type_params: None,
                        methods: vec![InterfaceMethod {
                            name: ident("print", 52, 57),
                            params: vec![],
                            return_type: None,
                            span: span(52, 60),
                        }],
                        fields: vec![],
                        doc_comment: None,
                        span: span(40, 62),
                    }),
                    exported: false,
                    decorators: vec![],
                    span: span(40, 62),
                },
                Item {
                    kind: ItemKind::Function(FnDecl {
                        is_async: false,
                        is_generator: false,
                        name: ident("process", 65, 72),
                        type_params: None,
                        params: vec![Param {
                            name: ident("input", 73, 78),
                            type_ann: TypeAnnotation {
                                kind: TypeKind::Intersection(vec![
                                    TypeAnnotation {
                                        kind: TypeKind::Named(ident("Serializable", 80, 92)),
                                        span: span(80, 92),
                                    },
                                    TypeAnnotation {
                                        kind: TypeKind::Named(ident("Printable", 95, 104)),
                                        span: span(95, 104),
                                    },
                                ]),
                                span: span(80, 104),
                            },
                            optional: false,
                            default_value: None,
                            is_rest: false,
                            span: span(73, 104),
                        }],
                        return_type: Some(ReturnTypeAnnotation {
                            type_ann: Some(TypeAnnotation {
                                kind: TypeKind::Named(ident("string", 107, 113)),
                                span: span(107, 113),
                            }),
                            throws: None,
                            span: span(107, 113),
                        }),
                        body: Block {
                            stmts: vec![Stmt::Return(ReturnStmt {
                                value: Some(Expr {
                                    kind: ExprKind::MethodCall(MethodCallExpr {
                                        object: Box::new(Expr {
                                            kind: ExprKind::Ident(ident("input", 130, 135)),
                                            span: span(130, 135),
                                        }),
                                        method: ident("serialize", 136, 145),
                                        args: vec![],
                                    }),
                                    span: span(130, 147),
                                }),
                                span: span(123, 148),
                            })],
                            span: span(115, 150),
                        },
                        doc_comment: None,
                        span: span(65, 150),
                    }),
                    exported: false,
                    decorators: vec![],
                    span: span(65, 150),
                },
            ],
            span: span(0, 150),
        };

        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

        // The function should have a generated type parameter T with bounds
        let func = match &file.items[2] {
            RustItem::Function(f) => f,
            other => panic!("expected Function, got {other:?}"),
        };
        // Should have a fresh type parameter T: Serializable + Printable
        assert_eq!(func.type_params.len(), 1);
        assert_eq!(func.type_params[0].name, "T");
        assert_eq!(func.type_params[0].bounds.len(), 2);
        assert!(
            func.type_params[0]
                .bounds
                .contains(&"Serializable".to_owned())
        );
        assert!(func.type_params[0].bounds.contains(&"Printable".to_owned()));
        // Parameter should use the type parameter
        assert_eq!(func.params[0].ty, RustType::TypeParam("T".to_owned()));
    }

    // ---------------------------------------------------------------
    // Task 018: For-of loops, break, continue lowering
    // ---------------------------------------------------------------

    // T018-5: Lower for-of → RustForInStmt with iterable
    #[test]
    fn test_lower_for_of_produces_for_in_stmt() {
        let source = r#"function main() {
  const items: Array<i32> = [1, 2, 3];
  for (const x of items) {
    console.log(x);
  }
}"#;
        let output = compile_and_emit(source);
        assert!(
            output.contains("for &x in &items"),
            "expected `for &x in &items` in output, got:\n{output}"
        );
    }

    // T018-6: Lower break → RustStmt::Break
    #[test]
    fn test_lower_break_produces_break_stmt() {
        let source = r#"function main() {
  while (true) {
    break;
  }
}"#;
        let output = compile_and_emit(source);
        assert!(
            output.contains("break;"),
            "expected `break;` in output, got:\n{output}"
        );
    }

    // T018-7: Lower continue → RustStmt::Continue
    #[test]
    fn test_lower_continue_produces_continue_stmt() {
        let source = r#"function main() {
  while (true) {
    continue;
  }
}"#;
        let output = compile_and_emit(source);
        assert!(
            output.contains("continue;"),
            "expected `continue;` in output, got:\n{output}"
        );
    }

    // ---------------------------------------------------------------
    // Task 024: import/export lowering
    // ---------------------------------------------------------------

    // Test 5: Lower import → RustUseDecl with correct path
    #[test]
    fn test_lower_import_produces_use_decl() {
        let source = r#"import { User } from "./models";
function main() {}"#;
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        let ir = crate::lower(&module).ir;
        let use_paths: Vec<&str> = ir.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            use_paths.contains(&"crate::models::User"),
            "expected use crate::models::User in uses, got: {use_paths:?}"
        );
    }

    // Test 6: Lower exported function → RustFnDecl with public: true
    #[test]
    fn test_lower_exported_function_is_public() {
        let source = "export function greet(): void { return; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        let ir = crate::lower(&module).ir;
        assert_eq!(ir.items.len(), 1);
        match &ir.items[0] {
            RustItem::Function(f) => {
                assert!(f.public, "exported function should be public");
                assert_eq!(f.name, "greet");
            }
            other => panic!("expected Function item, got {other:?}"),
        }
    }

    // Test 7: Lower non-exported function → RustFnDecl with public: false
    #[test]
    fn test_lower_non_exported_function_is_not_public() {
        let source = "function helper(): i32 { return 42; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        let ir = crate::lower(&module).ir;
        assert_eq!(ir.items.len(), 1);
        match &ir.items[0] {
            RustItem::Function(f) => {
                assert!(!f.public, "non-exported function should not be public");
            }
            other => panic!("expected Function item, got {other:?}"),
        }
    }

    // Test 8: Lower re-export → pub use
    #[test]
    fn test_lower_re_export_produces_pub_use() {
        let source = "export { User } from \"./models\";";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        let ir = crate::lower(&module).ir;
        let pub_uses: Vec<&RustUseDecl> = ir.uses.iter().filter(|u| u.public).collect();
        assert_eq!(pub_uses.len(), 1, "expected one pub use declaration");
        assert_eq!(pub_uses[0].path, "crate::models::User");
    }

    // Test: Lower wildcard re-export → pub use module::*
    #[test]
    fn test_lower_wildcard_re_export_local() {
        let source = r#"export * from "./utils";"#;
        let result = crate::lower(&parse_module(source));
        let pub_uses: Vec<&RustUseDecl> = result.ir.uses.iter().filter(|u| u.public).collect();
        assert_eq!(pub_uses.len(), 1, "expected one pub use declaration");
        assert_eq!(pub_uses[0].path, "crate::utils::*");
    }

    // Test: Lower wildcard re-export from external crate
    #[test]
    fn test_lower_wildcard_re_export_external() {
        let source = r#"export * from "serde";"#;
        let result = crate::lower(&parse_module(source));
        let pub_uses: Vec<&RustUseDecl> = result.ir.uses.iter().filter(|u| u.public).collect();
        assert_eq!(pub_uses.len(), 1, "expected one pub use declaration");
        assert_eq!(pub_uses[0].path, "serde::*");
        assert_eq!(result.crate_dependencies.len(), 1);
        assert_eq!(result.crate_dependencies[0].name, "serde");
    }

    // Test: Lower wildcard re-export from std library
    #[test]
    fn test_lower_wildcard_re_export_std() {
        let source = r#"export * from "std/collections";"#;
        let result = crate::lower(&parse_module(source));
        let pub_uses: Vec<&RustUseDecl> = result.ir.uses.iter().filter(|u| u.public).collect();
        assert_eq!(pub_uses.len(), 1, "expected one pub use declaration");
        assert_eq!(pub_uses[0].path, "std::collections::*");
        assert!(
            result.crate_dependencies.is_empty(),
            "std imports should not produce crate dependencies"
        );
    }

    // Test: Lower exported type → public struct
    #[test]
    fn test_lower_exported_type_is_public() {
        let source = "export type User = { name: string, age: u32 }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        let ir = crate::lower(&module).ir;
        match &ir.items[0] {
            RustItem::Struct(s) => {
                assert!(s.public, "exported type should be public");
                assert_eq!(s.name, "User");
            }
            other => panic!("expected Struct item, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 023: Class lowering tests
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_class_produces_struct_and_impl() {
        let source = "\
class Counter {
  private count: i32;
  constructor(initial: i32) {
    this.count = initial;
  }
  get(): i32 {
    return this.count;
  }
}";
        let file = lower_source(source);
        // Should produce: struct + impl block = 2 items
        assert!(file.items.len() >= 2, "expected at least 2 items");

        // First item: struct
        match &file.items[0] {
            RustItem::Struct(s) => {
                assert_eq!(s.name, "Counter");
                assert_eq!(s.fields.len(), 1);
                assert_eq!(s.fields[0].name, "count");
                assert!(!s.fields[0].public, "private field should not be pub");
            }
            other => panic!("expected Struct, got {other:?}"),
        }

        // Second item: impl block
        match &file.items[1] {
            RustItem::Impl(imp) => {
                assert_eq!(imp.type_name, "Counter");
                // Should have `new` + `get` = 2 methods
                assert_eq!(imp.methods.len(), 2);
                assert_eq!(imp.methods[0].name, "new");
                assert!(
                    imp.methods[0].self_param.is_none(),
                    "new should not have self"
                );
                assert_eq!(imp.methods[1].name, "get");
                assert_eq!(
                    imp.methods[1].self_param,
                    Some(RustSelfParam::Ref),
                    "get should be &self"
                );
            }
            other => panic!("expected Impl, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_class_constructor_produces_new_with_self_type() {
        let source = "\
class Point {
  public x: f64;
  public y: f64;
  constructor(x: f64, y: f64) {
    this.x = x;
    this.y = y;
  }
}";
        let file = lower_source(source);
        match &file.items[1] {
            RustItem::Impl(imp) => {
                let new_method = &imp.methods[0];
                assert_eq!(new_method.name, "new");
                assert_eq!(new_method.return_type, Some(RustType::SelfType));
                assert_eq!(new_method.params.len(), 2);
            }
            other => panic!("expected Impl, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_class_mutating_method_gets_mut_self() {
        let source = "\
class Counter {
  private count: i32;
  constructor(initial: i32) {
    this.count = initial;
  }
  increment(): void {
    this.count = this.count + 1;
  }
}";
        let file = lower_source(source);
        match &file.items[1] {
            RustItem::Impl(imp) => {
                let increment = imp.methods.iter().find(|m| m.name == "increment");
                assert!(increment.is_some(), "should have increment method");
                assert_eq!(
                    increment.unwrap().self_param,
                    Some(RustSelfParam::RefMut),
                    "mutating method should be &mut self"
                );
            }
            other => panic!("expected Impl, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_class_implements_generates_trait_impl() {
        let source = "\
interface Describable {
  describe(): string;
}
class User implements Describable {
  public name: string;
  constructor(name: string) {
    this.name = name;
  }
  describe(): string {
    return this.name;
  }
}";
        let file = lower_source(source);
        // Should produce: trait + struct + inherent impl + trait impl = 4 items
        assert_eq!(
            file.items.len(),
            4,
            "expected 4 items (trait + struct + impl + trait_impl)"
        );

        match &file.items[3] {
            RustItem::TraitImpl(ti) => {
                assert_eq!(ti.trait_name, "Describable");
                assert_eq!(ti.type_name, "User");
                assert_eq!(ti.methods.len(), 1);
                assert_eq!(ti.methods[0].name, "describe");
            }
            other => panic!("expected TraitImpl, got {other:?}"),
        }
    }

    #[test]
    fn test_lower_class_private_field_not_pub() {
        let source = "\
class Foo {
  private x: i32;
  public y: i32;
  constructor() {
  }
}";
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Struct(s) => {
                assert!(!s.fields[0].public, "private field should not be pub");
                assert!(s.fields[1].public, "public field should be pub");
            }
            other => panic!("expected Struct, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Async/await lowering tests (Task 028)
    // ---------------------------------------------------------------

    // 9. Lowering passthrough: async function AST → RustFnDecl { is_async: true }
    #[test]
    fn test_lower_async_function_produces_async_rust_fn() {
        let source = r#"async function greet(): string { return "hello"; }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                assert!(f.is_async, "expected is_async to be true");
                assert_eq!(f.name, "greet");
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Non-async function stays is_async: false
    #[test]
    fn test_lower_non_async_function_has_is_async_false() {
        let source = "function add(a: i32, b: i32): i32 { return a + b; }";
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                assert!(!f.is_async, "expected is_async to be false");
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // 10. Lowering await: Await(expr) → RustExprKind::Await(lowered_expr)
    #[test]
    fn test_lower_await_expression_produces_rust_await() {
        let source = r#"async function fetchData(): string {
            const result = await getData();
            return result;
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                assert!(f.is_async);
                // First statement should be a let binding with await init
                match &f.body.stmts[0] {
                    RustStmt::Let(let_stmt) => {
                        match &let_stmt.init.kind {
                            RustExprKind::Await(inner) => {
                                // Inner should be a function call to getData
                                match &inner.kind {
                                    RustExprKind::Call { func, .. } => {
                                        assert_eq!(func, "getData");
                                    }
                                    other => panic!("expected Call inside Await, got {other:?}"),
                                }
                            }
                            other => panic!("expected Await, got {other:?}"),
                        }
                    }
                    other => panic!("expected Let, got {other:?}"),
                }
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Await on external (unknown) async call in throws function → `.await?`
    #[test]
    fn test_lower_await_external_call_in_throws_fn_adds_question_mark() {
        let source = r#"async function startServer(): void throws string {
            const listener = await externalAsyncFn();
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                // First statement should be a let binding
                match &f.body.stmts[0] {
                    RustStmt::Let(let_stmt) => {
                        // The init should be QuestionMark(Await(Call))
                        match &let_stmt.init.kind {
                            RustExprKind::QuestionMark(inner) => {
                                assert!(
                                    matches!(&inner.kind, RustExprKind::Await(_)),
                                    "expected Await inside QuestionMark, got {:?}",
                                    inner.kind
                                );
                            }
                            other => panic!("expected QuestionMark(Await(...)), got {other:?}"),
                        }
                    }
                    other => panic!("expected Let, got {other:?}"),
                }
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Await on known throws async call in throws function → `.await` (no double `?`)
    #[test]
    fn test_lower_await_known_throws_call_no_double_question_mark() {
        let source = r#"
            async function inner(): i32 throws string {
                return 42;
            }
            async function outer(): i32 throws string {
                const x = await inner();
                return x;
            }
        "#;
        let file = lower_source(source);
        // outer is the second function (index 1)
        match &file.items[1] {
            RustItem::Function(f) => {
                match &f.body.stmts[0] {
                    RustStmt::Let(let_stmt) => {
                        // The init should be QuestionMark(Await(Call)) — `.await?`
                        // For async throws functions, await first then unwrap.
                        match &let_stmt.init.kind {
                            RustExprKind::QuestionMark(inner) => {
                                assert!(
                                    matches!(&inner.kind, RustExprKind::Await(_)),
                                    "expected Await inside QuestionMark, got {:?}",
                                    inner.kind
                                );
                            }
                            other => panic!("expected QuestionMark(Await(...)), got {other:?}"),
                        }
                    }
                    other => panic!("expected Let, got {other:?}"),
                }
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Await on external async call in NON-throws function → `.await` (no `?`)
    #[test]
    fn test_lower_await_external_call_in_non_throws_fn_no_question_mark() {
        let source = r#"async function doStuff(): string {
            const result = await externalAsyncFn();
            return result;
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                match &f.body.stmts[0] {
                    RustStmt::Let(let_stmt) => {
                        // The init should be Await(Call) — no QuestionMark
                        match &let_stmt.init.kind {
                            RustExprKind::Await(inner) => {
                                assert!(
                                    matches!(&inner.kind, RustExprKind::Call { .. }),
                                    "expected Call inside Await, got {:?}",
                                    inner.kind
                                );
                            }
                            other => panic!("expected Await(Call(...)), got {other:?}"),
                        }
                    }
                    other => panic!("expected Let, got {other:?}"),
                }
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Async closure lowering: is_async passthrough
    #[test]
    fn test_lower_async_closure_produces_async_rust_closure() {
        let source = r#"function test() {
            const handler = async () => {
                await processRequest();
            };
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[0] {
                RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                    RustExprKind::Closure { is_async, .. } => {
                        assert!(*is_async, "expected closure is_async to be true");
                    }
                    other => panic!("expected Closure, got {other:?}"),
                },
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 031: Crate consumption — external dependency import mapping
    // ---------------------------------------------------------------

    // Test 1: Local import unchanged — "./module" → use crate::module::X
    #[test]
    fn test_lower_local_import_unchanged() {
        let source = r#"import { User } from "./models";
function main() {}"#;
        let ir = crate::lower(&parse_module(source)).ir;
        let use_paths: Vec<&str> = ir.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            use_paths.contains(&"crate::models::User"),
            "expected use crate::models::User, got: {use_paths:?}"
        );
    }

    // Test 2: Std import — "std/collections" → use std::collections::HashMap, no dependency
    #[test]
    fn test_lower_std_import_produces_std_use_path() {
        let source = r#"import { HashMap } from "std/collections";
function main() {}"#;
        let result = crate::lower(&parse_module(source));
        let use_paths: Vec<&str> = result.ir.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            use_paths.contains(&"std::collections::HashMap"),
            "expected use std::collections::HashMap, got: {use_paths:?}"
        );
        assert!(
            result.crate_dependencies.is_empty(),
            "std imports should not produce crate dependencies, got: {:?}",
            result.crate_dependencies
        );
    }

    // Test 3: External crate import — "reqwest" → use reqwest::get + dependency
    #[test]
    fn test_lower_external_crate_import_produces_use_and_dependency() {
        let source = r#"import { get } from "reqwest";
function main() {}"#;
        let result = crate::lower(&parse_module(source));
        let use_paths: Vec<&str> = result.ir.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            use_paths.contains(&"reqwest::get"),
            "expected use reqwest::get, got: {use_paths:?}"
        );
        assert_eq!(result.crate_dependencies.len(), 1);
        assert_eq!(result.crate_dependencies[0].name, "reqwest");
    }

    // Test 4: Nested crate import — "tokio/sync" → use tokio::sync::channel
    #[test]
    fn test_lower_nested_crate_import() {
        let source = r#"import { channel } from "tokio/sync";
function main() {}"#;
        let result = crate::lower(&parse_module(source));
        let use_paths: Vec<&str> = result.ir.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            use_paths.contains(&"tokio::sync::channel"),
            "expected use tokio::sync::channel, got: {use_paths:?}"
        );
        assert_eq!(result.crate_dependencies.len(), 1);
        assert_eq!(result.crate_dependencies[0].name, "tokio");
    }

    // Test 5: Multiple imports from same crate → one dependency entry
    #[test]
    fn test_lower_multiple_imports_same_crate_one_dependency() {
        let source = r#"import { Serialize } from "serde";
import { Deserialize } from "serde";
function main() {}"#;
        let result = crate::lower(&parse_module(source));
        let use_paths: Vec<&str> = result.ir.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            use_paths.contains(&"serde::Serialize"),
            "expected use serde::Serialize, got: {use_paths:?}"
        );
        assert!(
            use_paths.contains(&"serde::Deserialize"),
            "expected use serde::Deserialize, got: {use_paths:?}"
        );
        assert_eq!(
            result.crate_dependencies.len(),
            1,
            "expected one dependency for two imports from serde, got: {:?}",
            result.crate_dependencies
        );
    }

    // Test 6: Multiple different crates → multiple dependency entries
    #[test]
    fn test_lower_multiple_crates_multiple_dependencies() {
        let source = r#"import { get } from "reqwest";
import { Serialize } from "serde";
function main() {}"#;
        let result = crate::lower(&parse_module(source));
        assert_eq!(result.crate_dependencies.len(), 2);
        let dep_names: Vec<&str> = result
            .crate_dependencies
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(dep_names.contains(&"reqwest"));
        assert!(dep_names.contains(&"serde"));
    }

    // Test 7: Re-export from external crate → pub use + dependency
    #[test]
    fn test_lower_re_export_from_external_crate() {
        let source = r#"export { Value } from "serde_json";"#;
        let result = crate::lower(&parse_module(source));
        let pub_uses: Vec<&RustUseDecl> = result.ir.uses.iter().filter(|u| u.public).collect();
        assert_eq!(pub_uses.len(), 1, "expected one pub use declaration");
        assert_eq!(pub_uses[0].path, "serde_json::Value");
        assert_eq!(result.crate_dependencies.len(), 1);
        assert_eq!(result.crate_dependencies[0].name, "serde_json");
    }

    // Test 8: Crate name normalization — hyphens to underscores
    #[test]
    fn test_lower_crate_name_normalization_hyphen_to_underscore() {
        let source = r#"import { Value } from "serde-json";
function main() {}"#;
        let result = crate::lower(&parse_module(source));
        let use_paths: Vec<&str> = result.ir.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            use_paths.contains(&"serde_json::Value"),
            "expected use serde_json::Value, got: {use_paths:?}"
        );
        assert_eq!(result.crate_dependencies.len(), 1);
        assert_eq!(result.crate_dependencies[0].name, "serde_json");
    }

    // Test 13: std/concurrent builtin — no use declaration, no dependency
    #[test]
    fn test_lower_std_concurrent_builtin_no_use_no_dependency() {
        let source = r#"import { spawn } from "std/concurrent";
function main() {}"#;
        let result = crate::lower(&parse_module(source));
        let use_paths: Vec<&str> = result.ir.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            !use_paths.iter().any(|p| p.contains("concurrent")),
            "std/concurrent should not produce a use declaration, got: {use_paths:?}"
        );
        assert!(
            result.crate_dependencies.is_empty(),
            "std/concurrent should not produce a dependency, got: {:?}",
            result.crate_dependencies
        );
    }

    // Helper for Task 031 tests
    fn parse_module(source: &str) -> rsc_syntax::ast::Module {
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        module
    }

    // ---------------------------------------------------------------
    // Task 029: Async lowering and tokio runtime integration
    // ---------------------------------------------------------------

    // Test 3: Lowering — tokio::main attribute on async main
    #[test]
    fn test_lower_async_main_gets_tokio_main_attribute() {
        let source = r#"async function main() {
            const data = await fetchData();
            console.log(data);
        }
        async function fetchData(): string {
            return "hello from async";
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                assert_eq!(f.name, "main");
                assert!(f.is_async, "expected main to be async");
                assert_eq!(f.attributes.len(), 1, "expected 1 attribute on async main");
                assert_eq!(f.attributes[0].path, "tokio::main");
                assert!(
                    f.attributes[0].args.is_none(),
                    "expected no args on tokio::main"
                );
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test 4: Lowering — non-main async function does NOT get #[tokio::main]
    #[test]
    fn test_lower_async_non_main_no_tokio_attribute() {
        let source = r#"async function fetchData(): string {
            return "hello";
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                assert_eq!(f.name, "fetchData");
                assert!(f.is_async, "expected fetchData to be async");
                assert!(
                    f.attributes.is_empty(),
                    "expected no attributes on non-main async fn, got: {:?}",
                    f.attributes
                );
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test 5: Lowering — async flag propagation: needs_async_runtime = true
    #[test]
    fn test_lower_module_with_async_fn_sets_needs_async_runtime() {
        let source = r#"async function fetchData(): string {
            return "hello";
        }"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, needs_async_runtime, _, _, _, _, _) = transform.lower_module(&module);
        assert!(
            needs_async_runtime,
            "expected needs_async_runtime to be true when async function exists"
        );
    }

    // Test 6: Lowering — no async flag: needs_async_runtime = false
    #[test]
    fn test_lower_module_without_async_fn_clears_needs_async_runtime() {
        let source = "function add(a: i32, b: i32): i32 { return a + b; }";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, needs_async_runtime, _, _, _, _, _) = transform.lower_module(&module);
        assert!(
            !needs_async_runtime,
            "expected needs_async_runtime to be false when no async function exists"
        );
    }

    // ---------------------------------------------------------------
    // Task 033: Collection method integration tests
    // ---------------------------------------------------------------

    // Test: arr.map(x => x * 2) produces IteratorChain
    #[test]
    fn test_lower_array_map_produces_iterator_chain_ir() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3];
            const doubled = arr.map((x: i32): i32 => x * 2);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                // Second statement should be the map
                match &f.body.stmts[1] {
                    RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                        RustExprKind::IteratorChain { ops, terminal, .. } => {
                            assert!(!ops.is_empty(), "expected at least one iterator op (Map)");
                            assert!(
                                matches!(
                                    terminal,
                                    rsc_syntax::rust_ir::IteratorTerminal::CollectVec
                                ),
                                "expected CollectVec terminal"
                            );
                        }
                        other => panic!("expected IteratorChain, got {other:?}"),
                    },
                    other => panic!("expected Let, got {other:?}"),
                }
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: arr.filter(x => x > 0) produces IteratorChain with Cloned
    #[test]
    fn test_lower_array_filter_produces_iterator_chain_ir() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3];
            const pos = arr.filter((x: i32): bool => x > 0);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[1] {
                RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                    RustExprKind::IteratorChain { ops, .. } => {
                        assert!(
                            ops.iter()
                                .any(|op| matches!(op, rsc_syntax::rust_ir::IteratorOp::Cloned)),
                            "expected Cloned op in filter chain"
                        );
                    }
                    other => panic!("expected IteratorChain, got {other:?}"),
                },
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: arr.reduce((acc, x) => acc + x, 0) produces IteratorChain with Fold
    #[test]
    fn test_lower_array_reduce_produces_fold_terminal() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3];
            const sum = arr.reduce((acc: i32, x: i32): i32 => acc + x, 0);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[1] {
                RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                    RustExprKind::IteratorChain { terminal, .. } => {
                        assert!(
                            matches!(terminal, rsc_syntax::rust_ir::IteratorTerminal::Fold { .. }),
                            "expected Fold terminal"
                        );
                    }
                    other => panic!("expected IteratorChain, got {other:?}"),
                },
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: arr.find(x => x > 3) produces IteratorChain with Find
    #[test]
    fn test_lower_array_find_produces_find_terminal() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3, 4, 5];
            const found = arr.find((x: i32): bool => x > 3);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[1] {
                RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                    RustExprKind::IteratorChain { terminal, .. } => {
                        assert!(
                            matches!(terminal, rsc_syntax::rust_ir::IteratorTerminal::Find(..)),
                            "expected Find terminal"
                        );
                    }
                    other => panic!("expected IteratorChain, got {other:?}"),
                },
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: arr.some(x => x > 5) produces IteratorChain with Any
    #[test]
    fn test_lower_array_some_produces_any_terminal() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3];
            const has = arr.some((x: i32): bool => x > 5);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[1] {
                RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                    RustExprKind::IteratorChain { terminal, .. } => {
                        assert!(
                            matches!(terminal, rsc_syntax::rust_ir::IteratorTerminal::Any(..)),
                            "expected Any terminal"
                        );
                    }
                    other => panic!("expected IteratorChain, got {other:?}"),
                },
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: arr.every(x => x > 0) produces IteratorChain with All
    #[test]
    fn test_lower_array_every_produces_all_terminal() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3];
            const all = arr.every((x: i32): bool => x > 0);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[1] {
                RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                    RustExprKind::IteratorChain { terminal, .. } => {
                        assert!(
                            matches!(terminal, rsc_syntax::rust_ir::IteratorTerminal::All(..)),
                            "expected All terminal"
                        );
                    }
                    other => panic!("expected IteratorChain, got {other:?}"),
                },
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: arr.forEach(x => ...) produces IteratorChain with ForEach
    #[test]
    fn test_lower_array_for_each_produces_for_each_terminal() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3];
            arr.forEach((x: i32): void => console.log(x));
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => {
                // forEach is a statement, so look at stmts[1]
                match &f.body.stmts[1] {
                    RustStmt::Semi(expr) => match &expr.kind {
                        RustExprKind::IteratorChain { terminal, .. } => {
                            assert!(
                                matches!(
                                    terminal,
                                    rsc_syntax::rust_ir::IteratorTerminal::ForEach(..)
                                ),
                                "expected ForEach terminal"
                            );
                        }
                        other => panic!("expected IteratorChain, got {other:?}"),
                    },
                    other => panic!("expected Semi, got {other:?}"),
                }
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: chained map+filter produces single IteratorChain with multiple ops
    #[test]
    fn test_lower_chained_map_filter_produces_single_chain() {
        let source = r#"function main() {
            const arr: Array<i32> = [1, 2, 3, 4, 5];
            const result = arr.map((x: i32): i32 => x * 2).filter((x: i32): bool => x > 4);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[1] {
                RustStmt::Let(let_stmt) => match &let_stmt.init.kind {
                    RustExprKind::IteratorChain { ops, terminal, .. } => {
                        // Should have: Map, Filter, Cloned
                        let has_map = ops
                            .iter()
                            .any(|op| matches!(op, rsc_syntax::rust_ir::IteratorOp::Map(..)));
                        let has_filter = ops
                            .iter()
                            .any(|op| matches!(op, rsc_syntax::rust_ir::IteratorOp::Filter(..)));
                        assert!(has_map, "expected Map op in chain");
                        assert!(has_filter, "expected Filter op in chain");
                        assert!(
                            matches!(terminal, rsc_syntax::rust_ir::IteratorTerminal::CollectVec),
                            "expected CollectVec terminal"
                        );
                    }
                    other => panic!("expected IteratorChain, got {other:?}"),
                },
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test: non-collection method falls through to regular method call
    #[test]
    fn test_lower_non_collection_method_falls_through() {
        let source = r#"function main() {
            const obj: Array<i32> = [1, 2, 3];
            const result = obj.customMethod();
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[1] {
                RustStmt::Let(let_stmt) => {
                    assert!(
                        matches!(
                            &let_stmt.init.kind,
                            RustExprKind::MethodCall { method, .. } if method == "customMethod"
                        ),
                        "expected regular MethodCall for unknown method, got {:?}",
                        let_stmt.init.kind
                    );
                }
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 030: Promise.all, spawn, and concurrency tests
    // ---------------------------------------------------------------

    // Test 1: Promise.all basic: await Promise.all([a(), b()]) → tokio::join!(a(), b())
    #[test]
    fn test_lower_promise_all_basic_produces_tokio_join() {
        let source = r#"async function main() {
            await Promise.all([getUser(), getPosts()]);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[0] {
                RustStmt::Semi(expr) => match &expr.kind {
                    RustExprKind::TokioJoin {
                        elements,
                        throwing_elements,
                    } => {
                        assert_eq!(elements.len(), 2, "expected 2 futures in tokio::join!");
                        assert!(
                            matches!(&elements[0].kind, RustExprKind::Call { func, .. } if func == "getUser"),
                            "expected getUser call"
                        );
                        assert!(
                            matches!(&elements[1].kind, RustExprKind::Call { func, .. } if func == "getPosts"),
                            "expected getPosts call"
                        );
                        assert!(
                            throwing_elements.iter().all(|t| !t),
                            "non-throws functions should not be marked throwing"
                        );
                    }
                    other => panic!("expected TokioJoin, got {other:?}"),
                },
                other => panic!("expected Semi, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test 2: Promise.all with destructuring
    #[test]
    fn test_lower_promise_all_with_destructuring_produces_tuple_destructure() {
        let source = r#"async function main() {
            const [user, posts] = await Promise.all([getUser(), getPosts()]);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[0] {
                RustStmt::TupleDestructure(td) => {
                    assert_eq!(td.bindings, vec!["user", "posts"]);
                    assert!(!td.mutable);
                    match &td.init.kind {
                        RustExprKind::TokioJoin { elements, .. } => {
                            assert_eq!(elements.len(), 2);
                        }
                        other => panic!("expected TokioJoin, got {other:?}"),
                    }
                }
                other => panic!("expected TupleDestructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test 3: Promise.all three futures
    #[test]
    fn test_lower_promise_all_three_futures_produces_tokio_join_three() {
        let source = r#"async function main() {
            const [a, b, c] = await Promise.all([getA(), getB(), getC()]);
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[0] {
                RustStmt::TupleDestructure(td) => {
                    assert_eq!(td.bindings, vec!["a", "b", "c"]);
                    match &td.init.kind {
                        RustExprKind::TokioJoin { elements, .. } => {
                            assert_eq!(elements.len(), 3, "expected 3 futures in tokio::join!");
                        }
                        other => panic!("expected TokioJoin, got {other:?}"),
                    }
                }
                other => panic!("expected TupleDestructure, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test 4: spawn basic: spawn(async () => { work(); }) → tokio::spawn(async move { work(); })
    #[test]
    fn test_lower_spawn_basic_produces_tokio_spawn() {
        let source = r#"async function main() {
            spawn(async () => {
                doWork();
            });
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[0] {
                RustStmt::Semi(expr) => match &expr.kind {
                    RustExprKind::Call { func, args } => {
                        assert_eq!(func, "tokio::spawn");
                        assert_eq!(args.len(), 1);
                        match &args[0].kind {
                            RustExprKind::AsyncBlock { is_move, body } => {
                                assert!(is_move, "spawn should add move to async block");
                                assert!(!body.stmts.is_empty(), "body should have statements");
                            }
                            other => panic!("expected AsyncBlock, got {other:?}"),
                        }
                    }
                    other => panic!("expected Call, got {other:?}"),
                },
                other => panic!("expected Semi, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test 5: spawn with await inside
    #[test]
    fn test_lower_spawn_with_await_produces_tokio_spawn_with_await() {
        let source = r#"async function main() {
            spawn(async () => {
                await asyncWork();
            });
        }"#;
        let file = lower_source(source);
        match &file.items[0] {
            RustItem::Function(f) => match &f.body.stmts[0] {
                RustStmt::Semi(expr) => match &expr.kind {
                    RustExprKind::Call { func, args } => {
                        assert_eq!(func, "tokio::spawn");
                        match &args[0].kind {
                            RustExprKind::AsyncBlock { is_move, body } => {
                                assert!(is_move);
                                // The body should contain an await expression
                                match &body.stmts[0] {
                                    RustStmt::Semi(inner) => {
                                        assert!(
                                            matches!(&inner.kind, RustExprKind::Await(_)),
                                            "expected Await inside spawn body"
                                        );
                                    }
                                    other => panic!("expected Semi, got {other:?}"),
                                }
                            }
                            other => panic!("expected AsyncBlock, got {other:?}"),
                        }
                    }
                    other => panic!("expected Call, got {other:?}"),
                },
                other => panic!("expected Semi, got {other:?}"),
            },
            other => panic!("expected Function, got {other:?}"),
        }
    }

    // Test 6: import { spawn } from "std/concurrent" does not produce use declaration
    #[test]
    fn test_lower_std_concurrent_import_no_use_declaration() {
        let source = r#"import { spawn } from "std/concurrent";
async function main() {
    spawn(async () => { doWork(); });
}"#;
        let file = lower_source(source);
        let use_paths: Vec<&str> = file.uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            !use_paths.iter().any(|p| p.contains("concurrent")),
            "std/concurrent should not produce a use declaration, got: {use_paths:?}"
        );
    }

    // Test 7: needs_async_runtime is set for spawn usage
    #[test]
    fn test_lower_module_with_spawn_sets_needs_async_runtime() {
        let source = r#"function main() {
            spawn(async () => { doWork(); });
        }"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, needs_async_runtime, _, _, _, _, _) = transform.lower_module(&module);
        assert!(
            needs_async_runtime,
            "expected needs_async_runtime to be true when spawn is used"
        );
    }

    // Test 8: needs_async_runtime is set for Promise.all usage
    #[test]
    fn test_lower_module_with_promise_all_sets_needs_async_runtime() {
        let source = r#"async function main() {
            const [a, b] = await Promise.all([getA(), getB()]);
        }"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, needs_async_runtime, _, _, _, _, _) = transform.lower_module(&module);
        assert!(
            needs_async_runtime,
            "expected needs_async_runtime to be true when Promise.all is used"
        );
    }

    // ---------------------------------------------------------------
    // Promise.all + throws — auto-unwrap Results from tokio::join!
    // ---------------------------------------------------------------

    // Test: Promise.all with throwing functions — elements are not wrapped with `?`,
    // and throwing_elements flags are set correctly.
    #[test]
    fn test_lower_promise_all_throws_strips_question_mark_and_flags_throwing() {
        let source = r#"async function fetchData(url: string): string throws string {
            return "data";
        }

        async function fetchAll(): void throws string {
            const [a, b] = await Promise.all([fetchData("/users"), fetchData("/posts")]);
            console.log(a);
        }"#;
        let file = lower_source(source);
        // fetchAll is the second function (index 1)
        let RustItem::Function(f) = &file.items[1] else {
            panic!("expected Function");
        };
        let RustStmt::TupleDestructure(td) = &f.body.stmts[0] else {
            panic!("expected TupleDestructure, got {:?}", f.body.stmts[0]);
        };
        assert_eq!(td.bindings, vec!["a", "b"]);
        let RustExprKind::TokioJoin {
            elements,
            throwing_elements,
        } = &td.init.kind
        else {
            panic!("expected TokioJoin, got {:?}", td.init.kind);
        };
        assert_eq!(elements.len(), 2);
        // Elements inside tokio::join! should be bare calls, NOT wrapped with `?`
        for (i, elem) in elements.iter().enumerate() {
            assert!(
                matches!(&elem.kind, RustExprKind::Call { func, .. } if func == "fetchData"),
                "element {i} should be a bare Call, got {:?}",
                elem.kind
            );
        }
        // Both elements should be flagged as throwing
        assert_eq!(
            throwing_elements,
            &vec![true, true],
            "both elements should be marked as throwing"
        );
    }

    // Test: Promise.all with non-throwing functions — throwing_elements all false
    #[test]
    fn test_lower_promise_all_non_throws_no_throwing_flags() {
        let source = r#"async function getA(): string {
            return "a";
        }

        async function getB(): string {
            return "b";
        }

        async function main() {
            const [a, b] = await Promise.all([getA(), getB()]);
        }"#;
        let file = lower_source(source);
        // main is the third function (index 2)
        let RustItem::Function(f) = &file.items[2] else {
            panic!("expected Function");
        };
        let RustStmt::TupleDestructure(td) = &f.body.stmts[0] else {
            panic!("expected TupleDestructure");
        };
        let RustExprKind::TokioJoin {
            throwing_elements, ..
        } = &td.init.kind
        else {
            panic!("expected TokioJoin");
        };
        assert!(
            throwing_elements.iter().all(|t| !t),
            "non-throwing functions should have all false flags"
        );
    }

    // Test: Promise.all with mixed throwing/non-throwing — selective flags
    #[test]
    fn test_lower_promise_all_mixed_throws_selective_flags() {
        let source = r#"async function safeFn(): string {
            return "safe";
        }

        async function riskyFn(url: string): string throws string {
            return "risky";
        }

        async function doWork(): void throws string {
            const [a, b] = await Promise.all([safeFn(), riskyFn("/api")]);
        }"#;
        let file = lower_source(source);
        // doWork is the third function (index 2)
        let RustItem::Function(f) = &file.items[2] else {
            panic!("expected Function");
        };
        let RustStmt::TupleDestructure(td) = &f.body.stmts[0] else {
            panic!("expected TupleDestructure");
        };
        let RustExprKind::TokioJoin {
            elements,
            throwing_elements,
        } = &td.init.kind
        else {
            panic!("expected TokioJoin");
        };
        assert_eq!(elements.len(), 2);
        // First element (safeFn) is not throwing, second (riskyFn) is
        assert_eq!(
            throwing_elements,
            &vec![false, true],
            "only the second element should be marked as throwing"
        );
    }

    // ---------------------------------------------------------------------------
    // Task 055: Function Features — Optional, Default, Rest Parameters
    // ---------------------------------------------------------------------------

    // T055-L1: Optional param → Option<T> in signature
    #[test]
    fn test_lower_optional_param_produces_option_type() {
        let source = "function greet(name: string, title?: string): string { return name; }";
        let ir = lower_source(source);
        let RustItem::Function(f) = &ir.items[0] else {
            panic!("expected function");
        };
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].ty, RustType::String);
        assert_eq!(f.params[1].ty, RustType::Option(Box::new(RustType::String)));
    }

    // T055-L2: Missing optional arg → None appended
    #[test]
    fn test_lower_missing_optional_arg_produces_none() {
        let output = compile_and_emit(
            "function greet(name: string, title?: string): string { return name; }\n\
             function main() { greet(\"Alice\"); }",
        );
        assert!(
            output.contains("None"),
            "missing optional arg should produce None: {output}"
        );
    }

    // T055-L3: Default param → default value inlined at call site
    #[test]
    fn test_lower_default_param_inlined_at_call_site() {
        let output = compile_and_emit(
            "function connect(host: string, port: i64 = 8080): string { return host; }\n\
             function main() { connect(\"localhost\"); }",
        );
        assert!(
            output.contains("8080"),
            "missing default arg should inline 8080: {output}"
        );
    }

    // T055-L4: Default param retains base type (not Option)
    #[test]
    fn test_lower_default_param_uses_base_type() {
        let source = "function connect(host: string, port: i64 = 8080): string { return host; }";
        let ir = lower_source(source);
        let RustItem::Function(f) = &ir.items[0] else {
            panic!("expected function");
        };
        assert_eq!(f.params[1].ty, RustType::I64);
    }

    // T055-L5: Rest param → Vec<T> in signature
    #[test]
    fn test_lower_rest_param_produces_vec_type() {
        let source = "function log_all(...messages: Array<string>): void { }";
        let ir = lower_source(source);
        let RustItem::Function(f) = &ir.items[0] else {
            panic!("expected function");
        };
        assert_eq!(f.params.len(), 1);
        assert_eq!(
            f.params[0].ty,
            RustType::Generic(
                Box::new(RustType::Named("Vec".to_owned())),
                vec![RustType::String]
            )
        );
    }

    // T055-L6: Excess call args → vec![...] for rest param
    #[test]
    fn test_lower_excess_args_collected_into_vec() {
        let output = compile_and_emit(
            "function log_all(prefix: string, ...messages: Array<string>): void { }\n\
             function main() { log_all(\"INFO\", \"hello\", \"world\"); }",
        );
        assert!(
            output.contains("vec!["),
            "excess args should produce vec![]: {output}"
        );
    }

    // ---------------------------------------------------------------
    // Task 063: Logical assignment operators lowering
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_nullish_assign_generates_is_none_some() {
        let output = compile_and_emit(
            "function main() {\n\
               let x: i32 | null = null;\n\
               x ??= 5;\n\
             }",
        );
        assert!(
            output.contains("is_none()"),
            "??= should lower to is_none() check: {output}"
        );
        assert!(
            output.contains("Some(5)"),
            "??= should wrap value in Some(): {output}"
        );
    }

    #[test]
    fn test_lower_or_assign_generates_negation() {
        let output = compile_and_emit(
            "function main() {\n\
               let enabled: bool = false;\n\
               enabled ||= true;\n\
             }",
        );
        assert!(
            output.contains("!enabled"),
            "||= should lower to !target check: {output}"
        );
        assert!(
            output.contains("enabled = true"),
            "||= should assign the value: {output}"
        );
    }

    #[test]
    fn test_lower_and_assign_generates_truthy_check() {
        let output = compile_and_emit(
            "function main() {\n\
               let active: bool = true;\n\
               active &&= false;\n\
             }",
        );
        assert!(
            output.contains("if active"),
            "&&= should lower to truthy check: {output}"
        );
        assert!(
            output.contains("active = false"),
            "&&= should assign the value: {output}"
        );
    }

    #[test]
    fn test_lower_nullish_assign_makes_variable_mutable() {
        let output = compile_and_emit(
            "function main() {\n\
               let x: i32 | null = null;\n\
               x ??= 5;\n\
             }",
        );
        assert!(
            output.contains("let mut x"),
            "??= target should be declared mut: {output}"
        );
    }

    #[test]
    fn test_lower_or_assign_makes_variable_mutable() {
        let output = compile_and_emit(
            "function main() {\n\
               let enabled: bool = false;\n\
               enabled ||= true;\n\
             }",
        );
        assert!(
            output.contains("let mut enabled"),
            "||= target should be declared mut: {output}"
        );
    }

    #[test]
    fn test_lower_and_assign_makes_variable_mutable() {
        let output = compile_and_emit(
            "function main() {\n\
               let active: bool = true;\n\
               active &&= false;\n\
             }",
        );
        assert!(
            output.contains("let mut active"),
            "&&= target should be declared mut: {output}"
        );
    }

    // ---------------------------------------------------------------
    // Task 066: Async iteration and Promise methods
    // ---------------------------------------------------------------

    // T066-L1: for await lowers to WhileLet with .next().await
    #[test]
    fn test_lower_for_await_produces_while_let() {
        let output = compile_and_emit(
            "async function main() { for await (const msg of channel) { console.log(msg); } }",
        );
        assert!(
            output.contains("while let Some(msg) = channel.next().await"),
            "for await should produce while let: {output}"
        );
    }

    // T066-L2: Promise.race lowers to tokio::select!
    #[test]
    fn test_lower_promise_race_produces_tokio_select() {
        let output = compile_and_emit(
            "async function main() { const first = await Promise.race([a(), b()]); }",
        );
        assert!(
            output.contains("tokio::select!"),
            "Promise.race should produce tokio::select!: {output}"
        );
    }

    // T066-L3: Promise.any lowers to futures::future::select_ok
    #[test]
    fn test_lower_promise_any_produces_futures_select_ok() {
        let output = compile_and_emit(
            "async function main() { const first = await Promise.any([tryA(), tryB()]); }",
        );
        assert!(
            output.contains("futures::future::select_ok"),
            "Promise.any should produce futures::future::select_ok: {output}"
        );
    }

    // T066-L4: for await sets needs_futures_crate flag
    #[test]
    fn test_lower_for_await_sets_needs_futures_crate() {
        let source =
            "async function main() { for await (const msg of channel) { console.log(msg); } }";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, _, needs_futures, _, _, _, _) = transform.lower_module(&module);
        assert!(
            needs_futures,
            "for await should set needs_futures_crate flag"
        );
    }

    // T066-L5: Promise.any sets needs_futures_crate flag
    #[test]
    fn test_lower_promise_any_sets_needs_futures_crate() {
        let source = "async function main() { const first = await Promise.any([tryA(), tryB()]); }";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, _, needs_futures, _, _, _, _) = transform.lower_module(&module);
        assert!(
            needs_futures,
            "Promise.any should set needs_futures_crate flag"
        );
    }

    // T066-L6: Promise.race sets needs_async_runtime flag
    #[test]
    fn test_lower_promise_race_sets_needs_async_runtime() {
        let source = "async function main() { const x = await Promise.race([a(), b()]); }";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, needs_async, _, _, _, _, _) = transform.lower_module(&module);
        assert!(
            needs_async,
            "Promise.race should set needs_async_runtime flag"
        );
    }

    // T066-L7: for await generates use futures::StreamExt
    #[test]
    fn test_lower_for_await_generates_stream_ext_use() {
        let output = compile_and_emit(
            "async function main() { for await (const msg of channel) { console.log(msg); } }",
        );
        assert!(
            output.contains("use futures::StreamExt;"),
            "for await should generate StreamExt use declaration: {output}"
        );
    }

    // ---- Static method call on imported types ----

    #[test]
    fn test_lower_imported_type_static_method_call() {
        let output = compile_and_emit(
            r#"
            import { TcpListener } from "tokio/net";
            function main() {
                const listener = TcpListener.bind("0.0.0.0:3000");
            }
            "#,
        );
        assert!(
            output.contains("TcpListener::bind("),
            "imported type method call should use :: notation: {output}"
        );
        assert!(
            !output.contains("TcpListener.bind("),
            "imported type method call should not use . notation: {output}"
        );
    }

    #[test]
    fn test_lower_variable_method_call_not_affected() {
        let output = compile_and_emit(
            r#"
            function main() {
                const listener: string = "hello";
                const result = listener.len();
            }
            "#,
        );
        // Variable method calls should still use dot notation
        assert!(
            !output.contains("listener::len("),
            "variable method call should not use :: notation: {output}"
        );
    }

    #[test]
    fn test_lower_pascal_case_identifier_static_method_call() {
        // Even without an explicit import, PascalCase identifiers not in scope
        // as variables should be treated as type names (static call)
        let output = compile_and_emit(
            r#"
            import { MyService } from "my_crate";
            function main() {
                const svc = MyService.create();
            }
            "#,
        );
        assert!(
            output.contains("MyService::create("),
            "PascalCase identifier method call should use :: notation: {output}"
        );
    }

    // ---- String literal arg stripping for external calls ----

    // String literal arg to external function call → no .to_string()
    #[test]
    fn test_lower_external_fn_call_string_arg_no_to_string() {
        // unknown_fn has no FnSignature → callee_modes is None
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Call(CallExpr {
                    callee: ident("unknown_fn", 0, 10),
                    args: vec![string_expr("hello", 11, 18)],
                }),
                span: span(0, 19),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Call { args, .. } => {
                    assert!(
                        matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "hello"),
                        "external fn string arg should be bare StringLit, got {:?}",
                        args[0].kind
                    );
                }
                other => panic!("expected Call, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // String literal arg to external method call → no .to_string()
    #[test]
    fn test_lower_external_method_call_string_arg_no_to_string() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("router", 0, 6)),
                    method: ident("route", 7, 12),
                    args: vec![string_expr("/", 13, 16)],
                }),
                span: span(0, 17),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::MethodCall { args, .. } => {
                    assert!(
                        matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "/"),
                        "external method string arg should be bare StringLit, got {:?}",
                        args[0].kind
                    );
                }
                other => panic!("expected MethodCall, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // String literal arg to internal function with read-only string param → no .to_string()
    // (Tier 2 changes read-only String param to &str, then strips .to_string() at call site)
    #[test]
    fn test_lower_internal_fn_readonly_string_param_strips_to_string() {
        let output = compile_and_emit(
            "function greet(name: string): void { console.log(name); }\n\
             function main() { greet(\"Alice\"); }",
        );
        // Tier 2 converts read-only String → &str, so call site should not have .to_string()
        assert!(
            !output.contains("\"Alice\".to_string()"),
            "read-only string param should strip .to_string() at call site: {output}"
        );
    }

    // String literal arg to internal function with mutated string param → .to_string() stays
    #[test]
    fn test_lower_internal_fn_mutated_string_param_keeps_to_string() {
        let output = compile_and_emit(
            "function consume(name: string): string { return name; }\n\
             function main() { consume(\"Alice\"); }",
        );
        // consume moves name (returns it), so Tier 2 keeps param as String.
        // Call site should retain .to_string() for owned param.
        assert!(
            output.contains("to_string"),
            "mutated/moved string param should keep .to_string(): {output}"
        );
    }

    // String literal in variable binding → .to_string() stays
    #[test]
    fn test_lower_string_literal_binding_keeps_to_string() {
        let output = compile_and_emit("function main() { const name = \"Alice\"; }");
        assert!(
            output.contains("to_string"),
            "string literal in let binding should keep .to_string(): {output}"
        );
    }

    // External free function with &str param → string literal arg uses BorrowedStr mode
    #[test]
    fn test_lower_external_fn_str_ref_param_strips_to_string() {
        use rsc_syntax::external_fn::{ExternalFnInfo, ExternalParamInfo, ExternalReturnType};

        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Call(CallExpr {
                    callee: ident("greet", 0, 5),
                    args: vec![string_expr("hello", 6, 13)],
                }),
                span: span(0, 14),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);

        let mut ext_sigs = HashMap::new();
        ext_sigs.insert(
            "some_crate::greet".to_owned(),
            ExternalFnInfo {
                name: "greet".to_owned(),
                crate_name: "some_crate".to_owned(),
                params: vec![ExternalParamInfo {
                    name: "msg".to_owned(),
                    is_ref: true,
                    is_str_ref: true,
                    is_mut_ref: false,
                }],
                return_type: ExternalReturnType::Unit,
                is_async: false,
                is_method: false,
                parent_type: None,
            },
        );
        transform.set_external_signatures(ext_sigs);

        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Call { args, .. } => {
                    // With BorrowedStr mode, the string literal should be a bare StringLit
                    // (the .to_string() wrapper is stripped)
                    assert!(
                        matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "hello"),
                        "external fn with &str param should strip .to_string(), got {:?}",
                        args[0].kind
                    );
                }
                other => panic!("expected Call, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // External throws function → `?` added at call site in throws context
    #[test]
    fn test_lower_external_fn_throws_adds_question_mark() {
        use rsc_syntax::external_fn::{ExternalFnInfo, ExternalReturnType};

        // Create a throws wrapper function that calls an external throws function
        let inner_call = Expr {
            kind: ExprKind::Call(CallExpr {
                callee: ident("connect", 0, 7),
                args: vec![],
            }),
            span: span(0, 9),
        };
        let throws_fn = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("main_throws", 0, 11),
            type_params: None,
            params: vec![],
            return_type: Some(ReturnTypeAnnotation {
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("void", 0, 4)),
                    span: span(0, 4),
                }),
                throws: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }),
                span: span(0, 4),
            }),
            body: Block {
                stmts: vec![Stmt::Expr(inner_call)],
                span: span(0, 100),
            },
            doc_comment: None,
            span: span(0, 100),
        };
        let module = make_module(vec![fn_item(throws_fn)]);
        let mut transform = Transform::new(false);

        let mut ext_sigs = HashMap::new();
        ext_sigs.insert(
            "db::connect".to_owned(),
            ExternalFnInfo {
                name: "connect".to_owned(),
                crate_name: "db".to_owned(),
                params: vec![],
                return_type: ExternalReturnType::Result,
                is_async: false,
                is_method: false,
                parent_type: None,
            },
        );
        transform.set_external_signatures(ext_sigs);

        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => {
                // The call should be wrapped in QuestionMark
                assert!(
                    matches!(&expr.kind, RustExprKind::QuestionMark(inner)
                        if matches!(&inner.kind, RustExprKind::Call { func, .. } if func == "connect")
                    ),
                    "external throws fn should add `?` at call site, got {:?}",
                    expr.kind
                );
            }
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // External static method with &str param → correct borrow at call site
    #[test]
    fn test_lower_external_static_method_str_ref_param() {
        use rsc_syntax::external_fn::{ExternalFnInfo, ExternalParamInfo, ExternalReturnType};

        // TcpListener.bind("addr") → TcpListener::bind("addr") with &str param mode
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("TcpListener", 0, 11)),
                    method: ident("bind", 12, 16),
                    args: vec![string_expr("0.0.0.0:3000", 17, 31)],
                }),
                span: span(0, 32),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);

        // Mark TcpListener as an imported type so it's recognized as a type name
        transform.imported_types.insert("TcpListener".to_owned());

        let mut ext_sigs = HashMap::new();
        ext_sigs.insert(
            "tokio::net::TcpListener::bind".to_owned(),
            ExternalFnInfo {
                name: "bind".to_owned(),
                crate_name: "tokio".to_owned(),
                params: vec![ExternalParamInfo {
                    name: "addr".to_owned(),
                    is_ref: true,
                    is_str_ref: true,
                    is_mut_ref: false,
                }],
                return_type: ExternalReturnType::Result,
                is_async: false,
                is_method: false,
                parent_type: Some("TcpListener".to_owned()),
            },
        );
        transform.set_external_signatures(ext_sigs);

        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::StaticCall { args, .. } => {
                    // With BorrowedStr mode from external sig, the string literal
                    // should be a bare StringLit (not wrapped in .to_string())
                    assert!(
                        matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "0.0.0.0:3000"),
                        "external static method with &str param should produce bare StringLit, got {:?}",
                        args[0].kind
                    );
                }
                other => panic!("expected StaticCall, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // External static method that throws + in throws context → `?` added
    #[test]
    fn test_lower_external_static_method_throws_adds_question_mark() {
        use rsc_syntax::external_fn::{ExternalFnInfo, ExternalReturnType};

        let inner_call = Expr {
            kind: ExprKind::MethodCall(MethodCallExpr {
                object: Box::new(ident_expr("TcpListener", 0, 11)),
                method: ident("bind", 12, 16),
                args: vec![string_expr("0.0.0.0:3000", 17, 31)],
            }),
            span: span(0, 32),
        };
        let throws_fn = FnDecl {
            is_async: false,
            is_generator: false,
            name: ident("start_server", 0, 12),
            type_params: None,
            params: vec![],
            return_type: Some(ReturnTypeAnnotation {
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("void", 0, 4)),
                    span: span(0, 4),
                }),
                throws: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }),
                span: span(0, 4),
            }),
            body: Block {
                stmts: vec![Stmt::Expr(inner_call)],
                span: span(0, 100),
            },
            doc_comment: None,
            span: span(0, 100),
        };
        let module = make_module(vec![fn_item(throws_fn)]);
        let mut transform = Transform::new(false);

        transform.imported_types.insert("TcpListener".to_owned());

        let mut ext_sigs = HashMap::new();
        ext_sigs.insert(
            "tokio::net::TcpListener::bind".to_owned(),
            ExternalFnInfo {
                name: "bind".to_owned(),
                crate_name: "tokio".to_owned(),
                params: vec![],
                return_type: ExternalReturnType::Result,
                is_async: false,
                is_method: false,
                parent_type: Some("TcpListener".to_owned()),
            },
        );
        transform.set_external_signatures(ext_sigs);

        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => {
                assert!(
                    matches!(&expr.kind, RustExprKind::QuestionMark(inner)
                        if matches!(&inner.kind, RustExprKind::StaticCall { type_name, method, .. }
                            if type_name == "TcpListener" && method == "bind")
                    ),
                    "external static throws method should add `?`, got {:?}",
                    expr.kind
                );
            }
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Internal function signatures still work correctly (no regression)
    #[test]
    fn test_lower_internal_fn_signature_not_overwritten_by_external() {
        use rsc_syntax::external_fn::{ExternalFnInfo, ExternalReturnType};

        // Define an internal function `greet` with a string param, then call it.
        // Also register an external signature for "greet" — the internal one should win.
        let greet_fn = make_fn("greet", vec![make_param("name", "string")], None, vec![]);
        let main_fn = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Call(CallExpr {
                    callee: ident("greet", 0, 5),
                    args: vec![string_expr("Alice", 6, 13)],
                }),
                span: span(0, 14),
            })],
        );
        let module = make_module(vec![fn_item(greet_fn), fn_item(main_fn)]);
        let mut transform = Transform::new(false);

        // Register external signature that would make greet "throws" — but
        // the internal one should take priority.
        let mut ext_sigs = HashMap::new();
        ext_sigs.insert(
            "other_crate::greet".to_owned(),
            ExternalFnInfo {
                name: "greet".to_owned(),
                crate_name: "other_crate".to_owned(),
                params: vec![],
                return_type: ExternalReturnType::Result,
                is_async: false,
                is_method: false,
                parent_type: None,
            },
        );
        transform.set_external_signatures(ext_sigs);

        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);
        // Find the main function (second item)
        let RustItem::Function(main_func) = &file.items[1] else {
            panic!("expected function item");
        };
        assert_eq!(main_func.name, "main");
        match &main_func.body.stmts[0] {
            RustStmt::Semi(expr) => {
                // Should NOT be wrapped in QuestionMark since internal greet doesn't throw
                assert!(
                    matches!(&expr.kind, RustExprKind::Call { func, .. } if func == "greet"),
                    "internal fn sig should not be overwritten by external, got {:?}",
                    expr.kind
                );
            }
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // ---- derives keyword: merge with auto-inferred derives ----

    #[test]
    fn test_lower_type_def_derives_merge_with_auto_inferred() {
        let source = "type Foo = { x: i32 } derives Serialize, Deserialize";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[0] else {
            panic!("expected Struct item");
        };
        assert_eq!(s.name, "Foo");
        // Should have Debug, Clone, PartialEq, Eq (auto) + Serialize, Deserialize (explicit)
        assert!(s.derives.contains(&"Debug".to_owned()));
        assert!(s.derives.contains(&"Clone".to_owned()));
        assert!(s.derives.contains(&"Serialize".to_owned()));
        assert!(s.derives.contains(&"Deserialize".to_owned()));
    }

    #[test]
    fn test_lower_type_def_derives_no_duplicates() {
        let source = "type Foo = { x: i32 } derives Debug, Clone";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[0] else {
            panic!("expected Struct item");
        };
        // Debug and Clone are auto-inferred AND explicit — should only appear once
        let debug_count = s.derives.iter().filter(|d| *d == "Debug").count();
        let clone_count = s.derives.iter().filter(|d| *d == "Clone").count();
        assert_eq!(debug_count, 1, "Debug should appear only once");
        assert_eq!(clone_count, 1, "Clone should appear only once");
    }

    #[test]
    fn test_lower_simple_enum_derives_merge() {
        let source = r#"type Dir = "north" | "south" derives Serialize"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Enum(e) = &file.items[0] else {
            panic!("expected Enum item");
        };
        assert_eq!(e.name, "Dir");
        // Simple enums get Debug, Clone, Copy, PartialEq, Eq, Hash auto + Serialize explicit
        assert!(e.derives.contains(&"Debug".to_owned()));
        assert!(e.derives.contains(&"Serialize".to_owned()));
    }

    #[test]
    fn test_lower_type_def_derives_empty_is_backward_compatible() {
        let source = "type Foo = { x: i32 }";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[0] else {
            panic!("expected Struct item");
        };
        // Should have only auto-inferred derives, no explicit
        assert!(s.derives.contains(&"Debug".to_owned()));
        assert!(s.derives.contains(&"Clone".to_owned()));
        assert!(!s.derives.contains(&"Serialize".to_owned()));
    }

    #[test]
    fn test_lower_needs_serde_flag_set_when_serialize_in_derives() {
        let source = "type Foo = { x: i32 } derives Serialize";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, _, _, _, _, needs_serde, _) = transform.lower_module(&module);
        assert!(
            needs_serde,
            "needs_serde should be true when Serialize is in derives"
        );
    }

    #[test]
    fn test_lower_needs_serde_flag_set_when_deserialize_in_derives() {
        let source = "type Foo = { x: i32 } derives Deserialize";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, _, _, _, _, needs_serde, _) = transform.lower_module(&module);
        assert!(
            needs_serde,
            "needs_serde should be true when Deserialize is in derives"
        );
    }

    #[test]
    fn test_lower_needs_serde_flag_not_set_without_serde_derives() {
        let source = "type Foo = { x: i32 } derives Hash";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, _, _, _, _, needs_serde, _) = transform.lower_module(&module);
        assert!(
            !needs_serde,
            "needs_serde should be false without Serialize/Deserialize"
        );
    }

    // ---------------------------------------------------------------
    // Index signatures
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_pure_index_signature_produces_type_alias() {
        let source = r#"type Config = { [key: string]: string }"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::TypeAlias(ta) = &file.items[0] else {
            panic!("expected TypeAlias item, got {:?}", file.items[0]);
        };
        assert_eq!(ta.name, "Config");
        assert_eq!(ta.ty.to_string(), "HashMap<String, String>");
    }

    #[test]
    fn test_lower_index_signature_numeric_keys() {
        let source = r#"type Scores = { [id: i32]: string }"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::TypeAlias(ta) = &file.items[0] else {
            panic!("expected TypeAlias item, got {:?}", file.items[0]);
        };
        assert_eq!(ta.name, "Scores");
        assert_eq!(ta.ty.to_string(), "HashMap<i32, String>");
    }

    #[test]
    fn test_lower_hashmap_init_from_empty_object() {
        let source = r#"
            function main() {
                const config: { [key: string]: string } = {};
            }
        "#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        // The function body should contain a let statement with HashMap::new()
        let RustItem::Function(f) = &file.items[0] else {
            panic!("expected Function item");
        };
        let RustStmt::Let(let_stmt) = &f.body.stmts[0] else {
            panic!("expected Let statement");
        };
        assert_eq!(let_stmt.name, "config");
        assert!(
            matches!(
                &let_stmt.init.kind,
                RustExprKind::StaticCall {
                    type_name,
                    method,
                    ..
                } if type_name == "HashMap" && method == "new"
            ),
            "expected HashMap::new(), got {:?}",
            let_stmt.init.kind
        );
    }

    #[test]
    fn test_lower_hashmap_insert_from_index_assign() {
        let source = r#"
            function main() {
                let config: { [key: string]: string } = {};
                config["debug"] = "true";
            }
        "#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(f) = &file.items[0] else {
            panic!("expected Function item");
        };
        // Second statement should be the insert call
        let RustStmt::Semi(insert_expr) = &f.body.stmts[1] else {
            panic!("expected Semi statement, got {:?}", &f.body.stmts[1]);
        };
        assert!(
            matches!(
                &insert_expr.kind,
                RustExprKind::MethodCall { method, .. } if method == "insert"
            ),
            "expected .insert() call, got {:?}",
            insert_expr.kind
        );
    }

    #[test]
    fn test_lower_hashmap_index_read() {
        let source = r#"
            function main() {
                const config: { [key: string]: string } = {};
                const val = config["debug"];
            }
        "#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(f) = &file.items[0] else {
            panic!("expected Function item");
        };
        // Second statement should have an Index expression (HashMap supports Index trait)
        let RustStmt::Let(let_stmt) = &f.body.stmts[1] else {
            panic!("expected Let statement");
        };
        assert_eq!(let_stmt.name, "val");
        assert!(
            matches!(&let_stmt.init.kind, RustExprKind::Index { .. }),
            "expected Index expression, got {:?}",
            let_stmt.init.kind
        );
    }

    // ---------------------------------------------------------------
    // Utility types: unit tests
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_utility_partial_produces_struct_with_option_fields() {
        let source = r#"type User = { name: string, age: u32 }
type PartialUser = Partial<User>"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.items.len(), 2);
        let RustItem::Struct(s) = &file.items[1] else {
            panic!("expected Struct item, got {:?}", file.items[1]);
        };
        assert_eq!(s.name, "PartialUser");
        assert_eq!(s.fields.len(), 2);
        assert_eq!(s.fields[0].name, "name");
        assert!(
            matches!(&s.fields[0].ty, RustType::Option(_)),
            "expected Option type for name, got {:?}",
            s.fields[0].ty
        );
        assert_eq!(s.fields[1].name, "age");
        assert!(
            matches!(&s.fields[1].ty, RustType::Option(_)),
            "expected Option type for age, got {:?}",
            s.fields[1].ty
        );
    }

    #[test]
    fn test_lower_utility_record_produces_type_alias() {
        let source = r#"type Scores = Record<string, i32>"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.items.len(), 1);
        let RustItem::TypeAlias(ta) = &file.items[0] else {
            panic!("expected TypeAlias item, got {:?}", file.items[0]);
        };
        assert_eq!(ta.name, "Scores");
        assert_eq!(ta.ty.to_string(), "HashMap<String, i32>");
    }

    #[test]
    fn test_lower_utility_pick_selects_named_fields() {
        let source = r#"type User = { name: string, age: u32, email: string }
type NameOnly = Pick<User, "name">"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[1] else {
            panic!("expected Struct item");
        };
        assert_eq!(s.name, "NameOnly");
        assert_eq!(s.fields.len(), 1);
        assert_eq!(s.fields[0].name, "name");
    }

    #[test]
    fn test_lower_utility_omit_removes_named_fields() {
        let source = r#"type User = { name: string, age: u32, email: string }
type NoEmail = Omit<User, "email">"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[1] else {
            panic!("expected Struct item");
        };
        assert_eq!(s.name, "NoEmail");
        assert_eq!(s.fields.len(), 2);
        assert_eq!(s.fields[0].name, "name");
        assert_eq!(s.fields[1].name, "age");
    }

    #[test]
    fn test_lower_utility_readonly_is_identity() {
        let source = r#"type Point = { x: f64, y: f64 }
type ReadonlyPoint = Readonly<Point>"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[1] else {
            panic!("expected Struct item");
        };
        assert_eq!(s.name, "ReadonlyPoint");
        assert_eq!(s.fields.len(), 2);
        assert_eq!(s.fields[0].name, "x");
        assert_eq!(s.fields[1].name, "y");
    }

    #[test]
    fn test_lower_utility_required_unwraps_option() {
        let source = r#"type Config = { name: string | null, debug: bool | null }
type FullConfig = Required<Config>"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Struct(s) = &file.items[1] else {
            panic!("expected Struct item");
        };
        assert_eq!(s.name, "FullConfig");
        assert_eq!(s.fields.len(), 2);
        // name should be String (not Option<String>)
        assert_eq!(s.fields[0].ty, RustType::String);
        // debug should be bool (not Option<bool>)
        assert_eq!(s.fields[1].ty, RustType::Bool);
    }

    #[test]
    fn test_lower_utility_partial_unknown_type_emits_diagnostic() {
        let source = r#"type Foo = Partial<NonExistent>"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(
            diags.iter().any(|d| d.message.contains("unknown type")),
            "expected diagnostic about unknown type, got: {diags:?}"
        );
    }

    #[test]
    fn test_lower_utility_pick_unknown_field_emits_diagnostic() {
        let source = r#"type User = { name: string, age: u32 }
type Bad = Pick<User, "nonexistent">"#;
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(
            diags.iter().any(|d| d.message.contains("unknown field")),
            "expected diagnostic about unknown field, got: {diags:?}"
        );
    }

    #[test]
    fn test_identify_utility_type_recognizes_all_six() {
        let make_generic = |name: &str| ast::TypeAnnotation {
            kind: TypeKind::Generic(ident(name, 0, name.len() as u32), vec![]),
            span: span(0, 10),
        };
        assert_eq!(
            Transform::identify_utility_type(&make_generic("Partial")),
            Some("Partial")
        );
        assert_eq!(
            Transform::identify_utility_type(&make_generic("Required")),
            Some("Required")
        );
        assert_eq!(
            Transform::identify_utility_type(&make_generic("Readonly")),
            Some("Readonly")
        );
        assert_eq!(
            Transform::identify_utility_type(&make_generic("Record")),
            Some("Record")
        );
        assert_eq!(
            Transform::identify_utility_type(&make_generic("Pick")),
            Some("Pick")
        );
        assert_eq!(
            Transform::identify_utility_type(&make_generic("Omit")),
            Some("Omit")
        );
        assert_eq!(
            Transform::identify_utility_type(&make_generic("NotAUtilityType")),
            None
        );
    }

    #[test]
    fn test_extract_string_literal_fields_single() {
        let ann = ast::TypeAnnotation {
            kind: TypeKind::StringLiteral("name".to_owned()),
            span: span(0, 6),
        };
        assert_eq!(Transform::extract_string_literal_fields(&ann), vec!["name"]);
    }

    #[test]
    fn test_extract_string_literal_fields_union() {
        let ann = ast::TypeAnnotation {
            kind: TypeKind::Union(vec![
                ast::TypeAnnotation {
                    kind: TypeKind::StringLiteral("name".to_owned()),
                    span: span(0, 6),
                },
                ast::TypeAnnotation {
                    kind: TypeKind::StringLiteral("age".to_owned()),
                    span: span(9, 14),
                },
            ]),
            span: span(0, 14),
        };
        assert_eq!(
            Transform::extract_string_literal_fields(&ann),
            vec!["name", "age"]
        );
    }

    // ---- keyof type operator ----

    #[test]
    fn test_lower_keyof_produces_simple_enum() {
        let output = compile_and_emit(
            "type User = { name: string, age: u32, email: string }\ntype UserKey = keyof User",
        );
        assert!(
            output.contains("enum UserKey"),
            "expected enum UserKey in output: {output}"
        );
        assert!(
            output.contains("Name"),
            "expected Name variant in output: {output}"
        );
        assert!(
            output.contains("Age"),
            "expected Age variant in output: {output}"
        );
        assert!(
            output.contains("Email"),
            "expected Email variant in output: {output}"
        );
    }

    #[test]
    fn test_lower_keyof_enum_has_derives() {
        let output =
            compile_and_emit("type Point = { x: f64, y: f64 }\ntype PointKey = keyof Point");
        assert!(
            output.contains("enum PointKey"),
            "expected enum PointKey in output: {output}"
        );
        // Simple enums get at least Debug + Clone derives
        assert!(
            output.contains("derive"),
            "expected derive attribute in output: {output}"
        );
    }

    #[test]
    fn test_lower_keyof_with_two_fields() {
        let output = compile_and_emit(
            "type Config = { debug: bool, verbose: bool }\ntype ConfigKey = keyof Config",
        );
        assert!(
            output.contains("enum ConfigKey"),
            "expected enum ConfigKey in output: {output}"
        );
        assert!(
            output.contains("Debug"),
            "expected Debug variant in output: {output}"
        );
        assert!(
            output.contains("Verbose"),
            "expected Verbose variant in output: {output}"
        );
    }

    #[test]
    fn test_lower_typeof_resolves_variable_type() {
        // typeof works for top-level const declarations with explicit type annotations
        let output = compile_and_emit("const x: i32 = 42;\ntype XType = typeof x");
        assert!(
            output.contains("type XType = i32"),
            "expected type alias to i32 in output: {output}"
        );
    }

    #[test]
    fn test_lower_keyof_produces_correct_ir_structure() {
        let ir = lower_source("type User = { name: string, age: u32 }\ntype UserKey = keyof User");
        // Should have a struct for User and an enum for UserKey
        let has_user_struct = ir
            .items
            .iter()
            .any(|item| matches!(item, RustItem::Struct(s) if s.name == "User"));
        let has_key_enum = ir.items.iter().any(|item| {
            if let RustItem::Enum(e) = item {
                e.name == "UserKey"
                    && e.variants.len() == 2
                    && e.variants[0].name == "Name"
                    && e.variants[1].name == "Age"
            } else {
                false
            }
        });
        assert!(has_user_struct, "expected User struct in IR");
        assert!(
            has_key_enum,
            "expected UserKey enum with Name, Age variants in IR"
        );
    }

    #[test]
    fn test_lower_typeof_produces_type_alias_ir() {
        let ir = lower_source("const x: i32 = 42;\ntype XType = typeof x");
        let has_alias = ir.items.iter().any(|item| {
            if let RustItem::TypeAlias(a) = item {
                a.name == "XType" && a.ty == RustType::I32
            } else {
                false
            }
        });
        assert!(has_alias, "expected XType type alias to i32 in IR");
    }

    // -----------------------------------------------------------------------
    // Task 126: import type enforcement
    // -----------------------------------------------------------------------

    /// Parse and lower a RustScript source string, returning the Rust IR
    /// and diagnostics (without asserting diagnostics are empty).
    fn lower_source_with_diagnostics(
        source: &str,
    ) -> (RustFile, Vec<rsc_syntax::diagnostic::Diagnostic>) {
        let file_id = rsc_syntax::source::FileId(0);
        let (module, parse_diags) = rsc_parser::parse(source, file_id);
        assert!(
            parse_diags.is_empty(),
            "unexpected parse diagnostics: {parse_diags:?}"
        );
        let lower_result = crate::lower(&module);
        (lower_result.ir, lower_result.diagnostics)
    }

    #[test]
    fn test_import_type_no_use_declaration() {
        let source = r#"import type { User } from "./models";
function main() {}"#;
        let ir = lower_source(source);
        // Type-only imports should NOT generate any use declarations
        assert!(
            ir.uses.is_empty() || ir.uses.iter().all(|u| !u.path.contains("User")),
            "type-only import should not generate a use declaration for User, got: {:?}",
            ir.uses
        );
    }

    #[test]
    fn test_regular_import_emits_use_declaration() {
        let source = r#"import { User } from "./models";
function main() {}"#;
        let ir = lower_source(source);
        // Regular import should generate a use declaration
        let has_user_use = ir.uses.iter().any(|u| u.path.contains("User"));
        assert!(
            has_user_use,
            "regular import should generate a use declaration for User, got: {:?}",
            ir.uses
        );
    }

    #[test]
    fn test_import_type_vs_regular_import_mixed() {
        let source = r#"import type { Config } from "./config";
import { process_data } from "./handlers";
function main() {
  process_data();
}"#;
        let ir = lower_source(source);
        // Should have use for process_data but NOT for Config
        let has_config_use = ir.uses.iter().any(|u| u.path.contains("Config"));
        let has_process_use = ir.uses.iter().any(|u| u.path.contains("process_data"));
        assert!(
            !has_config_use,
            "type-only import Config should not generate a use declaration"
        );
        assert!(
            has_process_use,
            "regular import process_data should generate a use declaration"
        );
    }

    #[test]
    fn test_import_type_used_as_value_emits_diagnostic() {
        let source = r#"import type { User } from "./models";
function main() {
  User.create();
}"#;
        let (_, diags) = lower_source_with_diagnostics(source);
        let has_error = diags.iter().any(|d| {
            d.message
                .contains("cannot use type-only import `User` as a value")
        });
        assert!(
            has_error,
            "expected diagnostic about using type-only import as value, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_import_type_used_in_new_emits_diagnostic() {
        let source = r#"import type { User } from "./models";
function main() {
  const u = new User();
}"#;
        let (_, diags) = lower_source_with_diagnostics(source);
        let has_error = diags.iter().any(|d| {
            d.message
                .contains("cannot use type-only import `User` as a value")
        });
        assert!(
            has_error,
            "expected diagnostic about using type-only import in new expression, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_import_type_used_as_type_annotation_ok() {
        // Use `import type` with the imported name in a type position only.
        // The type resolver emits "unknown type" for imported types that aren't
        // locally defined, which is expected for cross-module usage. The key
        // invariant is that no "cannot use type-only import as a value" diagnostic
        // is emitted.
        let source = r#"import type { User } from "./models";
function greet(user: User): string {
  return "hello";
}"#;
        let (_, diags) = lower_source_with_diagnostics(source);
        let has_type_only_error = diags
            .iter()
            .any(|d| d.message.contains("cannot use type-only import"));
        assert!(
            !has_type_only_error,
            "type-only import used as type annotation should not produce type-only import error, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_import_type_multiple_names() {
        let source = r#"import type { User, Post } from "./models";
function main() {}"#;
        let ir = lower_source(source);
        // Neither User nor Post should generate use declarations
        let has_user_use = ir.uses.iter().any(|u| u.path.contains("User"));
        let has_post_use = ir.uses.iter().any(|u| u.path.contains("Post"));
        assert!(
            !has_user_use,
            "type-only import User should not generate a use declaration"
        );
        assert!(
            !has_post_use,
            "type-only import Post should not generate a use declaration"
        );
    }

    // ---------------------------------------------------------------
    // Task 135: Promise utility methods
    // ---------------------------------------------------------------

    // T135-L1: await Promise.resolve(x) → just x
    #[test]
    fn test_lower_promise_resolve_awaited() {
        let output =
            compile_and_emit("async function main() { const val = await Promise.resolve(42); }");
        assert!(
            output.contains("let val = 42"),
            "await Promise.resolve(42) should produce just the value: {output}"
        );
        assert!(
            !output.contains("async {"),
            "await Promise.resolve should not produce async block: {output}"
        );
    }

    // T135-L2: await Promise.reject(msg) → panic!
    #[test]
    fn test_lower_promise_reject_awaited() {
        let output = compile_and_emit(r#"async function main() { await Promise.reject("oops"); }"#);
        assert!(
            output.contains("panic!"),
            "await Promise.reject should produce panic!: {output}"
        );
        assert!(
            output.contains("rejected"),
            "await Promise.reject should include 'rejected' in message: {output}"
        );
    }

    // T135-L3: bare Promise.resolve(x) → async { x }
    #[test]
    fn test_lower_promise_resolve_bare() {
        let output = compile_and_emit("async function main() { const p = Promise.resolve(42); }");
        assert!(
            output.contains("async { 42 }"),
            "bare Promise.resolve(42) should produce async {{ 42 }}: {output}"
        );
    }

    // T135-L4: bare Promise.reject(msg) → async { panic!(...) }
    #[test]
    fn test_lower_promise_reject_bare() {
        let output =
            compile_and_emit(r#"async function main() { const p = Promise.reject("oops"); }"#);
        assert!(
            output.contains("async { panic!"),
            "bare Promise.reject should produce async {{ panic!(...) }}: {output}"
        );
    }

    // T135-L5: await Promise.allSettled([...]) → tokio::join!(...)
    #[test]
    fn test_lower_promise_all_settled() {
        let output = compile_and_emit(
            "async function main() { const results = await Promise.allSettled([a(), b()]); }",
        );
        assert!(
            output.contains("tokio::join!("),
            "Promise.allSettled should produce tokio::join!: {output}"
        );
    }

    // T135-L6: Promise.allSettled sets needs_async_runtime flag
    #[test]
    fn test_lower_promise_all_settled_sets_needs_async_runtime() {
        let source =
            "async function main() { const results = await Promise.allSettled([a(), b()]); }";
        let module = parse_module(source);
        let mut transform = Transform::new(false);
        let (_, _, _, needs_async, _, _, _, _, _) = transform.lower_module(&module);
        assert!(
            needs_async,
            "Promise.allSettled should set needs_async_runtime flag"
        );
    }

    // T135-L7: Promise.resolve with string argument
    #[test]
    fn test_lower_promise_resolve_string() {
        let output = compile_and_emit(
            r#"async function main() { const val = await Promise.resolve("hello"); }"#,
        );
        assert!(
            output.contains("let val = \"hello\""),
            "await Promise.resolve(\"hello\") should produce the string value: {output}"
        );
    }

    // ---------------------------------------------------------------
    // Task 130: RegExp class support
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_regexp_constructor() {
        // new RegExp("\\d+") → Regex::new("\\d+").unwrap()
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("re", 0, 2),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("RegExp", 0, 6)),
                    span: span(0, 6),
                }),
                init: Expr {
                    kind: ExprKind::New(ast::NewExpr {
                        type_name: ident("RegExp", 0, 6),
                        type_args: vec![],
                        args: vec![Expr {
                            kind: ExprKind::StringLit("\\d+".to_owned()),
                            span: span(0, 5),
                        }],
                    }),
                    span: span(0, 20),
                },
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        // Should be: Regex::new("\\d+").unwrap()
        match &let_stmt.init.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "unwrap");
                match &receiver.kind {
                    RustExprKind::StaticCall {
                        type_name,
                        method,
                        args,
                    } => {
                        assert_eq!(type_name, "Regex");
                        assert_eq!(method, "new");
                        assert_eq!(args.len(), 1);
                        assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "\\d+"));
                    }
                    other => panic!("expected StaticCall(Regex::new), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_regexp_constructor_with_flags() {
        // new RegExp("\\d+", "gi") → Regex::new("(?i)\\d+").unwrap()
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("re", 0, 2),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("RegExp", 0, 6)),
                    span: span(0, 6),
                }),
                init: Expr {
                    kind: ExprKind::New(ast::NewExpr {
                        type_name: ident("RegExp", 0, 6),
                        type_args: vec![],
                        args: vec![
                            Expr {
                                kind: ExprKind::StringLit("\\d+".to_owned()),
                                span: span(0, 5),
                            },
                            Expr {
                                kind: ExprKind::StringLit("gi".to_owned()),
                                span: span(6, 10),
                            },
                        ],
                    }),
                    span: span(0, 20),
                },
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        // Should be: Regex::new("(?i)\\d+").unwrap()
        match &let_stmt.init.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "unwrap");
                match &receiver.kind {
                    RustExprKind::StaticCall { args, .. } => {
                        assert_eq!(args.len(), 1);
                        // Pattern should have (?i) prepended (g flag is ignored)
                        assert!(
                            matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "(?i)\\d+"),
                            "expected pattern with (?i) prefix, got {:?}",
                            args[0].kind
                        );
                    }
                    other => panic!("expected StaticCall(Regex::new), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_regexp_needs_regex_flag() {
        // new RegExp("test") should set needs_regex flag
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("re", 0, 2),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::New(ast::NewExpr {
                        type_name: ident("RegExp", 0, 6),
                        type_args: vec![],
                        args: vec![Expr {
                            kind: ExprKind::StringLit("test".to_owned()),
                            span: span(0, 6),
                        }],
                    }),
                    span: span(0, 20),
                },
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (_, _, _, _, _, _, _, _, needs_regex) = transform.lower_module(&module);
        assert!(needs_regex, "new RegExp() should set needs_regex flag");
    }

    #[test]
    fn test_lower_regexp_use_declaration() {
        // new RegExp("test") should produce `use regex::Regex;`
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("re", 0, 2),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::New(ast::NewExpr {
                        type_name: ident("RegExp", 0, 6),
                        type_args: vec![],
                        args: vec![Expr {
                            kind: ExprKind::StringLit("test".to_owned()),
                            span: span(0, 6),
                        }],
                    }),
                    span: span(0, 20),
                },
                span: span(0, 30),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);
        let has_regex_use = file.uses.iter().any(|u| u.path == "regex::Regex");
        assert!(
            has_regex_use,
            "expected `use regex::Regex;`, got uses: {:?}",
            file.uses.iter().map(|u| &u.path).collect::<Vec<_>>()
        );
    }

    // Test: var x = 1 lowers to let mut x = 1 (always mutable)
    #[test]
    fn test_var_lowers_to_let_mut() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Var,
                name: ident("x", 4, 5),
                type_ann: None,
                init: int_expr(1, 8, 9),
                span: span(0, 10),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 1);
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(
                    let_stmt.mutable,
                    "var should always lower to let mut, even without reassignment"
                );
                assert_eq!(let_stmt.name, "x");
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test: var x = 1; x = 2; (reassignment) also works
    #[test]
    fn test_var_reassignment_works() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![
                Stmt::VarDecl(VarDecl {
                    binding: VarBinding::Var,
                    name: ident("x", 4, 5),
                    type_ann: None,
                    init: int_expr(1, 8, 9),
                    span: span(0, 10),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident("x", 11, 12),
                        value: Box::new(int_expr(2, 15, 16)),
                    }),
                    span: span(11, 17),
                }),
            ],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, _, _, _, _, _, _, _, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function item");
        };
        assert_eq!(func.body.stmts.len(), 2);
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(let_stmt.mutable, "var with reassignment should be let mut");
                assert_eq!(let_stmt.name, "x");
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 154: Regex literal syntax
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_regex_literal_simple() {
        // /hello/ → Regex::new("hello").unwrap()
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("re", 0, 2),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::RegexLit {
                        pattern: "hello".to_owned(),
                        flags: String::new(),
                    },
                    span: span(0, 8),
                },
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        // Should be: Regex::new("hello").unwrap()
        match &let_stmt.init.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "unwrap");
                match &receiver.kind {
                    RustExprKind::StaticCall {
                        type_name,
                        method,
                        args,
                    } => {
                        assert_eq!(type_name, "Regex");
                        assert_eq!(method, "new");
                        assert_eq!(args.len(), 1);
                        assert!(
                            matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "hello")
                        );
                    }
                    other => panic!("expected StaticCall(Regex::new), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_regex_literal_with_flags() {
        // /pattern/i → Regex::new("(?i)pattern").unwrap()
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("re", 0, 2),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::RegexLit {
                        pattern: "pattern".to_owned(),
                        flags: "i".to_owned(),
                    },
                    span: span(0, 11),
                },
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        match &let_stmt.init.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "unwrap");
                match &receiver.kind {
                    RustExprKind::StaticCall {
                        type_name,
                        method,
                        args,
                    } => {
                        assert_eq!(type_name, "Regex");
                        assert_eq!(method, "new");
                        assert_eq!(args.len(), 1);
                        assert!(
                            matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "(?i)pattern")
                        );
                    }
                    other => panic!("expected StaticCall(Regex::new), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap), got {other:?}"),
        }
    }

    #[test]
    fn test_lower_regex_literal_global_flag_ignored() {
        // /\d+/g → Regex::new("\\d+").unwrap() (g flag ignored)
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("re", 0, 2),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::RegexLit {
                        pattern: "\\d+".to_owned(),
                        flags: "g".to_owned(),
                    },
                    span: span(0, 7),
                },
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new(false);
        let (file, diags, _, _, _, _, _, _, _) = transform.lower_module(&module);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let RustItem::Function(func) = &file.items[0] else {
            panic!("expected function");
        };
        let RustStmt::Let(let_stmt) = &func.body.stmts[0] else {
            panic!("expected let");
        };
        match &let_stmt.init.kind {
            RustExprKind::MethodCall {
                receiver, method, ..
            } => {
                assert_eq!(method, "unwrap");
                match &receiver.kind {
                    RustExprKind::StaticCall { args, .. } => {
                        assert_eq!(args.len(), 1);
                        // g flag ignored, pattern stays unchanged (no inline flags)
                        assert!(matches!(&args[0].kind, RustExprKind::StringLit(s) if s == "\\d+"));
                    }
                    other => panic!("expected StaticCall(Regex::new), got {other:?}"),
                }
            }
            other => panic!("expected MethodCall(unwrap), got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Task 159: debugger statement produces no meaningful output
    // ---------------------------------------------------------------

    /// Parse and lower, allowing diagnostics (warnings).
    fn lower_source_with_warnings(
        source: &str,
    ) -> (RustFile, Vec<rsc_syntax::diagnostic::Diagnostic>) {
        let file_id = rsc_syntax::source::FileId(0);
        let (module, parse_diags) = rsc_parser::parse(source, file_id);
        assert!(
            parse_diags.is_empty(),
            "unexpected parse diagnostics: {parse_diags:?}"
        );
        let lower_result = crate::lower(&module);
        (lower_result.ir, lower_result.diagnostics)
    }

    #[test]
    fn test_debugger_produces_comment() {
        let ir = lower_source("function main() { debugger; }");
        let func = &ir.items[0];
        if let RustItem::Function(f) = func {
            assert!(
                f.body
                    .stmts
                    .iter()
                    .any(|s| matches!(s, RustStmt::RawRust(code) if code == "// debugger")),
                "expected `// debugger` comment in output, got {:?}",
                f.body.stmts
            );
        } else {
            panic!("expected Function, got {func:?}");
        }
    }

    #[test]
    fn test_new_target_lowers_to_empty_string() {
        let (ir, diags) = lower_source_with_warnings("function main() { const x = new.target; }");
        // Should emit a warning about new.target
        assert!(
            diags.iter().any(|d| d.message.contains("new.target")),
            "expected new.target warning, got: {diags:?}"
        );
        let func = &ir.items[0];
        if let RustItem::Function(f) = func {
            // Should lower to a string literal
            if let RustStmt::Let(let_stmt) = &f.body.stmts[0] {
                assert!(
                    matches!(&let_stmt.init.kind, RustExprKind::StringLit(s) if s.is_empty()),
                    "expected empty string literal, got {:?}",
                    let_stmt.init.kind
                );
            } else {
                panic!("expected Let statement, got {:?}", f.body.stmts[0]);
            }
        } else {
            panic!("expected Function, got {func:?}");
        }
    }

    #[test]
    fn test_import_meta_lowers_to_module_path() {
        let (ir, diags) = lower_source_with_warnings("function main() { const x = import.meta; }");
        // Should emit a warning about import.meta
        assert!(
            diags.iter().any(|d| d.message.contains("import.meta")),
            "expected import.meta warning, got: {diags:?}"
        );
        let func = &ir.items[0];
        if let RustItem::Function(f) = func {
            if let RustStmt::Let(let_stmt) = &f.body.stmts[0] {
                assert!(
                    matches!(&let_stmt.init.kind, RustExprKind::Macro { name, .. } if name == "module_path"),
                    "expected module_path!() macro, got {:?}",
                    let_stmt.init.kind
                );
            } else {
                panic!("expected Let statement, got {:?}", f.body.stmts[0]);
            }
        } else {
            panic!("expected Function, got {func:?}");
        }
    }

    // ---------------------------------------------------------------
    // Task 161: Map/Set forEach and size
    // ---------------------------------------------------------------

    #[test]
    fn test_lower_map_for_each_emits_iter_for_each_with_tuple() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let m: Map<string, i64> = new Map();
                m.set("a", 1);
                m.forEach((value, key) => {
                    console.log(key);
                });
            }
            "#,
        );
        assert!(
            output.contains(".iter().for_each("),
            "expected iter().for_each, got:\n{output}"
        );
        assert!(
            output.contains("(key, value)"),
            "expected swapped tuple param (key, value), got:\n{output}"
        );
    }

    #[test]
    fn test_lower_set_for_each_emits_iter_for_each() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let s: Set<i64> = new Set();
                s.add(1);
                s.forEach((value) => {
                    console.log(value);
                });
            }
            "#,
        );
        assert!(
            output.contains(".iter().for_each("),
            "expected iter().for_each, got:\n{output}"
        );
        assert!(
            output.contains("|value|"),
            "expected single param |value|, got:\n{output}"
        );
    }

    #[test]
    fn test_lower_map_size_emits_len() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let m: Map<string, i64> = new Map();
                let n: i64 = m.size;
            }
            "#,
        );
        assert!(output.contains(".len()"), "expected .len(), got:\n{output}");
    }

    #[test]
    fn test_lower_set_size_emits_len() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let s: Set<i64> = new Set();
                let n: i64 = s.size;
            }
            "#,
        );
        assert!(output.contains(".len()"), "expected .len(), got:\n{output}");
    }

    /// Struct literal type is inferred from function parameter type.
    #[test]
    fn test_struct_literal_inferred_from_param() {
        let output = compile_and_emit(
            "type User = { name: string, age: i32 }\n\
             function greet(user: User): string { return user.name; }\n\
             function main() { greet({ name: \"Alice\", age: 30 }); }",
        );
        assert!(
            output.contains("User {"),
            "expected struct literal to use inferred type 'User', got:\n{output}"
        );
    }

    /// Nested struct literal inference: outer inferred from param, inner from field type.
    #[test]
    fn test_struct_literal_inferred_nested() {
        let output = compile_and_emit(
            "type Address = { city: string }\n\
             type Person = { name: string, addr: Address }\n\
             function process(p: Person): string { return p.name; }\n\
             function main() {\n\
               const a: Address = { city: \"NYC\" };\n\
               process({ name: \"Bob\", addr: a });\n\
             }",
        );
        assert!(
            output.contains("Person {"),
            "expected struct literal to use inferred type 'Person', got:\n{output}"
        );
    }

    /// Explicit type annotation on struct literal still works (regression check).
    #[test]
    fn test_struct_literal_explicit_still_works() {
        let output = compile_and_emit(
            "type User = { name: string, age: i32 }\n\
             function main() { const u: User = { name: \"Alice\", age: 30 }; }",
        );
        assert!(
            output.contains("User {"),
            "expected explicit struct literal to use 'User', got:\n{output}"
        );
    }

    // Array<[T, U]> with inline tuple literals should produce Vec<(T, U)> with tuple constructors
    #[test]
    fn test_array_of_tuples_literal() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                const pairs: Array<[string, i32]> = [["hello", 1], ["world", 2]];
            }
            "#,
        );
        // Should produce tuple constructors, not nested vec! calls
        assert!(
            output.contains("(\"hello\".to_string(), 1)"),
            "expected tuple constructor for first element, got:\n{output}"
        );
        assert!(
            output.contains("(\"world\".to_string(), 2)"),
            "expected tuple constructor for second element, got:\n{output}"
        );
        // Should use vec! macro at the outer level
        assert!(
            output.contains("vec!["),
            "expected vec! at outer level, got:\n{output}"
        );
    }

    // Accessing tuple elements from array of tuples should work with .0, .1
    #[test]
    fn test_array_of_tuples_access() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                const pairs: Array<[string, i32]> = [["hello", 1]];
                const first: [string, i32] = pairs[0];
            }
            "#,
        );
        // Should produce Vec<(String, i32)> with tuple constructor
        assert!(
            output.contains("(\"hello\".to_string(), 1)"),
            "expected tuple constructor, got:\n{output}"
        );
    }

    // Plain Array<i32> should still produce Vec<i32> (regression check)
    #[test]
    fn test_plain_array_not_affected() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                const nums: Array<i32> = [1, 2, 3];
            }
            "#,
        );
        assert!(
            output.contains("vec![1, 2, 3]"),
            "expected plain vec literal, got:\n{output}"
        );
        // Should NOT contain tuple syntax
        assert!(
            !output.contains("(1, 2, 3)"),
            "should not produce tuple for plain array, got:\n{output}"
        );
    }

    // -----------------------------------------------------------------------
    // Numeric widening cast tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_widening_i32_to_i64_in_addition() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let a: i32 = 1;
                let b: i64 = 2;
                let c: i64 = a + b;
            }
            "#,
        );
        assert!(
            output.contains("a as i64"),
            "expected i32 widened to i64 in addition, got:\n{output}"
        );
    }

    #[test]
    fn test_widening_i32_to_i64_compound_assign() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let total: i64 = 0;
                let n: i32 = 5;
                total += n;
            }
            "#,
        );
        assert!(
            output.contains("n as i64"),
            "expected i32 widened to i64 in compound assign, got:\n{output}"
        );
    }

    #[test]
    fn test_widening_i32_to_f64() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let a: i32 = 1;
                let b: f64 = 2.0;
                let c: f64 = a + b;
            }
            "#,
        );
        assert!(
            output.contains("a as f64"),
            "expected i32 widened to f64 in addition, got:\n{output}"
        );
    }

    #[test]
    fn test_widening_f32_to_f64() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let a: f32 = 1.0;
                let b: f64 = 2.0;
                let c: f64 = a + b;
            }
            "#,
        );
        assert!(
            output.contains("a as f64"),
            "expected f32 widened to f64 in addition, got:\n{output}"
        );
    }

    #[test]
    fn test_no_widening_same_type() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let a: i32 = 1;
                let b: i32 = 2;
                let c: i32 = a + b;
            }
            "#,
        );
        assert!(
            !output.contains("as i32"),
            "should not cast when both are i32, got:\n{output}"
        );
    }

    #[test]
    fn test_no_narrowing_i64_to_i32() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let a: i64 = 1;
                let b: i32 = 2;
                let c: i32 = a + b;
            }
            "#,
        );
        // The wider type should be i64, so b gets widened to i64, but a should NOT
        // be narrowed. The result type is i64 (the wider), not i32.
        assert!(
            !output.contains("a as i32"),
            "should not narrow i64 to i32, got:\n{output}"
        );
        assert!(
            output.contains("b as i64"),
            "should widen i32 to i64, got:\n{output}"
        );
    }

    #[test]
    fn test_widening_in_for_loop() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                const nums: Array<i32> = [1, 2, 3, 4, 5];
                let total: i64 = 0;
                for (const n of nums) {
                    total += n;
                }
            }
            "#,
        );
        assert!(
            output.contains("as i64"),
            "expected i32 loop var widened to i64 in compound assign, got:\n{output}"
        );
    }

    #[test]
    fn test_widening_comparison_operators() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let a: i32 = 1;
                let b: i64 = 2;
                let result: bool = a < b;
            }
            "#,
        );
        assert!(
            output.contains("a as i64"),
            "expected i32 widened to i64 in comparison, got:\n{output}"
        );
    }

    #[test]
    fn test_widening_plain_assign() {
        let output = compile_and_emit(
            r#"
            function main(): void {
                let total: i64 = 0;
                let n: i32 = 5;
                total = n;
            }
            "#,
        );
        assert!(
            output.contains("n as i64"),
            "expected i32 widened to i64 in plain assignment, got:\n{output}"
        );
    }
}
