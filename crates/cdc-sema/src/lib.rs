//! Analyse sémantique : résolution de noms, typage léger, et **vérification statique du budget
//! PA/PM** d'un `tour` (SPEC §1.1 / §1.1.b, §9.3).
//!
//! La vérif budget ne s'applique qu'aux `tour` au coût **statiquement calculable** (sans boucle
//! ni `tour` imbriqué) ; les coûts dynamiques sont laissés au runtime (`PaInsuffisant`).

use cdc_ast::*;
use cdc_runtime::Config;
use std::collections::{HashMap, HashSet};

/// Diagnostic sémantique localisé, éventuellement porteur d'un code (`E-PA`, `E-PM`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemaError {
    pub code: Option<&'static str>,
    pub msg: String,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for SemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(c) => write!(
                f,
                "error[{c}]: {} (ligne {}, colonne {})",
                self.msg, self.line, self.col
            ),
            None => write!(
                f,
                "error: {} (ligne {}, colonne {})",
                self.msg, self.line, self.col
            ),
        }
    }
}

/// Type inféré (superset des types AST + `Unknown` pour rester indulgent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Kamas,
    Flag,
    Txt,
    AfkTotal,
    Unknown,
}

fn ty_of(t: &Type) -> Ty {
    match t {
        Type::Kamas => Ty::Kamas,
        Type::Flag => Ty::Flag,
        Type::Txt => Ty::Txt,
        Type::AfkTotal => Ty::AfkTotal,
        Type::Named(_) => Ty::Unknown,
    }
}

#[derive(Clone)]
struct VarInfo {
    ty: Ty,
    is_const: bool,
}

struct BotSig {
    params: Vec<Ty>,
    ret: Ty,
}

/// Analyse complète. Retourne tous les diagnostics (vide ⇒ programme valide).
pub fn check(program: &Program) -> Vec<SemaError> {
    let mut cfg = Config::default();
    for p in &program.pragmas {
        cfg.apply(&p.key, p.value);
    }
    let effective = effective_costs(program, &cfg);
    let mut s = Sema {
        errors: Vec::new(),
        scopes: vec![HashMap::new()],
        bots: HashMap::new(),
        panos: HashMap::new(),
        persos: HashMap::new(),
        effective,
        cfg,
    };
    s.run(program);
    s.errors
}

struct Sema {
    errors: Vec<SemaError>,
    scopes: Vec<HashMap<String, VarInfo>>,
    bots: HashMap<String, BotSig>,
    /// `pano` → ensemble des noms de variants.
    panos: HashMap<String, HashSet<String>>,
    /// `perso` → ensemble des noms de champs.
    persos: HashMap<String, HashSet<String>>,
    /// Coût PA effectif par `bot` (déclaré ou auto-dérivé, Phase 7).
    effective: HashMap<String, i64>,
    cfg: Config,
}

impl Sema {
    fn err(&mut self, code: Option<&'static str>, msg: impl Into<String>, line: u32, col: u32) {
        self.errors.push(SemaError {
            code,
            msg: msg.into(),
            line,
            col,
        });
    }

    fn run(&mut self, program: &Program) {
        // 1) enregistrer toutes les signatures de bots (forward refs autorisées).
        for item in &program.items {
            if let Item::Bot(b) = item {
                let sig = BotSig {
                    params: b.params.iter().map(|p| ty_of(&p.ty)).collect(),
                    ret: b.ret.as_ref().map(ty_of).unwrap_or(Ty::AfkTotal),
                };
                self.bots.insert(b.name.clone(), sig);
            }
        }

        // pano / perso : enregistrer membres et valider les épinglages (SPEC §1.4).
        for item in &program.items {
            match item {
                Item::Pano(p) => {
                    let names = p.variants.iter().map(|v| v.name.clone()).collect();
                    self.panos.insert(p.name.clone(), names);
                    let pins: Vec<Option<i64>> = p.variants.iter().map(|v| v.pin).collect();
                    if let Err(e) = cdc_runtime::patch::layout(&pins, self.cfg.patch_seed) {
                        self.err(None, format!("pano « {} » : {e}", p.name), p.line, p.col);
                    }
                }
                Item::Perso(p) => {
                    let names = p.fields.iter().map(|f| f.name.clone()).collect();
                    self.persos.insert(p.name.clone(), names);
                    let pins: Vec<Option<i64>> = p.fields.iter().map(|f| f.pin).collect();
                    if let Err(e) = cdc_runtime::patch::layout(&pins, self.cfg.patch_seed) {
                        self.err(None, format!("perso « {} » : {e}", p.name), p.line, p.col);
                    }
                }
                _ => {}
            }
        }

        let mut has_connexion = false;
        for item in &program.items {
            match item {
                Item::Serveur(_) => {}
                Item::Pano(_) | Item::Perso(_) => {}
                Item::Bot(b) => self.check_bot(b),
                Item::Connexion(blk) => {
                    has_connexion = true;
                    self.check_block(blk);
                }
            }
        }
        if !has_connexion {
            self.err(None, "aucune connexion (point d'entrée)", 1, 1);
        }
    }

