//! The `LanguageServer` implementation: document store, scheduling of the
//! semantic pass, workspace indexing and dispatch to [`crate::features`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use serde_json::Value;
use tower_lsp_server::jsonrpc::{Error, Result};
use tower_lsp_server::ls_types::notification::Notification;
#[allow(clippy::wildcard_imports)] // The handler signatures use most of the protocol types.
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer};

use crate::config::Config;
use crate::document::Document;
use crate::features::{
    code_action, code_lens, commands, completion, diagnostics, folding, format, hover, inlay, navigation, rename, selection,
    semantic_tokens, signature, symbols,
};
use crate::position::Encoding;
use crate::{semantic, workspace};

/// Client capabilities the server adapts to.
#[derive(Debug, Clone, Copy, Default)]
struct ClientCaps {
    pull_diagnostics: bool,
    diagnostic_refresh: bool,
    configuration: bool,
    watched_files_dynamic: bool,
    work_done_progress: bool,
    inlay_hint_refresh: bool,
}

/// Shared server state. Locks are never held across `.await`.
#[derive(Debug, Default)]
struct State {
    /// Open documents.
    open: RwLock<HashMap<Uri, Arc<Document>>>,
    /// Files indexed from disk that are not open.
    indexed: RwLock<HashMap<Uri, Arc<Document>>>,
    config: RwLock<Config>,
    encoding: RwLock<Encoding>,
    caps: RwLock<ClientCaps>,
    roots: RwLock<Vec<PathBuf>>,
    /// Latest scheduled semantic pass per document (debounce generation).
    generations: RwLock<HashMap<Uri, u64>>,
    next_generation: AtomicU64,
    /// Last semantic tokens sent per document, for deltas.
    tokens: RwLock<HashMap<Uri, (String, Vec<SemanticToken>)>>,
    next_result_id: AtomicU64,
}

fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

/// The LP language server.
#[derive(Debug, Clone)]
pub struct Backend {
    client: Client,
    state: Arc<State>,
}

/// Documents up to this size get syntax diagnostics on every keystroke; larger
/// ones only once the edit debounce settles.
const QUICK_DIAGNOSTICS_BYTES: usize = 1024 * 1024;

/// Why a semantic pass was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Edit,
    Save,
}

