//! Interpréteur tree-walking : exécute l'AST en s'appuyant sur `cdc-runtime` (SPEC §9.7).
//!
//! Applique la table de coûts PA/PM (SPEC §1.1) — budget débité **uniquement dans un `tour`**
//! (Déviation 1) — et les builtins. La suspicion et les cooldowns arrivent en Phase 3.
//!
//! ## PM — model B (voir SPEC §1.1.b, Déviation 8)
//! Lire une variable d'un scope **extérieur au tour courant** coûte **1 PM, une seule fois par
//! tour** (ensuite « en cache », gratuit). Indépendant de la profondeur d'imbrication : un model
//! « distance × accès » rendrait une simple boucle (`acc += i`) impossible sous MAX_PM=3.

use cdc_ast::*;
use cdc_runtime::{Config, Fault, Runtime};
use std::collections::HashMap;

/// Valeur runtime.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
    /// Instance de `perso` : champ → valeur (SPEC §1.4).
    Struct(HashMap<String, Value>),
    /// `afk_total` — valeur unit.
    Unit,
}

/// Erreur d'exécution.
#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    /// Dépassement de budget (SPEC §1.1).
    Fault(Fault),
    /// `suspicion >= SEUIL_BAN` (SPEC §1.2) — message exact « compte banni ».
    Banni,
    /// Appel d'un `bot` en cooldown (SPEC §1.3) — message exact « sort en cooldown ».
    Cooldown,
    /// Erreur générique (variable indéfinie, type, etc.).
    Msg(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Fault(x) => write!(f, "{x}"),
            RunError::Banni => write!(f, "compte banni"),
            RunError::Cooldown => write!(f, "sort en cooldown"),
            RunError::Msg(m) => write!(f, "{m}"),
        }
    }
}

/// Contrôle de flux interne (propagation de `gg`).
enum Flow {
    Normal,
    Return(Value),
}

/// Interpréteur. Détient le runtime (toute la jouabilité) et l'environnement lexical.
pub struct Interp {
    rt: Runtime,
    bots: HashMap<String, Bot>,
    env: Vec<HashMap<String, Value>>,
    in_tour: bool,
    /// Indice de scope marquant le début du tour courant : un scope d'indice `< tour_base`
    /// est « extérieur au tour » (coûte du PM au premier accès).
    tour_base: usize,
    /// Identifiants PM stables par nom de variable (passés au runtime `pm_touch`, model B).
    pm_ids: HashMap<String, u64>,
    /// Mode perception : lectures gratuites (conditions). SPEC §1.1.
    perception: bool,
    /// Tags `pano` après dérive de patch : (pano, variant) → tag. SPEC §1.4.
    pano_tags: HashMap<String, HashMap<String, i64>>,
    /// Coût PA effectif par `bot` (déclaré ou auto-dérivé, Phase 7).
    bot_costs: HashMap<String, i64>,
}

