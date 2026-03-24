//! Scope management for the lowering pass.
//!
//! Tracks variable declarations, their types, and mutability within
//! nested scopes. Does not contain type or ownership logic.

use std::collections::HashMap;

use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::RustType;

/// The lowering context, managing variable scopes and diagnostics.
///
/// Maintains a stack of scopes for nested blocks and accumulates
/// diagnostics encountered during lowering.
pub(crate) struct LoweringContext {
    scopes: Vec<Scope>,
    diagnostics: Vec<Diagnostic>,
}

/// A single scope level containing variable declarations.
pub(crate) struct Scope {
    variables: HashMap<String, VarInfo>,
}

/// Information about a declared variable.
pub(crate) struct VarInfo {
    /// The resolved Rust type of the variable.
    pub ty: RustType,
    /// Whether the variable is mutable (`let mut` vs `let`).
    ///
    /// Currently read only in tests; will be used by later phases
    /// for scope-aware mutability queries.
    #[cfg_attr(not(test), allow(dead_code))]
    pub mutable: bool,
}

impl LoweringContext {
    /// Create a new lowering context with an empty global scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope {
                variables: HashMap::new(),
            }],
            diagnostics: Vec::new(),
        }
    }

    /// Push a new scope onto the scope stack.
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope {
            variables: HashMap::new(),
        });
    }

    /// Pop the innermost scope from the scope stack.
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Declare a variable in the current (innermost) scope.
    pub fn declare_variable(&mut self, name: String, ty: RustType, mutable: bool) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.variables.insert(name, VarInfo { ty, mutable });
        }
    }

    /// Look up a variable by name, searching from innermost to outermost scope.
    pub fn lookup_variable(&self, name: &str) -> Option<&VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.variables.get(name) {
                return Some(info);
            }
        }
        None
    }

    /// Mark a variable as mutable in whatever scope it was declared in.
    ///
    /// Currently used only in tests; will be needed by later phases
    /// for scope-aware mutability updates.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn mark_mutable(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.variables.get_mut(name) {
                info.mutable = true;
                return;
            }
        }
    }

    /// Add a diagnostic to the accumulated list.
    pub fn emit_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Consume the context and return all accumulated diagnostics.
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_declare_and_lookup_variable() {
        let mut ctx = LoweringContext::new();
        ctx.declare_variable("x".to_owned(), RustType::I32, false);
        let info = ctx.lookup_variable("x");
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.ty, RustType::I32);
        assert!(!info.mutable);
    }

    #[test]
    fn test_context_lookup_missing_variable_returns_none() {
        let ctx = LoweringContext::new();
        assert!(ctx.lookup_variable("x").is_none());
    }

    #[test]
    fn test_context_nested_scope_shadows_outer() {
        let mut ctx = LoweringContext::new();
        ctx.declare_variable("x".to_owned(), RustType::I32, false);
        ctx.push_scope();
        ctx.declare_variable("x".to_owned(), RustType::String, true);
        let info = ctx.lookup_variable("x").unwrap();
        assert_eq!(info.ty, RustType::String);
        assert!(info.mutable);
        ctx.pop_scope();
        let info = ctx.lookup_variable("x").unwrap();
        assert_eq!(info.ty, RustType::I32);
        assert!(!info.mutable);
    }

    #[test]
    fn test_context_mark_mutable() {
        let mut ctx = LoweringContext::new();
        ctx.declare_variable("x".to_owned(), RustType::I64, false);
        assert!(!ctx.lookup_variable("x").unwrap().mutable);
        ctx.mark_mutable("x");
        assert!(ctx.lookup_variable("x").unwrap().mutable);
    }

    #[test]
    fn test_context_emit_diagnostic() {
        let mut ctx = LoweringContext::new();
        ctx.emit_diagnostic(Diagnostic::error("test error"));
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "test error");
    }
}
