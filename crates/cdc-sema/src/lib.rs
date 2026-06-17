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
    cost_pa: i64,
}

/// Analyse complète. Retourne tous les diagnostics (vide ⇒ programme valide).
pub fn check(program: &Program) -> Vec<SemaError> {
    let mut cfg = Config::default();
    for p in &program.pragmas {
        cfg.apply(&p.key, p.value);
    }
    let mut s = Sema {
        errors: Vec::new(),
        scopes: vec![HashMap::new()],
        bots: HashMap::new(),
        cfg,
    };
    s.run(program);
    s.errors
}

struct Sema {
    errors: Vec<SemaError>,
    scopes: Vec<HashMap<String, VarInfo>>,
    bots: HashMap<String, BotSig>,
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
                    cost_pa: b.cost_pa.unwrap_or(0),
                };
                self.bots.insert(b.name.clone(), sig);
            }
        }

        let mut has_connexion = false;
        for item in &program.items {
            match item {
                Item::Serveur(_) => {}
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

        let pa = self.block_pa(body);
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
        self.block_external_reads(body, &locals, &mut externals);
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

    /// Coût PA (pire chemin) d'un bloc sans boucle.
    fn block_pa(&self, b: &Block) -> i64 {
        b.stmts.iter().map(|s| self.stmt_pa(s)).sum()
    }

    fn stmt_pa(&self, s: &Stmt) -> i64 {
        match s {
            Stmt::Loot { value, .. } | Stmt::Ban { value, .. } => {
                self.cfg.assign_pa + self.expr_pa(value)
            }
            Stmt::Assign { value, .. } => self.cfg.assign_pa + self.expr_pa(value),
            Stmt::Up(e) => self.cfg.up_pa + self.expr_pa(e),
            Stmt::Afk(e) => self.expr_pa(e), // afk = 0 PA, mais l'arg peut contenir un appel
            Stmt::Expr(e) | Stmt::Gg(Some(e)) => self.expr_pa(e),
            Stmt::Gg(None) | Stmt::Passer => 0,
            Stmt::Detect {
                cond,
                then_branch,
                else_branch,
            } => {
                let e = else_branch.as_ref().map(|b| self.block_pa(b)).unwrap_or(0);
                self.expr_pa(cond) + self.block_pa(then_branch).max(e)
            }
            // exclus par contains_dynamic
            Stmt::Tour(_) | Stmt::Farm { .. } | Stmt::Grind { .. } => 0,
        }
    }

    /// PA d'une expression = somme des `coute` des appels de bot qu'elle contient.
    fn expr_pa(&self, e: &Expr) -> i64 {
        match &e.kind {
            ExprKind::Unary(_, x) => self.expr_pa(x),
            ExprKind::Binary(_, l, r) => self.expr_pa(l) + self.expr_pa(r),
            ExprKind::Call(name, args) => {
                let here = match name.as_str() {
                    "rand" | "butin" | "cd_pret" => 0,
                    _ => self.bots.get(name).map(|s| s.cost_pa).unwrap_or(0),
                };
                here + args.iter().map(|a| self.expr_pa(a)).sum::<i64>()
            }
            _ => 0,
        }
    }

    /// Collecte les variables externes (non locales au tour) lues **hors condition**.
    fn block_external_reads(&self, b: &Block, locals: &HashSet<String>, out: &mut HashSet<String>) {
        for s in &b.stmts {
            match s {
                Stmt::Loot { value, .. } | Stmt::Ban { value, .. } => {
                    expr_reads(value, locals, out)
                }
                Stmt::Assign {
                    name, value, op, ..
                } => {
                    // += / -= lisent la cible ; = ne la lit pas
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
                    // condition = perception (0 PM) ; on ne scanne que les branches
                    self.block_external_reads(then_branch, locals, out);
                    if let Some(eb) = else_branch {
                        self.block_external_reads(eb, locals, out);
                    }
                }
                Stmt::Gg(None) | Stmt::Passer => {}
                // exclus par contains_dynamic
                Stmt::Tour(_) | Stmt::Farm { .. } | Stmt::Grind { .. } => {}
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn errs(src: &str) -> Vec<SemaError> {
        let prog = cdc_parser::parse(src).expect("parse");
        check(&prog)
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
