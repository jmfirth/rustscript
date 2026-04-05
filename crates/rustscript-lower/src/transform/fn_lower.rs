//! Function lowering logic.
//!
//! Handles lowering of function declarations (including generator functions),
//! function signature registration for throws detection and parameter mode
//! analysis, external signature registration, and top-level const lowering.
//! Generator functions are lowered to state machine structs with Iterator impls.

use std::collections::HashSet;

use rustscript_syntax::ast;
use rustscript_syntax::external_fn::ExternalReturnType;
use rustscript_syntax::rust_ir::{
    ParamMode, RustAttribute, RustBlock, RustConstItem, RustExpr, RustExprKind, RustFieldDef,
    RustFnDecl, RustImplBlock, RustItem, RustMethod, RustParam, RustSelfParam, RustStmt,
    RustStructDef, RustTraitImplBlock, RustType, RustTypeParam,
};

use crate::context::LoweringContext;
use crate::ownership::{self, UseMap};
use rustscript_typeck::resolve;

use super::type_lower::{collect_generic_param_names, lower_type_params};
use super::{FnSignature, Transform};

impl Transform {
    /// Register a function signature in the pre-pass for throws and parameter type detection.
    pub(super) fn register_fn_signature(&mut self, f: &ast::FnDecl, ctx: &mut LoweringContext) {
        let throws = f
            .return_type
            .as_ref()
            .and_then(|rt| rt.throws.as_ref())
            .is_some();

        let mut diags = Vec::new();
        let param_types: Vec<RustType> = f
            .params
            .iter()
            .map(|p| {
                let is_readonly = matches!(&p.type_ann.kind, ast::TypeKind::Readonly(_));
                let is_readonly_array_generic =
                    matches!(&p.type_ann.kind, ast::TypeKind::Generic(ident, _) if ident.name == "ReadonlyArray");

                let ty = rustscript_typeck::resolve::resolve_type_annotation_with_registry(
                    &p.type_ann,
                    &self.type_registry,
                    &mut diags,
                );
                let mut rust_ty = rustscript_typeck::bridge::type_to_rust_type(&ty);

                // readonly Array<T> or ReadonlyArray<T> in param → &[T] (borrowed slice)
                if (is_readonly || is_readonly_array_generic)
                    && let RustType::Generic(ref base, ref args) = rust_ty
                    && let RustType::Named(ref name) = **base
                    && name == "Vec"
                    && args.len() == 1
                {
                    rust_ty = RustType::Reference(Box::new(RustType::Slice(Box::new(args[0].clone()))));
                }

                // Rewrite base class types to &dyn {Name}Trait for polymorphism
                if let RustType::Named(ref name) = rust_ty
                    && self.extended_classes.contains(name)
                {
                    rust_ty = RustType::DynRef(format!("{name}Trait"));
                }
                // Wrap optional params (without defaults) in Option<T>
                if p.optional && p.default_value.is_none() {
                    rust_ty = RustType::Option(Box::new(rust_ty));
                }
                rust_ty
            })
            .collect();

        // Collect optional/default/rest info
        let optional_params: Vec<bool> = f
            .params
            .iter()
            .map(|p| p.optional || p.default_value.is_some())
            .collect();

        let use_map = ownership::UseMap::empty();
        let default_values: Vec<Option<RustExpr>> = f
            .params
            .iter()
            .map(|p| {
                p.default_value
                    .as_ref()
                    .map(|dv| self.lower_expr(dv, ctx, &use_map, 0))
            })
            .collect();

        let has_rest_param = f.params.last().is_some_and(|p| p.is_rest);
        let param_count = f.params.len();

        // Tier 2: analyze parameter usage to determine borrow modes
        // Skip analysis when --no-borrow-inference is set (all params stay Owned)
        let param_modes = if self.no_borrow_inference {
            None
        } else {
            let param_names: Vec<String> = f.params.iter().map(|p| p.name.name.clone()).collect();
            let builtins = &self.builtins;
            let usage_map = ownership::analyze_param_usage(&f.body, &param_names, |obj, method| {
                builtins.is_ref_args(obj, method)
            });
            let modes: Vec<ParamMode> = param_names
                .iter()
                .zip(param_types.iter())
                .enumerate()
                .map(|(i, (name, ty))| {
                    // Optional, default, and rest params always use Owned mode
                    let is_special = f
                        .params
                        .get(i)
                        .is_some_and(|p| p.optional || p.default_value.is_some() || p.is_rest);
                    if is_special {
                        return ParamMode::Owned;
                    }
                    let usage = usage_map
                        .get(name.as_str())
                        .copied()
                        .unwrap_or(ownership::ParamUsage::ReadOnly);
                    ownership::usage_to_mode(usage, ty, Some(&self.type_registry))
                })
                .collect();
            Some(modes)
        };

        // Resolve the return type for variable type inference at call sites
        let generic_names = collect_generic_param_names(f.type_params.as_ref());
        let return_type = f.return_type.as_ref().and_then(|rt| {
            rt.type_ann.as_ref().and_then(|ann| {
                let mut rt_diags = Vec::new();
                let ty_inner = resolve::resolve_type_annotation_with_generics(
                    ann,
                    &self.type_registry,
                    &generic_names,
                    &mut rt_diags,
                );
                let ty = rustscript_typeck::bridge::type_to_rust_type(&ty_inner);
                for d in rt_diags {
                    ctx.emit_diagnostic(d);
                }
                if ty == RustType::Unit { None } else { Some(ty) }
            })
        });

        self.fn_signatures.insert(
            f.name.name.clone(),
            FnSignature {
                throws,
                param_types,
                param_modes,
                optional_params,
                default_values,
                has_rest_param,
                param_count,
                return_type,
                generic_param_names: generic_names,
            },
        );
    }