    fn check_bot(&mut self, b: &Bot) {
        self.scopes.push(HashMap::new());
        for p in &b.params {
            self.declare(&p.name, ty_of(&p.ty), false);
        }
        self.check_block(&b.body);
        self.scopes.pop();
    }

    // ---- scopes ----

    fn declare(&mut self, name: &str, ty: Ty, is_const: bool) {
        self.scopes
            .last_mut()
            .expect("scope")
            .insert(name.to_string(), VarInfo { ty, is_const });
    }

    fn lookup(&self, name: &str) -> Option<&VarInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    // ---- statements ----

    fn check_block(&mut self, b: &Block) {
        self.scopes.push(HashMap::new());
        for s in &b.stmts {
            self.check_stmt(s);
        }
        self.scopes.pop();
    }

    fn check_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Loot { name, ty, value } | Stmt::Ban { name, ty, value } => {
                let vty = self.infer(value);
                let declared = ty.as_ref().map(ty_of).unwrap_or(vty);
                let is_const = matches!(s, Stmt::Ban { .. });
                self.declare(name, declared, is_const);
            }
            Stmt::Assign {
                name,
                value,
                line,
                col,
                ..
            } => {
                match self.lookup(name) {
                    None => self.err(None, format!("variable indéfinie « {name} »"), *line, *col),
                    Some(vi) if vi.is_const => self.err(
                        None,
                        format!("affectation d'une constante « {name} »"),
                        *line,
                        *col,
                    ),
                    Some(_) => {}
                }
                self.infer(value);
            }
            Stmt::Tour(block) => {
                self.analyze_budget(block);
                self.check_block(block);
            }
            Stmt::Passer => {}
            Stmt::Farm { cond, body } => {
                self.expect_cond(cond);
                self.check_block(body);
            }
            Stmt::Grind {
                var,
                from,
                to,
                body,
            } => {
                self.expect_kamas(from);
                self.expect_kamas(to);
                self.scopes.push(HashMap::new());
                self.declare(var, Ty::Kamas, true);
                // corps dans le scope de la variable de boucle
                for s in &body.stmts {
                    self.check_stmt(s);
                }
                self.scopes.pop();
            }
            Stmt::Detect {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expect_cond(cond);
                self.check_block(then_branch);
                if let Some(e) = else_branch {
                    self.check_block(e);
                }
            }
            Stmt::Gg(opt) => {
                if let Some(e) = opt {
                    self.infer(e);
                }
            }
            Stmt::Up(e) => {
                let t = self.infer(e);
                if t != Ty::Txt && t != Ty::Unknown {
                    self.err(None, "« up » attend une valeur txt", e.line, e.col);
                }
            }
            Stmt::Afk(e) => {
                self.expect_kamas(e);
            }
            Stmt::Expr(e) => {
                self.infer(e);
            }
        }
    }

    // ---- typage (léger, indulgent : Unknown ne déclenche rien) ----

    fn infer(&mut self, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::Int(_) => Ty::Kamas,
            ExprKind::Bool(_) => Ty::Flag,
            ExprKind::Str(_) => Ty::Txt,
            ExprKind::Pa | ExprKind::Pm | ExprKind::Suspicion => Ty::Kamas,
            ExprKind::Var(name) => match self.lookup(name) {
                Some(vi) => vi.ty,
                None => {
                    self.err(
                        None,
                        format!("variable indéfinie « {name} »"),
                        e.line,
                        e.col,
                    );
                    Ty::Unknown
                }
            },
            ExprKind::Unary(op, inner) => {
                let t = self.infer(inner);
                match op {
                    UnOp::Neg => {
                        self.require(
                            t,
                            Ty::Kamas,
                            inner.line,
                            inner.col,
                            "négation sur non-kamas",
                        );
                        Ty::Kamas
                    }
                    UnOp::Not => {
                        self.require(t, Ty::Flag, inner.line, inner.col, "« pas » sur non-flag");
                        Ty::Flag
                    }
                }
            }
            ExprKind::Binary(op, l, r) => self.infer_binary(*op, l, r),
            ExprKind::Call(name, args) => self.infer_call(name, args, e.line, e.col),
            ExprKind::Path(left, member) => {
                if let Some(variants) = self.panos.get(left) {
                    // Pano.Variant → tag
                    if !variants.contains(member) {
                        self.err(
                            None,
                            format!("variant « {member} » inconnu dans pano « {left} »"),
                            e.line,
                            e.col,
                        );
                    }
                    Ty::Kamas
                } else if let Some(fields) = self.persos.get(left) {
                    // Perso.champ → tag du champ (dérivant)
                    if !fields.contains(member) {
                        self.err(
                            None,
                            format!("champ « {member} » inconnu dans perso « {left} »"),
                            e.line,
                            e.col,
                        );
                    }
                    Ty::Kamas
                } else if self.lookup(left).is_some() {
                    // variable.champ → accès de champ (type non suivi finement)
                    Ty::Unknown
                } else {
                    self.err(
                        None,
                        format!("« {left} » n'est ni un pano, ni un perso, ni une variable"),
                        e.line,
                        e.col,
                    );
                    Ty::Unknown
                }
            }
            ExprKind::Struct(ty, fields) => {
                match self.persos.get(ty).cloned() {
                    Some(decl_fields) => {
                        for (fname, _) in fields {
                            if !decl_fields.contains(fname) {
                                self.err(
                                    None,
                                    format!("champ « {fname} » inconnu dans perso « {ty} »"),
                                    e.line,
                                    e.col,
                                );
                            }
                        }
                    }
                    None => self.err(None, format!("perso « {ty} » inconnu"), e.line, e.col),
                }
                for (_, v) in fields {
                    self.infer(v);
                }
                Ty::Unknown
            }
        }
    }

    fn infer_binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Ty {
        let lt = self.infer(l);
        let rt = self.infer(r);
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                self.require(
                    lt,
                    Ty::Kamas,
                    l.line,
                    l.col,
                    "opérande arithmétique non-kamas",
                );
                self.require(
                    rt,
                    Ty::Kamas,
                    r.line,
                    r.col,
                    "opérande arithmétique non-kamas",
                );
                Ty::Kamas
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                self.require(lt, Ty::Kamas, l.line, l.col, "comparaison sur non-kamas");
                self.require(rt, Ty::Kamas, r.line, r.col, "comparaison sur non-kamas");
                Ty::Flag
            }
            BinOp::Eq | BinOp::Ne => Ty::Flag,
            BinOp::And | BinOp::Or => {
                self.require(lt, Ty::Flag, l.line, l.col, "opérande booléen non-flag");
                self.require(rt, Ty::Flag, r.line, r.col, "opérande booléen non-flag");
                Ty::Flag
            }
        }
    }

    fn infer_call(&mut self, name: &str, args: &[Expr], line: u32, col: u32) -> Ty {
        match name {
            "rand" | "butin" => {
                if args.len() != 2 {
                    self.err(None, format!("« {name} » attend 2 arguments"), line, col);
                }
                for a in args {
                    self.expect_kamas(a);
                }
                Ty::Kamas
            }
            "cd_pret" => {
                match args.first().map(|a| &a.kind) {
                    Some(ExprKind::Var(b)) if self.bots.contains_key(b) => {}
                    _ => self.err(None, "« cd_pret » attend un nom de bot", line, col),
                }
                Ty::Flag
            }
            _ => {
                // appel de bot
                let (ret, nparams) = match self.bots.get(name) {
                    Some(sig) => (sig.ret, sig.params.len()),
                    None => {
                        self.err(None, format!("bot inconnu « {name} »"), line, col);
                        return Ty::Unknown;
                    }
                };
                if args.len() != nparams {
                    self.err(
                        None,
                        format!(
                            "« {name} » attend {nparams} argument(s), reçu {}",
                            args.len()
                        ),
                        line,
                        col,
                    );
                }
                for a in args {
                    self.infer(a);
                }
                ret
            }
        }
    }

    fn require(&mut self, got: Ty, want: Ty, line: u32, col: u32, msg: &str) {
        if got != Ty::Unknown && got != want {
            self.err(None, msg.to_string(), line, col);
        }
    }

    fn expect_kamas(&mut self, e: &Expr) {
        let t = self.infer(e);
        if t != Ty::Kamas && t != Ty::Unknown {
            self.err(None, "valeur kamas attendue", e.line, e.col);
        }
    }

    fn expect_cond(&mut self, e: &Expr) {
        let t = self.infer(e);
        if t != Ty::Flag && t != Ty::Kamas && t != Ty::Unknown {
            self.err(None, "condition non booléenne", e.line, e.col);
        }
    }

    // ---- vérification statique du budget (SPEC §1.1, §9.3) ----

    fn analyze_budget(&mut self, body: &Block) {
        // bornes : si le tour contient une boucle ou un tour imbriqué, le coût est dynamique.
        if contains_dynamic(body) {
            return;
        }
        let line_col = first_pos(body).unwrap_or((1, 1));

        let pa = block_pa(body, &self.effective, &self.cfg);
        if pa > self.cfg.max_pa {
            self.err(
                Some("E-PA"),
                format!(
                    "tour trop gourmand — {pa} PA demandés, budget max {}",
                    self.cfg.max_pa
                ),
                line_col.0,
                line_col.1,
            );
        }

        // PM (model B) : variables externes distinctes lues hors condition.
        let mut locals = HashSet::new();
        collect_locals(body, &mut locals);
        let mut externals = HashSet::new();
        block_external_reads(body, &locals, &mut externals);
        let pm = externals.len() as i64;
        if pm > self.cfg.max_pm {
            self.err(
                Some("E-PM"),
                format!(
                    "tour trop gourmand — {pm} PM demandés, budget max {}",
                    self.cfg.max_pm
                ),
                line_col.0,
                line_col.1,
            );
        }
    }
}

