//! Backend natif de cadernislang : **émission de LLVM IR textuel** (`.ll`) compilé par le `clang`
//! du système, lié à la staticlib `cdc-runtime` (C ABI).
//!
//! Choix d'implémentation (voir ADR-002) : on n'utilise PAS `inkwell`/`llvm-sys` — le paquet
//! Arch `llvm18` ne fournit pas les libs statiques que `llvm-sys` exige. Émettre de l'IR textuel et
//! le passer à `clang` contourne ce problème, reste vérifiable sur cette machine, et garde le
//! workspace buildable sans dépendance LLVM au build-time. Toute la jouabilité reste dans
//! `cdc-runtime` (invariant §9.7) : le code généré ne fait qu'orchestrer le flux et appeler les
//! fonctions `cdc_rt_*`.
//!
//! Toutes les valeurs sont des `i64` (kamas ; `flag` = 0/1). Les chaînes (`up`) sont des globales.

use cdc_ast::*;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Déclarations des fonctions du runtime (C ABI) consommées par le code généré.
const DECLARES: &str = r#"
declare ptr @cdc_rt_new(i64, i64)
declare void @cdc_rt_free(ptr)
declare void @cdc_rt_banner()
declare void @cdc_rt_start_turn(ptr)
declare void @cdc_rt_end_turn(ptr)
declare i64 @cdc_rt_pa(ptr)
declare i64 @cdc_rt_pm(ptr)
declare i64 @cdc_rt_suspicion(ptr)
declare void @cdc_rt_spend_pa(ptr, i64)
declare void @cdc_rt_spend_assign(ptr)
declare void @cdc_rt_spend_up(ptr)
declare void @cdc_rt_pm_touch(ptr, i64)
declare void @cdc_rt_afk(ptr, i64)
declare void @cdc_rt_up(ptr, ptr, i64)
declare void @cdc_rt_act_bot(ptr, ptr, i64)
declare i64 @cdc_rt_cd_pret(ptr, ptr, i64)
declare void @cdc_rt_guard_cd(ptr, ptr, i64)
declare void @cdc_rt_set_cooldown(ptr, ptr, i64, i64)
declare i64 @cdc_rt_rand(ptr, i64, i64)
declare i64 @cdc_rt_butin(ptr, i64, i64)
"#;

/// Compile `program` en binaire natif `out`, en liant la staticlib `cdc-runtime` (`runtime_lib`).
///
/// # Erreurs
/// Erreur de génération d'IR, écriture du `.ll`, ou échec de `clang`.
pub fn build(program: &Program, out: &Path, runtime_lib: &Path) -> Result<(), String> {
    let mut ir = Cg::new().emit(program)?;
    // Cible explicite (silence le warning clang « overriding the module target triple »).
    if let Some(triple) = host_triple() {
        ir = format!("target triple = \"{triple}\"\n{ir}");
    }
    let ll = out.with_extension("ll");
    std::fs::write(&ll, ir).map_err(|e| format!("écriture du .ll : {e}"))?;
    let status = Command::new("clang")
        .arg(&ll)
        .arg(runtime_lib)
        .arg("-o")
        .arg(out)
        .args(["-lpthread", "-ldl", "-lm"])
        .status()
        .map_err(|e| format!("échec du lancement de clang : {e}"))?;
    let _ = std::fs::remove_file(&ll);
    if status.success() {
        Ok(())
    } else {
        Err(format!("clang a échoué (code {:?})", status.code()))
    }
}

