//! End-to-end tests driving a real [`Server`] over in-memory duplex pipes.
//!
//! Each test starts a server task wired to a [`TestBackend`], then speaks the
//! framed JSON-RPC protocol over the pipe exactly as an editor would. This
//! exercises the whole stack: framing, message classification, dispatch,
//! lifecycle enforcement, server→client notifications, and cancellation.

use rusty_lsp::error::{Result, codes};
use rusty_lsp::jsonrpc::{Message, Notification, Request, RequestId, Response};
use rusty_lsp::lsp::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    ClientCapabilities, CodeAction, CodeActionOrCommand, CodeActionParams, CodeLens,
    CodeLensParams, Color, ColorInformation, ColorPresentation, ColorPresentationParams,
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    ConfigurationItem, CreateFilesParams, DeleteFilesParams, Diagnostic, DiagnosticSeverity,
    DidChangeConfigurationParams, DidChangeNotebookDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWorkspaceFoldersParams, DidCloseNotebookDocumentParams, DidOpenNotebookDocumentParams,
    DidOpenTextDocumentParams, DidSaveNotebookDocumentParams, DocumentColorParams,
    DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentFormattingParams, DocumentLink,
    DocumentLinkParams, DocumentOnTypeFormattingParams, DocumentRangeFormattingParams,
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, ExecuteCommandParams,
    FoldingRange, FoldingRangeParams, FullDocumentDiagnosticReport, GotoDefinitionResponse, Hover,
    HoverParams, InitializeParams, InitializeResult, InlayHint, InlayHintParams, Location,
    MessageActionItem, MessageType, NotebookCell, NotebookCellKind, NotebookDocument,
    NotebookDocumentIdentifier, Position, PrepareRenameResponse, Range, ReferenceParams,
    Registration, RenameFilesParams, RenameParams, SelectionRange, SelectionRangeParams,
    SemanticTokens, SemanticTokensDeltaParams, SemanticTokensDeltaResult, SemanticTokensParams,
    SemanticTokensRangeParams, ServerCapabilities, ServerInfo, SetTraceParams, ShowDocumentParams,
    SignatureHelp, SignatureHelpParams, SignatureInformation, SymbolInformation, SymbolKind,
    TextDocumentPositionParams, TextDocumentSyncKind, TextEdit, TypeHierarchyItem,
    TypeHierarchyPrepareParams, TypeHierarchySubtypesParams, TypeHierarchySupertypesParams,
    Unregistration, VersionedNotebookDocumentIdentifier, WillSaveTextDocumentParams,
    WorkDoneProgressBegin, WorkDoneProgressCancelParams, WorkDoneProgressEnd,
    WorkDoneProgressReport, WorkspaceDiagnosticParams, WorkspaceDiagnosticReport,
    WorkspaceDocumentDiagnosticReport, WorkspaceEdit, WorkspaceFullDocumentDiagnosticReport,
    WorkspaceSymbolParams, code_action_kind,
};
use rusty_lsp::{Client, Error, LanguageServer, Server};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{BufReader, DuplexStream};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// A backend with just enough real behaviour to observe the framework's
/// dispatch and message paths over the wire.
struct TestBackend {
    client: Client,
    documents: RwLock<HashMap<String, String>>,
    client_capabilities: RwLock<Option<ClientCapabilities>>,
}

impl TestBackend {
    fn new(client: Client) -> Self {
        TestBackend {
            client,
            documents: RwLock::new(HashMap::new()),
            client_capabilities: RwLock::new(None),
        }
    }
}

