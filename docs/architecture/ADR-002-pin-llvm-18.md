# ADR-002 : Backend LLVM épinglé sur LLVM 18 (via `inkwell`)

> **Status** : Accepted
> **Date** : 2026-06-16
> **Deciders** : Ninhache, Claude
> **Composants affectés** : `cdc-codegen`, `cdc-runtime` (link), build/CI

## Context

La cible finale est un binaire natif via LLVM (`docs/SPEC.md` §7). La machine de dev (Arch) a
**LLVM 22.1.6** en système, mais `inkwell`/`llvm-sys` ne suivent pas le rolling release — LLVM 22
n'est pas supporté. Arch fournit des paquets versionnés (`llvm18`, `llvm20`, `llvm21`).

On optimise pour : un backend qui **compile réellement** sur cette machine, avec une dépendance
stable et documentée.

## Decision

**On épingle `inkwell` sur LLVM 18 (feature `llvm18-1`), via le paquet Arch `llvm18` et
`LLVM_SYS_181_PREFIX=/usr/lib/llvm18`.** Le link final passe par `cc`, `cdc-runtime` étant compilé
en `staticlib` + `rlib`.

## Alternatives Considered

### Option 1 : Suivre le système (LLVM 22)
**Pros** : zéro install supplémentaire.
**Cons** : non supporté par inkwell ; ne compile pas.
**Ruled out because** : techniquement impossible aujourd'hui.

### Option 2 : Émettre du LLVM IR textuel + `llc`/`clang` système (pas d'inkwell)
**Pros** : découplé de la version ; pas de dépendance buildtime lourde.
**Cons** : perd le binding safe ; s'éloigne de la spec (« inkwell ») ; IR texte fragile.
**Ruled out because** : la spec demande inkwell ; LLVM 18 résout proprement le couplage de version.

### Option 3 (choisie) : Pin LLVM 18 via inkwell
**Pros** : conforme à la spec ; binding safe ; version disponible en paquet Arch ; reproductible.
**Cons / trade-offs acceptés** : install système requise (`sudo pacman -S llvm18`) ; à bumper
manuellement quand inkwell supportera LLVM ≥ 20/22.

## Consequences

### Positive
- `cdc build` (Phase 5) buildable et reproductible.
- `cdc run` / `cdc check` indépendants de LLVM → Phases 0-4 non bloquées par l'absence d'install.

### Negative / Trade-offs
- Prérequis système à documenter (README, issue #24).
- Divergence entre le LLVM du langage (18) et le système (22).

### Neutral
- Si LLVM est absent, le projet reste 100% utilisable en interpréteur.

## Validation Criteria

We'll know this decision was right if:
- [ ] `LLVM_SYS_181_PREFIX=/usr/lib/llvm18 cargo build -p cdc-codegen` compile.
- [ ] `cdc build examples/dopeuls.cdl` produit un binaire au comportement identique à `cdc run`
      (§9.6).

We'll revisit this decision if:
- `inkwell` publie un support LLVM ≥ 20/22 stable (bump possible), ou
- le pin 18 devient indisponible sur Arch.

## Implementation Notes

- `cdc-codegen/Cargo.toml` : `inkwell = { version = "...", features = ["llvm18-1"] }`.
- Le README (issue #24) doit documenter le prérequis et la variable `LLVM_SYS_181_PREFIX`.
- Concerne issues #20, #21, #22. Voir `docs/SPEC.md` §7.