    /// Convert external function signatures into `FnSignature` entries.
    ///
    /// Iterates over `self.external_signatures` and inserts corresponding
    /// `FnSignature` entries into `self.fn_signatures`. Free functions are
    /// keyed by their bare name; methods are keyed by `"TypeName::method_name"`.
    /// Does not overwrite locally-defined signatures.
    pub(super) fn register_external_signatures(&mut self) {
        for ext_info in self.external_signatures.values() {
            let param_modes: Vec<ParamMode> = ext_info
                .params
                .iter()
                .map(|p| {
                    if p.is_str_ref {
                        ParamMode::BorrowedStr
                    } else if p.is_ref {
                        ParamMode::Borrowed
                    } else {
                        ParamMode::Owned
                    }
                })
                .collect();

            let throws = matches!(ext_info.return_type, ExternalReturnType::Result);

            let sig = FnSignature {
                throws,
                param_types: vec![],
                param_modes: Some(param_modes),
                optional_params: vec![false; ext_info.params.len()],
                default_values: vec![None; ext_info.params.len()],
                has_rest_param: false,
                param_count: ext_info.params.len(),
                return_type: None,
                generic_param_names: vec![],
            };

            // Key by "TypeName::method_name" for methods, bare name for free functions.
            let key = if let Some(parent) = &ext_info.parent_type {
                format!("{}::{}", parent, ext_info.name)
            } else {
                ext_info.name.clone()
            };

            // Don't overwrite locally-defined signatures.
            self.fn_signatures.entry(key).or_insert(sig);
        }
    }

    /// Lower a top-level `const`/`let` declaration to a Rust `const` item.
    ///
    /// Resolves the type from annotation or literal inference, then lowers
    /// the initializer expression. Produces a `RustItem::Const`.
    pub(super) fn lower_top_level_const(
        &self,
        decl: &ast::VarDecl,
        exported: bool,
        ctx: &mut LoweringContext,
    ) -> RustItem {
        let mut diags = Vec::new();
        let ty = if let Some(ann) = &decl.type_ann {
            let ty_inner = rustscript_typeck::resolve::resolve_type_annotation_with_registry(
                ann,
                &self.type_registry,
                &mut diags,
            );
            rustscript_typeck::bridge::type_to_rust_type(&ty_inner)
        } else {
            rustscript_typeck::resolve::infer_literal_rust_type(&decl.init).unwrap_or(RustType::I64)
        };
        for d in diags {
            ctx.emit_diagnostic(d);
        }

        // If `as const` wraps an array literal, override the type to a static slice reference.
        let ty = if let ast::ExprKind::AsConst(inner) = &decl.init.kind
            && matches!(&inner.kind, ast::ExprKind::ArrayLit(_))
        {
            let elem_ty = super::infer_as_const_slice_element_type(inner);
            RustType::Reference(Box::new(RustType::Slice(Box::new(elem_ty))))
        } else {
            ty
        };

        // Register the const variable in the lowering context so that
        // `typeof x` can resolve to this variable's type.
        ctx.declare_variable(decl.name.name.clone(), ty.clone());

        let use_map = UseMap::empty();
        let init = self.lower_expr(&decl.init, ctx, &use_map, 0);

        RustItem::Const(RustConstItem {
            public: exported,
            name: decl.name.name.clone(),
            ty,
            init,
            span: Some(decl.span),
        })
    }