/// Coût PA (pire chemin) d'un bloc sans boucle.
fn block_pa(b: &Block, eff: &HashMap<String, i64>, cfg: &Config) -> i64 {
    b.stmts.iter().map(|s| stmt_pa(s, eff, cfg)).sum()
}

fn stmt_pa(s: &Stmt, eff: &HashMap<String, i64>, cfg: &Config) -> i64 {
    match s {
        Stmt::Loot { value, .. } | Stmt::Ban { value, .. } => cfg.assign_pa + expr_pa(value, eff),
        Stmt::Assign { value, .. } => cfg.assign_pa + expr_pa(value, eff),
        Stmt::Up(e) => cfg.up_pa + expr_pa(e, eff),
        Stmt::Afk(e) => expr_pa(e, eff), // afk = 0 PA, mais l'arg peut contenir un appel
        Stmt::Expr(e) | Stmt::Gg(Some(e)) => expr_pa(e, eff),
        Stmt::Gg(None) | Stmt::Passer => 0,
        Stmt::Detect {
            cond,
            then_branch,
            else_branch,
        } => {
            let e = else_branch
                .as_ref()
                .map(|b| block_pa(b, eff, cfg))
                .unwrap_or(0);
            expr_pa(cond, eff) + block_pa(then_branch, eff, cfg).max(e)
        }
        // exclus par contains_dynamic
        Stmt::Tour(_) | Stmt::Farm { .. } | Stmt::Grind { .. } => 0,
    }
}

