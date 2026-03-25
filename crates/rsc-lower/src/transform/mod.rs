//! AST-to-IR transformation.
//!
//! Consumes the `RustScript` AST and produces Rust IR, using the types,
//! ownership, and builtins modules for type resolution, clone insertion,
//! and builtin method lowering respectively.

mod async_lower;
mod class_lower;
mod expr_lower;
mod import_lower;
mod match_lower;
mod stmt_lower;
mod use_collector;

use std::collections::HashSet;

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{
    ParamMode, RustAttribute, RustBinaryOp, RustCompoundAssignOp, RustEnumDef, RustEnumVariant,
    RustFieldDef, RustFile, RustFnDecl, RustItem, RustParam, RustStructDef, RustTraitDef,
    RustTraitMethod, RustType, RustTypeParam, RustUnaryOp, RustUseDecl,
};

use crate::CrateDependency;
use crate::builtins::BuiltinRegistry;
use crate::context::LoweringContext;
use crate::derive_inference;
use crate::ownership::{self, UseMap};
use rsc_typeck::registry::TypeRegistry;
use rsc_typeck::resolve;
use rsc_typeck::types::Type;

/// Information about a function's signature, collected in a pre-pass.
///
/// Used by call-site lowering to determine whether to insert `?`, and
/// by Tier 2 ownership inference to record per-parameter borrow modes.
#[derive(Debug, Clone)]
struct FnSignature {
    /// Whether this function has a `throws` annotation.
    throws: bool,
    /// Resolved parameter types for enum variant resolution at call sites.
    param_types: Vec<RustType>,
    /// Inferred parameter modes from Tier 2 borrow analysis.
    /// `None` means analysis hasn't run (e.g., external functions).
    param_modes: Option<Vec<ParamMode>>,
}

/// Map from function name to its throws signature.
type FunctionSignatureMap = std::collections::HashMap<String, FnSignature>;

/// The AST-to-IR transformer.
///
/// Holds the builtin registry and type registry, and drives the lowering of
/// an entire module.
pub(crate) struct Transform {
    builtins: BuiltinRegistry,
    type_registry: TypeRegistry,
    /// Function signature map for `throws` detection during lowering.
    fn_signatures: FunctionSignatureMap,
}

