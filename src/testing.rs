//! An in-memory test harness for backends built on this framework.
//!
//! [`TestClient`] plays the editor's role: it spawns your
//! [`LanguageServer`] on a real [`crate::Server`] over in-memory pipes and
//! speaks framed JSON-RPC to it, so tests exercise the full stack (framing,
//! dispatch, lifecycle, cancellation) rather than calling handler methods
//! directly.
//!
//! ```rust,no_run
//! use rusty_lsp::error::Result;
//! use rusty_lsp::lsp::{Hover, InitializeParams, InitializeResult};
//! use rusty_lsp::testing::TestClient;
//! use rusty_lsp::{Client, LanguageServer};
//!
//! struct Backend;
//! impl LanguageServer for Backend {
//!     async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
//!         Ok(InitializeResult::default())
//!     }
//! }
//!
//! # async fn example() -> Result<()> {
//! let mut client = TestClient::spawn(|_client: Client| Backend);
//! client.initialize(InitializeParams::default()).await?;
//! client.notify("textDocument/didOpen", serde_json::json!({
//!     "textDocument": {
//!         "uri": "file:///a.txt", "languageId": "plaintext",
//!         "version": 1, "text": "hello",
//!     }
//! })).await?;
//! let hover: Option<Hover> = client.request("textDocument/hover", serde_json::json!({
//!     "textDocument": {"uri": "file:///a.txt"},
//!     "position": {"line": 0, "character": 0},
//! })).await?;
//! client.shutdown_and_exit().await?;
//! # Ok(())
//! # }
//! ```
//!
//! Reads are demand-driven and single-threaded: nothing is consumed from the
//! server until you call a `recv_*`/`request` method, and messages that
//! arrive while waiting for something specific are buffered, not lost.
//! If a handler under test calls the server-side [`crate::Client`] request
//! API (e.g. `workspace/configuration`), answer it between sending the
//! request and awaiting its response: use [`TestClient::start_request`],
//! then [`TestClient::recv_request`] + [`TestClient::respond`], then
//! [`TestClient::response`].

