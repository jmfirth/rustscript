//! Type name resolution and literal type inference.
//!
//! Maps `RustScript` type names to [`Type`] values and infers types from
//! literal expressions. Originally extracted from `rsc-lower/src/types.rs`.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;

use crate::bridge::type_to_rust_type;
use crate::types::{PrimitiveType, Type};

/// Resolve a `RustScript` type name to a [`Type`].
///
/// Handles primitives, `string`, `void`, and the collection type names.
/// User-defined type names are resolved by the type environment (later tasks).
/// Unknown names return `None` — the caller decides whether to treat as
/// user-defined or error.
#[must_use]
pub fn resolve_type_name(name: &str) -> Option<Type> {
    match name {
        "i8" => Some(Type::Primitive(PrimitiveType::I8)),
        "i16" => Some(Type::Primitive(PrimitiveType::I16)),
        "i32" => Some(Type::Primitive(PrimitiveType::I32)),
        "i64" => Some(Type::Primitive(PrimitiveType::I64)),
        "u8" => Some(Type::Primitive(PrimitiveType::U8)),
        "u16" => Some(Type::Primitive(PrimitiveType::U16)),
        "u32" => Some(Type::Primitive(PrimitiveType::U32)),
        "u64" => Some(Type::Primitive(PrimitiveType::U64)),
        "f32" => Some(Type::Primitive(PrimitiveType::F32)),
        "f64" => Some(Type::Primitive(PrimitiveType::F64)),
        "bool" => Some(Type::Primitive(PrimitiveType::Bool)),
        "string" => Some(Type::String),
        "void" => Some(Type::Unit),
        _ => None,
    }
}

/// Resolve a union type annotation (`T | null`) to `Option<T>`.
///
/// Only `T | null` unions are currently supported. The resolver identifies
/// which member is `null` and wraps the other member in `Type::Option`.
/// Multi-member non-null unions are not supported and resolve to the first
/// non-null member.
fn resolve_union_type(
    members: &[ast::TypeAnnotation],
    mut resolve_member: impl FnMut(&ast::TypeAnnotation) -> Type,
) -> Type {
    let mut non_null_types = Vec::new();
    let mut has_null = false;

    for member in members {
        if let ast::TypeKind::Named(ident) = &member.kind
            && ident.name == "null"
        {
            has_null = true;
            continue;
        }
        // `never` is the bottom type — it is eliminated from unions.
        if matches!(member.kind, ast::TypeKind::Never) {
            continue;
        }
        non_null_types.push(resolve_member(member));
    }

    if has_null {
        let inner = if non_null_types.len() == 1 {
            non_null_types.into_iter().next().unwrap_or(Type::Unit)
        } else if non_null_types.is_empty() {
            Type::Unit
        } else {
            // T1 | T2 | null → Option<Union(T1, T2)>
            Type::Union(non_null_types)
        };
        Type::Option(Box::new(inner))
    } else if non_null_types.len() <= 1 {
        // Single type — just return it
        non_null_types.into_iter().next().unwrap_or(Type::Unit)
    } else {
        // General union: string | i32, etc.
        Type::Union(non_null_types)
    }
}

/// Infer the type of a literal expression.
///
/// Returns `None` for non-literal expressions.
#[must_use]
pub fn infer_literal_type(expr: &ast::Expr) -> Option<Type> {
    match &expr.kind {
        ast::ExprKind::IntLit(_) => Some(Type::Primitive(PrimitiveType::I64)),
        ast::ExprKind::FloatLit(_) => Some(Type::Primitive(PrimitiveType::F64)),
        ast::ExprKind::StringLit(_) | ast::ExprKind::TemplateLit(_) => Some(Type::String),
        ast::ExprKind::BoolLit(_) => Some(Type::Primitive(PrimitiveType::Bool)),
        ast::ExprKind::NullLit => Some(Type::Option(Box::new(Type::Error))),
        _ => None,
    }
}