/// PA d'une expression = somme des coûts effectifs des appels de bot qu'elle contient.
fn expr_pa(e: &Expr, eff: &HashMap<String, i64>) -> i64 {
    match &e.kind {
        ExprKind::Unary(_, x) => expr_pa(x, eff),
        ExprKind::Binary(_, l, r) => expr_pa(l, eff) + expr_pa(r, eff),
        ExprKind::Call(name, args) => {
            let here = match name.as_str() {
                "rand" | "butin" | "cd_pret" => 0,
                _ => eff.get(name).copied().unwrap_or(0),
            };
            here + args.iter().map(|a| expr_pa(a, eff)).sum::<i64>()
        }
        _ => 0,
    }
}

/// Collecte les variables externes (non locales au tour) lues **hors condition**.
fn block_external_reads(b: &Block, locals: &HashSet<String>, out: &mut HashSet<String>) {
    for s in &b.stmts {
        match s {
            Stmt::Loot { value, .. } | Stmt::Ban { value, .. } => expr_reads(value, locals, out),
            Stmt::Assign {
                name, value, op, ..
            } => {
                if !matches!(op, AssignOp::Set) && !locals.contains(name) {
                    out.insert(name.clone());
                }
                expr_reads(value, locals, out);
            }
            Stmt::Up(e) | Stmt::Afk(e) | Stmt::Expr(e) | Stmt::Gg(Some(e)) => {
                expr_reads(e, locals, out)
            }
            Stmt::Detect {
                then_branch,
                else_branch,
                ..
            } => {
                block_external_reads(then_branch, locals, out);
                if let Some(eb) = else_branch {
                    block_external_reads(eb, locals, out);
                }
            }
            Stmt::Gg(None) | Stmt::Passer => {}
            Stmt::Tour(_) | Stmt::Farm { .. } | Stmt::Grind { .. } => {}
        }
    }
}