    /// Lower a function declaration.
    ///
    /// Performs two-pass analysis: first finds reassigned variables and builds
    /// a use map, then lowers the body with that context.
    #[allow(clippy::too_many_lines)]
    // The function coordinates multiple analysis passes (ownership, mutability,
    // type resolution, intersection desugaring, throws wrapping) that share
    // mutable context — splitting would fragment the coherent pipeline.
    pub fn lower_fn(&self, f: &ast::FnDecl, ctx: &mut LoweringContext) -> RustFnDecl {
        ctx.push_scope();

        let generic_names = collect_generic_param_names(f.type_params.as_ref());
        let mut type_params = lower_type_params(f.type_params.as_ref());

        // Phase 1: find reassigned variables for mutability analysis.
        // Also include method call receivers, which need `mut` when calling
        // `&mut self` methods on class instances.
        let mut reassigned = ownership::find_reassigned_variables(&f.body);
        let method_receivers = ownership::find_method_call_receivers(&f.body);
        reassigned.extend(method_receivers);

        // Phase 2: build use map for ownership analysis
        // Tier 2: pass callee param modes so arguments to borrowed params are not move positions
        let use_map = UseMap::analyze(
            &f.body,
            |obj, method| self.builtins.is_ref_args(obj, method),
            |callee_name| {
                self.fn_signatures
                    .get(callee_name)
                    .and_then(|sig| sig.param_modes.as_deref())
            },
        );

        // Track intersection type parameter counter for fresh names
        let mut intersection_param_counter = 0_u32;

        // Look up pre-computed param modes from the signature map (Tier 2)
        let precomputed_modes = self
            .fn_signatures
            .get(&f.name.name)
            .and_then(|sig| sig.param_modes.as_ref());

        // Declare parameters in scope
        let params: Vec<RustParam> = f
            .params
            .iter()
            .enumerate()
            .map(|(param_idx, p)| {
                let mut diags = Vec::new();

                // Check for intersection type parameters
                if let ast::TypeKind::Intersection(members) = &p.type_ann.kind {
                    let fresh_name = if intersection_param_counter == 0 {
                        "T".to_owned()
                    } else {
                        format!("T{intersection_param_counter}")
                    };
                    intersection_param_counter += 1;

                    // Collect trait bound names from each member
                    let bounds: Vec<String> = members
                        .iter()
                        .filter_map(|m| match &m.kind {
                            ast::TypeKind::Named(ident) => Some(ident.name.clone()),
                            _ => None,
                        })
                        .collect();

                    // Add the fresh type parameter with bounds
                    type_params.push(RustTypeParam {
                        name: fresh_name.clone(),
                        bounds,
                    });

                    let ty = RustType::TypeParam(fresh_name);
                    ctx.declare_variable(p.name.name.clone(), ty.clone());
                    return RustParam {
                        name: p.name.name.clone(),
                        ty,
                        mode: ParamMode::Owned,
                        span: Some(p.span),
                    };
                }

                // Check for readonly array/tuple in parameter position
                let is_readonly = matches!(&p.type_ann.kind, ast::TypeKind::Readonly(_));
                let is_readonly_array_generic =
                    matches!(&p.type_ann.kind, ast::TypeKind::Generic(ident, _) if ident.name == "ReadonlyArray");

                let ty_inner = resolve::resolve_type_annotation_with_generics(
                    &p.type_ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                let mut ty = rustscript_typeck::bridge::type_to_rust_type(&ty_inner);
                for d in diags {
                    ctx.emit_diagnostic(d);
                }

                // readonly Array<T> or ReadonlyArray<T> in param → &[T] (borrowed slice)
                if (is_readonly || is_readonly_array_generic)
                    && let RustType::Generic(ref base, ref args) = ty
                    && let RustType::Named(ref name) = **base
                    && name == "Vec"
                    && args.len() == 1
                {
                    ty = RustType::Reference(Box::new(RustType::Slice(Box::new(args[0].clone()))));
                }

                // Rewrite base class types to &dyn {Name}Trait for polymorphism
                if let RustType::Named(ref name) = ty
                    && self.extended_classes.contains(name)
                {
                    ty = RustType::DynRef(format!("{name}Trait"));
                }

                // Optional params (without defaults) get wrapped in Option<T>
                if p.optional && p.default_value.is_none() {
                    ty = RustType::Option(Box::new(ty));
                }

                ctx.declare_variable(p.name.name.clone(), ty.clone());

                // Use pre-computed param mode from Tier 2 analysis
                let mut mode = precomputed_modes
                    .and_then(|modes| modes.get(param_idx))
                    .copied()
                    .unwrap_or(ParamMode::Owned);

                // DynRef and Slice types are already references — force Owned mode
                // to avoid emitting `&&dyn Trait` or `&&[T]`
                if matches!(ty, RustType::DynRef(_) | RustType::Slice(_) | RustType::Reference(_)) {
                    mode = ParamMode::Owned;
                }

                // Borrowed parameters are already references — mark them so
                // downstream lowering (e.g., for-of) avoids double-borrowing.
                if matches!(mode, ParamMode::Borrowed | ParamMode::BorrowedStr)
                    || matches!(ty, RustType::DynRef(_) | RustType::Slice(_) | RustType::Reference(_))
                {
                    ctx.mark_as_reference(p.name.name.clone());
                }

                RustParam {
                    name: p.name.name.clone(),
                    ty,
                    mode,
                    span: Some(p.span),
                }
            })
            .collect();

        // Determine if this is a throws function
        let is_throws = f
            .return_type
            .as_ref()
            .and_then(|rt| rt.throws.as_ref())
            .is_some();

        // Resolve the base return type (success type)
        let base_return_type = f.return_type.as_ref().and_then(|rt| {
            rt.type_ann.as_ref().and_then(|ann| {
                let mut diags = Vec::new();
                let ty_inner = resolve::resolve_type_annotation_with_generics(
                    ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                let ty = rustscript_typeck::bridge::type_to_rust_type(&ty_inner);
                for d in diags {
                    ctx.emit_diagnostic(d);
                }
                if ty == RustType::Unit {
                    return None;
                }
                Some(ty)
            })
        });

        // Build the actual return type (Result<T, E> for throws, T otherwise)
        let return_type = if is_throws {
            let ok_ty = base_return_type.clone().unwrap_or(RustType::Unit);
            let err_ty = f
                .return_type
                .as_ref()
                .and_then(|rt| {
                    rt.throws.as_ref().map(|throws_ann| {
                        let mut diags = Vec::new();
                        let ty_inner = resolve::resolve_type_annotation_with_generics(
                            throws_ann,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        let ty = rustscript_typeck::bridge::type_to_rust_type(&ty_inner);
                        for d in diags {
                            ctx.emit_diagnostic(d);
                        }
                        ty
                    })
                })
                .unwrap_or(RustType::Unit);
            Some(RustType::Result(Box::new(ok_ty), Box::new(err_ty)))
        } else {
            base_return_type.clone()
        };

        // Set return type context for Option wrapping in return statements
        ctx.set_return_type(base_return_type);
        ctx.set_fn_throws(is_throws);

        // Lower the body
        let mut body = self.lower_block(&f.body, ctx, &use_map, 0, &reassigned);

        // For throws functions returning void (Result<(), E>), append Ok(()) if the
        // body doesn't already end with a return statement.
        if is_throws && !matches!(body.stmts.last(), Some(RustStmt::Return(_))) {
            body.stmts
                .push(RustStmt::Expr(RustExpr::synthetic(RustExprKind::Ok(
                    Box::new(RustExpr::synthetic(RustExprKind::Ident("()".to_owned()))),
                ))));
        }

        // Unmark borrowed parameters so their names don't leak into sibling functions.
        for param in &params {
            if matches!(param.mode, ParamMode::Borrowed | ParamMode::BorrowedStr) {
                ctx.unmark_reference(&param.name);
            }
        }

        ctx.set_return_type(None);
        ctx.set_fn_throws(false);
        ctx.pop_scope();

        // Add #[tokio::main] attribute for async main functions
        let attributes = if f.is_async && f.name.name == "main" {
            vec![RustAttribute {
                path: "tokio::main".to_owned(),
                args: None,
            }]
        } else {
            vec![]
        };

        RustFnDecl {
            attributes,
            is_async: f.is_async,
            public: false,
            name: f.name.name.clone(),
            type_params,
            params,
            return_type,
            body,
            doc_comment: f.doc_comment.clone(),
            span: Some(f.span),
        }
    }

    /// Lower a generator function (`function*`) to a state machine struct + Iterator impl.
    ///
    /// Produces:
    /// 1. A struct with fields for parameters, local variables, and `_state: u32`
    /// 2. An `impl StructName` block with `fn new(params) -> Self`
    /// 3. An `impl Iterator for StructName` block with `type Item` and `fn next(&mut self)`
    #[allow(clippy::too_many_lines)]
    // Generator lowering builds three IR items (struct, impl, trait impl); splitting would fragment the logic
    pub(super) fn lower_generator(
        &self,
        f: &ast::FnDecl,
        ctx: &mut LoweringContext,
        exported: bool,
    ) -> Vec<RustItem> {
        let struct_name = generator_struct_name(&f.name.name);
        let mut items = Vec::new();

        // Determine the yield type from the return type annotation
        let mut diags = Vec::new();
        let yield_type = f
            .return_type
            .as_ref()
            .and_then(|rt| rt.type_ann.as_ref())
            .map_or(RustType::Unit, |ta| {
                resolve::resolve_type_annotation_to_rust_type(ta, &mut diags)
            });

        // Collect parameter names and types
        let param_fields: Vec<(String, RustType)> = f
            .params
            .iter()
            .map(|p| {
                let ty = resolve::resolve_type_annotation_to_rust_type(&p.type_ann, &mut diags);
                (p.name.name.clone(), ty)
            })
            .collect();

        // Collect local variable names used across yield points
        let local_vars = collect_generator_locals(&f.body, &param_fields);

        // Build struct fields: params + locals + _state
        let mut fields: Vec<RustFieldDef> = Vec::new();
        for (name, ty) in &param_fields {
            fields.push(RustFieldDef {
                public: false,
                name: name.clone(),
                ty: ty.clone(),
                doc_comment: None,
                span: Some(f.span),
            });
        }
        for (name, ty) in &local_vars {
            fields.push(RustFieldDef {
                public: false,
                name: name.clone(),
                ty: ty.clone(),
                doc_comment: None,
                span: Some(f.span),
            });
        }
        fields.push(RustFieldDef {
            public: false,
            name: "_state".to_owned(),
            ty: RustType::U32,
            doc_comment: None,
            span: None,
        });

        // Emit the struct definition
        items.push(RustItem::Struct(RustStructDef {
            public: exported,
            name: struct_name.clone(),
            type_params: vec![],
            fields,
            derives: vec![],
            attributes: vec![],
            doc_comment: f.doc_comment.clone(),
            span: Some(f.span),
        }));

        // Build the `new` constructor: fn new(params) -> Self
        let new_params: Vec<RustParam> = param_fields
            .iter()
            .map(|(name, ty)| RustParam {
                name: name.clone(),
                ty: ty.clone(),
                mode: ParamMode::Owned,
                span: Some(f.span),
            })
            .collect();

        // Build struct literal fields for `new`
        let mut new_field_inits = String::new();
        for (name, _) in &param_fields {
            if !new_field_inits.is_empty() {
                new_field_inits.push_str(", ");
            }
            new_field_inits.push_str(name);
        }
        for (name, ty) in &local_vars {
            if !new_field_inits.is_empty() {
                new_field_inits.push_str(", ");
            }
            new_field_inits.push_str(name);
            new_field_inits.push_str(": ");
            new_field_inits.push_str(&default_value_for_type(ty));
        }
        if !new_field_inits.is_empty() {
            new_field_inits.push_str(", ");
        }
        new_field_inits.push_str("_state: 0");

        let new_body_code = format!("Self {{ {new_field_inits} }}");
        let new_body = RustBlock {
            stmts: vec![],
            expr: Some(Box::new(RustExpr::synthetic(RustExprKind::Raw(
                new_body_code,
            )))),
        };

        let new_method = RustMethod {
            is_async: false,
            name: "new".to_owned(),
            self_param: None,
            params: new_params,
            return_type: Some(RustType::Named("Self".to_owned())),
            body: new_body,
            doc_comment: None,
            span: Some(f.span),
        };

        items.push(RustItem::Impl(RustImplBlock {
            type_name: struct_name.clone(),
            type_params: vec![],
            associated_consts: vec![],
            methods: vec![new_method],
            span: Some(f.span),
        }));

        // Build the state machine body for `fn next(&mut self) -> Option<Item>`
        let next_body_code = build_state_machine_body(f, ctx, self, &yield_type);
        let next_body = RustBlock {
            stmts: vec![],
            expr: Some(Box::new(RustExpr::synthetic(RustExprKind::Raw(
                next_body_code,
            )))),
        };

        let next_method = RustMethod {
            is_async: false,
            name: "next".to_owned(),
            self_param: Some(RustSelfParam::RefMut),
            params: vec![],
            return_type: Some(RustType::Option(Box::new(yield_type.clone()))),
            body: next_body,
            doc_comment: None,
            span: Some(f.span),
        };

        items.push(RustItem::TraitImpl(RustTraitImplBlock {
            trait_name: "Iterator".to_owned(),
            type_name: struct_name,
            type_params: vec![],
            associated_types: vec![("Item".to_owned(), yield_type)],
            methods: vec![next_method],
            span: Some(f.span),
        }));

        items
    }
}

/// Generate the struct name for a generator function.
///
/// Capitalizes the first letter and appends `Iter`:
/// `range` → `RangeIter`, `fibonacci` → `FibonacciIter`.
pub(super) fn generator_struct_name(fn_name: &str) -> String {
    let mut chars = fn_name.chars();
    let first = chars
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    format!("{first}{}Iter", chars.as_str())
}

/// Collect local variable declarations from a generator function body.
///
/// Scans the body for `let`/`const` declarations and returns their names
/// with resolved types. These become fields in the state machine struct.
fn collect_generator_locals(
    body: &ast::Block,
    params: &[(String, RustType)],
) -> Vec<(String, RustType)> {
    let param_names: HashSet<&str> = params.iter().map(|(n, _)| n.as_str()).collect();
    let mut locals = Vec::new();
    collect_locals_from_stmts(&body.stmts, &param_names, params, &mut locals);
    locals
}

/// Recursively collect local variables from statements.
fn collect_locals_from_stmts(
    stmts: &[ast::Stmt],
    param_names: &HashSet<&str>,
    params: &[(String, RustType)],
    locals: &mut Vec<(String, RustType)>,
) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::VarDecl(vd) => {
                if !param_names.contains(vd.name.name.as_str())
                    && !locals.iter().any(|(n, _)| n == &vd.name.name)
                {
                    let ty = if let Some(ta) = vd.type_ann.as_ref() {
                        let mut diags = Vec::new();
                        resolve::resolve_type_annotation_to_rust_type(ta, &mut diags)
                    } else {
                        // Infer type from initializer: if init is an identifier,
                        // look up its type from params or existing locals
                        infer_generator_local_type(&vd.init, params, locals)
                    };
                    locals.push((vd.name.name.clone(), ty));
                }
            }
            ast::Stmt::While(w) => {
                collect_locals_from_stmts(&w.body.stmts, param_names, params, locals);
            }
            ast::Stmt::DoWhile(dw) => {
                collect_locals_from_stmts(&dw.body.stmts, param_names, params, locals);
            }
            ast::Stmt::If(if_stmt) => {
                collect_locals_from_stmts(&if_stmt.then_block.stmts, param_names, params, locals);
                if let Some(ast::ElseClause::Block(ref blk)) = if_stmt.else_clause {
                    collect_locals_from_stmts(&blk.stmts, param_names, params, locals);
                }
            }
            ast::Stmt::For(for_of) => {
                collect_locals_from_stmts(&for_of.body.stmts, param_names, params, locals);
            }
            ast::Stmt::ForIn(for_in) => {
                collect_locals_from_stmts(&for_in.body.stmts, param_names, params, locals);
            }
            ast::Stmt::ForClassic(fc) => {
                // Register the loop variable as a local
                if let Some(ast::ForInit::VarDecl(decl)) = &fc.init
                    && !param_names.contains(decl.name.name.as_str())
                    && !locals.iter().any(|(n, _)| n == &decl.name.name)
                {
                    locals.push((decl.name.name.clone(), RustType::Infer));
                }
                collect_locals_from_stmts(&fc.body.stmts, param_names, params, locals);
            }
            ast::Stmt::Using(decl) => {
                if !param_names.contains(decl.name.name.as_str())
                    && !locals.iter().any(|(n, _)| n == &decl.name.name)
                {
                    let ty = if let Some(ta) = decl.type_ann.as_ref() {
                        let mut diags = Vec::new();
                        resolve::resolve_type_annotation_to_rust_type(ta, &mut diags)
                    } else {
                        infer_generator_local_type(&decl.init, params, locals)
                    };
                    locals.push((decl.name.name.clone(), ty));
                }
            }
            _ => {}
        }
    }
}

