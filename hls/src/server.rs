//! The LSP server: lifecycle, request/notification dispatch, and document
//! analysis plumbing. Uses `lsp-server`'s stdio `Connection` (the same
//! foundation as rust-analyzer).

use std::collections::HashMap;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest, Request as _};
use lsp_types::{
    CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, InitializeResult, OneOf, Position,
    PublishDiagnosticsParams, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions,
};

use crate::analysis::Analysis;
use crate::diagnostics::diagnostics;
use crate::document::{position_to_offset, DocumentManager};

const SERVER_NAME: &str = "hls";
const SERVER_VERSION: &str = "0.1.0";

/// Server state: open documents plus a cached symbol analysis per URI.
#[derive(Default)]
struct State {
    docs: DocumentManager,
    analysis: HashMap<String, Analysis>,
}

/// Blocking entry point: runs the LSP loop until `exit`.
pub fn run() -> i32 {
    let (connection, io_threads) = Connection::stdio();

    // ── Initialize handshake ─────────────────────────────────────────
    let (initialize_id, init_params) = match connection.initialize_start() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{SERVER_NAME}: initialize failed: {e}");
            return 1;
        }
    };
    let _init: lsp_types::InitializeParams =
        serde_json::from_value(init_params).unwrap_or_default();

    let result = InitializeResult {
        capabilities: server_capabilities(),
        server_info: Some(ServerInfo {
            name: SERVER_NAME.into(),
            version: Some(SERVER_VERSION.into()),
        }),
    };
    if let Err(e) = connection
        .initialize_finish(initialize_id, serde_json::to_value(result).unwrap())
    {
        eprintln!("{SERVER_NAME}: initialize_finish failed: {e}");
        return 1;
    }

    let mut state = State::default();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if req.method == "shutdown" {
                    let resp = Response::new_ok(req.id.clone(), ());
                    let _ = connection.sender.send(resp.into());
                    continue;
                }
                if let Err(e) = handle_request(&connection, &mut state, req) {
                    eprintln!("{SERVER_NAME}: request handler error: {e}");
                }
            }
            Message::Notification(not) => {
                if not.method == "exit" {
                    break;
                }
                handle_notification(&connection, &mut state, not);
            }
            Message::Response(_) => {}
        }
    }

    // Drop the connection (and its writer-side channel sender) *before*
    // joining the IO threads: the writer thread only terminates once every
    // sender clone is gone, so keeping `connection` alive here would deadlock
    // the join and leave the process running after `exit`. Buffered messages
    // (e.g. final publishDiagnostics) are still flushed by the writer.
    drop(connection);
    let _ = io_threads.join();
    0
}

/// Advertised LSP capabilities.
fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::INCREMENTAL),
            will_save: None,
            will_save_wait_until: None,
            save: None,
        })),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        completion_provider: Some(lsp_types::CompletionOptions {
            trigger_characters: Some(vec![".".into(), "_".into(), "$".into()]),
            ..Default::default()
        }),
        document_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

fn handle_request(
    connection: &Connection,
    state: &mut State,
    req: Request,
) -> Result<(), Box<dyn std::error::Error>> {
    match req.method.as_str() {
        HoverRequest::METHOD => {
            let (id, params) = extract::<HoverParams>(req, HoverRequest::METHOD)?;
            let result = state
                .hover(&params)
                .map(|h| serde_json::to_value(h))
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            send_ok(connection, id, result);
        }
        GotoDefinition::METHOD => {
            let (id, params) = extract::<GotoDefinitionParams>(req, GotoDefinition::METHOD)?;
            let result = state
                .definition(&params)
                .map(|d| serde_json::to_value(d))
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            send_ok(connection, id, result);
        }
        Completion::METHOD => {
            let (id, params) = extract::<CompletionParams>(req, Completion::METHOD)?;
            let result = state
                .completions(&params)
                .map(|c| serde_json::to_value(c))
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            send_ok(connection, id, result);
        }
        DocumentSymbolRequest::METHOD => {
            let (id, params) =
                extract::<DocumentSymbolParams>(req, DocumentSymbolRequest::METHOD)?;
            let result = state
                .document_symbols(&params)
                .map(|d| serde_json::to_value(d))
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            send_ok(connection, id, result);
        }
        _ => {
            let resp = Response::new_err(
                req.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                format!("method not found: {}", req.method),
            );
            let _ = connection.sender.send(resp.into());
        }
    }
    Ok(())
}

fn extract<P: serde::de::DeserializeOwned>(
    req: Request,
    method: &str,
) -> Result<(RequestId, P), Box<dyn std::error::Error>> {
    let (id, params) = req
        .extract::<P>(method)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    Ok((id, params))
}

