//! Ownership analysis for clone insertion and borrow inference.
//!
//! Implements last-use analysis (Tier 1) to determine when a `String`-typed
//! variable needs to be cloned at a move point, and parameter borrow analysis
//! (Tier 2) to determine which function parameters can safely be borrowed
//! instead of owned.

use std::collections::{HashMap, HashSet};

use rsc_syntax::ast;
use rsc_syntax::rust_ir::{ParamMode, RustType};

/// A record of where a variable is used within a function body.
pub(crate) struct UseLocation {
    /// Index of the statement within the enclosing block.
    pub stmt_index: usize,
    /// Whether this use is in a "move position" (function call argument
    /// where the callee takes ownership). `println!` args are NOT move
    /// positions because `println!` takes references.
    pub is_move_position: bool,
}

/// Map from variable name to ordered list of use locations.
pub(crate) struct UseMap {
    uses: HashMap<String, Vec<UseLocation>>,
}

impl UseMap {
    /// Build a `UseMap` by scanning a function body.
    ///
    /// `is_ref_call` is a predicate that returns true if a method call's
    /// arguments are passed by reference (not moved). This decouples ownership
    /// analysis from the builtin registry.
    ///
    /// `callee_param_modes` returns the parameter modes for a callee by name.
    /// When a callee's parameter is `Borrowed` or `BorrowedStr`, passing an
    /// argument to that position is NOT a move — eliminating unnecessary clones.
    pub fn analyze<'a>(
        body: &ast::Block,
        is_ref_call: impl Fn(&str, &str) -> bool,
        callee_param_modes: impl Fn(&str) -> Option<&'a [ParamMode]>,
    ) -> Self {
        let mut uses: HashMap<String, Vec<UseLocation>> = HashMap::new();

        for (stmt_index, stmt) in body.stmts.iter().enumerate() {
            Self::collect_stmt_uses(
                stmt,
                stmt_index,
                &is_ref_call,
                &callee_param_modes,
                &mut uses,
            );
        }

        Self { uses }
    }

    /// Look up all uses of a variable.
    pub fn get_uses(&self, var_name: &str) -> Option<&Vec<UseLocation>> {
        self.uses.get(var_name)
    }

    /// Collect variable uses from a statement.
    #[allow(clippy::too_many_lines)]
    // Statement scanning covers all statement kinds with callee param mode threading
    fn collect_stmt_uses<'a>(
        stmt: &ast::Stmt,
        stmt_index: usize,
        is_ref_call: &impl Fn(&str, &str) -> bool,
        callee_param_modes: &impl Fn(&str) -> Option<&'a [ParamMode]>,
        uses: &mut HashMap<String, Vec<UseLocation>>,
    ) {
        match stmt {
            ast::Stmt::VarDecl(decl) => {
                // The initializer expression may reference variables
                Self::collect_expr_uses(
                    &decl.init,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::Stmt::Expr(expr) => {
                Self::collect_expr_uses(
                    expr,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::Stmt::Return(ret) => {
                if let Some(value) = &ret.value {
                    Self::collect_expr_uses(
                        value,
                        stmt_index,
                        false,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::Stmt::If(if_stmt) => {
                Self::collect_if_uses(if_stmt, stmt_index, is_ref_call, callee_param_modes, uses);
            }
            ast::Stmt::While(while_stmt) => {
                Self::collect_expr_uses(
                    &while_stmt.condition,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                for inner_stmt in &while_stmt.body.stmts {
                    Self::collect_stmt_uses(
                        inner_stmt,
                        stmt_index,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::Stmt::Destructure(destr) => {
                Self::collect_expr_uses(
                    &destr.init,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::Stmt::Switch(switch) => {
                Self::collect_expr_uses(
                    &switch.scrutinee,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                for case in &switch.cases {
                    for inner_stmt in &case.body {
                        Self::collect_stmt_uses(
                            inner_stmt,
                            stmt_index,
                            is_ref_call,
                            callee_param_modes,
                            uses,
                        );
                    }
                }
            }
            ast::Stmt::TryCatch(tc) => {
                for inner_stmt in &tc.try_block.stmts {
                    Self::collect_stmt_uses(
                        inner_stmt,
                        stmt_index,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
                for inner_stmt in &tc.catch_block.stmts {
                    Self::collect_stmt_uses(
                        inner_stmt,
                        stmt_index,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::Stmt::For(for_of) => {
                // The iterable is borrowed, not moved — mark as non-move position
                Self::collect_expr_uses(
                    &for_of.iterable,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                for inner_stmt in &for_of.body.stmts {
                    Self::collect_stmt_uses(
                        inner_stmt,
                        stmt_index,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::Stmt::ArrayDestructure(adestr) => {
                Self::collect_expr_uses(
                    &adestr.init,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::Stmt::Break(_) | ast::Stmt::Continue(_) | ast::Stmt::RustBlock(_) => {
                // No variable uses in break/continue/inline rust
            }
        }
    }

    /// Collect uses from an if statement.
    fn collect_if_uses<'a>(
        if_stmt: &ast::IfStmt,
        stmt_index: usize,
        is_ref_call: &impl Fn(&str, &str) -> bool,
        callee_param_modes: &impl Fn(&str) -> Option<&'a [ParamMode]>,
        uses: &mut HashMap<String, Vec<UseLocation>>,
    ) {
        Self::collect_expr_uses(
            &if_stmt.condition,
            stmt_index,
            false,
            is_ref_call,
            callee_param_modes,
            uses,
        );
        for inner_stmt in &if_stmt.then_block.stmts {
            Self::collect_stmt_uses(
                inner_stmt,
                stmt_index,
                is_ref_call,
                callee_param_modes,
                uses,
            );
        }
        if let Some(else_clause) = &if_stmt.else_clause {
            match else_clause {
                ast::ElseClause::Block(block) => {
                    for inner_stmt in &block.stmts {
                        Self::collect_stmt_uses(
                            inner_stmt,
                            stmt_index,
                            is_ref_call,
                            callee_param_modes,
                            uses,
                        );
                    }
                }
                ast::ElseClause::ElseIf(nested_if) => {
                    Self::collect_if_uses(
                        nested_if,
                        stmt_index,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
        }
    }

    /// Collect variable uses from an expression.
    ///
    /// `in_move_position` is true when this expression is a function call
    /// argument in a non-ref call.
    #[allow(clippy::too_many_lines)]
    // Expression scanning covers all expression kinds; splitting would obscure the match
    fn collect_expr_uses<'a>(
        expr: &ast::Expr,
        stmt_index: usize,
        in_move_position: bool,
        is_ref_call: &impl Fn(&str, &str) -> bool,
        callee_param_modes: &impl Fn(&str) -> Option<&'a [ParamMode]>,
        uses: &mut HashMap<String, Vec<UseLocation>>,
    ) {
        match &expr.kind {
            ast::ExprKind::Ident(ident) => {
                uses.entry(ident.name.clone())
                    .or_default()
                    .push(UseLocation {
                        stmt_index,
                        is_move_position: in_move_position,
                    });
            }
            ast::ExprKind::Binary(bin) => {
                Self::collect_expr_uses(
                    &bin.left,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                Self::collect_expr_uses(
                    &bin.right,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::ExprKind::Unary(un) => {
                Self::collect_expr_uses(
                    &un.operand,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::ExprKind::Call(call) => {
                // Tier 2: check callee's param modes to determine move positions
                let modes = callee_param_modes(&call.callee.name);
                for (i, arg) in call.args.iter().enumerate() {
                    let is_borrowed = modes
                        .and_then(|m| m.get(i))
                        .is_some_and(|m| matches!(m, ParamMode::Borrowed | ParamMode::BorrowedStr));
                    let is_move = !is_borrowed;
                    Self::collect_expr_uses(
                        arg,
                        stmt_index,
                        is_move,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::ExprKind::MethodCall(mc) => {
                // Check if this is a builtin that takes refs
                let obj_name = match &mc.object.kind {
                    ast::ExprKind::Ident(ident) => Some(ident.name.as_str()),
                    _ => None,
                };
                let is_ref = obj_name.is_some_and(|obj| is_ref_call(obj, &mc.method.name));
                let arg_move = !is_ref;

                // The object itself may be a variable reference
                Self::collect_expr_uses(
                    &mc.object,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                for arg in &mc.args {
                    Self::collect_expr_uses(
                        arg,
                        stmt_index,
                        arg_move,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::ExprKind::Paren(inner) => {
                Self::collect_expr_uses(
                    inner,
                    stmt_index,
                    in_move_position,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::ExprKind::Assign(assign) => {
                // The target is not a "use" in the ownership sense (it's being written to)
                // The value side may reference variables
                Self::collect_expr_uses(
                    &assign.value,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::ExprKind::StructLit(slit) => {
                for field in &slit.fields {
                    Self::collect_expr_uses(
                        &field.value,
                        stmt_index,
                        false,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::ExprKind::FieldAccess(fa) => {
                Self::collect_expr_uses(
                    &fa.object,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::ExprKind::TemplateLit(tpl) => {
                for part in &tpl.parts {
                    if let ast::TemplatePart::Expr(e) = part {
                        Self::collect_expr_uses(
                            e,
                            stmt_index,
                            false,
                            is_ref_call,
                            callee_param_modes,
                            uses,
                        );
                    }
                }
            }
            ast::ExprKind::ArrayLit(elements) => {
                for elem in elements {
                    Self::collect_expr_uses(
                        elem,
                        stmt_index,
                        false,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::ExprKind::New(new_expr) => {
                for arg in &new_expr.args {
                    Self::collect_expr_uses(
                        arg,
                        stmt_index,
                        false,
                        is_ref_call,
                        callee_param_modes,
                        uses,
                    );
                }
            }
            ast::ExprKind::Index(index_expr) => {
                Self::collect_expr_uses(
                    &index_expr.object,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                Self::collect_expr_uses(
                    &index_expr.index,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::ExprKind::IntLit(_)
            | ast::ExprKind::FloatLit(_)
            | ast::ExprKind::StringLit(_)
            | ast::ExprKind::BoolLit(_)
            | ast::ExprKind::NullLit
            | ast::ExprKind::This
            | ast::ExprKind::Closure(_) => {
                // Closure bodies and `this` are opaque for ownership analysis —
                // variables captured by a closure are not tracked in the
                // outer function's use map. This is the conservative Phase 1
                // approach per the task spec.
            }
            ast::ExprKind::FieldAssign(fa) => {
                Self::collect_expr_uses(
                    &fa.object,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                Self::collect_expr_uses(
                    &fa.value,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::ExprKind::OptionalChain(chain) => {
                Self::collect_expr_uses(
                    &chain.object,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                match &chain.access {
                    ast::OptionalAccess::Field(_) => {}
                    ast::OptionalAccess::Method(_, args) => {
                        for arg in args {
                            Self::collect_expr_uses(
                                arg,
                                stmt_index,
                                false,
                                is_ref_call,
                                callee_param_modes,
                                uses,
                            );
                        }
                    }
                }
            }
            ast::ExprKind::NullishCoalescing(nc) => {
                Self::collect_expr_uses(
                    &nc.left,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
                Self::collect_expr_uses(
                    &nc.right,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
            ast::ExprKind::Throw(inner)
            | ast::ExprKind::Await(inner)
            | ast::ExprKind::Shared(inner) => {
                Self::collect_expr_uses(
                    inner,
                    stmt_index,
                    false,
                    is_ref_call,
                    callee_param_modes,
                    uses,
                );
            }
        }
    }
}

/// Determine whether a variable reference at the given position needs cloning.
///
/// A clone is needed when:
/// 1. The variable's type is not `Copy` (i.e., it's `String`)
/// 2. The current use is in a move position
/// 3. There exists a later use of the same variable
pub(crate) fn needs_clone(
    var_name: &str,
    current_stmt_index: usize,
    use_map: &UseMap,
    var_type: &RustType,
) -> bool {
    // Copy types never need cloning
    if is_copy_type(var_type) {
        return false;
    }

    // Look up the current use in the map to check if it's a move position
    let Some(locations) = use_map.get_uses(var_name) else {
        return false;
    };

    // Find the current use — we need to check if IT is a move position
    let current_is_move = locations
        .iter()
        .any(|loc| loc.stmt_index == current_stmt_index && loc.is_move_position);

    if !current_is_move {
        return false;
    }

    // Check if there are any later uses
    locations
        .iter()
        .any(|loc| loc.stmt_index > current_stmt_index)
}

/// Determine which `let` variables are reassigned in a block.
///
/// Walks the block looking for assignment expressions and returns the set
/// of variable names that appear as assignment targets.
pub(crate) fn find_reassigned_variables(body: &ast::Block) -> HashSet<String> {
    let mut reassigned = HashSet::new();
    for stmt in &body.stmts {
        collect_assignments(stmt, &mut reassigned);
    }
    reassigned
}

/// Check whether a Rust type implements `Copy`.
///
/// **Tier 1 only.** Currently covers primitive numeric types, `bool`, and `()`.
/// Tier 2 ownership inference (Phase 4) will expand this to handle:
/// - User-defined types that derive `Copy`
/// - `Option<T>` where `T: Copy`
/// - Named types that happen to be `Copy` (e.g., simple enums)
///
/// Type parameters (`TypeParam`) are conservatively assumed non-Copy.
/// Generic types are also assumed non-Copy.
pub(crate) fn is_copy_type(ty: &RustType) -> bool {
    matches!(
        ty,
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
            | RustType::Unit
    )
}

/// Check whether a type is safe to borrow as `&T` in function parameters.
///
/// Phase 4 conservatively limits this to generic collection types where
/// `&Vec<T>`, `&HashMap<K,V>`, etc. are idiomatic Rust patterns. Named
/// types (structs, enums) are excluded because match destructuring with
/// `&T` produces reference bindings that can break arithmetic on Copy fields.
fn is_borrowable_type(ty: &RustType) -> bool {
    matches!(ty, RustType::Generic(..))
}

/// Recursively collect assignment targets from a statement.
fn collect_assignments(stmt: &ast::Stmt, reassigned: &mut HashSet<String>) {
    match stmt {
        ast::Stmt::Expr(expr) => collect_assignments_from_expr(expr, reassigned),
        ast::Stmt::If(if_stmt) => {
            collect_if_assignments(if_stmt, reassigned);
        }
        ast::Stmt::While(while_stmt) => {
            for inner in &while_stmt.body.stmts {
                collect_assignments(inner, reassigned);
            }
        }
        ast::Stmt::VarDecl(_)
        | ast::Stmt::Return(_)
        | ast::Stmt::Destructure(_)
        | ast::Stmt::ArrayDestructure(_)
        | ast::Stmt::Break(_)
        | ast::Stmt::Continue(_)
        | ast::Stmt::RustBlock(_) => {}
        ast::Stmt::For(for_of) => {
            for inner in &for_of.body.stmts {
                collect_assignments(inner, reassigned);
            }
        }
        ast::Stmt::Switch(switch) => {
            for case in &switch.cases {
                for inner in &case.body {
                    collect_assignments(inner, reassigned);
                }
            }
        }
        ast::Stmt::TryCatch(tc) => {
            for inner in &tc.try_block.stmts {
                collect_assignments(inner, reassigned);
            }
            for inner in &tc.catch_block.stmts {
                collect_assignments(inner, reassigned);
            }
        }
    }
}

/// Collect assignment targets from if statements.
fn collect_if_assignments(if_stmt: &ast::IfStmt, reassigned: &mut HashSet<String>) {
    for inner in &if_stmt.then_block.stmts {
        collect_assignments(inner, reassigned);
    }
    if let Some(else_clause) = &if_stmt.else_clause {
        match else_clause {
            ast::ElseClause::Block(block) => {
                for inner in &block.stmts {
                    collect_assignments(inner, reassigned);
                }
            }
            ast::ElseClause::ElseIf(nested_if) => {
                collect_if_assignments(nested_if, reassigned);
            }
        }
    }
}

/// Extract assignment targets from an expression.
fn collect_assignments_from_expr(expr: &ast::Expr, reassigned: &mut HashSet<String>) {
    if let ast::ExprKind::Assign(assign) = &expr.kind {
        reassigned.insert(assign.target.name.clone());
    }
    // FieldAssign (e.g., `this.field = value`) does not create new variable
    // bindings — it modifies an existing object. Handled by the wildcard.
}

/// Find variables that are receivers of method calls in a block.
///
/// This is used to detect variables that may need `mut` because
/// a class method called on them might take `&mut self`.
pub(crate) fn find_method_call_receivers(body: &ast::Block) -> HashSet<String> {
    let mut receivers = HashSet::new();
    for stmt in &body.stmts {
        collect_method_call_receivers(stmt, &mut receivers);
    }
    receivers
}

/// Collect variables that are receivers of method calls from a statement.
fn collect_method_call_receivers(stmt: &ast::Stmt, receivers: &mut HashSet<String>) {
    match stmt {
        ast::Stmt::Expr(expr) => collect_method_receivers_from_expr(expr, receivers),
        ast::Stmt::If(if_stmt) => {
            for s in &if_stmt.then_block.stmts {
                collect_method_call_receivers(s, receivers);
            }
            if let Some(else_clause) = &if_stmt.else_clause {
                match else_clause {
                    ast::ElseClause::Block(block) => {
                        for s in &block.stmts {
                            collect_method_call_receivers(s, receivers);
                        }
                    }
                    ast::ElseClause::ElseIf(nested) => {
                        let nested_block = ast::Block {
                            stmts: vec![ast::Stmt::If(nested.as_ref().clone())],
                            span: nested.span,
                        };
                        for s in &nested_block.stmts {
                            collect_method_call_receivers(s, receivers);
                        }
                    }
                }
            }
        }
        ast::Stmt::While(w) => {
            for s in &w.body.stmts {
                collect_method_call_receivers(s, receivers);
            }
        }
        ast::Stmt::For(for_of) => {
            for s in &for_of.body.stmts {
                collect_method_call_receivers(s, receivers);
            }
        }
        _ => {}
    }
}

/// Extract method call receivers from an expression.
fn collect_method_receivers_from_expr(expr: &ast::Expr, receivers: &mut HashSet<String>) {
    if let ast::ExprKind::MethodCall(mc) = &expr.kind
        && let ast::ExprKind::Ident(ident) = &mc.object.kind
    {
        receivers.insert(ident.name.clone());
    }
}

// ============================================================================
// Tier 2: Parameter Borrow Analysis
// ============================================================================

/// Result of analyzing a single parameter's usage within a function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamUsage {
    /// Parameter is only read (field access, passed to println!, used in expressions).
    /// Safe to borrow.
    ReadOnly,
    /// Parameter is moved (passed to another function that takes ownership,
    /// stored in a struct field, returned from the function).
    /// Must remain owned.
    Moved,
    /// Parameter is mutated (assigned to, passed to &mut method).
    /// Must remain owned (we don't infer &mut in Phase 4).
    Mutated,
    /// Parameter usage is ambiguous or complex (closure capture, conditional move).
    /// Fall back to owned.
    Unknown,
}

/// Analyze parameter usage within a function body.
///
/// Returns a map from parameter name to its inferred usage mode.
/// Parameters not found in the body are treated as `ReadOnly` (unused params
/// can trivially be borrowed).
pub(crate) fn analyze_param_usage(
    body: &ast::Block,
    params: &[String],
    is_ref_call: impl Fn(&str, &str) -> bool,
) -> HashMap<String, ParamUsage> {
    let param_set: HashSet<&str> = params.iter().map(String::as_str).collect();
    let mut usage: HashMap<String, ParamUsage> = HashMap::new();

    for stmt in &body.stmts {
        collect_param_usage_stmt(stmt, &param_set, &is_ref_call, &mut usage);
    }

    usage
}

/// Convert a `ParamUsage` to a `ParamMode` for a given parameter type.
///
/// Phase 4 is conservative: only `String` gets `BorrowedStr` and only
/// collection generics (`Vec<T>`, `HashMap<K,V>`, `HashSet<T>`) get
/// `Borrowed`. Named types (structs, enums) stay `Owned` because match
/// destructuring with `&T` can produce reference bindings that break
/// arithmetic on Copy inner fields. Task 047 may extend this.
pub(crate) fn usage_to_mode(usage: ParamUsage, param_type: &RustType) -> ParamMode {
    match usage {
        ParamUsage::ReadOnly => {
            if is_copy_type(param_type) {
                // Copy types: no benefit from borrowing
                ParamMode::Owned
            } else if matches!(param_type, RustType::String) {
                // String → &str is the highest-value optimization
                ParamMode::BorrowedStr
            } else if is_borrowable_type(param_type) {
                // Collection types: &Vec<T>, &HashMap<K,V>, &HashSet<T>
                ParamMode::Borrowed
            } else {
                // Named types (structs, enums), type params, etc. stay owned.
                // Borrowing these can cause issues with match destructuring
                // and other patterns. Task 047 may extend this.
                ParamMode::Owned
            }
        }
        ParamUsage::Moved | ParamUsage::Mutated | ParamUsage::Unknown => ParamMode::Owned,
    }
}

/// Merge a new usage into the existing usage for a parameter.
///
/// The merge rule is conservative: any non-ReadOnly usage taints the whole
/// parameter. `Unknown` takes priority over `Moved`/`Mutated`.
fn merge_usage(existing: ParamUsage, new: ParamUsage) -> ParamUsage {
    match (existing, new) {
        (ParamUsage::Unknown, _) | (_, ParamUsage::Unknown) => ParamUsage::Unknown,
        (ParamUsage::Moved, _) | (_, ParamUsage::Moved) => ParamUsage::Moved,
        (ParamUsage::Mutated, _) | (_, ParamUsage::Mutated) => ParamUsage::Mutated,
        (ParamUsage::ReadOnly, ParamUsage::ReadOnly) => ParamUsage::ReadOnly,
    }
}

/// Record a usage for a parameter, merging with any existing usage.
fn record_usage(
    name: &str,
    usage: ParamUsage,
    param_set: &HashSet<&str>,
    result: &mut HashMap<String, ParamUsage>,
) {
    if !param_set.contains(name) {
        return;
    }
    let entry = result
        .entry(name.to_owned())
        .or_insert(ParamUsage::ReadOnly);
    *entry = merge_usage(*entry, usage);
}

/// Collect parameter usage from a statement.
fn collect_param_usage_stmt(
    stmt: &ast::Stmt,
    param_set: &HashSet<&str>,
    is_ref_call: &impl Fn(&str, &str) -> bool,
    result: &mut HashMap<String, ParamUsage>,
) {
    match stmt {
        ast::Stmt::VarDecl(decl) => {
            collect_param_usage_expr(&decl.init, param_set, is_ref_call, result);
        }
        ast::Stmt::Expr(expr) => {
            collect_param_usage_expr(expr, param_set, is_ref_call, result);
        }
        ast::Stmt::Return(ret) => {
            if let Some(value) = &ret.value {
                // A parameter directly returned is Moved (ownership transfer)
                if let ast::ExprKind::Ident(ident) = &value.kind {
                    record_usage(&ident.name, ParamUsage::Moved, param_set, result);
                } else {
                    collect_param_usage_expr(value, param_set, is_ref_call, result);
                }
            }
        }
        ast::Stmt::If(if_stmt) => {
            collect_param_usage_if(if_stmt, param_set, is_ref_call, result);
        }
        ast::Stmt::While(while_stmt) => {
            collect_param_usage_expr(&while_stmt.condition, param_set, is_ref_call, result);
            for inner in &while_stmt.body.stmts {
                collect_param_usage_stmt(inner, param_set, is_ref_call, result);
            }
        }
        ast::Stmt::Destructure(destr) => {
            collect_param_usage_expr(&destr.init, param_set, is_ref_call, result);
        }
        ast::Stmt::Switch(switch) => {
            collect_param_usage_expr(&switch.scrutinee, param_set, is_ref_call, result);
            for case in &switch.cases {
                for inner in &case.body {
                    collect_param_usage_stmt(inner, param_set, is_ref_call, result);
                }
            }
        }
        ast::Stmt::TryCatch(tc) => {
            for inner in &tc.try_block.stmts {
                collect_param_usage_stmt(inner, param_set, is_ref_call, result);
            }
            for inner in &tc.catch_block.stmts {
                collect_param_usage_stmt(inner, param_set, is_ref_call, result);
            }
        }
        ast::Stmt::For(for_of) => {
            // The iterable is borrowed in for-of (iteration borrows)
            if let ast::ExprKind::Ident(ident) = &for_of.iterable.kind {
                record_usage(&ident.name, ParamUsage::ReadOnly, param_set, result);
            } else {
                collect_param_usage_expr(&for_of.iterable, param_set, is_ref_call, result);
            }
            for inner in &for_of.body.stmts {
                collect_param_usage_stmt(inner, param_set, is_ref_call, result);
            }
        }
        ast::Stmt::ArrayDestructure(adestr) => {
            collect_param_usage_expr(&adestr.init, param_set, is_ref_call, result);
        }
        ast::Stmt::Break(_) | ast::Stmt::Continue(_) | ast::Stmt::RustBlock(_) => {}
    }
}

/// Collect parameter usage from an if statement.
fn collect_param_usage_if(
    if_stmt: &ast::IfStmt,
    param_set: &HashSet<&str>,
    is_ref_call: &impl Fn(&str, &str) -> bool,
    result: &mut HashMap<String, ParamUsage>,
) {
    collect_param_usage_expr(&if_stmt.condition, param_set, is_ref_call, result);
    for inner in &if_stmt.then_block.stmts {
        collect_param_usage_stmt(inner, param_set, is_ref_call, result);
    }
    if let Some(else_clause) = &if_stmt.else_clause {
        match else_clause {
            ast::ElseClause::Block(block) => {
                for inner in &block.stmts {
                    collect_param_usage_stmt(inner, param_set, is_ref_call, result);
                }
            }
            ast::ElseClause::ElseIf(nested_if) => {
                collect_param_usage_if(nested_if, param_set, is_ref_call, result);
            }
        }
    }
}

/// Collect parameter usage from an expression.
///
/// This walks expressions and determines how each parameter is used.
/// The key distinction from `UseMap::collect_expr_uses` is that this tracks
/// richer usage categories (`ReadOnly`, `Moved`, `Mutated`, `Unknown`) instead of
/// just move position booleans.
#[allow(clippy::too_many_lines)]
// Match arms for all expression kinds; splitting would obscure the analysis logic
fn collect_param_usage_expr(
    expr: &ast::Expr,
    param_set: &HashSet<&str>,
    is_ref_call: &impl Fn(&str, &str) -> bool,
    result: &mut HashMap<String, ParamUsage>,
) {
    match &expr.kind {
        ast::ExprKind::Ident(ident) => {
            // A bare identifier reference in expression context is a read
            record_usage(&ident.name, ParamUsage::ReadOnly, param_set, result);
        }
        ast::ExprKind::Binary(bin) => {
            // Binary operands are reads
            collect_param_usage_expr(&bin.left, param_set, is_ref_call, result);
            collect_param_usage_expr(&bin.right, param_set, is_ref_call, result);
        }
        ast::ExprKind::Unary(un) => {
            collect_param_usage_expr(&un.operand, param_set, is_ref_call, result);
        }
        ast::ExprKind::Call(call) => {
            // Function call arguments are move positions (conservative)
            for arg in &call.args {
                if let ast::ExprKind::Ident(ident) = &arg.kind {
                    record_usage(&ident.name, ParamUsage::Moved, param_set, result);
                } else {
                    collect_param_usage_expr(arg, param_set, is_ref_call, result);
                }
            }
        }
        ast::ExprKind::MethodCall(mc) => {
            // Check if this is a builtin that takes refs (e.g., console.log → println!)
            let obj_name = match &mc.object.kind {
                ast::ExprKind::Ident(ident) => Some(ident.name.as_str()),
                _ => None,
            };
            let is_ref = obj_name.is_some_and(|obj| is_ref_call(obj, &mc.method.name));

            // The object itself is read (field access / method receiver)
            collect_param_usage_expr(&mc.object, param_set, is_ref_call, result);

            if is_ref {
                // Ref-call arguments (e.g., println!) are reads
                for arg in &mc.args {
                    collect_param_usage_expr(arg, param_set, is_ref_call, result);
                }
            } else {
                // Non-ref method call arguments are moves
                for arg in &mc.args {
                    if let ast::ExprKind::Ident(ident) = &arg.kind {
                        record_usage(&ident.name, ParamUsage::Moved, param_set, result);
                    } else {
                        collect_param_usage_expr(arg, param_set, is_ref_call, result);
                    }
                }
            }
        }
        ast::ExprKind::Assign(assign) => {
            // The assignment target is a mutation
            record_usage(&assign.target.name, ParamUsage::Mutated, param_set, result);
            // The value side is a regular expression
            collect_param_usage_expr(&assign.value, param_set, is_ref_call, result);
        }
        ast::ExprKind::StructLit(slit) => {
            // Struct field values are move positions (stored in struct)
            for field in &slit.fields {
                if let ast::ExprKind::Ident(ident) = &field.value.kind {
                    record_usage(&ident.name, ParamUsage::Moved, param_set, result);
                } else {
                    collect_param_usage_expr(&field.value, param_set, is_ref_call, result);
                }
            }
        }
        ast::ExprKind::FieldAccess(fa) => {
            // Field access is a read
            collect_param_usage_expr(&fa.object, param_set, is_ref_call, result);
        }
        ast::ExprKind::TemplateLit(tpl) => {
            // Template literal interpolations are reads (like format!)
            for part in &tpl.parts {
                if let ast::TemplatePart::Expr(e) = part {
                    collect_param_usage_expr(e, param_set, is_ref_call, result);
                }
            }
        }
        ast::ExprKind::ArrayLit(elements) => {
            // Array/vec literal elements are moves (stored in collection)
            for elem in elements {
                if let ast::ExprKind::Ident(ident) = &elem.kind {
                    record_usage(&ident.name, ParamUsage::Moved, param_set, result);
                } else {
                    collect_param_usage_expr(elem, param_set, is_ref_call, result);
                }
            }
        }
        ast::ExprKind::New(new_expr) => {
            // Constructor arguments are moves
            for arg in &new_expr.args {
                if let ast::ExprKind::Ident(ident) = &arg.kind {
                    record_usage(&ident.name, ParamUsage::Moved, param_set, result);
                } else {
                    collect_param_usage_expr(arg, param_set, is_ref_call, result);
                }
            }
        }
        ast::ExprKind::Index(index_expr) => {
            // Indexing is a read
            collect_param_usage_expr(&index_expr.object, param_set, is_ref_call, result);
            collect_param_usage_expr(&index_expr.index, param_set, is_ref_call, result);
        }
        ast::ExprKind::Closure(closure) => {
            // Any parameter referenced inside a closure body is conservatively Unknown
            // (closure capture analysis is deferred to Task 047)
            mark_closure_captures(closure, param_set, result);
        }
        ast::ExprKind::FieldAssign(fa) => {
            collect_param_usage_expr(&fa.object, param_set, is_ref_call, result);
            collect_param_usage_expr(&fa.value, param_set, is_ref_call, result);
        }
        ast::ExprKind::OptionalChain(chain) => {
            collect_param_usage_expr(&chain.object, param_set, is_ref_call, result);
            if let ast::OptionalAccess::Method(_, args) = &chain.access {
                for arg in args {
                    if let ast::ExprKind::Ident(ident) = &arg.kind {
                        record_usage(&ident.name, ParamUsage::Moved, param_set, result);
                    } else {
                        collect_param_usage_expr(arg, param_set, is_ref_call, result);
                    }
                }
            }
        }
        ast::ExprKind::NullishCoalescing(nc) => {
            collect_param_usage_expr(&nc.left, param_set, is_ref_call, result);
            collect_param_usage_expr(&nc.right, param_set, is_ref_call, result);
        }
        ast::ExprKind::Paren(inner)
        | ast::ExprKind::Throw(inner)
        | ast::ExprKind::Await(inner)
        | ast::ExprKind::Shared(inner) => {
            collect_param_usage_expr(inner, param_set, is_ref_call, result);
        }
        ast::ExprKind::IntLit(_)
        | ast::ExprKind::FloatLit(_)
        | ast::ExprKind::StringLit(_)
        | ast::ExprKind::BoolLit(_)
        | ast::ExprKind::NullLit
        | ast::ExprKind::This => {}
    }
}

/// Mark any parameter references inside a closure body as `Unknown`.
///
/// Closure capture analysis is deferred to Task 047. For now, any parameter
/// that appears inside a closure is conservatively treated as `Unknown`.
fn mark_closure_captures(
    closure: &ast::ClosureExpr,
    param_set: &HashSet<&str>,
    result: &mut HashMap<String, ParamUsage>,
) {
    // Collect all identifiers referenced in the closure body
    let mut captures = HashSet::new();
    match &closure.body {
        ast::ClosureBody::Expr(expr) => collect_idents_in_expr(expr, &mut captures),
        ast::ClosureBody::Block(block) => {
            for stmt in &block.stmts {
                collect_idents_in_stmt(stmt, &mut captures);
            }
        }
    }
    // Any parameter that appears in the closure is Unknown
    for name in &captures {
        record_usage(name, ParamUsage::Unknown, param_set, result);
    }
}

/// Collect all identifier names referenced in an expression (shallow scan for closure capture).
fn collect_idents_in_expr(expr: &ast::Expr, names: &mut HashSet<String>) {
    match &expr.kind {
        ast::ExprKind::Ident(ident) => {
            names.insert(ident.name.clone());
        }
        ast::ExprKind::Binary(bin) => {
            collect_idents_in_expr(&bin.left, names);
            collect_idents_in_expr(&bin.right, names);
        }
        ast::ExprKind::Unary(un) => {
            collect_idents_in_expr(&un.operand, names);
        }
        ast::ExprKind::Call(call) => {
            for arg in &call.args {
                collect_idents_in_expr(arg, names);
            }
        }
        ast::ExprKind::MethodCall(mc) => {
            collect_idents_in_expr(&mc.object, names);
            for arg in &mc.args {
                collect_idents_in_expr(arg, names);
            }
        }
        ast::ExprKind::Paren(inner) => collect_idents_in_expr(inner, names),
        ast::ExprKind::FieldAccess(fa) => collect_idents_in_expr(&fa.object, names),
        ast::ExprKind::Index(idx) => {
            collect_idents_in_expr(&idx.object, names);
            collect_idents_in_expr(&idx.index, names);
        }
        ast::ExprKind::TemplateLit(tpl) => {
            for part in &tpl.parts {
                if let ast::TemplatePart::Expr(e) = part {
                    collect_idents_in_expr(e, names);
                }
            }
        }
        ast::ExprKind::Assign(assign) => {
            names.insert(assign.target.name.clone());
            collect_idents_in_expr(&assign.value, names);
        }
        ast::ExprKind::StructLit(slit) => {
            for field in &slit.fields {
                collect_idents_in_expr(&field.value, names);
            }
        }
        ast::ExprKind::ArrayLit(elems) => {
            for elem in elems {
                collect_idents_in_expr(elem, names);
            }
        }
        ast::ExprKind::New(new_expr) => {
            for arg in &new_expr.args {
                collect_idents_in_expr(arg, names);
            }
        }
        ast::ExprKind::FieldAssign(fa) => {
            collect_idents_in_expr(&fa.object, names);
            collect_idents_in_expr(&fa.value, names);
        }
        ast::ExprKind::OptionalChain(chain) => {
            collect_idents_in_expr(&chain.object, names);
            if let ast::OptionalAccess::Method(_, args) = &chain.access {
                for arg in args {
                    collect_idents_in_expr(arg, names);
                }
            }
        }
        ast::ExprKind::NullishCoalescing(nc) => {
            collect_idents_in_expr(&nc.left, names);
            collect_idents_in_expr(&nc.right, names);
        }
        ast::ExprKind::Throw(inner)
        | ast::ExprKind::Await(inner)
        | ast::ExprKind::Shared(inner) => {
            collect_idents_in_expr(inner, names);
        }
        ast::ExprKind::Closure(_)
        | ast::ExprKind::IntLit(_)
        | ast::ExprKind::FloatLit(_)
        | ast::ExprKind::StringLit(_)
        | ast::ExprKind::BoolLit(_)
        | ast::ExprKind::NullLit
        | ast::ExprKind::This => {}
    }
}

/// Collect all identifier names referenced in a statement (for closure capture scanning).
fn collect_idents_in_stmt(stmt: &ast::Stmt, names: &mut HashSet<String>) {
    match stmt {
        ast::Stmt::VarDecl(decl) => collect_idents_in_expr(&decl.init, names),
        ast::Stmt::Expr(expr) => collect_idents_in_expr(expr, names),
        ast::Stmt::Return(ret) => {
            if let Some(value) = &ret.value {
                collect_idents_in_expr(value, names);
            }
        }
        ast::Stmt::If(if_stmt) => {
            collect_idents_in_expr(&if_stmt.condition, names);
            for s in &if_stmt.then_block.stmts {
                collect_idents_in_stmt(s, names);
            }
            if let Some(else_clause) = &if_stmt.else_clause {
                match else_clause {
                    ast::ElseClause::Block(block) => {
                        for s in &block.stmts {
                            collect_idents_in_stmt(s, names);
                        }
                    }
                    ast::ElseClause::ElseIf(nested) => {
                        let stmts = vec![ast::Stmt::If(nested.as_ref().clone())];
                        for s in &stmts {
                            collect_idents_in_stmt(s, names);
                        }
                    }
                }
            }
        }
        ast::Stmt::While(w) => {
            collect_idents_in_expr(&w.condition, names);
            for s in &w.body.stmts {
                collect_idents_in_stmt(s, names);
            }
        }
        ast::Stmt::For(f) => {
            collect_idents_in_expr(&f.iterable, names);
            for s in &f.body.stmts {
                collect_idents_in_stmt(s, names);
            }
        }
        ast::Stmt::Switch(sw) => {
            collect_idents_in_expr(&sw.scrutinee, names);
            for case in &sw.cases {
                for s in &case.body {
                    collect_idents_in_stmt(s, names);
                }
            }
        }
        ast::Stmt::TryCatch(tc) => {
            for s in &tc.try_block.stmts {
                collect_idents_in_stmt(s, names);
            }
            for s in &tc.catch_block.stmts {
                collect_idents_in_stmt(s, names);
            }
        }
        ast::Stmt::Destructure(d) => collect_idents_in_expr(&d.init, names),
        ast::Stmt::ArrayDestructure(ad) => collect_idents_in_expr(&ad.init, names),
        ast::Stmt::Break(_) | ast::Stmt::Continue(_) | ast::Stmt::RustBlock(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::ast::*;
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

    fn ident_expr(name: &str, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::Ident(ident(name, start, end)),
            span: span(start, end),
        }
    }

    fn int_expr(value: i64, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::IntLit(value),
            span: span(start, end),
        }
    }

    fn no_ref_call(_obj: &str, _method: &str) -> bool {
        false
    }

    fn no_callee_modes() -> impl Fn(&str) -> Option<&'static [ParamMode]> {
        |_: &str| -> Option<&'static [ParamMode]> { None }
    }

    fn console_log_ref(obj: &str, method: &str) -> bool {
        obj == "console" && method == "log"
    }

    // Test 5: UseMap::analyze with two uses of variable x
    #[test]
    fn test_use_map_analyze_two_uses_correct_indices() {
        // Block: { greet(x); greet(x); foo(); }
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 0, 5),
                        args: vec![ident_expr("x", 6, 7)],
                    }),
                    span: span(0, 8),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 10, 15),
                        args: vec![ident_expr("x", 16, 17)],
                    }),
                    span: span(10, 18),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("foo", 20, 23),
                        args: vec![],
                    }),
                    span: span(20, 25),
                }),
            ],
            span: span(0, 25),
        };

        let use_map = UseMap::analyze(&block, no_ref_call, no_callee_modes());
        let x_uses = use_map.get_uses("x").unwrap();
        assert_eq!(x_uses.len(), 2);
        assert_eq!(x_uses[0].stmt_index, 0);
        assert_eq!(x_uses[1].stmt_index, 1);
        assert!(x_uses[0].is_move_position);
        assert!(x_uses[1].is_move_position);
    }

    // Test 6: needs_clone for String at stmt 0 with later use at stmt 2
    #[test]
    fn test_needs_clone_string_with_later_use_returns_true() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 0, 5),
                        args: vec![ident_expr("x", 6, 7)],
                    }),
                    span: span(0, 8),
                }),
                Stmt::Expr(int_expr(42, 10, 12)),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 20, 25),
                        args: vec![ident_expr("x", 26, 27)],
                    }),
                    span: span(20, 28),
                }),
            ],
            span: span(0, 28),
        };

        let use_map = UseMap::analyze(&block, no_ref_call, no_callee_modes());
        assert!(needs_clone("x", 0, &use_map, &RustType::String));
    }

    // Test 7: needs_clone at last use returns false
    #[test]
    fn test_needs_clone_string_at_last_use_returns_false() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 0, 5),
                        args: vec![ident_expr("x", 6, 7)],
                    }),
                    span: span(0, 8),
                }),
                Stmt::Expr(int_expr(42, 10, 12)),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 20, 25),
                        args: vec![ident_expr("x", 26, 27)],
                    }),
                    span: span(20, 28),
                }),
            ],
            span: span(0, 28),
        };

        let use_map = UseMap::analyze(&block, no_ref_call, no_callee_modes());
        assert!(!needs_clone("x", 2, &use_map, &RustType::String));
    }

    // Test 8: needs_clone for Copy type returns false
    #[test]
    fn test_needs_clone_copy_type_returns_false() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("foo", 0, 3),
                        args: vec![ident_expr("x", 4, 5)],
                    }),
                    span: span(0, 6),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("foo", 10, 13),
                        args: vec![ident_expr("x", 14, 15)],
                    }),
                    span: span(10, 16),
                }),
            ],
            span: span(0, 16),
        };

        let use_map = UseMap::analyze(&block, no_ref_call, no_callee_modes());
        assert!(!needs_clone("x", 0, &use_map, &RustType::I32));
        assert!(!needs_clone("x", 0, &use_map, &RustType::I64));
        assert!(!needs_clone("x", 0, &use_map, &RustType::F64));
        assert!(!needs_clone("x", 0, &use_map, &RustType::Bool));
    }

    // Test 8b: needs_clone for extended Copy types returns false (Task 013)
    #[test]
    fn test_needs_clone_extended_copy_types_returns_false() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("foo", 0, 3),
                        args: vec![ident_expr("x", 4, 5)],
                    }),
                    span: span(0, 6),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("foo", 10, 13),
                        args: vec![ident_expr("x", 14, 15)],
                    }),
                    span: span(10, 16),
                }),
            ],
            span: span(0, 16),
        };

        let use_map = UseMap::analyze(&block, no_ref_call, no_callee_modes());
        assert!(!needs_clone("x", 0, &use_map, &RustType::I8));
        assert!(!needs_clone("x", 0, &use_map, &RustType::I16));
        assert!(!needs_clone("x", 0, &use_map, &RustType::U8));
        assert!(!needs_clone("x", 0, &use_map, &RustType::U16));
        assert!(!needs_clone("x", 0, &use_map, &RustType::U32));
        assert!(!needs_clone("x", 0, &use_map, &RustType::U64));
        assert!(!needs_clone("x", 0, &use_map, &RustType::F32));
    }

    // Test 9: println! args are NOT move positions
    #[test]
    fn test_needs_clone_println_args_not_move_position() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::MethodCall(MethodCallExpr {
                        object: Box::new(ident_expr("console", 0, 7)),
                        method: ident("log", 8, 11),
                        args: vec![ident_expr("x", 12, 13)],
                    }),
                    span: span(0, 14),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::MethodCall(MethodCallExpr {
                        object: Box::new(ident_expr("console", 20, 27)),
                        method: ident("log", 28, 31),
                        args: vec![ident_expr("x", 32, 33)],
                    }),
                    span: span(20, 34),
                }),
            ],
            span: span(0, 34),
        };

        let use_map = UseMap::analyze(&block, console_log_ref, no_callee_modes());
        // Even though x is String and used later, println! is not a move position
        assert!(!needs_clone("x", 0, &use_map, &RustType::String));
    }

    // Test T17-15: Vec<T> is non-Copy (clone inserted when needed)
    #[test]
    fn test_needs_clone_vec_type_is_non_copy() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("process", 0, 7),
                        args: vec![ident_expr("v", 8, 9)],
                    }),
                    span: span(0, 10),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("process", 20, 27),
                        args: vec![ident_expr("v", 28, 29)],
                    }),
                    span: span(20, 30),
                }),
            ],
            span: span(0, 30),
        };

        let use_map = UseMap::analyze(&block, no_ref_call, no_callee_modes());
        let vec_type = RustType::Generic(
            Box::new(RustType::Named("Vec".to_owned())),
            vec![RustType::I32],
        );
        // Vec is non-Copy, used in move position with later use → needs clone
        assert!(needs_clone("v", 0, &use_map, &vec_type));
    }

    // T018-11: Iterable in for-of is not a move position (no clone on collection)
    #[test]
    fn test_for_of_iterable_not_move_position() {
        let block = Block {
            stmts: vec![
                Stmt::For(ForOfStmt {
                    binding: VarBinding::Const,
                    variable: ident("x", 0, 1),
                    iterable: ident_expr("items", 5, 10),
                    body: Block {
                        stmts: vec![],
                        span: span(12, 14),
                    },
                    span: span(0, 14),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("process", 20, 27),
                        args: vec![ident_expr("items", 28, 33)],
                    }),
                    span: span(20, 34),
                }),
            ],
            span: span(0, 34),
        };

        let use_map = UseMap::analyze(&block, no_ref_call, no_callee_modes());
        let items_uses = use_map.get_uses("items").unwrap();
        // The for-of iterable should not be a move position
        assert_eq!(items_uses.len(), 2);
        assert!(
            !items_uses[0].is_move_position,
            "for-of iterable should NOT be a move position"
        );
    }

    // T018-12: Variables assigned inside for-of body are detected
    #[test]
    fn test_find_reassigned_variables_inside_for_of() {
        let block = Block {
            stmts: vec![Stmt::For(ForOfStmt {
                binding: VarBinding::Const,
                variable: ident("x", 0, 1),
                iterable: ident_expr("items", 5, 10),
                body: Block {
                    stmts: vec![Stmt::Expr(Expr {
                        kind: ExprKind::Assign(AssignExpr {
                            target: ident("total", 12, 17),
                            value: Box::new(int_expr(1, 20, 21)),
                        }),
                        span: span(12, 22),
                    })],
                    span: span(11, 23),
                },
                span: span(0, 23),
            })],
            span: span(0, 23),
        };

        let reassigned = find_reassigned_variables(&block);
        assert!(
            reassigned.contains("total"),
            "expected `total` to be in reassigned set"
        );
    }

    // Test 10: find_reassigned_variables with x = 10
    #[test]
    fn test_find_reassigned_variables_with_assignment() {
        let block = Block {
            stmts: vec![
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
                        value: Box::new(int_expr(10, 15, 17)),
                    }),
                    span: span(11, 18),
                }),
            ],
            span: span(0, 18),
        };

        let reassigned = find_reassigned_variables(&block);
        assert!(reassigned.contains("x"));
        assert_eq!(reassigned.len(), 1);
    }

    // Test 11: find_reassigned_variables on block with no assignments
    #[test]
    fn test_find_reassigned_variables_none() {
        let block = Block {
            stmts: vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: None,
                init: int_expr(42, 10, 12),
                span: span(0, 13),
            })],
            span: span(0, 13),
        };

        let reassigned = find_reassigned_variables(&block);
        assert!(reassigned.is_empty());
    }

    // ========================================================================
    // Tier 2: Parameter Borrow Analysis Tests
    // ========================================================================

    // Test 1: parameter only used in println! → ReadOnly
    #[test]
    fn test_param_usage_println_only_is_read_only() {
        // function greet(name: string) { console.log(name); }
        let block = Block {
            stmts: vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![ident_expr("name", 12, 16)],
                }),
                span: span(0, 17),
            })],
            span: span(0, 17),
        };

        let params = vec!["name".to_owned()];
        let usage = analyze_param_usage(&block, &params, console_log_ref);
        assert_eq!(
            usage.get("name").copied(),
            Some(ParamUsage::ReadOnly),
            "println!-only param should be ReadOnly"
        );
    }

    // Test 2: parameter passed to another function → Moved
    #[test]
    fn test_param_usage_function_call_arg_is_moved() {
        // function store(name: string) { save(name); }
        let block = Block {
            stmts: vec![Stmt::Expr(Expr {
                kind: ExprKind::Call(CallExpr {
                    callee: ident("save", 0, 4),
                    args: vec![ident_expr("name", 5, 9)],
                }),
                span: span(0, 10),
            })],
            span: span(0, 10),
        };

        let params = vec!["name".to_owned()];
        let usage = analyze_param_usage(&block, &params, no_ref_call);
        assert_eq!(
            usage.get("name").copied(),
            Some(ParamUsage::Moved),
            "param passed to function should be Moved"
        );
    }

    // Test 3: parameter returned from function → Moved
    #[test]
    fn test_param_usage_returned_is_moved() {
        // function identity(name: string): string { return name; }
        let block = Block {
            stmts: vec![Stmt::Return(ReturnStmt {
                value: Some(ident_expr("name", 10, 14)),
                span: span(0, 15),
            })],
            span: span(0, 15),
        };

        let params = vec!["name".to_owned()];
        let usage = analyze_param_usage(&block, &params, no_ref_call);
        assert_eq!(
            usage.get("name").copied(),
            Some(ParamUsage::Moved),
            "returned param should be Moved"
        );
    }

    // Test 4: parameter stored in struct field → Moved
    #[test]
    fn test_param_usage_struct_field_is_moved() {
        // function wrap(name: string) { let p = Point { name: name }; }
        let block = Block {
            stmts: vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("p", 4, 5),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::StructLit(StructLitExpr {
                        type_name: Some(ident("Point", 8, 13)),
                        fields: vec![FieldInit {
                            name: ident("name", 16, 20),
                            value: ident_expr("name", 22, 26),
                            span: span(16, 26),
                        }],
                    }),
                    span: span(8, 28),
                },
                span: span(0, 29),
            })],
            span: span(0, 29),
        };

        let params = vec!["name".to_owned()];
        let usage = analyze_param_usage(&block, &params, no_ref_call);
        assert_eq!(
            usage.get("name").copied(),
            Some(ParamUsage::Moved),
            "param stored in struct field should be Moved"
        );
    }

    // Test 5: parameter used in binary expression only → ReadOnly
    #[test]
    fn test_param_usage_binary_expr_is_read_only() {
        // function double(x: i32): i32 { return x * 2; }
        let block = Block {
            stmts: vec![Stmt::Return(ReturnStmt {
                value: Some(Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        left: Box::new(ident_expr("x", 10, 11)),
                        op: BinaryOp::Mul,
                        right: Box::new(int_expr(2, 14, 15)),
                    }),
                    span: span(10, 15),
                }),
                span: span(0, 16),
            })],
            span: span(0, 16),
        };

        let params = vec!["x".to_owned()];
        let usage = analyze_param_usage(&block, &params, no_ref_call);
        assert_eq!(
            usage.get("x").copied(),
            Some(ParamUsage::ReadOnly),
            "param in binary expression only should be ReadOnly"
        );
    }

    // Test 6: parameter on left side of assignment → Mutated
    #[test]
    fn test_param_usage_assignment_target_is_mutated() {
        // function mutate(x: i32) { x = 10; }
        let block = Block {
            stmts: vec![Stmt::Expr(Expr {
                kind: ExprKind::Assign(AssignExpr {
                    target: ident("x", 0, 1),
                    value: Box::new(int_expr(10, 4, 6)),
                }),
                span: span(0, 7),
            })],
            span: span(0, 7),
        };

        let params = vec!["x".to_owned()];
        let usage = analyze_param_usage(&block, &params, no_ref_call);
        assert_eq!(
            usage.get("x").copied(),
            Some(ParamUsage::Mutated),
            "param as assignment target should be Mutated"
        );
    }

    // Test 7: parameter used in field access only → ReadOnly
    #[test]
    fn test_param_usage_field_access_is_read_only() {
        // function getName(user: User): string { return user.name; }
        // Note: return of user.name is not return of user itself — the field is returned
        let block = Block {
            stmts: vec![Stmt::Return(ReturnStmt {
                value: Some(Expr {
                    kind: ExprKind::FieldAccess(FieldAccessExpr {
                        object: Box::new(ident_expr("user", 10, 14)),
                        field: ident("name", 15, 19),
                    }),
                    span: span(10, 19),
                }),
                span: span(0, 20),
            })],
            span: span(0, 20),
        };

        let params = vec!["user".to_owned()];
        let usage = analyze_param_usage(&block, &params, no_ref_call);
        assert_eq!(
            usage.get("user").copied(),
            Some(ParamUsage::ReadOnly),
            "param used in field access only should be ReadOnly"
        );
    }

    // Test 8: parameter captured by closure → Unknown
    #[test]
    fn test_param_usage_closure_capture_is_unknown() {
        // function process(name: string) { const f = () => console.log(name); }
        let block = Block {
            stmts: vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("f", 6, 7),
                type_ann: None,
                init: Expr {
                    kind: ExprKind::Closure(ClosureExpr {
                        is_async: false,
                        is_move: false,
                        params: vec![],
                        return_type: None,
                        body: ClosureBody::Expr(Box::new(Expr {
                            kind: ExprKind::MethodCall(MethodCallExpr {
                                object: Box::new(ident_expr("console", 20, 27)),
                                method: ident("log", 28, 31),
                                args: vec![ident_expr("name", 32, 36)],
                            }),
                            span: span(20, 37),
                        })),
                    }),
                    span: span(10, 37),
                },
                span: span(0, 38),
            })],
            span: span(0, 38),
        };

        let params = vec!["name".to_owned()];
        let usage = analyze_param_usage(&block, &params, console_log_ref);
        assert_eq!(
            usage.get("name").copied(),
            Some(ParamUsage::Unknown),
            "param captured by closure should be Unknown"
        );
    }

    // Test 9: unused parameter → ReadOnly (not in map, defaults to ReadOnly)
    #[test]
    fn test_param_usage_unused_is_read_only() {
        // function noop(name: string) { }
        let block = Block {
            stmts: vec![],
            span: span(0, 2),
        };

        let params = vec!["name".to_owned()];
        let usage = analyze_param_usage(&block, &params, no_ref_call);
        // Unused params are not in the map — the caller defaults to ReadOnly
        assert!(
            !usage.contains_key("name"),
            "unused param should not be in usage map"
        );
    }

    // Test 10: Copy type parameter that's ReadOnly → ParamMode::Owned
    #[test]
    fn test_usage_to_mode_copy_type_read_only_stays_owned() {
        assert_eq!(
            usage_to_mode(ParamUsage::ReadOnly, &RustType::I32),
            ParamMode::Owned,
            "Copy type ReadOnly should stay Owned"
        );
        assert_eq!(
            usage_to_mode(ParamUsage::ReadOnly, &RustType::F64),
            ParamMode::Owned,
        );
        assert_eq!(
            usage_to_mode(ParamUsage::ReadOnly, &RustType::Bool),
            ParamMode::Owned,
        );
    }

    // Test 11: String parameter that's ReadOnly → ParamMode::BorrowedStr
    #[test]
    fn test_usage_to_mode_string_read_only_is_borrowed_str() {
        assert_eq!(
            usage_to_mode(ParamUsage::ReadOnly, &RustType::String),
            ParamMode::BorrowedStr,
            "String ReadOnly should be BorrowedStr"
        );
    }

    // Test 12: Vec parameter that's ReadOnly → ParamMode::Borrowed
    #[test]
    fn test_usage_to_mode_vec_read_only_is_borrowed() {
        let vec_type = RustType::Generic(
            Box::new(RustType::Named("Vec".to_owned())),
            vec![RustType::I32],
        );
        assert_eq!(
            usage_to_mode(ParamUsage::ReadOnly, &vec_type),
            ParamMode::Borrowed,
            "Vec ReadOnly should be Borrowed"
        );
    }

    // Test 13: Named type parameter that's ReadOnly → ParamMode::Owned
    // Phase 4 conservatively keeps named types (structs, enums) as Owned
    // because match destructuring with &T can produce reference bindings
    // that break arithmetic on Copy inner fields. Task 047 may extend this.
    #[test]
    fn test_usage_to_mode_named_type_read_only_is_owned() {
        assert_eq!(
            usage_to_mode(ParamUsage::ReadOnly, &RustType::Named("User".to_owned())),
            ParamMode::Owned,
            "Named type ReadOnly should stay Owned in Phase 4"
        );
    }

    // Test 14: parameter used in for-of iterable → ReadOnly
    #[test]
    fn test_param_usage_for_of_iterable_is_read_only() {
        // function process(items: Vec<i32>) { for (const x of items) { } }
        let block = Block {
            stmts: vec![Stmt::For(ForOfStmt {
                binding: VarBinding::Const,
                variable: ident("x", 0, 1),
                iterable: ident_expr("items", 5, 10),
                body: Block {
                    stmts: vec![],
                    span: span(12, 14),
                },
                span: span(0, 14),
            })],
            span: span(0, 14),
        };

        let params = vec!["items".to_owned()];
        let usage = analyze_param_usage(&block, &params, no_ref_call);
        assert_eq!(
            usage.get("items").copied(),
            Some(ParamUsage::ReadOnly),
            "param used as for-of iterable should be ReadOnly (iteration borrows)"
        );
    }

    // Test 15: RustParam with ParamMode::Owned serializes identically to pre-Phase-4
    #[test]
    fn test_rust_param_owned_mode_preserves_behavior() {
        use rsc_syntax::rust_ir::{ParamMode, RustParam};

        let param = RustParam {
            name: "x".to_owned(),
            ty: RustType::I32,
            mode: ParamMode::Owned,
            span: None,
        };
        // The type still formats as "i32" — mode doesn't affect Display
        assert_eq!(param.ty.to_string(), "i32");
        assert_eq!(param.mode, ParamMode::Owned);
        assert_eq!(param.name, "x");
    }

    // Test 046-1: UseMap with borrowed-param callee does not mark argument as move
    #[test]
    fn test_use_map_borrowed_callee_arg_not_move() {
        // Block: { greet(x); greet(x); }
        // If greet takes BorrowedStr, x should NOT be in move position
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 0, 5),
                        args: vec![ident_expr("x", 6, 7)],
                    }),
                    span: span(0, 8),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 10, 15),
                        args: vec![ident_expr("x", 16, 17)],
                    }),
                    span: span(10, 18),
                }),
            ],
            span: span(0, 20),
        };

        let borrowed_modes: [ParamMode; 1] = [ParamMode::BorrowedStr];
        let use_map = UseMap::analyze(&block, no_ref_call, |callee| {
            if callee == "greet" {
                Some(&borrowed_modes[..])
            } else {
                None
            }
        });

        let uses = use_map.get_uses("x").expect("x should have uses");
        assert_eq!(uses.len(), 2);
        // With borrowed callee, arguments should NOT be move positions
        assert!(
            !uses[0].is_move_position,
            "arg to borrowed param should not be move"
        );
        assert!(
            !uses[1].is_move_position,
            "arg to borrowed param should not be move"
        );

        // Therefore no clone is needed
        assert!(
            !needs_clone("x", 0, &use_map, &RustType::String),
            "no clone needed when callee borrows"
        );
    }

    // Test 046-2: Generic type (Vec) read-only → Borrowed
    #[test]
    fn test_usage_to_mode_generic_vec_read_only_is_borrowed() {
        assert_eq!(
            usage_to_mode(
                ParamUsage::ReadOnly,
                &RustType::Generic(
                    Box::new(RustType::Named("Vec".to_owned())),
                    vec![RustType::I32]
                )
            ),
            ParamMode::Borrowed,
            "Generic Vec ReadOnly should be Borrowed"
        );
    }
}
