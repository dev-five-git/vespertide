//! LSP backend.
//!
//! Holds the [`Client`] handle, a shared [`DocumentStore`], and a
//! [`WorkspaceIndex`]; implements [`LanguageServer`] from tower-lsp-server.
//!
//! Wave 1 handled only the lifecycle requests (`initialize`, `initialized`,
//! `shutdown`). Wave 2 (T2 + T3) introduced the document data layer and
//! cross-file index. Wave 3 wires diagnostics publication on open/change/close.
//!
//! Note: tower-lsp-server re-exports the upstream `lsp-types` crate under
//! the name `ls_types` (NOT `lsp_types`). Using `lsp_types::` directly
//! would fail to resolve.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    CodeActionKind as LspCodeActionKind, CodeActionOptions, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CompletionOptions, CompletionParams,
    CompletionResponse, Diagnostic, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentFormattingParams, DocumentHighlight, DocumentHighlightParams, DocumentSymbolParams,
    DocumentSymbolResponse, FoldingRange, FoldingRangeParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, InlayHint, InlayHintOptions, InlayHintParams,
    InlayHintServerCapabilities, Location, MessageType, OneOf, Position, PrepareRenameResponse,
    Range, ReferenceParams, RenameOptions, RenameParams, SelectionRange, SelectionRangeParams,
    SelectionRangeProviderCapability, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Uri, WorkDoneProgressOptions,
    WorkspaceEdit, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};
use tower_lsp_server::{Client, LanguageServer};

use crate::diagnostics::{self, mapper};
use crate::drift::DriftCache;
use crate::parser::DocumentFormat;
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

/// Vespertide language server backend.
///
/// Owns the [`Client`] handle used to push notifications (log messages,
/// diagnostics) back to the editor, plus a shared [`DocumentStore`] that
/// holds parsed state for every open document and a [`WorkspaceIndex`]
/// mapping table names to URIs.
#[derive(Debug)]
pub struct Backend {
    /// LSP client handle for sending notifications to the editor.
    pub client: Client,
    /// Shared document store; mutated by the notification handlers.
    pub store: Arc<DocumentStore>,
    /// Cross-file table-name → URI index; kept in sync with `store`.
    pub index: Arc<WorkspaceIndex>,
    /// Disk-discovered model tables loaded from the workspace root.
    pub workspace_tables: Arc<WorkspaceTables>,
    /// Drift loader cache reused across did_change-triggered refreshes.
    pub drift_cache: Arc<DriftCache>,
    /// Memoized result of [`Self::collect_workspace_tables`], keyed on
    /// `(docstore_fingerprint, workspace_tables_generation)`. A single
    /// `did_change` fans out diagnostics to every open document
    /// (`publish` + `publish_related`), so without this each keystroke would
    /// re-deserialize + re-normalize + re-clone EVERY open model once per open
    /// document — O(N²) per keystroke. The fan-out shares one docstore state,
    /// so all but the first call hit this cache, collapsing it to O(N).
    workspace_tables_cache: Mutex<Option<WorkspaceTablesCacheEntry>>,
}

/// `(docstore_fingerprint, disk_generation, value)` — see
/// [`Backend::workspace_tables_cache`].
type WorkspaceTablesCacheEntry = (u64, u64, Arc<Vec<diagnostics::WorkspaceTable>>);

