//! The [`Server`]: the runtime that drives an LSP connection.
//!
//! [`Server::serve`] takes ownership of a read/write byte stream, builds your
//! [`LanguageServer`] backend, and runs the message loop until the client
//! disconnects or sends `exit`. It owns everything protocol-related:
//!
//! - **Framing & JSON-RPC** via [`crate::transport`].
//! - **Lifecycle**: enforces `initialize` first, rejects work after `shutdown`,
//!   stops on `exit`.
//! - **Concurrency**: notifications run in receipt order (so document state
//!   stays consistent), while requests are spawned so slow handlers never block
//!   the loop.
//! - **Cancellation**: `$/cancelRequest` aborts the in-flight handler and
//!   replies with [`REQUEST_CANCELLED`](crate::error::codes::REQUEST_CANCELLED),
//!   with the bookkeeping arranged so a request is answered exactly once.

use crate::client::Client;
use crate::error::{Error, ResponseError, Result, codes};
use crate::jsonrpc::{Message, Notification, Request, RequestId, Response};
use crate::lsp::{
    CompletionParams, DefinitionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, HoverParams, InitializeParams,
    MessageType,
};
use crate::service::LanguageServer;
use crate::transport;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

/// Map of request ids to the abort handle of their running handler task.
type InFlight = Arc<Mutex<HashMap<RequestId, AbortHandle>>>;

/// Params of the `$/cancelRequest` notification.
#[derive(serde::Deserialize)]
struct CancelParams {
    id: RequestId,
}

/// An LSP server bound to a byte transport.
///
/// Construct with [`Server::new`] over any async reader/writer, or
/// [`Server::stdio`] for the conventional stdin/stdout transport, then call
/// [`serve`](Server::serve).
pub struct Server<R, W> {
    reader: R,
    writer: W,
}

impl Server<tokio::io::Stdin, tokio::io::Stdout> {
    /// Build a server over the process's standard input and output.
    ///
    /// This is the transport editors use when they launch a language server as
    /// a child process.
    pub fn stdio() -> Self {
        Server::new(tokio::io::stdin(), tokio::io::stdout())
    }
}

impl<R, W> Server<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    /// Build a server over an arbitrary reader and writer.
    pub fn new(reader: R, writer: W) -> Self {
        Server { reader, writer }
    }

    /// Run the server until the connection closes or the client sends `exit`.
    ///
    /// `build` receives a [`Client`] and returns the backend; store the client
    /// so handlers can talk back to the editor. Returns `Ok(())` on a clean
    /// shutdown, or an error if the transport failed irrecoverably.
    pub async fn serve<B, F>(self, build: F) -> Result<()>
    where
        B: LanguageServer,
        F: FnOnce(Client) -> B,
    {
        let Server { reader, mut writer } = self;

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let client = Client::new(out_tx.clone());
        let backend = Arc::new(build(client.clone()));

        // Single writer task: serializes all outbound traffic and is the sole
        // owner of the write half. It ends once every sender is dropped.
        let writer_task = tokio::spawn(async move {
            while let Some(message) = out_rx.recv().await {
                transport::write_message(&mut writer, &message).await?;
            }
            Ok::<(), Error>(())
        });

        let result = run_loop(reader, client, out_tx, backend).await;

        // `run_loop` has dropped its senders and aborted in-flight handlers on
        // return; await the writer so buffered output is flushed before exit.
        match writer_task.await {
            Ok(write_result) => result.and(write_result),
            Err(join_err) if join_err.is_panic() => {
                Err(Error::internal(format!("writer task panicked: {join_err}")))
            }
            Err(_) => result,
        }
    }
}

