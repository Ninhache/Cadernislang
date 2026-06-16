# Cadernislang — instructions projet

**cadernislang** est un langage jouet (`.cdl`) compilé/interprété. Compilo : `cdc` (Cadernis
Compiler). La blague tient dans la **sémantique d'exécution** (culture Dofus / botting /
reverse), pas dans le vocabulaire.

## Règle d'or

> Chaque feature « thème » doit être une **contrainte d'exécution réelle** qui change la façon
> d'écrire le code. Un simple renommage cosmétique d'un construct classique n'a pas sa place ici.

La référence normative complète est **`docs/SPEC.md`**. Lis-la avant de coder. Toute déviation
par rapport à la spec d'origine y est explicitement flaggée (sections « ⚠️ Déviation »).

## Les 3 mécaniques réelles (le cœur)

1. **Budget PA/PM par tour.** Le code s'exécute par `tour { ... } passer`. Chaque tour dispose de
   `MAX_PA=6` / `MAX_PM=3`, régénérés à `passer`. Les actions coûtent (table dans la spec) ; la
   *perception* (lectures en condition, `pa`/`pm`/`suspicion`, `cd_pret`) coûte 0. Trop gourmand →
   `error[E-PA]` (statique) ou panic `PaInsuffisant` (dynamique).
2. **Suspicion (le déterminisme est illégal).** Variable globale implicite. Détection par
   **fenêtre glissante des K=8 dernières actions** : si `(id_action, bucket_délai)` y figure déjà
   → `+PENALITE` ; sinon `−DECAY`. `>= SEUIL_BAN` → `error: compte banni`, code retour ≠ 0.
   Horloge **virtuelle** alimentée par `afk` (pas de sleep réel). `afk rand(a,b)` disperse les
   buckets (survie) ; `afk 3000` fixe répète le bucket (ban).
3. **Cooldowns.** `bot f() ... cd M` : après appel, `f` indisponible M tours. `cd_pret(f)` teste.
   Appel en cooldown → `error: sort en cooldown`.

## Architecture

Workspace Cargo, 1 crate par responsabilité :

| Crate | Rôle |
|---|---|
| `cdc-lexer` | tokenisation (`logos`) |
| `cdc-ast` | types de l'AST |
| `cdc-parser` | descente récursive → AST |
| `cdc-sema` | résolution, typage, analyse statique budget PA/PM |
| `cdc-interp` | interpréteur tree-walking |
| `cdc-codegen` | backend LLVM (`inkwell`, **LLVM 18**) |
| `cdc-runtime` | **toute** la jouabilité, exposée en C ABI, partagée interp↔LLVM |
| `cdc` | binaire CLI (driver) |

**Invariant §9.7 : la sémantique des mécaniques vit UNIQUEMENT dans `cdc-runtime`.** Interp et
codegen l'appellent — jamais de duplication divergente.

## Commandes

```bash
cdc run   fichier.cdl   # interprète (cdc-interp)
cdc build fichier.cdl   # compile via LLVM + link cdc-runtime → binaire natif
cdc check fichier.cdl   # lexer + parser + sema (dont vérif budget), sans exécution
```

## Prérequis & build

- Rust 2021, toolchain 1.93+.
- **LLVM 18** pour `cdc build` (Phase 5). Sur Arch : `sudo pacman -S --needed llvm18`, puis
  `export LLVM_SYS_181_PREFIX=/usr/lib/llvm18`. (La machine a LLVM 22 système, incompatible
  inkwell → on épingle 18.) Sans LLVM, `cdc run`/`cdc check` fonctionnent.
- `cargo fmt` et `cargo clippy` doivent rester propres.

## Conventions

- **Messages d'erreur en français.** Ton « forum » assumé pour les easter eggs ; erreurs de
  parsing/typage claires et exploitables (ligne/colonne).
- **Constantes de gameplay** (`MAX_PA`, `MAX_PM`, `SEUIL_BAN`, `PENALITE`, `DECAY`, `K`, taille de
  bucket) centralisées dans `cdc-runtime` (module `config`), surchargeables par pragma d'en-tête
  (`#max_pa 8`, `#seed N`, …).
- **RNG seedable** : aléatoire par défaut ; `#seed N` ou env `CDC_SEED` pour des tests
  déterministes.
- Pas de dépendance superflue : `logos`, `inkwell`, `rand` suffisent.

## Easter eggs obligatoires

- Tout `.cdl` commence par la ligne exacte `// gg wp`. Sinon : `error: candidature contributeur
  refusée`, aucune compilation.
- Le binaire de `cdc build` affiche en en-tête de run : `cadernis compiler — gg wp`.

## Flow de contribution

- **Une issue par sous-tâche**, regroupées en milestones = phases (voir `docs/SPEC.md` §8).
- **Une branche + une PR par phase** : `phase-N-...`. Commits référençant/fermant les issues
  (`Closes #N`). `cargo fmt && cargo clippy && cargo test` vert avant la PR.
- Implémenter et committer **phase par phase** ; chaque phase compile et passe ses tests avant la
  suivante.
