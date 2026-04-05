//! Pretty-printer for the `RustScript` AST.
//!
//! Walks the parsed AST and emits canonical formatted source text with
//! consistent indentation, spacing, and layout. The printer tracks
//! indentation level and line position to produce clean output.

use rustscript_syntax::ast::{
    ArrayDestructureStmt, ArrayElement, AssignExpr, BinaryExpr, Block, CallExpr, ClassDef,
    ClassGetter, ClassMember, ClassSetter, ClosureBody, ClosureExpr, ConstructorParam,
    DestructureStmt, DoWhileStmt, ElseClause, EnumDef, EnumVariant, Expr, ExprKind,
    FieldAccessExpr, FieldAssignExpr, FieldDef, FieldInit, FnDecl, ForClassicStmt, ForInStmt,
    ForInit, ForOfStmt, IfStmt, ImportDecl, IndexAssignExpr, IndexExpr, InlineRustBlock,
    InterfaceDef, InterfaceField, InterfaceMethod, Item, ItemKind, LogicalAssignExpr,
    MethodCallExpr, Module, NewExpr, NullishCoalescingExpr, OptionalAccess, OptionalChainExpr,
    Param, ReExportDecl, ReturnStmt, ReturnTypeAnnotation, StructLitExpr, SwitchCase,
    SwitchPattern, SwitchStmt, TemplateLitExpr, TemplatePart, TestBlock, TestBlockKind, TestBody,
    TryCatchStmt, TypeAnnotation, TypeDef, TypeKind, TypeParam, TypeParams, UnaryExpr, UnaryOp,
    UsingDecl, VarBinding, VarDecl, Visibility, WhileStmt, WildcardReExportDecl,
};

/// Indentation unit: 2 spaces per level.
const INDENT: &str = "  ";

/// Pretty-printer that walks the AST and emits formatted source text.
pub(crate) struct Printer {
    output: String,
    indent_level: usize,
    at_line_start: bool,
}

impl Printer {
    /// Create a new printer with empty output.
    pub(crate) fn new() -> Self {
        Self {
            output: String::new(),
            indent_level: 0,
            at_line_start: true,
        }
    }

    /// Consume the printer and return the formatted output.
    pub(crate) fn into_output(self) -> String {
        self.output
    }

    /// Increase indentation by one level.
    fn indent(&mut self) {
        self.indent_level += 1;
    }

    /// Decrease indentation by one level.
    fn dedent(&mut self) {
        self.indent_level = self.indent_level.saturating_sub(1);
    }

    /// Write text to the output, emitting indentation if at the start of a line.
    fn write(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if self.at_line_start {
            for _ in 0..self.indent_level {
                self.output.push_str(INDENT);
            }
            self.at_line_start = false;
        }
        self.output.push_str(s);
    }

    /// Write a newline, setting the line-start flag.
    fn newline(&mut self) {
        self.output.push('\n');
        self.at_line_start = true;
    }

    /// Write text followed by a newline.
    fn writeln(&mut self, s: &str) {
        self.write(s);
        self.newline();
    }

    /// Emit a blank line (only if the last line isn't already blank).
    fn blank_line(&mut self) {
        // Avoid double blank lines: check if output already ends with \n\n
        if self.output.ends_with("\n\n") {
            return;
        }
        // If we're not at a line start, finish the current line first
        if !self.at_line_start {
            self.newline();
        }
        self.newline();
    }

