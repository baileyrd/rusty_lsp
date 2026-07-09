//! End-to-end tests driving a real [`Server`] over in-memory duplex pipes.
//!
//! Each test starts a server task wired to a [`TestBackend`], then speaks the
//! framed JSON-RPC protocol over the pipe exactly as an editor would. This
//! exercises the whole stack: framing, message classification, dispatch,
//! lifecycle enforcement, server→client notifications, and cancellation.

use rusty_lsp::error::{Result, codes};
use rusty_lsp::jsonrpc::{Message, Notification, Request, RequestId, Response};
use rusty_lsp::lsp::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    ConfigurationItem, Diagnostic, DiagnosticSeverity, DidChangeWorkspaceFoldersParams,
    DidOpenTextDocumentParams, Hover, HoverParams, InitializeParams, InitializeResult, Position,
    Range, ServerCapabilities, ServerInfo, TextDocumentSyncKind, TextEdit, WorkDoneProgressBegin,
    WorkDoneProgressCancelParams, WorkDoneProgressEnd, WorkDoneProgressReport, WorkspaceEdit,
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
}

impl TestBackend {
    fn new(client: Client) -> Self {
        TestBackend {
            client,
            documents: RwLock::new(HashMap::new()),
        }
    }
}

impl LanguageServer for TestBackend {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
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
            other => Err(Error::method_not_found(other.to_owned())),
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let _ = self.client.log_message(
            rusty_lsp::lsp::MessageType::Info,
            format!(
                "workspace folders changed: +{} -{}",
                params.event.added.len(),
                params.event.removed.len()
            ),
        );
    }

    async fn work_done_progress_cancel(&self, params: WorkDoneProgressCancelParams) {
        let _ = self.client.log_message(
            rusty_lsp::lsp::MessageType::Info,
            format!("progress cancelled: {:?}", params.token),
        );
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
async fn unknown_method_yields_method_not_found() {
    let mut harness = Harness::start();
    harness.initialize().await;
    let id = harness.request("textDocument/formatting", json!({})).await;
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