/// Infer the type of a generator local variable from its initializer expression.
fn infer_generator_local_type(
    init: &ast::Expr,
    params: &[(String, RustType)],
    locals: &[(String, RustType)],
) -> RustType {
    match &init.kind {
        ast::ExprKind::Ident(ident) => {
            // Look up in params first, then locals
            if let Some((_, ty)) = params.iter().find(|(n, _)| n == &ident.name) {
                return ty.clone();
            }
            if let Some((_, ty)) = locals.iter().find(|(n, _)| n == &ident.name) {
                return ty.clone();
            }
            RustType::I32
        }
        ast::ExprKind::FloatLit(_) => RustType::F64,
        ast::ExprKind::BoolLit(_) => RustType::Bool,
        ast::ExprKind::StringLit(_) => RustType::String,
        _ => RustType::I32,
    }
}

/// Get a default value literal for a given type (used in generator `new()` constructors).
fn default_value_for_type(ty: &RustType) -> String {
    match ty {
        RustType::I8
        | RustType::I16
        | RustType::I32
        | RustType::I64
        | RustType::U8
        | RustType::U16
        | RustType::U32
        | RustType::U64 => "0".to_owned(),
        RustType::F32 | RustType::F64 => "0.0".to_owned(),
        RustType::Bool => "false".to_owned(),
        RustType::String => "String::new()".to_owned(),
        RustType::Unit => "()".to_owned(),
        _ => "Default::default()".to_owned(),
    }
}