impl LanguageServer for TestBackend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        *self.client_capabilities.write().await = Some(params.capabilities);
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncKind::Full),
                hover_provider: Some(true),
                completion_provider: Some(CompletionOptions::default()),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "test-server".to_owned(),
                version: None,
            }),
        })
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text;
        let diagnostics = scan_todos(&text);
        self.documents.write().await.insert(uri.clone(), text);
        let _ =
            self.client
                .publish_diagnostics(uri, diagnostics, Some(params.text_document.version));
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position.text_document.uri;
        let documents = self.documents.read().await;
        let Some(text) = documents.get(uri) else {
            return Ok(None);
        };
        let hellos = text.split_whitespace().filter(|w| *w == "hello").count();
        Ok(Some(Hover::markdown(format!("hello x{hellos}"))))
    }

    async fn completion(&self, _params: CompletionParams) -> Result<Option<CompletionResponse>> {
        Ok(Some(CompletionResponse::Array(vec![
            CompletionItem::new("alpha").with_kind(CompletionItemKind::Text),
            CompletionItem::new("beta"),
        ])))
    }

    async fn handle_request(&self, method: &str, _params: Option<Value>) -> Result<Value> {
        match method {
            // A deliberately slow method, used to test cancellation. If
            // cancellation works the sleep never completes.
            "test/sleep" => {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(json!("slept"))
            }
            // A deliberately panicking method, used to test that a handler
            // panic still yields a response instead of hanging the request
            // forever. The panic backtrace printed by this test is expected.
            "test/panic" => panic!("intentional panic for test coverage"),
            // Drives a full work-done-progress sequence, used to test the
            // `Client` progress helpers round-trip over the wire.
            "test/progress" => {
                let token = "progress-1";
                self.client.create_progress(token).await?;
                self.client.progress_begin(
                    token,
                    WorkDoneProgressBegin {
                        title: "Working".to_owned(),
                        ..Default::default()
                    },
                )?;
                self.client.progress_report(
                    token,
                    WorkDoneProgressReport {
                        percentage: Some(50),
                        ..Default::default()
                    },
                )?;
                self.client.progress_end(
                    token,
                    WorkDoneProgressEnd {
                        message: Some("done".to_owned()),
                    },
                )?;
                Ok(json!("done"))
            }
            // Exercises `Client::configuration`.
            "test/configuration" => {
                let items = vec![ConfigurationItem {
                    section: Some("editor.tabSize".to_owned()),
                    scope_uri: None,
                }];
                let values = self.client.configuration(items).await?;
                Ok(json!(values))
            }
            // Exercises `Client::apply_edit`.
            "test/apply_edit" => {
                let edit = WorkspaceEdit::for_document(
                    "file:///a".to_owned(),
                    vec![TextEdit::new(
                        Range::new(Position::new(0, 0), Position::new(0, 1)),
                        "x",
                    )],
                );
                let result = self
                    .client
                    .apply_edit(edit, Some("test edit".to_owned()))
                    .await?;
                Ok(serde_json::to_value(result)?)
            }
            // Exercises the Client::refresh_* helpers.
            "test/refresh" => {
                self.client.refresh_semantic_tokens().await?;
                self.client.refresh_code_lenses().await?;
                self.client.refresh_inlay_hints().await?;
                self.client.refresh_diagnostics().await?;
                Ok(json!("refreshed"))
            }
            // Exercises `Client::send_progress` for partial-result streaming.
            "test/partial_result" => {
                self.client
                    .send_progress("partial-1", vec!["chunk-1", "chunk-2"])?;
                Ok(json!("done"))
            }
            // Exercises `Client::show_message_request`.
            "test/show_message_request" => {
                let choice = self
                    .client
                    .show_message_request(
                        MessageType::Info,
                        "pick one",
                        vec![
                            MessageActionItem {
                                title: "Yes".to_owned(),
                            },
                            MessageActionItem {
                                title: "No".to_owned(),
                            },
                        ],
                    )
                    .await?;
                Ok(serde_json::to_value(choice)?)
            }
            // Exercises `Client::show_document`.
            "test/show_document" => {
                let result = self
                    .client
                    .show_document(ShowDocumentParams {
                        uri: "file:///a".to_owned(),
                        external: None,
                        take_focus: Some(true),
                        selection: None,
                    })
                    .await?;
                Ok(serde_json::to_value(result)?)
            }
            // Exercises `Client::register_capability`.
            "test/register_capability" => {
                self.client
                    .register_capability(vec![Registration::new(
                        "reg-1",
                        "textDocument/formatting",
                        None,
                    )])
                    .await?;
                Ok(json!("registered"))
            }
            // Exercises `Client::unregister_capability`.
            "test/unregister_capability" => {
                self.client
                    .unregister_capability(vec![Unregistration {
                        id: "reg-1".to_owned(),
                        method: "textDocument/formatting".to_owned(),
                    }])
                    .await?;
                Ok(json!("unregistered"))
            }
            // Exercises `ClientCapabilities::get`/`supports` against the
            // capabilities captured during `initialize`.
            "test/capability_query" => {
                let capabilities = self.client_capabilities.read().await;
                let capabilities = capabilities.as_ref().expect("initialized");
                Ok(json!({
                    "hoverSupported": capabilities.supports("textDocument.hover"),
                    "applyEditSupported": capabilities.supports("workspace.applyEdit"),
                    "definitionSupported": capabilities.supports("textDocument.definition"),
                    "applyEditValue": capabilities.get("workspace.applyEdit"),
                }))
            }
            other => Err(Error::method_not_found(other.to_owned())),
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!(
                "workspace folders changed: +{} -{}",
                params.event.added.len(),
                params.event.removed.len()
            ),
        );
    }

    async fn work_done_progress_cancel(&self, params: WorkDoneProgressCancelParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!("progress cancelled: {:?}", params.token),
        );
    }

    async fn declaration(
        &self,
        _params: TextDocumentPositionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(Some(marker_location("declaration").into()))
    }

    async fn type_definition(
        &self,
        _params: TextDocumentPositionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(Some(marker_location("type-definition").into()))
    }

    async fn implementation(
        &self,
        _params: TextDocumentPositionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(Some(marker_location("implementation").into()))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        Ok(Some(vec![Location {
            uri,
            range: zero_range(),
        }]))
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        Ok(CompletionItem {
            detail: Some("resolved".to_owned()),
            ..item
        })
    }

    async fn document_symbol(
        &self,
        _params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        Ok(Some(
            vec![DocumentSymbol::new(
                "main",
                SymbolKind::Function,
                zero_range(),
                zero_range(),
            )]
            .into(),
        ))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        Ok(Some(vec![SymbolInformation::new(
            format!("match:{}", params.query),
            SymbolKind::Function,
            marker_location("workspace-symbol"),
        )]))
    }

    async fn signature_help(&self, _params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn foo(x: i32)".to_owned(),
                documentation: None,
                parameters: None,
                active_parameter: None,
            }],
            active_signature: Some(0),
            active_parameter: Some(0),
        }))
    }

    async fn code_action(
        &self,
        _params: CodeActionParams,
    ) -> Result<Option<Vec<CodeActionOrCommand>>> {
        Ok(Some(vec![
            CodeAction::new("Fix it")
                .with_kind(code_action_kind::QUICKFIX)
                .into(),
        ]))
    }

    async fn code_action_resolve(&self, action: CodeAction) -> Result<CodeAction> {
        Ok(CodeAction {
            edit: Some(WorkspaceEdit::for_document("file:///a".to_owned(), vec![])),
            ..action
        })
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        Ok(Some(WorkspaceEdit::for_document(
            uri,
            vec![TextEdit::new(zero_range(), params.new_name)],
        )))
    }

    async fn prepare_rename(
        &self,
        _params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        Ok(Some(PrepareRenameResponse::Range(zero_range())))
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = params.text_document_position.text_document.uri;
        Ok(Some(vec![call_hierarchy_item("main", uri)]))
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        Ok(Some(vec![CallHierarchyIncomingCall {
            from: call_hierarchy_item("caller", params.item.uri),
            from_ranges: vec![zero_range()],
        }]))
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        Ok(Some(vec![CallHierarchyOutgoingCall {
            to: call_hierarchy_item("callee", params.item.uri),
            from_ranges: vec![zero_range()],
        }]))
    }

    async fn prepare_type_hierarchy(
        &self,
        params: TypeHierarchyPrepareParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        let uri = params.text_document_position.text_document.uri;
        Ok(Some(vec![type_hierarchy_item("Main", uri)]))
    }

    async fn supertypes(
        &self,
        params: TypeHierarchySupertypesParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        Ok(Some(vec![type_hierarchy_item("Super", params.item.uri)]))
    }

    async fn subtypes(
        &self,
        params: TypeHierarchySubtypesParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        Ok(Some(vec![type_hierarchy_item("Sub", params.item.uri)]))
    }

    async fn set_trace(&self, params: SetTraceParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!("trace set to {:?}", params.value),
        );
    }

    async fn did_open_notebook_document(&self, params: DidOpenNotebookDocumentParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!(
                "notebook opened: {} ({} cells)",
                params.notebook_document.uri,
                params.notebook_document.cells.len()
            ),
        );
    }

    async fn did_change_notebook_document(&self, params: DidChangeNotebookDocumentParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!(
                "notebook changed: {} -> v{}",
                params.notebook_document.uri, params.notebook_document.version
            ),
        );
    }

    async fn did_save_notebook_document(&self, params: DidSaveNotebookDocumentParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!("notebook saved: {}", params.notebook_document.uri),
        );
    }

    async fn did_close_notebook_document(&self, params: DidCloseNotebookDocumentParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!("notebook closed: {}", params.notebook_document.uri),
        );
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        Ok(Some(json!({"ran": params.command})))
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!("config changed: {}", params.settings),
        );
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!("watched files changed: {}", params.changes.len()),
        );
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        Ok(Some(vec![TextEdit::new(
            zero_range(),
            format!("formatted(tabSize={})", params.options.tab_size),
        )]))
    }

    async fn range_formatting(
        &self,
        _params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        Ok(Some(vec![TextEdit::new(zero_range(), "range-formatted")]))
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        Ok(Some(vec![TextEdit::new(
            zero_range(),
            format!("on-type-formatted({})", params.ch),
        )]))
    }

    async fn folding_range(
        &self,
        _params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        Ok(Some(vec![FoldingRange::new(0, 3)]))
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        Ok(Some(
            params
                .positions
                .into_iter()
                .map(|_| SelectionRange {
                    range: zero_range(),
                    parent: None,
                })
                .collect(),
        ))
    }

    async fn code_lens(&self, _params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        Ok(Some(vec![CodeLens::new(zero_range())]))
    }

    async fn code_lens_resolve(&self, lens: CodeLens) -> Result<CodeLens> {
        Ok(CodeLens {
            command: Some(rusty_lsp::lsp::Command {
                title: "Run".to_owned(),
                command: "my.run".to_owned(),
                arguments: None,
            }),
            ..lens
        })
    }

    async fn document_link(
        &self,
        _params: DocumentLinkParams,
    ) -> Result<Option<Vec<DocumentLink>>> {
        Ok(Some(vec![DocumentLink {
            range: zero_range(),
            target: None,
            tooltip: None,
            data: None,
        }]))
    }

    async fn document_link_resolve(&self, link: DocumentLink) -> Result<DocumentLink> {
        Ok(DocumentLink {
            target: Some("file:///resolved".to_owned()),
            ..link
        })
    }

    async fn document_color(&self, _params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        Ok(vec![ColorInformation {
            range: zero_range(),
            color: Color {
                red: 1.0,
                green: 0.0,
                blue: 0.0,
                alpha: 1.0,
            },
        }])
    }

    async fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> Result<Vec<ColorPresentation>> {
        Ok(vec![ColorPresentation {
            label: format!(
                "rgb({}, {}, {})",
                params.color.red, params.color.green, params.color.blue
            ),
            text_edit: None,
            additional_text_edits: None,
        }])
    }

    async fn semantic_tokens_full(
        &self,
        _params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokens>> {
        Ok(Some(SemanticTokens {
            result_id: Some("1".to_owned()),
            data: vec![0, 0, 3, 0, 0],
        }))
    }

    async fn semantic_tokens_full_delta(
        &self,
        _params: SemanticTokensDeltaParams,
    ) -> Result<Option<SemanticTokensDeltaResult>> {
        Ok(Some(SemanticTokensDeltaResult::Tokens(SemanticTokens {
            result_id: Some("2".to_owned()),
            data: vec![1, 0, 4, 1, 0],
        })))
    }

    async fn semantic_tokens_range(
        &self,
        _params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokens>> {
        Ok(Some(SemanticTokens {
            result_id: None,
            data: vec![0, 0, 2, 2, 0],
        }))
    }

    async fn inlay_hint(&self, _params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        Ok(Some(vec![InlayHint::new(Position::new(0, 0), ": i32")]))
    }

    async fn inlay_hint_resolve(&self, hint: InlayHint) -> Result<InlayHint> {
        Ok(InlayHint {
            tooltip: Some(rusty_lsp::lsp::Documentation::plain_text(
                "resolved tooltip",
            )),
            ..hint
        })
    }

    async fn diagnostic(
        &self,
        _params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReport> {
        Ok(DocumentDiagnosticReport::Full(
            FullDocumentDiagnosticReport {
                result_id: Some("1".to_owned()),
                items: vec![Diagnostic::new(
                    zero_range(),
                    DiagnosticSeverity::Error,
                    "pulled diagnostic",
                )],
            },
        ))
    }

    async fn workspace_diagnostic(
        &self,
        _params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReport> {
        Ok(WorkspaceDiagnosticReport {
            items: vec![WorkspaceDocumentDiagnosticReport::Full(
                WorkspaceFullDocumentDiagnosticReport {
                    uri: "file:///a.txt".to_owned(),
                    version: None,
                    result_id: Some("1".to_owned()),
                    items: vec![Diagnostic::new(
                        zero_range(),
                        DiagnosticSeverity::Warning,
                        "workspace diagnostic",
                    )],
                },
            )],
        })
    }

    async fn will_save(&self, params: WillSaveTextDocumentParams) {
        let _ = self.client.log_message(
            MessageType::Info,
            format!(
                "will save {} (reason {:?})",
                params.text_document.uri, params.reason
            ),
        );
    }

    async fn will_save_wait_until(
        &self,
        _params: WillSaveTextDocumentParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        Ok(Some(vec![TextEdit::new(zero_range(), "trimmed")]))
    }

    async fn will_create_files(&self, params: CreateFilesParams) -> Result<Option<WorkspaceEdit>> {
        Ok(Some(WorkspaceEdit::for_document(
            params.files[0].uri.clone(),
            vec![TextEdit::new(zero_range(), "// boilerplate\n")],
        )))
    }

    async fn did_create_files(&self, params: CreateFilesParams) {
        let _ = self
            .client
            .log_message(MessageType::Info, format!("created {}", params.files.len()));
    }

    async fn will_rename_files(&self, params: RenameFilesParams) -> Result<Option<WorkspaceEdit>> {
        Ok(Some(WorkspaceEdit::for_document(
            params.files[0].old_uri.clone(),
            vec![TextEdit::new(zero_range(), "// import updated\n")],
        )))
    }

    async fn did_rename_files(&self, params: RenameFilesParams) {
        let _ = self
            .client
            .log_message(MessageType::Info, format!("renamed {}", params.files.len()));
    }

    async fn will_delete_files(&self, params: DeleteFilesParams) -> Result<Option<WorkspaceEdit>> {
        Ok(Some(WorkspaceEdit::for_document(
            params.files[0].uri.clone(),
            vec![TextEdit::new(zero_range(), "")],
        )))
    }

    async fn did_delete_files(&self, params: DeleteFilesParams) {
        let _ = self
            .client
            .log_message(MessageType::Info, format!("deleted {}", params.files.len()));
    }
}

/// A zero-width range at the document start, used by tests that don't care
/// about the specific range returned.
fn zero_range() -> Range {
    Range::new(Position::new(0, 0), Position::new(0, 1))
}

/// A `Location` whose URI embeds `marker`, so tests can assert exactly which
/// handler produced a given navigation result.
fn marker_location(marker: &str) -> Location {
    Location {
        uri: format!("file:///{marker}"),
        range: zero_range(),
    }
}

fn call_hierarchy_item(name: &str, uri: String) -> CallHierarchyItem {
    CallHierarchyItem {
        name: name.to_owned(),
        kind: SymbolKind::Function,
        tags: vec![],
        detail: None,
        uri,
        range: zero_range(),
        selection_range: zero_range(),
        data: None,
    }
}

fn type_hierarchy_item(name: &str, uri: String) -> TypeHierarchyItem {
    TypeHierarchyItem {
        name: name.to_owned(),
        kind: SymbolKind::Class,
        tags: vec![],
        detail: None,
        uri,
        range: zero_range(),
        selection_range: zero_range(),
        data: None,
    }
}

/// Flag each line containing a `TODO` substring as a warning (ASCII columns).
fn scan_todos(text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        if let Some(col) = line.find("TODO") {
            let range = Range::new(
                Position::new(line_no as u32, col as u32),
                Position::new(line_no as u32, (col + 4) as u32),
            );
            diagnostics.push(Diagnostic::new(
                range,
                DiagnosticSeverity::Warning,
                "TODO marker",
            ));
        }
    }
    diagnostics
}

/// Test harness: a running server plus the client end of its transport.
struct Harness {
    to_server: DuplexStream,
    from_server: BufReader<DuplexStream>,
    serve: JoinHandle<Result<()>>,
    next_id: i64,
}

impl Harness {
    fn start() -> Self {
        let (client_write, server_read) = tokio::io::duplex(1 << 16);
        let (server_write, client_read) = tokio::io::duplex(1 << 16);
        let serve = tokio::spawn(async move {
            Server::new(server_read, server_write)
                .serve(TestBackend::new)
                .await
        });
        Harness {
            to_server: client_write,
            from_server: rusty_lsp::transport::buffered(client_read),
            serve,
            next_id: 0,
        }
    }

    async fn send(&mut self, message: Message) {
        rusty_lsp::transport::write_message(&mut self.to_server, &message)
            .await
            .expect("write message");
    }

    async fn request(&mut self, method: &str, params: Value) -> RequestId {
        self.next_id += 1;
        let id = RequestId::Number(self.next_id);
        self.send(Message::Request(Request {
            id: id.clone(),
            method: method.to_owned(),
            params: Some(params),
        }))
        .await;
        id
    }

    async fn notify(&mut self, method: &str, params: Value) {
        self.send(Message::Notification(Notification {
            method: method.to_owned(),
            params: Some(params),
        }))
        .await;
    }

    /// Answer a server-to-client request (playing the client's role in the
    /// handshake for e.g. `window/workDoneProgress/create`).
    async fn respond(&mut self, id: RequestId, result: Value) {
        self.send(Message::Response(Response::success(id, result)))
            .await;
    }

    /// Read until a request with `method` arrives, skipping interleaved
    /// messages.
    async fn recv_request(&mut self, method: &str) -> Request {
        loop {
            if let Message::Request(req) = self.recv().await
                && req.method == method
            {
                return req;
            }
        }
    }

    async fn recv(&mut self) -> Message {
        rusty_lsp::transport::read_message(&mut self.from_server)
            .await
            .expect("read message")
            .expect("stream still open")
    }

    /// Read until the response for `id` arrives, skipping interleaved messages.
    async fn recv_response(&mut self, id: &RequestId) -> Response {
        loop {
            if let Message::Response(response) = self.recv().await
                && response.id.as_ref() == Some(id)
            {
                return response;
            }
        }
    }

    /// Read until a notification with `method` arrives.
    async fn recv_notification(&mut self, method: &str) -> Notification {
        loop {
            if let Message::Notification(note) = self.recv().await
                && note.method == method
            {
                return note;
            }
        }
    }

    /// Drive the full `initialize` / `initialized` handshake.
    async fn initialize(&mut self) -> Response {
        let id = self
            .request("initialize", json!({ "capabilities": {} }))
            .await;
        let response = self.recv_response(&id).await;
        self.notify("initialized", json!({})).await;
        response
    }

    async fn open(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "plaintext",
                    "version": 1,
                    "text": text,
                }
            }),
        )
        .await;
    }
}

