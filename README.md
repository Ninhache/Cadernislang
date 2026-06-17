# cadernislang

<p align="center">
  <img src="docs/assets/cadernis.webp" alt="cadernis" width="240">
</p>

Un langage jouet (`.cdl`) compilé/interprété dont **la blague est dans la sémantique
d'exécution**, pas dans le vocabulaire. Le modèle de calcul est calqué sur la culture
Dofus / botting / reverse : on programme un farmeur qui doit tenir un **budget d'actions
par tour**, **rester sous le radar anti-bot**, et **gérer les cooldowns** de ses sorts.

Le compilateur s'appelle `cdc` (Cadernis Compiler).

```
// gg wp
serveur incarnam

bot tuer_dopeul() : kamas, coute 4 pa, cd 2 {
    gg butin(50, 200)
}

connexion {
    loot total : kamas = 0

    farm total < 1_000_000 et suspicion < 80 {
        tour {
            detect pa >= 4 et cd_pret(tuer_dopeul) {
                total += tuer_dopeul()
            } sinon {
                afk rand(2000, 5000)   // disperse les délais → reste discret
            }
        }
        passer
    }

    detect total >= 1_000_000 {
        up "objectif atteint gg"
    } sinon {
        up "compte banni rip"
    }
}
```

## Pourquoi c'est une connerie (la philosophie)

La plupart des « langages thématiques » ne sont que du **renommage cosmétique** : on appelle
`if` un `detect`, `while` un `farm`, et voilà. C'est creux. Ici, **les mots-clés sont
explicitement du troll** — on pourrait tous les renommer sans rien changer. Ce qui fait le
langage, ce sont **trois contraintes d'exécution réelles** qui changent la façon d'écrire le code :

1. **Budget PA/PM par tour.** Le code s'exécute par `tour { … } passer`. Chaque tour dispose de
   `MAX_PA=6` / `MAX_PM=3`, régénérés à `passer`. Les actions coûtent (appel de sort, affectation,
   `up`…), la *perception* (lectures en condition) est gratuite. On ne peut plus écrire un gros
   calcul d'un bloc : il faut l'**étaler dans le temps** et mettre en cache. Cousin du *gas*
   Ethereum, segmenté en tours.

2. **Le déterminisme est illégal (suspicion).** Une jauge globale traque tes **actions
   observables** (lancer un sort, `afk`, `up`) sur une **fenêtre glissante**. Répéter la même
   action au même rythme (même *bucket* de délai) fait grimper la suspicion ; trop = `compte banni`,
   terminaison immédiate, code retour ≠ 0. Une boucle déterministe serrée = mort assurée. Le
   langage *force* la variabilité (`afk rand(...)`). C'est l'inversion du paradigme habituel :
   ailleurs le déterminisme est une vertu, ici c'est le bug fatal.

3. **Cooldowns.** `bot f() … cd M` : après un appel, `f` est indisponible M tours. Combiné au
   budget et à la suspicion, tout farm devient un **puzzle d'ordonnancement temporel**.

**Important :** seul le **calcul** observable compte pour la suspicion. L'arithmétique interne
(affectations, `detect`, `farm`…) est invisible — « l'anti-bot ne voit pas ta RAM ». La couche
calcul est donc libre et **Turing-complète** ; le puzzle de furtivité n'existe que quand tu
interagis avec le « jeu ». Voir `docs/SPEC.md` (la spec normative, avec toutes les déviations
explicitées).

## Commandes

```bash
cdc run   fichier.cdl   # interprète (cdc-interp)
cdc check fichier.cdl   # lexer + parser + sema (budget statique), sans exécution
cdc build fichier.cdl   # émet du LLVM IR, compile via clang + link cdc-runtime → binaire natif
```

Tout `.cdl` **doit** commencer par la ligne exacte `// gg wp` ; sinon
`error: candidature contributeur refusée`.

## Prérequis & build

- **Rust 2021**, toolchain **1.93+**. `cargo build` à la racine du workspace.
- **`clang`** uniquement pour `cdc build` (backend natif) : `cdc-codegen` émet du LLVM IR textuel
  et le compile avec le `clang` du système (n'importe quelle version récente). Aucune dépendance
  LLVM au build-time. Sur Arch : `sudo pacman -S --needed clang`. **Sans `clang`, `cdc run` et
  `cdc check` fonctionnent.** (Historique du choix : `docs/architecture/ADR-002`.)
- RNG **seedable** pour des exécutions déterministes : pragma `#seed N` en en-tête, ou variable
  d'environnement `CDC_SEED`.

```bash
cargo build
CDC_SEED=7 cargo run -p cdc -- run examples/dopeuls.cdl   # → objectif atteint gg
cargo run -p cdc -- run examples/dopeuls_ban.cdl          # → error: compte banni
cargo run -p cdc -- check examples/dopeuls.cdl            # AST + vérif budget
```

## Architecture

Workspace Cargo, un crate par responsabilité :

| Crate | Rôle |
|---|---|
| `cdc-lexer` | tokenisation (`logos`) + check de l'en-tête |
| `cdc-ast` | types de l'AST |
| `cdc-parser` | descente récursive → AST |
| `cdc-sema` | résolution, typage, analyse statique du budget PA/PM |
| `cdc-interp` | interpréteur tree-walking |
| `cdc-codegen` | backend natif : émission de LLVM IR textuel → `clang` |
| `cdc-runtime` | **toute** la jouabilité (PA/PM, suspicion, cooldowns, builtins), partagée interp ↔ LLVM |
| `cdc` | binaire CLI (driver) |

**Invariant :** la sémantique des mécaniques vit **uniquement** dans `cdc-runtime` — interp et
codegen l'appellent, jamais de duplication divergente.

## Statut

| Phase | Contenu | État |
|---|---|---|
| 0 | scaffold, CLI, en-tête, bannière | ✅ |
| 1 | lexer + AST + parser + `cdc check` (AST) | ✅ |
| 2 | interpréteur + budget PA/PM + builtins | ✅ |
| 3 | suspicion (fenêtre K) + cooldowns | ✅ |
| 4 | analyse statique du budget (`E-PA`/`E-PM`) | ✅ |
| 5 | backend natif (`cdc build` via IR textuel + `clang`) | ✅ |
| 6 | bonus : dérive de patch (`perso`/`pano`) | ⏳ optionnel |

## Licence

MIT.