/// Lectures de variables externes dans une expression (hors pseudo-vars).
fn expr_reads(e: &Expr, locals: &HashSet<String>, out: &mut HashSet<String>) {
    match &e.kind {
        ExprKind::Var(name) => {
            if !locals.contains(name) {
                out.insert(name.clone());
            }
        }
        ExprKind::Unary(_, x) => expr_reads(x, locals, out),
        ExprKind::Binary(_, l, r) => {
            expr_reads(l, locals, out);
            expr_reads(r, locals, out);
        }
        ExprKind::Call(name, args) => {
            // `cd_pret(bot)` : l'argument est un nom de bot, pas une lecture de variable.
            if name != "cd_pret" {
                for a in args {
                    expr_reads(a, locals, out);
                }
            }
        }
        _ => {}
    }
}

/// `true` si le bloc contient une boucle ou un `tour` imbriqué (coût dynamique).
fn contains_dynamic(b: &Block) -> bool {
    b.stmts.iter().any(|s| match s {
        Stmt::Farm { .. } | Stmt::Grind { .. } | Stmt::Tour(_) => true,
        Stmt::Detect {
            then_branch,
            else_branch,
            ..
        } => {
            contains_dynamic(then_branch)
                || else_branch.as_ref().map(contains_dynamic).unwrap_or(false)
        }
        _ => false,
    })
}

/// Noms déclarés (loot/ban) n'importe où dans le tour (= locaux au tour).
fn collect_locals(b: &Block, out: &mut HashSet<String>) {
    for s in &b.stmts {
        match s {
            Stmt::Loot { name, .. } | Stmt::Ban { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Detect {
                then_branch,
                else_branch,
                ..
            } => {
                collect_locals(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_locals(eb, out);
                }
            }
            _ => {}
        }
    }
}

/// Position du premier statement (pour localiser le diagnostic du tour).
fn first_pos(b: &Block) -> Option<(u32, u32)> {
    b.stmts.iter().find_map(stmt_pos)
}

fn stmt_pos(s: &Stmt) -> Option<(u32, u32)> {
    match s {
        Stmt::Assign { line, col, .. } => Some((*line, *col)),
        Stmt::Loot { value, .. }
        | Stmt::Ban { value, .. }
        | Stmt::Up(value)
        | Stmt::Afk(value)
        | Stmt::Expr(value)
        | Stmt::Gg(Some(value)) => Some((value.line, value.col)),
        Stmt::Detect { cond, .. } | Stmt::Farm { cond, .. } => Some((cond.line, cond.col)),
        _ => None,
    }
}

// ============================================================================================
// Coût PA effectif d'un `bot` (Phase 7) : `coute N pa` explicite, sinon **auto-dérivé** de la
// somme des coûts du corps (pire chemin). Source unique partagée interp ↔ codegen ↔ sema.
// ============================================================================================

/// Calcule le coût PA effectif de chaque `bot` du programme.
///
/// Règle : si `coute N pa` est déclaré, c'est `N` (override « coût imposé par le jeu ») ; sinon le
/// coût est la **somme des opérations du corps** (affectations, `up`, appels de bots imbriqués…),
/// pire chemin sur les `detect`. Les cycles d'appels contribuent 0 (garde anti-récursion).
pub fn effective_costs(program: &Program, cfg: &Config) -> HashMap<String, i64> {
    let bots: HashMap<String, &Bot> = program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Bot(b) => Some((b.name.clone(), b)),
            _ => None,
        })
        .collect();
    let mut memo = HashMap::new();
    let mut visiting = HashSet::new();
    let names: Vec<String> = bots.keys().cloned().collect();
    for name in names {
        cost_of_bot(&name, &bots, cfg, &mut visiting, &mut memo);
    }
    memo
}

