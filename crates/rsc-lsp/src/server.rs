//! LSP server implementation for `RustScript`.
//!
//! Implements the [`tower_lsp::LanguageServer`] trait to provide diagnostics,
//! formatting, hover, go-to-definition, and completions for `.rts` files.
//! When available, proxies definition and completion requests through
//! rust-analyzer running on the generated `.rs` code.

use std::collections::HashMap;
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
    SignatureHelp, SignatureHelpOptions, SignatureHelpParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer};

use crate::builtin_hover;
use crate::completions;
use crate::diagnostics;
use crate::name_map;
use crate::position_map::PositionMap;
use crate::ra_proxy::RustAnalyzerProxy;
use rsc_driver::rustdoc_cache::RustdocCache;
use rsc_driver::rustdoc_parser;

/// Debounce delay for recompilation after document changes.
const DEBOUNCE_MS: u64 = 300;

/// Cached type information from a successful compilation.
///
/// Stores variable type information extracted during lowering, keyed by
/// variable name. Used to provide type-aware hover information without
/// re-compiling on every hover request.
#[derive(Debug, Clone)]
pub struct CachedCompileInfo {
    /// Map of variable name to its inferred `RustScript` type string.
    pub variable_types: HashMap<String, String>,
    /// Map of function name to its formatted signature.
    pub function_signatures: HashMap<String, String>,
}

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
    position_maps: Arc<DashMap<Url, PositionMap>>,
    /// Cached compile information per document for hover type lookups.
    compile_cache: DashMap<Url, CachedCompileInfo>,
    /// Rust-analyzer proxy (initialized on first successful compilation).
    ra_proxy: RwLock<Option<RustAnalyzerProxy>>,
    /// Project build directory (`.rsc-build/`).
    build_dir: RwLock<Option<PathBuf>>,
    /// Cache of parsed rustdoc JSON for external crate hover documentation.
    rustdoc_cache: RwLock<RustdocCache>,
}

impl RscLanguageServer {
    /// Create a new `RustScript` language server.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
            debounce_notifiers: DashMap::new(),
            position_maps: Arc::new(DashMap::new()),
            compile_cache: DashMap::new(),
            ra_proxy: RwLock::new(None),
            build_dir: RwLock::new(None),
            rustdoc_cache: RwLock::new(RustdocCache::new()),
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

        // Build type cache from a parse + lowering pass (extracts variable types).
        let file_id = rsc_syntax::source::FileId(0);
        let (module, parse_diags) = rsc_parser::parse(source, file_id);
        let has_parse_errors = parse_diags
            .iter()
            .any(|d| matches!(d.severity, rsc_syntax::diagnostic::Severity::Error));

        if !has_parse_errors {
            let cache_info = build_compile_cache(&module);
            self.compile_cache.insert(uri.clone(), cache_info);
        }

        // Run the full compilation to build the position map.
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
        let compile_cache = self.compile_cache.clone();
        let position_maps = Arc::clone(&self.position_maps);
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
                let source_text = source.clone();
                drop(source);

                let lsp_diagnostics = diagnostics::collect_diagnostics(&source_text);

                // Update the compile cache with type information.
                let file_id = rsc_syntax::source::FileId(0);
                let (module, parse_diags) = rsc_parser::parse(&source_text, file_id);
                let has_parse_errors = parse_diags
                    .iter()
                    .any(|d| matches!(d.severity, rsc_syntax::diagnostic::Severity::Error));

                if !has_parse_errors {
                    let cache_info = build_compile_cache(&module);
                    compile_cache.insert(uri.clone(), cache_info);
                }

                // Also update position maps from compilation.
                let compile_result = rsc_driver::compile_source(&source_text, "lsp_input.rts");
                if !compile_result.has_errors {
                    let position_map = PositionMap::new(
                        compile_result.source_map_lines,
                        source_text,
                        compile_result.rust_source.clone(),
                    );
                    position_maps.insert(uri.clone(), position_map);
                }

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
                    trigger_characters: Some(vec![".".to_owned(), ":".to_owned(), "\"".to_owned()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
                    retrigger_characters: Some(vec![",".to_owned()]),
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

            // Get the cached compile info for type-aware hover.
            let cache = self.compile_cache.get(uri).map(|c| c.clone());

            if let Some(info) = find_hover_info(&module, offset, cache.as_ref()) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: info,
                    }),
                    range: None,
                }));
            }

            // Try rustdoc-based hover for imported symbols.
            if let Some((crate_name, symbol_name)) = find_import_at_cursor(&module, offset)
                && let Some(build_dir) = self.build_dir.read().await.as_ref()
            {
                let mut rustdoc = self.rustdoc_cache.write().await;
                if let Some(crate_data) = rustdoc.get_crate_docs(&crate_name, build_dir)
                    && let Some(item) = rustdoc_parser::lookup_item(&crate_data, &symbol_name)
                {
                    let hover_text = crate::rustdoc_translator::translate_item_to_hover(item);
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: hover_text,
                        }),
                        range: None,
                    }));
                }
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

        // Try native RustScript completions first.
        if let Some(source) = self.documents.get(uri) {
            let source_text = source.clone();
            drop(source);

            let cache = self.compile_cache.get(uri).map(|c| c.clone());
            let rustdoc = self.rustdoc_cache.read().await;
            let ctx = completions::CompletionContext {
                source: &source_text,
                line: position.line,
                character: position.character,
                cache: cache.as_ref(),
                rustdoc: Some(&rustdoc),
            };

            if let Some(response) = completions::resolve_completions(&ctx) {
                return Ok(Some(response));
            }
        }

        // Fall back to rust-analyzer proxy.
        if let Some(response) = self.ra_completion(uri, position).await {
            return Ok(Some(response));
        }

        // Graceful degradation: return empty when RA is not available.
        Ok(None)
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(source) = self.documents.get(uri) {
            let source_text = source.clone();
            drop(source);

            let cache = self.compile_cache.get(uri).map(|c| c.clone());
            let ctx = completions::SignatureHelpContext {
                source: &source_text,
                line: position.line,
                character: position.character,
                cache: cache.as_ref(),
            };

            if let Some(help) = completions::resolve_signature_help(&ctx) {
                return Ok(Some(help));
            }
        }

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
fn find_hover_info(
    module: &rsc_syntax::ast::Module,
    offset: u32,
    cache: Option<&CachedCompileInfo>,
) -> Option<String> {
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
                    let sig = format_function_signature(func);
                    let code = format!("```rustscript\n{sig}\n```");
                    return Some(hover_with_doc(&code, func.doc_comment.as_deref()));
                }

                // Check if cursor is on a parameter
                for param in &func.params {
                    if param.span.contains(pos) {
                        let p = format_param(param);
                        return Some(format!("```rustscript\n(parameter) {p}\n```"));
                    }
                }

                // Check statements in the body for variable declarations
                if let Some(info) = find_hover_in_stmts(&func.body.stmts, pos, cache) {
                    return Some(info);
                }
            }
            ItemKind::TypeDef(td) => {
                if td.name.span.contains(pos) {
                    let hover = format_typedef_hover(td);
                    let code = format!("```rustscript\n{hover}\n```");
                    return Some(hover_with_doc(&code, td.doc_comment.as_deref()));
                }
            }
            ItemKind::EnumDef(ed) => {
                if ed.name.span.contains(pos) {
                    let hover = format_enum_hover(ed);
                    let code = format!("```rustscript\n{hover}\n```");
                    return Some(hover_with_doc(&code, ed.doc_comment.as_deref()));
                }
            }
            ItemKind::Interface(iface) => {
                if iface.name.span.contains(pos) {
                    let hover = format_interface_hover(iface);
                    let code = format!("```rustscript\n{hover}\n```");
                    return Some(hover_with_doc(&code, iface.doc_comment.as_deref()));
                }
            }
            ItemKind::Class(class) => {
                if class.name.span.contains(pos) {
                    let hover = format_class_hover(class);
                    let code = format!("```rustscript\n{hover}\n```");
                    return Some(hover_with_doc(&code, class.doc_comment.as_deref()));
                }
                // Walk class members for hover on fields, methods, getters, setters.
                if let Some(info) = find_hover_in_class_members(class, pos, cache) {
                    return Some(info);
                }
            }
            _ => {}
        }
    }

    None
}