fn position_params(uri: &str, line: u32, character: u32) -> Value {
    json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character },
    })
}

#[tokio::test]
async fn initialize_advertises_capabilities() {
    let mut harness = Harness::start();
    let id = harness
        .request("initialize", json!({ "capabilities": {} }))
        .await;
    let response = harness.recv_response(&id).await;

    assert!(response.error.is_none());
    let result = response.result.expect("result present");
    assert_eq!(result["capabilities"]["hoverProvider"], json!(true));
    assert_eq!(result["capabilities"]["textDocumentSync"], json!(1));
    assert_eq!(result["serverInfo"]["name"], json!("test-server"));
}

#[tokio::test]
async fn requests_before_initialize_are_rejected() {
    let mut harness = Harness::start();
    let id = harness
        .request("textDocument/hover", position_params("file:///a.txt", 0, 0))
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.error.expect("error").code,
        codes::SERVER_NOT_INITIALIZED
    );
}

#[tokio::test]
async fn second_initialize_is_rejected() {
    let mut harness = Harness::start();
    harness.initialize().await;
    let id = harness
        .request("initialize", json!({ "capabilities": {} }))
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(response.error.expect("error").code, codes::INVALID_REQUEST);
}

#[tokio::test]
async fn did_open_publishes_diagnostics() {
    let mut harness = Harness::start();
    harness.initialize().await;
    harness
        .open("file:///todo.txt", "ok line\nplease TODO this\n")
        .await;

    let note = harness
        .recv_notification("textDocument/publishDiagnostics")
        .await;
    let params = note.params.expect("params");
    assert_eq!(params["uri"], json!("file:///todo.txt"));
    let diagnostics = params["diagnostics"].as_array().expect("array");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["range"]["start"]["line"], json!(1));
    assert_eq!(diagnostics[0]["range"]["start"]["character"], json!(7));
    assert_eq!(diagnostics[0]["severity"], json!(2));
}