fn cost_of_bot(
    name: &str,
    bots: &HashMap<String, &Bot>,
    cfg: &Config,
    visiting: &mut HashSet<String>,
    memo: &mut HashMap<String, i64>,
) -> i64 {
    if let Some(c) = memo.get(name) {
        return *c;
    }
    let bot = match bots.get(name) {
        Some(b) => *b,
        None => return 0,
    };
    if let Some(c) = bot.cost_pa {
        memo.insert(name.to_string(), c);
        return c;
    }
    if !visiting.insert(name.to_string()) {
        return 0; // cycle d'appels → 0
    }
    let c = block_cost(&bot.body, bots, cfg, visiting, memo);
    visiting.remove(name);
    memo.insert(name.to_string(), c);
    c
}

fn block_cost(
    b: &Block,
    bots: &HashMap<String, &Bot>,
    cfg: &Config,
    visiting: &mut HashSet<String>,
    memo: &mut HashMap<String, i64>,
) -> i64 {
    b.stmts
        .iter()
        .map(|s| stmt_cost(s, bots, cfg, visiting, memo))
        .sum()
}

fn stmt_cost(
    s: &Stmt,
    bots: &HashMap<String, &Bot>,
    cfg: &Config,
    visiting: &mut HashSet<String>,
    memo: &mut HashMap<String, i64>,
) -> i64 {
    match s {
        Stmt::Loot { value, .. } | Stmt::Ban { value, .. } | Stmt::Assign { value, .. } => {
            cfg.assign_pa + expr_cost(value, bots, cfg, visiting, memo)
        }
        Stmt::Up(e) => cfg.up_pa + expr_cost(e, bots, cfg, visiting, memo),
        Stmt::Afk(e) | Stmt::Expr(e) | Stmt::Gg(Some(e)) => expr_cost(e, bots, cfg, visiting, memo),
        Stmt::Gg(None) | Stmt::Passer => 0,
        Stmt::Tour(blk) => block_cost(blk, bots, cfg, visiting, memo),
        Stmt::Farm { body, .. } => block_cost(body, bots, cfg, visiting, memo),
        Stmt::Grind { body, .. } => block_cost(body, bots, cfg, visiting, memo),
        Stmt::Detect {
            cond,
            then_branch,
            else_branch,
        } => {
            let e = else_branch
                .as_ref()
                .map(|b| block_cost(b, bots, cfg, visiting, memo))
                .unwrap_or(0);
            expr_cost(cond, bots, cfg, visiting, memo)
                + block_cost(then_branch, bots, cfg, visiting, memo).max(e)
        }
    }
}

fn expr_cost(
    e: &Expr,
    bots: &HashMap<String, &Bot>,
    cfg: &Config,
    visiting: &mut HashSet<String>,
    memo: &mut HashMap<String, i64>,
) -> i64 {
    match &e.kind {
        ExprKind::Unary(_, x) => expr_cost(x, bots, cfg, visiting, memo),
        ExprKind::Binary(_, l, r) => {
            expr_cost(l, bots, cfg, visiting, memo) + expr_cost(r, bots, cfg, visiting, memo)
        }
        ExprKind::Call(name, args) => {
            let here = match name.as_str() {
                "rand" | "butin" | "cd_pret" => 0,
                _ => cost_of_bot(name, bots, cfg, visiting, memo),
            };
            here + args
                .iter()
                .map(|a| expr_cost(a, bots, cfg, visiting, memo))
                .sum::<i64>()
        }
        ExprKind::Struct(_, fields) => fields
            .iter()
            .map(|(_, v)| expr_cost(v, bots, cfg, visiting, memo))
            .sum(),
        _ => 0,
    }
}