impl Transform {
    /// Create a new transformer with the default builtin registry and an empty
    /// type registry.
    pub fn new() -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
            type_registry: TypeRegistry::new(),
            fn_signatures: FunctionSignatureMap::new(),
        }
    }

    /// Lower a complete `RustScript` module to a Rust file.
    ///
    /// Performs a pre-pass to register all type definitions, then lowers
    /// each item. Returns the Rust IR, diagnostics, and any external crate
    /// dependencies discovered from import statements.
    pub fn lower_module(
        &mut self,
        module: &ast::Module,
    ) -> (RustFile, Vec<Diagnostic>, HashSet<CrateDependency>, bool) {
        let mut ctx = LoweringContext::new();

        // Pre-pass: register all type definitions so they can be resolved
        // during function lowering.
        for item in &module.items {
            match &item.kind {
                ast::ItemKind::TypeDef(td) => self.register_type_def(td, &mut ctx),
                ast::ItemKind::EnumDef(ed) => self.register_enum_def(ed, &mut ctx),
                ast::ItemKind::Interface(iface) => self.register_interface_def(iface, &mut ctx),
                ast::ItemKind::Class(cls) => self.register_class_def(cls, &mut ctx),
                ast::ItemKind::Function(_)
                | ast::ItemKind::Import(_)
                | ast::ItemKind::ReExport(_)
                | ast::ItemKind::RustBlock(_) => {}
            }
        }

        // Pre-pass: collect function signatures for throws detection
        for item in &module.items {
            if let ast::ItemKind::Function(f) = &item.kind {
                self.register_fn_signature(f, &mut ctx);
            }
        }

        let mut items: Vec<RustItem> = Vec::new();
        let mut import_uses: Vec<RustUseDecl> = Vec::new();
        let mut crate_deps: HashSet<CrateDependency> = HashSet::new();
        let mut needs_async_runtime = async_lower::module_needs_async_runtime(module);

        for item in &module.items {
            let exported = item.exported;
            match &item.kind {
                ast::ItemKind::Function(f) => {
                    if f.is_async {
                        needs_async_runtime = true;
                    }
                    let mut lowered = self.lower_fn(f, &mut ctx);
                    lowered.public = exported;
                    items.push(RustItem::Function(lowered));
                }
                ast::ItemKind::TypeDef(td) => {
                    let mut lowered = self.lower_type_def(td, &mut ctx);
                    lowered.public = exported;
                    items.push(RustItem::Struct(lowered));
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
                    import_lower::classify_import(
                        &import.source.value,
                        &import.names,
                        false,
                        import.span,
                        &mut import_uses,
                        &mut crate_deps,
                    );
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
                }
                ast::ItemKind::RustBlock(rb) => {
                    items.push(RustItem::RawRust(rb.code.clone()));
                }
            }
        }

        // Collect use declarations by scanning generated items for HashMap/HashSet usage
        let mut uses = use_collector::collect_use_declarations(&items);
        // Prepend import-derived use declarations
        import_uses.append(&mut uses);
        // Deduplicate use declarations by path to avoid duplicate imports
        // (e.g., when an import and new X() both generate a use for the same type)
        let mut seen_paths = std::collections::HashSet::new();
        import_uses.retain(|u| seen_paths.insert(u.path.clone()));
        let uses = import_uses;

        let diagnostics = ctx.into_diagnostics();
        (
            RustFile {
                uses,
                mod_decls: Vec::new(),
                items,
            },
            diagnostics,
            crate_deps,
            needs_async_runtime,
        )
    }

    /// Register a type definition in the type registry during the pre-pass.
    fn register_type_def(&mut self, td: &ast::TypeDef, ctx: &mut LoweringContext) {
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
                (f.name.name.clone(), ty)
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        self.type_registry.register(td.name.name.clone(), fields);
    }

    /// Lower a type definition to a Rust struct.
    fn lower_type_def(&self, td: &ast::TypeDef, ctx: &mut LoweringContext) -> RustStructDef {
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
                let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                RustFieldDef {
                    public: true,
                    name: f.name.name.clone(),
                    ty: rust_ty,
                    span: Some(f.span),
                }
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        let field_types: Vec<&RustType> = fields.iter().map(|f| &f.ty).collect();
        let has_type_params = !type_params.is_empty();
        let derives = derive_inference::infer_struct_derives(&field_types, has_type_params);
        RustStructDef {
            public: false,
            name: td.name.name.clone(),
            type_params,
            fields,
            derives,
            span: Some(td.span),
        }
    }

    /// Register an enum definition in the type registry during the pre-pass.
    fn register_enum_def(&mut self, ed: &ast::EnumDef, ctx: &mut LoweringContext) {
        // Determine if simple or data enum
        let is_data = ed
            .variants
            .iter()
            .any(|v| matches!(v, ast::EnumVariant::Data { .. }));

        if is_data {
            let mut diags = Vec::new();
            let variants: Vec<(String, Vec<(String, rsc_typeck::types::Type)>)> = ed
                .variants
                .iter()
                .filter_map(|v| match v {
                    ast::EnumVariant::Data { name, fields, .. } => {
                        let field_types: Vec<(String, rsc_typeck::types::Type)> = fields
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
                    ast::EnumVariant::Data { .. } => None,
                })
                .collect();
            self.type_registry
                .register_simple_enum(ed.name.name.clone(), variants);
        }
    }

    /// Register an interface definition in the type registry during the pre-pass.
    fn register_interface_def(&mut self, iface: &ast::InterfaceDef, ctx: &mut LoweringContext) {
        let mut diags = Vec::new();
        let generic_names = collect_generic_param_names(iface.type_params.as_ref());
        let methods: Vec<rsc_typeck::registry::InterfaceMethodSig> = iface
            .methods
            .iter()
            .map(|m| {
                let param_types: Vec<(String, rsc_typeck::types::Type)> = m
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
                rsc_typeck::registry::InterfaceMethodSig {
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

    /// Lower an interface definition to a Rust trait.
    fn lower_interface_def(
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
                let params: Vec<RustParam> = m
                    .params
                    .iter()
                    .map(|p| {
                        let ty = resolve::resolve_type_annotation_with_generics(
                            &p.type_ann,
                            &self.type_registry,
                            &generic_names,
                            &mut diags,
                        );
                        let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                        RustParam {
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
                        rsc_typeck::bridge::type_to_rust_type(&ty)
                    })
                });

                RustTraitMethod {
                    name: m.name.name.clone(),
                    params,
                    return_type,
                    has_self: true, // All interface methods take &self
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
            span: Some(iface.span),
        }
    }

    /// Lower an enum definition to a Rust enum.
    fn lower_enum_def(&self, ed: &ast::EnumDef, ctx: &mut LoweringContext) -> RustEnumDef {
        let mut diags = Vec::new();
        let variants: Vec<RustEnumVariant> = ed
            .variants
            .iter()
            .map(|v| match v {
                ast::EnumVariant::Simple(ident, span) => RustEnumVariant {
                    name: ident.name.clone(),
                    fields: vec![],
                    span: Some(*span),
                },
                ast::EnumVariant::Data {
                    name, fields, span, ..
                } => {
                    let rust_fields = fields
                        .iter()
                        .map(|f| {
                            let ty = resolve::resolve_type_annotation_with_generics(
                                &f.type_ann,
                                &self.type_registry,
                                &[],
                                &mut diags,
                            );
                            let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                            RustFieldDef {
                                public: true,
                                name: f.name.name.clone(),
                                ty: rust_ty,
                                span: Some(f.span),
                            }
                        })
                        .collect();
                    RustEnumVariant {
                        name: name.name.clone(),
                        fields: rust_fields,
                        span: Some(*span),
                    }
                }
            })
            .collect();
        for d in diags {
            ctx.emit_diagnostic(d);
        }
        let derives = derive_inference::infer_enum_derives(&variants);
        RustEnumDef {
            public: false,
            name: ed.name.name.clone(),
            variants,
            derives,
            span: Some(ed.span),
        }
    }

    /// Register a function signature in the pre-pass for throws and parameter type detection.
    fn register_fn_signature(&mut self, f: &ast::FnDecl, _ctx: &mut LoweringContext) {
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
                let ty = rsc_typeck::resolve::resolve_type_annotation_with_registry(
                    &p.type_ann,
                    &self.type_registry,
                    &mut diags,
                );
                rsc_typeck::bridge::type_to_rust_type(&ty)
            })
            .collect();

        // Tier 2: analyze parameter usage to determine borrow modes
        let param_names: Vec<String> = f.params.iter().map(|p| p.name.name.clone()).collect();
        let builtins = &self.builtins;
        let usage_map = ownership::analyze_param_usage(&f.body, &param_names, |obj, method| {
            builtins.is_ref_args(obj, method)
        });
        let param_modes: Vec<ParamMode> = param_names
            .iter()
            .zip(param_types.iter())
            .map(|(name, ty)| {
                let usage = usage_map
                    .get(name.as_str())
                    .copied()
                    .unwrap_or(ownership::ParamUsage::ReadOnly);
                ownership::usage_to_mode(usage, ty)
            })
            .collect();

        self.fn_signatures.insert(
            f.name.name.clone(),
            FnSignature {
                throws,
                param_types,
                param_modes: Some(param_modes),
            },
        );
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

                let ty_inner = resolve::resolve_type_annotation_with_generics(
                    &p.type_ann,
                    &self.type_registry,
                    &generic_names,
                    &mut diags,
                );
                let ty = rsc_typeck::bridge::type_to_rust_type(&ty_inner);
                for d in diags {
                    ctx.emit_diagnostic(d);
                }
                ctx.declare_variable(p.name.name.clone(), ty.clone());

                // Use pre-computed param mode from Tier 2 analysis
                let mode = precomputed_modes
                    .and_then(|modes| modes.get(param_idx))
                    .copied()
                    .unwrap_or(ParamMode::Owned);

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
                let ty = rsc_typeck::bridge::type_to_rust_type(&ty_inner);
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
                        let ty = rsc_typeck::bridge::type_to_rust_type(&ty_inner);
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
        let body = self.lower_block(&f.body, ctx, &use_map, 0, &reassigned);

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
            span: Some(f.span),
        }
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
    }
}