/// Exécute un programme. Le seed RNG vient du pragma `#seed`, sinon de l'env `CDC_SEED`.
///
/// # Erreurs
/// [`RunError`] sur dépassement de budget ou erreur d'exécution.
pub fn run(program: &Program) -> Result<(), RunError> {
    let mut cfg = Config::default();
    for p in &program.pragmas {
        cfg.apply(&p.key, p.value);
    }
    if cfg.seed.is_none() {
        if let Ok(s) = std::env::var("CDC_SEED") {
            if let Ok(n) = s.parse::<u64>() {
                cfg.seed = Some(n);
            }
        }
    }
    if let Ok(s) = std::env::var("CDC_PATCH_SEED") {
        if let Ok(n) = s.parse::<u64>() {
            cfg.patch_seed = n;
        }
    }

    let mut bots = HashMap::new();
    let mut connexion = None;
    let mut pano_tags: HashMap<String, HashMap<String, i64>> = HashMap::new();
    for item in &program.items {
        match item {
            Item::Bot(b) => {
                bots.insert(b.name.clone(), b.clone());
            }
            Item::Connexion(blk) => connexion = Some(blk.clone()),
            Item::Serveur(_) => {} // no-op (SPEC §6)
            Item::Pano(p) => {
                // dérive de patch : tags permutés par le patch_seed (SPEC §1.4).
                let pins: Vec<Option<i64>> = p.variants.iter().map(|v| v.pin).collect();
                let tags = cdc_runtime::patch::layout(&pins, cfg.patch_seed)
                    .map_err(|e| RunError::Msg(format!("pano « {} » : {e}", p.name)))?;
                let map = p
                    .variants
                    .iter()
                    .zip(tags)
                    .map(|(v, t)| (v.name.clone(), t))
                    .collect();
                pano_tags.insert(p.name.clone(), map);
            }
            Item::Perso(p) => {
                // tags de champs dérivants (mêmes règles que pano, SPEC §1.4).
                let pins: Vec<Option<i64>> = p.fields.iter().map(|f| f.pin).collect();
                let tags = cdc_runtime::patch::layout(&pins, cfg.patch_seed)
                    .map_err(|e| RunError::Msg(format!("perso « {} » : {e}", p.name)))?;
                let map = p
                    .fields
                    .iter()
                    .zip(tags)
                    .map(|(f, t)| (f.name.clone(), t))
                    .collect();
                pano_tags.insert(p.name.clone(), map);
            }
        }
    }
    let connexion =
        connexion.ok_or_else(|| RunError::Msg("aucune connexion (point d'entrée)".to_string()))?;

    let bot_costs = cdc_sema::effective_costs(program, &cfg);

    let mut it = Interp {
        rt: Runtime::new(cfg),
        bots,
        env: Vec::new(),
        in_tour: false,
        tour_base: 0,
        pm_ids: HashMap::new(),
        perception: false,
        pano_tags,
        bot_costs,
    };
    it.exec_block(&connexion)?;
    Ok(())
}

impl Interp {
    // ---------------------------------------------------------------- scopes & variables

    fn lookup(&self, name: &str) -> Option<usize> {
        self.env.iter().rposition(|s| s.contains_key(name))
    }

    /// Identifiant PM stable pour un nom de variable.
    fn pm_id(&mut self, name: &str) -> u64 {
        if let Some(id) = self.pm_ids.get(name) {
            return *id;
        }
        let id = self.pm_ids.len() as u64;
        self.pm_ids.insert(name.to_string(), id);
        id
    }

    /// Lit une variable. Charge 1 PM au premier accès d'une variable externe au tour (model B,
    /// logique portée par `cdc_runtime::Runtime::pm_touch` — invariant §9.7).
    fn read_var(&mut self, name: &str) -> Result<Value, RunError> {
        let idx = self
            .lookup(name)
            .ok_or_else(|| RunError::Msg(format!("variable indéfinie « {name} »")))?;
        if self.in_tour && !self.perception && idx < self.tour_base {
            let id = self.pm_id(name);
            self.rt.pm_touch(id).map_err(RunError::Fault)?;
        }
        Ok(self.env[idx].get(name).cloned().expect("présence vérifiée"))
    }

    fn write_var(&mut self, name: &str, v: Value) -> Result<(), RunError> {
        let idx = self
            .lookup(name)
            .ok_or_else(|| RunError::Msg(format!("variable indéfinie « {name} »")))?;
        self.env[idx].insert(name.to_string(), v);
        Ok(())
    }

    // ---------------------------------------------------------------- statements

    fn exec_block(&mut self, b: &Block) -> Result<Flow, RunError> {
        self.env.push(HashMap::new());
        let mut out = Flow::Normal;
        for s in &b.stmts {
            match self.exec_stmt(s)? {
                Flow::Normal => {}
                Flow::Return(v) => {
                    out = Flow::Return(v);
                    break;
                }
            }
        }
        self.env.pop();
        Ok(out)
    }

