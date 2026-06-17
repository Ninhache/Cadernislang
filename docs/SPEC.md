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

> **Note sur le vocabulaire.** Les mots-clés (`bot`, `loot`, `farm`, `tour`, `gg`, …) sont du
> **troll cosmétique** assumé : la blague n'est *pas* dans le lexique, elle est dans la sémantique
> d'exécution (§1). On pourrait tous les renommer sans rien changer au langage. Aucun nom de
> keyword n'est « load-bearing » — seules les trois mécaniques le sont.

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
| `up <txt>` | 1 PA |
| `afk <ms>` | 0 PA (avance l'horloge → §1.2) |
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
> Comptage : nombre de blocs `{ ... }` franchis entre le site de lecture et le scope déclarant.
>
> **⚠️ Déviation 8 (PM « cache par tour », remplace le coût « distance × accès »).** Un coût
> proportionnel à la profondeur **et** facturé à chaque accès rend le langage inutilisable : une
> simple somme `acc += i` (2 variables externes, distance 2) coûterait déjà 4 PM > `MAX_PM`, donc
> aucune boucle de calcul ordinaire ne passerait — ce qui contredit l'objectif « écrivable »
> (§1.5). **Règle retenue (model B) :**
>
> > Lire une variable déclarée **hors du `tour` courant** coûte **1 PM, payé une seule fois par
> > tour** pour cette variable ; les accès suivants dans le même tour sont **gratuits** (« on s'est
> > déplacé une fois, la donnée est désormais sous la main »). Une variable locale au tour coûte
> > 0 PM. Les lectures **en condition** sont de la perception → 0 PM. Le coût est **indépendant de
> > la profondeur** d'imbrication.
>
> C'est fidèle à la narration d'origine (« mettre en cache pour ne pas repayer ») et supprime
> l'ambiguïté C1 (le `detect` ne change rien). Exemples : golden → `total` est la seule variable
> externe lue dans le tour ⇒ **1 PM**. `somme.cdl` → `i` et `acc` ⇒ **2 PM ≤ 3**. Un tour touchant
> **> `MAX_PM` variables externes distinctes** échoue (`E-PM` statique / `PmInsuffisant` runtime).
>
> Une **écriture** `total += …` lit puis écrit : la partie lecture déclenche le coût PM (une fois
> par tour) ; l'affectation coûte 1 PA (table §1.1).
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

**Coût statique d'un `tour` avec branches.** Le coût retenu est celui du **pire chemin** : pour un
`detect … { A } sinon { B }`, `coût = max(coût(A), coût(B))`. Une boucle bornée `grind`
statiquement connue est dépliée au pire cas si calculable ; sinon le `tour` est traité comme
dynamique (vérif runtime). Le golden : branche `detect` = 4 PA (appel) + 1 PA (`+=`) + 1 PM (`total`) ;
branche `sinon` = `afk` (0 PA) → `max` = 5 PA, 1 PM. ✅

### 1.2 — Jauge de suspicion : le déterminisme est illégal

C'est l'âme du langage. Variable globale implicite `suspicion` (entier non signé), démarre à `0`,
seuil `SEUIL_BAN = 80`.

**Deux couches : calcul (invisible) vs action de jeu (observable).**

> **⚠️ Déviation 6 (Turing-complétude).** La spec d'origine faisait entrer **toute** action dans la
> fenêtre de suspicion, y compris les affectations. Conséquence fatale : un calcul **déterministe**
> (une machine de Turing l'est par nature) répète le même `(id, bucket)` → pénalité → BAN. *Aucun*
> calcul long ne survit → le langage n'est **pas** Turing-complet en pratique.
>
> **Correction (et c'est thématiquement juste — un anti-bot ne voit pas ta RAM, il voit tes actions
> de jeu) :** on distingue deux couches.
>
> | Couche | Constructs | Mécaniques appliquées |
> |---|---|---|
> | **Calcul** (invisible côté serveur) | `loot`/`ban`, arithmétique, comparaisons, `et`/`ou`/`pas`, `detect`/`sinon`, `farm`, `grind`, affectations, `passer`, corps de `bot` | **PA / PM uniquement** (le gas) |
> | **Action de jeu** (observable) | appels de `bot`, `afk` / `afk rand`, `up` | **PA / PM + suspicion** |
>
> **Seules les actions observables entrent dans la fenêtre de suspicion.** Le calcul interne ne
> lève jamais de suspicion. Un programme qui ne fait que calculer (sans `bot`/`afk`/`up`) tourne
> indéfiniment → Turing-complet (voir §1.5). Le puzzle de furtivité n'existe que sur la couche
> action de jeu.
>
> `id_action` n'est défini que pour les actions observables, avec une granularité **par type** :
> un id unique partagé par tous les `afk`, un id unique partagé par tous les `up`, et un id distinct
> **par `bot`** (l'id du bot appelé). La **première** action observable (pas de précédente) a un
> `bucket = 0` (0 ms écoulés).

**Horloge virtuelle.**

> **⚠️ Déviation 3.** La spec d'origine parlait d'« horloge réelle ». On utilise une **horloge
> virtuelle** : `afk(ms)` avance un compteur interne de `ms` ; **aucun `sleep` réel**. Rationale :
> déterminisme et rapidité des tests (le golden test fait 16 000+ tours). Le `bucket_delai` est
> `floor(délai_accumulé / bucket_ms)` (voir Déviation 9 pour `bucket_ms` et l'accumulation).

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
> Conséquence : `afk 3000` fixe → bucket constant répété → pénalité répétée → BAN. `afk rand(a, b)`
> → buckets dispersés → decay dominant → survie. La mécanique devient **réelle et observable**,
> conformément à l'intention d'origine.

**Modèle de délai (accumulation) & taille de bucket.**

> **⚠️ Déviation 9 (timing & dispersion, calibré empiriquement).**
>
> *Accumulation du délai.* Seul `afk` introduit du délai. On maintient un délai **accumulé** depuis
> la dernière action **consommatrice** (`bot`/`up`) : `afk(ms)` l'augmente (et s'enregistre lui-même
> avec le bucket courant) **sans** le remettre à zéro ; un appel de `bot` ou un `up` s'enregistre
> avec le bucket courant **puis** remet l'accumulateur à 0. Ainsi, dans le golden, chaque `kill`
> hérite du bucket de l'`afk` qui l'a précédé : `afk rand` → buckets de kills dispersés (survie),
> `afk 3000` → buckets de kills constants (ban). Sans cela, les `kill` (instantanés) tomberaient
> tous dans le bucket 0 et banniraient même avec `afk rand`.
>
> *Taille de bucket.* `bucket_ms = 25` par défaut (au lieu de 500). Avec `afk rand(2000, 5000)`,
> 500 ms ne donne que ~7 buckets distincts : la fenêtre (K=8) collisionne trop souvent et la
> suspicion **dérive vers le ban malgré la variabilité** (vérifié : ban sur certains seeds). À
> 25 ms, `afk rand(2000, 5000)` couvre ~120 buckets → survie robuste (0 ban sur 50 seeds), tandis
> que `afk 3000` reste un bucket fixe → ban systématique. Surchargeable par `#bucket_ms`.

Si `suspicion >= SEUIL_BAN` → le runtime lève **`BAN`** :

```
error: compte banni
```

État effacé, **terminaison immédiate**, code retour **non-nul**. Pas de `gg`, pas de cleanup.

### 1.3 — Cooldowns sur les `bot`

`bot f() coute N pa, cd M { ... }` → après un appel à `f`, `f` est indisponible pendant `M` tours.

- Table runtime : `{ id_bot -> n° de tour de disponibilité }`. Appel de `f` au tour `T` →
  `dispo[f] = T + M`. `f` est donc rappelable au tour `T + M` (et indisponible pour `T+1 … T+M-1`).
  `cd 0` = rappelable le tour même.
- Builtin `cd_pret(f) -> flag` : vrai si `tour_courant >= dispo[f]` (perception, 0 PA).
- Appeler un `bot` en cooldown → erreur runtime :

```
error: sort en cooldown
```

### 1.4 — (Bonus, Phase 6) Dérive de patch

À chaque `cdc build`, un seed renumérote les tags internes des champs de `perso` et des variants
de `pano`, **sauf** ceux épinglés (`@N`). Un code épinglé survit ; un code paresseux casse au
rebuild. À faire une fois le reste stable.

### 1.5 — Turing-complétude & écrivabilité

**Argument de Turing-complétude.** Le sous-langage de la *couche calcul* est un langage WHILE :
variables entières mutables (`loot`), affectation, arithmétique (`+ - * /`), test (`detect`) et
boucle non bornée (`farm`). Un langage WHILE de cette forme simule une machine à compteurs, donc
est Turing-complet. Comme (Déviation 6) la couche calcul **ne lève jamais de suspicion** et que
`passer` (avancer d'un tour, régénérer le budget) n'est pas une action observable, **un programme
purement calculatoire s'exécute sans limite de durée** : le budget PA/PM ne borne que le travail
*par tour*, jamais le nombre de tours. La contrainte de gameplay **ne retire donc pas** la
Turing-complétude — elle structure seulement *comment* on étale le calcul dans le temps.

> Réserve usuelle des toy langs : `kamas` est un i64 (mémoire bornée comme dans tout langage réel).
> La Turing-complétude est entendue au sens du modèle (entiers non bornés) ; un type `bignum`
> pourra être ajouté plus tard si on veut être pédant.

**Écrivabilité — exemple de calcul pur (zéro suspicion).** Somme de 1 à n, étalée sur les tours :

```
// gg wp
connexion {
    loot n   : kamas = 100
    loot i   : kamas = 1
    loot acc : kamas = 0

    farm i <= n {
        tour {
            acc += i      // couche calcul : 1 PA, 0 suspicion
            i   += 1      // 1 PA
        }
        passer            // tour suivant, budget régénéré
    }
    up "fin du calcul"    // une seule action observable, en fin
}
```

Ce programme ne lance aucun `bot`, ne fait aucun `afk` : **suspicion reste 0**, il termine quel que
soit `n`. Preuve par l'exemple que la logique « ordinaire » est triviale à écrire.

**Écrivabilité — le puzzle n'apparaît que sur la couche action.** Dès qu'on interagit avec le jeu
(appels de `bot`, `afk`, `up` en boucle), il faut introduire de la variabilité (`afk rand`) pour
tenir la suspicion sous le seuil. C'est le golden test (§10).

**Garantie de survie (calibrage).** Sur la couche action, la dérive moyenne de suspicion par action
vaut `PENALITE · p − DECAY · (1 − p)`, où `p ≈ (nb d'entrées même-id dans la fenêtre) / B` est la
probabilité de collision (`B` = nombre de buckets distincts atteignables, ≈ `plage_afk / bucket_ms`).
Avec `PENALITE=7, DECAY=3` la dérive est négative dès que `p < 0,3`. Mais sur un farm long
(le golden ≈ 16 000 actions), une dérive faiblement négative ne suffit pas : il faut `p` **petit**
pour que les excursions n'atteignent jamais le seuil. D'où `bucket_ms = 25` (Déviation 9) :
`afk rand(2000, 5000)` ⇒ `B ≈ 120`, `p ≈ 0,03` ⇒ survie robuste (0 ban sur 50 seeds), alors que
`afk 3000` ⇒ `B = 1`, `p = 1` ⇒ ban systématique. → Il existe toujours une stratégie permettant un
farm observable **illimité** ; le skill consiste à disperser assez les délais.

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
`grind` (+ contextuels `de` / `a`), `loot` (var mutable), `ban` (constante), `gg` (return),
`detect` (if), `sinon` (else), `up` / `afk` (statements à mot-clé, voir ci-dessous),
`et` / `ou` / `pas`, `pa` / `pm` / `suspicion` (pseudo-variables runtime en lecture seule).

> **⚠️ Déviation 7 (C2 — syntaxe `up`/`afk`).** La spec d'origine notait `up(txt)` / `afk(ms)`
> (appel parenthésé), mais le golden utilise `up "..."` et `afk 3000` / `afk rand(2000,5000)`
> (mot-clé + argument nu). On tranche : **`up` et `afk` sont des *statements* à mot-clé prenant un
> seul `expr`**, sans parenthèses. `afk rand(2000, 5000)` n'est PAS une forme spéciale : c'est
> `afk <expr>` où `expr` est l'appel du builtin `rand(2000, 5000)`. `afk 3000` est `afk <expr>`
> avec un littéral. Le bucket de suspicion est calculé sur la valeur ms effective, quelle que soit
> sa provenance.

**Builtins (fonctions, fournies par `cdc-runtime`, appel parenthésé) :** `rand(a, b) -> kamas`,
`cd_pret(bot) -> flag` (l'argument est le nom d'un `bot`, pas une valeur),
`butin(min, max) -> kamas` (stub RNG seedable).

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

Les commentaires `// …` (jusqu'à la fin de ligne) sont des *trivia* ignorés **partout** par le
lexer. Cas particulier easter egg : la **ligne 1** du fichier doit être exactement `// gg wp`
(§4) ; au-delà, les commentaires sont libres.

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
            | farm_stmt | grind_stmt | detect_stmt | gg_stmt
            | up_stmt | afk_stmt | expr_stmt ;

up_stmt     = "up"  , expr ;     (* expr de type txt *)
afk_stmt    = "afk" , expr ;     (* expr de type kamas (ms) ; ex: afk 3000 | afk rand(2000,5000) *)

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
                et/ou/pas ; appels builtin f(args) ; appels de bot ;
                littéraux kamas/flag/txt ; pa/pm/suspicion ; IDENT *) ;
```

**Précédence des opérateurs** (du plus fort au plus faible), tous **associatifs à gauche** sauf le
`pas` unaire :

1. `pas` (négation unaire), `-` unaire
2. `*` `/`
3. `+` `-`
4. comparaisons `<` `<=` `>` `>=` `==` `!=`
5. `et`
6. `ou`

**`grind IDENT de A a B { … }`** : boucle bornée sur `IDENT`, **bornes incluses** `[A, B]`, pas de
`+1`. Si `A > B`, zéro itération. `IDENT` est une variable de boucle en lecture seule, scope = le
corps. (`de` / `a` sont des keywords contextuels.)

**`serveur IDENT`** : déclaration de namespace **no-op en v1** (purement décorative ; aucun effet
sémantique). Réservé pour un éventuel système de modules ultérieur.

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

**Comportement attendu :** le `tour` coûte `4 (appel) + 1 (+=) = 5 PA ≤ 6` et `1 PM ≤ 3` (lecture de `total`) → passe
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

---

## 12. Diagnostics & codes de retour

| Diagnostic | Quand | Phase | Sortie / code retour |
|---|---|---|---|
| `error: candidature contributeur refusée` | ligne 1 ≠ `// gg wp` | compile (toutes commandes) | code ≠ 0, aucune compilation |
| erreurs lexer/parser (ligne:col) | syntaxe invalide | compile | code ≠ 0 |
| erreurs de résolution/typage (ligne:col) | sema | `check`/`run`/`build` | code ≠ 0 |
| `error[E-PA]: tour trop gourmand — N PA demandés, budget max M` | `tour` statiquement > MAX_PA | sema | code ≠ 0 |
| `error[E-PM]: tour trop gourmand — N PM demandés, budget max M` | `tour` statiquement > MAX_PM | sema | code ≠ 0 |
| panic `PaInsuffisant` / `PmInsuffisant` | dépassement budget dynamique | runtime | code ≠ 0 |
| `error: sort en cooldown` | appel d'un `bot` indisponible | runtime | code ≠ 0 |
| `error: compte banni` | `suspicion >= SEUIL_BAN` | runtime | **terminaison immédiate**, état effacé, code ≠ 0, pas de `gg`/cleanup |

**Code de retour :** `0` = exécution normale terminée ; **tout** diagnostic ci-dessus ⇒ code ≠ 0.
Les tests d'acceptation (§9) fixent `CDC_SEED` pour rendre §9.1 (succès) et §9.2 (ban)
déterministes.