/// Map a `RustScript` unary operator to a Rust unary operator.
fn lower_unary_op(op: ast::UnaryOp) -> RustUnaryOp {
    match op {
        ast::UnaryOp::Neg => RustUnaryOp::Neg,
        ast::UnaryOp::Not => RustUnaryOp::Not,
    }
}

/// Collect generic parameter names from an optional `TypeParams`.
///
/// Returns a `Vec<String>` of type parameter names (e.g., `["T", "U"]`).
/// Used to set up the generic scope during lowering.
fn collect_generic_param_names(type_params: Option<&ast::TypeParams>) -> Vec<String> {
    match type_params {
        Some(tp) => tp.params.iter().map(|p| p.name.name.clone()).collect(),
        None => Vec::new(),
    }
}

/// Lower AST type parameters to Rust IR type parameters.
///
/// Maps `T extends Bound` to `RustTypeParam { name: "T", bounds: vec!["Bound"] }`.
fn lower_type_params(type_params: Option<&ast::TypeParams>) -> Vec<RustTypeParam> {
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
                        | ast::TypeKind::Union(_)
                        | ast::TypeKind::Function(_, _)
                        | ast::TypeKind::Inferred
                        | ast::TypeKind::Shared(_) => vec![],
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

/// Check if the expression is a literal whose default inferred type matches `ty`.
///
/// When this returns `true`, the type annotation can be omitted on the `let`
/// binding because Rust will infer the same type.
/// Check whether a collection type's element type is `Copy`.
///
/// For `Vec<i32>` (represented as `Generic(Named("Vec"), [I32])`), returns true.
/// For `Vec<String>` or non-collection types, returns false.
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
        RustType::Generic(base, _) => {
            if let RustType::Named(name) = base.as_ref() {
                Some(name.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Capitalize the first letter of a string.
///
/// Used to derive Rust enum variant names from `RustScript` string literals.
fn capitalize_first(s: &str) -> String {
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
            name: ident(name, 0, name.len() as u32),
            type_params: None,
            params,
            return_type: return_type.map(|ann| ret_type(ann)),
            body: Block {
                stmts: body,
                span: span(0, 100),
            },
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
            span: span(0, 10),
        }
    }

    // Test 15: Lower empty function main()
    #[test]
    fn test_lower_empty_main_function() {
        let f = make_fn("main", vec![], None, vec![]);
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (_, diags, _, _) = transform.lower_module(&module);

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
            span: span(0, 55),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
            span: span(0, 68),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
            span: span(0, 63),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
            span: span(0, 52),
        };

        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);

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
            span: span(0, 50),
        };
        let module = Module {
            items: vec![Item {
                kind: ItemKind::TypeDef(td),
                exported: false,
                span: span(0, 50),
            }],
            span: span(0, 50),
        };
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
                    fields: vec![
                        ast::FieldInit {
                            name: ident("x", 0, 1),
                            value: Expr {
                                kind: ExprKind::FloatLit(1.0),
                                span: span(0, 3),
                            },
                            span: span(0, 4),
                        },
                        ast::FieldInit {
                            name: ident("y", 0, 1),
                            value: Expr {
                                kind: ExprKind::FloatLit(2.0),
                                span: span(0, 3),
                            },
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
                    span: span(0, 30),
                },
                fn_item(make_fn("main", vec![], None, body)),
            ],
            span: span(0, 100),
        };
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
        let mut transform = Transform::new();
        let (file, _diags, _, _) = transform.lower_module(&module);
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
            span: span(0, 30),
        };
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
                    span: span(0, 3),
                },
                Param {
                    name: ident("b", 0, 1),
                    type_ann: TypeAnnotation {
                        kind: TypeKind::Named(ident("T", 0, 1)),
                        span: span(0, 1),
                    },
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
            span: span(0, 50),
        };
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
            span: span(0, 30),
        };
        let module = make_module(vec![Item {
            kind: ItemKind::TypeDef(td),
            exported: false,
            span: span(0, 30),
        }]);
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);

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
                    span: span(0, 50),
                }),
                exported: false,
                span: span(0, 50),
            }],
            span: span(0, 50),
        };

        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);
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
                    span: span(0, 80),
                }),
                exported: false,
                span: span(0, 80),
            }],
            span: span(0, 80),
        };

        let mut transform = Transform::new();
        let (file, _, _, _) = transform.lower_module(&module);
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
                        int_expr(1, 0, 1),
                        int_expr(2, 3, 4),
                        int_expr(3, 6, 7),
                    ]),
                    span: span(0, 8),
                },
                span: span(0, 10),
            })],
        );
        let module = make_module(vec![fn_item(f)]);
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
        let mut transform = Transform::new();
        let (file, _diags, _, _) = transform.lower_module(&module);

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
            span: span(0, 26),
        })]);

        let mut transform = Transform::new();
        let (file, _diags, _, _) = transform.lower_module(&module);
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

        let mut transform = Transform::new();
        let (file, _diags, _, _) = transform.lower_module(&module);
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
            span: span(0, 29),
        })]);

        let mut transform = Transform::new();
        let (file, _diags, _, _) = transform.lower_module(&module);
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

        let mut transform = Transform::new();
        let (file, _diags, _, _) = transform.lower_module(&module);
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

        let mut transform = Transform::new();
        let (file, _diags, _, _) = transform.lower_module(&module);
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

        let mut transform = Transform::new();
        let (file, _diags, _, _) = transform.lower_module(&module);
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
            span: span(0, 50),
        };

        let outer_fn = FnDecl {
            is_async: false,
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
                    span: span(0, 37),
                }),
                exported: false,
                span: span(0, 37),
            }],
            span: span(0, 37),
        };

        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
                    span: span(0, 28),
                }),
                exported: false,
                span: span(0, 28),
            }],
            span: span(0, 28),
        };

        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
                        span: span(0, 37),
                    }),
                    exported: false,
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
                        span: span(40, 62),
                    }),
                    exported: false,
                    span: span(40, 62),
                },
                Item {
                    kind: ItemKind::Function(FnDecl {
                        is_async: false,
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
                        span: span(65, 150),
                    }),
                    exported: false,
                    span: span(65, 150),
                },
            ],
            span: span(0, 150),
        };

        let mut transform = Transform::new();
        let (file, diags, _, _) = transform.lower_module(&module);
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
        let mut transform = Transform::new();
        let (_, _, _, needs_async_runtime) = transform.lower_module(&module);
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
        let mut transform = Transform::new();
        let (_, _, _, needs_async_runtime) = transform.lower_module(&module);
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
                    RustExprKind::TokioJoin(elements) => {
                        assert_eq!(elements.len(), 2, "expected 2 futures in tokio::join!");
                        assert!(
                            matches!(&elements[0].kind, RustExprKind::Call { func, .. } if func == "getUser"),
                            "expected getUser call"
                        );
                        assert!(
                            matches!(&elements[1].kind, RustExprKind::Call { func, .. } if func == "getPosts"),
                            "expected getPosts call"
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
                        RustExprKind::TokioJoin(elements) => {
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
                        RustExprKind::TokioJoin(elements) => {
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
        let mut transform = Transform::new();
        let (_, _, _, needs_async_runtime) = transform.lower_module(&module);
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
        let mut transform = Transform::new();
        let (_, _, _, needs_async_runtime) = transform.lower_module(&module);
        assert!(
            needs_async_runtime,
            "expected needs_async_runtime to be true when Promise.all is used"
        );
    }
}
