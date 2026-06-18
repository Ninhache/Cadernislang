# Extension VS Code — cadernislang

Client mince qui lance le serveur LSP **`cdc-lsp`** et l'attache aux fichiers `.cdl`.

Fonctionnalités (fournies par `cdc-lsp`) :
- **Diagnostics** live (parse + sema : `error[E-PA]`, cooldown, en-tête, etc.).
- **Complétions** : mots-clés, builtins, noms de `bot`/`pano`/`perso`.
- **Inlay hints** : coût **PA/PM par `tour`** affiché en ligne (ex. `5/6 PA · 1/3 PM`).

## Prérequis
- Compiler le serveur : `cargo build --release -p cdc-lsp` (à la racine du repo).
- Mettre `cdc-lsp` dans le `PATH`, ou régler `cadernislang.serverPath` dans les settings VS Code
  vers `target/release/cdc-lsp`.

## Lancer en dev
```bash
cd editors/vscode
npm install
npm run compile
```
Puis, dans VS Code, ouvrir ce dossier et lancer « Run Extension » (F5). Ouvrir un `.cdl` :
les diagnostics, complétions et coûts PA/PM en inline apparaissent.

> Note : ce client n'a pas pu être testé automatiquement dans l'environnement de dev (pas
> d'éditeur). Le serveur `cdc-lsp`, lui, est vérifié (build + tests d'analyse + smoke LSP
> initialize/inlayHint).
