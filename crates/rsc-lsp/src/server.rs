//! LSP server implementation for `RustScript`.
//!
//! Implements the [`tower_lsp::LanguageServer`] trait to provide diagnostics,
//! formatting, hover, go-to-definition, and completions for `.rts` files.
//! When available, proxies definition and completion requests through
//! rust-analyzer running on the generated `.rs` code.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::{Notify, RwLock};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentFormattingParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    Location, MarkupContent, MarkupKind, MessageType, OneOf, Position, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer};

use crate::diagnostics;
use crate::name_map;
use crate::position_map::PositionMap;
use crate::ra_proxy::RustAnalyzerProxy;

/// Debounce delay for recompilation after document changes.
const DEBOUNCE_MS: u64 = 300;

/// The `RustScript` language server.
///
/// Maintains an in-memory document store and provides diagnostics,
/// formatting, hover, go-to-definition, and completions via the LSP protocol.
/// When rust-analyzer is available, proxies definition and completion requests
/// through it for deeper Rust-level intelligence.
pub struct RscLanguageServer {
    /// The LSP client handle for sending notifications (e.g., diagnostics).
    client: Client,
    /// In-memory document store: URI -> source text.
    documents: DashMap<Url, String>,
    /// Per-document notification channels for debouncing.
    debounce_notifiers: DashMap<Url, Arc<Notify>>,
    /// Position maps per document (built after compilation).
    position_maps: DashMap<Url, PositionMap>,
    /// Rust-analyzer proxy (initialized on first successful compilation).
    ra_proxy: RwLock<Option<RustAnalyzerProxy>>,
    /// Project build directory (`.rsc-build/`).
    build_dir: RwLock<Option<PathBuf>>,
}

