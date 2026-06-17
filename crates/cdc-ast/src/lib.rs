//! Types de l'arbre syntaxique (AST) de cadernislang.
//!
//! Couvre la grammaire de `docs/SPEC.md` §6. Chaque `Expr` porte sa position (ligne, colonne)
//! pour les diagnostics des phases ultérieures (sema PA/PM, runtime).

/// Programme complet : pragmas d'en-tête puis items de premier niveau.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub pragmas: Vec<Pragma>,
    pub items: Vec<Item>,
}

/// Pragma d'en-tête `#clé valeur` (SPEC §5), p. ex. `#max_pa 8`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pragma {
    pub key: String,
    pub value: i64,
    pub line: u32,
    pub col: u32,
}

/// Item de premier niveau.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// `serveur IDENT` — namespace no-op en v1 (SPEC §6).
    Serveur(String),
    /// Déclaration de `bot` (fonction/sort).
    Bot(Bot),
    /// `connexion { … }` — point d'entrée (le `main`).
    Connexion(Block),
    /// `pano Nom { [@N] Variant, … }` — énumération à tags dérivants (SPEC §1.4, Phase 6).
    Pano(Pano),
}

/// Déclaration d'énumération `pano`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pano {
    pub name: String,
    pub variants: Vec<Variant>,
    pub line: u32,
    pub col: u32,
}

/// Variant de `pano` : nom + tag épinglé optionnel (`@N`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: String,
    pub pin: Option<i64>,
}

/// Déclaration de `bot` : `bot f(params) [: type] [, coute N pa] [, cd M] { … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bot {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    /// Coût PA déclaré (`coute N pa`). `None` ⇒ 0 par défaut.
    pub cost_pa: Option<i64>,
    /// Cooldown en tours (`cd M`). `None` ⇒ 0 (pas de cooldown).
    pub cd: Option<i64>,
    pub body: Block,
    pub line: u32,
    pub col: u32,
}

/// Paramètre formel `IDENT : type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

/// Types du langage (SPEC §2). Un nom inconnu devient [`Type::Named`] (`perso`/`pano`, Phase 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Kamas,
    Flag,
    Txt,
    AfkTotal,
    Named(String),
}

/// Bloc `{ stmt* }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

/// Opérateur d'affectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Set,
    /// `+=`
    Add,
    /// `-=`
    Sub,
}

/// Instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `loot IDENT [: type] = expr` — variable mutable.
    Loot {
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    /// `ban IDENT [: type] = expr` — constante.
    Ban {
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    /// `IDENT (= | += | -=) expr`.
    Assign {
        name: String,
        op: AssignOp,
        value: Expr,
        line: u32,
        col: u32,
    },
    /// `tour { … }`.
    Tour(Block),
    /// `passer`.
    Passer,
    /// `farm cond { … }` — boucle « tant que ».
    Farm { cond: Expr, body: Block },
    /// `grind IDENT de A a B { … }` — boucle bornée, bornes incluses (SPEC §6).
    Grind {
        var: String,
        from: Expr,
        to: Expr,
        body: Block,
    },
    /// `detect cond { … } [sinon { … }]`.
    Detect {
        cond: Expr,
        then_branch: Block,
        else_branch: Option<Block>,
    },
    /// `gg [expr]` — return.
    Gg(Option<Expr>),
    /// `up expr` — action observable (SPEC §1.2, Déviation 7).
    Up(Expr),
    /// `afk expr` — avance l'horloge virtuelle, action observable.
    Afk(Expr),
    /// Expression utilisée comme instruction (p. ex. un appel de `bot`).
    Expr(Expr),
}

/// Opérateur unaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `-` arithmétique.
    Neg,
    /// `pas` (négation booléenne).
    Not,
}

/// Opérateur binaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

/// Expression annotée de sa position source.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: u32,
    pub col: u32,
}

/// Forme d'une expression.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    /// `legit` (true) / `cheat` (false).
    Bool(bool),
    Str(String),
    Var(String),
    /// Pseudo-variables runtime en lecture seule (SPEC §3).
    Pa,
    Pm,
    Suspicion,
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// Appel `f(args)` : builtin (`rand`/`butin`/`cd_pret`) ou `bot`.
    Call(String, Vec<Expr>),
    /// `Pano.Variant` → tag (entier, dérivant selon le seed de patch). SPEC §1.4.
    Path(String, String),
}
