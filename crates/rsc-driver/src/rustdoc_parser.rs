//! Parser for rustdoc JSON output.
//!
//! Extracts function signatures, struct definitions, trait definitions,
//! enum definitions, and impl blocks from the JSON files produced by
//! `cargo doc --output-format json`. Only parses the subset of the rustdoc
//! JSON schema needed for hover display.

use std::collections::HashMap;

/// A parsed rustdoc crate containing all extracted items.
#[derive(Debug, Clone, Default)]
pub struct RustdocCrate {
    /// All extracted items, keyed by their rustdoc ID.
    pub items: HashMap<String, RustdocItem>,
    /// Index from item name to item IDs (multiple items can share a name).
    pub name_index: HashMap<String, Vec<String>>,
    /// IDs of items that are part of the crate's public API — direct children
    /// of the root module or re-exported by it. Empty if root is not found.
    pub public_api_ids: std::collections::HashSet<String>,
}

/// A single item extracted from rustdoc JSON.
#[derive(Debug, Clone)]
pub struct RustdocItem {
    /// The item's unique ID in the rustdoc JSON.
    pub id: String,
    /// The item's name.
    pub name: String,
    /// Documentation string, if any.
    pub docs: Option<String>,
    /// The kind-specific data.
    pub kind: RustdocItemKind,
}

/// Kind-specific data for a rustdoc item.
#[derive(Debug, Clone)]
pub enum RustdocItemKind {
    /// A function or method.
    Function(RustdocFunction),
    /// A struct definition.
    Struct(RustdocStruct),
    /// A trait definition.
    Trait(RustdocTrait),
    /// An enum definition.
    Enum(RustdocEnum),
}

/// A parsed function signature.
#[derive(Debug, Clone)]
pub struct RustdocFunction {
    /// Generic type parameters with optional bounds.
    pub generics: Vec<RustdocGenericParam>,
    /// Function parameters as (name, `type_string`) pairs.
    pub params: Vec<(String, RustdocType)>,
    /// Return type, if not unit.
    pub return_type: Option<RustdocType>,
    /// Whether this is an async function.
    pub is_async: bool,
    /// Whether this is unsafe.
    pub is_unsafe: bool,
    /// Whether this takes `self` (i.e., is a method).
    pub has_self: bool,
    /// The parent type name, if this is an impl method.
    pub parent_type: Option<String>,
    /// Whether this method comes from a trait impl (e.g., `impl Display for Foo`).
    /// Trait impl methods are typically internal plumbing, not the crate's public API.
    pub is_trait_impl: bool,
}

/// A parsed struct definition.
#[derive(Debug, Clone)]
pub struct RustdocStruct {
    /// Generic type parameters.
    pub generics: Vec<RustdocGenericParam>,
    /// Struct fields (empty for unit/tuple structs).
    pub fields: Vec<RustdocField>,
    /// Whether this is a tuple struct.
    pub is_tuple: bool,
    /// Method IDs from impl blocks.
    pub method_ids: Vec<String>,
}

/// A parsed trait definition.
#[derive(Debug, Clone)]
pub struct RustdocTrait {
    /// Generic type parameters.
    pub generics: Vec<RustdocGenericParam>,
    /// Required and provided method IDs.
    pub method_ids: Vec<String>,
}

/// A parsed enum definition.
#[derive(Debug, Clone)]
pub struct RustdocEnum {
    /// Generic type parameters.
    pub generics: Vec<RustdocGenericParam>,
    /// Variant names and optional associated data.
    pub variants: Vec<RustdocVariant>,
}

/// A generic type parameter.
#[derive(Debug, Clone)]
pub struct RustdocGenericParam {
    /// The parameter name (e.g., `T`).
    pub name: String,
    /// Trait bounds (e.g., `["Display", "Clone"]`).
    pub bounds: Vec<String>,
}

/// A struct field.
#[derive(Debug, Clone)]
pub struct RustdocField {
    /// Field name.
    pub name: String,
    /// Field type.
    pub ty: RustdocType,
}

/// An enum variant.
#[derive(Debug, Clone)]
pub struct RustdocVariant {
    /// Variant name.
    pub name: String,
    /// Variant kind (unit, tuple, struct).
    pub kind: RustdocVariantKind,
}

/// The kind of data an enum variant carries.
#[derive(Debug, Clone)]
pub enum RustdocVariantKind {
    /// A unit variant (no data).
    Plain,
    /// A tuple variant with positional fields.
    Tuple(Vec<RustdocType>),
    /// A struct variant with named fields.
    Struct(Vec<RustdocField>),
}

/// A type reference from rustdoc JSON.
///
/// This is a simplified representation of the rustdoc `Type` enum,
/// capturing only the variants needed for signature display.
#[derive(Debug, Clone)]
pub enum RustdocType {
    /// A resolved path like `String`, `Vec<T>`, `std::io::Error`.
    ResolvedPath {
        /// The type name (last segment of the path).
        name: String,
        /// Generic arguments, if any.
        args: Vec<RustdocType>,
    },
    /// A primitive type like `i32`, `bool`, `str`.
    Primitive(String),
    /// A borrowed reference `&T` or `&mut T`.
    BorrowedRef {
        /// Whether the reference is mutable.
        is_mutable: bool,
        /// The referenced type.
        ty: Box<RustdocType>,
    },
    /// A generic type parameter like `T`.
    Generic(String),
    /// A tuple type `(A, B, C)`.
    Tuple(Vec<RustdocType>),
    /// A slice type `[T]`.
    Slice(Box<RustdocType>),
    /// An array type `[T; N]`.
    Array {
        /// Element type.
        ty: Box<RustdocType>,
        /// Length as a string (may be a const expression).
        len: String,
    },
    /// A raw pointer `*const T` or `*mut T`.
    RawPointer {
        /// Whether the pointer is mutable.
        is_mutable: bool,
        /// The pointed-to type.
        ty: Box<RustdocType>,
    },
    /// `impl Trait` in argument or return position.
    ImplTrait(Vec<String>),
    /// A function pointer `fn(A) -> B`.
    FnPointer {
        /// Parameter types.
        params: Vec<RustdocType>,
        /// Return type.
        return_type: Box<RustdocType>,
    },
    /// A qualified path like `<T as Trait>::Assoc`.
    QualifiedPath {
        /// The associated type name (e.g., `Item`).
        name: String,
        /// The self type (e.g., `Self`, `T`), if extracted.
        self_type: Option<Box<RustdocType>>,
        /// The trait the associated type comes from (e.g., `Iterator`).
        trait_name: Option<String>,
    },
    /// An inferred type `_`.
    Infer,
    /// A type we couldn't parse -- stored as raw string.
    Unknown(String),
}