/// Triplet cible par défaut de `clang` (`clang -dumpmachine`), pour annoter l'IR.
fn host_triple() -> Option<String> {
    let out = Command::new("clang").arg("-dumpmachine").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let t = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

struct Cg {
    functions: String,
    globals: String,
    str_cache: HashMap<String, (String, usize)>,
    bot_meta: HashMap<String, (i64, i64)>,
    // état par fonction
    body: String,
    reg: usize,
    lbl: usize,
    scopes: Vec<HashMap<String, String>>,
    in_tour: bool,
    tour_base: usize,
    perception: bool,
    in_main: bool,
    pm_ids: HashMap<String, u64>,
}

impl Cg {
    fn new() -> Self {
        Cg {
            functions: String::new(),
            globals: String::new(),
            str_cache: HashMap::new(),
            bot_meta: HashMap::new(),
            body: String::new(),
            reg: 0,
            lbl: 0,
            scopes: Vec::new(),
            in_tour: false,
            tour_base: 0,
            perception: false,
            in_main: false,
            pm_ids: HashMap::new(),
        }
    }

    fn emit(mut self, program: &Program) -> Result<String, String> {
        for item in &program.items {
            if let Item::Bot(b) = item {
                self.bot_meta
                    .insert(b.name.clone(), (b.cost_pa.unwrap_or(0), b.cd.unwrap_or(0)));
            }
        }
        for item in &program.items {
            if let Item::Bot(b) = item {
                self.gen_bot(b)?;
            }
        }
        self.gen_main(program)?;

        let mut s = String::from("; cadernislang — LLVM IR généré\n");
        s.push_str(DECLARES);
        s.push('\n');
        s.push_str(&self.globals);
        s.push('\n');
        s.push_str(&self.functions);
        Ok(s)
    }

    // ------------------------------------------------------------- fonctions

    fn reset_fn(&mut self) {
        self.body.clear();
        self.reg = 0;
        self.lbl = 0;
        self.scopes = vec![HashMap::new()];
    }

    fn gen_bot(&mut self, b: &Bot) -> Result<(), String> {
        self.reset_fn();
        self.in_main = false;
        self.in_tour = false;
        self.perception = false;
        let mut sig = String::from("ptr %rt");
        for (i, _) in b.params.iter().enumerate() {
            sig.push_str(&format!(", i64 %arg{i}"));
        }
        for (i, p) in b.params.iter().enumerate() {
            let slot = self.alloca(&p.name);
            self.emit_line(&format!("store i64 %arg{i}, ptr {slot}"));
        }
        let term = self.gen_block(&b.body)?;
        if !term {
            self.emit_line("ret i64 0");
        }
        let body = std::mem::take(&mut self.body);
        self.functions.push_str(&format!(
            "define i64 @cdl_bot_{}({sig}) {{\n{body}}}\n\n",
            b.name
        ));
        Ok(())
    }

    fn gen_main(&mut self, program: &Program) -> Result<(), String> {
        self.reset_fn();
        self.in_main = true;
        self.in_tour = false;
        self.perception = false;
        self.emit_line("call void @cdc_rt_banner()");
        self.emit_line("%rt = call ptr @cdc_rt_new(i64 0, i64 0)");
        let connexion = program
            .items
            .iter()
            .find_map(|it| match it {
                Item::Connexion(b) => Some(b),
                _ => None,
            })
            .ok_or("aucune connexion (point d'entrée)")?;
        let term = self.gen_block(connexion)?;
        if !term {
            self.emit_line("br label %exit");
        }
        self.emit_line("exit:");
        self.emit_line("call void @cdc_rt_free(ptr %rt)");
        self.emit_line("ret i32 0");
        let body = std::mem::take(&mut self.body);
        self.functions
            .push_str(&format!("define i32 @main() {{\n{body}}}\n"));
        Ok(())
    }

    // ------------------------------------------------------------- helpers d'émission

    fn emit_line(&mut self, l: &str) {
        if l.ends_with(':') {
            self.body.push_str(l); // label : pas d'indentation
        } else {
            self.body.push_str("  ");
            self.body.push_str(l);
        }
        self.body.push('\n');
    }

    fn fresh(&mut self) -> String {
        let r = format!("%r{}", self.reg);
        self.reg += 1;
        r
    }

    fn fresh_label(&mut self) -> String {
        let l = format!("L{}", self.lbl);
        self.lbl += 1;
        l
    }

    fn rt(&self) -> &'static str {
        "%rt"
    }

    // ------------------------------------------------------------- scopes & vars

    fn alloca(&mut self, name: &str) -> String {
        let slot = self.fresh();
        self.emit_line(&format!("{slot} = alloca i64"));
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), slot.clone());
        slot
    }

    fn lookup(&self, name: &str) -> Option<(String, usize)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, s)| s.get(name).map(|p| (p.clone(), i)))
    }

    fn pm_id(&mut self, name: &str) -> u64 {
        if let Some(id) = self.pm_ids.get(name) {
            return *id;
        }
        let id = self.pm_ids.len() as u64;
        self.pm_ids.insert(name.to_string(), id);
        id
    }

    fn global_str(&mut self, content: &str) -> (String, usize) {
        if let Some(v) = self.str_cache.get(content) {
            return v.clone();
        }
        let name = format!("@.str{}", self.str_cache.len());
        let bytes = content.as_bytes();
        let mut lit = String::new();
        for &byte in bytes {
            if byte == b'"' || byte == b'\\' || !(0x20..=0x7e).contains(&byte) {
                lit.push_str(&format!("\\{byte:02X}"));
            } else {
                lit.push(byte as char);
            }
        }
        lit.push_str("\\00");
        let arr_len = bytes.len() + 1;
        self.globals.push_str(&format!(
            "{name} = private unnamed_addr constant [{arr_len} x i8] c\"{lit}\"\n"
        ));
        let v = (name, bytes.len());
        self.str_cache.insert(content.to_string(), v.clone());
        v
    }

    // ------------------------------------------------------------- statements

    fn gen_block(&mut self, b: &Block) -> Result<bool, String> {
        self.scopes.push(HashMap::new());
        let mut terminated = false;
        for s in &b.stmts {
            if self.gen_stmt(s)? {
                terminated = true;
                break;
            }
        }
        self.scopes.pop();
        Ok(terminated)
    }

    fn gen_stmt(&mut self, s: &Stmt) -> Result<bool, String> {
        let rt = self.rt();
        match s {
            Stmt::Loot { name, value, .. } | Stmt::Ban { name, value, .. } => {
                let v = self.gen_expr(value)?;
                if self.in_tour {
                    self.emit_line(&format!("call void @cdc_rt_spend_assign(ptr {rt})"));
                }
                let slot = self.alloca(name);
                self.emit_line(&format!("store i64 {v}, ptr {slot}"));
                Ok(false)
            }
            Stmt::Assign {
                name, op, value, ..
            } => {
                let rhs = self.gen_expr(value)?;
                let newv = match op {
                    AssignOp::Set => rhs,
                    AssignOp::Add | AssignOp::Sub => {
                        let cur = self.read_var(name)?;
                        let r = self.fresh();
                        let kind = if matches!(op, AssignOp::Add) {
                            "add"
                        } else {
                            "sub"
                        };
                        self.emit_line(&format!("{r} = {kind} i64 {cur}, {rhs}"));
                        r
                    }
                };
                if self.in_tour {
                    self.emit_line(&format!("call void @cdc_rt_spend_assign(ptr {rt})"));
                }
                let (slot, _) = self
                    .lookup(name)
                    .ok_or_else(|| format!("variable indéfinie « {name} »"))?;
                self.emit_line(&format!("store i64 {newv}, ptr {slot}"));
                Ok(false)
            }
            Stmt::Tour(block) => {
                self.emit_line(&format!("call void @cdc_rt_start_turn(ptr {rt})"));
                let (si, sb) = (self.in_tour, self.tour_base);
                self.in_tour = true;
                self.tour_base = self.scopes.len();
                let term = self.gen_block(block)?;
                self.in_tour = si;
                self.tour_base = sb;
                Ok(term)
            }
            Stmt::Passer => {
                self.emit_line(&format!("call void @cdc_rt_end_turn(ptr {rt})"));
                Ok(false)
            }
            Stmt::Farm { cond, body } => {
                let (c, b, e) = (self.fresh_label(), self.fresh_label(), self.fresh_label());
                self.emit_line(&format!("br label %{c}"));
                self.emit_line(&format!("{c}:"));
                let cnd = self.gen_cond(cond)?;
                self.emit_line(&format!("br i1 {cnd}, label %{b}, label %{e}"));
                self.emit_line(&format!("{b}:"));
                let term = self.gen_block(body)?;
                if !term {
                    self.emit_line(&format!("br label %{c}"));
                }
                self.emit_line(&format!("{e}:"));
                Ok(false)
            }
            Stmt::Grind {
                var,
                from,
                to,
                body,
            } => {
                let from_v = self.gen_expr(from)?;
                let to_v = self.gen_expr(to)?;
                self.scopes.push(HashMap::new());
                let slot = self.alloca(var);
                self.emit_line(&format!("store i64 {from_v}, ptr {slot}"));
                let (c, b, e) = (self.fresh_label(), self.fresh_label(), self.fresh_label());
                self.emit_line(&format!("br label %{c}"));
                self.emit_line(&format!("{c}:"));
                let cur = self.fresh();
                self.emit_line(&format!("{cur} = load i64, ptr {slot}"));
                let cmp = self.fresh();
                self.emit_line(&format!("{cmp} = icmp sle i64 {cur}, {to_v}"));
                self.emit_line(&format!("br i1 {cmp}, label %{b}, label %{e}"));
                self.emit_line(&format!("{b}:"));
                let term = self.gen_block(body)?;
                if !term {
                    let cur2 = self.fresh();
                    self.emit_line(&format!("{cur2} = load i64, ptr {slot}"));
                    let nx = self.fresh();
                    self.emit_line(&format!("{nx} = add i64 {cur2}, 1"));
                    self.emit_line(&format!("store i64 {nx}, ptr {slot}"));
                    self.emit_line(&format!("br label %{c}"));
                }
                self.emit_line(&format!("{e}:"));
                self.scopes.pop();
                Ok(false)
            }
            Stmt::Detect {
                cond,
                then_branch,
                else_branch,
            } => {
                let cnd = self.gen_cond(cond)?;
                let then_l = self.fresh_label();
                let merge_l = self.fresh_label();
                let else_l = if else_branch.is_some() {
                    self.fresh_label()
                } else {
                    merge_l.clone()
                };
                self.emit_line(&format!("br i1 {cnd}, label %{then_l}, label %{else_l}"));
                self.emit_line(&format!("{then_l}:"));
                let t_then = self.gen_block(then_branch)?;
                if !t_then {
                    self.emit_line(&format!("br label %{merge_l}"));
                }
                let both_term = if let Some(eb) = else_branch {
                    self.emit_line(&format!("{else_l}:"));
                    let t_else = self.gen_block(eb)?;
                    if !t_else {
                        self.emit_line(&format!("br label %{merge_l}"));
                    }
                    t_then && t_else
                } else {
                    false
                };
                if both_term {
                    Ok(true)
                } else {
                    self.emit_line(&format!("{merge_l}:"));
                    Ok(false)
                }
            }
            Stmt::Gg(opt) => {
                if self.in_main {
                    self.emit_line("br label %exit");
                } else {
                    let v = match opt {
                        Some(e) => self.gen_expr(e)?,
                        None => "0".to_string(),
                    };
                    self.emit_line(&format!("ret i64 {v}"));
                }
                Ok(true)
            }
            Stmt::Up(e) => {
                let (g, len) = self.gen_str(e)?;
                if self.in_tour {
                    self.emit_line(&format!("call void @cdc_rt_spend_up(ptr {rt})"));
                }
                self.emit_line(&format!(
                    "call void @cdc_rt_up(ptr {rt}, ptr {g}, i64 {len})"
                ));
                Ok(false)
            }
            Stmt::Afk(e) => {
                let ms = self.gen_expr(e)?;
                self.emit_line(&format!("call void @cdc_rt_afk(ptr {rt}, i64 {ms})"));
                Ok(false)
            }
            Stmt::Expr(e) => {
                self.gen_expr(e)?;
                Ok(false)
            }
        }
    }

    // ------------------------------------------------------------- expressions

    /// Évalue une condition (perception : lectures gratuites) → opérande i1.
    fn gen_cond(&mut self, e: &Expr) -> Result<String, String> {
        let saved = self.perception;
        self.perception = true;
        let v = self.gen_expr(e);
        self.perception = saved;
        let v = v?;
        let c = self.fresh();
        self.emit_line(&format!("{c} = icmp ne i64 {v}, 0"));
        Ok(c)
    }

    fn read_var(&mut self, name: &str) -> Result<String, String> {
        let (slot, idx) = self
            .lookup(name)
            .ok_or_else(|| format!("variable indéfinie « {name} »"))?;
        if self.in_tour && !self.perception && idx < self.tour_base {
            let id = self.pm_id(name);
            let rt = self.rt();
            self.emit_line(&format!("call void @cdc_rt_pm_touch(ptr {rt}, i64 {id})"));
        }
        let r = self.fresh();
        self.emit_line(&format!("{r} = load i64, ptr {slot}"));
        Ok(r)
    }

    /// Retourne un opérande i64 (registre `%rN` ou littéral).
    fn gen_expr(&mut self, e: &Expr) -> Result<String, String> {
        let rt = self.rt();
        match &e.kind {
            ExprKind::Int(n) => Ok(n.to_string()),
            ExprKind::Bool(b) => Ok(if *b { "1" } else { "0" }.to_string()),
            ExprKind::Str(_) => Err("chaîne hors d'un « up » non supportée par le backend".into()),
            ExprKind::Pa => Ok(self.call_i64("cdc_rt_pa", &format!("ptr {rt}"))),
            ExprKind::Pm => Ok(self.call_i64("cdc_rt_pm", &format!("ptr {rt}"))),
            ExprKind::Suspicion => Ok(self.call_i64("cdc_rt_suspicion", &format!("ptr {rt}"))),
            ExprKind::Var(name) => self.read_var(name),
            ExprKind::Unary(op, x) => {
                let v = self.gen_expr(x)?;
                let r = self.fresh();
                match op {
                    UnOp::Neg => self.emit_line(&format!("{r} = sub i64 0, {v}")),
                    UnOp::Not => {
                        let c = self.fresh();
                        self.emit_line(&format!("{c} = icmp eq i64 {v}, 0"));
                        self.emit_line(&format!("{r} = zext i1 {c} to i64"));
                    }
                }
                Ok(r)
            }
            ExprKind::Binary(op, l, rr) => self.gen_binary(*op, l, rr),
            ExprKind::Call(name, args) => self.gen_call(name, args),
        }
    }

    fn gen_binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Result<String, String> {
        let lv = self.gen_expr(l)?;
        let rv = self.gen_expr(r)?;
        let arith = |kind: &str| format!("{kind} i64 {lv}, {rv}");
        let (pred, rhs) = match op {
            BinOp::Add => (String::new(), arith("add")),
            BinOp::Sub => (String::new(), arith("sub")),
            BinOp::Mul => (String::new(), arith("mul")),
            BinOp::Div => (String::new(), arith("sdiv")),
            BinOp::Lt => ("slt".into(), String::new()),
            BinOp::Le => ("sle".into(), String::new()),
            BinOp::Gt => ("sgt".into(), String::new()),
            BinOp::Ge => ("sge".into(), String::new()),
            BinOp::Eq => ("eq".into(), String::new()),
            BinOp::Ne => ("ne".into(), String::new()),
            BinOp::And => return self.gen_bool(&lv, &rv, "and"),
            BinOp::Or => return self.gen_bool(&lv, &rv, "or"),
        };
        if rhs.is_empty() {
            let c = self.fresh();
            self.emit_line(&format!("{c} = icmp {pred} i64 {lv}, {rv}"));
            let r = self.fresh();
            self.emit_line(&format!("{r} = zext i1 {c} to i64"));
            Ok(r)
        } else {
            let r = self.fresh();
            self.emit_line(&format!("{r} = {rhs}"));
            Ok(r)
        }
    }

    /// `et`/`ou` (non court-circuit : opérandes de condition sans effet de bord).
    fn gen_bool(&mut self, lv: &str, rv: &str, kind: &str) -> Result<String, String> {
        let la = self.fresh();
        self.emit_line(&format!("{la} = icmp ne i64 {lv}, 0"));
        let ra = self.fresh();
        self.emit_line(&format!("{ra} = icmp ne i64 {rv}, 0"));
        let c = self.fresh();
        self.emit_line(&format!("{c} = {kind} i1 {la}, {ra}"));
        let r = self.fresh();
        self.emit_line(&format!("{r} = zext i1 {c} to i64"));
        Ok(r)
    }

    fn gen_call(&mut self, name: &str, args: &[Expr]) -> Result<String, String> {
        let rt = self.rt();
        match name {
            "rand" | "butin" => {
                let a = self.gen_expr(&args[0])?;
                let b = self.gen_expr(&args[1])?;
                let f = if name == "rand" {
                    "cdc_rt_rand"
                } else {
                    "cdc_rt_butin"
                };
                Ok(self.call_i64(f, &format!("ptr {rt}, i64 {a}, i64 {b}")))
            }
            "cd_pret" => {
                let bot = match args.first().map(|a| &a.kind) {
                    Some(ExprKind::Var(b)) => b.clone(),
                    _ => return Err("cd_pret attend un nom de bot".into()),
                };
                let (g, len) = self.global_str(&bot);
                Ok(self.call_i64("cdc_rt_cd_pret", &format!("ptr {rt}, ptr {g}, i64 {len}")))
            }
            _ => {
                let (cost, cd) = *self
                    .bot_meta
                    .get(name)
                    .ok_or_else(|| format!("bot inconnu « {name} »"))?;
                let (g, len) = self.global_str(name);
                self.emit_line(&format!(
                    "call void @cdc_rt_guard_cd(ptr {rt}, ptr {g}, i64 {len})"
                ));
                let mut argstr = format!("ptr {rt}");
                for a in args {
                    let v = self.gen_expr(a)?;
                    argstr.push_str(&format!(", i64 {v}"));
                }
                if self.in_tour {
                    self.emit_line(&format!("call void @cdc_rt_spend_pa(ptr {rt}, i64 {cost})"));
                }
                self.emit_line(&format!(
                    "call void @cdc_rt_act_bot(ptr {rt}, ptr {g}, i64 {len})"
                ));
                self.emit_line(&format!(
                    "call void @cdc_rt_set_cooldown(ptr {rt}, ptr {g}, i64 {len}, i64 {cd})"
                ));
                let r = self.fresh();
                self.emit_line(&format!("{r} = call i64 @cdl_bot_{name}({argstr})"));
                Ok(r)
            }
        }
    }

    fn call_i64(&mut self, f: &str, args: &str) -> String {
        let r = self.fresh();
        self.emit_line(&format!("{r} = call i64 @{f}({args})"));
        r
    }

    fn gen_str(&mut self, e: &Expr) -> Result<(String, usize), String> {
        match &e.kind {
            ExprKind::Str(s) => Ok(self.global_str(s)),
            _ => Err("« up » n'accepte qu'un littéral chaîne dans le backend natif".into()),
        }
    }
}
