# ADR-003 : Coût PA des `bot` — auto-dérivé par défaut, `coute` en override

> **Status** : Accepted
> **Date** : 2026-06-18
> **Deciders** : Ninhache, Claude
> **Composants affectés** : `cdc-sema` (source du calcul), `cdc-interp`, `cdc-codegen`

## Context

Retour utilisateur : devoir déclarer `coute N pa` à la main sur chaque `bot` paraît arbitraire ;
l'idéal serait que « le coût de chaque opération soit défini automatiquement, et qu'on gère juste
le passage de tour ».

Constat : au niveau d'un `tour`, **les opérations sont déjà costées automatiquement** (affectation,
`up`, lecture externe…). Le seul nombre manuel restant est le `coute` par `bot`. Aujourd'hui,
l'omettre donne **0 PA** (sort gratuit) — peu utile.

## Decision

**`coute N pa` devient optionnel. S'il est omis, le coût PA effectif du `bot` est
auto-dérivé** = somme statique des coûts de son corps (affectations, `up`, appels de bots
imbriqués…), pire chemin sur les `detect`. **S'il est déclaré, il fait override** (« coût imposé
par le jeu », façon Dofus).

Le calcul vit dans **`cdc_sema::effective_costs(program, &cfg)`** — source unique consommée par
l'interpréteur, le codegen et la vérif statique de `cdc-sema` (cohérent avec §9.7).

## Alternatives Considered

### Option 1 : Itemiser le corps dans le tour appelant (abandonner le coût plat)
**Pros** : zéro coût manuel ; transparent.
**Cons** : casse l'abstraction « gas » (un sort = prix fixe) ; **change la sémantique du golden**
(`tuer_dopeul` passerait de 4 à ~0) → §9 à recalibrer.
**Ruled out** : trop disruptif pour un gain ergonomique.

### Option 2 : Défaut auto-dérivé + override (choisie)
**Pros** : ergonomique ; **non-cassant** (les bots qui déclarent `coute` — dont le golden — sont
inchangés ; seul le défaut des bots sans `coute` passe de 0 à « somme du corps ») ; garde la
métaphore du coût fixe via l'override.
**Cons / trade-offs** : le coût dérivé « fuit » l'implémentation du corps (mitigé par l'override) ;
les boucles dans un corps sont approximées (une itération).

## Consequences

### Positive
- Moins de boilerplate ; omettre `coute` devient **significatif** (coût du travail réel).
- Le coût auto alimente aussi la **vérif statique** des `tour` (`error[E-PA]`).

### Negative / Trade-offs
- Refactorer un corps de `bot` sans `coute` change son coût (et celui de ses appelants).
- Récursion gérée par garde anti-cycle (contribue 0).

### Neutral
- `coute` explicite reste recommandé pour un coût « contractuel » stable.

## Validation Criteria
- [ ] §9 inchangé (golden déclare `coute 4` → toujours 5 PA/tour).
- [ ] Un `bot` sans `coute` avec 2 affectations → coût effectif 2 (testé).
- [ ] Un `bot` sans `coute` au corps lourd, appelé dans un `tour`, déclenche `error[E-PA]` (testé).

We'll revisit if : le coût dérivé crée des surprises (fuite d'implémentation) gênantes en pratique.

## Implementation Notes
- `cdc_sema::effective_costs` ; consommé par `cdc-interp` (charge à l'appel), `cdc-codegen`
  (constante bakée) et `cdc-sema` (analyse statique). Voir `docs/SPEC.md` §1.1 (Déviation 11).