#[tokio::test]
async fn hover_dispatches_and_serializes_result() {
    let mut harness = Harness::start();
    harness.initialize().await;
    harness.open("file:///h.txt", "hello hello world").await;

    let id = harness
        .request("textDocument/hover", position_params("file:///h.txt", 0, 0))
        .await;
    let response = harness.recv_response(&id).await;
    let result = response.result.expect("result");
    assert_eq!(result["contents"]["kind"], json!("markdown"));
    assert_eq!(result["contents"]["value"], json!("hello x2"));
}

#[tokio::test]
async fn completion_returns_array_of_items() {
    let mut harness = Harness::start();
    harness.initialize().await;
    harness.open("file:///c.txt", "anything").await;

    let id = harness
        .request(
            "textDocument/completion",
            position_params("file:///c.txt", 0, 0),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let items = response.result.expect("result");
    let items = items.as_array().expect("array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["label"], json!("alpha"));
    assert_eq!(items[0]["kind"], json!(1));
    assert_eq!(items[1]["label"], json!("beta"));
}

#[tokio::test]
async fn declaration_type_definition_implementation_route_to_distinct_handlers() {
    let mut harness = Harness::start();
    harness.initialize().await;

    for (method, marker) in [
        ("textDocument/declaration", "declaration"),
        ("textDocument/typeDefinition", "type-definition"),
        ("textDocument/implementation", "implementation"),
    ] {
        let id = harness
            .request(method, position_params("file:///a.txt", 0, 0))
            .await;
        let response = harness.recv_response(&id).await;
        let result = response.result.expect("result");
        assert_eq!(result["uri"], json!(format!("file:///{marker}")));
    }
}

#[tokio::test]
async fn references_returns_locations() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let mut params = position_params("file:///a.txt", 0, 0);
    params["context"] = json!({"includeDeclaration": true});
    let id = harness.request("textDocument/references", params).await;
    let response = harness.recv_response(&id).await;
    let locations = response.result.expect("result");
    assert_eq!(locations[0]["uri"], json!("file:///a.txt"));
}

#[tokio::test]
async fn completion_item_resolve_enriches_the_item() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request("completionItem/resolve", json!({"label": "alpha"}))
        .await;
    let response = harness.recv_response(&id).await;
    let item = response.result.expect("result");
    assert_eq!(item["label"], json!("alpha"));
    assert_eq!(item["detail"], json!("resolved"));
}

#[tokio::test]
async fn document_symbol_returns_nested_symbols() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": "file:///a.txt"}}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let symbols = response.result.expect("result");
    assert_eq!(symbols[0]["name"], json!("main"));
    assert_eq!(symbols[0]["kind"], json!(12));
}