use crate::client::Client;
use crate::error::{Error, Result};
use crate::jsonrpc::{Message, Notification, Request, RequestId, Response};
use crate::lsp::{InitializeParams, InitializeResult};
use crate::server::Server;
use crate::service::LanguageServer;
use crate::transport;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::VecDeque;
use tokio::io::{BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio::task::JoinHandle;

/// An in-memory LSP client driving a spawned [`crate::Server`].
pub struct TestClient {
    writer: WriteHalf<DuplexStream>,
    reader: BufReader<ReadHalf<DuplexStream>>,
    /// Messages read while scanning for something else, oldest first.
    buffered: VecDeque<Message>,
    next_id: i64,
    server: JoinHandle<Result<()>>,
}

impl TestClient {
    /// Spawn `build`'s backend on a server over in-memory pipes and return
    /// the client end. The server runs as a background task until
    /// [`shutdown_and_exit`](Self::shutdown_and_exit) (or drop, which severs
    /// the pipe).
    pub fn spawn<B, F>(build: F) -> Self
    where
        B: LanguageServer,
        F: FnOnce(Client) -> B + Send + 'static,
    {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(async move { Server::new(server_read, server_write).serve(build).await });
        let (client_read, client_write) = tokio::io::split(client_io);
        TestClient {
            writer: client_write,
            reader: transport::buffered(client_read),
            buffered: VecDeque::new(),
            next_id: 0,
            server,
        }
    }

    /// Send a raw [`Message`] to the server.
    pub async fn send(&mut self, message: Message) -> Result<()> {
        transport::write_message(&mut self.writer, &message).await
    }

    /// Send a notification with typed params.
    pub async fn notify<P: Serialize>(&mut self, method: &str, params: P) -> Result<()> {
        let params = serde_json::to_value(params)?;
        self.send(Message::Notification(Notification {
            method: method.to_owned(),
            params: Some(params),
        }))
        .await
    }

    /// Send a request without awaiting its response; returns the id to pass
    /// to [`response`](Self::response) later. Use this (instead of
    /// [`request`](Self::request)) when the handler will call back into the
    /// client — e.g. `workspace/configuration` — so you can answer that
    /// callback before awaiting the response.
    pub async fn start_request<P: Serialize>(
        &mut self,
        method: &str,
        params: P,
    ) -> Result<RequestId> {
        self.next_id += 1;
        let id = RequestId::Number(self.next_id);
        self.send(Message::Request(Request {
            id: id.clone(),
            method: method.to_owned(),
            params: Some(serde_json::to_value(params)?),
        }))
        .await?;
        Ok(id)
    }

    /// Send a request and await its typed result. An error response becomes
    /// [`Error::Response`].
    pub async fn request<P, R>(&mut self, method: &str, params: P) -> Result<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let id = self.start_request(method, params).await?;
        let response = self.response(&id).await?;
        if let Some(error) = response.error {
            return Err(Error::Response(error));
        }
        Ok(serde_json::from_value(
            response.result.unwrap_or(Value::Null),
        )?)
    }

    /// Read until the response for `id` arrives; other messages are buffered
    /// for later `recv_*` calls.
    pub async fn response(&mut self, id: &RequestId) -> Result<Response> {
        if let Some(index) = self
            .buffered
            .iter()
            .position(|m| matches!(m, Message::Response(r) if r.id.as_ref() == Some(id)))
        {
            let Some(Message::Response(response)) = self.buffered.remove(index) else {
                unreachable!("position matched a response");
            };
            return Ok(response);
        }
        loop {
            match self.read().await? {
                Message::Response(response) if response.id.as_ref() == Some(id) => {
                    return Ok(response);
                }
                other => self.buffered.push_back(other),
            }
        }
    }

    /// Read until a notification with `method` arrives; other messages are
    /// buffered.
    pub async fn recv_notification(&mut self, method: &str) -> Result<Notification> {
        if let Some(index) = self
            .buffered
            .iter()
            .position(|m| matches!(m, Message::Notification(n) if n.method == method))
        {
            let Some(Message::Notification(note)) = self.buffered.remove(index) else {
                unreachable!("position matched a notification");
            };
            return Ok(note);
        }
        loop {
            match self.read().await? {
                Message::Notification(note) if note.method == method => return Ok(note),
                other => self.buffered.push_back(other),
            }
        }
    }

    /// Read until a server→client request with `method` arrives; other
    /// messages are buffered.
    pub async fn recv_request(&mut self, method: &str) -> Result<Request> {
        if let Some(index) = self
            .buffered
            .iter()
            .position(|m| matches!(m, Message::Request(r) if r.method == method))
        {
            let Some(Message::Request(request)) = self.buffered.remove(index) else {
                unreachable!("position matched a request");
            };
            return Ok(request);
        }
        loop {
            match self.read().await? {
                Message::Request(request) if request.method == method => return Ok(request),
                other => self.buffered.push_back(other),
            }
        }
    }

    /// Answer a server→client request with a success result.
    pub async fn respond<T: Serialize>(&mut self, id: RequestId, result: T) -> Result<()> {
        let result = serde_json::to_value(result)?;
        self.send(Message::Response(Response::success(id, result)))
            .await
    }

    /// Send `$/cancelRequest` for an in-flight request.
    pub async fn cancel(&mut self, id: &RequestId) -> Result<()> {
        self.notify("$/cancelRequest", serde_json::json!({ "id": id }))
            .await
    }

    /// Drive the `initialize` request and `initialized` notification,
    /// returning the server's capabilities.
    pub async fn initialize(&mut self, params: InitializeParams) -> Result<InitializeResult> {
        let result: InitializeResult = self.request("initialize", params).await?;
        self.notify("initialized", serde_json::json!({})).await?;
        Ok(result)
    }

    /// Drive the `shutdown` request and `exit` notification, then await the
    /// server task's own result — the full clean-teardown path.
    pub async fn shutdown_and_exit(mut self) -> Result<()> {
        let _: Value = self.request("shutdown", Value::Null).await?;
        self.notify("exit", serde_json::json!({})).await?;
        match self.server.await {
            Ok(result) => result,
            Err(join_err) => Err(Error::internal(format!("server task failed: {join_err}"))),
        }
    }

    /// Read one message off the wire.
    async fn read(&mut self) -> Result<Message> {
        transport::read_message(&mut self.reader)
            .await?
            .ok_or_else(|| Error::protocol("server closed the connection"))
    }
}