/// Resolve a type annotation to a [`Type`], emitting a diagnostic for unknown types.
///
/// Unknown type names produce a diagnostic and default to `i64`.
#[allow(clippy::too_many_lines)]
// Type annotation resolution covers all TypeKind variants; splitting would obscure the match
pub fn resolve_type_annotation(
    ann: &ast::TypeAnnotation,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    match &ann.kind {
        ast::TypeKind::Void => Type::Unit,
        ast::TypeKind::Never => Type::Never,
        ast::TypeKind::Unknown => Type::Unknown,
        ast::TypeKind::Named(ident) => resolve_type_name(&ident.name).unwrap_or_else(|| {
            diagnostics.push(Diagnostic::error(format!("unknown type `{}`", ident.name)));
            Type::Primitive(PrimitiveType::I64)
        }),
        ast::TypeKind::Generic(ident, args) => {
            let resolved_args: Vec<Type> = args
                .iter()
                .map(|a| resolve_type_annotation(a, diagnostics))
                .collect();
            // Map collection type aliases to their Rust equivalents
            let rust_name = map_collection_type_name(&ident.name);
            Type::Generic(rust_name, resolved_args)
        }
        ast::TypeKind::Union(members) => {
            resolve_union_type(members, |m| resolve_type_annotation(m, diagnostics))
        }
        ast::TypeKind::Function(param_types, return_type) => {
            let params: Vec<Type> = param_types
                .iter()
                .map(|p| resolve_type_annotation(p, diagnostics))
                .collect();
            let ret = resolve_type_annotation(return_type, diagnostics);
            Type::Function(params, Box::new(ret))
        }
        ast::TypeKind::Intersection(members) => {
            // Intersection types are used for trait bounds — resolve each member
            // individually. The lowering pass handles them specially for function parameters.
            // For type resolution, treat the first member as the resolved type.
            members
                .first()
                .map_or(Type::Unit, |m| resolve_type_annotation(m, diagnostics))
        }
        ast::TypeKind::Inferred => Type::Error,
        ast::TypeKind::Shared(inner) => {
            let inner_ty = resolve_type_annotation(inner, diagnostics);
            Type::ArcMutex(Box::new(inner_ty))
        }
        ast::TypeKind::Tuple(types) => {
            let mut resolved = Vec::new();
            for t in types {
                if let ast::TypeKind::TupleSpread(inner) = &t.kind {
                    let spread_ty = resolve_type_annotation(inner, diagnostics);
                    if let Type::Tuple(inner_types) = spread_ty {
                        resolved.extend(inner_types);
                    } else {
                        diagnostics.push(Diagnostic::error(
                            "spread in tuple type must refer to a tuple type".to_owned(),
                        ));
                    }
                } else {
                    resolved.push(resolve_type_annotation(t, diagnostics));
                }
            }
            Type::Tuple(resolved)
        }
        ast::TypeKind::TupleSpread(_) => {
            // TupleSpread outside of a Tuple — invalid position
            diagnostics.push(Diagnostic::error(
                "spread type is only valid inside a tuple type".to_owned(),
            ));
            Type::Error
        }
        ast::TypeKind::IndexSignature(sig) => {
            let key_ty = resolve_type_annotation(&sig.key_type, diagnostics);
            let value_ty = resolve_type_annotation(&sig.value_type, diagnostics);
            Type::Generic("HashMap".to_owned(), vec![key_ty, value_ty])
        }
        ast::TypeKind::StringLiteral(_) => {
            // String literal types are used in utility type arguments
            // (e.g., Pick<User, "name" | "age">). They don't resolve to a runtime
            // type — they're consumed by the utility type lowering pass.
            Type::Error
        }
        ast::TypeKind::KeyOf(_) | ast::TypeKind::TypeOf(_) => {
            // keyof and typeof require the type registry to resolve.
            // Without registry context, treat as error and let the
            // registry-aware resolution handle them.
            Type::Error
        }
        ast::TypeKind::TypeGuard { .. } => {
            // Type guards are `param is Type` return annotations.
            // At runtime they return bool; the guard metadata is for narrowing only.
            Type::Primitive(PrimitiveType::Bool)
        }
        ast::TypeKind::Conditional {
            check_type,
            extends_type,
            true_type,
            false_type,
        } => {
            let check = resolve_type_annotation(check_type, diagnostics);
            let extends = resolve_type_annotation(extends_type, diagnostics);
            if type_satisfies_constraint(&check, &extends) {
                resolve_type_annotation(true_type, diagnostics)
            } else {
                resolve_type_annotation(false_type, diagnostics)
            }
        }
        ast::TypeKind::Infer(_) => {
            // infer outside a conditional type — cannot resolve
            diagnostics.push(Diagnostic::error(
                "`infer` is only valid inside the `extends` clause of a conditional type"
                    .to_owned(),
            ));
            Type::Error
        }
        ast::TypeKind::Readonly(inner) => {
            // readonly is a compile-time modifier — resolve to the inner type.
            // The lowering pass handles parameter-position semantics.
            resolve_type_annotation(inner, diagnostics)
        }
        ast::TypeKind::TemplateLiteralType { .. } => {
            // Template literal types are compile-time string patterns (e.g., `hello ${string}`).
            // Rust's type system cannot express string patterns, so they lower to String.
            Type::String
        }
    }
}

/// Resolve a type annotation to a [`Type`], treating unknown names as user-defined
/// types rather than errors.
///
/// Uses the [`crate::registry::TypeRegistry`] to confirm that the type is registered.
/// Unknown names that are also not in the registry fall back to diagnostics.
/// Type parameter names from the current generic scope resolve to `Type::TypeVar`.
pub fn resolve_type_annotation_with_registry(
    ann: &ast::TypeAnnotation,
    registry: &crate::registry::TypeRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    resolve_type_annotation_with_generics(ann, registry, &[], diagnostics)
}