    fn exec_stmt(&mut self, s: &Stmt) -> Result<Flow, RunError> {
        match s {
            Stmt::Loot { name, value, .. } | Stmt::Ban { name, value, .. } => {
                let v = self.eval(value)?;
                if self.in_tour {
                    self.rt
                        .spend_pa(self.rt.assign_pa())
                        .map_err(RunError::Fault)?;
                }
                self.env
                    .last_mut()
                    .expect("au moins un scope")
                    .insert(name.clone(), v);
                Ok(Flow::Normal)
            }
            Stmt::Assign {
                name, op, value, ..
            } => {
                let rhs = self.eval(value)?;
                let newv = match op {
                    AssignOp::Set => rhs,
                    AssignOp::Add => Value::Int(as_int(&self.read_var(name)?)? + as_int(&rhs)?),
                    AssignOp::Sub => Value::Int(as_int(&self.read_var(name)?)? - as_int(&rhs)?),
                };
                if self.in_tour {
                    self.rt
                        .spend_pa(self.rt.assign_pa())
                        .map_err(RunError::Fault)?;
                }
                self.write_var(name, newv)?;
                Ok(Flow::Normal)
            }
            Stmt::Tour(block) => {
                let (saved_in, saved_base) = (self.in_tour, self.tour_base);
                self.rt.start_turn(); // régénère le budget et vide le cache PM du tour
                self.in_tour = true;
                self.tour_base = self.env.len();
                let flow = self.exec_block(block);
                self.in_tour = saved_in;
                self.tour_base = saved_base;
                flow
            }
            Stmt::Passer => {
                self.rt.end_turn();
                Ok(Flow::Normal)
            }
            Stmt::Farm { cond, body } => {
                while as_bool(&self.eval_cond(cond)?)? {
                    if let Flow::Return(v) = self.exec_block(body)? {
                        return Ok(Flow::Return(v));
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Grind {
                var,
                from,
                to,
                body,
            } => {
                let lo = as_int(&self.eval(from)?)?;
                let hi = as_int(&self.eval(to)?)?;
                let mut i = lo;
                while i <= hi {
                    let mut scope = HashMap::new();
                    scope.insert(var.clone(), Value::Int(i));
                    self.env.push(scope);
                    let flow = self.exec_block(body);
                    self.env.pop();
                    if let Flow::Return(v) = flow? {
                        return Ok(Flow::Return(v));
                    }
                    i += 1;
                }
                Ok(Flow::Normal)
            }
            Stmt::Detect {
                cond,
                then_branch,
                else_branch,
            } => {
                if as_bool(&self.eval_cond(cond)?)? {
                    self.exec_block(then_branch)
                } else if let Some(e) = else_branch {
                    self.exec_block(e)
                } else {
                    Ok(Flow::Normal)
                }
            }
            Stmt::Gg(opt) => {
                let v = match opt {
                    Some(e) => self.eval(e)?,
                    None => Value::Unit,
                };
                Ok(Flow::Return(v))
            }
            Stmt::Up(e) => {
                let v = self.eval(e)?;
                if self.in_tour {
                    self.rt.spend_pa(self.rt.up_pa()).map_err(RunError::Fault)?;
                }
                let s = to_display(&v);
                self.rt.up(&s).map_err(|_| RunError::Banni)?;
                Ok(Flow::Normal)
            }
            Stmt::Afk(e) => {
                let ms = as_int(&self.eval(e)?)?;
                self.rt.afk(ms).map_err(|_| RunError::Banni)?; // 0 PA
                Ok(Flow::Normal)
            }
            Stmt::Expr(e) => {
                self.eval(e)?;
                Ok(Flow::Normal)
            }
        }
    }

    // ---------------------------------------------------------------- expressions

    fn eval_cond(&mut self, e: &Expr) -> Result<Value, RunError> {
        let saved = self.perception;
        self.perception = true;
        let r = self.eval(e);
        self.perception = saved;
        r
    }

    fn eval(&mut self, e: &Expr) -> Result<Value, RunError> {
        match &e.kind {
            ExprKind::Int(n) => Ok(Value::Int(*n)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Str(s) => Ok(Value::Str(s.clone())),
            ExprKind::Pa => Ok(Value::Int(self.rt.pa())),
            ExprKind::Pm => Ok(Value::Int(self.rt.pm())),
            ExprKind::Suspicion => Ok(Value::Int(self.rt.suspicion() as i64)),
            ExprKind::Var(name) => self.read_var(name),
            ExprKind::Unary(op, inner) => {
                let v = self.eval(inner)?;
                match op {
                    UnOp::Neg => Ok(Value::Int(-as_int(&v)?)),
                    UnOp::Not => Ok(Value::Bool(!as_bool(&v)?)),
                }
            }
            ExprKind::Binary(op, lhs, rhs) => self.eval_binary(*op, lhs, rhs),
            ExprKind::Call(name, args) => self.eval_call(name, args),
            ExprKind::Path(left, member) => {
                if self.lookup(left).is_some() {
                    // variable.champ → accès de champ d'une instance perso
                    match self.read_var(left)? {
                        Value::Struct(m) => m.get(member).cloned().ok_or_else(|| {
                            RunError::Msg(format!("champ inconnu « {left}.{member} »"))
                        }),
                        _ => Err(RunError::Msg(format!("« {left} » n'est pas un perso"))),
                    }
                } else if let Some(tag) = self.pano_tags.get(left).and_then(|m| m.get(member)) {
                    // Pano.Variant ou Perso.champ → tag (dérivant)
                    Ok(Value::Int(*tag))
                } else {
                    Err(RunError::Msg(format!("« {left}.{member} » inconnu")))
                }
            }
            ExprKind::Struct(_ty, fields) => {
                let mut map = HashMap::new();
                for (name, expr) in fields {
                    let v = self.eval(expr)?;
                    map.insert(name.clone(), v);
                }
                Ok(Value::Struct(map))
            }
        }
    }

    fn eval_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, RunError> {
        // court-circuit booléen
        match op {
            BinOp::And => {
                return Ok(Value::Bool(
                    as_bool(&self.eval(lhs)?)? && as_bool(&self.eval(rhs)?)?,
                ))
            }
            BinOp::Or => {
                return Ok(Value::Bool(
                    as_bool(&self.eval(lhs)?)? || as_bool(&self.eval(rhs)?)?,
                ))
            }
            _ => {}
        }
        let l = self.eval(lhs)?;
        let r = self.eval(rhs)?;
        Ok(match op {
            BinOp::Add => Value::Int(as_int(&l)? + as_int(&r)?),
            BinOp::Sub => Value::Int(as_int(&l)? - as_int(&r)?),
            BinOp::Mul => Value::Int(as_int(&l)? * as_int(&r)?),
            BinOp::Div => {
                let d = as_int(&r)?;
                if d == 0 {
                    return Err(RunError::Msg("division par zéro".to_string()));
                }
                Value::Int(as_int(&l)? / d)
            }
            BinOp::Lt => Value::Bool(as_int(&l)? < as_int(&r)?),
            BinOp::Le => Value::Bool(as_int(&l)? <= as_int(&r)?),
            BinOp::Gt => Value::Bool(as_int(&l)? > as_int(&r)?),
            BinOp::Ge => Value::Bool(as_int(&l)? >= as_int(&r)?),
            BinOp::Eq => Value::Bool(l == r),
            BinOp::Ne => Value::Bool(l != r),
            BinOp::And | BinOp::Or => unreachable!("traités plus haut"),
        })
    }

    fn eval_call(&mut self, name: &str, args: &[Expr]) -> Result<Value, RunError> {
        match name {
            "rand" | "butin" => {
                let a = as_int(&self.eval(&args[0])?)?;
                let b = as_int(&self.eval(&args[1])?)?;
                Ok(Value::Int(self.rt.rand(a, b)))
            }
            "cd_pret" => {
                // l'argument est le NOM d'un bot, pas une valeur (SPEC §3).
                let bot = match args.first().map(|e| &e.kind) {
                    Some(ExprKind::Var(b)) => b.clone(),
                    _ => return Err(RunError::Msg("cd_pret attend un nom de bot".to_string())),
                };
                Ok(Value::Bool(self.rt.cd_pret(&bot)))
            }
            _ => {
                // appel de bot : coûte son `coute N pa` (au tour appelant), corps « instantané ».
                let bot = self
                    .bots
                    .get(name)
                    .cloned()
                    .ok_or_else(|| RunError::Msg(format!("bot inconnu « {name} »")))?;
                // cooldown : appeler un bot indisponible est fatal (SPEC §1.3, §9.4).
                if !self.rt.cd_pret(name) {
                    return Err(RunError::Cooldown);
                }
                let mut argvals = Vec::with_capacity(args.len());
                for a in args {
                    argvals.push(self.eval(a)?);
                }
                if self.in_tour {
                    let cost = self.bot_costs.get(name).copied().unwrap_or(0);
                    self.rt.spend_pa(cost).map_err(RunError::Fault)?;
                }
                // action observable → suspicion (peut bannir), puis mise en cooldown.
                self.rt.act_bot(name).map_err(|_| RunError::Banni)?;
                self.rt.set_cooldown(name, bot.cd.unwrap_or(0));
                self.call_bot(&bot, argvals)
            }
        }
    }

    fn call_bot(&mut self, bot: &Bot, argvals: Vec<Value>) -> Result<Value, RunError> {
        // le corps s'exécute « instantanément », hors budget (SPEC §1.1).
        let saved_env = std::mem::take(&mut self.env);
        let (saved_in, saved_base) = (self.in_tour, self.tour_base);
        self.in_tour = false;

        let mut scope = HashMap::new();
        for (p, v) in bot.params.iter().zip(argvals) {
            scope.insert(p.name.clone(), v);
        }
        self.env = vec![scope];
        let flow = self.exec_block(&bot.body);

        self.env = saved_env;
        self.in_tour = saved_in;
        self.tour_base = saved_base;

        Ok(match flow? {
            Flow::Return(v) => v,
            Flow::Normal => Value::Unit,
        })
    }
}

// ---------------------------------------------------------------- helpers de valeurs

fn as_int(v: &Value) -> Result<i64, RunError> {
    match v {
        Value::Int(n) => Ok(*n),
        _ => Err(RunError::Msg(format!("entier attendu, eu {v:?}"))),
    }
}

fn as_bool(v: &Value) -> Result<bool, RunError> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Int(n) => Ok(*n != 0),
        _ => Err(RunError::Msg(format!("booléen attendu, eu {v:?}"))),
    }
}

fn to_display(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Bool(true) => "legit".to_string(),
        Value::Bool(false) => "cheat".to_string(),
        Value::Struct(_) => "<perso>".to_string(),
        Value::Unit => "afk_total".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_src(src: &str) -> Result<(), RunError> {
        // seed fixe pour le déterminisme
        std::env::set_var("CDC_SEED", "7");
        let prog = cdc_parser::parse(src).expect("parse");
        run(&prog)
    }

    #[test]
    fn somme_calcul_pur_ok() {
        // exemple §1.5 : aucune action observable, doit terminer (PM model B : 2 PM/tour).
        let src = include_str!("../../../examples/somme.cdl");
        assert!(run_src(src).is_ok());
    }

    #[test]
    fn golden_aboutit() {
        // §9.1 (partiel Phase 2 : sans suspicion/cd) : le farm aboutit sans faute.
        let src = include_str!("../../../examples/dopeuls.cdl");
        assert!(run_src(src).is_ok());
    }

    #[test]
    fn pa_insuffisant_dans_tour() {
        // 3 affectations + 1 bot coûteux > 6 PA dans un tour à coût dynamique → faute runtime.
        let src = "// gg wp
connexion {
    loot x = 0
    tour {
        x += 1
        x += 1
        x += 1
        x += 1
        x += 1
        x += 1
        x += 1
    }
    passer
}";
        let e = run_src(src).unwrap_err();
        assert!(matches!(e, RunError::Fault(Fault::PaInsuffisant { .. })));
    }

    #[test]
    fn variante_afk_fixe_bannit() {
        // §9.2 : afk 3000 fixe → suspicion grimpe → BAN avant d'atteindre l'objectif.
        let src = include_str!("../../../examples/dopeuls_ban.cdl");
        assert_eq!(run_src(src), Err(RunError::Banni));
    }

    #[test]
    fn appel_bot_en_cooldown() {
        // §9.4 : deux appels du même bot (cd 2) dans le même tour → « sort en cooldown ».
        let src = "// gg wp
bot f() : kamas, coute 1 pa, cd 2 { gg 1 }
connexion {
    tour {
        loot a = f()
        loot b = f()
    }
    passer
}";
        assert_eq!(run_src(src), Err(RunError::Cooldown));
    }

    #[test]
    fn calcul_hors_tour_non_budgete() {
        // 100 affectations hors tour : aucun budget, aucune faute.
        let src = "// gg wp
connexion {
    loot x = 0
    grind i de 1 a 100 { x += 1 }
}";
        assert!(run_src(src).is_ok());
    }
}