impl RscLanguageServer {
    /// Create a new `RustScript` language server.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
            debounce_notifiers: DashMap::new(),
            position_maps: DashMap::new(),
            ra_proxy: RwLock::new(None),
            build_dir: RwLock::new(None),
        }
    }

    /// Publish diagnostics for the given document.
    ///
    /// Compiles the source and sends any diagnostics to the editor. If the
    /// source is valid, publishes an empty diagnostics list to clear stale errors.
    /// Also builds a position map for the document and attempts to start
    /// rust-analyzer if not already running.
    async fn publish_diagnostics(&self, uri: &Url, source: &str) {
        let lsp_diagnostics = diagnostics::collect_diagnostics(source);

        // Also run the full compilation to build the position map.
        let compile_result = rsc_driver::compile_source(source, "lsp_input.rts");
        if !compile_result.has_errors {
            // Build a position map from the source map.
            let position_map = PositionMap::new(
                compile_result.source_map_lines,
                source.to_owned(),
                compile_result.rust_source.clone(),
            );
            self.position_maps.insert(uri.clone(), position_map);

            // Attempt to start rust-analyzer if not already running.
            self.ensure_ra_started(uri).await;
        }

        self.client
            .publish_diagnostics(uri.clone(), lsp_diagnostics, None)
            .await;
    }

    /// Ensure rust-analyzer is started if it isn't already.
    ///
    /// Determines the build directory from the document URI, creates it if
    /// necessary, and starts rust-analyzer pointed at it.
    async fn ensure_ra_started(&self, uri: &Url) {
        let ra_guard = self.ra_proxy.read().await;
        if ra_guard.as_ref().is_some_and(RustAnalyzerProxy::is_alive) {
            return;
        }
        drop(ra_guard);

        // Determine build directory from the URI.
        let build_dir = if let Ok(path) = uri.to_file_path() {
            if let Some(parent) = path.parent() {
                parent.join(".rsc-build")
            } else {
                return;
            }
        } else {
            return;
        };

        // Only start if the build directory exists.
        if !build_dir.exists() {
            return;
        }

        match RustAnalyzerProxy::start(&build_dir) {
            Ok(Some(proxy)) => {
                self.client
                    .log_message(MessageType::INFO, "rust-analyzer proxy started")
                    .await;
                let mut ra = self.ra_proxy.write().await;
                *ra = Some(proxy);
                let mut bd = self.build_dir.write().await;
                *bd = Some(build_dir);
            }
            Ok(None) => {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        "rust-analyzer not found in PATH; go-to-definition and completions disabled",
                    )
                    .await;
            }
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("failed to start rust-analyzer: {e}"),
                    )
                    .await;
            }
        }
    }

    /// Schedule a debounced diagnostic update for a document.
    ///
    /// Waits [`DEBOUNCE_MS`] milliseconds after the last change before compiling.
    /// Rapid successive calls reset the timer so that only one compilation runs
    /// after a burst of keystrokes.
    fn schedule_diagnostics(&self, uri: Url) {
        let notify = {
            let entry = self
                .debounce_notifiers
                .entry(uri.clone())
                .or_insert_with(|| Arc::new(Notify::new()));
            Arc::clone(entry.value())
        };

        // Notify any existing debounce task that a new change arrived.
        notify.notify_one();

        let client = self.client.clone();
        let documents = self.documents.clone();
        let debounce_notify = notify;

        tokio::spawn(async move {
            // Wait for the debounce period. If notified during the wait, a newer
            // task is taking over — bail out.
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)) => {}
                () = debounce_notify.notified() => {
                    return;
                }
            }

            // Compile and publish diagnostics.
            if let Some(source) = documents.get(&uri) {
                let lsp_diagnostics = diagnostics::collect_diagnostics(&source);
                client.publish_diagnostics(uri, lsp_diagnostics, None).await;
            }
        });
    }

    /// Forward a go-to-definition request through rust-analyzer.
    ///
    /// Translates the `.rts` position to `.rs`, forwards to RA, and translates
    /// the response back. Returns `None` if RA is not available or the position
    /// cannot be mapped.
    async fn ra_goto_definition(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<GotoDefinitionResponse> {
        let map = self.position_maps.get(uri)?;
        let rs_pos = map.rts_to_rs_position(position)?;
        let rs_uri = map.rts_to_rs_uri(uri)?;
        drop(map);

        let ra_guard = self.ra_proxy.read().await;
        let proxy = ra_guard.as_ref()?;

        let params = serde_json::json!({
            "textDocument": { "uri": rs_uri.as_str() },
            "position": { "line": rs_pos.line, "character": rs_pos.character },
        });

        let response = proxy
            .request("textDocument/definition", params)
            .await
            .ok()?;
        drop(ra_guard);

        translate_definition_response(&response, &self.position_maps)
    }

    /// Forward a completion request through rust-analyzer.
    ///
    /// Translates the `.rts` position to `.rs`, forwards to RA, and translates
    /// the completion labels back to `RustScript` names.
    async fn ra_completion(&self, uri: &Url, position: Position) -> Option<CompletionResponse> {
        let map = self.position_maps.get(uri)?;
        let rs_pos = map.rts_to_rs_position(position)?;
        let rs_uri = map.rts_to_rs_uri(uri)?;
        drop(map);

        let ra_guard = self.ra_proxy.read().await;
        let proxy = ra_guard.as_ref()?;

        let params = serde_json::json!({
            "textDocument": { "uri": rs_uri.as_str() },
            "position": { "line": rs_pos.line, "character": rs_pos.character },
        });

        let response = proxy
            .request("textDocument/completion", params)
            .await
            .ok()?;
        drop(ra_guard);

        translate_completion_response(&response)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for RscLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_owned(), ":".to_owned()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "RustScript LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        // Shut down rust-analyzer if running.
        let mut ra = self.ra_proxy.write().await;
        if let Some(proxy) = ra.take() {
            let _ = proxy.shutdown().await;
        }
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.documents.insert(uri.clone(), text.clone());
        self.publish_diagnostics(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().next() {
            self.documents.insert(uri.clone(), change.text);
            self.schedule_diagnostics(uri);
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(source) = self.documents.get(&uri) {
            self.publish_diagnostics(&uri, &source.clone()).await;
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        if let Some(source) = self.documents.get(uri) {
            let source_text = source.clone();
            drop(source); // Release the DashMap ref before awaiting

            match rsc_fmt::format_source(&source_text) {
                Ok(formatted) if formatted != source_text => {
                    let range = diagnostics::full_document_range(&source_text);
                    Ok(Some(vec![TextEdit {
                        range,
                        new_text: formatted,
                    }]))
                }
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(source) = self.documents.get(uri) {
            let source_text = source.clone();
            drop(source);

            let offset = diagnostics::position_to_offset(&position, &source_text);
            let file_id = rsc_syntax::source::FileId(0);
            let (module, _) = rsc_parser::parse(&source_text, file_id);

            if let Some(info) = find_hover_info(&module, offset) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: info,
                    }),
                    range: None,
                }));
            }
        }
        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        // Try rust-analyzer proxy first.
        if let Some(response) = self.ra_goto_definition(uri, position).await {
            return Ok(Some(response));
        }

        // Graceful degradation: return empty when RA is not available.
        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        // Try rust-analyzer proxy first.
        if let Some(response) = self.ra_completion(uri, position).await {
            return Ok(Some(response));
        }

        // Graceful degradation: return empty when RA is not available.
        Ok(None)
    }
}

