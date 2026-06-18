# ADR-004 : Outillage LSP — `cdc-lsp` (tower-lsp) + extension VS Code

> **Status** : Accepted
> **Date** : 2026-06-18
> **Deciders** : Ninhache, Claude
> **Composants affectés** : nouveau crate `cdc-lsp`, `editors/vscode`

## Context

Demande utilisateur : un « live-server » pour l'autocomplétion et une extension affichant le
**coût PA en inline**. Le compilo a un principe « dépendances minimales » (`logos`, `rand`) ; or un
serveur LSP confortable implique des deps lourdes (`tower-lsp`, `tokio`).

## Decision

**Créer un crate séparé `cdc-lsp`** (binaire) qui réutilise le front-end (`cdc-lexer`,
`cdc-parser`, `cdc-sema`) et expose un serveur LSP via `tower-lsp` + `tokio`. Les deps async sont
**confinées à ce crate tooling** — le compilo (`cdc`) et la lib restent minimaux. L'extension VS
Code (`editors/vscode`) est un client mince qui lance `cdc-lsp` en stdio.

La logique d'analyse (diagnostics, complétions, coûts) vit dans `cdc-lsp::analysis` (testable) et
s'appuie sur `cdc_sema::cost_report` (source unique des coûts, ADR-003) — le serveur n'est qu'un
adaptateur.

## Alternatives Considered

- **`lsp-server` (sync, sans tokio)** : plus léger mais plus de plomberie manuelle ; `tower-lsp`
  est le standard ergonomique. Retenu `tower-lsp` car deps confinées.
- **Pas de LSP, juste `cdc cost`** : insuffisant pour l'autocomplétion/diagnostics live demandés.

## Consequences

### Positive
- Diagnostics live, complétions, et **inlay hints PA/PM par `tour`** (le « coût inline »).
- Réutilise tout le front-end ; aucune duplication de sémantique.

### Negative / Trade-offs
- `tower-lsp` + `tokio` = grosses deps (mais hors du compilo).
- Le comportement *interactif* dans l'éditeur n'est pas testable en CI sans client ; couvert par
  tests d'analyse + smoke LSP (initialize/inlayHint vérifiés).

## Validation Criteria
- [ ] `cargo build -p cdc-lsp` OK ; tests `analysis` verts.
- [ ] Smoke LSP : `initialize` renvoie les capabilities ; `inlayHint` renvoie `X/Y PA · …` sur un
      `tour` (vérifié).
- [ ] Extension : diagnostics + complétions + hints visibles dans VS Code (à valider manuellement).

## Implementation Notes
- `crates/cdc-lsp` (bin), `editors/vscode` (client). Voir `cdc_sema::cost_report` / `report`.