#[tokio::test]
async fn workspace_symbol_search_echoes_the_query() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request("workspace/symbol", json!({"query": "foo"}))
        .await;
    let response = harness.recv_response(&id).await;
    let symbols = response.result.expect("result");
    assert_eq!(symbols[0]["name"], json!("match:foo"));
}

#[tokio::test]
async fn signature_help_returns_active_signature() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/signatureHelp",
            position_params("file:///a.txt", 0, 0),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let help = response.result.expect("result");
    assert_eq!(help["signatures"][0]["label"], json!("fn foo(x: i32)"));
    assert_eq!(help["activeParameter"], json!(0));
}

#[tokio::test]
async fn code_action_and_resolve_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": "file:///a.txt"},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                "context": {"diagnostics": []},
            }),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let actions = response.result.expect("result");
    assert_eq!(actions[0]["title"], json!("Fix it"));
    assert_eq!(actions[0]["kind"], json!("quickfix"));

    let resolve_id = harness
        .request("codeAction/resolve", actions[0].clone())
        .await;
    let resolved = harness.recv_response(&resolve_id).await;
    let action = resolved.result.expect("result");
    assert_eq!(action["edit"]["changes"]["file:///a"], json!([]));
}

#[tokio::test]
async fn rename_and_prepare_rename_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let mut params = position_params("file:///a.txt", 0, 0);
    params["newName"] = json!("renamed");
    let id = harness.request("textDocument/rename", params).await;
    let response = harness.recv_response(&id).await;
    let edit = response.result.expect("result");
    assert_eq!(
        edit["changes"]["file:///a.txt"][0]["newText"],
        json!("renamed")
    );

    let id = harness
        .request(
            "textDocument/prepareRename",
            position_params("file:///a.txt", 0, 0),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let prepared = response.result.expect("result");
    assert_eq!(prepared["start"]["line"], json!(0));
}

#[tokio::test]
async fn call_hierarchy_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/prepareCallHierarchy",
            position_params("file:///a.txt", 0, 0),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let items = response.result.expect("result");
    assert_eq!(items[0]["name"], json!("main"));
    assert_eq!(items[0]["uri"], json!("file:///a.txt"));

    let item = items[0].clone();

    let id = harness
        .request("callHierarchy/incomingCalls", json!({"item": item.clone()}))
        .await;
    let response = harness.recv_response(&id).await;
    let calls = response.result.expect("result");
    assert_eq!(calls[0]["from"]["name"], json!("caller"));

    let id = harness
        .request("callHierarchy/outgoingCalls", json!({"item": item}))
        .await;
    let response = harness.recv_response(&id).await;
    let calls = response.result.expect("result");
    assert_eq!(calls[0]["to"]["name"], json!("callee"));
}

#[tokio::test]
async fn type_hierarchy_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/prepareTypeHierarchy",
            position_params("file:///a.txt", 0, 0),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let items = response.result.expect("result");
    assert_eq!(items[0]["name"], json!("Main"));

    let item = items[0].clone();

    let id = harness
        .request("typeHierarchy/supertypes", json!({"item": item.clone()}))
        .await;
    let response = harness.recv_response(&id).await;
    let supertypes = response.result.expect("result");
    assert_eq!(supertypes[0]["name"], json!("Super"));

    let id = harness
        .request("typeHierarchy/subtypes", json!({"item": item}))
        .await;
    let response = harness.recv_response(&id).await;
    let subtypes = response.result.expect("result");
    assert_eq!(subtypes[0]["name"], json!("Sub"));
}

#[tokio::test]
async fn set_trace_notification_is_routed() {
    let mut harness = Harness::start();
    harness.initialize().await;

    harness
        .notify("$/setTrace", json!({"value": "verbose"}))
        .await;

    let note = harness.recv_notification("window/logMessage").await;
    let message = note.params.expect("params")["message"]
        .as_str()
        .expect("string")
        .to_owned();
    assert!(message.contains("Verbose"), "message was: {message}");
}

#[tokio::test]
async fn notebook_document_sync_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let notebook = NotebookDocument {
        uri: "file:///a.ipynb".to_owned(),
        notebook_type: "jupyter-notebook".to_owned(),
        version: 1,
        metadata: None,
        cells: vec![NotebookCell {
            kind: NotebookCellKind::Code,
            document: "file:///a.ipynb#cell1".to_owned(),
            metadata: None,
            execution_summary: None,
        }],
    };

    harness
        .notify(
            "notebookDocument/didOpen",
            json!({
                "notebookDocument": notebook,
                "cellTextDocuments": [{
                    "uri": "file:///a.ipynb#cell1",
                    "languageId": "python",
                    "version": 1,
                    "text": "print('hi')",
                }],
            }),
        )
        .await;
    let note = harness.recv_notification("window/logMessage").await;
    assert!(
        note.params.expect("params")["message"]
            .as_str()
            .unwrap()
            .contains("file:///a.ipynb")
    );

    harness
        .notify(
            "notebookDocument/didChange",
            json!({
                "notebookDocument": VersionedNotebookDocumentIdentifier {
                    version: 2,
                    uri: "file:///a.ipynb".to_owned(),
                },
                "change": {},
            }),
        )
        .await;
    let note = harness.recv_notification("window/logMessage").await;
    assert!(
        note.params.expect("params")["message"]
            .as_str()
            .unwrap()
            .contains("v2")
    );

    harness
        .notify(
            "notebookDocument/didSave",
            json!({"notebookDocument": NotebookDocumentIdentifier { uri: "file:///a.ipynb".to_owned() }}),
        )
        .await;
    let note = harness.recv_notification("window/logMessage").await;
    assert!(
        note.params.expect("params")["message"]
            .as_str()
            .unwrap()
            .contains("saved")
    );

    harness
        .notify(
            "notebookDocument/didClose",
            json!({
                "notebookDocument": NotebookDocumentIdentifier { uri: "file:///a.ipynb".to_owned() },
                "cellTextDocuments": [{"uri": "file:///a.ipynb#cell1"}],
            }),
        )
        .await;
    let note = harness.recv_notification("window/logMessage").await;
    assert!(
        note.params.expect("params")["message"]
            .as_str()
            .unwrap()
            .contains("closed")
    );
}

#[tokio::test]
async fn execute_command_runs_and_returns_a_result() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "workspace/executeCommand",
            json!({"command": "my.command", "arguments": []}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(response.result.expect("result")["ran"], json!("my.command"));
}

#[tokio::test]
async fn did_change_configuration_notification_is_routed() {
    let mut harness = Harness::start();
    harness.initialize().await;

    harness
        .notify(
            "workspace/didChangeConfiguration",
            json!({"settings": {"tabSize": 4}}),
        )
        .await;

    let note = harness.recv_notification("window/logMessage").await;
    assert_eq!(
        note.params.unwrap()["message"],
        json!("config changed: {\"tabSize\":4}")
    );
}

