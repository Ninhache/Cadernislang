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
Puis **ouvrir le dossier `editors/vscode` dans VS Code** (⚠️ pas la racine du repo — sinon F5
lance un debug générique). La config `.vscode/launch.json` ajoute la cible **« Run Extension
(cadernislang) »** : appuyer sur **F5** compile puis ouvre une fenêtre *Extension Development
Host*. Y ouvrir un fichier `.cdl` → diagnostics, complétions et coûts PA/PM en inline.

> Le serveur `cdc-lsp` doit être trouvable : soit dans le `PATH`, soit via le réglage
> `cadernislang.serverPath` pointant sur `../../target/release/cdc-lsp`.

> Note : ce client n'a pas pu être testé automatiquement dans l'environnement de dev (pas
> d'éditeur). Le serveur `cdc-lsp`, lui, est vérifié (build + tests d'analyse + smoke LSP
> initialize/inlayHint).