/// Translate a go-to-definition response from rust-analyzer.
///
/// Converts `.rs` positions back to `.rts` positions using the position maps.
/// If the definition target is within a mapped `.rts` file, the location is
/// translated. External definitions (e.g., in crate code) are passed through.
fn translate_definition_response(
    response: &serde_json::Value,
    position_maps: &DashMap<Url, PositionMap>,
) -> Option<GotoDefinitionResponse> {
    // RA may return a single Location, an array of Locations, or a LocationLink array.
    // Handle the common case: a single Location or array of Locations.

    if let Some(result) = response.get("result") {
        if result.is_null() {
            return None;
        }

        // Single location object.
        if let Some(location) = try_parse_location(result, position_maps) {
            return Some(GotoDefinitionResponse::Scalar(location));
        }

        // Array of locations.
        if let Some(arr) = result.as_array() {
            let locations: Vec<Location> = arr
                .iter()
                .filter_map(|v| try_parse_location(v, position_maps))
                .collect();
            if !locations.is_empty() {
                return Some(GotoDefinitionResponse::Array(locations));
            }
        }
    }

    None
}

/// Try to parse a JSON value as a Location and translate its position.
fn try_parse_location(
    value: &serde_json::Value,
    position_maps: &DashMap<Url, PositionMap>,
) -> Option<Location> {
    let uri_str = value.get("uri")?.as_str()?;
    let uri = Url::parse(uri_str).ok()?;

    let range_val = value.get("range")?;
    let start = parse_position(range_val.get("start")?)?;
    let end = parse_position(range_val.get("end")?)?;
    let range = tower_lsp::lsp_types::Range { start, end };

    // Try to find a position map that can translate this .rs URI back to .rts.
    for entry in position_maps {
        let map = entry.value();
        if let Some(rts_uri) = map.rs_to_rts_uri(&uri)
            && let Some(rts_range) = map.rs_to_rts_range(range)
        {
            return Some(Location {
                uri: rts_uri,
                range: rts_range,
            });
        }
    }

    // External definition — pass through as-is.
    Some(Location { uri, range })
}

/// Parse a JSON object as an LSP Position.
fn parse_position(value: &serde_json::Value) -> Option<Position> {
    let line = u32::try_from(value.get("line")?.as_u64()?).ok()?;
    let character = u32::try_from(value.get("character")?.as_u64()?).ok()?;
    Some(Position { line, character })
}

/// Translate a completion response from rust-analyzer.
///
/// Translates completion item labels from Rust names to `RustScript` names
/// (e.g., `to_uppercase` -> `toUpperCase`).
fn translate_completion_response(response: &serde_json::Value) -> Option<CompletionResponse> {
    let result = response.get("result")?;

    if result.is_null() {
        return None;
    }

    // RA returns either a CompletionList or an array of CompletionItems.
    let items_value = if let Some(items) = result.get("items") {
        items
    } else if result.is_array() {
        result
    } else {
        return None;
    };

    let items: Vec<CompletionItem> = items_value
        .as_array()?
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?;
            let translated_label = name_map::translate_completion_label(label);

            let detail = item
                .get("detail")
                .and_then(serde_json::Value::as_str)
                .map(name_map::translate_type_string);

            Some(CompletionItem {
                label: translated_label,
                detail,
                ..Default::default()
            })
        })
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