/// Build the state machine body for a generator's `next()` method.
///
/// Analyzes the generator function body and transforms it into a `loop { match self._state { ... } }`
/// state machine. Handles the MVP case of a single while loop with yield inside.
fn build_state_machine_body(
    f: &ast::FnDecl,
    _ctx: &mut LoweringContext,
    transform: &Transform,
    _yield_type: &RustType,
) -> String {
    // Analyze the body to find yield points and build state transitions.
    // MVP handles two patterns:
    // 1. Single while loop with yield inside
    // 2. Sequential statements with yields

    let states = analyze_generator_body(&f.body.stmts, transform);
    format_state_machine(&states)
}

/// A state in the generator state machine.
struct GeneratorState {
    /// The state number.
    index: u32,
    /// The Rust code for this state's body.
    code: String,
}

/// Analyze generator body statements and produce state machine states.
fn analyze_generator_body(stmts: &[ast::Stmt], transform: &Transform) -> Vec<GeneratorState> {
    // Check for the common pattern: while loop with yield inside
    if has_while_with_yield(stmts) {
        return analyze_while_yield_pattern(stmts, transform);
    }

    // Fallback: sequential yields
    analyze_sequential_pattern(stmts, transform)
}

/// Check if any statement is a while loop containing a yield.
fn has_while_with_yield(stmts: &[ast::Stmt]) -> bool {
    for stmt in stmts {
        if let ast::Stmt::While(w) = stmt
            && body_contains_yield(&w.body)
        {
            return true;
        }
    }
    false
}