// ============================================================================================
// Rapport de coût (Phase 8) : coût effectif des bots + usage PA/PM par `tour`. Réutilisé par la
// commande `cdc cost` et (à venir) le serveur LSP.
// ============================================================================================

/// Coût effectif d'un `bot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotCost {
    pub name: String,
    pub cost: i64,
    /// `true` si `coute` est déclaré ; `false` si auto-dérivé.
    pub declared: bool,
}

/// Usage budgétaire d'un `tour`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourCost {
    pub line: u32,
    pub col: u32,
    pub pa: i64,
    pub pm: i64,
    /// `true` si coût dynamique (boucle/tour imbriqué) : `pa`/`pm` sont alors indicatifs.
    pub dynamic: bool,
}

/// Rapport de coût d'un programme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostReport {
    pub bots: Vec<BotCost>,
    pub tours: Vec<TourCost>,
    pub max_pa: i64,
    pub max_pm: i64,
}

/// Calcule le rapport de coût : coût effectif de chaque `bot` et usage PA/PM de chaque `tour`.
pub fn cost_report(program: &Program, cfg: &Config) -> CostReport {
    let eff = effective_costs(program, cfg);
    let mut bots: Vec<BotCost> = program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Bot(b) => Some(BotCost {
                name: b.name.clone(),
                cost: eff.get(&b.name).copied().unwrap_or(0),
                declared: b.cost_pa.is_some(),
            }),
            _ => None,
        })
        .collect();
    bots.sort_by(|a, b| a.name.cmp(&b.name));

    let mut tours = Vec::new();
    for item in &program.items {
        match item {
            Item::Connexion(b) => collect_tours(b, &eff, cfg, &mut tours),
            Item::Bot(b) => collect_tours(&b.body, &eff, cfg, &mut tours),
            _ => {}
        }
    }
    CostReport {
        bots,
        tours,
        max_pa: cfg.max_pa,
        max_pm: cfg.max_pm,
    }
}

/// Rapport de coût en construisant la config depuis les pragmas du programme (commodité driver).
pub fn report(program: &Program) -> CostReport {
    let mut cfg = Config::default();
    for p in &program.pragmas {
        cfg.apply(&p.key, p.value);
    }
    cost_report(program, &cfg)
}

