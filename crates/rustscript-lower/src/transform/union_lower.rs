//! Union type handling.
//!
//! Manages registration and scanning of union types encountered during lowering.
//! Union types in `RustScript` (e.g., `string | number`) are lowered to Rust enums
//! with `From` impls. This module handles discovering union types in annotations
//! throughout the module, registering them in the `UnionRegistry`, and recursively
//! scanning nested types and statement blocks for additional unions.

use rustscript_syntax::ast;
use rustscript_syntax::diagnostic::Diagnostic;
use rustscript_syntax::rust_ir::RustType;

use rustscript_typeck::resolve;

use super::Transform;
use super::type_lower::collect_generic_param_names;

impl Transform {
    /// Recursively register any generated union types found within a `RustType`.
    pub(super) fn register_union_type(&mut self, ty: &RustType) {
        match ty {
            RustType::GeneratedUnion { name, variants } => {
                self.union_registry.register(name, variants);
                for (_, inner_ty) in variants {
                    self.register_union_type(inner_ty);
                }
            }
            RustType::Option(inner) | RustType::ArcMutex(inner) | RustType::Slice(inner) => {
                self.register_union_type(inner);
            }
            RustType::Result(ok, err) => {
                self.register_union_type(ok);
                self.register_union_type(err);
            }
            RustType::Generic(base, args) => {
                self.register_union_type(base);
                for arg in args {
                    self.register_union_type(arg);
                }
            }
            RustType::Tuple(types) => {
                for ty in types {
                    self.register_union_type(ty);
                }
            }
            _ => {}
        }
    }

    /// Resolve a type annotation and register any union types it contains.
    pub(super) fn resolve_and_register_type(
        &mut self,
        ann: &ast::TypeAnnotation,
        generic_names: &[String],
        diags: &mut Vec<Diagnostic>,
    ) -> RustType {
        let ty_inner = resolve::resolve_type_annotation_with_generics(
            ann,
            &self.type_registry,
            generic_names,
            diags,
        );
        let rust_ty = rustscript_typeck::bridge::type_to_rust_type(&ty_inner);
        self.register_union_type(&rust_ty);
        rust_ty
    }

    /// Pre-pass: scan all type annotations in the module and register any
    /// general union types. This ensures enum definitions are generated before
    /// the functions that use them.
    pub(super) fn register_union_types_in_module(&mut self, module: &ast::Module) {
        for item in &module.items {
            if let ast::ItemKind::Function(f) = &item.kind {
                let generic_names = collect_generic_param_names(f.type_params.as_ref());
                let mut diags = Vec::new();
                // Scan parameter types
                for p in &f.params {
                    self.resolve_and_register_type(&p.type_ann, &generic_names, &mut diags);
                }
                // Scan return type
                if let Some(rt) = &f.return_type
                    && let Some(ann) = &rt.type_ann
                {
                    self.resolve_and_register_type(ann, &generic_names, &mut diags);
                }
                // Scan variable declarations in function body
                self.scan_stmts_for_unions(&f.body.stmts, &generic_names);
            }
        }
    }

    /// Recursively scan statements for union type annotations to register.
    pub(super) fn scan_stmts_for_unions(&mut self, stmts: &[ast::Stmt], generic_names: &[String]) {
        for stmt in stmts {
            match stmt {
                ast::Stmt::VarDecl(decl) => {
                    if let Some(ann) = &decl.type_ann {
                        let mut diags = Vec::new();
                        self.resolve_and_register_type(ann, generic_names, &mut diags);
                    }
                }
                ast::Stmt::Using(decl) => {
                    if let Some(ann) = &decl.type_ann {
                        let mut diags = Vec::new();
                        self.resolve_and_register_type(ann, generic_names, &mut diags);
                    }
                }
                ast::Stmt::If(if_stmt) => {
                    self.scan_stmts_for_unions(&if_stmt.then_block.stmts, generic_names);
                    if let Some(else_clause) = &if_stmt.else_clause {
                        match else_clause {
                            ast::ElseClause::Block(block) => {
                                self.scan_stmts_for_unions(&block.stmts, generic_names);
                            }
                            ast::ElseClause::ElseIf(nested_if) => {
                                self.scan_stmts_for_unions(
                                    &nested_if.then_block.stmts,
                                    generic_names,
                                );
                            }
                        }
                    }
                }
                ast::Stmt::While(w) => {
                    self.scan_stmts_for_unions(&w.body.stmts, generic_names);
                }
                ast::Stmt::DoWhile(dw) => {
                    self.scan_stmts_for_unions(&dw.body.stmts, generic_names);
                }
                ast::Stmt::For(f) => {
                    self.scan_stmts_for_unions(&f.body.stmts, generic_names);
                }
                ast::Stmt::ForIn(f) => {
                    self.scan_stmts_for_unions(&f.body.stmts, generic_names);
                }
                ast::Stmt::ForClassic(fc) => {
                    self.scan_stmts_for_unions(&fc.body.stmts, generic_names);
                }
                _ => {}
            }
        }
    }
}