#[tokio::test]
async fn did_change_watched_files_notification_is_routed() {
    let mut harness = Harness::start();
    harness.initialize().await;

    harness
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": "file:///a", "type": 2}]}),
        )
        .await;

    let note = harness.recv_notification("window/logMessage").await;
    assert_eq!(
        note.params.unwrap()["message"],
        json!("watched files changed: 1")
    );
}

#[tokio::test]
async fn formatting_family_returns_text_edits() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/formatting",
            json!({"textDocument": {"uri": "file:///a.txt"}, "options": {"tabSize": 4, "insertSpaces": true}}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")[0]["newText"],
        json!("formatted(tabSize=4)")
    );

    let id = harness
        .request(
            "textDocument/rangeFormatting",
            json!({
                "textDocument": {"uri": "file:///a.txt"},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                "options": {"tabSize": 2, "insertSpaces": false},
            }),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")[0]["newText"],
        json!("range-formatted")
    );

    let mut params = position_params("file:///a.txt", 0, 0);
    params["ch"] = json!("}");
    params["options"] = json!({"tabSize": 4, "insertSpaces": true});
    let id = harness
        .request("textDocument/onTypeFormatting", params)
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")[0]["newText"],
        json!("on-type-formatted(})")
    );
}

#[tokio::test]
async fn folding_range_and_selection_range_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/foldingRange",
            json!({"textDocument": {"uri": "file:///a.txt"}}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let ranges = response.result.expect("result");
    assert_eq!(ranges[0]["startLine"], json!(0));
    assert_eq!(ranges[0]["endLine"], json!(3));
    assert!(ranges[0].get("startCharacter").is_none());

    let id = harness
        .request(
            "textDocument/selectionRange",
            json!({
                "textDocument": {"uri": "file:///a.txt"},
                "positions": [{"line": 0, "character": 0}],
            }),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let ranges = response.result.expect("result");
    assert_eq!(ranges.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn code_lens_and_resolve_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/codeLens",
            json!({"textDocument": {"uri": "file:///a.txt"}}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let lenses = response.result.expect("result");
    assert!(lenses[0].get("command").is_none());

    let resolve_id = harness.request("codeLens/resolve", lenses[0].clone()).await;
    let resolved = harness.recv_response(&resolve_id).await;
    assert_eq!(
        resolved.result.expect("result")["command"]["command"],
        json!("my.run")
    );
}

#[tokio::test]
async fn document_link_resolve_and_color_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/documentLink",
            json!({"textDocument": {"uri": "file:///a.txt"}}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let links = response.result.expect("result");
    assert!(links[0].get("target").is_none());

    let resolve_id = harness
        .request("documentLink/resolve", links[0].clone())
        .await;
    let resolved = harness.recv_response(&resolve_id).await;
    assert_eq!(
        resolved.result.expect("result")["target"],
        json!("file:///resolved")
    );

    let id = harness
        .request(
            "textDocument/documentColor",
            json!({"textDocument": {"uri": "file:///a.txt"}}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let colors = response.result.expect("result");
    assert_eq!(colors[0]["color"]["red"], json!(1.0));

    let id = harness
        .request(
            "textDocument/colorPresentation",
            json!({
                "textDocument": {"uri": "file:///a.txt"},
                "color": {"red": 1.0, "green": 0.0, "blue": 0.0, "alpha": 1.0},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            }),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")[0]["label"],
        json!("rgb(1, 0, 0)")
    );
}

#[tokio::test]
async fn semantic_tokens_full_delta_and_range_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/semanticTokens/full",
            json!({"textDocument": {"uri": "file:///a.txt"}}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")["data"],
        json!([0, 0, 3, 0, 0])
    );

    let id = harness
        .request(
            "textDocument/semanticTokens/full/delta",
            json!({"textDocument": {"uri": "file:///a.txt"}, "previousResultId": "1"}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")["data"],
        json!([1, 0, 4, 1, 0])
    );

    let id = harness
        .request(
            "textDocument/semanticTokens/range",
            json!({
                "textDocument": {"uri": "file:///a.txt"},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 1, "character": 0}},
            }),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")["data"],
        json!([0, 0, 2, 2, 0])
    );
}

#[tokio::test]
async fn inlay_hint_and_resolve_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": "file:///a.txt"},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 1, "character": 0}},
            }),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let hints = response.result.expect("result");
    assert_eq!(hints[0]["label"], json!(": i32"));
    assert!(hints[0].get("tooltip").is_none());

    let resolve_id = harness.request("inlayHint/resolve", hints[0].clone()).await;
    let resolved = harness.recv_response(&resolve_id).await;
    assert_eq!(
        resolved.result.expect("result")["tooltip"],
        json!("resolved tooltip")
    );
}

#[tokio::test]
async fn unknown_method_yields_method_not_found() {
    let mut harness = Harness::start();
    harness.initialize().await;
    let id = harness.request("textDocument/moniker", json!({})).await;
    let response = harness.recv_response(&id).await;
    assert_eq!(response.error.expect("error").code, codes::METHOD_NOT_FOUND);
}

#[tokio::test]
async fn invalid_params_yield_invalid_params_error() {
    let mut harness = Harness::start();
    harness.initialize().await;
    // hover with a missing `position` field fails to deserialize.
    let id = harness
        .request(
            "textDocument/hover",
            json!({ "textDocument": { "uri": "file:///a" } }),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(response.error.expect("error").code, codes::INVALID_PARAMS);
}

#[tokio::test]
async fn cancel_request_aborts_and_responds() {
    let mut harness = Harness::start();
    harness.initialize().await;

    // Kick off a 30s handler, then cancel it. With working cancellation the
    // cancellation response must arrive almost immediately.
    let id = harness.request("test/sleep", json!({})).await;
    let RequestId::Number(numeric_id) = id.clone() else {
        unreachable!("ids are numeric in this harness");
    };
    harness
        .notify("$/cancelRequest", json!({ "id": numeric_id }))
        .await;

    let response = tokio::time::timeout(Duration::from_secs(5), harness.recv_response(&id))
        .await
        .expect("cancellation response should arrive promptly");
    assert_eq!(
        response.error.expect("error").code,
        codes::REQUEST_CANCELLED
    );
}

#[tokio::test]
async fn duplicate_request_id_is_rejected_without_disturbing_the_original() {
    let mut harness = Harness::start();
    harness.initialize().await;

    // Kick off a slow, still-outstanding request.
    let id = RequestId::Number(999);
    harness
        .send(Message::Request(Request {
            id: id.clone(),
            method: "test/sleep".to_owned(),
            params: Some(json!({})),
        }))
        .await;

    // A second request reusing that same still-outstanding id (a client
    // protocol violation) must be rejected outright, not silently corrupt
    // the first request's cancellation/response bookkeeping.
    harness
        .send(Message::Request(Request {
            id: id.clone(),
            method: "textDocument/hover".to_owned(),
            params: Some(position_params("file:///a", 0, 0)),
        }))
        .await;
    let duplicate_response = harness.recv_response(&id).await;
    assert_eq!(
        duplicate_response.error.expect("error").code,
        codes::INVALID_REQUEST
    );

    // The original request's InFlight entry is untouched: cancelling it
    // still works, proving it was never stolen by the duplicate.
    let RequestId::Number(numeric_id) = id.clone() else {
        unreachable!("ids are numeric in this harness");
    };
    harness
        .notify("$/cancelRequest", json!({ "id": numeric_id }))
        .await;
    let cancel_response = tokio::time::timeout(Duration::from_secs(5), harness.recv_response(&id))
        .await
        .expect("cancellation response should arrive promptly");
    assert_eq!(
        cancel_response.error.expect("error").code,
        codes::REQUEST_CANCELLED
    );
}

#[tokio::test]
async fn shutdown_rejects_further_requests_then_exit_stops_server() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let shutdown_id = harness.request("shutdown", Value::Null).await;
    let shutdown = harness.recv_response(&shutdown_id).await;
    // Success (no error). The server emits `"result": null` on the wire, which
    // serde collapses to `None` when parsed back into `Option<Value>`.
    assert!(shutdown.error.is_none());
    assert!(shutdown.result.is_none());

    // After shutdown, feature requests are refused.
    let hover_id = harness
        .request("textDocument/hover", position_params("file:///a", 0, 0))
        .await;
    let refused = harness.recv_response(&hover_id).await;
    assert_eq!(refused.error.expect("error").code, codes::INVALID_REQUEST);

    // `exit` ends the loop; the server task returns cleanly.
    harness.notify("exit", Value::Null).await;
    let serve = harness.serve;
    let outcome = tokio::time::timeout(Duration::from_secs(5), serve)
        .await
        .expect("server should stop after exit")
        .expect("server task did not panic");
    assert!(outcome.is_ok());
}