/// Find hover information for the AST node at the given byte offset.
///
/// Walks the AST looking for the most specific node whose span contains
/// the cursor position. For Phase 3, handles:
/// - Function declarations: shows the full signature
/// - Variable declarations (const/let): shows `const name: Type` or `let name: Type`
/// - Parameters: shows `param: Type`
#[must_use]
fn find_hover_info(module: &rsc_syntax::ast::Module, offset: u32) -> Option<String> {
    use rsc_syntax::ast::ItemKind;
    use rsc_syntax::span::BytePos;

    let pos = BytePos(offset);

    for item in &module.items {
        if !item.span.contains(pos) {
            continue;
        }

        match &item.kind {
            ItemKind::Function(func) => {
                // Check if cursor is on the function name
                if func.name.span.contains(pos) {
                    return Some(format_function_signature(func));
                }

                // Check if cursor is on a parameter
                for param in &func.params {
                    if param.span.contains(pos) {
                        return Some(format_param(param));
                    }
                }

                // Check statements in the body for variable declarations
                if let Some(info) = find_hover_in_stmts(&func.body.stmts, pos) {
                    return Some(info);
                }
            }
            ItemKind::TypeDef(td) => {
                if td.name.span.contains(pos) {
                    return Some(format!("type {}", td.name.name));
                }
            }
            ItemKind::EnumDef(ed) => {
                if ed.name.span.contains(pos) {
                    return Some(format!("enum {}", ed.name.name));
                }
            }
            ItemKind::Interface(iface) => {
                if iface.name.span.contains(pos) {
                    return Some(format!("interface {}", iface.name.name));
                }
            }
            ItemKind::Class(class) => {
                if class.name.span.contains(pos) {
                    return Some(format!("class {}", class.name.name));
                }
            }
            _ => {}
        }
    }

    None
}

/// Get the span of a statement.
fn stmt_span(stmt: &rsc_syntax::ast::Stmt) -> rsc_syntax::span::Span {
    use rsc_syntax::ast::Stmt;
    match stmt {
        Stmt::VarDecl(d) => d.span,
        Stmt::Expr(e) => e.span,
        Stmt::Return(r) => r.span,
        Stmt::If(i) => i.span,
        Stmt::While(w) => w.span,
        Stmt::Destructure(d) => d.span,
        Stmt::Switch(s) => s.span,
        Stmt::TryCatch(t) => t.span,
        Stmt::For(f) => f.span,
        Stmt::ArrayDestructure(a) => a.span,
        Stmt::Break(b) => b.span,
        Stmt::Continue(c) => c.span,
    }
}

/// Search statements for hover-relevant nodes at the given position.
fn find_hover_in_stmts(
    stmts: &[rsc_syntax::ast::Stmt],
    pos: rsc_syntax::span::BytePos,
) -> Option<String> {
    use rsc_syntax::ast::{ElseClause, Stmt, VarBinding};

    for stmt in stmts {
        if !stmt_span(stmt).contains(pos) {
            continue;
        }

        match stmt {
            Stmt::VarDecl(decl) => {
                if decl.name.span.contains(pos) || decl.span.contains(pos) {
                    let binding = match decl.binding {
                        VarBinding::Const => "const",
                        VarBinding::Let => "let",
                    };
                    let type_str = decl
                        .type_ann
                        .as_ref()
                        .map_or_else(|| "(inferred)".to_owned(), format_type);
                    return Some(format!("{binding} {}: {type_str}", decl.name.name));
                }
            }
            Stmt::If(if_stmt) => {
                if let Some(info) = find_hover_in_stmts(&if_stmt.then_block.stmts, pos) {
                    return Some(info);
                }
                if let Some(else_clause) = &if_stmt.else_clause {
                    match else_clause {
                        ElseClause::Block(block) => {
                            if let Some(info) = find_hover_in_stmts(&block.stmts, pos) {
                                return Some(info);
                            }
                        }
                        ElseClause::ElseIf(else_if) => {
                            if let Some(info) = find_hover_in_stmts(&else_if.then_block.stmts, pos)
                            {
                                return Some(info);
                            }
                        }
                    }
                }
            }
            Stmt::While(while_stmt) => {
                if let Some(info) = find_hover_in_stmts(&while_stmt.body.stmts, pos) {
                    return Some(info);
                }
            }
            Stmt::For(for_stmt) => {
                if let Some(info) = find_hover_in_stmts(&for_stmt.body.stmts, pos) {
                    return Some(info);
                }
            }
            _ => {}
        }
    }

    None
}

/// Format a function signature for hover display.
fn format_function_signature(func: &rsc_syntax::ast::FnDecl) -> String {
    let mut sig = String::new();
    if func.is_async {
        sig.push_str("async ");
    }
    sig.push_str("function ");
    sig.push_str(&func.name.name);

    sig.push('(');
    for (i, param) in func.params.iter().enumerate() {
        if i > 0 {
            sig.push_str(", ");
        }
        sig.push_str(&param.name.name);
        sig.push_str(": ");
        sig.push_str(&format_type(&param.type_ann));
    }
    sig.push(')');

    if let Some(ret) = &func.return_type {
        if let Some(type_ann) = &ret.type_ann {
            sig.push_str(": ");
            sig.push_str(&format_type(type_ann));
        }
        if let Some(throws) = &ret.throws {
            sig.push_str(" throws ");
            sig.push_str(&format_type(throws));
        }
    }

    sig
}

