#![warn(clippy::pedantic)]
//! Shared hover inference logic for the `RustScript` compiler.
//!
//! Provides hover information for `RustScript` source code by analyzing the
//! parsed AST. Used by both the WASM playground (`rustscript-web`) and the VS Code
//! LSP (`rustscript-lsp`).

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Return hover information for the symbol at the given position.
///
/// For the MVP, this identifies the token at (line, column) and returns
/// descriptions for known builtins. For user-defined symbols, it returns
/// the parsed signature when possible.
///
/// `line` and `column` are 1-based (Monaco convention).
#[must_use]
pub fn hover(source: &str, line: u32, column: u32) -> String {
    // Convert 1-based line/column (Monaco convention) to 0-based indices.
    let line_0 = line.saturating_sub(1) as usize;
    let col = column.saturating_sub(1) as usize;

    // Find the token at the given position.
    let lines: Vec<&str> = source.lines().collect();
    let Some(line_text) = lines.get(line_0) else {
        return String::new();
    };

    // Extract the identifier at/around the column.
    let bytes = line_text.as_bytes();
    if col >= bytes.len() {
        return String::new();
    }

    // Find word boundaries around the cursor position.
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    if !is_ident_char(bytes[col]) {
        return String::new();
    }

    let start = (0..=col)
        .rev()
        .take_while(|&i| is_ident_char(bytes[i]))
        .last()
        .unwrap_or(col);
    let end = (col..bytes.len())
        .take_while(|&i| is_ident_char(bytes[i]))
        .last()
        .map_or(col + 1, |i| i + 1);

    let token = &line_text[start..end];

    // Check builtins first.
    if let Some(desc) = builtin_hover(token) {
        return desc.to_owned();
    }

    // For user-defined symbols, try to find a definition in the parsed AST.
    // Parse the source and look for function/type/variable declarations matching
    // the token name.
    let file_id = rustscript_syntax::source::FileId(0);
    let (module, _diagnostics) = rustscript_parser::parse(source, file_id);

    // Search top-level declarations for a matching name.
    for item in &module.items {
        if let Some(sig) = extract_declaration_signature(item, token) {
            return sig;
        }
    }

    // Search type/interface/class fields for property hover.
    for item in &module.items {
        if let Some(sig) = extract_field_hover(item, token) {
            return sig;
        }
    }

    // Build lookup maps for type inference.
    let fn_info = collect_fn_info(&module);
    let fn_return_types: HashMap<String, String> = fn_info
        .iter()
        .map(|(k, v)| (k.clone(), v.return_type.clone()))
        .collect();
    let type_fields = collect_type_fields(&module);
    let enum_variants = collect_enum_variants(&module);

    // Search inside function bodies for local variables and parameters.
    for item in &module.items {
        if let Some(sig) = extract_local_hover(
            source,
            item,
            token,
            line_0,
            &fn_return_types,
            &type_fields,
            &fn_info,
            &enum_variants,
        ) {
            return sig;
        }
    }

    // Fallback: return empty (no hover) rather than echoing the token.
    String::new()
}

// ---------------------------------------------------------------------------
// Builtin descriptions
// ---------------------------------------------------------------------------