/// Check if the cursor is on an imported name and return `(crate_name, symbol_name)`.
///
/// For `import { Router } from "axum"`, hovering over `Router` returns
/// `Some(("axum", "Router"))`. The source path must not start with `.` or `/`
/// (those are local imports, not external crate imports).
#[must_use]
fn find_import_at_cursor(
    module: &rsc_syntax::ast::Module,
    offset: u32,
) -> Option<(String, String)> {
    use rsc_syntax::ast::ItemKind;
    use rsc_syntax::span::BytePos;

    let pos = BytePos(offset);

    for item in &module.items {
        if !item.span.contains(pos) {
            continue;
        }

        if let ItemKind::Import(import) = &item.kind {
            let source = &import.source.value;

            // Skip local imports (relative paths).
            if source.starts_with('.') || source.starts_with('/') {
                continue;
            }

            // Check if cursor is on any imported name.
            for name in &import.names {
                if name.span.contains(pos) {
                    return Some((source.clone(), name.name.clone()));
                }
            }
        }
    }

    None
}

/// Format a hover response, optionally prepending a `JSDoc` comment as markdown.
fn hover_with_doc(code_block: &str, doc: Option<&str>) -> String {
    match doc {
        Some(doc) if !doc.is_empty() => format!("{doc}\n\n---\n\n{code_block}"),
        _ => code_block.to_owned(),
    }
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
        Stmt::DoWhile(dw) => dw.span,
        Stmt::Destructure(d) => d.span,
        Stmt::Switch(s) => s.span,
        Stmt::TryCatch(t) => t.span,
        Stmt::For(f) => f.span,
        Stmt::ArrayDestructure(a) => a.span,
        Stmt::Break(b) => b.span,
        Stmt::Continue(c) => c.span,
        Stmt::RustBlock(rb) => rb.span,
    }
}