/// Resolve a type annotation to a [`Type`], with generic type parameter scope.
///
/// `generic_param_names` is the list of type parameter names currently in scope
/// (e.g., `["T", "U"]` inside `function foo<T, U>(...)`). Names matching a
/// generic parameter resolve to `Type::TypeVar` rather than being looked up
/// in the registry.
#[allow(clippy::too_many_lines)]
// Type annotation resolution covers all TypeKind variants; splitting would obscure the match
pub fn resolve_type_annotation_with_generics(
    ann: &ast::TypeAnnotation,
    registry: &crate::registry::TypeRegistry,
    generic_param_names: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    match &ann.kind {
        ast::TypeKind::Void => Type::Unit,
        ast::TypeKind::Never => Type::Never,
        ast::TypeKind::Unknown => Type::Unknown,
        ast::TypeKind::Named(ident) => {
            // `Self` is a special type used in interface method return types.
            // It passes through to the lowering pass which handles it natively.
            if ident.name == "Self" {
                return Type::Named("Self".to_owned());
            }
            // Check if this is a generic type parameter in scope
            if generic_param_names.contains(&ident.name) {
                return Type::TypeVar(ident.name.clone());
            }
            // Try primitive/built-in first
            if let Some(ty) = resolve_type_name(&ident.name) {
                return ty;
            }
            // Try user-defined type from registry
            if registry.lookup(&ident.name).is_some() {
                return Type::Named(ident.name.clone());
            }
            // Unknown type
            diagnostics.push(Diagnostic::error(format!("unknown type `{}`", ident.name)));
            Type::Primitive(PrimitiveType::I64)
        }
        ast::TypeKind::Generic(ident, args) => {
            let resolved_args: Vec<Type> = args
                .iter()
                .map(|a| {
                    resolve_type_annotation_with_generics(
                        a,
                        registry,
                        generic_param_names,
                        diagnostics,
                    )
                })
                .collect();
            // Map collection type aliases to their Rust equivalents
            let rust_name = map_collection_type_name(&ident.name);
            Type::Generic(rust_name, resolved_args)
        }
        ast::TypeKind::Union(members) => resolve_union_type(members, |m| {
            resolve_type_annotation_with_generics(m, registry, generic_param_names, diagnostics)
        }),
        ast::TypeKind::Function(param_types, return_type) => {
            let params: Vec<Type> = param_types
                .iter()
                .map(|p| {
                    resolve_type_annotation_with_generics(
                        p,
                        registry,
                        generic_param_names,
                        diagnostics,
                    )
                })
                .collect();
            let ret = resolve_type_annotation_with_generics(
                return_type,
                registry,
                generic_param_names,
                diagnostics,
            );
            Type::Function(params, Box::new(ret))
        }
        ast::TypeKind::Intersection(members) => {
            // Intersection types are used for trait bounds — resolve each member.
            // For type resolution, treat the first member as the resolved type.
            members.first().map_or(Type::Unit, |m| {
                resolve_type_annotation_with_generics(m, registry, generic_param_names, diagnostics)
            })
        }
        ast::TypeKind::Inferred => {
            // Inferred types are used in closure parameters where the type is omitted.
            // Return Error to signal that no explicit type was provided.
            Type::Error
        }
        ast::TypeKind::Shared(inner) => {
            let inner_ty = resolve_type_annotation_with_generics(
                inner,
                registry,
                generic_param_names,
                diagnostics,
            );
            Type::ArcMutex(Box::new(inner_ty))
        }
        ast::TypeKind::Tuple(types) => {
            let mut resolved = Vec::new();
            for t in types {
                if let ast::TypeKind::TupleSpread(inner) = &t.kind {
                    let spread_ty = resolve_type_annotation_with_generics(
                        inner,
                        registry,
                        generic_param_names,
                        diagnostics,
                    );
                    if let Type::Tuple(inner_types) = spread_ty {
                        resolved.extend(inner_types);
                    } else {
                        diagnostics.push(Diagnostic::error(
                            "spread in tuple type must refer to a tuple type".to_owned(),
                        ));
                    }
                } else {
                    resolved.push(resolve_type_annotation_with_generics(
                        t,
                        registry,
                        generic_param_names,
                        diagnostics,
                    ));
                }
            }
            Type::Tuple(resolved)
        }
        ast::TypeKind::TupleSpread(_) => {
            // TupleSpread outside of a Tuple — invalid position
            diagnostics.push(Diagnostic::error(
                "spread type is only valid inside a tuple type".to_owned(),
            ));
            Type::Error
        }
        ast::TypeKind::IndexSignature(sig) => {
            let key_ty = resolve_type_annotation_with_generics(
                &sig.key_type,
                registry,
                generic_param_names,
                diagnostics,
            );
            let value_ty = resolve_type_annotation_with_generics(
                &sig.value_type,
                registry,
                generic_param_names,
                diagnostics,
            );
            Type::Generic("HashMap".to_owned(), vec![key_ty, value_ty])
        }
        ast::TypeKind::StringLiteral(_) => {
            // String literal types are used in utility type arguments
            // (e.g., Pick<User, "name" | "age">). They don't resolve to a runtime
            // type — they're consumed by the utility type lowering pass.
            Type::Error
        }
        ast::TypeKind::KeyOf(inner) => {
            // keyof T — resolve T, look up its fields, return a Named type
            // that matches the generated enum name. The actual enum generation
            // happens during lowering; here we just resolve to the named type.
            // The lowering pass generates an enum named "{T}Key" with the field
            // names as variants.
            if let ast::TypeKind::Named(ref ident) = inner.kind {
                if registry.lookup(&ident.name).is_some() {
                    // The lowering pass will generate a simple enum for this keyof.
                    // Resolve to a Named type matching the generated enum name.
                    Type::Named(format!("{}Key", ident.name))
                } else {
                    diagnostics.push(Diagnostic::error(format!(
                        "`keyof` requires a known type, but `{}` is not defined",
                        ident.name
                    )));
                    Type::Error
                }
            } else {
                diagnostics.push(Diagnostic::error(
                    "`keyof` requires a named type".to_owned(),
                ));
                Type::Error
            }
        }
        ast::TypeKind::TypeOf(_) => {
            // typeof x — resolution happens during lowering where variable
            // scope information is available. At type-resolution time, we
            // cannot look up variable types, so this is handled as a pass-through.
            // The lowering pass resolves typeof by looking up the variable's type.
            Type::Error
        }
        ast::TypeKind::TypeGuard { .. } => {
            // Type guards are `param is Type` return annotations.
            // At runtime they return bool; the guard metadata is for narrowing only.
            Type::Primitive(PrimitiveType::Bool)
        }
        ast::TypeKind::Conditional {
            check_type,
            extends_type,
            true_type,
            false_type,
        } => resolve_conditional_type(
            check_type,
            extends_type,
            true_type,
            false_type,
            registry,
            generic_param_names,
            diagnostics,
        ),
        ast::TypeKind::Infer(_) => {
            // infer outside a conditional type — cannot resolve
            diagnostics.push(Diagnostic::error(
                "`infer` is only valid inside the `extends` clause of a conditional type"
                    .to_owned(),
            ));
            Type::Error
        }
        ast::TypeKind::Readonly(inner) => {
            // readonly is a compile-time modifier — resolve to the inner type.
            // The lowering pass handles parameter-position semantics.
            resolve_type_annotation_with_generics(inner, registry, generic_param_names, diagnostics)
        }
        ast::TypeKind::TemplateLiteralType { .. } => {
            // Template literal types are compile-time string patterns (e.g., `hello ${string}`).
            // Rust's type system cannot express string patterns, so they lower to String.
            Type::String
        }
    }
}