/// Builtin descriptions for well-known `RustScript` APIs.
// Lookup table for all builtin API descriptions
#[allow(clippy::too_many_lines)]
fn builtin_hover(token: &str) -> Option<&'static str> {
    Some(match token {
        "console" => {
            "The console object provides access to the debugging console.\n\n```rustscript\nconst console: Console\n```"
        }
        "log" => {
            "Outputs a message to the console.\n\n```rustscript\nfunction console.log(...args: any[]): void\n```"
        }
        "error" => {
            "Outputs an error message to the console.\n\n```rustscript\nfunction console.error(...args: any[]): void\n```"
        }
        "warn" => {
            "Outputs a warning message to the console.\n\n```rustscript\nfunction console.warn(...args: any[]): void\n```"
        }
        "push" => {
            "Appends an element to the end of an array.\n\n```rustscript\nfunction Array<T>.push(value: T): void\n```"
        }
        "pop" => {
            "Removes the last element from an array and returns it.\n\n```rustscript\nfunction Array<T>.pop(): T | null\n```"
        }
        "map" => {
            "Creates a new array by applying a function to each element.\n\n```rustscript\nfunction Array<T>.map<U>(f: (item: T) => U): Array<U>\n```"
        }
        "filter" => {
            "Creates a new array with elements that pass a test.\n\n```rustscript\nfunction Array<T>.filter(f: (item: T) => boolean): Array<T>\n```"
        }
        "forEach" => {
            "Calls a function for each element in an array.\n\n```rustscript\nfunction Array<T>.forEach(f: (item: T) => void): void\n```"
        }
        "length" => {
            "The number of elements in an array or characters in a string.\n\n```rustscript\nreadonly length: number\n```"
        }
        "includes" => {
            "Determines whether an array or string contains a specified value.\n\n```rustscript\nfunction Array<T>.includes(value: T): boolean\n```"
        }
        "keys" => {
            "Returns an array of a Map's keys.\n\n```rustscript\nfunction Map<K, V>.keys(): Array<K>\n```"
        }
        "values" => {
            "Returns an array of a Map's values.\n\n```rustscript\nfunction Map<K, V>.values(): Array<V>\n```"
        }
        "has" => {
            "Returns whether a key exists in a Map or Set.\n\n```rustscript\nfunction Map<K, V>.has(key: K): boolean\n```"
        }
        "get" => {
            "Returns the value for a key in a Map.\n\n```rustscript\nfunction Map<K, V>.get(key: K): V | null\n```"
        }
        "set" => {
            "Sets a key-value pair in a Map.\n\n```rustscript\nfunction Map<K, V>.set(key: K, value: V): void\n```"
        }
        "delete" => {
            "Removes a key from a Map or Set.\n\n```rustscript\nfunction Map<K, V>.delete(key: K): boolean\n```"
        }
        "parseInt" => {
            "Parses a string and returns an integer.\n\n```rustscript\nfunction parseInt(s: string): number\n```"
        }
        "parseFloat" => {
            "Parses a string and returns a floating-point number.\n\n```rustscript\nfunction parseFloat(s: string): number\n```"
        }
        "toString" => {
            "Returns a string representation of a value.\n\n```rustscript\nfunction toString(): string\n```"
        }
        "JSON" => {
            "The JSON object provides methods for parsing and stringifying JSON.\n\n```rustscript\nconst JSON: JSON\n```"
        }
        "stringify" => {
            "Converts a value to a JSON string.\n\n```rustscript\nfunction JSON.stringify(value: any): string\n```"
        }
        "parse" => {
            "Parses a JSON string into a value.\n\n```rustscript\nfunction JSON.parse(text: string): any\n```"
        }
        "Math" => {
            "The Math object provides mathematical constants and functions.\n\n```rustscript\nconst Math: Math\n```"
        }
        "floor" => {
            "Returns the largest integer less than or equal to a number.\n\n```rustscript\nfunction Math.floor(x: number): number\n```"
        }
        "ceil" => {
            "Returns the smallest integer greater than or equal to a number.\n\n```rustscript\nfunction Math.ceil(x: number): number\n```"
        }
        "abs" => {
            "Returns the absolute value of a number.\n\n```rustscript\nfunction Math.abs(x: number): number\n```"
        }
        "random" => {
            "Returns a pseudo-random number between 0 and 1.\n\n```rustscript\nfunction Math.random(): number\n```"
        }
        "max" => {
            "Returns the largest of the given numbers.\n\n```rustscript\nfunction Math.max(...values: number[]): number\n```"
        }
        "min" => {
            "Returns the smallest of the given numbers.\n\n```rustscript\nfunction Math.min(...values: number[]): number\n```"
        }
        "join" => {
            "Joins all elements of an array into a string.\n\n```rustscript\nfunction Array<T>.join(separator?: string): string\n```"
        }
        "split" => {
            "Splits a string into an array of substrings.\n\n```rustscript\nfunction string.split(separator: string): Array<string>\n```"
        }
        "trim" => {
            "Removes whitespace from both ends of a string.\n\n```rustscript\nfunction string.trim(): string\n```"
        }
        "replace" => {
            "Replaces the first occurrence of a pattern in a string.\n\n```rustscript\nfunction string.replace(search: string, replacement: string): string\n```"
        }
        "replaceAll" => {
            "Replaces all occurrences of a pattern in a string.\n\n```rustscript\nfunction string.replaceAll(search: string, replacement: string): string\n```"
        }
        "toUpperCase" => {
            "Converts a string to uppercase.\n\n```rustscript\nfunction string.toUpperCase(): string\n```"
        }
        "toLowerCase" => {
            "Converts a string to lowercase.\n\n```rustscript\nfunction string.toLowerCase(): string\n```"
        }
        "startsWith" => {
            "Determines whether a string begins with the specified characters.\n\n```rustscript\nfunction string.startsWith(search: string): boolean\n```"
        }
        "endsWith" => {
            "Determines whether a string ends with the specified characters.\n\n```rustscript\nfunction string.endsWith(search: string): boolean\n```"
        }
        "indexOf" => {
            "Returns the index of the first occurrence of a value, or -1.\n\n```rustscript\nfunction Array<T>.indexOf(value: T): i64\nfunction string.indexOf(search: string): i64\n```"
        }
        "slice" => {
            "Returns a shallow copy of a portion of an array or string.\n\n```rustscript\nfunction Array<T>.slice(start?: i64, end?: i64): Array<T>\nfunction string.slice(start?: i64, end?: i64): string\n```"
        }
        "reduce" => {
            "Reduces an array to a single value by applying a function.\n\n```rustscript\nfunction Array<T>.reduce<U>(f: (acc: U, item: T) => U, initial: U): U\n```"
        }
        "findIndex" => {
            "Returns the index of the first element that satisfies the test.\n\n```rustscript\nfunction Array<T>.findIndex(f: (item: T) => boolean): i64\n```"
        }
        "every" => {
            "Tests whether all elements pass the provided function.\n\n```rustscript\nfunction Array<T>.every(f: (item: T) => boolean): boolean\n```"
        }
        "some" => {
            "Tests whether at least one element passes the provided function.\n\n```rustscript\nfunction Array<T>.some(f: (item: T) => boolean): boolean\n```"
        }
        "sort" => {
            "Sorts the elements of an array in place.\n\n```rustscript\nfunction Array<T>.sort(): void\n```"
        }
        "reverse" => {
            "Reverses the elements of an array in place.\n\n```rustscript\nfunction Array<T>.reverse(): void\n```"
        }
        "concat" => {
            "Merges two arrays or strings.\n\n```rustscript\nfunction Array<T>.concat(other: Array<T>): Array<T>\nfunction string.concat(other: string): string\n```"
        }
        "flat" => {
            "Flattens nested arrays by one level.\n\n```rustscript\nfunction Array<Array<T>>.flat(): Array<T>\n```"
        }
        "flatMap" => {
            "Maps each element then flattens the result by one level.\n\n```rustscript\nfunction Array<T>.flatMap<U>(f: (item: T) => Array<U>): Array<U>\n```"
        }
        "fill" => {
            "Fills all elements with a static value.\n\n```rustscript\nfunction Array<T>.fill(value: T): void\n```"
        }
        "shift" => {
            "Removes the first element from an array and returns it.\n\n```rustscript\nfunction Array<T>.shift(): T | null\n```"
        }
        "unshift" => {
            "Adds elements to the beginning of an array.\n\n```rustscript\nfunction Array<T>.unshift(value: T): void\n```"
        }
        "splice" => {
            "Changes array contents by removing/replacing elements.\n\n```rustscript\nfunction Array<T>.splice(start: i64, deleteCount: i64): Array<T>\n```"
        }
        "charAt" => {
            "Returns the character at the specified index.\n\n```rustscript\nfunction string.charAt(index: i64): string\n```"
        }
        "repeat" => {
            "Returns a new string repeated the specified number of times.\n\n```rustscript\nfunction string.repeat(count: i64): string\n```"
        }
        "padStart" => {
            "Pads the start of a string to a given length.\n\n```rustscript\nfunction string.padStart(targetLength: i64, padString?: string): string\n```"
        }
        "padEnd" => {
            "Pads the end of a string to a given length.\n\n```rustscript\nfunction string.padEnd(targetLength: i64, padString?: string): string\n```"
        }
        "add" => {
            "Adds a value to a Set.\n\n```rustscript\nfunction Set<T>.add(value: T): void\n```"
        }
        "clear" => {
            "Removes all elements from a Map or Set.\n\n```rustscript\nfunction Map<K, V>.clear(): void\n```"
        }
        "size" => {
            "The number of elements in a Map or Set.\n\n```rustscript\nreadonly size: i64\n```"
        }
        "entries" => {
            "Returns an array of [key, value] pairs.\n\n```rustscript\nfunction Map<K, V>.entries(): Array<[K, V]>\n```"
        }
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Declaration signatures
// ---------------------------------------------------------------------------

/// Try to extract a hover signature from a top-level item if it declares
/// something with the given name.
fn extract_declaration_signature(
    item: &rustscript_syntax::ast::Item,
    name: &str,
) -> Option<String> {
    use rustscript_syntax::ast::ItemKind;

    match &item.kind {
        ItemKind::Function(f) if f.name.name == name => {
            let params: Vec<String> = f
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name.name, format_type_ann(&p.type_ann)))
                .collect();

            let ret = f
                .return_type
                .as_ref()
                .and_then(|rta| rta.type_ann.as_ref())
                .map_or_else(|| "void".to_owned(), format_type_ann);

            let async_prefix = if f.is_async { "async " } else { "" };
            let sig = format!(
                "```rustscript\n{async_prefix}function {name}({}): {ret}\n```",
                params.join(", ")
            );
            Some(with_doc_comment(f.doc_comment.as_ref(), &sig))
        }
        ItemKind::TypeDef(td) if td.name.name == name => {
            let sig = format_type_def_hover(name, td);
            Some(with_doc_comment(td.doc_comment.as_ref(), &sig))
        }
        ItemKind::Interface(iface) if iface.name.name == name => {
            let fields: Vec<String> = iface
                .fields
                .iter()
                .map(|f| format!("  {}: {}", f.name.name, format_type_ann(&f.type_ann)))
                .collect();
            let sig = if fields.is_empty() {
                format!("```rustscript\ninterface {name}\n```")
            } else {
                format!(
                    "```rustscript\ninterface {name} {{\n{}\n}}\n```",
                    fields.join(",\n")
                )
            };
            Some(with_doc_comment(iface.doc_comment.as_ref(), &sig))
        }
        ItemKind::EnumDef(e) if e.name.name == name => {
            let variants: Vec<String> = e
                .variants
                .iter()
                .map(|v| match v {
                    rustscript_syntax::ast::EnumVariant::Simple(ident, _) => ident.name.clone(),
                    rustscript_syntax::ast::EnumVariant::Data { name: n, .. } => n.name.clone(),
                    rustscript_syntax::ast::EnumVariant::TypeRef { type_name, .. } => {
                        type_name.name.clone()
                    }
                })
                .collect();
            let sig = format!(
                "```rustscript\nenum {name} {{ {} }}\n```",
                variants.join(", ")
            );
            Some(with_doc_comment(e.doc_comment.as_ref(), &sig))
        }
        ItemKind::Class(c) if c.name.name == name => {
            let sig = format!("```rustscript\nclass {name}\n```");
            Some(with_doc_comment(c.doc_comment.as_ref(), &sig))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Field hover
// ---------------------------------------------------------------------------

/// Search type definitions, interfaces, and classes for a field matching the token.
fn extract_field_hover(item: &rustscript_syntax::ast::Item, name: &str) -> Option<String> {
    use rustscript_syntax::ast::ItemKind;

    match &item.kind {
        ItemKind::TypeDef(td) => {
            for f in &td.fields {
                if f.name.name == name {
                    let opt = if f.optional { "?" } else { "" };
                    return Some(format!(
                        "```rustscript\n(property) {name}{opt}: {}\n```",
                        format_type_ann(&f.type_ann)
                    ));
                }
            }
            None
        }
        ItemKind::Interface(iface) => {
            for f in &iface.fields {
                if f.name.name == name {
                    return Some(format!(
                        "```rustscript\n(property) {name}: {}\n```",
                        format_type_ann(&f.type_ann)
                    ));
                }
            }
            None
        }
        ItemKind::Class(c) => {
            for member in &c.members {
                if let rustscript_syntax::ast::ClassMember::Field(f) = member
                    && f.name.name == name
                {
                    return Some(format!(
                        "```rustscript\n(property) {name}: {}\n```",
                        format_type_ann(&f.type_ann)
                    ));
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Local variable / param hover
// ---------------------------------------------------------------------------

/// Search inside a top-level item (function body) for local variable declarations
/// and parameters matching the given name.
fn extract_local_hover(
    source: &str,
    item: &rustscript_syntax::ast::Item,
    name: &str,
    cursor_line: usize,
    fn_return_types: &HashMap<String, String>,
    type_fields: &HashMap<String, Vec<(String, String)>>,
    fn_info: &HashMap<String, FnInfo>,
    enum_variants: &HashMap<String, Vec<VariantInfo>>,
) -> Option<String> {
    use rustscript_syntax::ast::ItemKind;

    match &item.kind {
        ItemKind::Function(f) => {
            // Check if cursor is inside a switch case arm — if so, try type narrowing
            if let Some(narrowed) = find_narrowed_type_in_switch(
                source,
                &f.body.stmts,
                name,
                cursor_line,
                &f.params,
                enum_variants,
            ) {
                return Some(narrowed);
            }

            // Check parameters
            for param in &f.params {
                if param.name.name == name {
                    let ty = format_type_ann(&param.type_ann);
                    return Some(format!("```rustscript\n(parameter) {name}: {ty}\n```"));
                }
            }

            // Collect variable types for inference
            let var_types = collect_var_types(&f.body.stmts);

            // Build full inference context
            let infer_ctx = InferCtx {
                fn_return_types,
                fn_info,
                var_types: &var_types,
                type_fields,
            };

            // Check body statements for variable declarations (with full inference)
            for stmt in &f.body.stmts {
                if let Some(sig) = extract_var_hover_ctx(stmt, name, &infer_ctx) {
                    return Some(sig);
                }
            }

            // Check closure parameters in expressions
            for stmt in &f.body.stmts {
                if let Some(sig) = extract_closure_param_hover(stmt, name, &var_types) {
                    return Some(sig);
                }
            }

            None
        }
        _ => None,
    }
}

/// Extract hover info from a variable declaration with full inference context.
fn extract_var_hover_ctx(
    stmt: &rustscript_syntax::ast::Stmt,
    name: &str,
    ctx: &InferCtx<'_>,
) -> Option<String> {
    use rustscript_syntax::ast::{Stmt, VarBinding};

    match stmt {
        Stmt::VarDecl(decl) if decl.name.name == name => {
            let binding = match decl.binding {
                VarBinding::Const => "const",
                VarBinding::Let => "let",
                VarBinding::Var => "var",
            };
            let ty = if let Some(ann) = &decl.type_ann {
                format!(": {}", format_type_ann(ann))
            } else {
                infer_type_from_expr_ctx(&decl.init, ctx)
                    .map_or_else(String::new, |t| format!(": {t}"))
            };
            Some(format!("```rustscript\n{binding} {name}{ty}\n```"))
        }
        // Recurse into nested blocks
        Stmt::If(if_stmt) => {
            for s in &if_stmt.then_block.stmts {
                if let Some(sig) = extract_var_hover_ctx(s, name, ctx) {
                    return Some(sig);
                }
            }
            None
        }
        Stmt::While(w) => {
            for s in &w.body.stmts {
                if let Some(sig) = extract_var_hover_ctx(s, name, ctx) {
                    return Some(sig);
                }
            }
            None
        }
        Stmt::For(f) => {
            for s in &f.body.stmts {
                if let Some(sig) = extract_var_hover_ctx(s, name, ctx) {
                    return Some(sig);
                }
            }
            None
        }
        Stmt::ArrayDestructure(ad) => {
            for (i, elem) in ad.elements.iter().enumerate() {
                let ident = match elem {
                    rustscript_syntax::ast::ArrayDestructureElement::Single(id)
                    | rustscript_syntax::ast::ArrayDestructureElement::Rest(id) => id,
                };
                if ident.name == name {
                    let binding = match ad.binding {
                        VarBinding::Const => "const",
                        VarBinding::Let => "let",
                        VarBinding::Var => "var",
                    };
                    // Try to infer element type from tuple type annotation
                    if let Some(ann) = &ad.type_ann
                        && let rustscript_syntax::ast::TypeKind::Tuple(types) = &ann.kind
                        && let Some(elem_ty) = types.get(i)
                    {
                        return Some(format!(
                            "```rustscript\n{binding} {name}: {}\n```",
                            format_type_ann(elem_ty)
                        ));
                    }
                    // Try to infer from the init expression
                    // Handle: await Promise.all([f(), g()]) -> tuple of return types
                    if let Some(elem_type) = infer_array_destructure_element(i, &ad.init, ctx) {
                        return Some(format!("```rustscript\n{binding} {name}: {elem_type}\n```"));
                    }
                    return Some(format!("```rustscript\n{binding} {name}\n```"));
                }
            }
            None
        }
        Stmt::Destructure(ds) => {
            for field in &ds.fields {
                let local = field.local_name.as_ref().unwrap_or(&field.field_name);
                if local.name == name {
                    // Look up the type from the init expression's type fields
                    let init_type = infer_type_from_expr_ctx(&ds.init, ctx);
                    if let Some(ref type_name) = init_type
                        && let Some(fields) = ctx.type_fields.get(type_name.as_str())
                        && let Some((_, ty)) =
                            fields.iter().find(|(n, _)| n == &field.field_name.name)
                    {
                        return Some(format!("```rustscript\nconst {name}: {ty}\n```"));
                    }
                    return Some(format!("```rustscript\nconst {name}\n```"));
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract hover info from a variable declaration statement (legacy, without full context).
#[allow(dead_code)] // reserved for legacy hover path
fn extract_var_hover(
    stmt: &rustscript_syntax::ast::Stmt,
    name: &str,
    fn_return_types: &HashMap<String, String>,
) -> Option<String> {
    use rustscript_syntax::ast::{ElseClause, Stmt, VarBinding};

    match stmt {
        Stmt::VarDecl(decl) if decl.name.name == name => {
            let binding = match decl.binding {
                VarBinding::Const => "const",
                VarBinding::Let => "let",
                VarBinding::Var => "var",
            };
            // Use explicit type annotation, or infer from initializer
            let ty = if let Some(ann) = &decl.type_ann {
                format!(": {}", format_type_ann(ann))
            } else {
                infer_type_from_expr(&decl.init, fn_return_types)
                    .map_or_else(String::new, |t| format!(": {t}"))
            };
            Some(format!("```rustscript\n{binding} {name}{ty}\n```"))
        }
        Stmt::If(if_stmt) => {
            for s in &if_stmt.then_block.stmts {
                if let Some(sig) = extract_var_hover(s, name, fn_return_types) {
                    return Some(sig);
                }
            }
            if let Some(ref else_clause) = if_stmt.else_clause {
                match else_clause {
                    ElseClause::Block(block) => {
                        for s in &block.stmts {
                            if let Some(sig) = extract_var_hover(s, name, fn_return_types) {
                                return Some(sig);
                            }
                        }
                    }
                    ElseClause::ElseIf(nested_if) => {
                        let nested_stmt = Stmt::If(nested_if.as_ref().clone());
                        if let Some(sig) = extract_var_hover(&nested_stmt, name, fn_return_types) {
                            return Some(sig);
                        }
                    }
                }
            }
            None
        }
        Stmt::While(w) => {
            for s in &w.body.stmts {
                if let Some(sig) = extract_var_hover(s, name, fn_return_types) {
                    return Some(sig);
                }
            }
            None
        }
        Stmt::For(f) => {
            if f.variable.name == name {
                let binding = match f.binding {
                    VarBinding::Const => "const",
                    VarBinding::Let => "let",
                    VarBinding::Var => "var",
                };
                return Some(format!(
                    "```rustscript\n{binding} {name} (for-of loop variable)\n```"
                ));
            }
            for s in &f.body.stmts {
                if let Some(sig) = extract_var_hover(s, name, fn_return_types) {
                    return Some(sig);
                }
            }
            None
        }
        _ => None,
    }
}

/// Infer the type of the i-th element in an array destructuring.
/// Handles: `const [a, b] = await Promise.all([f(), g()])` and plain array literals.
fn infer_array_destructure_element(
    index: usize,
    init: &rustscript_syntax::ast::Expr,
    ctx: &InferCtx<'_>,
) -> Option<String> {
    use rustscript_syntax::ast::ExprKind;

    // Unwrap `await`
    let inner = match &init.kind {
        ExprKind::Await(e) => e.as_ref(),
        _ => init,
    };

    match &inner.kind {
        // Promise.all([f(), g()]) -> infer each element
        ExprKind::MethodCall(mc)
            if mc.method.name == "all"
                && matches!(mc.object.kind, ExprKind::Ident(ref id) if id.name == "Promise") =>
        {
            // The argument should be an array literal
            if let Some(arr_arg) = mc.args.first()
                && let ExprKind::ArrayLit(elements) = &arr_arg.kind
                && let Some(rustscript_syntax::ast::ArrayElement::Expr(elem)) = elements.get(index)
            {
                return infer_type_from_expr_ctx(elem, ctx);
            }
            None
        }
        // Plain array literal: const [a, b] = [expr1, expr2]
        ExprKind::ArrayLit(elements) => {
            if let Some(rustscript_syntax::ast::ArrayElement::Expr(elem)) = elements.get(index) {
                infer_type_from_expr_ctx(elem, ctx)
            } else {
                None
            }
        }
        // Function call returning a tuple: const [a, b] = pair(...)
        ExprKind::Call(call) => {
            // If function returns a known type, we can't decompose it further without
            // full type system support. Return None for now.
            let _ = call;
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Closure param inference
// ---------------------------------------------------------------------------

/// Walk expressions to find closure parameters matching the given name.
fn extract_closure_param_hover(
    stmt: &rustscript_syntax::ast::Stmt,
    name: &str,
    var_types: &HashMap<String, String>,
) -> Option<String> {
    use rustscript_syntax::ast::Stmt;

    match stmt {
        Stmt::VarDecl(decl) => find_closure_param_in_expr(&decl.init, name, var_types),
        Stmt::Expr(expr) => find_closure_param_in_expr(expr, name, var_types),
        Stmt::Return(ret) => ret
            .value
            .as_ref()
            .and_then(|e| find_closure_param_in_expr(e, name, var_types)),
        _ => None,
    }
}

/// Recursively search an expression tree for closure parameters.
fn find_closure_param_in_expr(
    expr: &rustscript_syntax::ast::Expr,
    name: &str,
    var_types: &HashMap<String, String>,
) -> Option<String> {
    find_closure_param_inner(expr, name, var_types, None)
}

fn find_closure_param_inner(
    expr: &rustscript_syntax::ast::Expr,
    name: &str,
    var_types: &HashMap<String, String>,
    inferred_element_type: Option<&str>,
) -> Option<String> {
    use rustscript_syntax::ast::ExprKind;

    match &expr.kind {
        ExprKind::Closure(closure) => {
            for param in &closure.params {
                if param.name.name == name {
                    let ty = format_type_ann(&param.type_ann);
                    if ty == "(inferred)" || ty == "inferred" || ty.is_empty() {
                        // Use inferred element type from collection method if available
                        if let Some(elem_ty) = inferred_element_type {
                            return Some(format!(
                                "```rustscript\n(parameter) {name}: {elem_ty}\n```"
                            ));
                        }
                        return Some(format!("```rustscript\n(parameter) {name}\n```"));
                    }
                    return Some(format!("```rustscript\n(parameter) {name}: {ty}\n```"));
                }
            }
            // Recurse into closure body
            match &closure.body {
                rustscript_syntax::ast::ClosureBody::Expr(e) => {
                    find_closure_param_inner(e, name, var_types, None)
                }
                rustscript_syntax::ast::ClosureBody::Block(block) => {
                    for s in &block.stmts {
                        if let Some(sig) = extract_closure_param_hover(s, name, var_types) {
                            return Some(sig);
                        }
                    }
                    None
                }
            }
        }
        ExprKind::MethodCall(mc) => {
            // Check the receiver first
            if let Some(sig) = find_closure_param_inner(&mc.object, name, var_types, None) {
                return Some(sig);
            }

            // For collection methods, infer the element type from the receiver
            let is_collection_method = matches!(
                mc.method.name.as_str(),
                "filter"
                    | "map"
                    | "find"
                    | "forEach"
                    | "some"
                    | "every"
                    | "findIndex"
                    | "flatMap"
                    | "reduce"
                    | "findLast"
            );

            let element_type = if is_collection_method {
                resolve_receiver_element_type(&mc.object, var_types)
            } else {
                None
            };

            // Check arguments with element type context
            for arg in &mc.args {
                if let Some(sig) =
                    find_closure_param_inner(arg, name, var_types, element_type.as_deref())
                {
                    return Some(sig);
                }
            }
            None
        }
        ExprKind::Call(call) => {
            for arg in &call.args {
                if let Some(sig) = find_closure_param_inner(arg, name, var_types, None) {
                    return Some(sig);
                }
            }
            None
        }
        ExprKind::Binary(bin) => find_closure_param_inner(&bin.left, name, var_types, None)
            .or_else(|| find_closure_param_inner(&bin.right, name, var_types, None)),
        ExprKind::Paren(inner) => find_closure_param_inner(inner, name, var_types, None),
        _ => None,
    }
}

/// Resolve the element type of a collection receiver expression.
/// e.g., `books` where `books: Array<Book>` -> `Some("Book")`
fn resolve_receiver_element_type(
    expr: &rustscript_syntax::ast::Expr,
    var_types: &HashMap<String, String>,
) -> Option<String> {
    use rustscript_syntax::ast::ExprKind;

    match &expr.kind {
        // Direct variable reference: look up in var_types
        ExprKind::Ident(ident) => {
            let type_str = var_types.get(&ident.name)?;
            extract_element_type(type_str).map(str::to_owned)
        }
        // Chained method that preserves element type (e.g., books.filter(...).map(...))
        ExprKind::MethodCall(mc) => {
            match mc.method.name.as_str() {
                "filter" | "reverse" | "slice" | "concat" | "sort" => {
                    // These preserve the element type
                    resolve_receiver_element_type(&mc.object, var_types)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Type inference
// ---------------------------------------------------------------------------

/// Context for type inference during hover.
struct InferCtx<'a> {
    fn_return_types: &'a HashMap<String, String>,
    fn_info: &'a HashMap<String, FnInfo>,
    var_types: &'a HashMap<String, String>,
    type_fields: &'a HashMap<String, Vec<(String, String)>>,
}

/// Collected function info for hover inference.
struct FnInfo {
    return_type: String,
    generic_params: Vec<String>,
}

/// Collect function name -> return type + generic params from top-level declarations.
fn collect_fn_info(module: &rustscript_syntax::ast::Module) -> HashMap<String, FnInfo> {
    use rustscript_syntax::ast::ItemKind;
    let mut map = HashMap::new();
    for item in &module.items {
        if let ItemKind::Function(f) = &item.kind
            && let Some(rta) = &f.return_type
            && let Some(ann) = &rta.type_ann
        {
            let generic_params = f
                .type_params
                .as_ref()
                .map(|tp| tp.params.iter().map(|p| p.name.name.clone()).collect())
                .unwrap_or_default();
            map.insert(
                f.name.name.clone(),
                FnInfo {
                    return_type: format_type_ann(ann),
                    generic_params,
                },
            );
        }
    }
    map
}

/// Legacy wrapper: collect just return types for backward compat.
#[allow(dead_code)]
fn collect_fn_return_types(module: &rustscript_syntax::ast::Module) -> HashMap<String, String> {
    collect_fn_info(module)
        .into_iter()
        .map(|(k, v)| (k, v.return_type))
        .collect()
}

/// Collect variable name -> type annotation string from a list of statements.
fn collect_var_types(stmts: &[rustscript_syntax::ast::Stmt]) -> HashMap<String, String> {
    use rustscript_syntax::ast::Stmt;
    let mut map = HashMap::new();
    for stmt in stmts {
        if let Stmt::VarDecl(decl) = stmt
            && let Some(ann) = &decl.type_ann
        {
            map.insert(decl.name.name.clone(), format_type_ann(ann));
        }
    }
    map
}

/// Collect type name -> vec of (`field_name`, `field_type`) from module.
fn collect_type_fields(
    module: &rustscript_syntax::ast::Module,
) -> HashMap<String, Vec<(String, String)>> {
    use rustscript_syntax::ast::ItemKind;
    let mut map = HashMap::new();
    for item in &module.items {
        if let ItemKind::TypeDef(td) = &item.kind {
            let fields: Vec<(String, String)> = td
                .fields
                .iter()
                .map(|f| (f.name.name.clone(), format_type_ann(&f.type_ann)))
                .collect();
            if !fields.is_empty() {
                map.insert(td.name.name.clone(), fields);
            }
        }
    }
    map
}

/// Info about a single variant in a discriminated union (data enum).
struct VariantInfo {
    /// The discriminant value (e.g., `"circle"`).
    discriminant: String,
    /// The data fields (excluding `kind`), as `(name, type)` pairs.
    fields: Vec<(String, String)>,
}

/// Collect enum name -> variant info from discriminated unions in the module.
fn collect_enum_variants(
    module: &rustscript_syntax::ast::Module,
) -> HashMap<String, Vec<VariantInfo>> {
    use rustscript_syntax::ast::{EnumVariant, ItemKind};
    let mut map = HashMap::new();
    for item in &module.items {
        if let ItemKind::EnumDef(ed) = &item.kind {
            let variants: Vec<VariantInfo> = ed
                .variants
                .iter()
                .filter_map(|v| match v {
                    EnumVariant::Data {
                        discriminant_value,
                        fields,
                        ..
                    } => Some(VariantInfo {
                        discriminant: discriminant_value.clone(),
                        fields: fields
                            .iter()
                            .map(|f| (f.name.name.clone(), format_type_ann(&f.type_ann)))
                            .collect(),
                    }),
                    EnumVariant::TypeRef { type_name, .. } => {
                        // Resolve the named type reference to extract variant info
                        resolve_type_ref_for_hover(&type_name.name, &module.items)
                    }
                    EnumVariant::Simple(..) => None,
                })
                .collect();
            if !variants.is_empty() {
                map.insert(ed.name.name.clone(), variants);
            }
        }
    }
    map
}

/// Resolve a `TypeRef` enum variant for hover by finding the referenced `TypeDef`.
fn resolve_type_ref_for_hover(
    type_name: &str,
    items: &[rustscript_syntax::ast::Item],
) -> Option<VariantInfo> {
    use rustscript_syntax::ast::{ItemKind, TypeKind};
    for item in items {
        if let ItemKind::TypeDef(td) = &item.kind {
            if td.name.name == type_name {
                // Find the `kind` field and extract the string literal discriminant
                let kind_field = td.fields.iter().find(|f| f.name.name == "kind")?;
                if let TypeKind::StringLiteral(ref disc_value) = kind_field.type_ann.kind {
                    let fields = td
                        .fields
                        .iter()
                        .filter(|f| f.name.name != "kind")
                        .map(|f| (f.name.name.clone(), format_type_ann(&f.type_ann)))
                        .collect();
                    return Some(VariantInfo {
                        discriminant: disc_value.clone(),
                        fields,
                    });
                }
                return None;
            }
        }
    }
    None
}

/// Convert a byte offset to a 0-based line number in the source text.
fn byte_offset_to_line(source: &str, offset: u32) -> usize {
    source
        .bytes()
        .take(offset as usize)
        .filter(|&b| b == b'\n')
        .count()
}

/// Check if the cursor is inside a switch case arm, and if the hovered name
/// matches the scrutinee variable, return a narrowed type showing only the
/// fields for that variant.
fn find_narrowed_type_in_switch(
    source: &str,
    stmts: &[rustscript_syntax::ast::Stmt],
    name: &str,
    cursor_line: usize,
    params: &[rustscript_syntax::ast::Param],
    enum_variants: &HashMap<String, Vec<VariantInfo>>,
) -> Option<String> {
    use rustscript_syntax::ast::{ExprKind, Stmt, SwitchPattern};

    for stmt in stmts {
        let Stmt::Switch(sw) = stmt else {
            continue;
        };

        // Get the scrutinee variable name
        let scrutinee_name = match &sw.scrutinee.kind {
            ExprKind::Ident(ident) => &ident.name,
            _ => continue,
        };

        // Only narrow if hovering the scrutinee variable
        if scrutinee_name != name {
            continue;
        }

        // Find the scrutinee's declared type (from params or variables)
        let scrutinee_type = params
            .iter()
            .find(|p| p.name.name == *scrutinee_name)
            .map(|p| format_type_ann(&p.type_ann));

        let scrutinee_type = scrutinee_type?;

        // Look up enum variants for this type
        let variants = enum_variants.get(&scrutinee_type)?;

        // Find which case arm the cursor is in
        for (i, case) in sw.cases.iter().enumerate() {
            let case_start = byte_offset_to_line(source, case.span.start.0);
            // Case end: use the start of the next case, or the switch span end
            let case_end = if i + 1 < sw.cases.len() {
                byte_offset_to_line(source, sw.cases[i + 1].span.start.0)
            } else {
                byte_offset_to_line(source, sw.span.end.0)
            };

            if cursor_line < case_start || cursor_line >= case_end {
                continue;
            }

            // Get the discriminant value from the case pattern
            let discriminant = match &case.pattern {
                SwitchPattern::StringLit(s) => s,
                _ => continue,
            };

            // Find the matching variant
            let variant = variants.iter().find(|v| v.discriminant == *discriminant)?;

            // Format the narrowed type
            let fields_str: Vec<String> =
                std::iter::once(format!("  kind: \"{}\"", variant.discriminant))
                    .chain(
                        variant
                            .fields
                            .iter()
                            .map(|(fname, ftype)| format!("  {fname}: {ftype}")),
                    )
                    .collect();

            return Some(format!(
                "```rustscript\n(parameter) {name}: {scrutinee_type} (narrowed)\n{{\n{}\n}}\n```",
                fields_str.join(",\n")
            ));
        }
    }
    None
}

/// Extract the element type from a collection type string.
/// e.g., `"Array<Book>"` -> `Some("Book")`, `"Map<string, i32>"` -> `None`
fn extract_element_type(type_str: &str) -> Option<&str> {
    let trimmed = type_str.trim();
    if let Some(inner) = trimmed
        .strip_prefix("Array<")
        .and_then(|s| s.strip_suffix('>'))
    {
        Some(inner)
    } else if let Some(inner) = trimmed
        .strip_prefix("Set<")
        .and_then(|s| s.strip_suffix('>'))
    {
        Some(inner)
    } else {
        None
    }
}

/// Try to infer the type of an expression for hover display.
#[allow(dead_code)] // reserved for legacy hover path
fn infer_type_from_expr(
    expr: &rustscript_syntax::ast::Expr,
    fn_return_types: &HashMap<String, String>,
) -> Option<String> {
    // Legacy wrapper -- builds minimal context
    let empty_var = HashMap::new();
    let empty_fields = HashMap::new();
    let empty_info = HashMap::new();
    let ctx = InferCtx {
        fn_return_types,
        fn_info: &empty_info,
        var_types: &empty_var,
        type_fields: &empty_fields,
    };
    infer_type_from_expr_ctx(expr, &ctx)
}

/// Try to infer the type of an expression with full context.
fn infer_type_from_expr_ctx(
    expr: &rustscript_syntax::ast::Expr,
    ctx: &InferCtx<'_>,
) -> Option<String> {
    use rustscript_syntax::ast::ExprKind;

    match &expr.kind {
        // Function call -> look up return type, substitute generic args if present
        ExprKind::Call(call) => {
            let raw_return = ctx.fn_return_types.get(&call.callee.name)?;

            // If the call has explicit type arguments and the function has generic params,
            // substitute them into the return type.
            if !call.type_args.is_empty()
                && let Some(info) = ctx.fn_info.get(&call.callee.name)
            {
                let mut result = raw_return.clone();
                for (param_name, type_arg) in info.generic_params.iter().zip(&call.type_args) {
                    let concrete = format_type_ann(type_arg);
                    result = result.replace(param_name.as_str(), &concrete);
                }
                return Some(result);
            }

            Some(raw_return.clone())
        }
        // String / template literal -> string
        ExprKind::StringLit(_) | ExprKind::TemplateLit(_) => Some("string".to_owned()),
        // Number literal -> i64 or f64
        ExprKind::IntLit(_) => Some("i64".to_owned()),
        ExprKind::FloatLit(_) => Some("f64".to_owned()),
        // Boolean literal -> boolean
        ExprKind::BoolLit(_) => Some("boolean".to_owned()),
        // Array literal -> Array<...>
        ExprKind::ArrayLit(_) => Some("Array<...>".to_owned()),
        // Await -> unwrap the inner expression
        ExprKind::Await(inner) => infer_type_from_expr_ctx(inner, ctx),
        // Field access -> look up field type on receiver
        ExprKind::FieldAccess(fa) => {
            // Try to infer the receiver type, then look up the field
            let receiver_type = infer_type_from_expr_ctx(&fa.object, ctx)?;
            let fields = ctx.type_fields.get(&receiver_type)?;
            fields
                .iter()
                .find(|(name, _)| name == &fa.field.name)
                .map(|(_, ty)| ty.clone())
        }
        // Identifier -> look up in var_types
        ExprKind::Ident(ident) => ctx.var_types.get(&ident.name).cloned(),
        // Method call on known collection method
        ExprKind::MethodCall(mc) => {
            match mc.method.name.as_str() {
                "filter" | "sort" | "reverse" | "slice" | "concat" => {
                    // Preserves the collection type
                    infer_type_from_expr_ctx(&mc.object, ctx)
                }
                "map" => {
                    // map return type is Array<ReturnTypeOfClosure>
                    // Try to infer from the closure argument
                    if let Some(closure_arg) = mc.args.first()
                        && let ExprKind::Closure(closure) = &closure_arg.kind
                    {
                        let closure_ret = infer_closure_return_type(closure, &mc.object, ctx);
                        if let Some(ret) = closure_ret {
                            return Some(format!("Array<{ret}>"));
                        }
                    }
                    Some("Array<...>".to_owned())
                }
                "find" => {
                    // Returns element type | null
                    let receiver_type = infer_type_from_expr_ctx(&mc.object, ctx)?;
                    extract_element_type(&receiver_type).map(|e| format!("{e} | null"))
                }
                "join" | "toString" => Some("string".to_owned()),
                "length" | "indexOf" | "findIndex" => Some("i64".to_owned()),
                "some" | "every" | "includes" => Some("boolean".to_owned()),
                "pop" | "shift" => {
                    let receiver_type = infer_type_from_expr_ctx(&mc.object, ctx)?;
                    extract_element_type(&receiver_type).map(|e| format!("{e} | null"))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Infer the return type of a closure, using the receiver's element type for param inference.
fn infer_closure_return_type(
    closure: &rustscript_syntax::ast::ClosureExpr,
    receiver: &rustscript_syntax::ast::Expr,
    ctx: &InferCtx<'_>,
) -> Option<String> {
    use rustscript_syntax::ast::ClosureBody;

    // If the closure has an explicit return type, use it
    if let Some(rt) = &closure.return_type {
        return Some(format_type_ann(rt));
    }

    // For expression body closures (b => b.title), infer from the body
    if let ClosureBody::Expr(body_expr) = &closure.body {
        // Build a temporary context with the closure param's inferred type
        let receiver_type = infer_type_from_expr_ctx(receiver, ctx)?;
        let element_type = extract_element_type(&receiver_type)?;

        // If the body is a field access on the param, look up the field type
        if let rustscript_syntax::ast::ExprKind::FieldAccess(fa) = &body_expr.kind
            && let rustscript_syntax::ast::ExprKind::Ident(ident) = &fa.object.kind
            && closure.params.first().map(|p| &p.name.name) == Some(&ident.name)
        {
            // Look up field type on the element type
            let fields = ctx.type_fields.get(element_type)?;
            return fields
                .iter()
                .find(|(name, _)| name == &fa.field.name)
                .map(|(_, ty)| ty.clone());
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Format a full type definition for hover display.
fn format_type_def_hover(name: &str, td: &rustscript_syntax::ast::TypeDef) -> String {
    let generics = td.type_params.as_ref().map_or_else(String::new, |tp| {
        let params: Vec<String> = tp.params.iter().map(|p| p.name.name.clone()).collect();
        if params.is_empty() {
            String::new()
        } else {
            format!("<{}>", params.join(", "))
        }
    });

    let derives_str = if td.derives.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = td.derives.iter().map(|d| d.name.as_str()).collect();
        format!(" derives {}", names.join(", "))
    };

    if !td.fields.is_empty() {
        let fields: Vec<String> = td
            .fields
            .iter()
            .map(|f| {
                let opt = if f.optional { "?" } else { "" };
                format!("  {}{opt}: {}", f.name.name, format_type_ann(&f.type_ann))
            })
            .collect();
        format!(
            "```rustscript\ntype {name}{generics} = {{\n{}\n}}{derives_str}\n```",
            fields.join(",\n")
        )
    } else if let Some(ref alias) = td.type_alias {
        format!(
            "```rustscript\ntype {name}{generics} = {}{derives_str}\n```",
            format_type_ann(alias)
        )
    } else {
        format!("```rustscript\ntype {name}{generics}{derives_str}\n```")
    }
}

/// Prepend a doc comment to a signature if present.
fn with_doc_comment(doc: Option<&String>, sig: &str) -> String {
    match doc {
        Some(comment) if !comment.is_empty() => format!("{comment}\n\n---\n\n{sig}"),
        _ => sig.to_owned(),
    }
}

/// Format a type annotation for display.
#[must_use]
pub fn format_type_ann(ty: &rustscript_syntax::ast::TypeAnnotation) -> String {
    use rustscript_syntax::ast::TypeKind;

    match &ty.kind {
        TypeKind::Named(ident) => ident.name.clone(),
        TypeKind::Void => "void".to_owned(),
        TypeKind::Never => "never".to_owned(),
        TypeKind::Unknown => "unknown".to_owned(),
        TypeKind::Inferred => "(inferred)".to_owned(),
        TypeKind::Generic(ident, args) => {
            let args_str: Vec<String> = args.iter().map(format_type_ann).collect();
            format!("{}<{}>", ident.name, args_str.join(", "))
        }
        TypeKind::Union(types) => {
            let inner: Vec<String> = types.iter().map(format_type_ann).collect();
            inner.join(" | ")
        }
        TypeKind::Function(params, ret) => {
            let params_str: Vec<String> = params.iter().map(format_type_ann).collect();
            format!("({}) => {}", params_str.join(", "), format_type_ann(ret))
        }
        TypeKind::Intersection(types) => {
            let inner: Vec<String> = types.iter().map(format_type_ann).collect();
            inner.join(" & ")
        }
        TypeKind::Shared(inner) => format!("shared<{}>", format_type_ann(inner)),
        TypeKind::Tuple(types) => {
            let inner: Vec<String> = types.iter().map(format_type_ann).collect();
            format!("[{}]", inner.join(", "))
        }
        TypeKind::StringLiteral(s) => format!("\"{s}\""),
        TypeKind::KeyOf(inner) => format!("keyof {}", format_type_ann(inner)),
        TypeKind::TypeOf(ident) => format!("typeof {}", ident.name),
        TypeKind::IndexSignature(idx) => {
            format!(
                "{{ [{}]: {} }}",
                format_type_ann(&idx.key_type),
                format_type_ann(&idx.value_type)
            )
        }
        TypeKind::IndexAccess(obj, idx) => {
            format!("{}[{}]", format_type_ann(obj), format_type_ann(idx))
        }
        TypeKind::Readonly(inner) => format!("readonly {}", format_type_ann(inner)),
        TypeKind::Conditional { .. } => "...".to_owned(),
        TypeKind::Infer(ident) => format!("infer {}", ident.name),
        TypeKind::TupleSpread(inner) => format!("...{}", format_type_ann(inner)),
        TypeKind::TypeGuard {
            param,
            guarded_type,
        } => {
            format!("{} is {}", param.name, format_type_ann(guarded_type))
        }
        TypeKind::Asserts {
            param,
            guarded_type,
        } => {
            if let Some(gt) = guarded_type {
                format!("asserts {} is {}", param.name, format_type_ann(gt))
            } else {
                format!("asserts {}", param.name)
            }
        }
        TypeKind::TemplateLiteralType { .. } => "string".to_owned(),
        TypeKind::MappedType { .. } => "{ [key: string]: ... }".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hover_builtin_console() {
        let source = "function main() { console.log(\"hello\"); }";
        // "console" starts at 0-based col 18, 1-based col 19
        let result = hover(source, 1, 19);
        assert!(
            result.contains("console"),
            "expected console hover, got: {result}"
        );
    }

    #[test]
    fn test_hover_builtin_log() {
        let source = "function main() { console.log(\"hello\"); }";
        // "log" starts at 0-based col 26, 1-based col 27
        let result = hover(source, 1, 27);
        assert!(result.contains("log"), "expected log hover, got: {result}");
    }

    #[test]
    fn test_hover_user_function() {
        let source = "function greet(name: string): string { return name; }";
        // "greet" starts at 0-based col 9, 1-based col 10
        let result = hover(source, 1, 10);
        assert!(
            result.contains("function greet"),
            "expected greet signature, got: {result}"
        );
    }

    #[test]
    fn test_hover_doc_comment() {
        let source =
            "/** Greets a person */\nfunction greet(name: string): string { return name; }";
        let result = hover(source, 2, 10);
        assert!(
            result.contains("Greets a person"),
            "expected doc comment in hover, got: {result}"
        );
        assert!(
            result.contains("function greet"),
            "expected signature in hover, got: {result}"
        );
    }

    #[test]
    fn test_hover_out_of_bounds() {
        let source = "let x = 1;";
        let result = hover(source, 99, 1);
        assert!(result.is_empty());
    }

    #[test]
    fn test_hover_type_def() {
        let source = "type User = { name: string, age: i64 }";
        // "User" starts at col 5, 1-based col 6
        let result = hover(source, 1, 6);
        assert!(
            result.contains("type User"),
            "expected type hover, got: {result}"
        );
    }

    #[test]
    fn test_hover_interface() {
        let source = "interface Printable { toString(): string }";
        // "Printable" starts at col 10, 1-based col 11
        let result = hover(source, 1, 11);
        assert!(
            result.contains("interface Printable"),
            "expected interface hover, got: {result}"
        );
    }

    #[test]
    fn test_hover_parameter() {
        let source = "function add(a: i32, b: i32): i32 { return a + b; }";
        // "a" is in "a + b" at col 43, 1-based col 44
        // Actually, let's hover on the param declaration: "a" at col 13, 1-based col 14
        let result = hover(source, 1, 14);
        assert!(
            result.contains("(parameter)") && result.contains("a"),
            "expected parameter hover, got: {result}"
        );
    }

    #[test]
    fn test_hover_empty_on_whitespace() {
        let source = "let x = 1;";
        // col 3 (0-based) is a space, 1-based col 4
        let result = hover(source, 1, 4);
        assert!(
            result.is_empty(),
            "expected empty for whitespace, got: {result}"
        );
    }

    #[test]
    fn test_format_type_ann_named() {
        use rustscript_syntax::ast::{Ident, TypeAnnotation, TypeKind};
        use rustscript_syntax::span::Span;

        let ty = TypeAnnotation {
            kind: TypeKind::Named(Ident {
                name: "string".to_owned(),
                span: Span::dummy(),
            }),
            span: Span::dummy(),
        };
        assert_eq!(format_type_ann(&ty), "string");
    }

    #[test]
    fn test_format_type_ann_void() {
        use rustscript_syntax::ast::{TypeAnnotation, TypeKind};
        use rustscript_syntax::span::Span;

        let ty = TypeAnnotation {
            kind: TypeKind::Void,
            span: Span::dummy(),
        };
        assert_eq!(format_type_ann(&ty), "void");
    }

    #[test]
    fn test_hover_switch_narrowing() {
        let source = r#"type Shape =
  | { kind: "circle", radius: f64 }
  | { kind: "rect", width: f64, height: f64 }

function area(shape: Shape): f64 {
  switch (shape) {
    case "circle":
      return 3.14 * shape.radius * shape.radius;
    case "rect":
      return shape.width * shape.height;
  }
}"#;
        // "shape" on line 8 (1-based), inside case "circle" arm
        // "shape" starts at col 20 (0-based), 1-based col 21
        let result = hover(source, 8, 21);
        assert!(
            result.contains("narrowed"),
            "expected narrowed type, got: {result}"
        );
        assert!(
            result.contains("radius"),
            "expected radius field in narrowed type, got: {result}"
        );
        assert!(
            !result.contains("width"),
            "should NOT contain rect fields, got: {result}"
        );
    }

    #[test]
    fn test_hover_switch_narrowing_second_arm() {
        let source = r#"type Shape =
  | { kind: "circle", radius: f64 }
  | { kind: "rect", width: f64, height: f64 }

function area(shape: Shape): f64 {
  switch (shape) {
    case "circle":
      return 3.14 * shape.radius * shape.radius;
    case "rect":
      return shape.width * shape.height;
  }
}"#;
        // "shape" on line 10 (1-based), inside case "rect" arm
        let result = hover(source, 10, 14);
        assert!(
            result.contains("narrowed"),
            "expected narrowed type, got: {result}"
        );
        assert!(
            result.contains("width"),
            "expected width field, got: {result}"
        );
        assert!(
            result.contains("height"),
            "expected height field, got: {result}"
        );
    }

    #[test]
    fn test_hover_switch_narrowing_with_type_ref_variants() {
        let source = r#"type Circle = { kind: "circle", radius: f64 }
type Rect = { kind: "rect", width: f64, height: f64 }

type Shape =
  | Circle
  | Rect

function area(shape: Shape): f64 {
  switch (shape) {
    case "circle":
      return 3.14 * shape.radius * shape.radius;
    case "rect":
      return shape.width * shape.height;
  }
}"#;
        // "shape" on line 11 (1-based), inside case "circle" arm
        // `      return 3.14 * shape.radius` — "shape" starts at col 21
        let result = hover(source, 11, 21);
        assert!(
            result.contains("narrowed"),
            "expected narrowed type, got: {result}"
        );
        assert!(
            result.contains("radius"),
            "expected radius field in narrowed type, got: {result}"
        );
        assert!(
            !result.contains("width"),
            "should NOT contain rect fields, got: {result}"
        );
    }
}