impl Backend {
    /// Construct a new backend bound to the given LSP [`Client`].
    ///
    /// Designed to be passed directly to `LspService::new(Backend::new)`.
    #[must_use]
    #[cfg(not(tarpaulin_include))]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            store: Arc::new(DocumentStore::new()),
            index: Arc::new(WorkspaceIndex::new()),
            workspace_tables: Arc::new(WorkspaceTables::new()),
            drift_cache: Arc::new(DriftCache::new()),
            workspace_tables_cache: Mutex::new(None),
        }
    }

    /// Reindex a document after open/change. No-op if the document was just
    /// closed or never parsed (tree is `None`).
    fn reindex(&self, uri: &Uri) {
        self.store.with_doc(uri, |text, tree| {
            if let Some(tree) = tree {
                self.index.upsert(uri, text, tree);
            }
        });
    }

    /// Compute and publish diagnostics for a document.
    ///
    /// V1 publishes immediately on full-sync events. A 100ms debounce can be
    /// added later if clients report noisy updates during rapid editing.
    async fn publish(&self, uri: Uri) {
        let diagnostics = self.compute_lsp_diagnostics(&uri);
        let counts = diagnostic_severity_counts(&diagnostics);
        log_publishing_diagnostics(&uri, diagnostics.len(), counts.errors, counts.warnings);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn publish_related(&self, changed_uri: &Uri) {
        let other_uris: Vec<Uri> = self
            .store
            .open_uris()
            .into_iter()
            .filter(|uri| uri != changed_uri)
            .collect();

        for uri in other_uris {
            self.publish(uri).await;
        }
    }

    fn collect_workspace_tables(&self) -> Arc<Vec<diagnostics::WorkspaceTable>> {
        // Sample the cache key: open-document fingerprint + disk-table
        // generation. `compute_lsp_diagnostics` later reads the current
        // document text SEPARATELY, so to avoid serving a workspace snapshot
        // that is inconsistent with that read under concurrent handlers, every
        // cache decision below is gated on the state being UNCHANGED across the
        // operation (`workspace_state_unchanged`). This keeps the cache's
        // consistency window equal to the pre-cache two-read window while still
        // collapsing the per-`did_change` `publish` + `publish_related` fan-out
        // (which shares one docstore state) from O(N²) to O(N).
        let disk_generation = self.workspace_tables.generation();
        let fingerprint = crate::cache::docstore_fingerprint(self.store.as_ref());

        let cached = {
            let cache = self.workspace_tables_cache.lock().expect(
                "workspace_tables_cache lock poisoned — invariant: no panic while holding lock",
            );
            match cache.as_ref() {
                Some((fp, generation, value))
                    if *fp == fingerprint && *generation == disk_generation =>
                {
                    Some(Arc::clone(value))
                }
                _ => None,
            }
        };
        if let Some(value) = cached
            && self.workspace_state_unchanged(fingerprint, disk_generation)
        {
            // Re-validated: no concurrent handler moved the workspace between
            // sampling the key and now, so this snapshot is still current.
            return value;
        }

        let built = Arc::new(self.build_workspace_tables());
        // Only memoize when the sampled state is still current. If a concurrent
        // mutation raced the build, caching it could later serve a stale
        // snapshot, so skip the store and let the next call rebuild.
        if self.workspace_state_unchanged(fingerprint, disk_generation) {
            *self.workspace_tables_cache.lock().expect(
                "workspace_tables_cache lock poisoned — invariant: no panic while holding lock",
            ) = Some((fingerprint, disk_generation, Arc::clone(&built)));
        }
        built
    }

    /// `true` when the open-document fingerprint and disk-table generation still
    /// match the sampled `(fingerprint, disk_generation)` — i.e. no concurrent
    /// handler changed the workspace since the caller sampled the cache key.
    /// `generation` is read first (cheap atomic) so a disk refresh short-circuits
    /// before recomputing the document fingerprint.
    fn workspace_state_unchanged(&self, fingerprint: u64, disk_generation: u64) -> bool {
        self.workspace_tables.generation() == disk_generation
            && crate::cache::docstore_fingerprint(self.store.as_ref()) == fingerprint
    }

    fn build_workspace_tables(&self) -> Vec<diagnostics::WorkspaceTable> {
        let mut workspace = Vec::new();
        // Dedup by NORMALIZED FILESYSTEM PATH so a file that is both open
        // in the editor and present on disk is registered only once.
        //
        // URI-level dedup is not enough: Zed and our own `path_to_uri`
        // helper can emit slightly different strings (drive-letter case
        // on Windows, %20 vs space, trailing slashes) for the same file.
        // Two registrations of the same file would make the planner report
        // a spurious `DuplicateTableName`.
        let mut seen_paths: BTreeSet<PathBuf> = BTreeSet::new();

        self.store.for_each(|uri, state| {
            let Some(entry) = open_workspace_table(uri, state) else {
                return;
            };

            if let Some(path) = crate::position::uri_to_path(uri) {
                seen_paths.insert(normalize_path(&path));
            }
            workspace.push(entry);
        });

        for (name, table) in self.workspace_tables.all() {
            if let Some((disk_path, entry)) =
                disk_workspace_table(&name, table, self.workspace_tables.model_path(&name))
            {
                if !seen_paths.insert(normalize_path(&disk_path)) {
                    // Same physical file is already in the workspace as an open document.
                    continue;
                }

                workspace.push(entry);
            }
        }

        workspace
    }

    fn fallback_disk_uri(table_name: &str) -> Uri {
        Uri::from_str(&format!("file:///__disk__/{table_name}.json"))
            .expect("synthetic disk model URI should parse")
    }

    fn compute_lsp_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic> {
        let Some(format) = DocumentFormat::from_uri(uri) else {
            return Vec::new();
        };

        let workspace = self.collect_workspace_tables();

        let mut diagnostics: Vec<Diagnostic> = self
            .store
            .docs_iter_for_uri(uri, |state| {
                let domain = diagnostics::compute_workspace(
                    state.text(),
                    format,
                    state.tree.as_ref(),
                    &workspace,
                    uri,
                );
                domain
                    .iter()
                    .map(|diag| mapper::to_lsp(diag, &state.doc))
                    .collect()
            })
            .unwrap_or_default();

        if let Some(root) = Self::workspace_root_for(uri) {
            let drifts: Vec<_> = crate::drift::compute_with_cache(
                &root,
                self.index.as_ref(),
                self.store.as_ref(),
                self.drift_cache.as_ref(),
            )
            .into_iter()
            .filter(|d| d.uri == *uri)
            .filter_map(crate::drift::DomainDrift::into_domain_diagnostic)
            .collect();
            if !drifts.is_empty() {
                let lsp_drifts = self
                    .store
                    .docs_iter_for_uri(uri, |state| {
                        drifts
                            .iter()
                            .map(|d| mapper::to_lsp(d, &state.doc))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                diagnostics.extend(lsp_drifts);
            }
        }

        diagnostics
    }

    fn workspace_root_for(uri: &Uri) -> Option<PathBuf> {
        let path = crate::position::uri_to_path(uri)?;
        let mut current = path.parent();
        while let Some(dir) = current {
            if dir.join("vespertide.json").exists() {
                return Some(dir.to_path_buf());
            }
            current = dir.parent();
        }
        None
    }

    fn refresh_workspace_tables_for_uri(&self, uri: &Uri) {
        if let Some(root) = Self::workspace_root_for(uri) {
            self.workspace_tables.refresh(&root);
        }
    }

    fn refresh_workspace_tables_from_initialize(&self, params: &InitializeParams) {
        if let Some(root_uri) = initialize_root_uri(params) {
            if let Some(root) = crate::position::uri_to_path(root_uri) {
                self.workspace_tables.refresh(&root);
            }
            return;
        }

        let Some(folders) = params.workspace_folders.as_ref() else {
            return;
        };
        for folder in folders {
            if let Some(root) = crate::position::uri_to_path(&folder.uri)
                && self.workspace_tables.refresh(&root)
            {
                break;
            }
        }
    }
}

#[cfg(not(tarpaulin_include))]
fn log_publishing_diagnostics(uri: &Uri, total: usize, errors: usize, warnings: usize) {
    tracing::info!(
        target: "vespertide_lsp::diagnostics",
        uri = %uri.as_str(),
        total,
        errors,
        warnings,
        "publishing diagnostics"
    );
}

#[expect(
    deprecated,
    reason = "initialize preserves deprecated LSP rootUri fallback when older clients omit workspaceFolders"
)]
fn initialize_root_uri(params: &InitializeParams) -> Option<&Uri> {
    // `root_uri` is deprecated in newer LSP versions, but several editors still
    // send it without `workspace_folders`. Keep it as the first fallback for
    // workspace discovery while isolating the compatibility warning here.
    params.root_uri.as_ref()
}

