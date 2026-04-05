//! Type lowering logic.
//!
//! Handles resolution and lowering of type definitions, utility types (Partial,
//! Required, Readonly, Record, Pick, Omit, `ReturnType`, Parameters), mapped types,
//! keyof types, intersection types, enum definitions, and interface definitions.
//! Also provides free functions for type parameter lowering, derive merging, and
//! name capitalization used across the transform pipeline.

use std::collections::HashSet;

use rustscript_syntax::ast;
use rustscript_syntax::diagnostic::Diagnostic;
use rustscript_syntax::rust_ir::{
    ParamMode, RustEnumDef, RustEnumVariant, RustFieldDef, RustItem, RustStructDef, RustTraitDef,
    RustTraitMethod, RustType, RustTypeAlias, RustTypeParam,
};

use crate::context::LoweringContext;
use crate::derive_inference;
use rustscript_typeck::resolve;
use rustscript_typeck::types::Type;

use super::Transform;

impl Transform {
    /// Register a type definition in the type registry during the pre-pass.
    pub(super) fn register_type_def(&mut self, td: &ast::TypeDef, ctx: &mut LoweringContext) {
        // Check for utility type alias: type X = Partial<Y>
        if let Some(ref alias) = td.type_alias {
            if Self::identify_utility_type(alias).is_some() {
                self.register_utility_type_def(td, alias, ctx);
                return;
            }
            // Mapped type: { [K in keyof T]: V }
            if matches!(alias.kind, ast::TypeKind::MappedType { .. }) {
                self.register_mapped_type_def(td, alias, ctx);
                return;
            }
            // keyof T — register as a simple enum with field name variants
            if let ast::TypeKind::KeyOf(ref inner) = alias.kind {
                if let ast::TypeKind::Named(ref ident) = inner.kind {
                    if let Some(reg_type) = self.type_registry.lookup(&ident.name) {
                        let field_names: Vec<String> = match &reg_type.kind {
                            rustscript_typeck::registry::TypeDefKind::Struct(fields) => fields
                                .iter()
                                .map(|(name, _)| capitalize_first(name))
                                .collect(),
                            rustscript_typeck::registry::TypeDefKind::Class { fields, .. } => {
                                fields
                                    .iter()
                                    .map(|(name, _)| capitalize_first(name))
                                    .collect()
                            }
                            _ => {
                                ctx.emit_diagnostic(Diagnostic::error(format!(
                                    "`keyof` requires a struct or class type, but `{}` is an enum or interface",
                                    ident.name
                                )));
                                Vec::new()
                            }
                        };
                        self.type_registry
                            .register_simple_enum(td.name.name.clone(), field_names);
                    } else {
                        ctx.emit_diagnostic(Diagnostic::error(format!(
                            "`keyof` requires a known type, but `{}` is not defined",
                            ident.name
                        )));
                    }
                }
                return;
            }
            // Intersection type alias: type X = A & B
            // Merge fields from all constituent struct types into one struct.
            if let ast::TypeKind::Intersection(ref members) = alias.kind {
                let merged = self.collect_intersection_fields(members, td, ctx);
                if !merged.is_empty() {
                    self.type_registry.register(td.name.name.clone(), merged);
                    return;
                }
            }
            // Non-utility type alias — register as empty struct
            // (the actual lowering will produce a type alias)
            self.type_registry
                .register(td.name.name.clone(), Vec::new());
            return;
        }

        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(td.type_params.as_ref());
        let fields: Vec<(String, Type)> = td
            .fields
            .iter()
            .map(|f| {
                let ty = resolve::resolve_type_annotation_with_generics(
                    &f.type_ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                // Optional fields (`name?: Type`) wrap the resolved type in Option<T>
                let ty = if f.optional {
                    Type::Option(Box::new(ty))
                } else {
                    ty
                };
                (f.name.name.clone(), ty)
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        self.type_registry.register(td.name.name.clone(), fields);
    }

    /// Collect merged fields from all constituent types of an intersection.
    ///
    /// For `type X = A & B`, looks up A and B in the type registry and merges
    /// their fields. Duplicate field names are kept once (first occurrence wins).
    /// Returns an empty vec if any member is not a struct type.
    pub(super) fn collect_intersection_fields(
        &self,
        members: &[ast::TypeAnnotation],
        td: &ast::TypeDef,
        ctx: &mut LoweringContext,
    ) -> Vec<(String, Type)> {
        let mut merged_fields: Vec<(String, Type)> = Vec::new();
        let mut seen_names: HashSet<String> = HashSet::new();
        let generic_names = collect_generic_param_names(td.type_params.as_ref());

        for member in members {
            if let ast::TypeKind::Named(ident) = &member.kind {
                if let Some(reg_type) = self.type_registry.lookup(&ident.name)
                    && let Some(fields) = reg_type.struct_fields()
                {
                    for (name, ty) in fields {
                        if seen_names.insert(name.clone()) {
                            merged_fields.push((name.clone(), ty.clone()));
                        }
                    }
                }
            } else {
                // Inline object types are not supported in type annotation position,
                // but handle Named types with generics gracefully.
                // Resolve the member type normally for non-named types
                let mut diags = Vec::new();
                let ty = resolve::resolve_type_annotation_with_generics(
                    member,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                for d in diags {
                    ctx.emit_diagnostic(d);
                }
                // If this resolves to a named type, try looking it up
                if let Type::Named(ref name) = ty
                    && let Some(reg_type) = self.type_registry.lookup(name)
                    && let Some(fields) = reg_type.struct_fields()
                {
                    for (name, ty) in fields {
                        if seen_names.insert(name.clone()) {
                            merged_fields.push((name.clone(), ty.clone()));
                        }
                    }
                }
            }
        }

        merged_fields
    }

    /// Lower an intersection type alias to a Rust struct with merged fields.
    ///
    /// `type Person = Named & Aged` where Named has `name: String` and Aged has
    /// `age: i32` produces `struct Person { pub name: String, pub age: i32 }`.
    #[allow(clippy::unused_self)] // method on Transform for API consistency with other lowering methods
    pub(super) fn lower_intersection_struct(
        &self,
        td: &ast::TypeDef,
        merged_fields: &[(String, Type)],
    ) -> RustStructDef {
        let type_params = lower_type_params(td.type_params.as_ref());
        let fields: Vec<RustFieldDef> = merged_fields
            .iter()
            .map(|(name, ty)| {
                let rust_ty = rustscript_typeck::bridge::type_to_rust_type(ty);
                RustFieldDef {
                    public: true,
                    name: name.clone(),
                    ty: rust_ty,
                    doc_comment: None,
                    span: Some(td.span),
                }
            })
            .collect();
        let field_types: Vec<&RustType> = fields.iter().map(|f| &f.ty).collect();
        let has_type_params = !type_params.is_empty();
        let derives = merge_derives(
            derive_inference::infer_struct_derives(&field_types, has_type_params),
            &td.derives,
        );
        RustStructDef {
            public: false,
            name: td.name.name.clone(),
            type_params,
            fields,
            derives,
            attributes: vec![],
            doc_comment: td.doc_comment.clone(),
            span: Some(td.span),
        }
    }

    /// Lower a type definition to a Rust struct.
    pub(super) fn lower_type_def(
        &self,
        td: &ast::TypeDef,
        ctx: &mut LoweringContext,
    ) -> RustStructDef {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(td.type_params.as_ref());
        let type_params = lower_type_params(td.type_params.as_ref());
        let fields: Vec<RustFieldDef> = td
            .fields
            .iter()
            .map(|f| {
                let ty = resolve::resolve_type_annotation_with_generics(
                    &f.type_ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                // Optional fields (`name?: Type`) wrap in Option<T>
                let ty = if f.optional {
                    Type::Option(Box::new(ty))
                } else {
                    ty
                };
                let rust_ty = rustscript_typeck::bridge::type_to_rust_type(&ty);
                RustFieldDef {
                    public: true,
                    name: f.name.name.clone(),
                    ty: rust_ty,
                    doc_comment: None,
                    span: Some(f.span),
                }
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        let field_types: Vec<&RustType> = fields.iter().map(|f| &f.ty).collect();
        let has_type_params = !type_params.is_empty();
        let derives = merge_derives(
            derive_inference::infer_struct_derives(&field_types, has_type_params),
            &td.derives,
        );
        RustStructDef {
            public: false,
            name: td.name.name.clone(),
            type_params,
            fields,
            derives,
            attributes: vec![],
            doc_comment: td.doc_comment.clone(),
            span: Some(td.span),
        }
    }

    /// Lower a `keyof T` type alias to a simple enum.
    ///
    /// `type UserKey = keyof User` where User has fields `name`, `age`, `email`
    /// → `enum UserKey { Name, Age, Email }` with standard simple-enum derives.
    pub(super) fn lower_keyof_type(
        &self,
        td: &ast::TypeDef,
        inner: &ast::TypeAnnotation,
        exported: bool,
        ctx: &mut LoweringContext,
    ) -> Option<RustEnumDef> {
        if let ast::TypeKind::Named(ref ident) = inner.kind {
            if let Some(reg_type) = self.type_registry.lookup(&ident.name) {
                let field_names: Vec<String> = match &reg_type.kind {
                    rustscript_typeck::registry::TypeDefKind::Struct(fields) => {
                        fields.iter().map(|(name, _)| name.clone()).collect()
                    }
                    rustscript_typeck::registry::TypeDefKind::Class { fields, .. } => {
                        fields.iter().map(|(name, _)| name.clone()).collect()
                    }
                    _ => {
                        ctx.emit_diagnostic(Diagnostic::error(format!(
                            "`keyof` requires a struct or class type, but `{}` is an enum or interface",
                            ident.name
                        )));
                        return None;
                    }
                };
                let variants: Vec<RustEnumVariant> = field_names
                    .iter()
                    .map(|name| RustEnumVariant {
                        name: capitalize_first(name),
                        fields: vec![],
                        tuple_types: vec![],
                        span: None,
                    })
                    .collect();
                let derives =
                    merge_derives(derive_inference::infer_enum_derives(&variants), &td.derives);
                Some(RustEnumDef {
                    public: exported,
                    name: td.name.name.clone(),
                    variants,
                    derives,
                    attributes: vec![],
                    doc_comment: td.doc_comment.clone(),
                    span: Some(td.span),
                })
            } else {
                ctx.emit_diagnostic(Diagnostic::error(format!(
                    "`keyof` requires a known type, but `{}` is not defined",
                    ident.name
                )));
                None
            }
        } else {
            ctx.emit_diagnostic(Diagnostic::error(
                "`keyof` requires a named type".to_owned(),
            ));
            None
        }
    }

    /// Lower a pure index signature type definition to a type alias.
    ///
    /// `type Config = { [key: string]: string }` → `type Config = HashMap<String, String>;`
    pub(super) fn lower_index_signature_type_alias(
        &self,
        td: &ast::TypeDef,
        ctx: &mut LoweringContext,
    ) -> RustTypeAlias {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(td.type_params.as_ref());
        let sig = td
            .index_signature
            .as_ref()
            .expect("called only when index_signature is Some");

        let key_ty = resolve::resolve_type_annotation_with_generics(
            &sig.key_type,
            &self.type_registry,
            &generic_names,
            &mut diags,
        );
        let value_ty = resolve::resolve_type_annotation_with_generics(
            &sig.value_type,
            &self.type_registry,
            &generic_names,
            &mut diags,
        );
        for d in diags {
            ctx.emit_diagnostic(d);
        }

        let key_rust = rustscript_typeck::bridge::type_to_rust_type(&key_ty);
        let value_rust = rustscript_typeck::bridge::type_to_rust_type(&value_ty);

        RustTypeAlias {
            public: false,
            name: td.name.name.clone(),
            ty: RustType::Generic(
                Box::new(RustType::Named("HashMap".to_owned())),
                vec![key_rust, value_rust],
            ),
            span: Some(td.span),
        }
    }

    /// Resolve a type alias body, handling variadic tuple spreads.
    ///
    /// If the alias body is a tuple type containing `TupleSpread` elements,
    /// resolves each spread by looking up previously-lowered type aliases and
    /// flattening their tuple elements. Falls back to normal type resolution
    /// for non-spread cases.
    pub(super) fn resolve_type_alias_body(
        &self,
        alias: &ast::TypeAnnotation,
        td: &ast::TypeDef,
        ctx: &mut LoweringContext,
    ) -> RustType {
        // Check if this is a tuple type with spread elements
        if let ast::TypeKind::Tuple(elements) = &alias.kind {
            let has_spread = elements
                .iter()
                .any(|e| matches!(e.kind, ast::TypeKind::TupleSpread(_)));
            if has_spread {
                return self.resolve_variadic_tuple(elements, td, ctx);
            }
        }

        // Normal type alias resolution
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(td.type_params.as_ref());
        let ty = resolve::resolve_type_annotation_with_generics(
            alias,
            &self.type_registry,
            &generic_names,
            &mut diags,
        );
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        rustscript_typeck::bridge::type_to_rust_type(&ty)
    }

    /// Resolve a variadic tuple type by flattening spread elements.
    ///
    /// For each element in the tuple:
    /// - Plain types are resolved normally
    /// - `...T` spreads look up `T` in `type_alias_types` and flatten
    fn resolve_variadic_tuple(
        &self,
        elements: &[ast::TypeAnnotation],
        td: &ast::TypeDef,
        ctx: &mut LoweringContext,
    ) -> RustType {
        let mut result_types = Vec::new();
        let generic_names = collect_generic_param_names(td.type_params.as_ref());

        for element in elements {
            if let ast::TypeKind::TupleSpread(inner) = &element.kind {
                // First, check if it's a named type alias we've already resolved
                let resolved = if let ast::TypeKind::Named(ident) = &inner.kind {
                    if let Some(alias_ty) = self.type_alias_types.get(&ident.name) {
                        Some(alias_ty.clone())
                    } else {
                        // Not a known type alias — try resolving normally
                        let mut diags = Vec::new();
                        let ty = resolve::resolve_type_annotation_with_generics(
                            inner,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        for d in diags {
                            ctx.emit_diagnostic(d);
                        }
                        Some(rustscript_typeck::bridge::type_to_rust_type(&ty))
                    }
                } else {
                    // Spread of a non-named type (e.g., inline tuple) — resolve it
                    let mut diags = Vec::new();
                    let ty = resolve::resolve_type_annotation_with_generics(
                        inner,
                        &self.type_registry,
                        &generic_names,
                        &mut diags,
                    );
                    for d in diags {
                        ctx.emit_diagnostic(d);
                    }
                    Some(rustscript_typeck::bridge::type_to_rust_type(&ty))
                };

                if let Some(RustType::Tuple(inner_types)) = resolved {
                    result_types.extend(inner_types);
                } else {
                    ctx.emit_diagnostic(Diagnostic::error(
                        "spread in tuple type must refer to a tuple type".to_owned(),
                    ));
                }
            } else {
                // Normal tuple element
                let mut diags = Vec::new();
                let ty = resolve::resolve_type_annotation_with_generics(
                    element,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                for d in diags {
                    ctx.emit_diagnostic(d);
                }
                result_types.push(rustscript_typeck::bridge::type_to_rust_type(&ty));
            }
        }

        RustType::Tuple(result_types)
    }

    /// Check whether a type alias annotation is a built-in utility type application.
    ///
    /// Returns the utility type name if recognized: `Partial`, `Required`,
    /// `Readonly`, `Record`, `Pick`, `Omit`, `ReturnType`, or `Parameters`.
    pub(super) fn identify_utility_type(ann: &ast::TypeAnnotation) -> Option<&str> {
        if let ast::TypeKind::Generic(ident, _) = &ann.kind {
            match ident.name.as_str() {
                "Partial" | "Required" | "Readonly" | "Record" | "Pick" | "Omit" | "ReturnType"
                | "Parameters" => Some(&ident.name),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Extract string literal field names from a type annotation.
    ///
    /// Handles both a single string literal and a union of string literals
    /// (e.g., `"name" | "age"`). Returns the field names as a `Vec<String>`.
    pub(super) fn extract_string_literal_fields(ann: &ast::TypeAnnotation) -> Vec<String> {
        match &ann.kind {
            ast::TypeKind::StringLiteral(value) => vec![value.clone()],
            ast::TypeKind::Union(members) => members
                .iter()
                .filter_map(|m| {
                    if let ast::TypeKind::StringLiteral(value) = &m.kind {
                        Some(value.clone())
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Register a utility type application in the type registry during the pre-pass.
    ///
    /// Resolves `Partial<User>`, `Pick<User, "name">`, etc. by looking up the
    /// source type's fields and generating a transformed field list.
    #[allow(clippy::too_many_lines)]
    // Each utility type variant has distinct validation and transformation logic; splitting would fragment the dispatch
    pub(super) fn register_utility_type_def(
        &mut self,
        td: &ast::TypeDef,
        ann: &ast::TypeAnnotation,
        ctx: &mut LoweringContext,
    ) {
        let ast::TypeKind::Generic(utility_ident, args) = &ann.kind else {
            return;
        };
        let utility_name = utility_ident.name.as_str();

        // Record<K, V> does not reference a source type — register as empty struct
        if utility_name == "Record" {
            self.type_registry
                .register(td.name.name.clone(), Vec::new());
            return;
        }

        // ReturnType<T> and Parameters<T> extract parts of function types.
        // They don't reference struct fields — register as empty and resolve during lowering.
        if utility_name == "ReturnType" || utility_name == "Parameters" {
            self.type_registry
                .register(td.name.name.clone(), Vec::new());
            return;
        }

        // All other utility types require a source type as the first argument
        let source_name = args.first().and_then(|a| {
            if let ast::TypeKind::Named(ident) = &a.kind {
                Some(ident.name.clone())
            } else {
                None
            }
        });

        let Some(source_name) = source_name else {
            ctx.emit_diagnostic(Diagnostic::error(format!(
                "utility type `{utility_name}` requires a type argument"
            )));
            return;
        };

        // Look up source type fields
        let source_fields = self
            .type_registry
            .lookup(&source_name)
            .and_then(|td| td.struct_fields())
            .map(<[(String, Type)]>::to_vec);

        let Some(source_fields) = source_fields else {
            ctx.emit_diagnostic(Diagnostic::error(format!(
                "unknown type `{source_name}` in `{utility_name}<{source_name}>`"
            )));
            return;
        };

        let new_fields: Vec<(String, Type)> = match utility_name {
            "Partial" => source_fields
                .iter()
                .map(|(name, ty)| (name.clone(), Type::Option(Box::new(ty.clone()))))
                .collect(),
            "Required" => source_fields
                .iter()
                .map(|(name, ty)| {
                    let unwrapped = if let Type::Option(inner) = ty {
                        (**inner).clone()
                    } else {
                        ty.clone()
                    };
                    (name.clone(), unwrapped)
                })
                .collect(),
            "Pick" => {
                let field_names = args
                    .get(1)
                    .map_or_else(Vec::new, Self::extract_string_literal_fields);
                if field_names.is_empty() {
                    ctx.emit_diagnostic(Diagnostic::error(format!(
                        "`Pick<{source_name}, ...>` requires string literal field names"
                    )));
                }
                // Validate field names exist
                for name in &field_names {
                    if !source_fields.iter().any(|(n, _)| n == name) {
                        ctx.emit_diagnostic(Diagnostic::error(format!(
                            "unknown field `{name}` in `Pick<{source_name}, \"{name}\">`"
                        )));
                    }
                }
                source_fields
                    .iter()
                    .filter(|(name, _)| field_names.contains(name))
                    .cloned()
                    .collect()
            }
            "Omit" => {
                let field_names = args
                    .get(1)
                    .map_or_else(Vec::new, Self::extract_string_literal_fields);
                if field_names.is_empty() {
                    ctx.emit_diagnostic(Diagnostic::error(format!(
                        "`Omit<{source_name}, ...>` requires string literal field names"
                    )));
                }
                // Validate field names exist
                for name in &field_names {
                    if !source_fields.iter().any(|(n, _)| n == name) {
                        ctx.emit_diagnostic(Diagnostic::error(format!(
                            "unknown field `{name}` in `Omit<{source_name}, \"{name}\">`"
                        )));
                    }
                }
                source_fields
                    .iter()
                    .filter(|(name, _)| !field_names.contains(name))
                    .cloned()
                    .collect()
            }
            _ => source_fields.clone(),
        };

        self.type_registry
            .register(td.name.name.clone(), new_fields);
    }

    /// Lower a utility type application to a Rust struct or type alias.
    ///
    /// Handles `Partial<T>`, `Required<T>`, `Readonly<T>`, `Record<K, V>`,
    /// `Pick<T, K>`, `Omit<T, K>`, `ReturnType<T>`, and `Parameters<T>`.
    #[allow(clippy::too_many_lines)]
    // Each utility type variant produces a different IR shape; splitting the dispatch would obscure the grammar
    pub(super) fn lower_utility_type(
        &self,
        td: &ast::TypeDef,
        ann: &ast::TypeAnnotation,
        ctx: &mut LoweringContext,
    ) -> RustItem {
        let ast::TypeKind::Generic(utility_ident, args) = &ann.kind else {
            unreachable!("called only for utility type applications");
        };
        let utility_name = utility_ident.name.as_str();

        // Record<K, V> → type alias to HashMap<K, V>
        if utility_name == "Record" {
            let mut diags = Vec::new();
            let generic_names = collect_generic_param_names(td.type_params.as_ref());
            let key_ty = args.first().map_or(Type::String, |a| {
                resolve::resolve_type_annotation_with_generics(
                    a,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                )
            });
            let value_ty = args.get(1).map_or(Type::Unit, |a| {
                resolve::resolve_type_annotation_with_generics(
                    a,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                )
            });
            for d in diags {
                ctx.emit_diagnostic(d);
            }
            let key_rust = rustscript_typeck::bridge::type_to_rust_type(&key_ty);
            let value_rust = rustscript_typeck::bridge::type_to_rust_type(&value_ty);

            return RustItem::TypeAlias(RustTypeAlias {
                public: false,
                name: td.name.name.clone(),
                ty: RustType::Generic(
                    Box::new(RustType::Named("HashMap".to_owned())),
                    vec![key_rust, value_rust],
                ),
                span: Some(td.span),
            });
        }

        // ReturnType<T> → extract function return type
        // Parameters<T> → extract function parameter types as tuple
        if utility_name == "ReturnType" || utility_name == "Parameters" {
            let mut diags = Vec::new();
            let generic_names = collect_generic_param_names(td.type_params.as_ref());
            let arg_ty = args.first().map_or(Type::Error, |a| {
                resolve::resolve_type_annotation_with_generics(
                    a,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                )
            });
            for d in diags {
                ctx.emit_diagnostic(d);
            }

            let result_ty = match (&arg_ty, utility_name) {
                (Type::Function(_, ret), "ReturnType") => (**ret).clone(),
                (Type::Function(params, _), "Parameters") => Type::Tuple(params.clone()),
                _ => {
                    ctx.emit_diagnostic(Diagnostic::error(format!(
                        "`{utility_name}` requires a function type argument"
                    )));
                    Type::Error
                }
            };

            let rust_ty = rustscript_typeck::bridge::type_to_rust_type(&result_ty);
            return RustItem::TypeAlias(RustTypeAlias {
                public: false,
                name: td.name.name.clone(),
                ty: rust_ty,
                span: Some(td.span),
            });
        }

        // All other utility types generate structs
        let source_name = args.first().and_then(|a| {
            if let ast::TypeKind::Named(ident) = &a.kind {
                Some(ident.name.clone())
            } else {
                None
            }
        });

        let source_name = source_name.unwrap_or_default();

        // Look up the registered fields (already computed in pre-pass)
        let registered_fields = self
            .type_registry
            .lookup(&td.name.name)
            .and_then(|td| td.struct_fields())
            .map(<[(String, Type)]>::to_vec)
            .unwrap_or_default();

        let fields: Vec<RustFieldDef> = registered_fields
            .iter()
            .map(|(name, ty)| {
                let rust_ty = rustscript_typeck::bridge::type_to_rust_type(ty);
                RustFieldDef {
                    public: true,
                    name: name.clone(),
                    ty: rust_ty,
                    doc_comment: None,
                    span: Some(td.span),
                }
            })
            .collect();

        let field_types: Vec<&RustType> = fields.iter().map(|f| &f.ty).collect();
        let type_params = lower_type_params(td.type_params.as_ref());
        let has_type_params = !type_params.is_empty();

        // Propagate derives from source type
        let source_derives = self
            .type_registry
            .lookup(&source_name)
            .and_then(|td| td.struct_fields())
            .map(|source_fields| {
                let source_field_types: Vec<RustType> = source_fields
                    .iter()
                    .map(|(_, ty)| rustscript_typeck::bridge::type_to_rust_type(ty))
                    .collect();
                let refs: Vec<&RustType> = source_field_types.iter().collect();
                derive_inference::infer_struct_derives(&refs, has_type_params)
            });

        let auto_derives = source_derives.unwrap_or_else(|| {
            derive_inference::infer_struct_derives(&field_types, has_type_params)
        });
        let derives = merge_derives(auto_derives, &td.derives);

        RustItem::Struct(RustStructDef {
            public: false,
            name: td.name.name.clone(),
            type_params,
            fields,
            derives,
            attributes: vec![],
            doc_comment: td.doc_comment.clone(),
            span: Some(td.span),
        })
    }

    /// Register a mapped type alias in the type registry during the pre-pass.
    ///
    /// Resolves `{ [K in keyof T]: V }` by looking up T's fields and applying
    /// the value type transformation to each field.
    pub(super) fn register_mapped_type_def(
        &mut self,
        td: &ast::TypeDef,
        alias: &ast::TypeAnnotation,
        ctx: &mut LoweringContext,
    ) {
        let fields = self.resolve_mapped_type_fields(td, alias, ctx);
        self.type_registry.register(td.name.name.clone(), fields);
    }

    /// Resolve the fields produced by a mapped type.
    ///
    /// Given `{ [K in keyof T]: ValueType }`, looks up T's fields and applies
    /// the value type transformation. Handles `T[K]` index access by substituting
    /// the concrete field type for each key.
    #[allow(clippy::too_many_lines)]
    fn resolve_mapped_type_fields(
        &self,
        td: &ast::TypeDef,
        alias: &ast::TypeAnnotation,
        ctx: &mut LoweringContext,
    ) -> Vec<(String, Type)> {
        let ast::TypeKind::MappedType {
            type_param,
            constraint,
            value_type,
            optional,
            ..
        } = &alias.kind
        else {
            return Vec::new();
        };

        // Resolve the constraint to get source field names.
        // Currently supports `keyof T` where T is a registered struct/class.
        let source_type_name = if let ast::TypeKind::KeyOf(inner) = &constraint.kind {
            if let ast::TypeKind::Named(ref ident) = inner.kind {
                Some(ident.name.clone())
            } else {
                ctx.emit_diagnostic(Diagnostic::error(
                    "mapped type constraint `keyof` requires a named type".to_owned(),
                ));
                None
            }
        } else {
            ctx.emit_diagnostic(Diagnostic::error(
                "mapped type constraint must be `keyof T`; example: `{ [K in keyof MyType]: ... }`",
            ));
            None
        };

        let Some(source_name) = source_type_name else {
            return Vec::new();
        };

        // Look up source type fields
        let source_fields = self
            .type_registry
            .lookup(&source_name)
            .and_then(|td| td.struct_fields())
            .map(<[(String, Type)]>::to_vec);

        let Some(source_fields) = source_fields else {
            ctx.emit_diagnostic(Diagnostic::error(format!(
                "unknown type `{source_name}` in mapped type"
            )));
            return Vec::new();
        };

        let generic_names = collect_generic_param_names(td.type_params.as_ref());
        let mut diags = Vec::new();

        // For each field in the source type, resolve the value type
        let new_fields: Vec<(String, Type)> = source_fields
            .iter()
            .map(|(field_name, field_type)| {
                let resolved_value = self.resolve_mapped_value_type(
                    value_type,
                    &type_param.name,
                    field_type,
                    &source_name,
                    &generic_names,
                    &mut diags,
                );

                // Apply optional modifier
                let final_type = match optional {
                    Some(ast::MappedModifier::Add) => {
                        // Make optional: wrap in Option if not already
                        if matches!(resolved_value, Type::Option(_)) {
                            resolved_value
                        } else {
                            Type::Option(Box::new(resolved_value))
                        }
                    }
                    Some(ast::MappedModifier::Remove) => {
                        // Remove optional: unwrap Option if present
                        if let Type::Option(inner) = resolved_value {
                            (*inner).clone()
                        } else {
                            resolved_value
                        }
                    }
                    None => resolved_value,
                };

                (field_name.clone(), final_type)
            })
            .collect();

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        new_fields
    }

    /// Resolve the value type of a mapped type for a specific field.
    ///
    /// Substitutes `T[K]` with the concrete field type, resolves unions like `V | null`
    /// to `Option<V>`, etc.
    fn resolve_mapped_value_type(
        &self,
        value_type: &ast::TypeAnnotation,
        type_param_name: &str,
        field_type: &Type,
        source_type_name: &str,
        generic_names: &[String],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Type {
        match &value_type.kind {
            // T[K] — the original field type
            ast::TypeKind::IndexAccess(obj, idx) => {
                if let ast::TypeKind::Named(ref obj_ident) = obj.kind
                    && let ast::TypeKind::Named(ref idx_ident) = idx.kind
                    && idx_ident.name == type_param_name
                {
                    // Check if the object type matches the source type
                    if obj_ident.name == source_type_name
                        || self.type_registry.lookup(&obj_ident.name).is_none()
                    {
                        return field_type.clone();
                    }
                }
                // Try generic resolution
                resolve::resolve_type_annotation_with_generics(
                    value_type,
                    &self.type_registry,
                    generic_names,
                    diagnostics,
                )
            }
            // Union type: T[K] | null → Option<field_type>
            ast::TypeKind::Union(members) => {
                let mut resolved_members = Vec::new();
                let mut has_null = false;

                for member in members {
                    if let ast::TypeKind::Named(ident) = &member.kind
                        && ident.name == "null"
                    {
                        has_null = true;
                        continue;
                    }
                    resolved_members.push(self.resolve_mapped_value_type(
                        member,
                        type_param_name,
                        field_type,
                        source_type_name,
                        generic_names,
                        diagnostics,
                    ));
                }

                let inner = if resolved_members.len() == 1 {
                    resolved_members.into_iter().next().unwrap_or(Type::Unit)
                } else if resolved_members.is_empty() {
                    Type::Unit
                } else {
                    Type::Union(resolved_members)
                };

                if has_null {
                    Type::Option(Box::new(inner))
                } else {
                    inner
                }
            }
            // Named type that matches the key type param — the field name (string literal)
            ast::TypeKind::Named(ident) if ident.name == type_param_name => {
                // K resolves to the field name — this is a string literal type
                // In practice this means the field type stays the same
                Type::String
            }
            // Any other type — resolve normally
            _ => resolve::resolve_type_annotation_with_generics(
                value_type,
                &self.type_registry,
                generic_names,
                diagnostics,
            ),
        }
    }

    /// Lower a mapped type alias to a Rust struct definition.
    ///
    /// Generates a struct with fields derived from iterating over the source type's
    /// fields and applying the mapped type's value transformation.
    pub(super) fn lower_mapped_type(
        &self,
        td: &ast::TypeDef,
        alias: &ast::TypeAnnotation,
        ctx: &mut LoweringContext,
    ) -> RustStructDef {
        // Look up the registered fields (already computed in pre-pass)
        let registered_fields = self
            .type_registry
            .lookup(&td.name.name)
            .and_then(|td| td.struct_fields())
            .map(<[(String, Type)]>::to_vec)
            .unwrap_or_default();

        let fields: Vec<RustFieldDef> = registered_fields
            .iter()
            .map(|(name, ty)| {
                let rust_ty = rustscript_typeck::bridge::type_to_rust_type(ty);
                RustFieldDef {
                    public: true,
                    name: name.clone(),
                    ty: rust_ty,
                    doc_comment: None,
                    span: Some(td.span),
                }
            })
            .collect();

        let field_types: Vec<&RustType> = fields.iter().map(|f| &f.ty).collect();
        let type_params = lower_type_params(td.type_params.as_ref());
        let has_type_params = !type_params.is_empty();

        // Infer derives from the source type if possible
        let source_name = Self::extract_mapped_source_name(alias);
        let source_derives = source_name.and_then(|name| {
            self.type_registry
                .lookup(&name)
                .and_then(|td| td.struct_fields())
                .map(|source_fields| {
                    let source_field_types: Vec<RustType> = source_fields
                        .iter()
                        .map(|(_, ty)| rustscript_typeck::bridge::type_to_rust_type(ty))
                        .collect();
                    let refs: Vec<&RustType> = source_field_types.iter().collect();
                    derive_inference::infer_struct_derives(&refs, has_type_params)
                })
        });

        let auto_derives = source_derives.unwrap_or_else(|| {
            derive_inference::infer_struct_derives(&field_types, has_type_params)
        });
        let derives = merge_derives(auto_derives, &td.derives);

        let _ = ctx; // ctx available for future diagnostics

        RustStructDef {
            public: false,
            name: td.name.name.clone(),
            type_params,
            fields,
            derives,
            attributes: vec![],
            doc_comment: td.doc_comment.clone(),
            span: Some(td.span),
        }
    }

    /// Extract the source type name from a mapped type's constraint.
    ///
    /// For `{ [K in keyof T]: V }`, returns `Some("T")`.
    fn extract_mapped_source_name(alias: &ast::TypeAnnotation) -> Option<String> {
        if let ast::TypeKind::MappedType { constraint, .. } = &alias.kind
            && let ast::TypeKind::KeyOf(inner) = &constraint.kind
            && let ast::TypeKind::Named(ref ident) = inner.kind
        {
            Some(ident.name.clone())
        } else {
            None
        }
    }

    /// Register an enum definition in the type registry during the pre-pass.
    pub(super) fn register_enum_def(
        &mut self,
        ed: &ast::EnumDef,
        module_items: &[ast::Item],
        ctx: &mut LoweringContext,
    ) {
        // Determine if simple or data enum
        let is_data = ed.variants.iter().any(|v| {
            matches!(
                v,
                ast::EnumVariant::Data { .. } | ast::EnumVariant::TypeRef { .. }
            )
        });

        if is_data {
            let mut diags = Vec::new();
            let variants: Vec<(String, Vec<(String, rustscript_typeck::types::Type)>)> = ed
                .variants
                .iter()
                .filter_map(|v| match v {
                    ast::EnumVariant::Data { name, fields, .. } => {
                        let field_types: Vec<(String, rustscript_typeck::types::Type)> = fields
                            .iter()
                            .map(|f| {
                                let ty = resolve::resolve_type_annotation_with_generics(
                                    &f.type_ann,
                                    &self.type_registry,
                                    &[],
                                    &mut diags,
                                );
                                (f.name.name.clone(), ty)
                            })
                            .collect();
                        Some((name.name.clone(), field_types))
                    }
                    ast::EnumVariant::TypeRef { type_name, .. } => {
                        if let Some((disc_value, all_fields)) =
                            resolve_type_ref_variant(&type_name.name, module_items)
                        {
                            let variant_name = capitalize_first(&disc_value);
                            let field_types: Vec<(String, rustscript_typeck::types::Type)> =
                                all_fields
                                    .iter()
                                    .filter(|f| f.name.name != "kind")
                                    .map(|f| {
                                        let ty = resolve::resolve_type_annotation_with_generics(
                                            &f.type_ann,
                                            &self.type_registry,
                                            &[],
                                            &mut diags,
                                        );
                                        (f.name.name.clone(), ty)
                                    })
                                    .collect();
                            Some((variant_name, field_types))
                        } else {
                            ctx.emit_diagnostic(Diagnostic::error(format!(
                                "type `{}` not found or missing `kind` discriminant field",
                                type_name.name
                            )));
                            None
                        }
                    }
                    ast::EnumVariant::Simple(..) => None,
                })
                .collect();
            for d in diags {
                ctx.emit_diagnostic(d);
            }
            self.type_registry
                .register_data_enum(ed.name.name.clone(), variants);
        } else {
            let variants: Vec<String> = ed
                .variants
                .iter()
                .filter_map(|v| match v {
                    ast::EnumVariant::Simple(ident, _) => Some(ident.name.clone()),
                    ast::EnumVariant::Data { .. } | ast::EnumVariant::TypeRef { .. } => None,
                })
                .collect();
            self.type_registry
                .register_simple_enum(ed.name.name.clone(), variants);
        }
    }

    /// Register an interface definition in the type registry during the pre-pass.
    pub(super) fn register_interface_def(
        &mut self,
        iface: &ast::InterfaceDef,
        ctx: &mut LoweringContext,
    ) {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(iface.type_params.as_ref());
        let methods: Vec<rustscript_typeck::registry::InterfaceMethodSig> = iface
            .methods
            .iter()
            .map(|m| {
                let param_types: Vec<(String, rustscript_typeck::types::Type)> = m
                    .params
                    .iter()
                    .map(|p| {
                        let ty = resolve::resolve_type_annotation_with_generics(
                            &p.type_ann,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        (p.name.name.clone(), ty)
                    })
                    .collect();
                let return_type = m.return_type.as_ref().and_then(|rt| {
                    rt.type_ann.as_ref().map(|ann| {
                        resolve::resolve_type_annotation_with_generics(
                            ann,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        )
                    })
                });
                rustscript_typeck::registry::InterfaceMethodSig {
                    name: m.name.name.clone(),
                    param_types,
                    return_type,
                }
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        self.type_registry
            .register_interface(iface.name.name.clone(), methods);
    }

    /// Register an abstract class as an interface in the type registry.
    ///
    /// This enables concrete classes to `extends` the abstract class, which is
    /// resolved to a trait impl during lowering.
    pub(super) fn register_abstract_class_as_interface(&mut self, cls: &ast::ClassDef) {
        let generic_names = collect_generic_param_names(cls.type_params.as_ref());
        let mut diags = Vec::new();

        let methods: Vec<rustscript_typeck::registry::InterfaceMethodSig> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ast::ClassMember::Method(method) => {
                    let param_types: Vec<(String, rustscript_typeck::types::Type)> = method
                        .params
                        .iter()
                        .map(|p| {
                            let ty = resolve::resolve_type_annotation_with_generics(
                                &p.type_ann,
                                &self.type_registry,
                                &generic_names,
                                &mut diags,
                            );
                            (p.name.name.clone(), ty)
                        })
                        .collect();
                    let return_type = method.return_type.as_ref().and_then(|rt| {
                        rt.type_ann.as_ref().map(|ann| {
                            resolve::resolve_type_annotation_with_generics(
                                ann,
                                &self.type_registry,
                                &generic_names,
                                &mut diags,
                            )
                        })
                    });
                    Some(rustscript_typeck::registry::InterfaceMethodSig {
                        name: method.name.name.clone(),
                        param_types,
                        return_type,
                    })
                }
                _ => None,
            })
            .collect();

        self.type_registry
            .register_interface(cls.name.name.clone(), methods);
    }

    /// Register a concrete extended class as an interface in the type registry.
    ///
    /// When a concrete class is used as a base class (another class `extends` it),
    /// its instance methods are registered as interface methods. This enables the
    /// derived class to generate `impl {Name}Trait for DerivedClass` during lowering.
    pub(super) fn register_concrete_class_as_interface(
        &mut self,
        cls: &ast::ClassDef,
        ctx: &mut LoweringContext,
    ) {
        let generic_names = collect_generic_param_names(cls.type_params.as_ref());
        let mut diags = Vec::new();

        let methods: Vec<rustscript_typeck::registry::InterfaceMethodSig> = cls
            .members
            .iter()
            .filter_map(|m| match m {
                ast::ClassMember::Method(method) if !method.is_static => {
                    let param_types: Vec<(String, rustscript_typeck::types::Type)> = method
                        .params
                        .iter()
                        .map(|p| {
                            let ty = resolve::resolve_type_annotation_with_generics(
                                &p.type_ann,
                                &self.type_registry,
                                &generic_names,
                                &mut diags,
                            );
                            (p.name.name.clone(), ty)
                        })
                        .collect();
                    let return_type = method.return_type.as_ref().and_then(|rt| {
                        rt.type_ann.as_ref().map(|ann| {
                            resolve::resolve_type_annotation_with_generics(
                                ann,
                                &self.type_registry,
                                &generic_names,
                                &mut diags,
                            )
                        })
                    });
                    Some(rustscript_typeck::registry::InterfaceMethodSig {
                        name: method.name.name.clone(),
                        param_types,
                        return_type,
                    })
                }
                _ => None,
            })
            .collect();

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        // Set methods on the existing class registration — does not overwrite
        // the Class kind, so get_class_fields() still works.
        self.type_registry
            .set_class_methods(&cls.name.name, methods);
    }

    /// Lower an interface definition to a Rust trait.
    pub(super) fn lower_interface_def(
        &self,
        iface: &ast::InterfaceDef,
        ctx: &mut LoweringContext,
    ) -> RustTraitDef {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(iface.type_params.as_ref());
        let type_params = lower_type_params(iface.type_params.as_ref());

        let methods = iface
            .methods
            .iter()
            .map(|m| {
                let params: Vec<super::RustParam> = m
                    .params
                    .iter()
                    .map(|p| {
                        let ty = resolve::resolve_type_annotation_with_generics(
                            &p.type_ann,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        let rust_ty = rustscript_typeck::bridge::type_to_rust_type(&ty);
                        super::RustParam {
                            name: p.name.name.clone(),
                            ty: rust_ty,
                            mode: ParamMode::Owned,
                            span: Some(p.span),
                        }
                    })
                    .collect();

                let return_type = m.return_type.as_ref().and_then(|rt| {
                    rt.type_ann.as_ref().map(|ann| {
                        // Handle `Self` return type specially
                        if let ast::TypeKind::Named(ident) = &ann.kind
                            && ident.name == "Self"
                        {
                            return RustType::SelfType;
                        }
                        let ty = resolve::resolve_type_annotation_with_generics(
                            ann,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        rustscript_typeck::bridge::type_to_rust_type(&ty)
                    })
                });

                RustTraitMethod {
                    name: m.name.name.clone(),
                    params,
                    return_type,
                    has_self: true, // All interface methods take &self
                    default_body: None,
                    doc_comment: None,
                    span: Some(m.span),
                }
            })
            .collect();

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        RustTraitDef {
            public: false,
            name: iface.name.name.clone(),
            type_params,
            methods,
            doc_comment: iface.doc_comment.clone(),
            span: Some(iface.span),
        }
    }

    /// Lower an enum definition to a Rust enum.
    pub(super) fn lower_enum_def(
        &self,
        ed: &ast::EnumDef,
        module_items: &[ast::Item],
        ctx: &mut LoweringContext,
    ) -> RustEnumDef {
        let mut diags = Vec::new();
        let variants: Vec<RustEnumVariant> = ed
            .variants
            .iter()
            .filter_map(|v| match v {
                ast::EnumVariant::Simple(ident, span) => Some(RustEnumVariant {
                    name: ident.name.clone(),
                    fields: vec![],
                    tuple_types: vec![],
                    span: Some(*span),
                }),
                ast::EnumVariant::Data {
                    name, fields, span, ..
                } => {
                    let rust_fields =
                        Self::lower_variant_fields(fields, &self.type_registry, &mut diags);
                    Some(RustEnumVariant {
                        name: name.name.clone(),
                        fields: rust_fields,
                        tuple_types: vec![],
                        span: Some(*span),
                    })
                }
                ast::EnumVariant::TypeRef { type_name, span } => {
                    if let Some((disc_value, all_fields)) =
                        resolve_type_ref_variant(&type_name.name, module_items)
                    {
                        let variant_name = capitalize_first(&disc_value);
                        let data_fields: Vec<&ast::FieldDef> = all_fields
                            .iter()
                            .filter(|f| f.name.name != "kind")
                            .collect();
                        let rust_fields = Self::lower_variant_fields(
                            &data_fields.iter().copied().cloned().collect::<Vec<_>>(),
                            &self.type_registry,
                            &mut diags,
                        );
                        Some(RustEnumVariant {
                            name: variant_name,
                            fields: rust_fields,
                            tuple_types: vec![],
                            span: Some(*span),
                        })
                    } else {
                        ctx.emit_diagnostic(Diagnostic::error(format!(
                            "type `{}` not found or missing `kind` discriminant field",
                            type_name.name
                        )));
                        None
                    }
                }
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        let derives = merge_derives(derive_inference::infer_enum_derives(&variants), &ed.derives);
        RustEnumDef {
            public: false,
            name: ed.name.name.clone(),
            variants,
            derives,
            attributes: vec![],
            doc_comment: ed.doc_comment.clone(),
            span: Some(ed.span),
        }
    }

    /// Lower a slice of `FieldDef` into `RustFieldDef` for enum variant fields.
    fn lower_variant_fields(
        fields: &[ast::FieldDef],
        type_registry: &rustscript_typeck::registry::TypeRegistry,
        diags: &mut Vec<Diagnostic>,
    ) -> Vec<RustFieldDef> {
        fields
            .iter()
            .map(|f| {
                let ty = resolve::resolve_type_annotation_with_generics(
                    &f.type_ann,
                    type_registry,
                    &[],
                    diags,
                );
                // Optional fields (`name?: Type`) wrap in Option<T>
                let ty = if f.optional {
                    Type::Option(Box::new(ty))
                } else {
                    ty
                };
                let rust_ty = rustscript_typeck::bridge::type_to_rust_type(&ty);
                RustFieldDef {
                    public: true,
                    name: f.name.name.clone(),
                    ty: rust_ty,
                    doc_comment: None,
                    span: Some(f.span),
                }
            })
            .collect()
    }
}

/// Collect generic parameter names from an optional `TypeParams`.
///
/// Returns a `Vec<String>` of type parameter names (e.g., `["T", "U"]`).
/// Used to set up the generic scope during lowering.
pub(super) fn collect_generic_param_names(type_params: Option<&ast::TypeParams>) -> Vec<String> {
    match type_params {
        Some(tp) => tp.params.iter().map(|p| p.name.name.clone()).collect(),
        None => Vec::new(),
    }
}

/// Lower AST type parameters to Rust IR type parameters.
///
/// Maps `T extends Bound` to `RustTypeParam { name: "T", bounds: vec!["Bound"] }`.
pub(super) fn lower_type_params(type_params: Option<&ast::TypeParams>) -> Vec<RustTypeParam> {
    match type_params {
        Some(tp) => tp
            .params
            .iter()
            .map(|p| {
                let bounds = p
                    .constraint
                    .as_ref()
                    .map(|c| match &c.kind {
                        ast::TypeKind::Named(ident) | ast::TypeKind::Generic(ident, _) => {
                            vec![ident.name.clone()]
                        }
                        ast::TypeKind::Intersection(members) => members
                            .iter()
                            .filter_map(|m| match &m.kind {
                                ast::TypeKind::Named(ident) => Some(ident.name.clone()),
                                _ => None,
                            })
                            .collect(),
                        ast::TypeKind::Void
                        | ast::TypeKind::Never
                        | ast::TypeKind::Unknown
                        | ast::TypeKind::Union(_)
                        | ast::TypeKind::Function(_, _)
                        | ast::TypeKind::Inferred
                        | ast::TypeKind::Shared(_)
                        | ast::TypeKind::Tuple(_)
                        | ast::TypeKind::IndexSignature(_)
                        | ast::TypeKind::StringLiteral(_)
                        | ast::TypeKind::KeyOf(_)
                        | ast::TypeKind::TypeOf(_)
                        | ast::TypeKind::Conditional { .. }
                        | ast::TypeKind::Infer(_)
                        | ast::TypeKind::TupleSpread(_)
                        | ast::TypeKind::TypeGuard { .. }
                        | ast::TypeKind::Asserts { .. }
                        | ast::TypeKind::Readonly(_)
                        | ast::TypeKind::TemplateLiteralType { .. }
                        | ast::TypeKind::MappedType { .. }
                        | ast::TypeKind::IndexAccess(_, _) => vec![],
                    })
                    .unwrap_or_default();
                RustTypeParam {
                    name: p.name.name.clone(),
                    bounds,
                }
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Resolve a `TypeRef` enum variant by finding the referenced `TypeDef` in the module.
///
/// Looks up the type by name, extracts the `kind` discriminant field (which must have a
/// `StringLiteral` type annotation), and returns the discriminant value along with the
/// remaining data fields (excluding `kind`).
///
/// Returns `None` if the type cannot be found or lacks a valid `kind` discriminant.
fn resolve_type_ref_variant<'a>(
    type_name: &str,
    items: &'a [ast::Item],
) -> Option<(String, &'a [ast::FieldDef])> {
    // Find the TypeDef with matching name
    for item in items {
        if let ast::ItemKind::TypeDef(td) = &item.kind
            && td.name.name == type_name
        {
            // Find the `kind` field and extract the string literal discriminant
            let kind_field = td.fields.iter().find(|f| f.name.name == "kind")?;
            if let ast::TypeKind::StringLiteral(ref disc_value) = kind_field.type_ann.kind {
                return Some((disc_value.clone(), &td.fields));
            }
            return None;
        }
    }
    None
}

/// Capitalize the first letter of a string.
///
/// Used to derive Rust enum variant names from `RustScript` string literals.
pub(super) fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let mut result = c.to_uppercase().to_string();
            result.push_str(chars.as_str());
            result
        }
        None => String::new(),
    }
}

/// Merge auto-inferred derives with explicit user-specified derives.
///
/// Deduplicates entries (if a derive appears in both auto and explicit, it
/// appears only once). Explicit derives are appended after auto-inferred ones.
pub(super) fn merge_derives(mut auto_derives: Vec<String>, explicit: &[ast::Ident]) -> Vec<String> {
    for derive in explicit {
        if !auto_derives.iter().any(|d| d == &derive.name) {
            auto_derives.push(derive.name.clone());
        }
    }
    auto_derives
}
