//! Scope management for the lowering pass.
//!
//! Tracks variable declarations, their types, and mutability within
//! nested scopes. Does not contain type or ownership logic.

use std::collections::{HashMap, HashSet};

use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::RustType;

/// The lowering context, managing variable scopes and diagnostics.
///
/// Maintains a stack of scopes for nested blocks and accumulates
/// diagnostics encountered during lowering.
pub(crate) struct LoweringContext {
    scopes: Vec<Scope>,
    diagnostics: Vec<Diagnostic>,
    /// The current expected struct type name for struct literal resolution.
    /// Set by `lower_var_decl` when the variable has a named type annotation.
    current_struct_type: Option<String>,
    /// The current function's return type, if it's `Option<T>`.
    /// Set during function lowering when the return type is `T | null`.
    current_return_type: Option<RustType>,
    /// Whether the current function is a `throws` function.
    /// Used to determine whether to wrap returns in `Ok()` and insert `?`.
    current_fn_throws: bool,
    /// Variables that are references (e.g., for-of loop variables, iterator
    /// closure parameters). Any use that requires ownership (return, function
    /// call with owned param, struct field assignment) should auto-clone.
    reference_variables: HashSet<String>,
}

/// A single scope level containing variable declarations.
pub(crate) struct Scope {
    variables: HashMap<String, VarInfo>,
}

/// Information about a declared variable.
pub(crate) struct VarInfo {
    /// The resolved Rust type of the variable.
    pub ty: RustType,
}

impl LoweringContext {
    /// Create a new lowering context with an empty global scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope {
                variables: HashMap::new(),
            }],
            diagnostics: Vec::new(),
            current_struct_type: None,
            current_return_type: None,
            current_fn_throws: false,
            reference_variables: HashSet::new(),
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
    pub fn declare_variable(&mut self, name: String, ty: RustType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.variables.insert(name, VarInfo { ty });
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

    /// Set the current struct type name context for struct literal resolution.
    pub fn set_struct_type_name(&mut self, name: Option<String>) {
        self.current_struct_type = name;
    }

    /// Get the current struct type name context, if set.
    pub fn current_struct_type_name(&self) -> Option<&str> {
        self.current_struct_type.as_deref()
    }

    /// Set the current function's return type.
    pub fn set_return_type(&mut self, ty: Option<RustType>) {
        self.current_return_type = ty;
    }

    /// Get the current function's return type, if set.
    pub fn current_return_type(&self) -> Option<&RustType> {
        self.current_return_type.as_ref()
    }

    /// Set whether the current function is a `throws` function.
    pub fn set_fn_throws(&mut self, throws: bool) {
        self.current_fn_throws = throws;
    }

    /// Check whether the current function is a `throws` function.
    pub fn is_fn_throws(&self) -> bool {
        self.current_fn_throws
    }

    /// Mark a variable as a reference (e.g., for-of loop variable).
    ///
    /// Variables marked as references will be auto-cloned when used in
    /// owned contexts (return, function call with owned param).
    pub fn mark_as_reference(&mut self, name: String) {
        self.reference_variables.insert(name);
    }

    /// Remove a variable from the reference set (e.g., when leaving a for-of scope).
    pub fn unmark_reference(&mut self, name: &str) {
        self.reference_variables.remove(name);
    }

    /// Check whether a variable is currently a reference.
    pub fn is_reference_variable(&self, name: &str) -> bool {
        self.reference_variables.contains(name)
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
        ctx.declare_variable("x".to_owned(), RustType::I32);
        let info = ctx.lookup_variable("x");
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.ty, RustType::I32);
    }

    #[test]
    fn test_context_lookup_missing_variable_returns_none() {
        let ctx = LoweringContext::new();
        assert!(ctx.lookup_variable("x").is_none());
    }

    #[test]
    fn test_context_nested_scope_shadows_outer() {
        let mut ctx = LoweringContext::new();
        ctx.declare_variable("x".to_owned(), RustType::I32);
        ctx.push_scope();
        ctx.declare_variable("x".to_owned(), RustType::String);
        let info = ctx.lookup_variable("x").unwrap();
        assert_eq!(info.ty, RustType::String);
        ctx.pop_scope();
        let info = ctx.lookup_variable("x").unwrap();
        assert_eq!(info.ty, RustType::I32);
    }

    #[test]
    fn test_context_emit_diagnostic() {
        let mut ctx = LoweringContext::new();
        ctx.emit_diagnostic(Diagnostic::error("test error"));
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "test error");
    }

    #[test]
    fn test_context_mark_and_check_reference_variable() {
        let mut ctx = LoweringContext::new();
        assert!(!ctx.is_reference_variable("u"));
        ctx.mark_as_reference("u".to_owned());
        assert!(ctx.is_reference_variable("u"));
    }

    #[test]
    fn test_context_unmark_reference_variable() {
        let mut ctx = LoweringContext::new();
        ctx.mark_as_reference("u".to_owned());
        assert!(ctx.is_reference_variable("u"));
        ctx.unmark_reference("u");
        assert!(!ctx.is_reference_variable("u"));
    }

    #[test]
    fn test_context_reference_variables_independent_of_scope() {
        let mut ctx = LoweringContext::new();
        ctx.mark_as_reference("u".to_owned());
        ctx.push_scope();
        // Reference tracking is global, not scoped
        assert!(ctx.is_reference_variable("u"));
        ctx.pop_scope();
        assert!(ctx.is_reference_variable("u"));
    }
}