#[allow(clippy::too_many_lines)]
// Statement hover covers all statement kinds including destructure variants
/// Search statements for hover-relevant nodes at the given position.
fn find_hover_in_stmts(
    stmts: &[rsc_syntax::ast::Stmt],
    pos: rsc_syntax::span::BytePos,
    cache: Option<&CachedCompileInfo>,
) -> Option<String> {
    use rsc_syntax::ast::{ElseClause, Stmt, VarBinding};

    for stmt in stmts {
        if !stmt_span(stmt).contains(pos) {
            continue;
        }

        match stmt {
            Stmt::VarDecl(decl) => {
                if decl.name.span.contains(pos) {
                    let binding = match decl.binding {
                        VarBinding::Const => "const",
                        VarBinding::Let => "let",
                    };
                    let type_str = if let Some(type_ann) = &decl.type_ann {
                        format_type(type_ann)
                    } else if let Some(ci) = cache
                        && let Some(t) = ci.variable_types.get(&decl.name.name)
                    {
                        t.clone()
                    } else {
                        "(inferred)".to_owned()
                    };
                    return Some(format!(
                        "```rustscript\n{binding} {}: {type_str}\n```",
                        decl.name.name
                    ));
                }
                // Walk into the init expression for identifier/method hover.
                if let Some(info) = find_hover_in_expr(&decl.init, pos, cache) {
                    return Some(info);
                }
            }
            Stmt::Expr(expr) => {
                if let Some(info) = find_hover_in_expr(expr, pos, cache) {
                    return Some(info);
                }
            }
            Stmt::If(if_stmt) => {
                if let Some(info) = find_hover_in_stmts(&if_stmt.then_block.stmts, pos, cache) {
                    return Some(info);
                }
                if let Some(else_clause) = &if_stmt.else_clause {
                    match else_clause {
                        ElseClause::Block(block) => {
                            if let Some(info) = find_hover_in_stmts(&block.stmts, pos, cache) {
                                return Some(info);
                            }
                        }
                        ElseClause::ElseIf(else_if) => {
                            if let Some(info) =
                                find_hover_in_stmts(&else_if.then_block.stmts, pos, cache)
                            {
                                return Some(info);
                            }
                        }
                    }
                }
            }
            Stmt::While(while_stmt) => {
                if let Some(info) = find_hover_in_stmts(&while_stmt.body.stmts, pos, cache) {
                    return Some(info);
                }
            }
            Stmt::For(for_stmt) => {
                if let Some(info) = find_hover_in_stmts(&for_stmt.body.stmts, pos, cache) {
                    return Some(info);
                }
            }
            Stmt::Return(ret) => {
                if let Some(value) = &ret.value
                    && let Some(info) = find_hover_in_expr(value, pos, cache)
                {
                    return Some(info);
                }
            }
            Stmt::TryCatch(tc) => {
                if let Some(info) = find_hover_in_stmts(&tc.try_block.stmts, pos, cache) {
                    return Some(info);
                }
                if let Some(block) = &tc.catch_block
                    && let Some(info) = find_hover_in_stmts(&block.stmts, pos, cache)
                {
                    return Some(info);
                }
                if let Some(finally_block) = &tc.finally_block
                    && let Some(info) = find_hover_in_stmts(&finally_block.stmts, pos, cache)
                {
                    return Some(info);
                }
            }
            Stmt::Destructure(d) => {
                // Hover on destructure fields: show rename/default info.
                for field in &d.fields {
                    if let Some(local) = &field.local_name
                        && local.span.contains(pos)
                    {
                        return Some(format!(
                            "```rustscript\n(destructure) {} — renamed from `{}`\n```",
                            local.name, field.field_name.name
                        ));
                    }
                    if field.field_name.span.contains(pos) {
                        let type_str = if let Some(ci) = cache
                            && let Some(t) = ci.variable_types.get(&field.field_name.name)
                        {
                            t.clone()
                        } else {
                            "(inferred)".to_owned()
                        };
                        return Some(format!(
                            "```rustscript\n(destructure field) {}: {type_str}\n```",
                            field.field_name.name
                        ));
                    }
                    if let Some(default_val) = &field.default_value
                        && default_val.span.contains(pos)
                    {
                        return Some(format!(
                            "```rustscript\n(default value) for `{}`\n```",
                            field.field_name.name
                        ));
                    }
                }
                if let Some(info) = find_hover_in_expr(&d.init, pos, cache) {
                    return Some(info);
                }
            }
            Stmt::ArrayDestructure(a) => {
                for elem in &a.elements {
                    use rsc_syntax::ast::ArrayDestructureElement;
                    match elem {
                        ArrayDestructureElement::Rest(ident) => {
                            if ident.span.contains(pos) {
                                return Some(format!(
                                    "```rustscript\n(rest element) ...{}: Array<T>\n```",
                                    ident.name
                                ));
                            }
                        }
                        ArrayDestructureElement::Single(ident) => {
                            if ident.span.contains(pos) {
                                let type_str = if let Some(ci) = cache
                                    && let Some(t) = ci.variable_types.get(&ident.name)
                                {
                                    t.clone()
                                } else {
                                    "(inferred)".to_owned()
                                };
                                return Some(format!(
                                    "```rustscript\n(array element) {}: {type_str}\n```",
                                    ident.name
                                ));
                            }
                        }
                    }
                }
                if let Some(info) = find_hover_in_expr(&a.init, pos, cache) {
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
///
/// Includes `...` prefix for rest parameters, `?` suffix for optional parameters,
/// and `= defaultValue` for parameters with defaults.
fn format_param(param: &rsc_syntax::ast::Param) -> String {
    let mut result = String::new();
    if param.is_rest {
        result.push_str("...");
    }
    result.push_str(&param.name.name);
    if param.optional {
        result.push('?');
    }
    result.push_str(": ");
    result.push_str(&format_type(&param.type_ann));
    if param.default_value.is_some() {
        result.push_str(" = ...");
    }
    result
}

/// Format optional type parameters for display: `<T, U extends Comparable>`.
fn format_type_params(type_params: Option<&rsc_syntax::ast::TypeParams>) -> String {
    match type_params {
        Some(tp) if !tp.params.is_empty() => {
            let params: Vec<String> = tp
                .params
                .iter()
                .map(|p| {
                    if let Some(constraint) = &p.constraint {
                        format!("{} extends {}", p.name.name, format_type(constraint))
                    } else {
                        p.name.name.clone()
                    }
                })
                .collect();
            format!("<{}>", params.join(", "))
        }
        _ => String::new(),
    }
}

/// Format a type definition for hover: `type User = { name: string, age: u32 }`.
fn format_typedef_hover(td: &rsc_syntax::ast::TypeDef) -> String {
    let tp = format_type_params(td.type_params.as_ref());
    let fields: Vec<String> = td
        .fields
        .iter()
        .map(|f| format!("{}: {}", f.name.name, format_type(&f.type_ann)))
        .collect();
    if fields.is_empty() {
        format!("type {}{tp}", td.name.name)
    } else {
        format!("type {}{tp} = {{ {} }}", td.name.name, fields.join(", "))
    }
}

/// Format an enum definition for hover.
fn format_enum_hover(ed: &rsc_syntax::ast::EnumDef) -> String {
    use rsc_syntax::ast::EnumVariant;
    let variants: Vec<String> = ed
        .variants
        .iter()
        .map(|v| match v {
            EnumVariant::Simple(ident, _) => format!("\"{}\"", ident.name),
            EnumVariant::Data { name, fields, .. } => {
                let field_strs: Vec<String> = fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name.name, format_type(&f.type_ann)))
                    .collect();
                format!("{{ kind: \"{}\", {} }}", name.name, field_strs.join(", "))
            }
        })
        .collect();
    if variants.is_empty() {
        format!("enum {}", ed.name.name)
    } else {
        format!("type {} = {}", ed.name.name, variants.join(" | "))
    }
}

/// Format an interface for hover: `interface Printable<T>`.
fn format_interface_hover(iface: &rsc_syntax::ast::InterfaceDef) -> String {
    let tp = format_type_params(iface.type_params.as_ref());
    format!("interface {}{tp}", iface.name.name)
}

/// Format a class for hover: `class MyClass implements Trait`.
fn format_class_hover(class: &rsc_syntax::ast::ClassDef) -> String {
    let tp = format_type_params(class.type_params.as_ref());
    let implements = if class.implements.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = class.implements.iter().map(|i| i.name.as_str()).collect();
        format!(" implements {}", names.join(", "))
    };
    format!("class {}{tp}{implements}", class.name.name)
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
        TypeKind::Shared(inner) => format!("shared<{}>", format_type(inner)),
        TypeKind::Tuple(types) => {
            let types_str: Vec<String> = types.iter().map(format_type).collect();
            format!("[{}]", types_str.join(", "))
        }
        TypeKind::IndexSignature(sig) => {
            format!(
                "{{ [{}:  {}]: {} }}",
                sig.key_name.name,
                format_type(&sig.key_type),
                format_type(&sig.value_type)
            )
        }
        TypeKind::StringLiteral(value) => format!("\"{value}\""),
        TypeKind::KeyOf(inner) => format!("keyof {}", format_type(inner)),
        TypeKind::TypeOf(ident) => format!("typeof {}", ident.name),
        TypeKind::Conditional {
            check_type,
            extends_type,
            true_type,
            false_type,
        } => format!(
            "{} extends {} ? {} : {}",
            format_type(check_type),
            format_type(extends_type),
            format_type(true_type),
            format_type(false_type)
        ),
        TypeKind::Infer(ident) => format!("infer {}", ident.name),
        TypeKind::TupleSpread(inner) => format!("...{}", format_type(inner)),
    }
}

/// Walk an expression tree to find hover-relevant nodes at the given position.
#[allow(clippy::too_many_lines)]
fn find_hover_in_expr(
    expr: &rsc_syntax::ast::Expr,
    pos: rsc_syntax::span::BytePos,
    cache: Option<&CachedCompileInfo>,
) -> Option<String> {
    use rsc_syntax::ast::ExprKind;

    if !expr.span.contains(pos) {
        return None;
    }

    match &expr.kind {
        ExprKind::Ident(ident) if ident.span.contains(pos) => {
            if let Some(hover) = builtin_hover::lookup_identifier(&ident.name) {
                return Some(hover.to_owned());
            }
            if let Some(ci) = cache
                && let Some(type_str) = ci.variable_types.get(&ident.name)
            {
                return Some(format!("```rustscript\n{}: {type_str}\n```", ident.name));
            }
            None
        }
        ExprKind::MethodCall(mc) => {
            if mc.method.span.contains(pos) {
                let receiver_type = match &mc.object.kind {
                    ExprKind::Ident(ident) => builtin_hover::classify_receiver(&ident.name)
                        .map(String::from)
                        .or_else(|| {
                            cache
                                .and_then(|ci| ci.variable_types.get(&ident.name))
                                .and_then(|ty| rust_type_to_builtin_category(ty))
                        }),
                    _ => None,
                };
                if let Some(recv) = &receiver_type
                    && let Some(hover) = builtin_hover::lookup_method(recv, &mc.method.name)
                {
                    return Some(hover.to_owned());
                }
                return Some(format!("```rustscript\n.{}()\n```", mc.method.name));
            }
            if let Some(info) = find_hover_in_expr(&mc.object, pos, cache) {
                return Some(info);
            }
            for arg in &mc.args {
                if let Some(info) = find_hover_in_expr(arg, pos, cache) {
                    return Some(info);
                }
            }
            None
        }
        ExprKind::Call(call) => {
            if call.callee.span.contains(pos) {
                if let Some(hover) = builtin_hover::lookup_identifier(&call.callee.name) {
                    return Some(hover.to_owned());
                }
                if let Some(ci) = cache
                    && let Some(sig) = ci.function_signatures.get(&call.callee.name)
                {
                    return Some(format!("```rustscript\n{sig}\n```"));
                }
            }
            for arg in &call.args {
                if let Some(info) = find_hover_in_expr(arg, pos, cache) {
                    return Some(info);
                }
            }
            None
        }
        ExprKind::NullLit => Some(
            builtin_hover::lookup_identifier("null")
                .unwrap_or("null")
                .to_owned(),
        ),
        ExprKind::BoolLit(true) => Some(
            builtin_hover::lookup_identifier("true")
                .unwrap_or("true")
                .to_owned(),
        ),
        ExprKind::BoolLit(false) => Some(
            builtin_hover::lookup_identifier("false")
                .unwrap_or("false")
                .to_owned(),
        ),
        ExprKind::This => Some(
            builtin_hover::lookup_identifier("this")
                .unwrap_or("this")
                .to_owned(),
        ),
        ExprKind::IntLit(v) => Some(format!("```rustscript\n{v}: i64\n```")),
        ExprKind::FloatLit(v) => Some(format!("```rustscript\n{v}: f64\n```")),
        ExprKind::StringLit(_) => Some("```rustscript\nstring\n```".to_owned()),
        ExprKind::Binary(bin) => {
            // Try children first; if cursor is not on a child, provide operator hover.
            if let Some(info) = find_hover_in_expr(&bin.left, pos, cache) {
                return Some(info);
            }
            if let Some(info) = find_hover_in_expr(&bin.right, pos, cache) {
                return Some(info);
            }
            // Cursor is on the operator itself — provide operator-specific hover.
            Some(format_binary_op_hover(bin.op))
        }
        ExprKind::Unary(un) => {
            if let Some(info) = find_hover_in_expr(&un.operand, pos, cache) {
                return Some(info);
            }
            Some(format_unary_op_hover(un.op))
        }
        ExprKind::Ternary(cond, then_expr, else_expr) => {
            if let Some(info) = find_hover_in_expr(cond, pos, cache) {
                return Some(info);
            }
            if let Some(info) = find_hover_in_expr(then_expr, pos, cache) {
                return Some(info);
            }
            if let Some(info) = find_hover_in_expr(else_expr, pos, cache) {
                return Some(info);
            }
            Some("```rustscript\n(ternary) condition ? consequent : alternate\n```\nShort-circuit conditional. Lowers to `if/else` expression.".to_owned())
        }
        ExprKind::NonNullAssert(inner) => {
            if let Some(info) = find_hover_in_expr(inner, pos, cache) {
                return Some(info);
            }
            Some("```rustscript\n(assert) expr!\n```\nNon-null assertion. Lowers to `.unwrap()`. Panics if `None`.".to_owned())
        }
        ExprKind::Cast(inner, _ty) => {
            if let Some(info) = find_hover_in_expr(inner, pos, cache) {
                return Some(info);
            }
            Some("```rustscript\n(cast) expr as Type\n```\nType cast. Lowers to Rust `as` for numeric types.".to_owned())
        }
        ExprKind::TypeOf(inner) => {
            if let Some(info) = find_hover_in_expr(inner, pos, cache) {
                return Some(info);
            }
            Some("```rustscript\n(operator) typeof expr\n```\nType-of operator. Resolves statically at compile time.".to_owned())
        }
        ExprKind::SpreadArg(inner) => {
            if let Some(info) = find_hover_in_expr(inner, pos, cache) {
                return Some(info);
            }
            Some("```rustscript\n(spread) ...expr\n```\nSpread operator. Expands array/object elements inline.".to_owned())
        }
        ExprKind::LogicalAssign(la) => {
            if let Some(info) = find_hover_in_expr(&la.value, pos, cache) {
                return Some(info);
            }
            let op_str = match la.op {
                rsc_syntax::ast::LogicalAssignOp::NullishAssign => "??=",
                rsc_syntax::ast::LogicalAssignOp::OrAssign => "||=",
                rsc_syntax::ast::LogicalAssignOp::AndAssign => "&&=",
            };
            builtin_hover::lookup_keyword(op_str).map(std::string::ToString::to_string)
        }
        ExprKind::Paren(inner)
        | ExprKind::Await(inner)
        | ExprKind::Throw(inner)
        | ExprKind::Shared(inner) => find_hover_in_expr(inner, pos, cache),
        ExprKind::FieldAccess(fa) => {
            if fa.field.span.contains(pos) {
                return Some(format!("```rustscript\n.{}\n```", fa.field.name));
            }
            find_hover_in_expr(&fa.object, pos, cache)
        }
        ExprKind::Assign(assign) => find_hover_in_expr(&assign.value, pos, cache),
        ExprKind::Index(idx) => {
            if let Some(info) = find_hover_in_expr(&idx.object, pos, cache) {
                return Some(info);
            }
            find_hover_in_expr(&idx.index, pos, cache)
        }
        ExprKind::ArrayLit(elems) => {
            for elem in elems {
                let inner = match elem {
                    rsc_syntax::ast::ArrayElement::Expr(e)
                    | rsc_syntax::ast::ArrayElement::Spread(e) => e,
                };
                if let Some(info) = find_hover_in_expr(inner, pos, cache) {
                    return Some(info);
                }
            }
            None
        }
        ExprKind::TemplateLit(tl) => {
            for part in &tl.parts {
                if let rsc_syntax::ast::TemplatePart::Expr(expr) = part
                    && let Some(info) = find_hover_in_expr(expr, pos, cache)
                {
                    return Some(info);
                }
            }
            None
        }
        _ => None,
    }
}

/// Walk class members for hover information.
///
/// Provides hover for class fields (including `readonly` / `static` modifiers),
/// methods (including `static` prefix), getters, setters, and constructor parameters
/// (including parameter properties).
#[allow(clippy::too_many_lines)]
fn find_hover_in_class_members(
    class: &rsc_syntax::ast::ClassDef,
    pos: rsc_syntax::span::BytePos,
    cache: Option<&CachedCompileInfo>,
) -> Option<String> {
    use rsc_syntax::ast::{ClassMember, Visibility};

    for member in &class.members {
        match member {
            ClassMember::Field(field) => {
                if field.name.span.contains(pos) {
                    let mut hover = String::new();
                    if field.is_static {
                        hover.push_str("static ");
                    }
                    if field.readonly {
                        hover.push_str("readonly ");
                    }
                    hover.push_str(&field.name.name);
                    hover.push_str(": ");
                    hover.push_str(&format_type(&field.type_ann));
                    if field.initializer.is_some() {
                        hover.push_str(" = ...");
                    }
                    let code = format!("```rustscript\n{hover}\n```");
                    return Some(hover_with_doc(&code, field.doc_comment.as_deref()));
                }
            }
            ClassMember::Method(method) => {
                if method.name.span.contains(pos) {
                    let mut hover = String::new();
                    if matches!(method.visibility, Visibility::Private) {
                        hover.push_str("private ");
                    }
                    if method.is_static {
                        hover.push_str("static ");
                    }
                    if method.is_async {
                        hover.push_str("async ");
                    }
                    hover.push_str(&method.name.name);
                    hover.push('(');
                    for (i, param) in method.params.iter().enumerate() {
                        if i > 0 {
                            hover.push_str(", ");
                        }
                        hover.push_str(&format_param(param));
                    }
                    hover.push(')');
                    if let Some(ret) = &method.return_type
                        && let Some(type_ann) = &ret.type_ann
                    {
                        hover.push_str(": ");
                        hover.push_str(&format_type(type_ann));
                    }
                    let code = format!("```rustscript\n{hover}\n```");
                    return Some(hover_with_doc(&code, method.doc_comment.as_deref()));
                }
                // Walk method body for variable hover.
                for param in &method.params {
                    if param.span.contains(pos) {
                        let p = format_param(param);
                        return Some(format!("```rustscript\n(parameter) {p}\n```"));
                    }
                }
                if let Some(info) = find_hover_in_stmts(&method.body.stmts, pos, cache) {
                    return Some(info);
                }
            }
            ClassMember::Getter(getter) => {
                if getter.name.span.contains(pos) {
                    let mut hover = String::from("(getter) get ");
                    hover.push_str(&getter.name.name);
                    hover.push_str("()");
                    if let Some(ret) = &getter.return_type
                        && let Some(type_ann) = &ret.type_ann
                    {
                        hover.push_str(": ");
                        hover.push_str(&format_type(type_ann));
                    }
                    return Some(format!(
                        "```rustscript\n{hover}\n```\nProperty getter. Accessed as `obj.{}`.)",
                        getter.name.name
                    ));
                }
                if let Some(info) = find_hover_in_stmts(&getter.body.stmts, pos, cache) {
                    return Some(info);
                }
            }
            ClassMember::Setter(setter) => {
                if setter.name.span.contains(pos) {
                    let mut hover = String::from("(setter) set ");
                    hover.push_str(&setter.name.name);
                    hover.push('(');
                    hover.push_str(&setter.param.name.name);
                    hover.push_str(": ");
                    hover.push_str(&format_type(&setter.param.type_ann));
                    hover.push(')');
                    return Some(format!(
                        "```rustscript\n{hover}\n```\nProperty setter. Assigned as `obj.{} = value`.)",
                        setter.name.name
                    ));
                }
                if let Some(info) = find_hover_in_stmts(&setter.body.stmts, pos, cache) {
                    return Some(info);
                }
            }
            ClassMember::Constructor(ctor) => {
                // Constructor param properties
                for param in &ctor.params {
                    if param.name.span.contains(pos) {
                        let mut hover = String::new();
                        if param.property_visibility.is_some() {
                            hover.push_str("(parameter property) ");
                        } else {
                            hover.push_str("(parameter) ");
                        }
                        hover.push_str(&param.name.name);
                        hover.push_str(": ");
                        hover.push_str(&format_type(&param.type_ann));
                        if param.property_visibility.is_some() {
                            hover.push_str("\n\nAutomatically creates and assigns a field.");
                        }
                        return Some(format!("```rustscript\n{hover}\n```"));
                    }
                }
                if let Some(info) = find_hover_in_stmts(&ctor.body.stmts, pos, cache) {
                    return Some(info);
                }
            }
        }
    }

    None
}

/// Format hover text for a binary operator.
fn format_binary_op_hover(op: rsc_syntax::ast::BinaryOp) -> String {
    use rsc_syntax::ast::BinaryOp;
    match op {
        BinaryOp::Pow => "```rustscript\n(operator) **\n```\nExponentiation. Lowers to `.pow()` for integers, `.powf()` for floats.".to_owned(),
        BinaryOp::BitAnd => "```rustscript\n(operator) &\n```\nBitwise AND. Direct passthrough to Rust.".to_owned(),
        BinaryOp::BitOr => "```rustscript\n(operator) |\n```\nBitwise OR. Direct passthrough to Rust.".to_owned(),
        BinaryOp::BitXor => "```rustscript\n(operator) ^\n```\nBitwise XOR. Direct passthrough to Rust.".to_owned(),
        BinaryOp::Shl => "```rustscript\n(operator) <<\n```\nLeft shift. Direct passthrough to Rust.".to_owned(),
        BinaryOp::Shr => "```rustscript\n(operator) >>\n```\nRight shift. Direct passthrough to Rust.".to_owned(),
        BinaryOp::Add => "```rustscript\n(operator) +\n```\nAddition.".to_owned(),
        BinaryOp::Sub => "```rustscript\n(operator) -\n```\nSubtraction.".to_owned(),
        BinaryOp::Mul => "```rustscript\n(operator) *\n```\nMultiplication.".to_owned(),
        BinaryOp::Div => "```rustscript\n(operator) /\n```\nDivision.".to_owned(),
        BinaryOp::Mod => "```rustscript\n(operator) %\n```\nModulo / remainder.".to_owned(),
        BinaryOp::Eq => "```rustscript\n(operator) ==\n```\nEquality comparison.".to_owned(),
        BinaryOp::Ne => "```rustscript\n(operator) !=\n```\nInequality comparison.".to_owned(),
        BinaryOp::Lt => "```rustscript\n(operator) <\n```\nLess than.".to_owned(),
        BinaryOp::Gt => "```rustscript\n(operator) >\n```\nGreater than.".to_owned(),
        BinaryOp::Le => "```rustscript\n(operator) <=\n```\nLess than or equal.".to_owned(),
        BinaryOp::Ge => "```rustscript\n(operator) >=\n```\nGreater than or equal.".to_owned(),
        BinaryOp::And => "```rustscript\n(operator) &&\n```\nLogical AND.".to_owned(),
        BinaryOp::Or => "```rustscript\n(operator) ||\n```\nLogical OR.".to_owned(),
    }
}

/// Format hover text for a unary operator.
fn format_unary_op_hover(op: rsc_syntax::ast::UnaryOp) -> String {
    use rsc_syntax::ast::UnaryOp;
    match op {
        UnaryOp::Neg => "```rustscript\n(operator) -\n```\nNegation.".to_owned(),
        UnaryOp::Not => "```rustscript\n(operator) !\n```\nLogical NOT.".to_owned(),
        UnaryOp::BitNot => {
            "```rustscript\n(operator) ~\n```\nBitwise NOT. Lowers to Rust `!` on integer types."
                .to_owned()
        }
    }
}

/// Map a Rust type string from the compile cache to a builtin category for method lookup.
fn rust_type_to_builtin_category(rust_type: &str) -> Option<String> {
    if rust_type == "String" || rust_type == "string" {
        Some("string".to_owned())
    } else if rust_type.starts_with("Vec<") || rust_type.starts_with("Array<") {
        Some("array".to_owned())
    } else {
        None
    }
}

/// Build a [`CachedCompileInfo`] by lowering the parsed AST.
fn build_compile_cache(module: &rsc_syntax::ast::Module) -> CachedCompileInfo {
    let lower_result = rsc_lower::lower(module);
    let mut variable_types = HashMap::new();
    let mut function_signatures = HashMap::new();

    for item in &lower_result.ir.items {
        match item {
            rsc_syntax::rust_ir::RustItem::Function(func) => {
                let sig = format_rust_function_sig(func);
                function_signatures.insert(func.name.clone(), sig);
                extract_var_types_from_stmts(&func.body.stmts, &mut variable_types);
            }
            rsc_syntax::rust_ir::RustItem::Struct(s) => {
                let fields: Vec<String> = s
                    .fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, rust_type_to_rts_type(&f.ty)))
                    .collect();
                let info = format!("type {} = {{ {} }}", s.name, fields.join(", "));
                variable_types.insert(s.name.clone(), info);
            }
            _ => {}
        }
    }

    // Infer literal types for variables without annotations.
    for item in &module.items {
        if let rsc_syntax::ast::ItemKind::Function(func) = &item.kind {
            extract_inferred_var_types(&func.body.stmts, &mut variable_types);
        }
    }

    CachedCompileInfo {
        variable_types,
        function_signatures,
    }
}