/// Check if a block contains a yield expression.
fn body_contains_yield(body: &ast::Block) -> bool {
    for stmt in &body.stmts {
        if stmt_contains_yield(stmt) {
            return true;
        }
    }
    false
}

/// Check if a statement contains a yield expression.
fn stmt_contains_yield(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Expr(expr) => expr_contains_yield(expr),
        ast::Stmt::If(if_stmt) => {
            body_contains_yield(&if_stmt.then_block)
                || if_stmt.else_clause.as_ref().is_some_and(|ec| match ec {
                    ast::ElseClause::Block(blk) => body_contains_yield(blk),
                    ast::ElseClause::ElseIf(elif) => body_contains_yield(&elif.then_block),
                })
        }
        ast::Stmt::While(w) => body_contains_yield(&w.body),
        ast::Stmt::DoWhile(dw) => body_contains_yield(&dw.body),
        _ => false,
    }
}

/// Check if an expression contains a yield.
fn expr_contains_yield(expr: &ast::Expr) -> bool {
    matches!(expr.kind, ast::ExprKind::Yield(_))
}

/// Analyze the common pattern: init statements + while loop with yield.
///
/// Pattern:
/// ```text
/// let i = start;         // init statements (state 0 preamble)
/// while (i < end) {      // condition check
///   yield i;             // yield → return Some(i), go to state 1
///   i += 1;              // post-yield code (state 1)
/// }
/// ```
#[allow(clippy::too_many_lines)]
// State machine construction for while-yield pattern; splitting would fragment coherent logic
fn analyze_while_yield_pattern(stmts: &[ast::Stmt], transform: &Transform) -> Vec<GeneratorState> {
    let mut states = Vec::new();
    let mut pre_while_code = String::new();
    let mut while_found = false;

    for stmt in stmts {
        if let ast::Stmt::While(w) = stmt
            && body_contains_yield(&w.body)
        {
            while_found = true;
            let condition = emit_expr_to_string(&w.condition, transform);

            // Split while body at yield points
            let mut pre_yield = Vec::new();
            let mut yielded = String::new();
            let mut post_yield = Vec::new();
            let mut found_yield = false;

            for body_stmt in &w.body.stmts {
                if found_yield {
                    post_yield.push(body_stmt);
                } else if let ast::Stmt::Expr(expr) = body_stmt
                    && let ast::ExprKind::Yield(ref inner) = expr.kind
                {
                    yielded = emit_expr_to_string(inner, transform);
                    found_yield = true;
                } else {
                    pre_yield.push(body_stmt);
                }
            }

            let pre_yield_code = pre_yield
                .iter()
                .map(|s| emit_stmt_to_string(s, transform))
                .collect::<Vec<_>>()
                .join("\n                        ");

            let pre_code = if pre_yield_code.is_empty() {
                String::new()
            } else {
                format!("{pre_yield_code}\n                        ")
            };

            let yield_body = format!(
                "if {condition} {{\n                        \
                 {pre_code}self._state = 1;\n                        \
                 return Some({yielded});\n                    \
                 }} else {{\n                        \
                 return None;\n                    \
                 }}"
            );

            if pre_while_code.is_empty() {
                // No init code — condition is state 0 directly
                states.push(GeneratorState {
                    index: 0,
                    code: yield_body,
                });
                push_post_yield_state(&mut states, 1, 0, &post_yield, transform);
            } else {
                // Init code in state 0, condition in state 1
                states.push(GeneratorState {
                    index: 0,
                    code: format!(
                        "{pre_while_code}\n                        \
                         self._state = 1;\n                        \
                         continue;"
                    ),
                });
                states.push(GeneratorState {
                    index: 1,
                    code: yield_body.replace("self._state = 1", "self._state = 2"),
                });
                push_post_yield_state(&mut states, 2, 1, &post_yield, transform);
            }

            continue;
        }

        if !while_found {
            let code = emit_stmt_to_string(stmt, transform);
            if !pre_while_code.is_empty() {
                pre_while_code.push_str("\n                        ");
            }
            pre_while_code.push_str(&code);
        }
    }

    if states.is_empty() {
        states.push(GeneratorState {
            index: 0,
            code: "return None;".to_owned(),
        });
    }

    states
}