    /// Print a complete module.
    pub(crate) fn print_module(&mut self, module: &Module) {
        // Separate imports from other items for sorting
        let mut imports: Vec<&ImportDecl> = Vec::new();
        let mut other_items: Vec<&Item> = Vec::new();

        for item in &module.items {
            match &item.kind {
                ItemKind::Import(imp) => imports.push(imp),
                _ => other_items.push(item),
            }
        }

        // Sort imports alphabetically by source path
        let mut sorted_imports = imports;
        sorted_imports.sort_by(|a, b| a.source.value.cmp(&b.source.value));

        // Print sorted imports
        for imp in &sorted_imports {
            self.print_import(imp);
        }

        // Blank line after imports if there are both imports and other items
        if !sorted_imports.is_empty() && !other_items.is_empty() {
            self.blank_line();
        }

        // Print other items with blank lines between them
        for (i, item) in other_items.iter().enumerate() {
            if i > 0 {
                self.blank_line();
            }
            self.print_item(item);
        }

        // Ensure trailing newline
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.newline();
        }
    }

    /// Print a top-level item.
    fn print_item(&mut self, item: &Item) {
        // Emit decorators before the item
        for decorator in &item.decorators {
            self.indent();
            self.write("@");
            self.write(&decorator.name);
            if let Some(ref args) = decorator.args {
                self.write("(");
                self.write(args);
                self.write(")");
            }
            self.newline();
        }
        // Emit doc comment before the item if present
        let doc = match &item.kind {
            ItemKind::Function(f) => f.doc_comment.as_deref(),
            ItemKind::TypeDef(t) => t.doc_comment.as_deref(),
            ItemKind::EnumDef(e) => e.doc_comment.as_deref(),
            ItemKind::Interface(i) => i.doc_comment.as_deref(),
            ItemKind::Class(c) => c.doc_comment.as_deref(),
            _ => None,
        };
        if let Some(doc) = doc {
            self.print_jsdoc(doc);
        }
        if item.exported {
            self.write("export ");
        }
        match &item.kind {
            ItemKind::Function(f) => self.print_fn_decl(f),
            ItemKind::TypeDef(t) => self.print_type_def(t),
            ItemKind::EnumDef(e) => self.print_enum_def(e),
            ItemKind::Interface(i) => self.print_interface_def(i),
            ItemKind::Import(imp) => self.print_import(imp),
            ItemKind::ReExport(re) => self.print_re_export(re),
            ItemKind::WildcardReExport(re) => self.print_wildcard_re_export(re),
            ItemKind::Class(c) => self.print_class_def(c),
            ItemKind::RustBlock(rb) => self.print_rust_block(rb),
            ItemKind::Const(decl) => self.print_var_decl(decl),
            ItemKind::TestBlock(tb) => self.print_test_block(tb),
        }
    }

    /// Print a test block: `test("name", () => { ... })`.
    fn print_test_block(&mut self, tb: &TestBlock) {
        let keyword = match tb.kind {
            TestBlockKind::Test => "test",
            TestBlockKind::Describe => "describe",
            TestBlockKind::It => "it",
        };
        self.write(keyword);
        self.write("(\"");
        self.write(&tb.description);
        self.write("\", () => ");
        match &tb.body {
            TestBody::Stmts(block) => self.print_block(block),
            TestBody::Items(items) => {
                self.writeln("{");
                self.indent();
                for item in items {
                    self.print_test_block(item);
                    self.newline();
                }
                self.dedent();
                self.write("}");
            }
        }
        self.writeln(");");
    }

    /// Print a `JSDoc` comment block: `/** ... */`.
    fn print_jsdoc(&mut self, doc: &str) {
        self.writeln("/**");
        for line in doc.lines() {
            self.write(" * ");
            self.writeln(line);
        }
        self.writeln(" */");
    }

    /// Print an import declaration.
    fn print_import(&mut self, imp: &ImportDecl) {
        if imp.is_type_only {
            self.write("import type { ");
        } else {
            self.write("import { ");
        }
        for (i, name) in imp.names.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&name.name);
        }
        self.write(" } from \"");
        self.write(&imp.source.value);
        self.writeln("\";");
    }

    /// Print a re-export declaration.
    fn print_re_export(&mut self, re: &ReExportDecl) {
        self.write("export { ");
        for (i, name) in re.names.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&name.name);
        }
        self.write(" } from \"");
        self.write(&re.source.value);
        self.writeln("\";");
    }

    /// Print a wildcard re-export: `export * from "./module";`.
    fn print_wildcard_re_export(&mut self, re: &WildcardReExportDecl) {
        self.write("export * from \"");
        self.write(&re.source.value);
        self.writeln("\";");
    }

    /// Print a function declaration.
    fn print_fn_decl(&mut self, f: &FnDecl) {
        if f.is_async {
            self.write("async ");
        }
        if f.is_generator {
            self.write("function* ");
        } else {
            self.write("function ");
        }
        self.write(&f.name.name);
        self.print_optional_type_params(f.type_params.as_ref());
        self.write("(");
        self.print_params(&f.params);
        self.write(")");
        if let Some(ret) = &f.return_type {
            self.print_return_type(ret);
        }
        self.write(" ");
        self.print_block(&f.body);
        self.newline();
    }

    /// Print optional generic type parameters.
    fn print_optional_type_params(&mut self, type_params: Option<&TypeParams>) {
        if let Some(tp) = type_params {
            self.write("<");
            for (i, param) in tp.params.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.print_type_param(param);
            }
            self.write(">");
        }
    }

    /// Print a single type parameter.
    fn print_type_param(&mut self, param: &TypeParam) {
        self.write(&param.name.name);
        if let Some(constraint) = &param.constraint {
            self.write(" extends ");
            self.print_type_annotation(constraint);
        }
    }

    /// Print a parameter list (without surrounding parentheses).
    fn print_params(&mut self, params: &[Param]) {
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            if param.is_rest {
                self.write("...");
            }
            if let Some(fields) = &param.destructure_fields {
                // Destructuring param: `{ field, field }: Type`
                self.write("{ ");
                for (j, field) in fields.iter().enumerate() {
                    if j > 0 {
                        self.write(", ");
                    }
                    self.write(&field.field_name.name);
                    if let Some(local) = &field.local_name {
                        self.write(": ");
                        self.write(&local.name);
                    }
                }
                self.write(" }");
            } else {
                self.write(&param.name.name);
            }
            if param.optional {
                self.write("?");
            }
            self.write(": ");
            self.print_type_annotation(&param.type_ann);
            if let Some(default) = &param.default_value {
                self.write(" = ");
                self.print_expr(default);
            }
        }
    }

    /// Print a return type annotation.
    fn print_return_type(&mut self, ret: &ReturnTypeAnnotation) {
        if let Some(type_ann) = &ret.type_ann {
            self.write(": ");
            self.print_type_annotation(type_ann);
        }
        if let Some(throws) = &ret.throws {
            self.write(" throws ");
            self.print_type_annotation(throws);
        }
    }

    /// Print a type annotation.
    #[allow(clippy::too_many_lines)]
    // Match arms for all TypeKind variants; splitting would obscure the type printing
    fn print_type_annotation(&mut self, ty: &TypeAnnotation) {
        match &ty.kind {
            TypeKind::Named(ident) => self.write(&ident.name),
            TypeKind::Void => self.write("void"),
            TypeKind::Never => self.write("never"),
            TypeKind::Unknown => self.write("unknown"),
            TypeKind::Generic(name, args) => {
                self.write(&name.name);
                self.write("<");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type_annotation(arg);
                }
                self.write(">");
            }
            TypeKind::Union(types) => {
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        self.write(" | ");
                    }
                    self.print_type_annotation(t);
                }
            }
            TypeKind::Function(params, ret) => {
                self.write("(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type_annotation(p);
                }
                self.write(") => ");
                self.print_type_annotation(ret);
            }
            TypeKind::Intersection(types) => {
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        self.write(" & ");
                    }
                    self.print_type_annotation(t);
                }
            }
            TypeKind::Inferred => {}
            TypeKind::Shared(inner) => {
                self.write("shared<");
                self.print_type_annotation(inner);
                self.write(">");
            }
            TypeKind::Tuple(types) => {
                self.write("[");
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type_annotation(t);
                }
                self.write("]");
            }
            TypeKind::IndexSignature(sig) => {
                self.write("{ [");
                self.write(&sig.key_name.name);
                self.write(": ");
                self.print_type_annotation(&sig.key_type);
                self.write("]: ");
                self.print_type_annotation(&sig.value_type);
                self.write(" }");
            }
            TypeKind::StringLiteral(value) => {
                self.write("\"");
                self.write(value);
                self.write("\"");
            }
            TypeKind::KeyOf(inner) => {
                self.write("keyof ");
                self.print_type_annotation(inner);
            }
            TypeKind::TypeOf(ident) => {
                self.write("typeof ");
                self.write(&ident.name);
            }
            TypeKind::Conditional {
                check_type,
                extends_type,
                true_type,
                false_type,
            } => {
                self.print_type_annotation(check_type);
                self.write(" extends ");
                self.print_type_annotation(extends_type);
                self.write(" ? ");
                self.print_type_annotation(true_type);
                self.write(" : ");
                self.print_type_annotation(false_type);
            }
            TypeKind::Infer(ident) => {
                self.write("infer ");
                self.write(&ident.name);
            }
            TypeKind::TupleSpread(inner) => {
                self.write("...");
                self.print_type_annotation(inner);
            }
            TypeKind::TypeGuard {
                param,
                guarded_type,
            } => {
                self.write(&param.name);
                self.write(" is ");
                self.print_type_annotation(guarded_type);
            }
            TypeKind::Asserts {
                param,
                guarded_type,
            } => {
                self.write("asserts ");
                self.write(&param.name);
                if let Some(gt) = guarded_type {
                    self.write(" is ");
                    self.print_type_annotation(gt);
                }
            }
            TypeKind::Readonly(inner) => {
                self.write("readonly ");
                self.print_type_annotation(inner);
            }
            TypeKind::TemplateLiteralType { quasis, types } => {
                self.write("`");
                for (i, quasi) in quasis.iter().enumerate() {
                    self.write(quasi);
                    if i < types.len() {
                        self.write("${");
                        self.print_type_annotation(&types[i]);
                        self.write("}");
                    }
                }
                self.write("`");
            }
            TypeKind::MappedType {
                type_param,
                constraint,
                value_type,
                optional,
                readonly,
            } => {
                self.write("{ ");
                if readonly.is_some() {
                    self.write("readonly ");
                }
                self.write("[");
                self.write(&type_param.name);
                self.write(" in ");
                self.print_type_annotation(constraint);
                self.write("]");
                if let Some(modifier) = optional {
                    match modifier {
                        rustscript_syntax::ast::MappedModifier::Add => self.write("?"),
                        rustscript_syntax::ast::MappedModifier::Remove => self.write("-?"),
                    }
                }
                self.write(": ");
                self.print_type_annotation(value_type);
                self.write(" }");
            }
            TypeKind::IndexAccess(object_type, index_type) => {
                self.print_type_annotation(object_type);
                self.write("[");
                self.print_type_annotation(index_type);
                self.write("]");
            }
        }
    }

    /// Print a type definition.
    fn print_type_def(&mut self, t: &TypeDef) {
        self.write("type ");
        self.write(&t.name.name);
        self.print_optional_type_params(t.type_params.as_ref());
        if let Some(ref alias) = t.type_alias {
            self.write(" = ");
            self.print_type_annotation(alias);
            self.print_derives_clause(&t.derives);
            self.writeln(";");
        } else {
            self.writeln(" = {");
            self.indent();
            for field in &t.fields {
                self.print_field_def(field);
                self.writeln(",");
            }
            if let Some(sig) = &t.index_signature {
                self.write("[");
                self.write(&sig.key_name.name);
                self.write(": ");
                self.print_type_annotation(&sig.key_type);
                self.write("]: ");
                self.print_type_annotation(&sig.value_type);
                self.writeln(",");
            }
            self.dedent();
            self.write("}");
            self.print_derives_clause(&t.derives);
            self.writeln(";");
        }
    }

    /// Print a field definition.
    fn print_field_def(&mut self, field: &FieldDef) {
        self.write(&field.name.name);
        if field.optional {
            self.write("?: ");
        } else {
            self.write(": ");
        }
        self.print_type_annotation(&field.type_ann);
    }

    /// Print an optional `derives Ident, Ident, ...` clause.
    fn print_derives_clause(&mut self, derives: &[rustscript_syntax::ast::Ident]) {
        if derives.is_empty() {
            return;
        }
        self.write(" derives ");
        for (i, derive) in derives.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&derive.name);
        }
    }

    /// Print an enum definition.
    fn print_enum_def(&mut self, e: &EnumDef) {
        if e.is_const {
            // TS-style const enum: `const enum Name { Variant, ... }`
            self.print_ts_enum_def(e, true);
            return;
        }

        self.write("type ");
        self.write(&e.name.name);
        self.write(" =");

        // Check if all variants are simple (string union) vs data variants
        let all_simple = e
            .variants
            .iter()
            .all(|v| matches!(v, EnumVariant::Simple(..)));

        if all_simple {
            // Single-line string union format
            for (i, variant) in e.variants.iter().enumerate() {
                if i > 0 {
                    self.write(" |");
                }
                if let EnumVariant::Simple(ident, _) = variant {
                    // Convert PascalCase name back to the original string literal
                    self.write(" \"");
                    self.write(&ident.name.to_lowercase());
                    self.write("\"");
                }
            }
            self.print_derives_clause(&e.derives);
            self.writeln(";");
        } else {
            // Multi-line discriminated union format
            self.newline();
            self.indent();
            for variant in &e.variants {
                match variant {
                    EnumVariant::Simple(ident, _) => {
                        self.write("| \"");
                        self.write(&ident.name.to_lowercase());
                        self.writeln("\"");
                    }
                    EnumVariant::Data {
                        discriminant_value,
                        fields,
                        ..
                    } => {
                        self.write("| { kind: \"");
                        self.write(discriminant_value);
                        self.write("\"");
                        for field in fields {
                            self.write(", ");
                            self.write(&field.name.name);
                            self.write(": ");
                            self.print_type_annotation(&field.type_ann);
                        }
                        self.writeln(" }");
                    }
                    EnumVariant::TypeRef { type_name, .. } => {
                        self.write("| ");
                        self.writeln(&type_name.name);
                    }
                }
            }
            self.dedent();
            self.print_derives_clause(&e.derives);
            self.writeln(";");
        }
    }

    /// Print a TS-style enum definition: `[const] enum Name { Variant, ... }`.
    fn print_ts_enum_def(&mut self, e: &EnumDef, is_const: bool) {
        if is_const {
            self.write("const ");
        }
        self.write("enum ");
        self.write(&e.name.name);
        self.writeln(" {");
        self.indent();
        for (i, variant) in e.variants.iter().enumerate() {
            if let EnumVariant::Simple(ident, _) = variant {
                self.write(&ident.name);
                if i + 1 < e.variants.len() {
                    self.writeln(",");
                } else {
                    self.newline();
                }
            }
        }
        self.dedent();
        self.write("}");
        self.print_derives_clause(&e.derives);
        self.newline();
    }

    /// Print an interface definition.
    fn print_interface_def(&mut self, iface: &InterfaceDef) {
        self.write("interface ");
        self.write(&iface.name.name);
        self.print_optional_type_params(iface.type_params.as_ref());
        self.writeln(" {");
        self.indent();
        for field in &iface.fields {
            self.print_interface_field(field);
        }
        for method in &iface.methods {
            self.print_interface_method(method);
        }
        self.dedent();
        self.writeln("}");
    }

    /// Print an interface method signature.
    fn print_interface_method(&mut self, method: &InterfaceMethod) {
        self.write(&method.name.name);
        self.write("(");
        self.print_params(&method.params);
        self.write(")");
        if let Some(ret) = &method.return_type {
            self.print_return_type(ret);
        }
        self.writeln(";");
    }

    /// Print an interface field declaration.
    fn print_interface_field(&mut self, field: &InterfaceField) {
        self.write(&field.name.name);
        self.write(": ");
        self.print_type_annotation(&field.type_ann);
        self.writeln(";");
    }

    /// Print a class definition.
    fn print_class_def(&mut self, class: &ClassDef) {
        if class.is_abstract {
            self.write("abstract ");
        }
        self.write("class ");
        self.write(&class.name.name);
        self.print_optional_type_params(class.type_params.as_ref());
        if let Some(base) = &class.extends {
            self.write(" extends ");
            self.write(&base.name);
        }
        if !class.implements.is_empty() {
            self.write(" implements ");
            for (i, iface) in class.implements.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&iface.name);
            }
        }
        if !class.derives.is_empty() {
            if class.implements.is_empty() {
                self.write(" derives ");
            } else {
                self.write(", derives ");
            }
            for (i, derive) in class.derives.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&derive.name);
            }
        }
        self.writeln(" {");
        self.indent();
        for (i, member) in class.members.iter().enumerate() {
            if i > 0 {
                self.blank_line();
            }
            match member {
                ClassMember::Field(f) => self.print_class_field(f),
                ClassMember::Constructor(c) => self.print_class_constructor(c),
                ClassMember::Method(m) => self.print_class_method(m),
                ClassMember::Getter(g) => self.print_class_getter(g),
                ClassMember::Setter(s) => self.print_class_setter(s),
                ClassMember::StaticBlock(block) => self.print_static_block(block),
            }
        }
        self.dedent();
        self.writeln("}");
    }

    /// Print a class field.
    fn print_class_field(&mut self, field: &rustscript_syntax::ast::ClassField) {
        if let Some(ref doc) = field.doc_comment {
            self.print_jsdoc(doc);
        }
        if !field.is_hash_private {
            match field.visibility {
                Visibility::Private => self.write("private "),
                Visibility::Public => {}
            }
        }
        if field.is_static {
            self.write("static ");
        }
        if field.readonly {
            self.write("readonly ");
        }
        if field.is_hash_private {
            self.write("#");
        }
        self.write(&field.name.name);
        self.write(": ");
        self.print_type_annotation(&field.type_ann);
        if let Some(init) = &field.initializer {
            self.write(" = ");
            self.print_expr(init);
        }
        self.writeln(";");
    }

    /// Print a class constructor.
    fn print_class_constructor(&mut self, ctor: &rustscript_syntax::ast::ClassConstructor) {
        if let Some(ref doc) = ctor.doc_comment {
            self.print_jsdoc(doc);
        }
        self.write("constructor(");
        self.print_constructor_params(&ctor.params);
        self.write(") ");
        self.print_block(&ctor.body);
        self.newline();
    }

    /// Print constructor parameter list (may include parameter properties).
    fn print_constructor_params(&mut self, params: &[ConstructorParam]) {
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            if let Some(vis) = param.property_visibility {
                match vis {
                    Visibility::Public => self.write("public "),
                    Visibility::Private => self.write("private "),
                }
            }
            self.write(&param.name.name);
            self.write(": ");
            self.print_type_annotation(&param.type_ann);
            if let Some(ref default) = param.default_value {
                self.write(" = ");
                self.print_expr(default);
            }
        }
    }

    /// Print a class method.
    fn print_class_method(&mut self, method: &rustscript_syntax::ast::ClassMethod) {
        if let Some(ref doc) = method.doc_comment {
            self.print_jsdoc(doc);
        }
        match method.visibility {
            Visibility::Private => self.write("private "),
            Visibility::Public => {}
        }
        if method.is_abstract {
            self.write("abstract ");
        }
        if method.is_override {
            self.write("override ");
        }
        if method.is_static {
            self.write("static ");
        }
        if method.is_async {
            self.write("async ");
        }
        self.write(&method.name.name);
        self.print_optional_type_params(method.type_params.as_ref());
        self.write("(");
        self.print_params(&method.params);
        self.write(")");
        if let Some(ret) = &method.return_type {
            self.print_return_type(ret);
        }
        if method.is_abstract {
            // Abstract methods have no body — end with semicolon
            self.writeln(";");
        } else {
            self.write(" ");
            self.print_block(&method.body);
            self.newline();
        }
    }

    /// Print a getter accessor.
    fn print_class_getter(&mut self, getter: &ClassGetter) {
        match getter.visibility {
            Visibility::Private => self.write("private "),
            Visibility::Public => {}
        }
        self.write("get ");
        self.write(&getter.name.name);
        self.write("()");
        if let Some(ret) = &getter.return_type {
            self.print_return_type(ret);
        }
        self.write(" ");
        self.print_block(&getter.body);
        self.newline();
    }

    /// Print a setter accessor.
    fn print_class_setter(&mut self, setter: &ClassSetter) {
        match setter.visibility {
            Visibility::Private => self.write("private "),
            Visibility::Public => {}
        }
        self.write("set ");
        self.write(&setter.name.name);
        self.write("(");
        self.write(&setter.param.name.name);
        self.write(": ");
        self.print_type_annotation(&setter.param.type_ann);
        self.write(") ");
        self.print_block(&setter.body);
        self.newline();
    }

    /// Print a static initialization block: `static { ... }`.
    fn print_static_block(&mut self, block: &Block) {
        self.write("static ");
        self.print_block(block);
        self.newline();
    }

    /// Print a block with braces.
    fn print_block(&mut self, block: &Block) {
        if block.stmts.is_empty() {
            self.write("{}");
            return;
        }
        self.writeln("{");
        self.indent();
        for stmt in &block.stmts {
            self.print_stmt(stmt);
        }
        self.dedent();
        self.write("}");
    }

    /// Print a `rust { ... }` block, preserving the raw contents.
    fn print_rust_block(&mut self, rb: &InlineRustBlock) {
        self.writeln("rust {");
        self.indent();
        for line in rb.code.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                self.newline();
            } else {
                self.writeln(trimmed);
            }
        }
        self.dedent();
        self.writeln("}");
    }

    /// Print a statement.
    fn print_stmt(&mut self, stmt: &rustscript_syntax::ast::Stmt) {
        use rustscript_syntax::ast::Stmt;
        match stmt {
            Stmt::VarDecl(v) => self.print_var_decl(v),
            Stmt::Expr(e) => {
                self.print_expr(e);
                self.writeln(";");
            }
            Stmt::Return(r) => self.print_return_stmt(r),
            Stmt::If(i) => self.print_if_stmt(i),
            Stmt::While(w) => self.print_while_stmt(w),
            Stmt::DoWhile(dw) => self.print_do_while_stmt(dw),
            Stmt::Destructure(d) => self.print_destructure_stmt(d),
            Stmt::Switch(s) => self.print_switch_stmt(s),
            Stmt::TryCatch(t) => self.print_try_catch_stmt(t),
            Stmt::For(f) => self.print_for_of_stmt(f),
            Stmt::ForIn(f) => self.print_for_in_stmt(f),
            Stmt::ForClassic(f) => self.print_for_classic_stmt(f),
            Stmt::ArrayDestructure(a) => self.print_array_destructure_stmt(a),
            Stmt::Break(brk) => {
                if let Some(lbl) = &brk.label {
                    self.write("break ");
                    self.write(lbl);
                    self.writeln(";");
                } else {
                    self.writeln("break;");
                }
            }
            Stmt::Continue(cont) => {
                if let Some(lbl) = &cont.label {
                    self.write("continue ");
                    self.write(lbl);
                    self.writeln(";");
                } else {
                    self.writeln("continue;");
                }
            }
            Stmt::RustBlock(rb) => self.print_rust_block(rb),
            Stmt::Using(u) => self.print_using_decl(u),
            Stmt::Debugger(_) => self.writeln("debugger;"),
            Stmt::Block(block) => {
                self.writeln("{");
                self.indent();
                for s in &block.stmts {
                    self.print_stmt(s);
                }
                self.dedent();
                self.writeln("}");
            }
        }
    }

    /// Print a `using` or `await using` declaration.
    fn print_using_decl(&mut self, u: &UsingDecl) {
        if u.is_await {
            self.write("await ");
        }
        self.write("using ");
        self.write(&u.name.name);
        if let Some(ty) = &u.type_ann {
            self.write(": ");
            self.print_type_annotation(ty);
        }
        self.write(" = ");
        self.print_expr(&u.init);
        self.writeln(";");
    }

    /// Print a variable declaration.
    fn print_var_decl(&mut self, v: &VarDecl) {
        match v.binding {
            VarBinding::Const => self.write("const "),
            VarBinding::Let => self.write("let "),
            VarBinding::Var => self.write("var "),
        }
        self.write(&v.name.name);
        if let Some(ty) = &v.type_ann {
            self.write(": ");
            self.print_type_annotation(ty);
        }
        self.write(" = ");
        self.print_expr(&v.init);
        self.writeln(";");
    }

    /// Print a return statement.
    fn print_return_stmt(&mut self, r: &ReturnStmt) {
        self.write("return");
        if let Some(value) = &r.value {
            self.write(" ");
            self.print_expr(value);
        }
        self.writeln(";");
    }

    /// Print an if statement.
    fn print_if_stmt(&mut self, i: &IfStmt) {
        self.write("if (");
        self.print_expr(&i.condition);
        self.write(") ");
        self.print_block(&i.then_block);
        match &i.else_clause {
            Some(ElseClause::Block(block)) => {
                self.write(" else ");
                self.print_block(block);
                self.newline();
            }
            Some(ElseClause::ElseIf(else_if)) => {
                self.write(" else ");
                self.print_if_stmt(else_if);
            }
            None => {
                self.newline();
            }
        }
    }

    /// Print a while statement.
    fn print_while_stmt(&mut self, w: &WhileStmt) {
        if let Some(lbl) = &w.label {
            self.write(lbl);
            self.write(": ");
        }
        self.write("while (");
        self.print_expr(&w.condition);
        self.write(") ");
        self.print_block(&w.body);
        self.newline();
    }

    /// Print a do-while statement.
    fn print_do_while_stmt(&mut self, dw: &DoWhileStmt) {
        if let Some(lbl) = &dw.label {
            self.write(lbl);
            self.write(": ");
        }
        self.write("do ");
        self.print_block(&dw.body);
        self.write(" while (");
        self.print_expr(&dw.condition);
        self.writeln(");");
    }

    /// Print a destructure statement.
    ///
    /// Formats rename (`{ name: n }`) and default (`{ name = expr }`) syntax.
    fn print_destructure_stmt(&mut self, d: &DestructureStmt) {
        match d.binding {
            VarBinding::Const => self.write("const "),
            VarBinding::Let => self.write("let "),
            VarBinding::Var => self.write("var "),
        }
        self.write("{ ");
        for (i, field) in d.fields.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.print_destructure_field(field);
        }
        self.write(" } = ");
        self.print_expr(&d.init);
        self.writeln(";");
    }

    /// Print a single destructure field with optional rename and default.
    fn print_destructure_field(&mut self, field: &rustscript_syntax::ast::DestructureField) {
        self.write(&field.field_name.name);
        if let Some(local) = &field.local_name {
            self.write(": ");
            self.write(&local.name);
        }
        if let Some(default_val) = &field.default_value {
            self.write(" = ");
            self.print_expr(default_val);
        }
    }

    /// Print an array destructure statement.
    ///
    /// Formats rest elements (`[first, ...rest]`).
    fn print_array_destructure_stmt(&mut self, a: &ArrayDestructureStmt) {
        use rustscript_syntax::ast::ArrayDestructureElement;
        match a.binding {
            VarBinding::Const => self.write("const "),
            VarBinding::Let => self.write("let "),
            VarBinding::Var => self.write("var "),
        }
        self.write("[");
        for (i, elem) in a.elements.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            match elem {
                ArrayDestructureElement::Single(ident) => self.write(&ident.name),
                ArrayDestructureElement::Rest(ident) => {
                    self.write("...");
                    self.write(&ident.name);
                }
            }
        }
        self.write("] = ");
        self.print_expr(&a.init);
        self.writeln(";");
    }

    /// Print a switch statement.
    fn print_switch_stmt(&mut self, s: &SwitchStmt) {
        self.write("switch (");
        self.print_expr(&s.scrutinee);
        self.writeln(") {");
        self.indent();
        for case in &s.cases {
            self.print_switch_case(case);
        }
        self.dedent();
        self.writeln("}");
    }

    /// Print a switch case.
    fn print_switch_case(&mut self, case: &SwitchCase) {
        match &case.pattern {
            SwitchPattern::StringLit(s) => {
                self.write("case \"");
                self.write(s);
                self.writeln("\":");
            }
            SwitchPattern::IntLit(v) => {
                self.write("case ");
                self.write(&v.to_string());
                self.writeln(":");
            }
            SwitchPattern::EnumMember(enum_name, variant) => {
                if enum_name.is_empty() {
                    self.write("case ");
                    self.write(variant);
                    self.writeln(":");
                } else {
                    self.write("case ");
                    self.write(enum_name);
                    self.write(".");
                    self.write(variant);
                    self.writeln(":");
                }
            }
            SwitchPattern::Default => {
                self.writeln("default:");
            }
        }
        self.indent();
        for stmt in &case.body {
            self.print_stmt(stmt);
        }
        self.dedent();
    }

    /// Print a try/catch/finally statement.
    fn print_try_catch_stmt(&mut self, t: &TryCatchStmt) {
        self.write("try ");
        self.print_block(&t.try_block);
        if let (Some(binding), Some(block)) = (&t.catch_binding, &t.catch_block) {
            self.write(" catch (");
            self.write(&binding.name);
            if let Some(ty) = &t.catch_type {
                self.write(": ");
                self.print_type_annotation(ty);
            }
            self.write(") ");
            self.print_block(block);
        }
        if let Some(finally_block) = &t.finally_block {
            self.write(" finally ");
            self.print_block(finally_block);
        }
        self.newline();
    }

    /// Print a for-of statement.
    fn print_for_of_stmt(&mut self, f: &ForOfStmt) {
        if let Some(lbl) = &f.label {
            self.write(lbl);
            self.write(": ");
        }
        if f.is_await {
            self.write("for await (");
        } else {
            self.write("for (");
        }
        match f.binding {
            VarBinding::Const => self.write("const "),
            VarBinding::Let => self.write("let "),
            VarBinding::Var => self.write("var "),
        }
        self.write(&f.variable.name);
        self.write(" of ");
        self.print_expr(&f.iterable);
        self.write(") ");
        self.print_block(&f.body);
        self.newline();
    }

    /// Print a for-in statement.
    fn print_for_in_stmt(&mut self, f: &ForInStmt) {
        if let Some(lbl) = &f.label {
            self.write(lbl);
            self.write(": ");
        }
        self.write("for (");
        match f.binding {
            VarBinding::Const => self.write("const "),
            VarBinding::Let => self.write("let "),
            VarBinding::Var => self.write("var "),
        }
        self.write(&f.variable.name);
        self.write(" in ");
        self.print_expr(&f.iterable);
        self.write(") ");
        self.print_block(&f.body);
        self.newline();
    }

    /// Print a classic C-style for statement.
    fn print_for_classic_stmt(&mut self, f: &ForClassicStmt) {
        if let Some(lbl) = &f.label {
            self.write(lbl);
            self.write(": ");
        }
        self.write("for (");
        if let Some(init) = &f.init {
            match init {
                ForInit::VarDecl(v) => {
                    match v.binding {
                        VarBinding::Const => self.write("const "),
                        VarBinding::Let => self.write("let "),
                        VarBinding::Var => self.write("var "),
                    }
                    self.write(&v.name.name);
                    if let Some(ty) = &v.type_ann {
                        self.write(": ");
                        self.print_type_annotation(ty);
                    }
                    self.write(" = ");
                    self.print_expr(&v.init);
                }
                ForInit::Expr(e) => self.print_expr(e),
            }
        }
        self.write("; ");
        if let Some(cond) = &f.condition {
            self.print_expr(cond);
        }
        self.write("; ");
        if let Some(update) = &f.update {
            self.print_expr(update);
        }
        self.write(") ");
        self.print_block(&f.body);
        self.newline();
    }

    /// Print an expression.
    #[allow(clippy::too_many_lines)]
    // Expression printing covers all ExprKind variants; splitting would fragment the match
    fn print_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::IntLit(v) => self.write(&v.to_string()),
            ExprKind::FloatLit(v) => self.write(&format_float(*v)),
            ExprKind::StringLit(s) => {
                self.write("\"");
                self.write(s);
                self.write("\"");
            }
            ExprKind::BoolLit(b) => self.write(if *b { "true" } else { "false" }),
            ExprKind::NullLit => self.write("null"),
            ExprKind::This => self.write("this"),
            ExprKind::Super => self.write("super"),
            ExprKind::Ident(ident) => self.write(&ident.name),
            ExprKind::Binary(b) => self.print_binary_expr(b),
            ExprKind::Unary(u) => self.print_unary_expr(u),
            ExprKind::Call(c) => self.print_call_expr(c),
            ExprKind::MethodCall(m) => self.print_method_call_expr(m),
            ExprKind::Paren(inner) => {
                self.write("(");
                self.print_expr(inner);
                self.write(")");
            }
            ExprKind::Assign(a) => self.print_assign_expr(a),
            ExprKind::FieldAssign(fa) => self.print_field_assign_expr(fa),
            ExprKind::LogicalAssign(la) => self.print_logical_assign_expr(la),
            ExprKind::StructLit(s) => self.print_struct_lit_expr(s),
            ExprKind::FieldAccess(fa) => self.print_field_access_expr(fa),
            ExprKind::TemplateLit(t) => self.print_template_lit_expr(t),
            ExprKind::TaggedTemplate {
                tag,
                quasis,
                expressions,
            } => self.print_tagged_template(tag, quasis, expressions),
            ExprKind::ArrayLit(items) => self.print_array_lit(items),
            ExprKind::New(n) => self.print_new_expr(n),
            ExprKind::Index(idx) => self.print_index_expr(idx),
            ExprKind::OptionalChain(oc) => self.print_optional_chain_expr(oc),
            ExprKind::NullishCoalescing(nc) => self.print_nullish_coalescing_expr(nc),
            ExprKind::Throw(inner) => {
                self.write("throw ");
                self.print_expr(inner);
            }
            ExprKind::Closure(c) => self.print_closure_expr(c),
            ExprKind::Await(inner) => {
                self.write("await ");
                self.print_expr(inner);
            }
            ExprKind::Shared(inner) => {
                self.write("shared(");
                self.print_expr(inner);
                self.write(")");
            }
            ExprKind::SpreadArg(inner) => {
                self.write("...");
                self.print_expr(inner);
            }
            ExprKind::Ternary(cond, then_expr, else_expr) => {
                self.print_expr(cond);
                self.write(" ? ");
                self.print_expr(then_expr);
                self.write(" : ");
                self.print_expr(else_expr);
            }
            ExprKind::NonNullAssert(inner) => {
                self.print_expr(inner);
                self.write("!");
            }
            ExprKind::Cast(inner, ty) => {
                self.print_expr(inner);
                self.write(" as ");
                self.print_type_annotation(ty);
            }
            ExprKind::TypeOf(inner) => {
                self.write("typeof ");
                self.print_expr(inner);
            }
            ExprKind::Satisfies(inner, ty) => {
                self.print_expr(inner);
                self.write(" satisfies ");
                self.print_type_annotation(ty);
            }
            ExprKind::IndexAssign(ia) => self.print_index_assign_expr(ia),
            ExprKind::Yield(inner) => {
                self.write("yield ");
                self.print_expr(inner);
            }
            ExprKind::Delete(inner) => {
                self.write("delete ");
                self.print_expr(inner);
            }
            ExprKind::Void(inner) => {
                self.write("void ");
                self.print_expr(inner);
            }
            ExprKind::Comma(exprs) => {
                self.write("(");
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(e);
                }
                self.write(")");
            }
            ExprKind::AsConst(inner) => {
                self.print_expr(inner);
                self.write(" as const");
            }
            ExprKind::DynamicImport(module) => {
                self.write("import(\"");
                self.write(module);
                self.write("\")");
            }
            ExprKind::PostfixIncrement(operand) => {
                self.print_expr(operand);
                self.write("++");
            }
            ExprKind::PostfixDecrement(operand) => {
                self.print_expr(operand);
                self.write("--");
            }
            ExprKind::PrefixIncrement(operand) => {
                self.write("++");
                self.print_expr(operand);
            }
            ExprKind::PrefixDecrement(operand) => {
                self.write("--");
                self.print_expr(operand);
            }
            ExprKind::ClassExpr(class_def) => {
                self.write("class ");
                if class_def.name.name != "__AnonymousClass" {
                    self.write(&class_def.name.name);
                    self.write(" ");
                }
                self.print_optional_type_params(class_def.type_params.as_ref());
                if let Some(base) = &class_def.extends {
                    self.write("extends ");
                    self.write(&base.name);
                    self.write(" ");
                }
                self.writeln("{");
                self.indent();
                for (i, member) in class_def.members.iter().enumerate() {
                    if i > 0 {
                        self.blank_line();
                    }
                    match member {
                        ClassMember::Field(f) => self.print_class_field(f),
                        ClassMember::Constructor(c) => self.print_class_constructor(c),
                        ClassMember::Method(m) => self.print_class_method(m),
                        ClassMember::Getter(g) => self.print_class_getter(g),
                        ClassMember::Setter(s) => self.print_class_setter(s),
                        ClassMember::StaticBlock(block) => self.print_static_block(block),
                    }
                }
                self.dedent();
                self.write("}");
            }
            ExprKind::RegexLit { pattern, flags } => {
                self.write("/");
                self.write(pattern);
                self.write("/");
                self.write(flags);
            }
            ExprKind::NewTarget => {
                self.write("new.target");
            }
            ExprKind::ImportMeta => {
                self.write("import.meta");
            }
        }
    }

    /// Print a binary expression with spaces around the operator.
    fn print_binary_expr(&mut self, b: &BinaryExpr) {
        self.print_expr(&b.left);
        self.write(" ");
        self.write(&b.op.to_string());
        self.write(" ");
        self.print_expr(&b.right);
    }

    /// Print a unary expression.
    fn print_unary_expr(&mut self, u: &UnaryExpr) {
        match u.op {
            UnaryOp::Neg => self.write("-"),
            UnaryOp::Not => self.write("!"),
            UnaryOp::BitNot => self.write("~"),
        }
        self.print_expr(&u.operand);
    }

    /// Print a function call expression.
    fn print_call_expr(&mut self, c: &CallExpr) {
        self.write(&c.callee.name);
        self.write("(");
        for (i, arg) in c.args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.print_expr(arg);
        }
        self.write(")");
    }

    /// Print a method call expression.
    fn print_method_call_expr(&mut self, m: &MethodCallExpr) {
        self.print_expr(&m.object);
        self.write(".");
        self.write(&m.method.name);
        self.write("(");
        for (i, arg) in m.args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.print_expr(arg);
        }
        self.write(")");
    }

    /// Print an assignment expression.
    fn print_assign_expr(&mut self, a: &AssignExpr) {
        self.write(&a.target.name);
        self.write(" = ");
        self.print_expr(&a.value);
    }

    /// Print a logical assignment expression: `x ??= val`, `x ||= val`, `x &&= val`.
    fn print_logical_assign_expr(&mut self, la: &LogicalAssignExpr) {
        self.write(&la.target.name);
        self.write(&format!(" {} ", la.op));
        self.print_expr(&la.value);
    }

    /// Print a field assignment expression.
    fn print_field_assign_expr(&mut self, fa: &FieldAssignExpr) {
        self.print_expr(&fa.object);
        self.write(".");
        self.write(&fa.field.name);
        self.write(" = ");
        self.print_expr(&fa.value);
    }

    /// Print a struct literal expression.
    fn print_struct_lit_expr(&mut self, s: &StructLitExpr) {
        if let Some(type_name) = &s.type_name {
            self.write(&type_name.name);
            self.write(" ");
        }
        self.write("{ ");
        if let Some(spread) = &s.spread {
            self.write("...");
            self.print_expr(spread);
            if !s.fields.is_empty() {
                self.write(", ");
            }
        }
        for (i, field) in s.fields.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.print_field_init(field);
        }
        self.write(" }");
    }

    /// Print a field initializer.
    fn print_field_init(&mut self, field: &FieldInit) {
        if let Some(key_expr) = &field.computed_key {
            self.write("[");
            self.print_expr(key_expr);
            self.write("]");
        } else {
            self.write(&field.name.name);
        }
        self.write(": ");
        self.print_expr(&field.value);
    }

    /// Print a field access expression.
    fn print_field_access_expr(&mut self, fa: &FieldAccessExpr) {
        self.print_expr(&fa.object);
        self.write(".");
        self.write(&fa.field.name);
    }

    /// Print a template literal expression.
    fn print_template_lit_expr(&mut self, t: &TemplateLitExpr) {
        self.write("`");
        for part in &t.parts {
            match part {
                TemplatePart::String(s, _) => self.write(s),
                TemplatePart::Expr(e) => {
                    self.write("${");
                    self.print_expr(e);
                    self.write("}");
                }
            }
        }
        self.write("`");
    }

    /// Print a tagged template literal expression.
    fn print_tagged_template(&mut self, tag: &Expr, quasis: &[String], expressions: &[Expr]) {
        self.print_expr(tag);
        self.write("`");
        for (i, quasi) in quasis.iter().enumerate() {
            self.write(quasi);
            if i < expressions.len() {
                self.write("${");
                self.print_expr(&expressions[i]);
                self.write("}");
            }
        }
        self.write("`");
    }

    /// Print an array literal.
    fn print_array_lit(&mut self, items: &[ArrayElement]) {
        self.write("[");
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            match item {
                ArrayElement::Expr(expr) => self.print_expr(expr),
                ArrayElement::Spread(expr) => {
                    self.write("...");
                    self.print_expr(expr);
                }
            }
        }
        self.write("]");
    }

    /// Print a new expression.
    fn print_new_expr(&mut self, n: &NewExpr) {
        self.write("new ");
        self.write(&n.type_name.name);
        if !n.type_args.is_empty() {
            self.write("<");
            for (i, arg) in n.type_args.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.print_type_annotation(arg);
            }
            self.write(">");
        }
        self.write("(");
        for (i, arg) in n.args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.print_expr(arg);
        }
        self.write(")");
    }

    /// Print an index expression.
    fn print_index_expr(&mut self, idx: &IndexExpr) {
        self.print_expr(&idx.object);
        self.write("[");
        self.print_expr(&idx.index);
        self.write("]");
    }

    /// Print an index assignment expression: `obj["key"] = value`.
    fn print_index_assign_expr(&mut self, ia: &IndexAssignExpr) {
        self.print_expr(&ia.object);
        self.write("[");
        self.print_expr(&ia.index);
        self.write("] = ");
        self.print_expr(&ia.value);
    }

    /// Print an optional chain expression.
    fn print_optional_chain_expr(&mut self, oc: &OptionalChainExpr) {
        self.print_expr(&oc.object);
        match &oc.access {
            OptionalAccess::Field(field) => {
                self.write("?.");
                self.write(&field.name);
            }
            OptionalAccess::Method(method, args) => {
                self.write("?.");
                self.write(&method.name);
                self.write("(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(arg);
                }
                self.write(")");
            }
        }
    }

    /// Print a nullish coalescing expression.
    fn print_nullish_coalescing_expr(&mut self, nc: &NullishCoalescingExpr) {
        self.print_expr(&nc.left);
        self.write(" ?? ");
        self.print_expr(&nc.right);
    }

    /// Print a closure expression.
    fn print_closure_expr(&mut self, c: &ClosureExpr) {
        if c.is_async {
            self.write("async ");
        }
        if c.is_move {
            self.write("move ");
        }
        self.write("(");
        for (i, param) in c.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&param.name.name);
            if !matches!(param.type_ann.kind, TypeKind::Inferred) {
                self.write(": ");
                self.print_type_annotation(&param.type_ann);
            }
        }
        self.write(")");
        if let Some(ret) = &c.return_type {
            self.write(": ");
            self.print_type_annotation(ret);
        }
        self.write(" => ");
        match &c.body {
            ClosureBody::Expr(expr) => self.print_expr(expr),
            ClosureBody::Block(block) => self.print_block(block),
        }
    }
}