#[tokio::test]
async fn progress_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness.request("test/progress", json!({})).await;

    // The server reserves a token before using it; accept the reservation.
    let create = harness.recv_request("window/workDoneProgress/create").await;
    assert_eq!(create.params.unwrap()["token"], json!("progress-1"));
    harness.respond(create.id, Value::Null).await;

    let begin = harness.recv_notification("$/progress").await;
    let begin_value = begin.params.unwrap();
    assert_eq!(begin_value["token"], json!("progress-1"));
    assert_eq!(begin_value["value"]["kind"], json!("begin"));
    assert_eq!(begin_value["value"]["title"], json!("Working"));

    let report = harness.recv_notification("$/progress").await;
    assert_eq!(report.params.unwrap()["value"]["kind"], json!("report"));

    let end = harness.recv_notification("$/progress").await;
    let end_value = end.params.unwrap();
    assert_eq!(end_value["value"]["kind"], json!("end"));
    assert_eq!(end_value["value"]["message"], json!("done"));

    let response = harness.recv_response(&id).await;
    assert_eq!(response.result, Some(json!("done")));
}

#[tokio::test]
async fn configuration_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness.request("test/configuration", json!({})).await;

    let config_request = harness.recv_request("workspace/configuration").await;
    assert_eq!(
        config_request.params.unwrap()["items"][0]["section"],
        json!("editor.tabSize")
    );
    harness.respond(config_request.id, json!([4])).await;

    let response = harness.recv_response(&id).await;
    assert_eq!(response.result, Some(json!([4])));
}

#[tokio::test]
async fn apply_edit_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness.request("test/apply_edit", json!({})).await;

    let edit_request = harness.recv_request("workspace/applyEdit").await;
    let params = edit_request.params.unwrap();
    assert_eq!(params["label"], json!("test edit"));
    assert_eq!(
        params["edit"]["changes"]["file:///a"][0]["newText"],
        json!("x")
    );
    harness
        .respond(edit_request.id, json!({"applied": true}))
        .await;

    let response = harness.recv_response(&id).await;
    assert_eq!(response.result.unwrap()["applied"], json!(true));
}

#[tokio::test]
async fn will_save_and_wait_until_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    harness
        .notify(
            "textDocument/willSave",
            json!({"textDocument": {"uri": "file:///a.txt"}, "reason": 1}),
        )
        .await;
    let note = harness.recv_notification("window/logMessage").await;
    assert!(
        note.params.unwrap()["message"]
            .as_str()
            .unwrap()
            .contains("file:///a.txt")
    );

    let id = harness
        .request(
            "textDocument/willSaveWaitUntil",
            json!({"textDocument": {"uri": "file:///a.txt"}, "reason": 1}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")[0]["newText"],
        json!("trimmed")
    );
}

#[tokio::test]
async fn file_operation_hooks_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "workspace/willCreateFiles",
            json!({"files": [{"uri": "file:///new.txt"}]}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")["changes"]["file:///new.txt"][0]["newText"],
        json!("// boilerplate\n")
    );

    harness
        .notify(
            "workspace/didCreateFiles",
            json!({"files": [{"uri": "file:///new.txt"}]}),
        )
        .await;
    let note = harness.recv_notification("window/logMessage").await;
    assert_eq!(note.params.unwrap()["message"], json!("created 1"));

    let id = harness
        .request(
            "workspace/willRenameFiles",
            json!({"files": [{"oldUri": "file:///a.txt", "newUri": "file:///b.txt"}]}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")["changes"]["file:///a.txt"][0]["newText"],
        json!("// import updated\n")
    );

    harness
        .notify(
            "workspace/didRenameFiles",
            json!({"files": [{"oldUri": "file:///a.txt", "newUri": "file:///b.txt"}]}),
        )
        .await;
    let note = harness.recv_notification("window/logMessage").await;
    assert_eq!(note.params.unwrap()["message"], json!("renamed 1"));

    let id = harness
        .request(
            "workspace/willDeleteFiles",
            json!({"files": [{"uri": "file:///gone.txt"}]}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(
        response.result.expect("result")["changes"]["file:///gone.txt"][0]["newText"],
        json!("")
    );

    harness
        .notify(
            "workspace/didDeleteFiles",
            json!({"files": [{"uri": "file:///gone.txt"}]}),
        )
        .await;
    let note = harness.recv_notification("window/logMessage").await;
    assert_eq!(note.params.unwrap()["message"], json!("deleted 1"));
}

#[tokio::test]
async fn diagnostic_pull_model_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/diagnostic",
            json!({"textDocument": {"uri": "file:///a.txt"}}),
        )
        .await;
    let response = harness.recv_response(&id).await;
    let report = response.result.expect("result");
    assert_eq!(report["kind"], json!("full"));
    assert_eq!(report["items"][0]["message"], json!("pulled diagnostic"));

    let id = harness
        .request("workspace/diagnostic", json!({"previousResultIds": []}))
        .await;
    let response = harness.recv_response(&id).await;
    let report = response.result.expect("result");
    assert_eq!(report["items"][0]["kind"], json!("full"));
    assert_eq!(report["items"][0]["uri"], json!("file:///a.txt"));
}

#[tokio::test]
async fn refresh_helpers_send_paramless_requests() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness.request("test/refresh", json!({})).await;

    for method in [
        "workspace/semanticTokens/refresh",
        "workspace/codeLens/refresh",
        "workspace/inlayHint/refresh",
        "workspace/diagnostic/refresh",
    ] {
        let request = harness.recv_request(method).await;
        // The refresh methods take no params at all on the wire, not `null`.
        assert!(request.params.is_none());
        harness.respond(request.id, Value::Null).await;
    }

    let response = harness.recv_response(&id).await;
    assert_eq!(response.result, Some(json!("refreshed")));
}

