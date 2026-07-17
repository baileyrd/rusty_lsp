# Changelog

All notable changes to `rusty_lsp` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) (0.x: minor bumps may break).

## [0.4.0] — 2026-07-17

### Added

- `textDocument/documentHighlight` gains `LocationLink` company: the goto
  family (`definition`/`declaration`/`typeDefinition`/`implementation`) can
  now return `GotoDefinitionResponse::Links(Vec<LocationLink>)` for clients
  advertising `linkSupport`.
- `TextDocumentSyncOptions`/`SaveOptions`: the full form of the
  `textDocumentSync` capability (open/close, will-save, save-with-text).
- `SemanticTokensBuilder`: builds the spec's relative-encoded token array
  from absolute positions, with legend name resolution, document-order
  sorting, and multi-line range splitting (`push_range`).
- `Documents::uris()`, `len()`, `is_empty()`, and `for_each()` for
  enumerating the open set (e.g. for `workspace/diagnostic`).
- `Client::begin_progress_for(&WorkDoneProgressParams, …)`: progress on the
  client-supplied `workDoneToken`, no `create` round trip.
- `Client::config_section::<T>(section, scope)`: one-section, typed
  `workspace/configuration`.
- `Position` implements `PartialOrd`/`Ord` (line, then character);
  `Range::contains`, `Range::overlaps`, `Range::intersection` (half-open
  semantics).
- `TestClient::open`/`change_full`/`close` document-lifecycle helpers.
- Direct unit tests for the transport framing layer.
- The example server implements `documentHighlight` via
  `Documents::offset_at`.

### Changed

- **Breaking**: notifications now run on a dedicated serialized worker
  instead of inline on the message loop. Receipt order is preserved, and
  requests still observe every notification received before them (they
  synchronize on a completion watermark) — but `$/cancelRequest` handling
  and response delivery are no longer delayed by slow notification
  handlers. A panicking notification handler no longer tears down the
  server; it is caught and logged.
- **Breaking**: `ServerCapabilities::text_document_sync` is now
  `Option<TextDocumentSyncCapability>` (`Kind | Options`); a bare
  `TextDocumentSyncKind` converts with `.into()`.
- `shutdown` waits for already-received notifications to land before the
  backend's `shutdown` handler runs.

## [0.3.0] — 2026-07-17

### Added

- Typed `textDocument/documentHighlight` (method, types, capability).
- Fully modelled `CompletionItem` (label details, tags, snippets,
  `textEdit`/`InsertReplaceEdit`, `additionalTextEdits`, `command`, resolve
  `data`) and `CompletionList::item_defaults` (LSP 3.17).
- `Diagnostic` rich fields: `tags`, `relatedInformation`,
  `codeDescription`, `data`, with builder helpers.
- Typed dynamic registration: `DocumentFilter`/`DocumentSelector`/
  `TextDocumentRegistrationOptions`, `Registration::for_documents`.
- Typed `WorkspaceEdit::document_changes`: `TextDocumentEdit`,
  `CreateFile`/`RenameFile`/`DeleteFile`, `changeAnnotations`.
- `Error::content_modified()`, `Error::server_cancelled()`,
  `Error::request_failed()`.
- `Documents::offset_at`/`position_at`/`with_index` over a per-document
  cached `LineIndex`.
- `text::apply_edits`/`apply_edits_with`: atomic, overlap-checked batch
  edit application.
- `TestClient` receive timeouts (default 10s, `with_timeout`).
- CI runs `--all-features` and builds docs with `-D warnings`;
  `rust-version = "1.85"` and `readme` declared in the manifest.

### Changed

- **Breaking**: `CompletionItem::documentation` is now
  `Option<Documentation>` (string or markup).
- **Breaking**: `exit` without a prior `shutdown` makes `Server::serve`
  return an error, so a `Result` main exits with code 1 per the spec.
- **Breaking**: fields were added to public structs (`CompletionItem`,
  `Diagnostic`, `CompletionList`, `WorkspaceEdit`); struct literals must
  use `..Default::default()` or the builders.

## [0.2.0] — 2026-07-17

### Added

- Typed methods for `textDocument/moniker`, `linkedEditingRange`,
  `inlineValue`, and `inlineCompletion` (3.18 proposed), with matching
  capabilities.
- `workspace/symbol` 3.17 form (`WorkspaceSymbol`, URI-only locations) and
  `workspaceSymbol/resolve`.
- `Client` helpers: `workspace_folders()`, `telemetry_event()`,
  `refresh_folding_ranges()`, `refresh_inline_values()`,
  `send_request_with_timeout`, `begin_progress` (RAII `ProgressGuard`).
- Cooperative cancellation: `rusty_lsp::cancel::current()` exposes a
  `CancelToken` tripped by `$/cancelRequest` before the task abort.
- `Server::with_max_concurrent_requests` and
  `Server::with_outbound_queue_limit`.
- `Documents::with_encoding`, `Documents::with`, stale-version guarding;
  `text::LineIndex`.
- `testing::TestClient` in-memory harness; `tcp` and `tracing` cargo
  features; `InitializeParams::workspace_roots()`.

### Changed

- **Breaking**: `Uri` is a normalizing newtype (was a `String` alias).
- **Breaking**: `symbol()` returns `Option<WorkspaceSymbolResponse>`;
  `workspace_symbol_provider` accepts `bool | WorkspaceSymbolOptions`.
- Handler panics are caught in-task; panic responses no longer race a
  watcher task.

## [0.1.1] — baseline

Initial framework: JSON-RPC framing and dispatch, the `LanguageServer`
trait with typed handlers for the core protocol, lifecycle enforcement,
abort-based request cancellation, `Client` handle, optional `Documents`
store, and UTF-8/16/32 position conversion utilities.