/// Check whether `check` satisfies the `extends` constraint.
///
/// Simple subtype checks used for conditional type resolution:
/// - Any type extends itself (structural equality).
/// - Any type extends `any` (represented as a named type "any").
/// - `never` extends everything.
/// - `T | null` extends `T | null` if the non-null part matches.
/// - Function types match structurally (parameter/return type matching with infer ignored).
fn type_satisfies_constraint(check: &Type, constraint: &Type) -> bool {
    // Same type always satisfies
    if check == constraint {
        return true;
    }

    // `any` matches everything
    if let Type::Named(name) = constraint
        && name == "any"
    {
        return true;
    }

    // `never` extends everything (it's the bottom type)
    if let Type::Named(name) = check
        && name == "never"
    {
        return true;
    }

    // String extends string, etc. — already handled by equality above.
    // Check generic compatibility: Vec<T> extends Vec<U> if T extends U
    if let (Type::Generic(name1, args1), Type::Generic(name2, args2)) = (check, constraint)
        && name1 == name2
        && args1.len() == args2.len()
    {
        return args1
            .iter()
            .zip(args2.iter())
            .all(|(a, b)| type_satisfies_constraint(a, b));
    }

    // Function types: (A) -> B extends (C) -> D if args match and return matches
    // (simplified — not full contravariance)
    if let (Type::Function(params1, ret1), Type::Function(params2, ret2)) = (check, constraint)
        && params1.len() == params2.len()
    {
        let params_match = params1
            .iter()
            .zip(params2.iter())
            .all(|(a, b)| type_satisfies_constraint(a, b) || *b == Type::Error);
        let ret_match = type_satisfies_constraint(ret1, ret2) || **ret2 == Type::Error;
        return params_match && ret_match;
    }

    false
}

