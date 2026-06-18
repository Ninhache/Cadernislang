//! `cdc-lsp` — serveur Language Server pour cadernislang (LSP via stdin/stdout).
//!
//! Fournit : diagnostics live (parse + sema), complétions (mots-clés/builtins/noms déclarés), et
//! **inlay hints du coût PA/PM par `tour`** (le « PA inline » demandé). Toute l'analyse vit dans
//! [`analysis`] (testable) ; ce fichier n'est qu'un adaptateur tower-lsp.

mod analysis;

use std::collections::HashMap;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct Backend {
    client: Client,
    docs: Mutex<HashMap<Url, String>>,
}

impl Backend {
    async fn publish(&self, uri: Url, text: &str) {
        let a = analysis::analyze(text);
        let diags = a
            .diags
            .iter()
            .map(|d| {
                let line = d.line.saturating_sub(1);
                let start = Position::new(line, d.col.saturating_sub(1));
                let end = Position::new(line, d.col);
                let msg = match &d.code {
                    Some(c) => format!("[{c}] {}", d.msg),
                    None => d.msg.clone(),
                };
                Diagnostic::new_simple(Range::new(start, end), msg)
            })
            .collect();
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    async fn text_of(&self, uri: &Url) -> String {
        self.docs.lock().await.get(uri).cloned().unwrap_or_default()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions::default()),
                inlay_hint_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "cdc-lsp".to_string(),
                version: None,
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "cdc-lsp prêt — gg wp")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, p: DidOpenTextDocumentParams) {
        let uri = p.text_document.uri.clone();
        let text = p.text_document.text;
        self.docs.lock().await.insert(uri.clone(), text.clone());
        self.publish(uri, &text).await;
    }

    async fn did_change(&self, mut p: DidChangeTextDocumentParams) {
        let uri = p.text_document.uri.clone();
        if let Some(change) = p.content_changes.pop() {
            self.docs
                .lock()
                .await
                .insert(uri.clone(), change.text.clone());
            self.publish(uri, &change.text).await;
        }
    }

    async fn completion(&self, p: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = p.text_document_position.text_document.uri;
        let text = self.text_of(&uri).await;
        let items = analysis::completion_labels(&text)
            .into_iter()
            .map(|l| CompletionItem::new_simple(l, String::new()))
            .collect();
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn inlay_hint(&self, p: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = p.text_document.uri;
        let text = self.text_of(&uri).await;
        let a = analysis::analyze(&text);
        let mut hints = Vec::new();
        if let Some(r) = a.report {
            for t in r.tours {
                let label = if t.dynamic {
                    "tour: coût dynamique".to_string()
                } else {
                    format!("{}/{} PA · {}/{} PM", t.pa, r.max_pa, t.pm, r.max_pm)
                };
                hints.push(InlayHint {
                    position: Position::new(t.line.saturating_sub(1), t.col.saturating_sub(1)),
                    label: InlayHintLabel::String(label),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    data: None,
                });
            }
        }
        Ok(Some(hints))
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        docs: Mutex::new(HashMap::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