/// The message loop. Owns the loop-side handles so they drop on return.
async fn run_loop<R, B>(
    reader: R,
    client: Client,
    out_tx: mpsc::UnboundedSender<Message>,
    backend: Arc<B>,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    B: LanguageServer,
{
    let mut reader = transport::buffered(reader);
    let in_flight: InFlight = Arc::new(Mutex::new(HashMap::new()));
    let mut initialized = false;
    let mut shutdown_requested = false;

    let loop_result = loop {
        let message = match transport::read_message(&mut reader).await {
            Ok(Some(message)) => message,
            Ok(None) => break Ok(()), // clean EOF at a frame boundary
            // The frame itself was read intact (exactly `Content-Length` bytes
            // consumed) but its body was not valid JSON, or classified to an
            // invalid `Message` shape (e.g. a non-numeric/string id). The
            // stream is not desynchronised, so report a Parse error for this
            // message and keep the connection alive, per JSON-RPC/LSP.
            Err(Error::Serde(err)) => {
                let _ = out_tx.send(
                    Response::error(
                        None,
                        ResponseError {
                            code: codes::PARSE_ERROR,
                            message: err.to_string(),
                            data: None,
                        },
                    )
                    .into(),
                );
                continue;
            }
            Err(err) => {
                // A framing error (bad headers, mid-frame EOF, ...)
                // desynchronises the stream; we cannot safely continue.
                // Surface it to the caller.
                break Err(err);
            }
        };

        match message {
            Message::Response(response) => client.resolve(response),

            Message::Notification(note) => match note.method.as_str() {
                "exit" => break Ok(()),
                "$/cancelRequest" => cancel_request(&note, &in_flight, &out_tx),
                "initialized" => {
                    if initialized {
                        backend.initialized().await;
                    }
                }
                _ => {
                    // Per spec, drop notifications that arrive before
                    // `initialize` (other than `exit`, handled above).
                    if initialized {
                        dispatch_notification(backend.as_ref(), &client, &note.method, note.params)
                            .await;
                    }
                }
            },

            Message::Request(req) => match req.method.as_str() {
                "initialize" => {
                    if initialized {
                        send_error(
                            &out_tx,
                            req.id,
                            Error::invalid_request("server already initialized"),
                        );
                    } else {
                        let outcome = match parse_params::<InitializeParams>(req.params) {
                            Ok(params) => backend
                                .initialize(params)
                                .await
                                .and_then(|result| to_json(&result)),
                            Err(err) => Err(err),
                        };
                        match outcome {
                            Ok(value) => {
                                initialized = true;
                                let _ = out_tx.send(Response::success(req.id, value).into());
                            }
                            Err(err) => send_error(&out_tx, req.id, err),
                        }
                    }
                }
                "shutdown" => {
                    if !initialized {
                        send_error(&out_tx, req.id, Error::server_not_initialized());
                    } else {
                        shutdown_requested = true;
                        match backend.shutdown().await {
                            Ok(()) => {
                                let _ = out_tx.send(Response::success(req.id, Value::Null).into());
                            }
                            Err(err) => send_error(&out_tx, req.id, err),
                        }
                    }
                }
                _ => {
                    if !initialized {
                        send_error(&out_tx, req.id, Error::server_not_initialized());
                    } else if shutdown_requested {
                        send_error(
                            &out_tx,
                            req.id,
                            Error::invalid_request("server is shutting down"),
                        );
                    } else {
                        spawn_request(&backend, &out_tx, &in_flight, req);
                    }
                }
            },
        }
    };

    // Tear down: abort any handlers still running so their captured senders and
    // backend references drop, letting the writer task wind down.
    for (_, handle) in lock(&in_flight).drain() {
        handle.abort();
    }

    loop_result
}

/// Spawn a feature request handler as its own task.
///
/// The task, any concurrent `$/cancelRequest`, and the panic watcher below
/// race to remove the id from [`InFlight`]; whoever wins sends the single
/// response, guaranteeing a request is answered exactly once.
fn spawn_request<B: LanguageServer>(
    backend: &Arc<B>,
    out_tx: &mpsc::UnboundedSender<Message>,
    in_flight: &InFlight,
    req: Request,
) {
    let request_id = req.id;
    let method = req.method;
    let params = req.params;
    let backend = Arc::clone(backend);
    let handler_out_tx = out_tx.clone();
    let in_flight_for_task = Arc::clone(in_flight);
    let response_id = request_id.clone();

    let join = tokio::spawn(async move {
        let outcome = dispatch_request(backend.as_ref(), &method, params).await;
        // Claim the right to respond. If the entry is already gone, a cancel
        // beat us to it and has sent the cancellation response.
        if lock(&in_flight_for_task).remove(&response_id).is_some() {
            let response = match outcome {
                Ok(value) => Response::success(response_id, value),
                Err(err) => Response::error(Some(response_id), err.into_response_error()),
            };
            let _ = handler_out_tx.send(response.into());
        }
    });

    lock(in_flight).insert(request_id.clone(), join.abort_handle());

    // A handler that panics unwinds before it can claim its `InFlight` entry
    // or send a response, which would otherwise leave the request answered
    // never. Watch the join and, if it failed because the handler panicked
    // (not because a cancel aborted it first), answer with INTERNAL_ERROR.
    let in_flight_for_watch = Arc::clone(in_flight);
    let watch_out_tx = out_tx.clone();
    tokio::spawn(async move {
        if let Err(join_err) = join.await
            && join_err.is_panic()
            && lock(&in_flight_for_watch).remove(&request_id).is_some()
        {
            let _ = watch_out_tx.send(
                Response::error(
                    Some(request_id),
                    Error::internal(format!("handler panicked: {join_err}")).into_response_error(),
                )
                .into(),
            );
        }
    });
}

/// Handle `$/cancelRequest`: abort the named handler and reply with a
/// cancellation error, but only if the request is still in flight.
fn cancel_request(
    note: &Notification,
    in_flight: &InFlight,
    out_tx: &mpsc::UnboundedSender<Message>,
) {
    let Some(params) = note.params.clone() else {
        return;
    };
    let Ok(CancelParams { id }) = serde_json::from_value(params) else {
        return;
    };
    // Removing here both stops the handler and prevents it from later sending
    // its own response (it will find its entry already gone).
    if let Some(handle) = lock(in_flight).remove(&id) {
        handle.abort();
        let _ = out_tx.send(
            Response::error(Some(id), Error::request_cancelled().into_response_error()).into(),
        );
    }
}

/// Route a feature request to the matching typed handler, falling back to the
/// untyped [`LanguageServer::handle_request`] escape hatch.
async fn dispatch_request<B: LanguageServer>(
    backend: &B,
    method: &str,
    params: Option<Value>,
) -> Result<Value> {
    match method {
        "textDocument/hover" => {
            to_json(&backend.hover(parse_params::<HoverParams>(params)?).await?)
        }
        "textDocument/completion" => to_json(
            &backend
                .completion(parse_params::<CompletionParams>(params)?)
                .await?,
        ),
        "textDocument/definition" => to_json(
            &backend
                .definition(parse_params::<DefinitionParams>(params)?)
                .await?,
        ),
        _ => backend.handle_request(method, params).await,
    }
}

/// Route a notification to the matching typed handler, falling back to the
/// untyped [`LanguageServer::handle_notification`] escape hatch.
///
/// Malformed params for a known notification are logged (notifications have no
/// response channel) rather than silently dropped.
async fn dispatch_notification<B: LanguageServer>(
    backend: &B,
    client: &Client,
    method: &str,
    params: Option<Value>,
) {
    match method {
        "textDocument/didOpen" => match parse_params::<DidOpenTextDocumentParams>(params) {
            Ok(p) => backend.did_open(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "textDocument/didChange" => match parse_params::<DidChangeTextDocumentParams>(params) {
            Ok(p) => backend.did_change(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "textDocument/didClose" => match parse_params::<DidCloseTextDocumentParams>(params) {
            Ok(p) => backend.did_close(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "textDocument/didSave" => match parse_params::<DidSaveTextDocumentParams>(params) {
            Ok(p) => backend.did_save(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        _ => backend.handle_notification(method, params).await,
    }
}

/// Log a parameter-decoding failure for a notification.
fn log_bad_params(client: &Client, method: &str, err: &Error) {
    let _ = client.log_message(
        MessageType::Warning,
        format!("ignoring `{method}` with invalid params: {err}"),
    );
}

/// Deserialize request/notification params, mapping failures to
/// [`Error::invalid_params`]. Per JSON-RPC, `params` is optional; absent
/// params are treated as an empty JSON object so structs whose fields are
/// all optional/defaultable still decode successfully.
fn parse_params<T: DeserializeOwned>(params: Option<Value>) -> Result<T> {
    serde_json::from_value(params.unwrap_or_else(|| Value::Object(Default::default())))
        .map_err(|e| Error::invalid_params(e.to_string()))
}

/// Serialize a handler result into the JSON value sent on the wire.
fn to_json<T: Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(Error::from)
}

/// Enqueue an error response for `id`.
fn send_error(out_tx: &mpsc::UnboundedSender<Message>, id: RequestId, err: Error) {
    let _ = out_tx.send(Response::error(Some(id), err.into_response_error()).into());
}

/// Lock a mutex, recovering the guard if a panicking task poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, PartialEq, Deserialize)]
    struct AllOptionalParams {
        #[serde(default)]
        foo: Option<i32>,
    }

    #[test]
    fn parse_params_treats_absent_params_as_empty_object() {
        let parsed: AllOptionalParams = parse_params(None).expect("absent params should default");
        assert_eq!(parsed, AllOptionalParams::default());
    }

    #[test]
    fn parse_params_still_rejects_wrong_shape() {
        let err = parse_params::<AllOptionalParams>(Some(Value::Bool(true)))
            .expect_err("a bool is not a valid params object");
        assert!(matches!(err, Error::Response(e) if e.code == crate::error::codes::INVALID_PARAMS));
    }
}
