// Client VS Code minimal : lance le serveur `cdc-lsp` en stdio et le connecte aux fichiers .cdl.
import { workspace, ExtensionContext } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient;

export function activate(_context: ExtensionContext) {
  const serverPath = workspace
    .getConfiguration("cadernislang")
    .get<string>("serverPath", "cdc-lsp");

  // Le serveur communique via stdin/stdout (cf. crates/cdc-lsp).
  const serverOptions: ServerOptions = {
    run: { command: serverPath, transport: TransportKind.stdio },
    debug: { command: serverPath, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "cadernislang" }],
  };

  client = new LanguageClient(
    "cadernislang",
    "cadernislang LSP",
    serverOptions,
    clientOptions
  );
  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client ? client.stop() : undefined;
}