/// Extract inferred types from AST statements.
fn extract_inferred_var_types(
    stmts: &[rsc_syntax::ast::Stmt],
    types: &mut HashMap<String, String>,
) {
    use rsc_syntax::ast::Stmt;
    for stmt in stmts {
        if let Stmt::VarDecl(decl) = stmt
            && decl.type_ann.is_none()
            && !types.contains_key(&decl.name.name)
            && let Some(type_str) = infer_literal_type_string(&decl.init.kind)
        {
            types.insert(decl.name.name.clone(), type_str);
        }
    }
}

/// Infer a `RustScript` type string from a literal expression kind.
fn infer_literal_type_string(kind: &rsc_syntax::ast::ExprKind) -> Option<String> {
    use rsc_syntax::ast::ExprKind;
    match kind {
        ExprKind::IntLit(_) => Some("i64".to_owned()),
        ExprKind::FloatLit(_) => Some("f64".to_owned()),
        ExprKind::StringLit(_) | ExprKind::TemplateLit(_) => Some("string".to_owned()),
        ExprKind::BoolLit(_) => Some("boolean".to_owned()),
        ExprKind::NullLit => Some("null".to_owned()),
        ExprKind::ArrayLit(_) => Some("Array<_>".to_owned()),
        _ => None,
    }
}