impl Backend {
    /// Create a backend bound to `client`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client, state: Arc::new(State::default()) }
    }

    fn config(&self) -> Config {
        read(&self.state.config).clone()
    }

    fn encoding(&self) -> Encoding {
        *read(&self.state.encoding)
    }

    fn caps(&self) -> ClientCaps {
        *read(&self.state.caps)
    }

    /// Open document, else the indexed copy from disk.
    fn document(&self, uri: &Uri) -> Option<Arc<Document>> {
        read(&self.state.open).get(uri).cloned().or_else(|| read(&self.state.indexed).get(uri).cloned())
    }

    fn document_or_error(&self, uri: &Uri) -> Result<Arc<Document>> {
        self.document(uri).ok_or_else(|| Error::invalid_params(format!("unknown document {}", uri.as_str())))
    }

    /// Every known document: open ones plus indexed files that are not open.
    fn all_documents(&self) -> Vec<Arc<Document>> {
        let open = read(&self.state.open);
        let mut docs: Vec<Arc<Document>> = open.values().cloned().collect();
        docs.extend(read(&self.state.indexed).iter().filter(|(uri, _)| !open.contains_key(*uri)).map(|(_, doc)| doc.clone()));
        docs
    }

    /// Run feature code on the blocking pool so large documents never stall
    /// the async workers (the first request after an edit may build the index).
    async fn run<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
        tokio::task::spawn_blocking(f).await.map_err(|e| Error {
            code: tower_lsp_server::jsonrpc::ErrorCode::InternalError,
            message: format!("request worker failed: {e}").into(),
            data: None,
        })
    }

    async fn log(&self, typ: MessageType, message: impl std::fmt::Display + Send) {
        self.client.log_message(typ, message).await;
    }

    /// Push diagnostics (push mode) or ask the client to pull (pull mode).
    async fn publish(&self, uri: &Uri) {
        let caps = self.caps();
        if caps.pull_diagnostics {
            if caps.diagnostic_refresh
                && let Err(e) = self.client.workspace_diagnostic_refresh().await
            {
                self.log(MessageType::WARNING, format!("diagnostic refresh failed: {e}")).await;
            }
            return;
        }
        let Some(doc) = read(&self.state.open).get(uri).cloned() else { return };
        let config = self.config();
        let version = doc.version;
        match self.run(move || diagnostics::compute(&doc, &config)).await {
            Ok(diagnostics) => self.client.publish_diagnostics(uri.clone(), diagnostics, Some(version)).await,
            Err(e) => self.log(MessageType::ERROR, format!("diagnostics failed for {}: {}", uri.as_str(), e.message)).await,
        }
    }

    /// Schedule the semantic pass for `uri`: debounced after edits, immediate
    /// on save. Large files only run on save. Stale results are discarded.
    fn schedule_semantic(&self, uri: Uri, trigger: Trigger) {
        let generation = self.state.next_generation.fetch_add(1, Ordering::Relaxed);
        write(&self.state.generations).insert(uri.clone(), generation);
        let this = self.clone();
        tokio::spawn(async move {
            let config = this.config();
            if trigger == Trigger::Edit {
                tokio::time::sleep(Duration::from_millis(config.semantic.debounce_ms)).await;
            }
            if read(&this.state.generations).get(&uri) != Some(&generation) {
                return;
            }
            let Some(doc) = read(&this.state.open).get(&uri).cloned() else { return };
            if trigger == Trigger::Edit && doc.text.len() > QUICK_DIAGNOSTICS_BYTES {
                // Large files skip per-keystroke diagnostics; publish once typing pauses.
                this.publish(&uri).await;
            }
            if trigger == Trigger::Edit && doc.text.len() > config.max_semantic_bytes() {
                return;
            }
            let version = doc.version;
            let analysis = config.analysis.to_upstream();
            let result = tokio::task::spawn_blocking(move || semantic::run(&doc.text, version, &analysis)).await;
            let result = match result {
                Ok(result) => Arc::new(result),
                Err(e) => {
                    this.log(MessageType::ERROR, format!("semantic pass failed for {}: {e}", uri.as_str())).await;
                    return;
                }
            };
            {
                let mut open = write(&this.state.open);
                let Some(doc) = open.get_mut(&uri) else { return };
                if doc.version != version {
                    return; // A newer edit arrived; its own pass will publish.
                }
                Arc::make_mut(doc).semantic_result = Some(result);
            }
            this.publish(&uri).await;
            this.refresh_views().await;
        });
    }

    /// Ask the client to refresh views that depend on the semantic pass.
    async fn refresh_views(&self) {
        if !self.caps().inlay_hint_refresh {
            return;
        }
        if let Err(e) = self.client.inlay_hint_refresh().await {
            self.log(MessageType::LOG, format!("inlay hint refresh unsupported: {e}")).await;
        }
    }

    async fn load_config(&self) {
        if !self.caps().configuration {
            return;
        }
        let items = vec![ConfigurationItem { scope_uri: None, section: Some("lp".to_owned()) }];
        match self.client.configuration(items).await {
            Ok(mut values) => self.apply_config(values.pop().unwrap_or(Value::Null)).await,
            Err(e) => self.log(MessageType::WARNING, format!("cannot read configuration: {e}")).await,
        }
    }

    async fn apply_config(&self, value: Value) {
        match Config::from_value(value) {
            Ok(config) => *write(&self.state.config) = config,
            Err(message) => {
                self.log(MessageType::ERROR, &message).await;
                self.client.show_message(MessageType::ERROR, message).await;
            }
        }
    }

    /// Index every `*.lp` file under the workspace roots, reporting progress.
    async fn index_workspace(&self) {
        let roots = read(&self.state.roots).clone();
        if roots.is_empty() {
            return;
        }
        let encoding = self.encoding();
        let (files, errors) = match tokio::task::spawn_blocking(move || workspace::find_lp_files(&roots)).await {
            Ok(found) => found,
            Err(e) => {
                self.log(MessageType::ERROR, format!("workspace scan failed: {e}")).await;
                return;
            }
        };
        for error in errors {
            self.log(MessageType::WARNING, error).await;
        }
        let token = ProgressToken::String("lp-lsp/index".to_owned());
        let progress = if self.caps().work_done_progress && self.client.create_work_done_progress(token.clone()).await.is_ok() {
            Some(self.client.progress(token, "Indexing LP files").with_percentage(0).begin().await)
        } else {
            None
        };
        let total = files.len().max(1);
        for (i, path) in files.into_iter().enumerate() {
            let loaded = tokio::task::spawn_blocking(move || workspace::load(&path, encoding)).await;
            match loaded {
                Ok(Ok(doc)) => {
                    write(&self.state.indexed).insert(doc.uri.clone(), Arc::new(doc));
                }
                Ok(Err(message)) => self.log(MessageType::WARNING, message).await,
                Err(e) => self.log(MessageType::ERROR, format!("indexing task failed: {e}")).await,
            }
            if let Some(progress) = &progress {
                let percentage = u32::try_from((i + 1) * 100 / total).unwrap_or(100);
                progress.report(percentage).await;
            }
        }
        if let Some(progress) = progress {
            progress.finish().await;
        }
    }

    async fn register_watchers(&self) {
        if !self.caps().watched_files_dynamic {
            return;
        }
        let options = DidChangeWatchedFilesRegistrationOptions {
            watchers: vec![FileSystemWatcher { glob_pattern: GlobPattern::String("**/*.lp".to_owned()), kind: None }],
        };
        let registration = Registration {
            id: "lp-lsp/watch".to_owned(),
            method: notification::DidChangeWatchedFiles::METHOD.to_owned(),
            register_options: serde_json::to_value(options).ok(),
        };
        if let Err(e) = self.client.register_capability(vec![registration]).await {
            self.log(MessageType::WARNING, format!("cannot watch *.lp files: {e}")).await;
        }
    }

    fn position_params(&self, params: &TextDocumentPositionParams) -> Result<(Arc<Document>, Position)> {
        Ok((self.document_or_error(&params.text_document.uri)?, params.position))
    }

    fn cache_tokens(&self, uri: &Uri, tokens: Vec<SemanticToken>) -> String {
        let id = self.state.next_result_id.fetch_add(1, Ordering::Relaxed).to_string();
        write(&self.state.tokens).insert(uri.clone(), (id.clone(), tokens));
        id
    }
}

