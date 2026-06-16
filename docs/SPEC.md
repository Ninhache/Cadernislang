# Cadernislang — Spécification normative

> Document de référence pour l'implémentation de `cdc`. Cette spec dérive de la spec d'origine
> mais en **corrige les imperfections** ; chaque écart est signalé par un bloc **⚠️ Déviation**.

---

## 0. Mission

Implémenter **cadernislang**, un langage jouet (`.cdl`) compilé/interprété dont la blague tient
dans la **sémantique d'exécution** (culture Dofus / botting / reverse), pas dans le vocabulaire.
Compilo : `cdc`. Cible finale : LLVM (« llvm-slop »), via étapes (interpréteur d'abord).

**Règle d'or :** chaque feature « thème » doit être une *contrainte d'exécution réelle*. Un
renommage cosmétique d'un construct classique ne va pas dans le langage.

---

## 1. Les trois mécaniques (le cœur)

### 1.1 — Exécution au tour par tour avec budget PA/PM

Le programme s'exécute par **tours de jeu**.

- Chaque tour régénère le budget : `MAX_PA = 6`, `MAX_PM = 3` (surchargeable par pragma).
- Un bloc `tour { ... }` représente les actions d'un tour ; il **doit** tenir dans le budget.
- `passer` termine le tour courant → régénération du budget → tour suivant.
- Une boucle `farm` / `grind` autour de `tour { ... } passer` fait avancer le jeu.

**Table de coûts (normative) :**

| Élément | Coût |
|---|---|
| Appel de `bot` | son `coute N pa` déclaré |
| Affectation (`=`, `+=`, `-=`) | 1 PA |
| `up(...)` | 1 PA |
| `afk(...)` / `afk rand(...)` | 0 PA (avance l'horloge → §1.2) |
| Comparaison, opérateur booléen (`et`/`ou`/`pas`), lecture de `pa`/`pm`/`suspicion`, `cd_pret(...)`, lecture de variable **dans une condition** | **0 PA** (*perception*) |
| Lecture d'une variable d'un scope plus externe (hors condition) | **1 PM par niveau de scope franchi** (voir §1.1.b) |

**Modèle d'évaluation des `bot` :** un appel de `bot` coûte **uniquement** son `coute N pa`
déclaré. Le corps s'exécute « instantanément » et ne consomme pas le budget du tour appelant
(un sort coûte ses PA, on n'itemise pas l'intérieur).

> **⚠️ Déviation 1 (clarification) — Budget actif uniquement dans `tour {}`.** Les statements hors
> `tour` (setup en tête de `connexion`, `up` final…) ne sont pas budgétés. Ils alimentent
> néanmoins l'horloge et la suspicion si ce sont des actions. La spec d'origine ne tranchait pas ;
> sans cela, `loot total = 0` (affectation = 1 PA) et le `up` final seraient incohérents.

#### 1.1.b — Coût PM (déplacement vers la donnée)

> **⚠️ Déviation 2 (définition précise).** La spec d'origine disait « 1 PM par niveau d'imbrication
> traversé » sans définir « niveau ». Définition retenue :
>
> Le **coût PM d'une lecture** d'une variable `v`, à l'intérieur d'un `tour`, hors condition, est
> le **nombre de scopes lexicaux franchis** entre le bloc où la lecture a lieu et le bloc qui
> déclare `v`. Une lecture d'une variable déclarée dans le scope courant coûte 0 PM.
>
> Comptage : chaque `{ ... }` (`tour`, `farm`, `grind`, `detect`/`sinon`, corps de `bot`,
> `connexion`) introduit un scope. On compte les frontières franchies en remontant du site de
> lecture jusqu'au scope déclarant (exclus). Exemple golden : `total` est déclaré dans
> `connexion` ; lu dans `total += tuer_dopeul()` situé dans `tour ⊂ farm ⊂ connexion` → 2 scopes
> franchis (`tour`, `farm`) → 2 PM ≤ `MAX_PM = 3`. ✅
>
> Une **écriture** (`total += …`) lit puis écrit : le coût PM de la partie lecture s'applique ;
> l'affectation elle-même coûte 1 PA (déjà dans la table). Les lectures **en condition** sont de
> la perception → 0 PM.

**Vérification statique (sema).** Pour un `tour { ... }` au coût calculable statiquement (pas de
boucle dynamique interne), `cdc` **rejette à la compilation** un bloc qui dépasse `MAX_PA`/`MAX_PM` :

```
error[E-PA]: tour trop gourmand — 9 PA demandés, budget max 6
error[E-PM]: tour trop gourmand — 4 PM demandés, budget max 3
```

Pour les coûts dynamiques (boucle dans un `tour`), vérification **au runtime** → panic
`PaInsuffisant` / `PmInsuffisant`.

### 1.2 — Jauge de suspicion : le déterminisme est illégal

C'est l'âme du langage. Variable globale implicite `suspicion` (entier non signé), démarre à `0`,
seuil `SEUIL_BAN = 80`.

**Horloge virtuelle.**

> **⚠️ Déviation 3.** La spec d'origine parlait d'« horloge réelle ». On utilise une **horloge
> virtuelle** : `afk(ms)` avance un compteur interne de `ms` ; **aucun `sleep` réel**. Rationale :
> déterminisme et rapidité des tests (le golden test fait 16 000+ tours). Le `bucket_delai` est
> `floor(ms_écoulés_depuis_l_action_précédente / 500)` (taille de bucket = 500 ms, configurable).

**Modèle de détection — fenêtre glissante.**

> **⚠️ Déviation 4 (correction de bug).** La spec d'origine pénalisait si `(id, bucket)` est
> identique à **l'action immédiatement précédente**. Or dans le golden test les tours alternent
> `kill` / `afk` (à cause du `cd 2`) : deux actions consécutives ne sont jamais identiques, donc
> la suspicion ne ferait que **décroître** et `afk 3000` ne bannirait **jamais** — le critère
> §9.2 serait infalsifiable. **Correction :** on maintient une **fenêtre glissante des K = 8
> dernières actions** ; une action est `(id_action, bucket_delai)`.
>
> À chaque action terminée :
> - si `(id, bucket)` **figure déjà** dans la fenêtre → `suspicion += PENALITE` (`PENALITE = 7`) ;
> - sinon → `suspicion = max(0, suspicion − DECAY)` (`DECAY = 3`) ;
> - puis on pousse `(id, bucket)` dans la fenêtre (taille max K, FIFO).
>
> Conséquence : `afk 3000` fixe → chaque `afk` retombe sur `(afk, 6)` déjà présent → pénalité
> répétée → BAN. `afk rand(a, b)` → buckets dispersés → decay dominant → survie. La mécanique
> devient **réelle et observable**, conformément à l'intention d'origine.

Si `suspicion >= SEUIL_BAN` → le runtime lève **`BAN`** :

```
error: compte banni
```

État effacé, **terminaison immédiate**, code retour **non-nul**. Pas de `gg`, pas de cleanup.

### 1.3 — Cooldowns sur les `bot`

`bot f() coute N pa, cd M { ... }` → après un appel à `f`, `f` est indisponible pendant `M` tours.

- Table runtime : `{ id_bot -> n° de tour de disponibilité }`.
- Builtin `cd_pret(f) -> flag` : vrai si `f` est appelable ce tour (perception, 0 PA).
- Appeler un `bot` en cooldown → erreur runtime :

```
error: sort en cooldown
```

### 1.4 — (Bonus, Phase 6) Dérive de patch

À chaque `cdc build`, un seed renumérote les tags internes des champs de `perso` et des variants
de `pano`, **sauf** ceux épinglés (`@N`). Un code épinglé survit ; un code paresseux casse au
rebuild. À faire une fois le reste stable.

---

## 2. Types & littéraux

| Type | Sémantique | Notes |
|---|---|---|
| `kamas` | entier signé 64 bits | type numérique par défaut |
| `flag` | booléen | littéraux `legit` (true) / `cheat` (false) |
| `txt` | chaîne UTF-8 | pour `up` |
| `perso` | struct (champs nommés) | Phase 6 |
| `pano` | énumération | Phase 6 |
| `afk_total` | unit / valeur nulle | |

Littéraux numériques : underscores autorisés et ignorés (`1_000_000`).

---

## 3. Mots-clés & builtins

**Keywords :** `bot`, `coute … pa`, `cd …`, `connexion`, `serveur`, `tour`, `passer`, `farm`,
`grind`, `loot` (var mutable), `ban` (constante), `gg` (return), `detect` (if), `sinon` (else),
`et` / `ou` / `pas`, `pa` / `pm` / `suspicion` (pseudo-variables runtime en lecture seule).

**Builtins (fournis par `cdc-runtime`) :** `up(txt)`, `afk(ms)`, `afk rand(a, b)`,
`rand(a, b) -> kamas`, `cd_pret(bot) -> flag`, `butin(min, max) -> kamas` (stub RNG seedable).

---

## 4. Easter eggs obligatoires

1. Tout `.cdl` **doit** commencer par la ligne exacte `// gg wp`. Sinon, erreur fatale du compilo,
   **aucune compilation** :
   ```
   error: candidature contributeur refusée
   ```
2. Le binaire produit par `cdc build` affiche en en-tête de run : `cadernis compiler — gg wp`.

---

## 5. En-tête de fichier & pragmas

> **⚠️ Déviation 5 (clarification).** Disposition de l'en-tête :
> - **Ligne 1** : exactement `// gg wp`.
> - **Lignes suivantes** (optionnelles, avant tout `item`) : pragmas `#clé valeur`.
>
> Pragmas reconnus : `#max_pa N`, `#max_pm N`, `#seuil_ban N`, `#penalite N`, `#decay N`,
> `#fenetre N` (K), `#bucket_ms N`, `#seed N`. Toute valeur surcharge la constante par défaut de
> `cdc-runtime::config`.

---

## 6. Grammaire (EBNF, indicative)

```ebnf
programme   = entete , { pragma } , { item } ;
entete      = "// gg wp" , NEWLINE ;
pragma      = "#" , IDENT , INT , NEWLINE ;
item        = serveur_decl | bot_decl | connexion_decl ;

serveur_decl   = "serveur" , IDENT ;
connexion_decl = "connexion" , bloc ;

bot_decl    = "bot" , IDENT , "(" , [ params ] , ")" ,
              [ ":" , type ] ,
              [ "," , "coute" , INT , "pa" ] ,
              [ "," , "cd" , INT ] ,
              bloc ;
params      = param , { "," , param } ;
param       = IDENT , ":" , type ;
type        = "kamas" | "flag" | "txt" | "afk_total" | IDENT ;

bloc        = "{" , { stmt } , "}" ;
stmt        = decl_loot | decl_ban | affect | tour_stmt | passer_stmt
            | farm_stmt | grind_stmt | detect_stmt | gg_stmt | expr_stmt ;

decl_loot   = "loot" , IDENT , [ ":" , type ] , "=" , expr ;
decl_ban    = "ban"  , IDENT , [ ":" , type ] , "=" , expr ;
affect      = IDENT , ( "=" | "+=" | "-=" ) , expr ;
tour_stmt   = "tour" , bloc ;
passer_stmt = "passer" ;
farm_stmt   = "farm" , expr , bloc ;
grind_stmt  = "grind" , IDENT , "de" , expr , "a" , expr , bloc ;
detect_stmt = "detect" , expr , bloc , [ "sinon" , bloc ] ;
gg_stmt     = "gg" , [ expr ] ;
expr_stmt   = expr ;

expr        = (* arith +,-,*,/ ; comparaisons <,<=,>,>=,==,!= ;
                et/ou/pas ; appels f(args) ; afk rand(a,b) ;
                littéraux kamas/flag/txt ; pa/pm/suspicion ; IDENT *) ;
```

---

## 7. Architecture technique

Workspace Cargo, 1 crate par responsabilité : `cdc-lexer`, `cdc-ast`, `cdc-parser`, `cdc-sema`,
`cdc-interp`, `cdc-codegen`, `cdc-runtime`, `cdc` (driver).

**`cdc-runtime` porte TOUTE la jouabilité** et expose une C ABI consommée par l'interpréteur ET le
code LLVM généré : compteurs PA/PM + régen `passer` ; moteur de suspicion (fenêtre glissante,
buckets, BAN) ; table de cooldowns ; builtins. **La sémantique est définie une seule fois**
(§9.7).

**Backend LLVM :** `inkwell` épinglé **LLVM 18** (feature `llvm18-1`). Le codegen abaisse en LLVM
IR avec des `call` vers les symboles `extern "C"` de `cdc-runtime`, émet un objet, puis linke
(via `cc`) avec `cdc-runtime` (crate-type `staticlib` + `rlib`) en binaire natif.
`llvm-sys` exige LLVM installé : `LLVM_SYS_181_PREFIX=/usr/lib/llvm18` sur Arch.

**CLI `cdc` :** `run` (interp), `build` (codegen + link), `check` (lexer+parser+sema, sans
exécution).

---

## 8. Plan d'implémentation par phases

Implémenter et committer **phase par phase** ; chaque phase compile et passe ses tests avant la
suivante. Une branche + PR par phase, issues fermées par les commits.

- **Phase 0 — Scaffold.** Workspace, crates vides, CLI `cdc` (parse args). Check header `// gg wp`
  (`candidature contributeur refusée`). Bannière `gg wp`.
- **Phase 1 — Front-end.** Lexer (`logos`) + parser descente récursive + AST. `cdc check` dump AST.
- **Phase 2 — Interp + budget PA/PM.** `cdc run` exécute `tour`/`passer`/`farm`/`grind`/`detect`,
  applique la table de coûts, builtins `up`/`afk`/`rand`/`butin`. Farmeur de dopeuls (sans
  suspicion ni cd).
- **Phase 3 — Suspicion + cooldowns.** Moteur §1.2 + cooldowns §1.3 dans `cdc-runtime`, branchés à
  l'interp. Test de non-régression `afk 3000` → `BAN`.
- **Phase 4 — Analyse statique de budget.** `cdc-sema` rejette un `tour` statiquement trop coûteux
  (`error[E-PA]` / `error[E-PM]`). PM (déplacement vers scope externe) inclus.
- **Phase 5 — Backend LLVM.** `cdc-codegen` via `inkwell` ; `cdc build` → binaire natif liant
  `cdc-runtime`. Parité de comportement avec `cdc run` sur le golden test.
- **Phase 6 — (optionnel) Dérive de patch.** Renumérotation des tags `perso`/`pano` par seed,
  épinglage `@N`, test « code non épinglé casse au rebuild ».

---

## 9. Critères d'acceptation

1. `cdc run examples/dopeuls.cdl` : le farm aboutit, sortie `objectif atteint gg`, code retour 0.
2. Variante `afk 3000` : sortie `error: compte banni`, code retour ≠ 0, objectif non atteint.
3. Un `tour { }` statiquement > 6 PA : `cdc check` échoue avec `error[E-PA]`.
4. Appel d'un `bot` en cooldown : erreur runtime `sort en cooldown`.
5. Fichier sans `// gg wp` : `error: candidature contributeur refusée`, aucune compilation.
6. `cdc build examples/dopeuls.cdl` produit un binaire natif au comportement identique à `cdc run`.
7. La sémantique des mécaniques vit dans `cdc-runtime`, partagée interp ↔ LLVM (pas de
   duplication divergente).

Les tests d'acceptation fixent `CDC_SEED` (ou `#seed N`) pour un comportement déterministe.

---

## 10. Programme de référence (golden test) — `examples/dopeuls.cdl`

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
                afk rand(2000, 5000)
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

**Comportement attendu :** le `tour` coûte `4 (appel) + 1 (+=) = 5 PA ≤ 6` et `2 PM ≤ 3` → passe
la vérif statique. `tuer_dopeul` (cd 2) n'est pas spammable → certains tours partent en `afk`. Les
`afk rand(...)` dispersent les buckets → suspicion basse → le farm aboutit.

**Test de non-régression :** remplacer `afk rand(2000, 5000)` par `afk 3000` (délai fixe) **doit**
faire grimper la suspicion jusqu'au `BAN` → `error: compte banni`, code retour ≠ 0, `total` jamais
atteint. Ce test prouve que la mécanique de suspicion est réelle (cf. Déviation 4).

---

## 11. Conventions

- Rust 2021, `cargo clippy` propre, `cargo fmt`.
- Erreurs **en français** ; ton « forum » pour les easter eggs, mais erreurs de parsing/typage
  claires (ligne/colonne).
- Constantes de gameplay centralisées dans `cdc-runtime::config`, surchargeables par pragma (§5).
- Dépendances minimales : `logos`, `inkwell`, `rand`.
