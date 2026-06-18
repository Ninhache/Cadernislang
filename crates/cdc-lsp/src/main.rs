//! `cdc-lsp` — serveur Language Server pour cadernislang (LSP via stdin/stdout).
//!
//! Fournit : diagnostics live (parse + sema), complétions, **hover** (coût PA d'un `bot` + ✅/❌,
//! usage PA/PM d'un `tour`) et **go-to-definition** (bot/pano/perso). Toute l'analyse vit dans
//! [`analysis`] (testable) ; ce fichier n'est qu'un adaptateur tower-lsp. (La coloration
//! syntaxique n'est PAS gérée par le LSP : c'est une grammaire TextMate de l'extension.)

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
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
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

    async fn hover(&self, p: HoverParams) -> Result<Option<Hover>> {
        let pos = p.text_document_position_params.position;
        let uri = p.text_document_position_params.text_document.uri;
        let text = self.text_of(&uri).await;
        let word = match word_at(&text, pos) {
            Some(w) => w,
            None => return Ok(None),
        };
        let sy = analysis::symbols(&text);
        // hover sur un nom de bot → coût PA + ✅/❌ (tient-il dans un tour ?)
        if let Some((_, _, cost)) = sy.bots.get(&word) {
            let mark = if *cost <= sy.max_pa {
                "✅".to_string()
            } else {
                format!("❌ (dépasse le budget d'un tour : max {} PA)", sy.max_pa)
            };
            let md = format!("**bot `{word}`** — coût **{cost} PA** {mark}");
            return Ok(Some(hover_md(md)));
        }
        // hover sur le mot-clé `tour` → usage PA/PM du tour + ✅/❌
        if word == "tour" {
            let cur = pos.line + 1; // 1-based
            if let Some(t) = sy
                .tours
                .iter()
                .filter(|t| t.line >= cur)
                .min_by_key(|t| t.line)
            {
                let md = if t.dynamic {
                    "**tour** — coût **dynamique** (boucle/tour imbriqué → vérifié au runtime)"
                        .to_string()
                } else {
                    let ok = t.pa <= sy.max_pa && t.pm <= sy.max_pm;
                    let mark = if ok { "✅" } else { "❌ dépasse le budget" };
                    format!(
                        "**tour** — **{}/{} PA** · **{}/{} PM** {mark}",
                        t.pa, sy.max_pa, t.pm, sy.max_pm
                    )
                };
                return Ok(Some(hover_md(md)));
            }
        }
        Ok(None)
    }

    async fn goto_definition(
        &self,
        p: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = p.text_document_position_params.position;
        let uri = p.text_document_position_params.text_document.uri;
        let text = self.text_of(&uri).await;
        let word = match word_at(&text, pos) {
            Some(w) => w,
            None => return Ok(None),
        };
        let sy = analysis::symbols(&text);
        let def = sy
            .bots
            .get(&word)
            .map(|(l, c, _)| (*l, *c))
            .or_else(|| sy.types.get(&word).copied());
        if let Some((line, col)) = def {
            let p0 = Position::new(line.saturating_sub(1), col.saturating_sub(1));
            let range = Range::new(p0, p0);
            return Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
                uri, range,
            ))));
        }
        Ok(None)
    }
}

/// Construit un Hover markdown.
fn hover_md(value: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    }
}

/// Mot identifiant sous le curseur (ou `None`).
fn word_at(text: &str, pos: Position) -> Option<String> {
    let line = text.lines().nth(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let c = (pos.character as usize).min(chars.len());
    let is_id = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut s = c;
    while s > 0 && is_id(chars[s - 1]) {
        s -= 1;
    }
    let mut e = c;
    while e < chars.len() && is_id(chars[e]) {
        e += 1;
    }
    if s == e {
        None
    } else {
        Some(chars[s..e].iter().collect())
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