/// Push a post-yield state that executes code then transitions back to loop start.
fn push_post_yield_state(
    states: &mut Vec<GeneratorState>,
    state_idx: u32,
    loop_state: u32,
    post_yield: &[&ast::Stmt],
    transform: &Transform,
) {
    let post_code = post_yield
        .iter()
        .map(|s| emit_stmt_to_string(s, transform))
        .collect::<Vec<_>>()
        .join("\n                        ");

    let code = if post_code.is_empty() {
        format!("self._state = {loop_state};\n                        continue;")
    } else {
        format!(
            "{post_code}\n                        \
             self._state = {loop_state};\n                        \
             continue;"
        )
    };

    states.push(GeneratorState {
        index: state_idx,
        code,
    });
}

/// Analyze sequential yield pattern (no loop).
fn analyze_sequential_pattern(stmts: &[ast::Stmt], transform: &Transform) -> Vec<GeneratorState> {
    let mut states = Vec::new();
    let mut current_code = String::new();
    let mut state_idx = 0u32;

    for stmt in stmts {
        if let ast::Stmt::Expr(expr) = stmt
            && let ast::ExprKind::Yield(ref inner) = expr.kind
        {
            let yield_val = emit_expr_to_string(inner, transform);
            let next_state = state_idx + 1;

            let code = if current_code.is_empty() {
                format!(
                    "self._state = {next_state};\n                        \
                         return Some({yield_val});"
                )
            } else {
                format!(
                    "{current_code}\n                        \
                         self._state = {next_state};\n                        \
                         return Some({yield_val});"
                )
            };

            states.push(GeneratorState {
                index: state_idx,
                code,
            });

            state_idx = next_state;
            current_code = String::new();
            continue;
        }

        let code = emit_stmt_to_string(stmt, transform);
        if !current_code.is_empty() {
            current_code.push_str("\n                        ");
        }
        current_code.push_str(&code);
    }

    // Final state returns None (generator exhausted)
    let final_code = if current_code.is_empty() {
        "return None;".to_owned()
    } else {
        format!("{current_code}\n                        return None;")
    };
    states.push(GeneratorState {
        index: state_idx,
        code: final_code,
    });

    states
}