fn capabilities(pull_diagnostics: bool, encoding: Encoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding.kind()),
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::INCREMENTAL),
            save: Some(TextDocumentSyncSaveOptions::Supported(true)),
            ..Default::default()
        })),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(true),
            trigger_characters: Some([":", "=", " ", "("].map(str::to_owned).to_vec()),
            ..Default::default()
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
            retrigger_characters: None,
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        definition_provider: Some(OneOf::Left(true)),
        declaration_provider: Some(DeclarationCapability::Simple(true)),
        type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![
                CodeActionKind::QUICKFIX,
                CodeActionKind::REFACTOR,
                CodeActionKind::REFACTOR_REWRITE,
                CodeActionKind::new(code_action::ORGANIZE_SECTIONS),
            ]),
            ..Default::default()
        })),
        code_lens_provider: Some(CodeLensOptions { resolve_provider: Some(false) }),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: "\n".to_owned(),
            more_trigger_character: None,
        }),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
            legend: semantic_tokens::legend(),
            range: Some(true),
            full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        inlay_hint_provider: Some(OneOf::Left(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: commands::ALL.iter().map(|c| (*c).to_owned()).collect(),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        diagnostic_provider: pull_diagnostics.then(|| {
            DiagnosticServerCapabilities::Options(DiagnosticOptions {
                identifier: Some("lp".to_owned()),
                inter_file_dependencies: false,
                workspace_diagnostics: false,
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })
        }),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: None,
        }),
        ..Default::default()
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let caps = &params.capabilities;
        let encoding = Encoding::negotiate(caps.general.as_ref().and_then(|g| g.position_encodings.as_deref()));
        let text = caps.text_document.as_ref();
        let workspace = caps.workspace.as_ref();
        let client_caps = ClientCaps {
            pull_diagnostics: text.is_some_and(|t| t.diagnostic.is_some()),
            diagnostic_refresh: workspace.and_then(|w| w.diagnostics.as_ref()).and_then(|d| d.refresh_support).unwrap_or(false),
            configuration: workspace.and_then(|w| w.configuration).unwrap_or(false),
            watched_files_dynamic: workspace
                .and_then(|w| w.did_change_watched_files.as_ref())
                .and_then(|d| d.dynamic_registration)
                .unwrap_or(false),
            work_done_progress: caps.window.as_ref().and_then(|w| w.work_done_progress).unwrap_or(false),
            inlay_hint_refresh: workspace.and_then(|w| w.inlay_hint.as_ref()).and_then(|i| i.refresh_support).unwrap_or(false),
        };
        *write(&self.state.encoding) = encoding;
        *write(&self.state.caps) = client_caps;

        let mut roots: Vec<PathBuf> = params
            .workspace_folders
            .iter()
            .flatten()
            .filter_map(|folder| folder.uri.to_file_path().map(std::borrow::Cow::into_owned))
            .collect();
        #[allow(deprecated)]
        if roots.is_empty()
            && let Some(root) = params.root_uri.as_ref().and_then(Uri::to_file_path)
        {
            roots.push(root.into_owned());
        }
        *write(&self.state.roots) = roots;

        if let Some(options) = params.initialization_options {
            self.apply_config(options).await;
        }

        Ok(InitializeResult {
            capabilities: capabilities(client_caps.pull_diagnostics, encoding),
            server_info: Some(ServerInfo { name: "lp-lsp".to_owned(), version: Some(env!("CARGO_PKG_VERSION").to_owned()) }),
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.load_config().await;
        self.register_watchers().await;
        let this = self.clone();
        tokio::spawn(async move { this.index_workspace().await });
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let item = params.text_document;
        let uri = item.uri.clone();
        let encoding = self.encoding();
        // A full parse of a large file takes a while: let the runtime move other
        // tasks off this worker (no `.await`, so messages stay ordered).
        let doc = tokio::task::block_in_place(|| Document::new(item.uri, item.text, item.version, encoding));
        write(&self.state.open).insert(uri.clone(), Arc::new(doc));
        self.publish(&uri).await;
        self.schedule_semantic(uri, Trigger::Save);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        // Handlers run concurrently, so the edit must be applied before the first
        // `.await` to keep changes in order. `block_in_place` keeps the reparse
        // from stalling other tasks on this worker.
        let size = tokio::task::block_in_place(|| {
            let mut open = write(&self.state.open);
            open.get_mut(&uri).map(|doc| {
                let doc = Arc::make_mut(doc);
                doc.apply_changes(&params.content_changes, params.text_document.version);
                doc.text.len()
            })
        });
        let Some(size) = size else {
            self.log(MessageType::WARNING, format!("didChange for unopened document {}", uri.as_str())).await;
            return;
        };
        if size <= QUICK_DIAGNOSTICS_BYTES {
            self.publish(&uri).await;
        }
        self.schedule_semantic(uri, Trigger::Edit);
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.schedule_semantic(params.text_document.uri, Trigger::Save);
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let closed = write(&self.state.open).remove(&uri);
        write(&self.state.generations).remove(&uri);
        write(&self.state.tokens).remove(&uri);
        // Keep the closed file in the workspace index if it still exists on disk.
        if let Some(doc) = closed
            && uri.to_file_path().is_some_and(|p| p.exists())
        {
            write(&self.state.indexed).insert(uri.clone(), doc);
        }
        if !self.caps().pull_diagnostics {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        if self.caps().configuration {
            self.load_config().await;
        } else {
            let value = params.settings.get("lp").cloned().unwrap_or(params.settings);
            self.apply_config(value).await;
        }
        let uris: Vec<Uri> = read(&self.state.open).keys().cloned().collect();
        for uri in uris {
            self.schedule_semantic(uri, Trigger::Save);
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let encoding = self.encoding();
        for change in params.changes {
            if read(&self.state.open).contains_key(&change.uri) {
                continue;
            }
            let Some(path) = change.uri.to_file_path().map(std::borrow::Cow::into_owned) else { continue };
            if change.typ == FileChangeType::DELETED {
                write(&self.state.indexed).remove(&change.uri);
                continue;
            }
            match tokio::task::spawn_blocking(move || workspace::load(&path, encoding)).await {
                Ok(Ok(doc)) => {
                    write(&self.state.indexed).insert(change.uri, Arc::new(doc));
                }
                Ok(Err(message)) => self.log(MessageType::WARNING, message).await,
                Err(e) => self.log(MessageType::ERROR, format!("indexing task failed: {e}")).await,
            }
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        {
            let mut roots = write(&self.state.roots);
            for removed in &params.event.removed {
                if let Some(path) = removed.uri.to_file_path() {
                    roots.retain(|r| r != path.as_ref());
                }
            }
            roots.extend(params.event.added.iter().filter_map(|f| f.uri.to_file_path().map(std::borrow::Cow::into_owned)));
        }
        let this = self.clone();
        tokio::spawn(async move { this.index_workspace().await });
    }

    async fn diagnostic(&self, params: DocumentDiagnosticParams) -> Result<DocumentDiagnosticReportResult> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        let config = self.config();
        let items = self.run(move || diagnostics::compute(&doc, &config)).await?;
        Ok(DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport { result_id: None, items },
        })))
    }

    async fn document_symbol(&self, params: DocumentSymbolParams) -> Result<Option<DocumentSymbolResponse>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        self.run(move || Some(DocumentSymbolResponse::Nested(symbols::document_symbols(&doc)))).await
    }

    async fn symbol(&self, params: WorkspaceSymbolParams) -> Result<Option<WorkspaceSymbolResponse>> {
        let docs = self.all_documents();
        self.run(move || Some(WorkspaceSymbolResponse::Nested(symbols::workspace_symbols(&docs, &params.query)))).await
    }

    async fn goto_definition(&self, params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>> {
        let (doc, position) = self.position_params(&params.text_document_position_params)?;
        self.run(move || navigation::definition(&doc, position).map(GotoDefinitionResponse::Scalar)).await
    }

    async fn goto_declaration(&self, params: request::GotoDeclarationParams) -> Result<Option<request::GotoDeclarationResponse>> {
        let (doc, position) = self.position_params(&params.text_document_position_params)?;
        self.run(move || navigation::declaration(&doc, position).map(GotoDefinitionResponse::Scalar)).await
    }

    async fn goto_type_definition(&self, params: request::GotoTypeDefinitionParams) -> Result<Option<request::GotoTypeDefinitionResponse>> {
        let (doc, position) = self.position_params(&params.text_document_position_params)?;
        self.run(move || navigation::type_definition(&doc, position).map(GotoDefinitionResponse::Scalar)).await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let (doc, position) = self.position_params(&params.text_document_position)?;
        let include_declaration = params.context.include_declaration;
        self.run(move || Some(navigation::references(&doc, position, include_declaration))).await
    }

    async fn document_highlight(&self, params: DocumentHighlightParams) -> Result<Option<Vec<DocumentHighlight>>> {
        let (doc, position) = self.position_params(&params.text_document_position_params)?;
        self.run(move || Some(navigation::highlights(&doc, position))).await
    }

    async fn prepare_rename(&self, params: TextDocumentPositionParams) -> Result<Option<PrepareRenameResponse>> {
        let (doc, position) = self.position_params(&params)?;
        self.run(move || rename::prepare(&doc, position)).await?.map_err(Error::invalid_params)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let (doc, position) = self.position_params(&params.text_document_position)?;
        let others: Vec<Arc<Document>> = self.all_documents().into_iter().filter(|d| d.uri != doc.uri).collect();
        self.run(move || rename::rename(&doc, &others, position, &params.new_name)).await?.map_err(Error::invalid_params)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let (doc, position) = self.position_params(&params.text_document_position_params)?;
        self.run(move || hover::hover(&doc, position)).await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let (doc, position) = self.position_params(&params.text_document_position)?;
        self.run(move || Some(CompletionResponse::Array(completion::complete(&doc, position)))).await
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        Ok(completion::resolve(item))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let (doc, position) = self.position_params(&params.text_document_position_params)?;
        self.run(move || signature::help(&doc, position)).await
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        let settings = self.config().inlay_hints;
        self.run(move || Some(inlay::hints(&doc, params.range, &settings))).await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        let config = self.config();
        self.run(move || Some(code_action::actions(&doc, params.range, &params.context, &config))).await
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        self.run(move || Some(code_lens::lenses(&doc))).await
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        let settings = self.config().format;
        self.run(move || format::format_document(&doc, &settings)).await
    }

    async fn range_formatting(&self, params: DocumentRangeFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        let settings = self.config().format;
        self.run(move || format::format_range(&doc, params.range, &settings)).await
    }

    async fn on_type_formatting(&self, params: DocumentOnTypeFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let (doc, position) = self.position_params(&params.text_document_position)?;
        let settings = self.config().format;
        self.run(move || format::format_on_type(&doc, position, &params.ch, &settings)).await
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        self.run(move || Some(folding::ranges(&doc))).await
    }

    async fn selection_range(&self, params: SelectionRangeParams) -> Result<Option<Vec<SelectionRange>>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        self.run(move || Some(selection::ranges(&doc, &params.positions))).await
    }

    async fn semantic_tokens_full(&self, params: SemanticTokensParams) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let doc = self.document_or_error(&uri)?;
        let data = self.run(move || semantic_tokens::tokens(&doc, None)).await?;
        let result_id = self.cache_tokens(&uri, data.clone());
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens { result_id: Some(result_id), data })))
    }

    async fn semantic_tokens_full_delta(&self, params: SemanticTokensDeltaParams) -> Result<Option<SemanticTokensFullDeltaResult>> {
        let uri = params.text_document.uri;
        let doc = self.document_or_error(&uri)?;
        let previous =
            read(&self.state.tokens).get(&uri).filter(|(id, _)| *id == params.previous_result_id).map(|(_, tokens)| tokens.clone());
        let (data, edits) = self
            .run(move || {
                let data = semantic_tokens::tokens(&doc, None);
                let edits = previous.map(|old| semantic_tokens::delta(&old, &data));
                (data, edits)
            })
            .await?;
        let result_id = self.cache_tokens(&uri, data.clone());
        Ok(Some(match edits {
            Some(edits) => SemanticTokensFullDeltaResult::TokensDelta(SemanticTokensDelta { result_id: Some(result_id), edits }),
            None => SemanticTokensFullDeltaResult::Tokens(SemanticTokens { result_id: Some(result_id), data }),
        }))
    }

    async fn semantic_tokens_range(&self, params: SemanticTokensRangeParams) -> Result<Option<SemanticTokensRangeResult>> {
        let doc = self.document_or_error(&params.text_document.uri)?;
        let data = self
            .run(move || {
                let range = doc.byte_range(params.range);
                semantic_tokens::tokens(&doc, Some(range))
            })
            .await?;
        Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens { result_id: None, data })))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<LSPAny>> {
        let uri: Uri = params
            .arguments
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid_params(format!("{} expects a document URI argument", params.command)))?
            .parse()
            .map_err(|e| Error::invalid_params(format!("invalid document URI: {e}")))?;
        let doc = self.document_or_error(&uri)?;
        let config = self.config();
        let command = params.command.clone();
        // Commands parse the whole model; keep them off the async workers.
        let output = tokio::task::spawn_blocking(move || commands::execute(&command, &doc, &config))
            .await
            .map_err(|e| Error::invalid_params(format!("{} failed: {e}", params.command)))?;
        match output {
            Ok(output) => {
                self.client.show_message(MessageType::INFO, &output.message).await;
                Ok(Some(output.value))
            }
            Err(message) => {
                self.client.show_message(MessageType::ERROR, &message).await;
                Err(Error::invalid_params(message))
            }
        }
    }
}