fn collect_tours(b: &Block, eff: &HashMap<String, i64>, cfg: &Config, out: &mut Vec<TourCost>) {
    for s in &b.stmts {
        match s {
            Stmt::Tour(body) => {
                let (line, col) = first_pos(body).unwrap_or((1, 1));
                let dynamic = contains_dynamic(body);
                let (pa, pm) = if dynamic {
                    (0, 0)
                } else {
                    let mut locals = HashSet::new();
                    collect_locals(body, &mut locals);
                    let mut ext = HashSet::new();
                    block_external_reads(body, &locals, &mut ext);
                    (block_pa(body, eff, cfg), ext.len() as i64)
                };
                out.push(TourCost {
                    line,
                    col,
                    pa,
                    pm,
                    dynamic,
                });
            }
            Stmt::Farm { body, .. } | Stmt::Grind { body, .. } => {
                collect_tours(body, eff, cfg, out)
            }
            Stmt::Detect {
                then_branch,
                else_branch,
                ..
            } => {
                collect_tours(then_branch, eff, cfg, out);
                if let Some(eb) = else_branch {
                    collect_tours(eb, eff, cfg, out);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errs(src: &str) -> Vec<SemaError> {
        let prog = cdc_parser::parse(src).expect("parse");
        check(&prog)
    }

    fn costs(src: &str) -> std::collections::HashMap<String, i64> {
        let prog = cdc_parser::parse(src).expect("parse");
        effective_costs(&prog, &Config::default())
    }

    #[test]
    fn rapport_de_cout() {
        let prog = cdc_parser::parse(include_str!("../../../examples/dopeuls.cdl")).unwrap();
        let r = report(&prog);
        let bot = r.bots.iter().find(|b| b.name == "tuer_dopeul").unwrap();
        assert_eq!(bot.cost, 4);
        assert!(bot.declared);
        assert_eq!(r.tours.len(), 1);
        assert_eq!(r.tours[0].pa, 5);
        assert_eq!(r.tours[0].pm, 1);
        assert!(!r.tours[0].dynamic);
    }

    #[test]
    fn cout_explicite_prioritaire() {
        let c = costs("// gg wp\nbot f() : kamas, coute 4 pa { gg 1 }\nconnexion {}");
        assert_eq!(c["f"], 4);
    }

    #[test]
    fn cout_auto_derive_du_corps() {
        // sans `coute` : 2 affectations → 2 PA auto-dérivés (Phase 7)
        let c = costs("// gg wp\nbot f() : kamas { loot a = 1\nloot b = 2\ngg b }\nconnexion {}");
        assert_eq!(c["f"], 2);
    }

    #[test]
    fn cout_auto_appels_imbriques() {
        let c = costs(
            "// gg wp\nbot g() : kamas { loot a = 1\ngg a }\nbot f() : kamas { gg g() }\nconnexion {}",
        );
        assert_eq!(c["g"], 1);
        assert_eq!(c["f"], 1, "f hérite du coût de g");
    }

    #[test]
    fn cout_auto_alimente_verif_tour() {
        // bot sans `coute`, corps lourd (7 PA dérivés) → appelé dans un tour → E-PA statique.
        let src = "// gg wp
bot lourd() : kamas { loot a = 0
a = 1
a = 2
a = 3
a = 4
a = 5
a = 6
gg a }
connexion {
    tour { loot x = lourd() }
    passer
}";
        let e = errs(src);
        assert!(
            e.iter().any(|x| x.code == Some("E-PA")),
            "attendu E-PA : {e:?}"
        );
    }

    #[test]
    fn golden_sans_erreur() {
        let src = include_str!("../../../examples/dopeuls.cdl");
        assert!(
            errs(src).is_empty(),
            "golden ne doit pas avoir d'erreur sema"
        );
    }

    #[test]
    fn somme_sans_erreur() {
        let src = include_str!("../../../examples/somme.cdl");
        assert!(errs(src).is_empty());
    }

    #[test]
    fn tour_trop_gourmand_pa() {
        // 7 affectations = 7 PA > 6 → E-PA (§9.3).
        let src = "// gg wp
connexion {
    loot x = 0
    tour {
        x = 1
        x = 2
        x = 3
        x = 4
        x = 5
        x = 6
        x = 7
    }
    passer
}";
        let e = errs(src);
        assert!(
            e.iter().any(|x| x.code == Some("E-PA")),
            "attendu E-PA, eu {e:?}"
        );
    }

    #[test]
    fn tour_trop_gourmand_pm() {
        // 4 variables externes distinctes lues dans un tour → E-PM (MAX_PM=3).
        let src = "// gg wp
connexion {
    loot a = 0
    loot b = 0
    loot c = 0
    loot d = 0
    loot t = 0
    tour {
        t = a
        t = b
        t = c
        t = d
    }
    passer
}";
        let e = errs(src);
        assert!(
            e.iter().any(|x| x.code == Some("E-PM")),
            "attendu E-PM, eu {e:?}"
        );
    }

    #[test]
    fn variable_indefinie() {
        let e = errs("// gg wp\nconnexion { loot x = y }");
        assert!(e.iter().any(|x| x.msg.contains("indéfinie")));
    }

    #[test]
    fn affectation_constante() {
        let e = errs("// gg wp\nconnexion { ban k = 1\n k = 2 }");
        assert!(e.iter().any(|x| x.msg.contains("constante")));
    }

    #[test]
    fn boucle_dans_tour_pas_de_faux_e_pa() {
        // coût dynamique (farm dans tour) → pas de E-PA statique (laissé au runtime).
        let src = "// gg wp
connexion {
    loot x = 0
    tour {
        farm x < 100 { x = x }
    }
    passer
}";
        let e = errs(src);
        assert!(!e.iter().any(|x| x.code == Some("E-PA")));
    }
}