fn send_ok(connection: &Connection, id: RequestId, result: serde_json::Value) {
    let resp = Response::new_ok(id, result);
    let _ = connection.sender.send(resp.into());
}

fn handle_notification(connection: &Connection, state: &mut State, not: Notification) {
    match not.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let params: DidOpenTextDocumentParams = match serde_json::from_value(not.params) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{SERVER_NAME}: bad didOpen: {e}");
                    return;
                }
            };
            state.docs.open(
                params.text_document.uri.clone(),
                params.text_document.version,
                params.text_document.text,
            );
            state.analyze(&params.text_document.uri);
            publish(connection, state, &params.text_document.uri);
        }
        DidChangeTextDocument::METHOD => {
            let params: DidChangeTextDocumentParams = match serde_json::from_value(not.params) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{SERVER_NAME}: bad didChange: {e}");
                    return;
                }
            };
            state.docs.change(
                &params.text_document.uri,
                params.text_document.version,
                &params.content_changes,
            );
            state.analyze(&params.text_document.uri);
            publish(connection, state, &params.text_document.uri);
        }
        DidCloseTextDocument::METHOD => {
            let params: DidCloseTextDocumentParams = match serde_json::from_value(not.params) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{SERVER_NAME}: bad didClose: {e}");
                    return;
                }
            };
            // Clear any outstanding diagnostics.
            let clear = PublishDiagnosticsParams::new(
                params.text_document.uri.clone(),
                vec![],
                None,
            );
            let not = Notification::new(PublishDiagnostics::METHOD.to_string(), clear);
            let _ = connection.sender.send(not.into());
            state.docs.close(&params.text_document.uri);
            state.analysis.remove(params.text_document.uri.as_str());
        }
        // Ignored notifications (initialized, change configuration, etc.).
        _ => {}
    }
}

/// Run diagnostics for a document and publish them.
fn publish(connection: &Connection, state: &State, uri: &lsp_types::Uri) {
    let Some(doc) = state.docs.get(uri) else {
        return;
    };
    // Project-model context for import resolution + the `main`-file gate.
    // `None` (non-file URIs) keeps the legacy single-file behavior.
    let path = crate::document::uri_to_path(uri);
    let diags = diagnostics(&doc.text, path.as_deref());
    let params = PublishDiagnosticsParams::new(uri.clone(), diags, Some(doc.version));
    let not = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
    let _ = connection.sender.send(not.into());
}
impl State {
    /// Re-run analysis for a document, replacing the cached symbol table.
    fn analyze(&mut self, uri: &lsp_types::Uri) {
        let Some(doc) = self.docs.get(uri).cloned() else {
            return;
        };
        let mut analysis = Analysis::default();
        let out = compiler::lexer::lex(&doc.text);
        if let Ok(prog) = compiler::parse::parse(out.tokens, doc.text.clone()) {
            analysis = Analysis::from_program(&prog);
        }
        self.analysis.insert(uri.as_str().to_string(), analysis);
    }

    fn hover(&self, params: &HoverParams) -> Option<lsp_types::Hover> {
        let (doc, offset) = self.doc_at(
            &params.text_document_position_params.text_document.uri,
            &params.text_document_position_params.position,
        )?;
        let a = self.analysis.get(doc.uri.as_str())?;
        a.hover(&doc.text, offset)
    }

    fn definition(&self, params: &GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let (doc, offset) = self.doc_at(
            &params.text_document_position_params.text_document.uri,
            &params.text_document_position_params.position,
        )?;
        let a = self.analysis.get(doc.uri.as_str())?;
        a.definition(&doc.text, &doc.uri, offset)
            .map(GotoDefinitionResponse::Scalar)
    }

    fn completions(&self, params: &CompletionParams) -> Option<CompletionResponse> {
        let (doc, offset) = self.doc_at(
            &params.text_document_position.text_document.uri,
            &params.text_document_position.position,
        )?;
        let a = self.analysis.get(doc.uri.as_str())?;
        let items = a.completions(&doc.text, offset);
        Some(CompletionResponse::Array(items))
    }

    fn document_symbols(&self, params: &DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        let doc = self.docs.get(&params.text_document.uri)?;
        let a = self.analysis.get(doc.uri.as_str())?;
        let symbols = a.document_symbols(&doc.text);
        Some(DocumentSymbolResponse::Nested(symbols))
    }

    fn doc_at(
        &self,
        uri: &lsp_types::Uri,
        pos: &Position,
    ) -> Option<(crate::document::Document, usize)> {
        let doc = self.docs.get(uri)?.clone();
        let offset = position_to_offset(&doc.text, pos);
        Some((doc, offset))
    }
}