/// Format a Rust IR function signature as a `RustScript` signature string.
fn format_rust_function_sig(func: &rsc_syntax::rust_ir::RustFnDecl) -> String {
    let mut sig = String::new();
    if func.is_async {
        sig.push_str("async ");
    }
    sig.push_str("function ");
    sig.push_str(&func.name);
    sig.push('(');
    for (i, param) in func.params.iter().enumerate() {
        if i > 0 {
            sig.push_str(", ");
        }
        sig.push_str(&param.name);
        sig.push_str(": ");
        sig.push_str(&rust_type_to_rts_type(&param.ty));
    }
    sig.push(')');
    if let Some(ret_type) = &func.return_type {
        sig.push_str(": ");
        sig.push_str(&rust_type_to_rts_type(ret_type));
    }
    sig
}

/// Convert a Rust IR type to a `RustScript` display string.
///
/// Delegates to [`name_map::rust_type_to_rts_display`] — the single authoritative
/// translation from Rust IR types to `RustScript` display syntax.
fn rust_type_to_rts_type(ty: &rsc_syntax::rust_ir::RustType) -> String {
    name_map::rust_type_to_rts_display(ty)
}

/// Extract variable types from Rust IR statements into the type map.
fn extract_var_types_from_stmts(
    stmts: &[rsc_syntax::rust_ir::RustStmt],
    types: &mut HashMap<String, String>,
) {
    use rsc_syntax::rust_ir::{RustElse, RustStmt};
    for stmt in stmts {
        match stmt {
            RustStmt::Let(let_stmt) => {
                if let Some(ty) = &let_stmt.ty {
                    types.insert(let_stmt.name.clone(), rust_type_to_rts_type(ty));
                }
            }
            RustStmt::If(if_stmt) => {
                extract_var_types_from_stmts(&if_stmt.then_block.stmts, types);
                if let Some(else_clause) = &if_stmt.else_clause {
                    match else_clause {
                        RustElse::Block(block) => extract_var_types_from_stmts(&block.stmts, types),
                        RustElse::ElseIf(else_if) => {
                            extract_var_types_from_stmts(&else_if.then_block.stmts, types);
                        }
                    }
                }
            }
            RustStmt::While(w) => extract_var_types_from_stmts(&w.body.stmts, types),
            RustStmt::ForIn(f) => extract_var_types_from_stmts(&f.body.stmts, types),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::ServerCapabilities;

    // Test: Server capabilities are correct (updated with definition + completion + signature help)
    #[test]
    fn test_server_capabilities_include_formatting_and_hover() {
        let capabilities = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            document_formatting_provider: Some(OneOf::Left(true)),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            definition_provider: Some(OneOf::Left(true)),
            completion_provider: Some(CompletionOptions {
                trigger_characters: Some(vec![".".to_owned(), ":".to_owned(), "\"".to_owned()]),
                ..Default::default()
            }),
            signature_help_provider: Some(SignatureHelpOptions {
                trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
                retrigger_characters: Some(vec![",".to_owned()]),
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
        assert!(capabilities.signature_help_provider.is_some());
    }

    // Test: Hover on function name returns signature
    #[test]
    fn test_server_hover_function_name_returns_signature() {
        let source = "function add(a: i32, b: i32): i32 {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "add" starts at offset 9
        let info = find_hover_info(&module, 9, None);
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
        let info = find_hover_info(&module, 13, None);
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
        let info = find_hover_info(&module, 1000, None);
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

    // -----------------------------------------------------------------------
    // Hover: type definitions show RustScript syntax
    // -----------------------------------------------------------------------

    #[test]
    fn test_server_hover_type_def_shows_fields() {
        let source = "type User = { name: string, age: u32 }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "User" starts at offset 5
        let info = find_hover_info(&module, 5, None);
        assert!(info.is_some(), "should find hover info for type name");
        let text = info.unwrap();
        assert!(
            text.contains("type User"),
            "should show 'type User': {text}"
        );
        assert!(
            text.contains("name: string"),
            "should show field types: {text}"
        );
        assert!(text.contains("age: u32"), "should show field types: {text}");
    }

    #[test]
    fn test_server_hover_interface_shows_name() {
        let source = "interface Printable { print(): void; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "Printable" starts at offset 10
        let info = find_hover_info(&module, 10, None);
        assert!(info.is_some(), "should find hover info for interface name");
        let text = info.unwrap();
        assert!(
            text.contains("interface Printable"),
            "should show 'interface Printable': {text}"
        );
    }

    #[test]
    fn test_server_hover_enum_shows_variants() {
        let source = r#"type Direction = "north" | "south" | "east" | "west""#;
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "Direction" starts at offset 5
        let info = find_hover_info(&module, 5, None);
        assert!(info.is_some(), "should find hover info for enum name");
        let text = info.unwrap();
        assert!(text.contains("Direction"), "should show enum name: {text}");
    }

    #[test]
    fn test_server_hover_class_shows_name() {
        let source = "class MyClass { x: i32; constructor(x: i32) { this.x = x; } }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "MyClass" starts at offset 6
        let info = find_hover_info(&module, 6, None);
        assert!(info.is_some(), "should find hover info for class name");
        let text = info.unwrap();
        assert!(
            text.contains("class MyClass"),
            "should show 'class MyClass': {text}"
        );
    }

    #[test]
    fn test_server_hover_variable_const_with_type_annotation() {
        let source = "function foo() { const x: i32 = 42; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "x" is at offset 23
        let info = find_hover_info(&module, 23, None);
        assert!(info.is_some(), "should find hover for const variable");
        let text = info.unwrap();
        assert!(
            text.contains("const x: i32"),
            "should show 'const x: i32': {text}"
        );
    }

    #[test]
    fn test_server_hover_variable_let_with_type_annotation() {
        let source = "function foo() { let y: string = \"hello\"; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "y" is at offset 21
        let info = find_hover_info(&module, 21, None);
        assert!(info.is_some(), "should find hover for let variable");
        let text = info.unwrap();
        assert!(
            text.contains("let y: string"),
            "should show 'let y: string': {text}"
        );
    }

    #[test]
    fn test_server_hover_int_literal() {
        let source = "function foo() { const x = 42; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "42" starts at offset 27
        let info = find_hover_info(&module, 27, None);
        assert!(info.is_some(), "should find hover for int literal");
        let text = info.unwrap();
        assert!(text.contains("i64"), "should show i64 type: {text}");
    }

    #[test]
    fn test_server_hover_string_literal() {
        let source = "function foo() { const x = \"hello\"; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // The string literal starts at offset 27
        let info = find_hover_info(&module, 28, None);
        assert!(info.is_some(), "should find hover for string literal");
        let text = info.unwrap();
        assert!(text.contains("string"), "should show string type: {text}");
    }

    #[test]
    fn test_server_hover_null_literal() {
        let source = "function foo() { const x = null; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "null" starts at offset 27
        let info = find_hover_info(&module, 27, None);
        assert!(info.is_some(), "should find hover for null literal");
        let text = info.unwrap();
        assert!(text.contains("null"), "should show null info: {text}");
    }

    #[test]
    fn test_server_hover_bool_literal() {
        let source = "function foo() { const x = true; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "true" starts at offset 27
        let info = find_hover_info(&module, 27, None);
        assert!(info.is_some(), "should find hover for bool literal");
        let text = info.unwrap();
        assert!(text.contains("boolean"), "should show boolean type: {text}");
    }

    #[test]
    fn test_server_hover_function_signature_uses_rts_syntax() {
        let source = "function greet(name: string): void {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "greet" starts at offset 9
        let info = find_hover_info(&module, 9, None);
        assert!(info.is_some());
        let text = info.unwrap();
        // Must use RustScript syntax, not Rust syntax
        assert!(
            text.contains("function greet"),
            "should use 'function', not 'fn': {text}"
        );
        assert!(
            !text.contains("fn greet"),
            "must NOT show Rust 'fn' syntax: {text}"
        );
        assert!(
            text.contains("name: string"),
            "should show param with string type: {text}"
        );
        assert!(
            text.contains(": void"),
            "should show void return type: {text}"
        );
    }

    #[test]
    fn test_server_hover_async_function_shows_async() {
        let source = "async function fetchData(): string {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "fetchData" starts at offset 15
        let info = find_hover_info(&module, 15, None);
        assert!(info.is_some());
        let text = info.unwrap();
        assert!(
            text.contains("async function fetchData"),
            "should show async: {text}"
        );
    }

    #[test]
    fn test_server_hover_uses_rustscript_code_block() {
        let source = "function foo() {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        // "foo" starts at offset 9
        let info = find_hover_info(&module, 9, None);
        assert!(info.is_some());
        let text = info.unwrap();
        assert!(
            text.contains("```rustscript"),
            "should use rustscript code block: {text}"
        );
    }

    #[test]
    fn test_server_format_type_params_single() {
        use rsc_syntax::ast::{Ident, TypeParam, TypeParams};
        use rsc_syntax::span::Span;

        let tp = Some(TypeParams {
            params: vec![TypeParam {
                name: Ident {
                    name: "T".to_owned(),
                    span: Span::dummy(),
                },
                constraint: None,
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        });
        assert_eq!(format_type_params(tp.as_ref()), "<T>");
    }

    #[test]
    fn test_server_format_type_params_with_constraint() {
        use rsc_syntax::ast::{Ident, TypeAnnotation, TypeKind, TypeParam, TypeParams};
        use rsc_syntax::span::Span;

        let tp = Some(TypeParams {
            params: vec![TypeParam {
                name: Ident {
                    name: "T".to_owned(),
                    span: Span::dummy(),
                },
                constraint: Some(TypeAnnotation {
                    kind: TypeKind::Named(Ident {
                        name: "Comparable".to_owned(),
                        span: Span::dummy(),
                    }),
                    span: Span::dummy(),
                }),
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        });
        assert_eq!(format_type_params(tp.as_ref()), "<T extends Comparable>");
    }

    #[test]
    fn test_server_format_type_params_none() {
        assert_eq!(format_type_params(None), "");
    }

    #[test]
    fn test_server_hover_variable_with_cache_inferred_type() {
        let source = "function foo() { const x = 42; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        let mut variable_types = HashMap::new();
        variable_types.insert("x".to_owned(), "i64".to_owned());
        let cache = CachedCompileInfo {
            variable_types,
            function_signatures: HashMap::new(),
        };

        // "x" is at offset 23
        let info = find_hover_info(&module, 23, Some(&cache));
        assert!(info.is_some(), "should find hover with cached type");
        let text = info.unwrap();
        assert!(
            text.contains("const x: i64"),
            "should show inferred type from cache: {text}"
        );
    }

    #[test]
    fn test_server_hover_function_call_with_cached_signature() {
        let source = "function foo() { bar(); }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        let mut function_signatures = HashMap::new();
        function_signatures.insert("bar".to_owned(), "function bar(): void".to_owned());
        let cache = CachedCompileInfo {
            variable_types: HashMap::new(),
            function_signatures,
        };

        // "bar" is at offset 17
        let info = find_hover_info(&module, 17, Some(&cache));
        assert!(info.is_some(), "should find hover for function call");
        let text = info.unwrap();
        assert!(
            text.contains("function bar(): void"),
            "should show cached signature: {text}"
        );
    }

    #[test]
    fn test_server_hover_no_rust_syntax_leaks() {
        // Verify that hover output never contains Rust-specific syntax
        let source = "function greet(name: string): string {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);

        let info = find_hover_info(&module, 9, None);
        assert!(info.is_some());
        let text = info.unwrap();

        // Must not contain Rust syntax
        assert!(
            !text.contains("fn "),
            "hover must not contain 'fn ': {text}"
        );
        assert!(
            !text.contains("-> "),
            "hover must not contain '-> ': {text}"
        );
        assert!(
            !text.contains("String"),
            "hover must not contain 'String' (should be 'string'): {text}"
        );
    }

    // -----------------------------------------------------------------------
    // Task 062: Phase 5 expression hover tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_server_hover_ternary_expression() {
        let source = "function foo(x: bool): i32 { const r = x ? 1 : 0; return r; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // The ternary expression `x ? 1 : 0` starts around offset 40-50.
        // Search for hover on the `?` or the ternary region.
        // The entire `x ? 1 : 0` is in the init expression of `const r`.
        // "r" is at offset 35, and the init expression is after "= ".
        // Let's try offset 44 which should be somewhere in the ternary.
        let info = find_hover_info(&module, 44, None);
        assert!(info.is_some(), "should find hover for ternary expression");
        // Could be on a sub-expression or on the ternary itself
    }

    #[test]
    fn test_server_hover_non_null_assert() {
        let source = "function foo(x: i32 | null): i32 { return x!; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "x!" starts at offset 42
        let info = find_hover_info(&module, 43, None);
        assert!(info.is_some(), "should find hover for non-null assert");
    }

    #[test]
    fn test_server_hover_as_cast() {
        let source = "function foo(x: i32): f64 { return x as f64; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "x as f64" starts around offset 35
        let info = find_hover_info(&module, 37, None);
        assert!(info.is_some(), "should find hover for as cast");
    }

    #[test]
    fn test_server_hover_typeof_expression() {
        let source = "function foo(x: i32): string { return typeof x; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "typeof x" starts around offset 38
        let info = find_hover_info(&module, 40, None);
        assert!(info.is_some(), "should find hover for typeof");
    }

    #[test]
    fn test_server_hover_exponentiation_operator() {
        let source = "function foo(a: i32, b: i32): i32 { return a ** b; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "a ** b" starts around offset 43. The `**` operator is at ~45.
        let info = find_hover_info(&module, 45, None);
        assert!(info.is_some(), "should find hover for ** operator");
        let text = info.unwrap();
        assert!(
            text.contains("**") || text.contains("pow"),
            "should mention ** or pow: {text}"
        );
    }

    #[test]
    fn test_server_hover_bitwise_operator() {
        let source = "function foo(a: i32, b: i32): i32 { return a & b; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "a & b" starts around offset 43. The `&` operator is at ~45.
        let info = find_hover_info(&module, 45, None);
        assert!(info.is_some(), "should find hover for bitwise operator");
        let text = info.unwrap();
        assert!(
            text.contains("&") || text.contains("Bitwise"),
            "should mention bitwise: {text}"
        );
    }

    #[test]
    fn test_server_hover_optional_param_shows_question_mark() {
        let source = "function foo(x?: string) {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "x" is at offset 13
        let info = find_hover_info(&module, 13, None);
        assert!(info.is_some(), "should find hover for optional param");
        let text = info.unwrap();
        assert!(text.contains("x?"), "should show optional marker: {text}");
    }

    #[test]
    fn test_server_hover_rest_param_shows_dots() {
        let source = "function foo(...args: Array<i32>) {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "args" starts around offset 16
        let info = find_hover_info(&module, 16, None);
        assert!(info.is_some(), "should find hover for rest param");
        let text = info.unwrap();
        assert!(text.contains("...args"), "should show rest prefix: {text}");
    }

    #[test]
    fn test_server_hover_default_param_shows_default() {
        let source = "function foo(x: i32 = 5) {}";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "x" is at offset 13
        let info = find_hover_info(&module, 13, None);
        assert!(info.is_some(), "should find hover for default param");
        let text = info.unwrap();
        assert!(text.contains("= ..."), "should show default marker: {text}");
    }

    #[test]
    fn test_server_hover_readonly_field() {
        let source = "class C { readonly x: i32 = 0; }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "x" is at offset 19
        let info = find_hover_info(&module, 19, None);
        assert!(info.is_some(), "should find hover for readonly field");
        let text = info.unwrap();
        assert!(text.contains("readonly"), "should show readonly: {text}");
    }

    #[test]
    fn test_server_hover_static_method() {
        let source = "class C { static foo(): void {} }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "foo" starts at offset 17
        let info = find_hover_info(&module, 17, None);
        assert!(info.is_some(), "should find hover for static method");
        let text = info.unwrap();
        assert!(text.contains("static"), "should show static: {text}");
    }

    #[test]
    fn test_server_hover_getter_shows_get() {
        let source = "class C { x: i32 = 0; get value(): i32 { return this.x; } }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "value" in "get value()" is at offset 26
        let info = find_hover_info(&module, 26, None);
        assert!(info.is_some(), "should find hover for getter");
        let text = info.unwrap();
        assert!(text.contains("get"), "should show getter: {text}");
    }

    #[test]
    fn test_server_hover_setter_shows_set() {
        let source = "class C { x: i32 = 0; set value(v: i32) { this.x = v; } }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "value" in "set value()" is at offset 26
        let info = find_hover_info(&module, 26, None);
        assert!(info.is_some(), "should find hover for setter");
        let text = info.unwrap();
        assert!(text.contains("set"), "should show setter: {text}");
    }

    #[test]
    fn test_server_hover_spread_arg() {
        // Spread inside a function call (SpreadArg variant), not array literal
        let source = "function bar(...args: Array<i32>) {} function foo() { bar(...items); }";
        let file_id = rsc_syntax::source::FileId(0);
        let (module, _) = rsc_parser::parse(source, file_id);
        // "...items" is in foo's body. "bar(...items);" starts after `{ `.
        // The SpreadArg expression `...items` is inside a call.
        // We need an offset inside the spread expression.
        // "foo() { bar(...items); }" — "bar" starts at ~53, "..." at 57, "items" at 60
        let info = find_hover_info(&module, 60, None);
        // The `items` identifier inside the spread should produce hover
        // (or the spread itself if the offset is between ... and the ident)
        assert!(
            info.is_some(),
            "should find hover for identifier within spread arg"
        );
    }
}