/// Try to match `check_type` against `extends_pattern`, extracting `infer` bindings.
///
/// Returns `Some(bindings)` if the match succeeds, where bindings maps infer
/// variable names to their resolved types. Returns `None` if the match fails.
fn match_with_infer(
    check_type: &Type,
    extends_ast: &ast::TypeAnnotation,
    registry: &crate::registry::TypeRegistry,
    generic_param_names: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<std::collections::HashMap<String, Type>> {
    let mut bindings = std::collections::HashMap::new();
    if match_type_pattern(
        check_type,
        extends_ast,
        &mut bindings,
        registry,
        generic_param_names,
        diagnostics,
    ) {
        Some(bindings)
    } else {
        None
    }
}

/// Recursively match a concrete type against an AST type pattern, binding `infer` variables.
fn match_type_pattern(
    concrete: &Type,
    pattern: &ast::TypeAnnotation,
    bindings: &mut std::collections::HashMap<String, Type>,
    registry: &crate::registry::TypeRegistry,
    generic_param_names: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    match &pattern.kind {
        ast::TypeKind::Infer(ident) => {
            // Bind this infer variable to the concrete type
            bindings.insert(ident.name.clone(), concrete.clone());
            true
        }
        ast::TypeKind::Function(param_patterns, return_pattern) => {
            // Match function type pattern: (...args: P) => R
            if let Type::Function(concrete_params, concrete_ret) = concrete {
                // Match return type
                let ret_match = match_type_pattern(
                    concrete_ret,
                    return_pattern,
                    bindings,
                    registry,
                    generic_param_names,
                    diagnostics,
                );
                // Match parameter types (simplified: check count and each param)
                let params_match = if param_patterns.len() == concrete_params.len() {
                    param_patterns
                        .iter()
                        .zip(concrete_params.iter())
                        .all(|(pat, conc)| {
                            match_type_pattern(
                                conc,
                                pat,
                                bindings,
                                registry,
                                generic_param_names,
                                diagnostics,
                            )
                        })
                } else if param_patterns.len() == 1 {
                    // Single param pattern could match a tuple of params via `infer P`
                    if let ast::TypeKind::Infer(ident) = &param_patterns[0].kind {
                        bindings.insert(ident.name.clone(), Type::Tuple(concrete_params.clone()));
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                ret_match && params_match
            } else {
                false
            }
        }
        ast::TypeKind::Generic(ident, arg_patterns) => {
            // Match generic type: Array<infer T>, etc.
            let rust_name = map_collection_type_name(&ident.name);
            if let Type::Generic(concrete_name, concrete_args) = concrete
                && &rust_name == concrete_name
                && arg_patterns.len() == concrete_args.len()
            {
                return arg_patterns
                    .iter()
                    .zip(concrete_args.iter())
                    .all(|(pat, conc)| {
                        match_type_pattern(
                            conc,
                            pat,
                            bindings,
                            registry,
                            generic_param_names,
                            diagnostics,
                        )
                    });
            }
            false
        }
        ast::TypeKind::Named(ident) if ident.name == "any" => {
            // `any` matches everything in extends position
            true
        }
        _ => {
            // For non-pattern types, resolve and compare structurally
            let resolved = resolve_type_annotation_with_generics(
                pattern,
                registry,
                generic_param_names,
                diagnostics,
            );
            type_satisfies_constraint(concrete, &resolved)
        }
    }
}

/// Resolve a conditional type with full registry and generic context.
///
/// Evaluates `CheckType extends ExtendsType ? TrueType : FalseType` by:
/// 1. Checking for `infer` bindings in the extends clause
/// 2. Performing the extends check (possibly with infer pattern matching)
/// 3. Resolving the appropriate branch, substituting any infer bindings
#[allow(clippy::too_many_arguments)]
fn resolve_conditional_type(
    check_type: &ast::TypeAnnotation,
    extends_type: &ast::TypeAnnotation,
    true_type: &ast::TypeAnnotation,
    false_type: &ast::TypeAnnotation,
    registry: &crate::registry::TypeRegistry,
    generic_param_names: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    let check = resolve_type_annotation_with_generics(
        check_type,
        registry,
        generic_param_names,
        diagnostics,
    );

    // If check type is still a type variable (unresolved generic), fall back to true branch
    if let Type::TypeVar(_) = &check {
        return resolve_type_annotation_with_generics(
            true_type,
            registry,
            generic_param_names,
            diagnostics,
        );
    }

    // Check if extends clause contains `infer` patterns
    let has_infer = ast_contains_infer(extends_type);

    if has_infer {
        // Try to match with infer pattern extraction
        if let Some(bindings) = match_with_infer(
            &check,
            extends_type,
            registry,
            generic_param_names,
            diagnostics,
        ) {
            // Resolve true branch, substituting infer bindings
            resolve_type_with_infer_bindings(
                true_type,
                &bindings,
                registry,
                generic_param_names,
                diagnostics,
            )
        } else {
            // Match failed — resolve false branch
            resolve_type_annotation_with_generics(
                false_type,
                registry,
                generic_param_names,
                diagnostics,
            )
        }
    } else {
        // No infer — simple extends check
        let extends = resolve_type_annotation_with_generics(
            extends_type,
            registry,
            generic_param_names,
            diagnostics,
        );
        if type_satisfies_constraint(&check, &extends) {
            resolve_type_annotation_with_generics(
                true_type,
                registry,
                generic_param_names,
                diagnostics,
            )
        } else {
            resolve_type_annotation_with_generics(
                false_type,
                registry,
                generic_param_names,
                diagnostics,
            )
        }
    }
}

/// Check whether an AST type annotation contains any `infer` nodes.
fn ast_contains_infer(ann: &ast::TypeAnnotation) -> bool {
    match &ann.kind {
        ast::TypeKind::Infer(_) => true,
        ast::TypeKind::Function(params, ret) => {
            params.iter().any(ast_contains_infer) || ast_contains_infer(ret)
        }
        ast::TypeKind::Generic(_, args) => args.iter().any(ast_contains_infer),
        ast::TypeKind::Union(members) | ast::TypeKind::Intersection(members) => {
            members.iter().any(ast_contains_infer)
        }
        ast::TypeKind::Tuple(types) | ast::TypeKind::TemplateLiteralType { types, .. } => {
            types.iter().any(ast_contains_infer)
        }
        ast::TypeKind::Conditional {
            check_type,
            extends_type,
            true_type,
            false_type,
        } => {
            ast_contains_infer(check_type)
                || ast_contains_infer(extends_type)
                || ast_contains_infer(true_type)
                || ast_contains_infer(false_type)
        }
        ast::TypeKind::KeyOf(inner)
        | ast::TypeKind::Shared(inner)
        | ast::TypeKind::TupleSpread(inner)
        | ast::TypeKind::Readonly(inner) => ast_contains_infer(inner),
        ast::TypeKind::IndexSignature(sig) => {
            ast_contains_infer(&sig.key_type) || ast_contains_infer(&sig.value_type)
        }
        ast::TypeKind::Named(_)
        | ast::TypeKind::Void
        | ast::TypeKind::Never
        | ast::TypeKind::Unknown
        | ast::TypeKind::Inferred
        | ast::TypeKind::StringLiteral(_)
        | ast::TypeKind::TypeOf(_)
        | ast::TypeKind::TypeGuard { .. } => false,
    }
}

/// Resolve a type annotation, substituting any `infer` variable references
/// with their bound values from the bindings map.
fn resolve_type_with_infer_bindings(
    ann: &ast::TypeAnnotation,
    bindings: &std::collections::HashMap<String, Type>,
    registry: &crate::registry::TypeRegistry,
    generic_param_names: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    match &ann.kind {
        // If this is a named type that matches an infer binding, use the binding
        ast::TypeKind::Named(ident) if bindings.contains_key(&ident.name) => {
            bindings[&ident.name].clone()
        }
        // For `infer R` in the result position, look up the binding
        ast::TypeKind::Infer(ident) => bindings.get(&ident.name).cloned().unwrap_or(Type::Error),
        // For all other types, resolve normally
        _ => resolve_type_annotation_with_generics(ann, registry, generic_param_names, diagnostics),
    }
}

/// Map `RustScript` collection type names to their Rust equivalents.
///
/// `Array` → `Vec`, `Map` → `HashMap`, `Set` → `HashSet`.
/// All other names pass through unchanged.
#[must_use]
pub fn map_collection_type_name(name: &str) -> String {
    match name {
        "Array" | "ReadonlyArray" => "Vec".to_owned(),
        "Map" => "HashMap".to_owned(),
        "Set" => "HashSet".to_owned(),
        other => other.to_owned(),
    }
}

/// Resolve a type annotation to a `RustType`, emitting diagnostics for unknown types.
///
/// This is a convenience function that resolves through [`Type`] and converts
/// to [`rsc_syntax::rust_ir::RustType`] via the bridge. It preserves the exact
/// behavior of the original `rsc-lower/types.rs` resolution.
pub fn resolve_type_annotation_to_rust_type(
    ann: &ast::TypeAnnotation,
    diagnostics: &mut Vec<Diagnostic>,
) -> rsc_syntax::rust_ir::RustType {
    let ty = resolve_type_annotation(ann, diagnostics);
    type_to_rust_type(&ty)
}

/// Infer the `RustType` of a literal expression.
///
/// Convenience function that infers through [`Type`] and converts via the bridge.
#[must_use]
pub fn infer_literal_rust_type(expr: &ast::Expr) -> Option<rsc_syntax::rust_ir::RustType> {
    infer_literal_type(expr).map(|ty| type_to_rust_type(&ty))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::ast::{Expr, ExprKind, Ident, TypeAnnotation, TypeKind};
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

    // Test 1: All known types resolve correctly (expanded for Phase 1)
    #[test]
    fn test_resolve_type_name_all_known_types() {
        assert_eq!(
            resolve_type_name("i8"),
            Some(Type::Primitive(PrimitiveType::I8))
        );
        assert_eq!(
            resolve_type_name("i16"),
            Some(Type::Primitive(PrimitiveType::I16))
        );
        assert_eq!(
            resolve_type_name("i32"),
            Some(Type::Primitive(PrimitiveType::I32))
        );
        assert_eq!(
            resolve_type_name("i64"),
            Some(Type::Primitive(PrimitiveType::I64))
        );
        assert_eq!(
            resolve_type_name("u8"),
            Some(Type::Primitive(PrimitiveType::U8))
        );
        assert_eq!(
            resolve_type_name("u16"),
            Some(Type::Primitive(PrimitiveType::U16))
        );
        assert_eq!(
            resolve_type_name("u32"),
            Some(Type::Primitive(PrimitiveType::U32))
        );
        assert_eq!(
            resolve_type_name("u64"),
            Some(Type::Primitive(PrimitiveType::U64))
        );
        assert_eq!(
            resolve_type_name("f32"),
            Some(Type::Primitive(PrimitiveType::F32))
        );
        assert_eq!(
            resolve_type_name("f64"),
            Some(Type::Primitive(PrimitiveType::F64))
        );
        assert_eq!(
            resolve_type_name("bool"),
            Some(Type::Primitive(PrimitiveType::Bool))
        );
        assert_eq!(resolve_type_name("string"), Some(Type::String));
        assert_eq!(resolve_type_name("void"), Some(Type::Unit));
    }

    // Test 2: Unknown type returns None
    #[test]
    fn test_resolve_type_name_unknown_returns_none() {
        assert_eq!(resolve_type_name("Foo"), None);
        assert_eq!(resolve_type_name("Bar"), None);
        assert_eq!(resolve_type_name(""), None);
        assert_eq!(resolve_type_name("UnknownType"), None);
    }

    // Test 3: New types not in Phase 0
    #[test]
    fn test_resolve_type_name_u8_new_type() {
        assert_eq!(
            resolve_type_name("u8"),
            Some(Type::Primitive(PrimitiveType::U8))
        );
    }

    #[test]
    fn test_resolve_type_name_f32_new_type() {
        assert_eq!(
            resolve_type_name("f32"),
            Some(Type::Primitive(PrimitiveType::F32))
        );
    }

    // Test 4: Literal type inference
    #[test]
    fn test_infer_literal_type_int_lit() {
        let expr = Expr {
            kind: ExprKind::IntLit(42),
            span: span(0, 2),
        };
        assert_eq!(
            infer_literal_type(&expr),
            Some(Type::Primitive(PrimitiveType::I64))
        );
    }

    #[test]
    fn test_infer_literal_type_float_lit() {
        let expr = Expr {
            kind: ExprKind::FloatLit(3.14),
            span: span(0, 4),
        };
        assert_eq!(
            infer_literal_type(&expr),
            Some(Type::Primitive(PrimitiveType::F64))
        );
    }

    #[test]
    fn test_infer_literal_type_string_lit() {
        let expr = Expr {
            kind: ExprKind::StringLit("hello".to_owned()),
            span: span(0, 7),
        };
        assert_eq!(infer_literal_type(&expr), Some(Type::String));
    }

    #[test]
    fn test_infer_literal_type_bool_lit() {
        let expr = Expr {
            kind: ExprKind::BoolLit(true),
            span: span(0, 4),
        };
        assert_eq!(
            infer_literal_type(&expr),
            Some(Type::Primitive(PrimitiveType::Bool))
        );
    }

    #[test]
    fn test_infer_literal_type_non_literal_returns_none() {
        let expr = Expr {
            kind: ExprKind::Ident(ident("x", 0, 1)),
            span: span(0, 1),
        };
        assert_eq!(infer_literal_type(&expr), None);
    }

    // Test 5: resolve_type_annotation with known type
    #[test]
    fn test_resolve_type_annotation_known_type() {
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("i32", 0, 3)),
            span: span(0, 3),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::I32));
        assert!(diags.is_empty());
    }

    #[test]
    fn test_resolve_type_annotation_void() {
        let ann = TypeAnnotation {
            kind: TypeKind::Void,
            span: span(0, 4),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Unit);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_resolve_type_annotation_unknown_emits_diagnostic_defaults_i64() {
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("Foo", 0, 3)),
            span: span(0, 3),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::I64));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown type"));
        assert!(diags[0].message.contains("Foo"));
    }

    // Test T14-14: Type registry resolves user-defined types
    #[test]
    fn test_resolve_type_annotation_with_registry_known_user_type() {
        let mut registry = crate::registry::TypeRegistry::new();
        registry.register(
            "User".to_owned(),
            vec![
                ("name".to_owned(), Type::String),
                ("age".to_owned(), Type::Primitive(PrimitiveType::U32)),
            ],
        );

        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("User", 0, 4)),
            span: span(0, 4),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert_eq!(ty, Type::Named("User".to_owned()));
        assert!(diags.is_empty());
    }

    // Test T14-15: Type registry still resolves primitives
    #[test]
    fn test_resolve_type_annotation_with_registry_primitive_type() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("i32", 0, 3)),
            span: span(0, 3),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::I32));
        assert!(diags.is_empty());
    }

    // Test T14-16: Unknown type with registry emits diagnostic
    #[test]
    fn test_resolve_type_annotation_with_registry_unknown_type() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("Unknown", 0, 7)),
            span: span(0, 7),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::I64));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown type"));
    }

    // ---- Task 016: Generic type resolution ----

    // Test T16-13: Type parameter T in function body resolves to TypeVar
    #[test]
    fn test_resolve_type_param_in_generic_scope() {
        let registry = crate::registry::TypeRegistry::new();
        let generic_names = vec!["T".to_owned()];
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("T", 0, 1)),
            span: span(0, 1),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_generics(&ann, &registry, &generic_names, &mut diags);
        assert_eq!(ty, Type::TypeVar("T".to_owned()));
        assert!(diags.is_empty());
    }

    // Test T16-14: Generic type annotation resolves to Type::Generic
    // Array<string> maps to Vec<String> via collection type alias mapping
    #[test]
    fn test_resolve_generic_type_annotation() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Generic(
                ident("Array", 0, 5),
                vec![TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }],
            ),
            span: span(0, 13),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_generics(&ann, &registry, &[], &mut diags);
        match &ty {
            Type::Generic(name, args) => {
                assert_eq!(name, "Vec");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], Type::String);
            }
            _ => panic!("expected Generic type, got {ty:?}"),
        }
    }

    // ---- Task 017: Collection type mapping ----

    // Test T17-10b: Map<string, u32> resolves to Generic("HashMap", [String, u32])
    #[test]
    fn test_resolve_map_type_to_hashmap() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Generic(
                ident("Map", 0, 3),
                vec![
                    TypeAnnotation {
                        kind: TypeKind::Named(ident("string", 0, 6)),
                        span: span(0, 6),
                    },
                    TypeAnnotation {
                        kind: TypeKind::Named(ident("u32", 0, 3)),
                        span: span(0, 3),
                    },
                ],
            ),
            span: span(0, 20),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_generics(&ann, &registry, &[], &mut diags);
        match &ty {
            Type::Generic(name, args) => {
                assert_eq!(name, "HashMap");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], Type::String);
                assert_eq!(args[1], Type::Primitive(PrimitiveType::U32));
            }
            _ => panic!("expected Generic type, got {ty:?}"),
        }
    }

    // Test T17-10c: Set<string> resolves to Generic("HashSet", [String])
    #[test]
    fn test_resolve_set_type_to_hashset() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Generic(
                ident("Set", 0, 3),
                vec![TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }],
            ),
            span: span(0, 13),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_generics(&ann, &registry, &[], &mut diags);
        match &ty {
            Type::Generic(name, args) => {
                assert_eq!(name, "HashSet");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], Type::String);
            }
            _ => panic!("expected Generic type, got {ty:?}"),
        }
    }

    // Test T17-10d: map_collection_type_name passes through unknown names
    #[test]
    fn test_map_collection_type_name_passthrough() {
        assert_eq!(map_collection_type_name("Array"), "Vec");
        assert_eq!(map_collection_type_name("Map"), "HashMap");
        assert_eq!(map_collection_type_name("Set"), "HashSet");
        assert_eq!(map_collection_type_name("Container"), "Container");
    }

    // ---- Task 065: General union types ----

    // Test T065-1: string | i32 (no null) produces Type::Union, not Option
    #[test]
    fn test_resolve_general_union_produces_union_type() {
        let ann = TypeAnnotation {
            kind: TypeKind::Union(vec![
                TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                },
                TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 9, 12)),
                    span: span(9, 12),
                },
            ]),
            span: span(0, 12),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert!(diags.is_empty());
        match &ty {
            Type::Union(members) => {
                assert_eq!(members.len(), 2);
                assert_eq!(members[0], Type::String);
                assert_eq!(members[1], Type::Primitive(PrimitiveType::I32));
            }
            other => panic!("expected Union, got {other:?}"),
        }
    }

    // Test T065-2: string | null still produces Option<String>
    #[test]
    fn test_resolve_union_with_null_still_produces_option() {
        let ann = TypeAnnotation {
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
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert!(diags.is_empty());
        assert_eq!(ty, Type::Option(Box::new(Type::String)));
    }

    // Test T065-3: string | i32 | bool produces three-member Union
    #[test]
    fn test_resolve_three_type_union() {
        let ann = TypeAnnotation {
            kind: TypeKind::Union(vec![
                TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                },
                TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 9, 12)),
                    span: span(9, 12),
                },
                TypeAnnotation {
                    kind: TypeKind::Named(ident("bool", 15, 19)),
                    span: span(15, 19),
                },
            ]),
            span: span(0, 19),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert!(diags.is_empty());
        match &ty {
            Type::Union(members) => {
                assert_eq!(members.len(), 3);
            }
            other => panic!("expected Union, got {other:?}"),
        }
    }

    // ---- keyof type operator ----

    #[test]
    fn test_resolve_keyof_with_registry_known_type() {
        let mut registry = crate::registry::TypeRegistry::new();
        registry.register(
            "User".to_owned(),
            vec![
                ("name".to_owned(), Type::String),
                ("age".to_owned(), Type::Primitive(PrimitiveType::U32)),
            ],
        );

        let ann = TypeAnnotation {
            kind: TypeKind::KeyOf(Box::new(TypeAnnotation {
                kind: TypeKind::Named(ident("User", 6, 10)),
                span: span(6, 10),
            })),
            span: span(0, 10),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert!(diags.is_empty());
        assert_eq!(ty, Type::Named("UserKey".to_owned()));
    }

    #[test]
    fn test_resolve_keyof_unknown_type_emits_diagnostic() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::KeyOf(Box::new(TypeAnnotation {
                kind: TypeKind::Named(ident("Unknown", 6, 13)),
                span: span(6, 13),
            })),
            span: span(0, 13),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert_eq!(ty, Type::Error);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("keyof"));
    }

    #[test]
    fn test_resolve_keyof_without_registry_returns_error() {
        let ann = TypeAnnotation {
            kind: TypeKind::KeyOf(Box::new(TypeAnnotation {
                kind: TypeKind::Named(ident("User", 6, 10)),
                span: span(6, 10),
            })),
            span: span(0, 10),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        // Without registry, keyof returns Error
        assert_eq!(ty, Type::Error);
    }

    // ---- typeof type operator ----

    #[test]
    fn test_resolve_typeof_returns_error_at_type_level() {
        let ann = TypeAnnotation {
            kind: TypeKind::TypeOf(ident("config", 7, 13)),
            span: span(0, 13),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        // typeof requires variable scope info — returns Error at type-resolution time
        assert_eq!(ty, Type::Error);
    }

    #[test]
    fn test_resolve_typeof_with_registry_returns_error() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::TypeOf(ident("config", 7, 13)),
            span: span(0, 13),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        // typeof requires variable scope info — returns Error even with registry
        assert_eq!(ty, Type::Error);
    }

    // ---- Conditional types ----

    #[test]
    fn test_resolve_conditional_true_branch_same_type() {
        // string extends string ? bool : i32 → bool
        let ann = TypeAnnotation {
            kind: TypeKind::Conditional {
                check_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }),
                extends_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 15, 21)),
                    span: span(15, 21),
                }),
                true_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("bool", 24, 28)),
                    span: span(24, 28),
                }),
                false_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 31, 34)),
                    span: span(31, 34),
                }),
            },
            span: span(0, 34),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::Bool));
        assert!(diags.is_empty());
    }

    #[test]
    fn test_resolve_conditional_false_branch_different_type() {
        // i32 extends string ? bool : f64 → f64
        let ann = TypeAnnotation {
            kind: TypeKind::Conditional {
                check_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 0, 3)),
                    span: span(0, 3),
                }),
                extends_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 12, 18)),
                    span: span(12, 18),
                }),
                true_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("bool", 21, 25)),
                    span: span(21, 25),
                }),
                false_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("f64", 28, 31)),
                    span: span(28, 31),
                }),
            },
            span: span(0, 31),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::F64));
        assert!(diags.is_empty());
    }

    #[test]
    fn test_type_satisfies_constraint_same_type() {
        assert!(type_satisfies_constraint(&Type::String, &Type::String));
    }

    #[test]
    fn test_type_satisfies_constraint_any_matches_all() {
        assert!(type_satisfies_constraint(
            &Type::Primitive(PrimitiveType::I32),
            &Type::Named("any".to_owned())
        ));
    }

    #[test]
    fn test_type_satisfies_constraint_different_types() {
        assert!(!type_satisfies_constraint(
            &Type::Primitive(PrimitiveType::I32),
            &Type::String
        ));
    }

    #[test]
    fn test_type_satisfies_constraint_never_extends_all() {
        assert!(type_satisfies_constraint(
            &Type::Named("never".to_owned()),
            &Type::String
        ));
    }

    #[test]
    fn test_resolve_infer_outside_conditional_emits_diagnostic() {
        let ann = TypeAnnotation {
            kind: TypeKind::Infer(ident("R", 6, 7)),
            span: span(0, 7),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Error);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("infer"));
    }

    #[test]
    fn test_resolve_conditional_with_registry() {
        // i32 extends i32 ? string : bool → string
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Conditional {
                check_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 0, 3)),
                    span: span(0, 3),
                }),
                extends_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 12, 15)),
                    span: span(12, 15),
                }),
                true_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 18, 24)),
                    span: span(18, 24),
                }),
                false_type: Box::new(TypeAnnotation {
                    kind: TypeKind::Named(ident("bool", 27, 31)),
                    span: span(27, 31),
                }),
            },
            span: span(0, 31),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert_eq!(ty, Type::String);
        assert!(diags.is_empty());
    }

    // ---------------------------------------------------------------
    // Variadic tuple spread resolution tests
    // ---------------------------------------------------------------

    #[test]
    fn test_resolve_tuple_with_spread_of_inline_tuple() {
        // [...[string, i32], bool] → (String, i32, bool)
        let ann = TypeAnnotation {
            kind: TypeKind::Tuple(vec![
                TypeAnnotation {
                    kind: TypeKind::TupleSpread(Box::new(TypeAnnotation {
                        kind: TypeKind::Tuple(vec![
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("string", 4, 10)),
                                span: span(4, 10),
                            },
                            TypeAnnotation {
                                kind: TypeKind::Named(ident("i32", 12, 15)),
                                span: span(12, 15),
                            },
                        ]),
                        span: span(4, 16),
                    })),
                    span: span(1, 16),
                },
                TypeAnnotation {
                    kind: TypeKind::Named(ident("bool", 18, 22)),
                    span: span(18, 22),
                },
            ]),
            span: span(0, 23),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(
            ty,
            Type::Tuple(vec![
                Type::String,
                Type::Primitive(PrimitiveType::I32),
                Type::Primitive(PrimitiveType::Bool),
            ])
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn test_resolve_tuple_spread_of_non_tuple_produces_diagnostic() {
        // [...string, i32] — string is not a tuple type
        let ann = TypeAnnotation {
            kind: TypeKind::Tuple(vec![
                TypeAnnotation {
                    kind: TypeKind::TupleSpread(Box::new(TypeAnnotation {
                        kind: TypeKind::Named(ident("string", 4, 10)),
                        span: span(4, 10),
                    })),
                    span: span(1, 10),
                },
                TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 12, 15)),
                    span: span(12, 15),
                },
            ]),
            span: span(0, 16),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        // The spread failed, so only i32 remains
        assert_eq!(ty, Type::Tuple(vec![Type::Primitive(PrimitiveType::I32)]));
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .message
                .contains("spread in tuple type must refer to a tuple type")
        );
    }

    #[test]
    fn test_resolve_tuple_spread_outside_tuple_produces_diagnostic() {
        // TupleSpread at top level — invalid
        let ann = TypeAnnotation {
            kind: TypeKind::TupleSpread(Box::new(TypeAnnotation {
                kind: TypeKind::Named(ident("Pair", 3, 7)),
                span: span(3, 7),
            })),
            span: span(0, 7),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Error);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .message
                .contains("spread type is only valid inside a tuple type")
        );
    }

    // ---- Task 117: unknown type ----

    #[test]
    fn test_resolve_type_annotation_unknown_produces_unknown() {
        let ann = TypeAnnotation {
            kind: TypeKind::Unknown,
            span: span(0, 7),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Unknown);
        assert!(diags.is_empty(), "unknown type should not emit diagnostics");
    }

    #[test]
    fn test_resolve_type_annotation_unknown_to_rust_type() {
        let ann = TypeAnnotation {
            kind: TypeKind::Unknown,
            span: span(0, 7),
        };
        let mut diags = Vec::new();
        let rust_ty = resolve_type_annotation_to_rust_type(&ann, &mut diags);
        assert_eq!(
            rust_ty,
            rsc_syntax::rust_ir::RustType::BoxDynAny,
            "unknown type should resolve to BoxDynAny"
        );
    }
}