/// Format a float to preserve its literal representation.
fn format_float(v: f64) -> String {
    let s = v.to_string();
    // Ensure there's always a decimal point
    if s.contains('.') { s } else { format!("{s}.0") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustscript_syntax::ast::*;
    use rustscript_syntax::span::Span;

    fn ident(name: &str) -> Ident {
        Ident {
            name: name.to_owned(),
            span: Span::dummy(),
        }
    }

    fn named_type(name: &str) -> TypeAnnotation {
        TypeAnnotation {
            kind: TypeKind::Named(ident(name)),
            span: Span::dummy(),
        }
    }

    fn int_expr(value: i64) -> Expr {
        Expr {
            kind: ExprKind::IntLit(value),
            span: Span::dummy(),
        }
    }

    fn ident_expr(name: &str) -> Expr {
        Expr {
            kind: ExprKind::Ident(ident(name)),
            span: Span::dummy(),
        }
    }

    #[test]
    fn test_printer_empty_function_canonical_form() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Function(FnDecl {
                    is_async: false,
                    is_generator: false,
                    name: ident("foo"),
                    type_params: None,
                    params: vec![],
                    return_type: None,
                    body: Block {
                        stmts: vec![],
                        span: Span::dummy(),
                    },
                    doc_comment: None,
                    span: Span::dummy(),
                }),
                exported: false,
                decorators: vec![],
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        assert_eq!(output, "function foo() {}\n");
    }

    #[test]
    fn test_printer_indentation_nested_blocks() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Function(FnDecl {
                    is_async: false,
                    is_generator: false,
                    name: ident("foo"),
                    type_params: None,
                    params: vec![],
                    return_type: None,
                    body: Block {
                        stmts: vec![Stmt::If(IfStmt {
                            condition: Expr {
                                kind: ExprKind::BoolLit(true),
                                span: Span::dummy(),
                            },
                            then_block: Block {
                                stmts: vec![Stmt::Return(ReturnStmt {
                                    value: Some(int_expr(1)),
                                    span: Span::dummy(),
                                })],
                                span: Span::dummy(),
                            },
                            else_clause: None,
                            span: Span::dummy(),
                        })],
                        span: Span::dummy(),
                    },
                    doc_comment: None,
                    span: Span::dummy(),
                }),
                exported: false,
                decorators: vec![],
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        assert_eq!(
            output,
            "function foo() {\n  if (true) {\n    return 1;\n  }\n}\n"
        );
    }

    #[test]
    fn test_printer_operator_spacing() {
        let expr = Expr {
            kind: ExprKind::Binary(BinaryExpr {
                op: BinaryOp::Add,
                left: Box::new(ident_expr("a")),
                right: Box::new(ident_expr("b")),
            }),
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_expr(&expr);
        let output = printer.into_output();
        assert_eq!(output, "a + b");
    }

    #[test]
    fn test_printer_comma_spacing_params() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Function(FnDecl {
                    is_async: false,
                    is_generator: false,
                    name: ident("foo"),
                    type_params: None,
                    params: vec![
                        Param {
                            name: ident("a"),
                            type_ann: named_type("i32"),
                            optional: false,
                            default_value: None,
                            is_rest: false,
                            destructure_fields: None,
                            span: Span::dummy(),
                        },
                        Param {
                            name: ident("b"),
                            type_ann: named_type("i32"),
                            optional: false,
                            default_value: None,
                            is_rest: false,
                            destructure_fields: None,
                            span: Span::dummy(),
                        },
                        Param {
                            name: ident("c"),
                            type_ann: named_type("i32"),
                            optional: false,
                            default_value: None,
                            is_rest: false,
                            destructure_fields: None,
                            span: Span::dummy(),
                        },
                    ],
                    return_type: None,
                    body: Block {
                        stmts: vec![],
                        span: Span::dummy(),
                    },
                    doc_comment: None,
                    span: Span::dummy(),
                }),
                exported: false,
                decorators: vec![],
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        assert_eq!(output, "function foo(a: i32, b: i32, c: i32) {}\n");
    }

    #[test]
    fn test_printer_blank_lines_between_items() {
        let module = Module {
            items: vec![
                Item {
                    kind: ItemKind::Function(FnDecl {
                        is_async: false,
                        is_generator: false,
                        name: ident("foo"),
                        type_params: None,
                        params: vec![],
                        return_type: None,
                        body: Block {
                            stmts: vec![],
                            span: Span::dummy(),
                        },
                        doc_comment: None,
                        span: Span::dummy(),
                    }),
                    exported: false,
                    decorators: vec![],
                    span: Span::dummy(),
                },
                Item {
                    kind: ItemKind::Function(FnDecl {
                        is_async: false,
                        is_generator: false,
                        name: ident("bar"),
                        type_params: None,
                        params: vec![],
                        return_type: None,
                        body: Block {
                            stmts: vec![],
                            span: Span::dummy(),
                        },
                        doc_comment: None,
                        span: Span::dummy(),
                    }),
                    exported: false,
                    decorators: vec![],
                    span: Span::dummy(),
                },
            ],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        assert_eq!(output, "function foo() {}\n\nfunction bar() {}\n");
    }

    #[test]
    fn test_printer_trailing_newline() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Function(FnDecl {
                    is_async: false,
                    is_generator: false,
                    name: ident("x"),
                    type_params: None,
                    params: vec![],
                    return_type: None,
                    body: Block {
                        stmts: vec![],
                        span: Span::dummy(),
                    },
                    doc_comment: None,
                    span: Span::dummy(),
                }),
                exported: false,
                decorators: vec![],
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        assert!(output.ends_with('\n'), "output should end with newline");
        assert!(
            !output.ends_with("\n\n"),
            "output should not end with double newline"
        );
    }

    #[test]
    fn test_printer_import_sorting() {
        let module = Module {
            items: vec![
                Item {
                    kind: ItemKind::Import(ImportDecl {
                        names: vec![ident("X")],
                        source: StringLiteral {
                            value: "./mod".to_owned(),
                            span: Span::dummy(),
                        },
                        is_type_only: false,
                        span: Span::dummy(),
                    }),
                    exported: false,
                    decorators: vec![],
                    span: Span::dummy(),
                },
                Item {
                    kind: ItemKind::Import(ImportDecl {
                        names: vec![ident("A")],
                        source: StringLiteral {
                            value: "./alpha".to_owned(),
                            span: Span::dummy(),
                        },
                        is_type_only: false,
                        span: Span::dummy(),
                    }),
                    exported: false,
                    decorators: vec![],
                    span: Span::dummy(),
                },
            ],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        // ./alpha should come before ./mod
        assert_eq!(
            output,
            "import { A } from \"./alpha\";\nimport { X } from \"./mod\";\n"
        );
    }

    #[test]
    fn test_printer_type_annotation_colon_spacing() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Function(FnDecl {
                    is_async: false,
                    is_generator: false,
                    name: ident("foo"),
                    type_params: None,
                    params: vec![Param {
                        name: ident("x"),
                        type_ann: named_type("i32"),
                        optional: false,
                        default_value: None,
                        is_rest: false,
                        destructure_fields: None,
                        span: Span::dummy(),
                    }],
                    return_type: None,
                    body: Block {
                        stmts: vec![],
                        span: Span::dummy(),
                    },
                    doc_comment: None,
                    span: Span::dummy(),
                }),
                exported: false,
                decorators: vec![],
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        assert!(
            output.contains("x: i32"),
            "should have space after colon: {output}"
        );
    }

    #[test]
    fn test_printer_var_decl_formatting() {
        let stmt = Stmt::VarDecl(VarDecl {
            binding: VarBinding::Const,
            name: ident("x"),
            type_ann: Some(named_type("i32")),
            init: int_expr(42),
            span: Span::dummy(),
        });

        let mut printer = Printer::new();
        printer.print_stmt(&stmt);
        let output = printer.into_output();
        assert_eq!(output, "const x: i32 = 42;\n");
    }

    #[test]
    fn test_printer_closure_expression() {
        let expr = Expr {
            kind: ExprKind::Closure(ClosureExpr {
                is_async: false,
                is_move: false,
                params: vec![Param {
                    name: ident("x"),
                    type_ann: named_type("i32"),
                    optional: false,
                    default_value: None,
                    is_rest: false,
                    destructure_fields: None,
                    span: Span::dummy(),
                }],
                return_type: Some(named_type("i32")),
                body: ClosureBody::Expr(Box::new(Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Mul,
                        left: Box::new(ident_expr("x")),
                        right: Box::new(int_expr(2)),
                    }),
                    span: Span::dummy(),
                })),
            }),
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_expr(&expr);
        let output = printer.into_output();
        assert_eq!(output, "(x: i32): i32 => x * 2");
    }

    #[test]
    fn test_printer_async_function() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Function(FnDecl {
                    is_async: true,
                    is_generator: false,
                    name: ident("fetch_data"),
                    type_params: None,
                    params: vec![],
                    return_type: Some(ReturnTypeAnnotation {
                        type_ann: Some(named_type("string")),
                        throws: None,
                        span: Span::dummy(),
                    }),
                    body: Block {
                        stmts: vec![],
                        span: Span::dummy(),
                    },
                    doc_comment: None,
                    span: Span::dummy(),
                }),
                exported: false,
                decorators: vec![],
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        assert_eq!(output, "async function fetch_data(): string {}\n");
    }

    #[test]
    fn test_printer_export_keyword() {
        let module = Module {
            items: vec![Item {
                kind: ItemKind::Function(FnDecl {
                    is_async: false,
                    is_generator: false,
                    name: ident("foo"),
                    type_params: None,
                    params: vec![],
                    return_type: None,
                    body: Block {
                        stmts: vec![],
                        span: Span::dummy(),
                    },
                    doc_comment: None,
                    span: Span::dummy(),
                }),
                exported: true,
                decorators: vec![],
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_module(&module);
        let output = printer.into_output();
        assert_eq!(output, "export function foo() {}\n");
    }

    #[test]
    fn test_printer_template_literal() {
        let expr = Expr {
            kind: ExprKind::TemplateLit(TemplateLitExpr {
                parts: vec![
                    TemplatePart::String("Hello, ".to_owned(), Span::dummy()),
                    TemplatePart::Expr(ident_expr("name")),
                    TemplatePart::String("!".to_owned(), Span::dummy()),
                ],
            }),
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_expr(&expr);
        let output = printer.into_output();
        assert_eq!(output, "`Hello, ${name}!`");
    }

    // ---------------------------------------------------------------
    // Task 066: for await formatting
    // ---------------------------------------------------------------

    #[test]
    fn test_printer_for_await_preserves_await_keyword() {
        let for_stmt = ForOfStmt {
            label: None,
            binding: VarBinding::Const,
            variable: ident("msg"),
            iterable: ident_expr("channel"),
            body: Block {
                stmts: vec![],
                span: Span::dummy(),
            },
            is_await: true,
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_for_of_stmt(&for_stmt);
        let output = printer.into_output();
        assert!(
            output.starts_with("for await ("),
            "should start with 'for await (': {output}"
        );
        assert!(
            output.contains("const msg"),
            "should contain binding: {output}"
        );
    }

    #[test]
    fn test_printer_regular_for_no_await() {
        let for_stmt = ForOfStmt {
            label: None,
            binding: VarBinding::Const,
            variable: ident("x"),
            iterable: ident_expr("items"),
            body: Block {
                stmts: vec![],
                span: Span::dummy(),
            },
            is_await: false,
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_for_of_stmt(&for_stmt);
        let output = printer.into_output();
        assert!(
            output.starts_with("for ("),
            "regular for should start with 'for (': {output}"
        );
        assert!(
            !output.contains("await"),
            "regular for should not contain await: {output}"
        );
    }

    // ---------------------------------------------------------------
    // Task 110: for-in formatting
    // ---------------------------------------------------------------

    #[test]
    fn test_printer_for_in_formats_correctly() {
        let for_in_stmt = ForInStmt {
            label: None,
            binding: VarBinding::Const,
            variable: ident("k"),
            iterable: ident_expr("obj"),
            body: Block {
                stmts: vec![],
                span: Span::dummy(),
            },
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_for_in_stmt(&for_in_stmt);
        let output = printer.into_output();
        assert!(
            output.starts_with("for (const k in obj)"),
            "for-in should format as 'for (const k in obj)': {output}"
        );
    }

    #[test]
    fn test_printer_for_in_let_binding() {
        let for_in_stmt = ForInStmt {
            label: None,
            binding: VarBinding::Let,
            variable: ident("key"),
            iterable: ident_expr("map"),
            body: Block {
                stmts: vec![],
                span: Span::dummy(),
            },
            span: Span::dummy(),
        };

        let mut printer = Printer::new();
        printer.print_for_in_stmt(&for_in_stmt);
        let output = printer.into_output();
        assert!(
            output.starts_with("for (let key in map)"),
            "for-in with let should format as 'for (let key in map)': {output}"
        );
    }
}
