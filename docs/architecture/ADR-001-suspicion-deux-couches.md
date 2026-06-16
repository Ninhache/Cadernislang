# ADR-001 : Modèle de suspicion à deux couches (calcul invisible / action observable)

> **Status** : Accepted
> **Date** : 2026-06-16
> **Deciders** : Ninhache, Claude
> **Composants affectés** : `cdc-runtime` (moteur de suspicion), `cdc-interp`, `cdc-sema`, `cdc-codegen`

## Context

La jauge de suspicion (`docs/SPEC.md` §1.2) est l'âme du langage : le déterminisme doit être puni.
La spec d'origine faisait entrer **toute** action dans la fenêtre glissante, y compris les
affectations. Or une machine de Turing est déterministe : un calcul long répète le même
`(id, bucket)` → pénalité → BAN. Conséquence : **aucun calcul non-trivial ne survit → le langage
n'est pas Turing-complet en pratique**, et il est quasi-inécrivable.

On optimise pour : préserver la mécanique anti-déterminisme **réelle** tout en garantissant la
Turing-complétude et l'écrivabilité.

## Decision

**On distingue deux couches. La suspicion ne traque QUE les actions observables (`bot`, `afk`,
`up`) ; le calcul interne (`loot`/`ban`, arithmétique, `detect`/`farm`/`grind`, affectations,
`passer`, corps de `bot`) ne lève jamais de suspicion.**

Rationale : un anti-bot ne voit pas la RAM, il voit les actions de jeu. C'est thématiquement juste
*et* ça rend la couche calcul libre → Turing-complète.

## Alternatives Considered

### Option 1 : Toute action est observable (spec d'origine)
**Pros** : conforme à la lettre ; mécanique « pure ».
**Cons** : non Turing-complet en pratique ; inécrivable.
**Ruled out because** : casse l'exigence Turing-complétude (et le golden §9.2 devient infalsifiable
combiné au bug Déviation 4).

### Option 2 : Seuil/pénalité dépendant du type d'action
**Pros** : granularité fine.
**Cons** : complexe à calibrer/expliquer ; sur-ingénierie pour un toy lang.
**Ruled out because** : viole « ne pas sur-ingénier » ; pas de gain de gameplay clair.

### Option 3 (choisie) : Deux couches calcul/action
**Pros** : Turing-complet ; écrivable ; thématiquement fidèle ; simple à implémenter (filtrer ce
qui entre dans la fenêtre).
**Cons / trade-offs acceptés** : on ne peut plus « piéger » un calcul déterministe — mais ce
n'était pas l'intention.

## Consequences

### Positive
- Couche calcul libre (preuve `examples/somme.cdl`).
- Mécanique de furtivité concentrée là où elle a du sens (interactions de jeu).
- `passer` non observable → tours de calcul gratuits, nombre de tours illimité.

### Negative / Trade-offs
- `id_action` n'est défini que pour les actions observables → le runtime doit étiqueter
  explicitement bot/afk/up.
- Deux notions de « coût » coexistent (PA/PM partout vs suspicion sur observables seulement).

### Neutral
- Le PA/PM (gas) reste appliqué partout, indépendamment de l'observabilité.

## Validation Criteria

We'll know this decision was right if:
- [ ] `examples/somme.cdl` (calcul pur) termine avec `suspicion == 0` quel que soit `n`.
- [ ] Golden `afk rand` aboutit (§9.1) ; variante `afk 3000` bannit (§9.2).
- [ ] La logique de filtrage « observable » vit **uniquement** dans `cdc-runtime` (§9.7).

We'll revisit this decision if:
- on constate qu'un calcul peut indirectement déclencher un ban, ou
- une feature future rend une action de calcul réellement observable côté « serveur ».

## Implementation Notes

- `cdc-runtime` : seule l'API d'action observable (`bot`/`afk`/`up`) pousse dans la fenêtre.
- Voir `docs/SPEC.md` §1.2 (Déviation 6) et §1.5 (Turing-complétude).
- Concerne issues #14 (moteur suspicion) et #12 (builtins).