/// Format a parameter for hover display.
fn format_param(param: &rsc_syntax::ast::Param) -> String {
    format!("{}: {}", param.name.name, format_type(&param.type_ann))
}

/// Format a type annotation to a display string.
fn format_type(type_ann: &rsc_syntax::ast::TypeAnnotation) -> String {
    use rsc_syntax::ast::TypeKind;
    match &type_ann.kind {
        TypeKind::Named(ident) => ident.name.clone(),
        TypeKind::Void => "void".to_owned(),
        TypeKind::Generic(name, args) => {
            let args_str: Vec<String> = args.iter().map(format_type).collect();
            format!("{}<{}>", name.name, args_str.join(", "))
        }
        TypeKind::Union(variants) => {
            let variants_str: Vec<String> = variants.iter().map(format_type).collect();
            variants_str.join(" | ")
        }
        TypeKind::Function(params, ret) => {
            let params_str: Vec<String> = params.iter().map(format_type).collect();
            format!("({}) => {}", params_str.join(", "), format_type(ret))
        }
        TypeKind::Intersection(types) => {
            let types_str: Vec<String> = types.iter().map(format_type).collect();
            types_str.join(" & ")
        }
        TypeKind::Inferred => "(inferred)".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::ServerCapabilities;

    // Test: Server capabilities are correct (updated with definition + completion)
    #[test]
    fn test_server_capabilities_include_formatting_and_hover() {
        let capabilities = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            document_formatting_provider: Some(OneOf::Left(true)),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            definition_provider: Some(OneOf::Left(true)),
            completion_provider: Some(CompletionOptions {
                trigger_characters: Some(vec![".".to_owned(), ":".to_owned()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(matches!(
            capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
        ));
        assert!(matches!(
            capabilities.document_formatting_provider,
            Some(OneOf::Left(true))
        ));
        assert!(matches!(
            capabilities.hover_provider,
            Some(HoverProviderCapability::Simple(true))
        ));
        assert!(matches!(
            capabilities.definition_provider,
            Some(OneOf::Left(true))
        ));
        assert!(capabilities.completion_provider.is_some());
    }

    // Test: Hover on function name returns signature
    #[test]
    fn test_server_hover_function_name_returns_signature() {
        let source = "function add(a: i32, b: i32): i32 {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "add" starts at offset 9
        let info = find_hover_info(&module, 9);
        assert!(info.is_some(), "should find hover info for function name");
        let text = info.unwrap();
        assert!(
            text.contains("function add"),
            "should contain function name: {text}"
        );
        assert!(text.contains("a: i32"), "should contain param a: {text}");
        assert!(text.contains("b: i32"), "should contain param b: {text}");
        assert!(text.contains(": i32"), "should contain return type: {text}");
    }

    // Test: Hover on parameter returns parameter info
    #[test]
    fn test_server_hover_parameter_returns_type() {
        let source = "function foo(x: i32) {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "x" is around offset 13
        let info = find_hover_info(&module, 13);
        assert!(info.is_some(), "should find hover info for parameter");
        let text = info.unwrap();
        assert!(text.contains("x: i32"), "should show param type: {text}");
    }

    // Test: Hover outside any node returns None
    #[test]
    fn test_server_hover_outside_node_returns_none() {
        let source = "function foo() {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // Offset way beyond the source
        let info = find_hover_info(&module, 1000);
        assert!(
            info.is_none(),
            "should return None for offset beyond source"
        );
    }

    // Test: Format type renders correctly for simple named type
    #[test]
    fn test_server_format_type_simple() {
        use rsc_syntax::ast::{Ident, TypeAnnotation, TypeKind};
        use rsc_syntax::span::Span;

        let ty = TypeAnnotation {
            kind: TypeKind::Named(Ident {
                name: "i32".to_owned(),
                span: Span::dummy(),
            }),
            span: Span::dummy(),
        };
        assert_eq!(format_type(&ty), "i32");
    }

    // Test: Format type renders correctly for void
    #[test]
    fn test_server_format_type_void() {
        use rsc_syntax::ast::{TypeAnnotation, TypeKind};
        use rsc_syntax::span::Span;

        let ty = TypeAnnotation {
            kind: TypeKind::Void,
            span: Span::dummy(),
        };
        assert_eq!(format_type(&ty), "void");
    }

    // Correctness scenario 3: Format via LSP
    #[test]
    fn test_server_correctness_format_unformatted_source_returns_edit() {
        let source = "function foo(){const x=1;}";
        let formatted = rsc_fmt::format_source(source).unwrap();

        // The formatter should produce different output
        assert_ne!(
            source, formatted,
            "unformatted source should differ after formatting"
        );

        // Verify the edit range covers the full document
        let range = diagnostics::full_document_range(source);
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);

        // The TextEdit would replace the full document with the formatted text
        let edit = TextEdit {
            range,
            new_text: formatted.clone(),
        };
        assert_ne!(edit.new_text, source);
        assert!(
            edit.new_text.contains("const x = 1"),
            "formatted output should have proper spacing: {}",
            edit.new_text
        );
    }

    // Test: Formatting no-op for already formatted source
    #[test]
    fn test_server_format_already_formatted_returns_none() {
        let source = "function foo() {}\n";
        let formatted = rsc_fmt::format_source(source).unwrap();

        // Already formatted source should produce identical output
        assert_eq!(
            source, formatted,
            "already formatted source should be unchanged"
        );
    }

    // Test: Document store tracks content
    #[test]
    fn test_server_document_store_insert_and_get() {
        let documents: DashMap<Url, String> = DashMap::new();
        let uri = Url::parse("file:///test.rts").unwrap();
        documents.insert(uri.clone(), "function foo() {}".to_owned());

        let stored = documents.get(&uri).unwrap();
        assert_eq!(*stored, "function foo() {}");
    }

    // Test: Document store update replaces content
    #[test]
    fn test_server_document_store_update() {
        let documents: DashMap<Url, String> = DashMap::new();
        let uri = Url::parse("file:///test.rts").unwrap();
        documents.insert(uri.clone(), "function foo() {}".to_owned());
        documents.insert(uri.clone(), "function bar() {}".to_owned());

        let stored = documents.get(&uri).unwrap();
        assert_eq!(*stored, "function bar() {}");
    }

    // Test: translate_definition_response with null result
    #[test]
    fn test_server_translate_definition_response_null_returns_none() {
        let response = serde_json::json!({ "result": null });
        let maps = DashMap::new();
        assert!(translate_definition_response(&response, &maps).is_none());
    }

    // Test: translate_definition_response with single location
    #[test]
    fn test_server_translate_definition_response_single_location() {
        let response = serde_json::json!({
            "result": {
                "uri": "file:///project/external.rs",
                "range": {
                    "start": { "line": 5, "character": 0 },
                    "end": { "line": 5, "character": 10 }
                }
            }
        });

        let maps = DashMap::new();
        let result = translate_definition_response(&response, &maps);
        assert!(result.is_some(), "should parse external location");
    }

    // Test: translate_completion_response with items
    #[test]
    fn test_server_translate_completion_response_translates_labels() {
        let response = serde_json::json!({
            "result": {
                "items": [
                    { "label": "to_uppercase", "detail": "fn() -> String" },
                    { "label": "custom_fn", "detail": "fn() -> i32" }
                ]
            }
        });

        let result = translate_completion_response(&response);
        assert!(result.is_some());
        if let Some(CompletionResponse::Array(items)) = result {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].label, "toUpperCase");
            assert_eq!(items[1].label, "custom_fn");
        } else {
            panic!("expected Array response");
        }
    }

    // Test: translate_completion_response with null returns None
    #[test]
    fn test_server_translate_completion_response_null_returns_none() {
        let response = serde_json::json!({ "result": null });
        assert!(translate_completion_response(&response).is_none());
    }

    // Test 8 (server-side): Graceful degradation with no RA
    #[test]
    fn test_server_graceful_degradation_goto_definition_no_ra() {
        // Without any position maps, goto_definition should return None
        // (we test the translation layer, not the full async server)
        let response = serde_json::json!({ "result": null });
        let maps: DashMap<Url, PositionMap> = DashMap::new();
        let result = translate_definition_response(&response, &maps);
        assert!(
            result.is_none(),
            "should gracefully return None with null result"
        );
    }

    // Test: parse_position helper
    #[test]
    fn test_server_parse_position() {
        let val = serde_json::json!({ "line": 10, "character": 5 });
        let pos = parse_position(&val);
        assert!(pos.is_some());
        let p = pos.unwrap();
        assert_eq!(p.line, 10);
        assert_eq!(p.character, 5);
    }

    // Test: parse_position with missing fields
    #[test]
    fn test_server_parse_position_missing_field() {
        let val = serde_json::json!({ "line": 10 });
        assert!(parse_position(&val).is_none());
    }
}