fn open_workspace_table(
    uri: &Uri,
    state: &crate::document::DocumentState,
) -> Option<diagnostics::WorkspaceTable> {
    let text = state.text();
    let tree = state.tree.clone()?;
    let parsed = match state.format {
        DocumentFormat::Json => serde_json::from_str::<vespertide_core::TableDef>(text).ok(),
        DocumentFormat::Yaml => serde_yaml::from_str::<vespertide_core::TableDef>(text).ok(),
    }?;
    let table = parsed.normalize().ok()?;

    Some(diagnostics::WorkspaceTable {
        uri: uri.clone(),
        table,
        source: text.to_string(),
        tree: Some(tree),
    })
}

fn disk_workspace_table(
    name: &str,
    table: vespertide_core::TableDef,
    disk_path: Option<PathBuf>,
) -> Option<(PathBuf, diagnostics::WorkspaceTable)> {
    let disk_path = disk_path?;
    let disk_uri = crate::position::path_to_uri(&disk_path)
        .unwrap_or_else(|| Backend::fallback_disk_uri(name));
    let entry = diagnostics::WorkspaceTable {
        uri: disk_uri,
        table,
        source: String::new(),
        tree: None,
    };
    Some((disk_path, entry))
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root = params
            .workspace_folders
            .as_ref()
            .and_then(|f| f.first().map(|folder| folder.uri.as_str().to_string()))
            .or_else(|| initialize_root_uri(&params).map(|uri| uri.as_str().to_string()));
        let root_for_log = root.as_deref().unwrap_or("<none>");
        let client_name = params.client_info.as_ref().map(|c| c.name.as_str());
        tracing::info!(
            target: "vespertide_lsp::handler",
            root = root_for_log,
            client = ?client_name,
            "initialize"
        );

        self.refresh_workspace_tables_from_initialize(&params);
        let discovered = self.workspace_tables.names();
        let disk_table_count = discovered.len();
        tracing::info!(
            target: "vespertide_lsp::handler",
            disk_table_count,
            disk_tables = ?discovered,
            "workspace tables discovered"
        );

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        will_save: None,
                        will_save_wait_until: None,
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![LspCodeActionKind::REFACTOR]),
                        resolve_provider: Some(false),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    },
                )),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                inlay_hint_provider: Some(tower_lsp_server::ls_types::OneOf::Right(
                    InlayHintServerCapabilities::Options(InlayHintOptions {
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                        resolve_provider: Some(false),
                    }),
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: crate::semantic_tokens::legend(),
                            range: Some(true),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                document_symbol_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(
                    tower_lsp_server::ls_types::FoldingRangeProviderCapability::Simple(true),
                ),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    // `"` triggers key/value strings, `:` value position,
                    // `,` opens a new pair, `{` and `[` open new objects.
                    trigger_characters: Some(vec![
                        "\"".to_string(),
                        ":".to_string(),
                        ",".to_string(),
                        "{".to_string(),
                        "[".to_string(),
                    ]),
                    ..CompletionOptions::default()
                }),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "vespertide-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            // tower-lsp-server 0.23 exposes an explicit offset_encoding field;
            // leaving it `None` keeps the default (UTF-16) negotiated by the client.
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        tracing::info!(target: "vespertide_lsp::handler", "initialized");
        let log_path = std::env::var_os("VESPERTIDE_LSP_LOG").map_or_else(
            || std::env::temp_dir().join("vespertide-lsp.log"),
            std::path::PathBuf::from,
        );
        let message = format!(
            "vespertide-lsp v{} initialized. File log: {}",
            env!("CARGO_PKG_VERSION"),
            log_path.display()
        );
        self.client.log_message(MessageType::INFO, &message).await;

        // Ask the client to watch model + migration files. Clients that
        // don't support dynamic registration (older Zed builds, basic
        // LSP clients) simply ignore this — they'll still work via the
        // editor's own save / change notifications.
        let registration = crate::watched_files::build_registration();
        if let Err(err) = self.client.register_capability(vec![registration]).await {
            tracing::warn!(
                target: "vespertide_lsp::handler",
                error = %err,
                "client refused workspace/didChangeWatchedFiles registration; relying on editor save events"
            );
        }
    }

    #[cfg(not(tarpaulin_include))]
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        handler_navigation::completion_impl(self, params).await
    }

    #[cfg(not(tarpaulin_include))]
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        handler_navigation::hover_impl(self, params).await
    }

    #[cfg(not(tarpaulin_include))]
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        handler_navigation::goto_definition_impl(self, params).await
    }

    #[cfg(not(tarpaulin_include))]
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        handler_navigation::references_impl(self, params).await
    }

    #[cfg(not(tarpaulin_include))]
    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        handler_navigation::code_action_impl(self, params).await
    }

    #[cfg(not(tarpaulin_include))]
    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        handler_navigation::inlay_hint_impl(self, params).await
    }

    #[cfg(not(tarpaulin_include))]
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        handler_navigation::symbol_impl(self, params).await
    }

    #[cfg(not(tarpaulin_include))]
    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        handler_rename::prepare_rename_impl(self, params).await
    }

    #[cfg(not(tarpaulin_include))]
    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        handler_rename::rename_impl(self, params).await
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        Ok(crate::semantic_tokens::handler::compute_full(
            self.store.as_ref(),
            &params,
        ))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        Ok(crate::semantic_tokens::handler::compute_range(
            self.store.as_ref(),
            &params,
        ))
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let Some(root) = self.workspace_tables.root() else {
            return;
        };
        let models_dir = root.join("models");
        let migrations_dir = root.join("migrations");

        let mut touched = false;
        for event in &params.changes {
            let Some(path) = crate::position::uri_to_path(&event.uri) else {
                continue;
            };
            if crate::watched_files::should_refresh_for(&root, &models_dir, &migrations_dir, &path)
            {
                touched = true;
                break;
            }
        }
        if !touched {
            return;
        }

        self.workspace_tables.refresh(&root);
        let change_count = params.changes.len();
        tracing::info!(
            target: "vespertide_lsp::handler",
            changes = change_count,
            "did_change_watched_files: refreshed workspace_tables"
        );
        for uri in self.store.open_uris() {
            self.publish(uri).await;
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        handler_file_features::document_symbol_impl(self, params).await
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        handler_file_features::folding_range_impl(self, params).await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        handler_file_features::document_highlight_impl(self, params).await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        handler_file_features::selection_range_impl(self, params).await
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let Some(format) = DocumentFormat::from_uri(uri) else {
            return Ok(None);
        };

        let result = self.store.docs_iter_for_uri(uri, |state| {
            let original = state.text();
            let formatted = crate::formatting::format_text(original, format)?;
            if formatted == original {
                return Some(Vec::new());
            }

            let end = crate::position::byte_to_lsp_position(&state.doc, original.len());
            Some(vec![TextEdit {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: end.line,
                        character: end.character,
                    },
                },
                new_text: formatted,
            }])
        });

        Ok(result.flatten())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let td = params.text_document;
        let uri = td.uri.clone();
        let uri_for_log = uri.as_str();
        let language_for_log = td.language_id.as_str();
        let version_for_log = td.version;
        let bytes_for_log = td.text.len();
        tracing::info!(
            target: "vespertide_lsp::handler",
            uri = %uri_for_log,
            language = %language_for_log,
            version = version_for_log,
            bytes = bytes_for_log,
            "did_open"
        );
        self.store
            .open(uri.clone(), td.language_id, td.version, td.text);
        self.reindex(&uri);
        self.refresh_workspace_tables_for_uri(&uri);
        self.publish(uri.clone()).await;
        self.publish_related(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let td = params.text_document;
        // V1 = FULL sync: changes[0].text is the entire new content.
        if let Some(change) = params.content_changes.into_iter().next() {
            let uri = td.uri;
            let uri_for_log = uri.as_str();
            let version_for_log = td.version;
            let bytes_for_log = change.text.len();
            tracing::debug!(
                target: "vespertide_lsp::handler",
                uri = %uri_for_log,
                version = version_for_log,
                bytes = bytes_for_log,
                "did_change"
            );
            self.store.update_full(&uri, change.text, td.version);
            self.reindex(&uri);
            self.refresh_workspace_tables_for_uri(&uri);
            self.publish(uri.clone()).await;
            self.publish_related(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        let uri_for_log = uri.as_str();
        tracing::info!(
            target: "vespertide_lsp::handler",
            uri = %uri_for_log,
            "did_save"
        );
        self.refresh_workspace_tables_for_uri(&uri);
        self.publish(uri.clone()).await;
        self.publish_related(&uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let uri_for_log = uri.as_str();
        tracing::info!(
            target: "vespertide_lsp::handler",
            uri = %uri_for_log,
            "did_close"
        );
        self.store.close(&uri);
        self.index.remove(&uri);
        self.refresh_workspace_tables_for_uri(&uri);
        self.client
            .publish_diagnostics(uri.clone(), Vec::new(), None)
            .await;
        self.publish_related(&uri).await;
    }
}

mod handler_file_features;
mod handler_navigation;
mod handler_rename;
mod helpers;
use helpers::{diagnostic_severity_counts, normalize_path};

#[cfg(test)]
mod tests;