/// Format a list of states into a loop/match state machine body.
fn format_state_machine(states: &[GeneratorState]) -> String {
    use std::fmt::Write;
    let mut arms = String::new();
    for state in states {
        let _ = write!(
            arms,
            "\n                    {} => {{\n                        {}\n                    }}",
            state.index, state.code,
        );
    }

    format!(
        "loop {{\n                match self._state {{{arms}\n                    _ => return None,\n                }}\n            }}"
    )
}

/// Emit a simple expression as a Rust string.
///
/// This is a lightweight emitter used only for generator state machine bodies.
/// It handles the common cases found in generator expressions.
#[allow(clippy::only_used_in_recursion)]
// The transform parameter is threaded for potential future use (e.g., generator call rewriting)
pub(super) fn emit_expr_to_string(expr: &ast::Expr, transform: &Transform) -> String {
    match &expr.kind {
        ast::ExprKind::Ident(ident) => format!("self.{}", ident.name),
        ast::ExprKind::IntLit(v) => v.to_string(),
        ast::ExprKind::FloatLit(v) => format!("{v:?}"),
        ast::ExprKind::BoolLit(b) => b.to_string(),
        ast::ExprKind::StringLit(s) => format!("\"{s}\".to_string()"),
        ast::ExprKind::Binary(bin) => {
            let left = emit_expr_to_string(&bin.left, transform);
            let right = emit_expr_to_string(&bin.right, transform);
            let op = match bin.op {
                ast::BinaryOp::Sub => "-",
                ast::BinaryOp::Mul => "*",
                ast::BinaryOp::Div => "/",
                ast::BinaryOp::Mod => "%",
                ast::BinaryOp::Eq => "==",
                ast::BinaryOp::Ne => "!=",
                ast::BinaryOp::Lt => "<",
                ast::BinaryOp::Gt => ">",
                ast::BinaryOp::Le => "<=",
                ast::BinaryOp::Ge => ">=",
                ast::BinaryOp::And => "&&",
                ast::BinaryOp::Or => "||",
                // Add and unsupported bitwise/shift ops default to "+"
                ast::BinaryOp::Add
                | ast::BinaryOp::Pow
                | ast::BinaryOp::BitAnd
                | ast::BinaryOp::BitOr
                | ast::BinaryOp::BitXor
                | ast::BinaryOp::Shl
                | ast::BinaryOp::Shr
                | ast::BinaryOp::In
                | ast::BinaryOp::InstanceOf => "+",
            };
            format!("{left} {op} {right}")
        }
        ast::ExprKind::Unary(u) => {
            let operand = emit_expr_to_string(&u.operand, transform);
            match u.op {
                ast::UnaryOp::Neg => format!("-{operand}"),
                ast::UnaryOp::Not | ast::UnaryOp::BitNot => format!("!{operand}"),
            }
        }
        ast::ExprKind::Assign(assign) => {
            let value = emit_expr_to_string(&assign.value, transform);
            format!("self.{} = {value}", assign.target.name)
        }
        ast::ExprKind::Call(call) => {
            let args: Vec<String> = call
                .args
                .iter()
                .map(|a| emit_expr_to_string(a, transform))
                .collect();
            format!("{}({})", call.callee.name, args.join(", "))
        }
        ast::ExprKind::Paren(inner) => {
            format!("({})", emit_expr_to_string(inner, transform))
        }
        _ => "/* unsupported expr */".to_owned(),
    }
}

/// Emit a simple statement as a Rust string for generator state machine bodies.
pub(super) fn emit_stmt_to_string(stmt: &ast::Stmt, transform: &Transform) -> String {
    match stmt {
        ast::Stmt::Expr(expr) => {
            // Check for compound assignments like `x += 1`
            if let ast::ExprKind::Assign(ref assign) = expr.kind {
                let value = emit_expr_to_string(&assign.value, transform);
                // Check for compound assignment pattern: x = x + val
                if let ast::ExprKind::Binary(ref bin) = assign.value.kind
                    && let ast::ExprKind::Ident(ref lhs_ident) = bin.left.kind
                    && lhs_ident.name == assign.target.name
                {
                    let rhs = emit_expr_to_string(&bin.right, transform);
                    let op = match bin.op {
                        ast::BinaryOp::Add => "+=",
                        ast::BinaryOp::Sub => "-=",
                        ast::BinaryOp::Mul => "*=",
                        ast::BinaryOp::Div => "/=",
                        ast::BinaryOp::Mod => "%=",
                        _ => return format!("self.{} = {value};", assign.target.name),
                    };
                    return format!("self.{} {op} {rhs};", assign.target.name);
                }
                return format!("self.{} = {value};", assign.target.name);
            }
            let code = emit_expr_to_string(expr, transform);
            format!("{code};")
        }
        ast::Stmt::VarDecl(vd) => {
            let init = emit_expr_to_string(&vd.init, transform);
            format!("self.{} = {init};", vd.name.name)
        }
        ast::Stmt::Return(ret) => {
            if let Some(ref val) = ret.value {
                format!("return Some({});", emit_expr_to_string(val, transform))
            } else {
                "return None;".to_owned()
            }
        }
        _ => "/* unsupported stmt */".to_owned(),
    }
}