#[tokio::test]
async fn partial_result_streaming_sends_progress_notification() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness.request("test/partial_result", json!({})).await;

    let note = harness.recv_notification("$/progress").await;
    let params = note.params.expect("params");
    assert_eq!(params["token"], json!("partial-1"));
    assert_eq!(params["value"], json!(["chunk-1", "chunk-2"]));

    let response = harness.recv_response(&id).await;
    assert_eq!(response.result, Some(json!("done")));
}

#[tokio::test]
async fn show_message_request_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request("test/show_message_request", json!({}))
        .await;

    let request = harness.recv_request("window/showMessageRequest").await;
    let params = request.params.expect("params");
    assert_eq!(params["type"], json!(3));
    assert_eq!(params["message"], json!("pick one"));
    assert_eq!(params["actions"][0]["title"], json!("Yes"));
    assert_eq!(params["actions"][1]["title"], json!("No"));
    harness.respond(request.id, json!({"title": "Yes"})).await;

    let response = harness.recv_response(&id).await;
    assert_eq!(response.result, Some(json!({"title": "Yes"})));
}

#[tokio::test]
async fn show_document_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness.request("test/show_document", json!({})).await;

    let request = harness.recv_request("window/showDocument").await;
    let params = request.params.expect("params");
    assert_eq!(params["uri"], json!("file:///a"));
    assert_eq!(params["takeFocus"], json!(true));
    harness.respond(request.id, json!({"success": true})).await;

    let response = harness.recv_response(&id).await;
    assert_eq!(response.result, Some(json!({"success": true})));
}

#[tokio::test]
async fn register_and_unregister_capability_round_trip() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let register_id = harness.request("test/register_capability", json!({})).await;
    let register_request = harness.recv_request("client/registerCapability").await;
    let params = register_request.params.expect("params");
    assert_eq!(
        params["registrations"][0]["method"],
        json!("textDocument/formatting")
    );
    harness.respond(register_request.id, Value::Null).await;
    let response = harness.recv_response(&register_id).await;
    assert_eq!(response.result, Some(json!("registered")));

    let unregister_id = harness
        .request("test/unregister_capability", json!({}))
        .await;
    let unregister_request = harness.recv_request("client/unregisterCapability").await;
    let params = unregister_request.params.expect("params");
    // The spec's field name is missing an "r" -- verify the wire shape
    // matches it exactly, not the more natural "unregistrations".
    assert_eq!(params["unregisterations"][0]["id"], json!("reg-1"));
    assert!(params.get("unregistrations").is_none());
    harness.respond(unregister_request.id, Value::Null).await;
    let response = harness.recv_response(&unregister_id).await;
    assert_eq!(response.result, Some(json!("unregistered")));
}

#[tokio::test]
async fn client_capabilities_query_walks_dotted_paths() {
    let mut harness = Harness::start();
    let init_id = harness
        .request(
            "initialize",
            json!({
                "capabilities": {
                    "textDocument": {"hover": {"dynamicRegistration": true}},
                    "workspace": {"applyEdit": true},
                },
            }),
        )
        .await;
    harness.recv_response(&init_id).await;
    harness.notify("initialized", json!({})).await;

    let id = harness.request("test/capability_query", json!({})).await;
    let response = harness.recv_response(&id).await;
    let result = response.result.expect("result");
    assert_eq!(result["hoverSupported"], json!(true));
    assert_eq!(result["applyEditSupported"], json!(true));
    assert_eq!(result["definitionSupported"], json!(false));
    assert_eq!(result["applyEditValue"], json!(true));
}

#[tokio::test]
async fn document_symbol_accepts_progress_token_fields() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness
        .request(
            "textDocument/documentSymbol",
            json!({
                "textDocument": {"uri": "file:///a.txt"},
                "workDoneToken": "w1",
                "partialResultToken": "p1",
            }),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert_eq!(response.result.expect("result")[0]["name"], json!("main"));
}

#[tokio::test]
async fn workspace_folders_change_notification_is_routed() {
    let mut harness = Harness::start();
    harness.initialize().await;

    harness
        .notify(
            "workspace/didChangeWorkspaceFolders",
            json!({
                "event": {
                    "added": [{"uri": "file:///a", "name": "a"}],
                    "removed": [],
                }
            }),
        )
        .await;

    let note = harness.recv_notification("window/logMessage").await;
    assert_eq!(
        note.params.unwrap()["message"],
        json!("workspace folders changed: +1 -0")
    );
}

#[tokio::test]
async fn work_done_progress_cancel_notification_is_routed() {
    let mut harness = Harness::start();
    harness.initialize().await;

    harness
        .notify(
            "window/workDoneProgress/cancel",
            json!({ "token": "progress-1" }),
        )
        .await;

    let note = harness.recv_notification("window/logMessage").await;
    assert_eq!(
        note.params.unwrap()["message"],
        json!("progress cancelled: String(\"progress-1\")")
    );
}

#[tokio::test]
async fn malformed_json_body_gets_parse_error_and_connection_survives() {
    use tokio::io::AsyncWriteExt;

    let mut harness = Harness::start();
    harness.initialize().await;

    // A syntactically invalid JSON body behind a *correct* Content-Length
    // header: the frame boundary is intact, so this must not desynchronise
    // or kill the connection -- it should just produce a Parse error.
    let body = b"{not valid json}";
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    harness
        .to_server
        .write_all(header.as_bytes())
        .await
        .unwrap();
    harness.to_server.write_all(body).await.unwrap();
    harness.to_server.flush().await.unwrap();

    let parse_error = loop {
        if let Message::Response(response) = harness.recv().await
            && response.id.is_none()
        {
            break response;
        }
    };
    assert_eq!(
        parse_error.error.expect("parse error").code,
        codes::PARSE_ERROR
    );

    // The connection is still alive: a well-formed request right behind the
    // malformed one still gets a normal response.
    harness.open("file:///still-alive.txt", "hello").await;
    let id = harness
        .request(
            "textDocument/hover",
            position_params("file:///still-alive.txt", 0, 0),
        )
        .await;
    let response = harness.recv_response(&id).await;
    assert!(response.error.is_none());
}

#[tokio::test]
async fn panicking_handler_receives_internal_error_response() {
    let mut harness = Harness::start();
    harness.initialize().await;

    let id = harness.request("test/panic", json!({})).await;
    let response = tokio::time::timeout(Duration::from_secs(5), harness.recv_response(&id))
        .await
        .expect("an INTERNAL_ERROR response should arrive instead of hanging forever");
    assert_eq!(response.error.expect("error").code, codes::INTERNAL_ERROR);
}

#[tokio::test]
async fn eof_stops_the_server() {
    let harness = Harness::start();
    // Dropping the client write half closes the stream; the server should see
    // EOF at a frame boundary and return Ok.
    let Harness {
        to_server, serve, ..
    } = harness;
    drop(to_server);
    let outcome = tokio::time::timeout(Duration::from_secs(5), serve)
        .await
        .expect("server should stop on EOF")
        .expect("server task did not panic");
    assert!(outcome.is_ok());
}