/// Parse a rustdoc JSON file into a [`RustdocCrate`].
///
/// Expects the top-level JSON object from `cargo doc --output-format json`.
/// Extracts functions, structs, traits, and enums from the `index` map.
///
/// Returns `None` if the JSON is not a valid rustdoc format (missing `index`).
#[must_use]
pub fn parse_rustdoc_json(json: &serde_json::Value) -> Option<RustdocCrate> {
    let index = json.get("index")?.as_object()?;
    let paths = json.get("paths").and_then(|v| v.as_object());

    let mut crate_data = RustdocCrate::default();

    // First pass: collect all items.
    for (id, item_value) in index {
        if let Some(item) = parse_item(id, item_value) {
            crate_data
                .name_index
                .entry(item.name.clone())
                .or_default()
                .push(item.id.clone());
            crate_data.items.insert(item.id.clone(), item);
        }
    }

    // Second pass: resolve impl blocks to attach methods to their types.
    for (_id, item_value) in index {
        resolve_impl_block(item_value, &mut crate_data, paths);
    }

    // Third pass: tag functions that belong to a trait's method list as trait methods.
    // These are required/provided methods defined inside `trait Foo { fn bar(); }`.
    let trait_method_ids: std::collections::HashSet<String> = crate_data
        .items
        .values()
        .filter_map(|item| match &item.kind {
            RustdocItemKind::Trait(t) => Some(t.method_ids.iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect();

    for method_id in &trait_method_ids {
        if let Some(item) = crate_data.items.get_mut(method_id) {
            if let RustdocItemKind::Function(func) = &mut item.kind {
                func.is_trait_impl = true;
            }
        }
    }

    // Fourth pass: resolve enum variants from the index.
    // The enum's variant list contains IDs, not definitions. Look them up.
    let enum_ids: Vec<String> = crate_data
        .items
        .values()
        .filter(|item| matches!(item.kind, RustdocItemKind::Enum(_)))
        .map(|item| item.id.clone())
        .collect();

    for enum_id in enum_ids {
        let variant_ids: Vec<String> = {
            let Some(item) = crate_data.items.get(&enum_id) else {
                continue;
            };
            let RustdocItemKind::Enum(e) = &item.kind else {
                continue;
            };
            // Current variants are just names from ID strings — get the actual IDs
            // from the enum data in the raw index
            if let Some(raw_item) = index.get(&enum_id) {
                if let Some(variants) = raw_item
                    .get("inner")
                    .and_then(|i| i.get("enum"))
                    .and_then(|e| e.get("variants"))
                    .and_then(|v| v.as_array())
                {
                    variants
                        .iter()
                        .filter_map(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .or_else(|| v.as_u64().map(|n| n.to_string()))
                        })
                        .collect()
                } else {
                    continue;
                }
            } else {
                continue;
            }
        };

        // Resolve each variant from the index
        let mut resolved_variants = Vec::new();
        for vid in &variant_ids {
            if let Some(v_item) = index.get(vid.as_str()) {
                let v_name = v_item.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                let v_kind = v_item
                    .get("inner")
                    .and_then(|i| i.get("variant"))
                    .and_then(|v| v.get("kind"))
                    .and_then(|k| {
                        if k.get("plain").is_some() || k.as_str() == Some("plain") {
                            Some(RustdocVariantKind::Plain)
                        } else if let Some(tuple_ids) = k.get("tuple").and_then(|t| t.as_array()) {
                            let types: Vec<RustdocType> = tuple_ids
                                .iter()
                                .filter_map(|tid| {
                                    let tid_str = tid
                                        .as_str()
                                        .map(str::to_owned)
                                        .or_else(|| tid.as_u64().map(|n| n.to_string()))?;
                                    let field_item = index.get(&tid_str)?;
                                    let ty_value = field_item
                                        .get("inner")
                                        .and_then(|i| i.get("struct_field"))?;
                                    Some(parse_type(ty_value))
                                })
                                .collect();
                            Some(RustdocVariantKind::Tuple(types))
                        } else if let Some(struct_data) = k.get("struct") {
                            let field_ids = struct_data
                                .get("fields")
                                .and_then(|f| f.as_array())
                                .unwrap_or(&Vec::new())
                                .clone();
                            let fields: Vec<RustdocField> = field_ids
                                .iter()
                                .filter_map(|fid| {
                                    let fid_str = fid
                                        .as_str()
                                        .map(str::to_owned)
                                        .or_else(|| fid.as_u64().map(|n| n.to_string()))?;
                                    let field_item = index.get(&fid_str)?;
                                    let f_name = field_item.get("name")?.as_str()?.to_owned();
                                    let f_ty = field_item
                                        .get("inner")
                                        .and_then(|i| i.get("struct_field"))
                                        .map(parse_type)
                                        .unwrap_or(RustdocType::Unknown("?".to_owned()));
                                    Some(RustdocField {
                                        name: f_name,
                                        ty: f_ty,
                                    })
                                })
                                .collect();
                            Some(RustdocVariantKind::Struct(fields))
                        } else {
                            Some(RustdocVariantKind::Plain)
                        }
                    })
                    .unwrap_or(RustdocVariantKind::Plain);
                resolved_variants.push(RustdocVariant {
                    name: v_name.to_owned(),
                    kind: v_kind,
                });
            }
        }

        if !resolved_variants.is_empty() {
            if let Some(item) = crate_data.items.get_mut(&enum_id) {
                if let RustdocItemKind::Enum(e) = &mut item.kind {
                    e.variants = resolved_variants;
                }
            }
        }
    }

    // Fifth pass: identify the public API by walking from the root module.
    // Only items that are direct children of the root (or re-exported by it)
    // are considered public API. This filters out internal trait methods,
    // impl details, and submodule internals.
    if let Some(root_id) = json.get("root") {
        let root_str = root_id
            .as_str()
            .map(str::to_owned)
            .or_else(|| root_id.as_u64().map(|n| n.to_string()));
        if let Some(root_str) = root_str {
            collect_public_api(index, &root_str, &mut crate_data.public_api_ids);
        }
    }

    Some(crate_data)
}

/// Walk the root module and collect IDs of publicly accessible items.
fn collect_public_api(
    index: &serde_json::Map<String, serde_json::Value>,
    module_id: &str,
    public_ids: &mut std::collections::HashSet<String>,
) {
    let Some(module_item) = index.get(module_id) else {
        return;
    };
    let Some(inner) = module_item.get("inner") else {
        return;
    };
    let Some(mod_data) = inner.get("module") else {
        return;
    };
    let Some(items) = mod_data.get("items").and_then(|i| i.as_array()) else {
        return;
    };

    for item_id_val in items {
        let item_id = item_id_val
            .as_str()
            .map(str::to_owned)
            .or_else(|| item_id_val.as_u64().map(|n| n.to_string()));
        let Some(item_id) = item_id else { continue };

        let Some(child) = index.get(&item_id) else {
            continue;
        };
        let Some(child_inner) = child.get("inner") else {
            continue;
        };

        if let Some(use_data) = child_inner.get("use") {
            // Re-export: follow to the target item
            if let Some(target_id) = use_data.get("id") {
                let target_str = target_id
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| target_id.as_u64().map(|n| n.to_string()));
                if let Some(target_str) = target_str {
                    public_ids.insert(target_str);
                }
            }
        } else if child_inner.get("module").is_some() {
            // Submodule: recurse into it
            collect_public_api(index, &item_id, public_ids);
        } else {
            // Direct item (function, struct, trait, enum, etc.)
            public_ids.insert(item_id);
        }
    }
}

/// Parse a single item from the rustdoc JSON index.
fn parse_item(id: &str, value: &serde_json::Value) -> Option<RustdocItem> {
    let name = value.get("name")?.as_str()?.to_owned();
    let docs = value.get("docs").and_then(|d| d.as_str()).map(String::from);

    let inner = value.get("inner")?;
    let kind_tag = inner.as_object()?.keys().next()?;

    let kind = match kind_tag.as_str() {
        "function" => {
            let func_data = inner.get("function")?;
            Some(RustdocItemKind::Function(parse_function(func_data)?))
        }
        "struct" => {
            let struct_data = inner.get("struct")?;
            Some(RustdocItemKind::Struct(parse_struct(struct_data)))
        }
        "trait" => {
            let trait_data = inner.get("trait")?;
            Some(RustdocItemKind::Trait(parse_trait(trait_data)))
        }
        "enum" => {
            let enum_data = inner.get("enum")?;
            Some(RustdocItemKind::Enum(parse_enum(enum_data)))
        }
        _ => None,
    };

    kind.map(|k| RustdocItem {
        id: id.to_owned(),
        name,
        docs,
        kind: k,
    })
}

/// Parse a function from rustdoc JSON.
fn parse_function(value: &serde_json::Value) -> Option<RustdocFunction> {
    let sig = value.get("sig")?;
    let inputs = sig.get("inputs")?.as_array()?;
    let output = sig.get("output");

    let generics = parse_generics(value.get("generics"));

    let header = value.get("header");
    let is_async = header
        .and_then(|h| h.get("is_async"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let is_unsafe = header
        .and_then(|h| h.get("is_unsafe"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let mut params = Vec::new();
    let mut has_self = false;

    for input in inputs {
        let arr = input.as_array()?;
        if arr.len() >= 2 {
            let param_name = arr[0].as_str().unwrap_or("_").to_owned();
            if param_name == "self" {
                has_self = true;
                continue;
            }
            let param_type = parse_type(&arr[1]);
            params.push((param_name, param_type));
        }
    }

    let return_type = output.and_then(|o| {
        let ty = parse_type(o);
        // Filter out unit returns.
        if matches!(ty, RustdocType::Tuple(ref v) if v.is_empty()) {
            None
        } else {
            Some(ty)
        }
    });

    Some(RustdocFunction {
        generics,
        params,
        return_type,
        is_async,
        is_unsafe,
        has_self,
        parent_type: None,
        is_trait_impl: false,
    })
}

/// Parse a struct from rustdoc JSON.
fn parse_struct(value: &serde_json::Value) -> RustdocStruct {
    let generics = parse_generics(value.get("generics"));

    let kind = value.get("kind");
    let is_tuple = kind.and_then(|k| k.as_str()).is_some_and(|k| k == "tuple");

    let mut fields = Vec::new();
    if let Some(field_ids) = value.get("fields").and_then(|f| f.as_array()) {
        for field_id in field_ids {
            if let Some(field_obj) = field_id.as_object()
                && let (Some(name), Some(ty)) = (
                    field_obj.get("name").and_then(|n| n.as_str()),
                    field_obj.get("type"),
                )
            {
                fields.push(RustdocField {
                    name: name.to_owned(),
                    ty: parse_type(ty),
                });
            }
        }
    }

    RustdocStruct {
        generics,
        fields,
        is_tuple,
        method_ids: Vec::new(),
    }
}

/// Parse a trait from rustdoc JSON.
fn parse_trait(value: &serde_json::Value) -> RustdocTrait {
    let generics = parse_generics(value.get("generics"));

    let method_ids = value
        .get("items")
        .and_then(|i| i.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    RustdocTrait {
        generics,
        method_ids,
    }
}

/// Parse an enum from rustdoc JSON.
fn parse_enum(value: &serde_json::Value) -> RustdocEnum {
    let generics = parse_generics(value.get("generics"));

    let variants = value
        .get("variants")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_variant_ref).collect())
        .unwrap_or_default();

    RustdocEnum { generics, variants }
}

/// Parse a variant reference from the enum's variant list.
fn parse_variant_ref(value: &serde_json::Value) -> Option<RustdocVariant> {
    // In rustdoc JSON, variants in the enum object are IDs.
    // We'll handle them as simple names for now and resolve later.
    let name = value.as_str()?.to_owned();
    Some(RustdocVariant {
        name,
        kind: RustdocVariantKind::Plain,
    })
}

/// Parse generic parameters from a rustdoc `generics` object.
///
/// Parses both `params` (inline bounds like `<T: Display>`) and
/// `where_predicates` (where clause bounds like `where T: Clone`),
/// merging bounds from both sources into a unified parameter list.
fn parse_generics(value: Option<&serde_json::Value>) -> Vec<RustdocGenericParam> {
    let Some(generics) = value else {
        return Vec::new();
    };

    let mut result: Vec<RustdocGenericParam> = generics
        .get("params")
        .and_then(|p| p.as_array())
        .map(|params| {
            params
                .iter()
                .filter_map(|p| {
                    let name = p.get("name")?.as_str()?.to_owned();
                    let kind = p.get("kind")?;

                    // Only include type parameters (not lifetime or const).
                    let type_obj = kind.get("type")?;

                    let bounds = type_obj
                        .get("bounds")
                        .and_then(|b| b.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|bound| {
                                    let trait_bound = bound.get("trait_bound")?;
                                    let trait_ref = trait_bound.get("trait")?;
                                    extract_type_name(trait_ref)
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    Some(RustdocGenericParam { name, bounds })
                })
                .collect()
        })
        .unwrap_or_default();

    // Parse where_predicates and merge bounds into existing params.
    if let Some(where_preds) = generics.get("where_predicates").and_then(|w| w.as_array()) {
        for pred in where_preds {
            if let Some(bound_pred) = pred.get("bound_predicate") {
                // Extract the type this predicate applies to.
                let type_name = bound_pred
                    .get("type")
                    .and_then(|t| t.get("generic").and_then(|g| g.as_str()))
                    .map(str::to_owned);

                if let Some(name) = type_name {
                    // Extract bounds from the predicate.
                    let new_bounds: Vec<String> = bound_pred
                        .get("bounds")
                        .and_then(|b| b.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|bound| {
                                    let trait_bound = bound.get("trait_bound")?;
                                    let trait_ref = trait_bound.get("trait")?;
                                    extract_type_name(trait_ref)
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    // Merge into existing param or create a new one.
                    if let Some(existing) = result.iter_mut().find(|p| p.name == name) {
                        existing.bounds.extend(new_bounds);
                    } else {
                        result.push(RustdocGenericParam {
                            name,
                            bounds: new_bounds,
                        });
                    }
                }
            }
        }
    }

    result
}

/// Parse a rustdoc `Type` value into our simplified representation.
#[allow(clippy::too_many_lines)]
// Rustdoc JSON has many type variants; splitting this match would obscure the mapping
fn parse_type(value: &serde_json::Value) -> RustdocType {
    // The rustdoc JSON type is a tagged enum.
    let Some(obj) = value.as_object() else {
        // Could be a string shorthand for primitive types.
        if let Some(s) = value.as_str() {
            return RustdocType::Primitive(s.to_owned());
        }
        return RustdocType::Unknown(format!("{value}"));
    };

    // Single-key object: the key is the variant tag.
    let Some((tag, inner)) = obj.iter().next() else {
        return RustdocType::Unknown(format!("{value}"));
    };

    match tag.as_str() {
        "resolved_path" => parse_resolved_path(inner),
        "primitive" => {
            let name = inner.as_str().unwrap_or("unknown");
            RustdocType::Primitive(name.to_owned())
        }
        "borrowed_ref" => {
            let is_mutable = inner
                .get("is_mutable")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let ty = inner.get("type").map_or_else(
                || Box::new(RustdocType::Unknown("?".to_owned())),
                |t| Box::new(parse_type(t)),
            );
            RustdocType::BorrowedRef { is_mutable, ty }
        }
        "generic" => {
            let name = inner.as_str().unwrap_or("T");
            RustdocType::Generic(name.to_owned())
        }
        "tuple" => {
            let types = inner
                .as_array()
                .map(|arr| arr.iter().map(parse_type).collect())
                .unwrap_or_default();
            RustdocType::Tuple(types)
        }
        "slice" => {
            let ty = Box::new(parse_type(inner));
            RustdocType::Slice(ty)
        }
        "array" => {
            let ty = inner.get("type").map_or_else(
                || Box::new(RustdocType::Unknown("?".to_owned())),
                |t| Box::new(parse_type(t)),
            );
            let len = inner
                .get("len")
                .and_then(|l| l.as_str())
                .unwrap_or("?")
                .to_owned();
            RustdocType::Array { ty, len }
        }
        "raw_pointer" => {
            let is_mutable = inner
                .get("is_mutable")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let ty = inner.get("type").map_or_else(
                || Box::new(RustdocType::Unknown("?".to_owned())),
                |t| Box::new(parse_type(t)),
            );
            RustdocType::RawPointer { is_mutable, ty }
        }
        "impl_trait" => {
            let bounds = inner
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|b| {
                            let trait_bound = b.get("trait_bound")?;
                            let trait_ref = trait_bound.get("trait")?;
                            extract_type_name(trait_ref)
                        })
                        .collect()
                })
                .unwrap_or_default();
            RustdocType::ImplTrait(bounds)
        }
        "dyn_trait" => {
            // dyn Trait — same structure as impl_trait but with "traits" key
            let bounds = inner
                .get("traits")
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|b| {
                            let trait_ref = b.get("trait")?;
                            extract_type_name(trait_ref)
                        })
                        .collect()
                })
                .unwrap_or_default();
            RustdocType::ImplTrait(bounds)
        }
        "function_pointer" => {
            let sig = inner.get("sig");
            let params = sig
                .and_then(|s| s.get("inputs"))
                .and_then(|i| i.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|input| {
                            let pair = input.as_array()?;
                            if pair.len() >= 2 {
                                Some(parse_type(&pair[1]))
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let return_type = sig.and_then(|s| s.get("output")).map_or_else(
                || Box::new(RustdocType::Tuple(Vec::new())),
                |o| Box::new(parse_type(o)),
            );
            RustdocType::FnPointer {
                params,
                return_type,
            }
        }
        "qualified_path" => {
            let name = inner
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("?")
                .to_owned();
            let self_type = inner.get("self_type").map(|st| Box::new(parse_type(st)));
            let trait_name = inner.get("trait").and_then(|t| extract_type_name(t));
            RustdocType::QualifiedPath {
                name,
                self_type,
                trait_name,
            }
        }
        "infer" => RustdocType::Infer,
        _ => RustdocType::Unknown(format!("{value}")),
    }
}

/// Parse a `resolved_path` type from rustdoc JSON.
fn parse_resolved_path(value: &serde_json::Value) -> RustdocType {
    // Newer rustdoc JSON uses "path", older uses "name".
    let name = value
        .get("name")
        .or_else(|| value.get("path"))
        .and_then(|n| n.as_str())
        .unwrap_or("?")
        .to_owned();

    let args = value
        .get("args")
        .and_then(|a| a.get("angle_bracketed"))
        .and_then(|ab| ab.get("args"))
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|arg| {
                    // Each arg is either {"type": ...} or {"lifetime": ...} etc.
                    arg.get("type").map(parse_type)
                })
                .collect()
        })
        .unwrap_or_default();

    RustdocType::ResolvedPath { name, args }
}

/// Extract the display name from a type reference (for trait bounds).
fn extract_type_name(value: &serde_json::Value) -> Option<String> {
    // Could be a resolved_path or other type variant.
    if let Some(obj) = value.as_object()
        && let Some(rp) = obj.get("resolved_path")
    {
        return rp.get("name").and_then(|n| n.as_str()).map(String::from);
    }
    value.get("name").and_then(|n| n.as_str()).map(String::from)
}

/// Resolve an impl block, attaching method IDs to their parent struct/enum.
fn resolve_impl_block(
    item_value: &serde_json::Value,
    crate_data: &mut RustdocCrate,
    paths: Option<&serde_json::Map<String, serde_json::Value>>,
) {
    let Some(inner) = item_value.get("inner") else {
        return;
    };

    let Some(impl_data) = inner.get("impl") else {
        return;
    };

    let Some(for_type) = impl_data.get("for") else {
        return;
    };

    let Some(type_name) = extract_impl_type_name(for_type, paths) else {
        return;
    };

    // Check if this is a trait impl (has a "trait" field) vs an inherent impl.
    let is_trait_impl = impl_data.get("trait").is_some();

    // Get method IDs (may be strings or integers depending on rustdoc JSON version).
    let method_ids: Vec<String> = impl_data
        .get("items")
        .and_then(|i| i.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(String::from)
                        .or_else(|| v.as_u64().map(|n| n.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    // Tag each method item with its parent type and trait impl status.
    for method_id in &method_ids {
        if let Some(item) = crate_data.items.get_mut(method_id)
            && let RustdocItemKind::Function(func) = &mut item.kind
        {
            func.parent_type = Some(type_name.clone());
            func.is_trait_impl = is_trait_impl;
        }
    }

    // Find the struct/enum item and add method IDs.
    if let Some(ids) = crate_data.name_index.get(&type_name) {
        for id in ids {
            if let Some(item) = crate_data.items.get_mut(id) {
                match &mut item.kind {
                    RustdocItemKind::Struct(s) => {
                        s.method_ids.extend(method_ids.iter().cloned());
                    }
                    RustdocItemKind::Enum(_e) => {
                        // Enums don't have method_ids in our model yet.
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Extract the type name from a `for` clause in an impl block.
fn extract_impl_type_name(
    for_type: &serde_json::Value,
    paths: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<String> {
    let obj = for_type.as_object()?;
    let rp = obj.get("resolved_path")?;

    // `resolved_path` may use "name" (older format) or "path" (newer format).
    if let Some(name) = rp
        .get("name")
        .or_else(|| rp.get("path"))
        .and_then(|n| n.as_str())
    {
        // Extract the last segment of a qualified path (e.g., "TcpListener" from "net::TcpListener")
        return Some(name.rsplit("::").next().unwrap_or(name).to_owned());
    }

    // Fall back to looking up by ID in paths.
    // ID may be a string or integer depending on rustdoc JSON version.
    let id_str = rp.get("id").and_then(|i| {
        i.as_str()
            .map(String::from)
            .or_else(|| i.as_u64().map(|n| n.to_string()))
    });
    if let Some(id) = id_str.as_deref()
        && let Some(path_entry) = paths.and_then(|p| p.get(id))
        && let Some(name) = path_entry.get("path").and_then(|p| {
            p.as_array()
                .and_then(|arr| arr.last())
                .and_then(|n| n.as_str())
        })
    {
        return Some(name.to_owned());
    }

    None
}

/// Look up an item by name in the parsed crate data.
///
/// Returns the first item matching the given name, preferring non-method items
/// (structs, enums, traits) over methods.
#[must_use]
pub fn lookup_item<'a>(crate_data: &'a RustdocCrate, name: &str) -> Option<&'a RustdocItem> {
    let ids = crate_data.name_index.get(name)?;

    // Prefer non-function items (struct/enum/trait definitions).
    let mut function_item = None;
    for id in ids {
        if let Some(item) = crate_data.items.get(id) {
            match &item.kind {
                RustdocItemKind::Function(_) => {
                    if function_item.is_none() {
                        function_item = Some(item);
                    }
                }
                _ => return Some(item),
            }
        }
    }

    function_item
}

/// Look up all items by name in the parsed crate data.
#[must_use]
pub fn lookup_items<'a>(crate_data: &'a RustdocCrate, name: &str) -> Vec<&'a RustdocItem> {
    crate_data
        .name_index
        .get(name)
        .map(|ids| {
            ids.iter()
                .filter_map(|id| crate_data.items.get(id))
                .collect()
        })
        .unwrap_or_default()
}

/// Look up methods on a type by the type's name.
#[must_use]
pub fn lookup_methods<'a>(crate_data: &'a RustdocCrate, type_name: &str) -> Vec<&'a RustdocItem> {
    crate_data
        .items
        .values()
        .filter(|item| {
            if let RustdocItemKind::Function(func) = &item.kind {
                func.parent_type.as_deref() == Some(type_name)
            } else {
                false
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_function_json() -> serde_json::Value {
        serde_json::json!({
            "index": {
                "0:3": {
                    "name": "greet",
                    "docs": "Greets a user by name.",
                    "inner": {
                        "function": {
                            "sig": {
                                "inputs": [
                                    ["name", {"resolved_path": {"name": "String", "id": "0:1", "args": {"angle_bracketed": {"args": []}}}}]
                                ],
                                "output": {"resolved_path": {"name": "String", "id": "0:1", "args": {"angle_bracketed": {"args": []}}}}
                            },
                            "generics": {
                                "params": []
                            },
                            "header": {
                                "is_async": false,
                                "is_unsafe": false
                            }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn test_rustdoc_parser_parses_function_name() {
        let crate_data = parse_rustdoc_json(&sample_function_json()).unwrap();
        let item = lookup_item(&crate_data, "greet").unwrap();
        assert_eq!(item.name, "greet");
    }

    #[test]
    fn test_rustdoc_parser_parses_function_docs() {
        let crate_data = parse_rustdoc_json(&sample_function_json()).unwrap();
        let item = lookup_item(&crate_data, "greet").unwrap();
        assert_eq!(item.docs.as_deref(), Some("Greets a user by name."));
    }

    #[test]
    fn test_rustdoc_parser_parses_function_params() {
        let crate_data = parse_rustdoc_json(&sample_function_json()).unwrap();
        let item = lookup_item(&crate_data, "greet").unwrap();
        if let RustdocItemKind::Function(func) = &item.kind {
            assert_eq!(func.params.len(), 1);
            assert_eq!(func.params[0].0, "name");
            assert!(matches!(
                &func.params[0].1,
                RustdocType::ResolvedPath { name, .. } if name == "String"
            ));
        } else {
            panic!("expected function");
        }
    }

    #[test]
    fn test_rustdoc_parser_parses_function_return_type() {
        let crate_data = parse_rustdoc_json(&sample_function_json()).unwrap();
        let item = lookup_item(&crate_data, "greet").unwrap();
        if let RustdocItemKind::Function(func) = &item.kind {
            assert!(func.return_type.is_some());
            assert!(matches!(
                func.return_type.as_ref().unwrap(),
                RustdocType::ResolvedPath { name, .. } if name == "String"
            ));
        } else {
            panic!("expected function");
        }
    }

    fn sample_struct_json() -> serde_json::Value {
        serde_json::json!({
            "index": {
                "0:5": {
                    "name": "User",
                    "docs": "A user in the system.",
                    "inner": {
                        "struct": {
                            "generics": {
                                "params": []
                            },
                            "kind": "plain",
                            "fields": []
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn test_rustdoc_parser_parses_struct_name() {
        let crate_data = parse_rustdoc_json(&sample_struct_json()).unwrap();
        let item = lookup_item(&crate_data, "User").unwrap();
        assert_eq!(item.name, "User");
        assert!(matches!(item.kind, RustdocItemKind::Struct(_)));
    }

    #[test]
    fn test_rustdoc_parser_parses_struct_docs() {
        let crate_data = parse_rustdoc_json(&sample_struct_json()).unwrap();
        let item = lookup_item(&crate_data, "User").unwrap();
        assert_eq!(item.docs.as_deref(), Some("A user in the system."));
    }

    fn sample_trait_json() -> serde_json::Value {
        serde_json::json!({
            "index": {
                "0:8": {
                    "name": "Handler",
                    "docs": "A request handler.",
                    "inner": {
                        "trait": {
                            "generics": {
                                "params": [
                                    {
                                        "name": "T",
                                        "kind": {
                                            "type": {
                                                "bounds": []
                                            }
                                        }
                                    }
                                ]
                            },
                            "items": ["0:9"]
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn test_rustdoc_parser_parses_trait_name() {
        let crate_data = parse_rustdoc_json(&sample_trait_json()).unwrap();
        let item = lookup_item(&crate_data, "Handler").unwrap();
        assert_eq!(item.name, "Handler");
        assert!(matches!(item.kind, RustdocItemKind::Trait(_)));
    }

    #[test]
    fn test_rustdoc_parser_parses_trait_generics() {
        let crate_data = parse_rustdoc_json(&sample_trait_json()).unwrap();
        let item = lookup_item(&crate_data, "Handler").unwrap();
        if let RustdocItemKind::Trait(t) = &item.kind {
            assert_eq!(t.generics.len(), 1);
            assert_eq!(t.generics[0].name, "T");
        } else {
            panic!("expected trait");
        }
    }

    fn sample_enum_json() -> serde_json::Value {
        serde_json::json!({
            "index": {
                "0:10": {
                    "name": "Method",
                    "docs": "HTTP methods.",
                    "inner": {
                        "enum": {
                            "generics": {
                                "params": []
                            },
                            "variants": ["Get", "Post", "Put", "Delete"]
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn test_rustdoc_parser_parses_enum_name() {
        let crate_data = parse_rustdoc_json(&sample_enum_json()).unwrap();
        let item = lookup_item(&crate_data, "Method").unwrap();
        assert_eq!(item.name, "Method");
        assert!(matches!(item.kind, RustdocItemKind::Enum(_)));
    }

    #[test]
    fn test_rustdoc_parser_parses_enum_variants() {
        let crate_data = parse_rustdoc_json(&sample_enum_json()).unwrap();
        let item = lookup_item(&crate_data, "Method").unwrap();
        if let RustdocItemKind::Enum(e) = &item.kind {
            assert_eq!(e.variants.len(), 4);
            assert_eq!(e.variants[0].name, "Get");
            assert_eq!(e.variants[1].name, "Post");
            assert_eq!(e.variants[2].name, "Put");
            assert_eq!(e.variants[3].name, "Delete");
        } else {
            panic!("expected enum");
        }
    }

    #[test]
    fn test_rustdoc_parser_missing_index_returns_none() {
        let json = serde_json::json!({"no_index": {}});
        assert!(parse_rustdoc_json(&json).is_none());
    }

    #[test]
    fn test_rustdoc_parser_name_index_lookup() {
        let crate_data = parse_rustdoc_json(&sample_function_json()).unwrap();
        assert!(crate_data.name_index.contains_key("greet"));
        assert!(!crate_data.name_index.contains_key("nonexistent"));
    }

    #[test]
    fn test_rustdoc_parser_type_primitive() {
        let ty = parse_type(&serde_json::json!({"primitive": "i32"}));
        assert!(matches!(ty, RustdocType::Primitive(ref s) if s == "i32"));
    }

    #[test]
    fn test_rustdoc_parser_type_generic() {
        let ty = parse_type(&serde_json::json!({"generic": "T"}));
        assert!(matches!(ty, RustdocType::Generic(ref s) if s == "T"));
    }

    #[test]
    fn test_rustdoc_parser_type_borrowed_ref() {
        let ty = parse_type(&serde_json::json!({
            "borrowed_ref": {
                "is_mutable": false,
                "type": {"primitive": "str"}
            }
        }));
        assert!(matches!(
            ty,
            RustdocType::BorrowedRef {
                is_mutable: false,
                ..
            }
        ));
    }

    #[test]
    fn test_rustdoc_parser_type_borrowed_ref_mutable() {
        let ty = parse_type(&serde_json::json!({
            "borrowed_ref": {
                "is_mutable": true,
                "type": {"primitive": "str"}
            }
        }));
        assert!(matches!(
            ty,
            RustdocType::BorrowedRef {
                is_mutable: true,
                ..
            }
        ));
    }

    #[test]
    fn test_rustdoc_parser_type_tuple() {
        let ty = parse_type(&serde_json::json!({
            "tuple": [{"primitive": "i32"}, {"primitive": "bool"}]
        }));
        if let RustdocType::Tuple(types) = ty {
            assert_eq!(types.len(), 2);
        } else {
            panic!("expected tuple type");
        }
    }

    #[test]
    fn test_rustdoc_parser_type_resolved_path_with_args() {
        let ty = parse_type(&serde_json::json!({
            "resolved_path": {
                "name": "Vec",
                "id": "0:1",
                "args": {
                    "angle_bracketed": {
                        "args": [
                            {"type": {"primitive": "i32"}}
                        ]
                    }
                }
            }
        }));
        if let RustdocType::ResolvedPath { name, args } = ty {
            assert_eq!(name, "Vec");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected resolved path");
        }
    }

    #[test]
    fn test_rustdoc_parser_type_impl_trait() {
        let ty = parse_type(&serde_json::json!({
            "impl_trait": [
                {
                    "trait_bound": {
                        "trait": {
                            "resolved_path": {
                                "name": "Display"
                            }
                        }
                    }
                }
            ]
        }));
        if let RustdocType::ImplTrait(bounds) = ty {
            assert_eq!(bounds, vec!["Display"]);
        } else {
            panic!("expected impl trait");
        }
    }

    #[test]
    fn test_rustdoc_parser_type_slice() {
        let ty = parse_type(&serde_json::json!({
            "slice": {"primitive": "u8"}
        }));
        assert!(matches!(ty, RustdocType::Slice(_)));
    }

    #[test]
    fn test_rustdoc_parser_type_infer() {
        let ty = parse_type(&serde_json::json!({"infer": null}));
        assert!(matches!(ty, RustdocType::Infer));
    }

    #[test]
    fn test_rustdoc_parser_type_unknown_tag() {
        let ty = parse_type(&serde_json::json!({"some_future_type": {}}));
        assert!(matches!(ty, RustdocType::Unknown(_)));
    }

    fn sample_generic_function_json() -> serde_json::Value {
        serde_json::json!({
            "index": {
                "0:20": {
                    "name": "get",
                    "docs": "Create a GET handler.",
                    "inner": {
                        "function": {
                            "sig": {
                                "inputs": [
                                    ["handler", {"generic": "H"}]
                                ],
                                "output": {"resolved_path": {"name": "MethodRouter", "id": "0:21", "args": {"angle_bracketed": {"args": []}}}}
                            },
                            "generics": {
                                "params": [
                                    {
                                        "name": "H",
                                        "kind": {
                                            "type": {
                                                "bounds": [
                                                    {
                                                        "trait_bound": {
                                                            "trait": {
                                                                "resolved_path": {
                                                                    "name": "Handler"
                                                                }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    },
                                    {
                                        "name": "T",
                                        "kind": {
                                            "type": {
                                                "bounds": []
                                            }
                                        }
                                    }
                                ]
                            },
                            "header": {
                                "is_async": false,
                                "is_unsafe": false
                            }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn test_rustdoc_parser_parses_generic_function_params() {
        let crate_data = parse_rustdoc_json(&sample_generic_function_json()).unwrap();
        let item = lookup_item(&crate_data, "get").unwrap();
        if let RustdocItemKind::Function(func) = &item.kind {
            assert_eq!(func.generics.len(), 2);
            assert_eq!(func.generics[0].name, "H");
            assert_eq!(func.generics[0].bounds, vec!["Handler"]);
            assert_eq!(func.generics[1].name, "T");
            assert!(func.generics[1].bounds.is_empty());
        } else {
            panic!("expected function");
        }
    }

    fn sample_method_json() -> serde_json::Value {
        serde_json::json!({
            "index": {
                "0:30": {
                    "name": "route",
                    "docs": "Add a route.",
                    "inner": {
                        "function": {
                            "sig": {
                                "inputs": [
                                    ["self", {"generic": "Self"}],
                                    ["path", {"borrowed_ref": {"is_mutable": false, "type": {"primitive": "str"}}}],
                                    ["handler", {"resolved_path": {"name": "MethodRouter", "id": "0:21", "args": {"angle_bracketed": {"args": []}}}}]
                                ],
                                "output": {"resolved_path": {"name": "Router", "id": "0:31", "args": {"angle_bracketed": {"args": []}}}}
                            },
                            "generics": {
                                "params": []
                            },
                            "header": {
                                "is_async": false,
                                "is_unsafe": false
                            }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn test_rustdoc_parser_parses_method_self_param() {
        let crate_data = parse_rustdoc_json(&sample_method_json()).unwrap();
        let item = lookup_item(&crate_data, "route").unwrap();
        if let RustdocItemKind::Function(func) = &item.kind {
            assert!(func.has_self);
            // `self` should not appear in params list
            assert_eq!(func.params.len(), 2);
            assert_eq!(func.params[0].0, "path");
            assert_eq!(func.params[1].0, "handler");
        } else {
            panic!("expected function");
        }
    }

    fn sample_async_function_json() -> serde_json::Value {
        serde_json::json!({
            "index": {
                "0:40": {
                    "name": "serve",
                    "docs": null,
                    "inner": {
                        "function": {
                            "sig": {
                                "inputs": [],
                                "output": {"tuple": []}
                            },
                            "generics": {
                                "params": []
                            },
                            "header": {
                                "is_async": true,
                                "is_unsafe": false
                            }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn test_rustdoc_parser_parses_async_function() {
        let crate_data = parse_rustdoc_json(&sample_async_function_json()).unwrap();
        let item = lookup_item(&crate_data, "serve").unwrap();
        if let RustdocItemKind::Function(func) = &item.kind {
            assert!(func.is_async);
            assert!(func.return_type.is_none()); // unit return
        } else {
            panic!("expected function");
        }
    }

    // ----- Where clause parsing tests -----

    fn sample_where_clause_function_json() -> serde_json::Value {
        serde_json::json!({
            "index": {
                "0:60": {
                    "name": "handler",
                    "docs": "Handle a request.",
                    "inner": {
                        "function": {
                            "sig": {
                                "inputs": [
                                    ["h", {"generic": "H"}]
                                ],
                                "output": {"tuple": []}
                            },
                            "generics": {
                                "params": [
                                    {
                                        "name": "H",
                                        "kind": {
                                            "type": {
                                                "bounds": []
                                            }
                                        }
                                    }
                                ],
                                "where_predicates": [
                                    {
                                        "bound_predicate": {
                                            "type": {"generic": "H"},
                                            "bounds": [
                                                {
                                                    "trait_bound": {
                                                        "trait": {
                                                            "resolved_path": {
                                                                "name": "Handler"
                                                            }
                                                        }
                                                    }
                                                },
                                                {
                                                    "trait_bound": {
                                                        "trait": {
                                                            "resolved_path": {
                                                                "name": "Clone"
                                                            }
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    }
                                ]
                            },
                            "header": {
                                "is_async": false,
                                "is_unsafe": false
                            }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn test_rustdoc_parser_where_clause_merges_bounds_into_existing_param() {
        let crate_data = parse_rustdoc_json(&sample_where_clause_function_json()).unwrap();
        let item = lookup_item(&crate_data, "handler").unwrap();
        if let RustdocItemKind::Function(func) = &item.kind {
            assert_eq!(
                func.generics.len(),
                1,
                "should have exactly one generic param"
            );
            assert_eq!(func.generics[0].name, "H");
            assert_eq!(
                func.generics[0].bounds,
                vec!["Handler", "Clone"],
                "where clause bounds should be merged into H"
            );
        } else {
            panic!("expected function");
        }
    }

    #[test]
    fn test_rustdoc_parser_where_clause_creates_new_param() {
        // Where clause on a type not in params list.
        let json = serde_json::json!({
            "index": {
                "0:70": {
                    "name": "process",
                    "docs": null,
                    "inner": {
                        "function": {
                            "sig": {
                                "inputs": [
                                    ["x", {"generic": "T"}]
                                ],
                                "output": {"tuple": []}
                            },
                            "generics": {
                                "params": [
                                    {
                                        "name": "T",
                                        "kind": {
                                            "type": {
                                                "bounds": [
                                                    {
                                                        "trait_bound": {
                                                            "trait": {
                                                                "resolved_path": {
                                                                    "name": "Debug"
                                                                }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    }
                                ],
                                "where_predicates": [
                                    {
                                        "bound_predicate": {
                                            "type": {"generic": "T"},
                                            "bounds": [
                                                {
                                                    "trait_bound": {
                                                        "trait": {
                                                            "resolved_path": {
                                                                "name": "Clone"
                                                            }
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    },
                                    {
                                        "bound_predicate": {
                                            "type": {"generic": "U"},
                                            "bounds": [
                                                {
                                                    "trait_bound": {
                                                        "trait": {
                                                            "resolved_path": {
                                                                "name": "Send"
                                                            }
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    }
                                ]
                            },
                            "header": {
                                "is_async": false,
                                "is_unsafe": false
                            }
                        }
                    }
                }
            }
        });
        let crate_data = parse_rustdoc_json(&json).unwrap();
        let item = lookup_item(&crate_data, "process").unwrap();
        if let RustdocItemKind::Function(func) = &item.kind {
            assert_eq!(
                func.generics.len(),
                2,
                "should have T from params + U from where clause"
            );
            // T: Debug (from params) + Clone (from where clause)
            assert_eq!(func.generics[0].name, "T");
            assert_eq!(func.generics[0].bounds, vec!["Debug", "Clone"]);
            // U: Send (new param from where clause)
            assert_eq!(func.generics[1].name, "U");
            assert_eq!(func.generics[1].bounds, vec!["Send"]);
        } else {
            panic!("expected function");
        }
    }

    // ----- Qualified path parsing tests -----

    #[test]
    fn test_rustdoc_parser_type_qualified_path_self() {
        let ty = parse_type(&serde_json::json!({
            "qualified_path": {
                "name": "Item",
                "self_type": {"generic": "Self"},
                "trait": {"resolved_path": {"name": "Iterator"}}
            }
        }));
        if let RustdocType::QualifiedPath {
            name,
            self_type,
            trait_name,
        } = &ty
        {
            assert_eq!(name, "Item");
            assert!(matches!(self_type.as_deref(), Some(RustdocType::Generic(g)) if g == "Self"));
            assert_eq!(trait_name.as_deref(), Some("Iterator"));
        } else {
            panic!("expected QualifiedPath, got {ty:?}");
        }
    }

    #[test]
    fn test_rustdoc_parser_type_qualified_path_generic() {
        let ty = parse_type(&serde_json::json!({
            "qualified_path": {
                "name": "Output",
                "self_type": {"generic": "T"},
                "trait": {"resolved_path": {"name": "Add"}}
            }
        }));
        if let RustdocType::QualifiedPath {
            name,
            self_type,
            trait_name,
        } = &ty
        {
            assert_eq!(name, "Output");
            assert!(matches!(self_type.as_deref(), Some(RustdocType::Generic(g)) if g == "T"));
            assert_eq!(trait_name.as_deref(), Some("Add"));
        } else {
            panic!("expected QualifiedPath, got {ty:?}");
        }
    }

    #[test]
    fn test_rustdoc_parser_lookup_items_returns_all_matches() {
        let json = serde_json::json!({
            "index": {
                "0:50": {
                    "name": "new",
                    "docs": null,
                    "inner": {
                        "function": {
                            "sig": {"inputs": [], "output": {"tuple": []}},
                            "generics": {"params": []},
                            "header": {"is_async": false, "is_unsafe": false}
                        }
                    }
                },
                "0:51": {
                    "name": "new",
                    "docs": null,
                    "inner": {
                        "function": {
                            "sig": {"inputs": [], "output": {"tuple": []}},
                            "generics": {"params": []},
                            "header": {"is_async": false, "is_unsafe": false}
                        }
                    }
                }
            }
        });
        let crate_data = parse_rustdoc_json(&json).unwrap();
        let items = lookup_items(&crate_data, "new");
        assert_eq!(items.len(), 2);
    }
}
