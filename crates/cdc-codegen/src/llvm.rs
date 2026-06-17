//! Backend LLVM (via `inkwell`, LLVM 18 — ADR-002) : AST → LLVM IR → objet natif, lié à
//! `cdc-runtime` (C ABI). Le code généré reproduit la sémantique de l'interpréteur en déléguant
//! TOUTES les mécaniques au runtime (invariant §9.7) : il ne fait qu'orchestrer le flux de
//! contrôle et appeler les fonctions `cdc_rt_*`.
//!
//! Valeurs : tout est `i64` (kamas ; `flag` = 0/1). Les chaînes (`up`) sont des globales i8*.

use cdc_ast::*;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{CodeModel, FileType, RelocMode, Target, TargetMachine};
use inkwell::values::{FunctionValue, IntValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Compile `program` en binaire natif `out`, en liant la staticlib `runtime_lib`
/// (`libcdc_runtime.a`). Le front-end (parse + sema) doit déjà avoir validé le programme.
///
/// # Erreurs
/// Toute erreur de génération, d'écriture de l'objet ou de link (message lisible).
pub fn build(program: &Program, out: &Path, runtime_lib: &Path) -> Result<(), String> {
    let ctx = Context::create();
    let mut cg = Cg::new(&ctx);
    cg.declare_runtime();
    cg.declare_bots(program);
    cg.gen_bots(program)?;
    cg.gen_main(program)?;
    cg.module
        .verify()
        .map_err(|e| format!("module LLVM invalide : {e}"))?;

    let obj = out.with_extension("o");
    cg.emit_object(&obj)?;
    link(&obj, runtime_lib, out)?;
    let _ = std::fs::remove_file(&obj);
    Ok(())
}

struct Cg<'ctx> {
    ctx: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    rt_fns: HashMap<&'static str, FunctionValue<'ctx>>,
    bot_fns: HashMap<String, FunctionValue<'ctx>>,
    bot_meta: HashMap<String, (i64, i64)>, // nom -> (cost_pa, cd)
    // état courant
    cur_rt: Option<PointerValue<'ctx>>,
    cur_fn: Option<FunctionValue<'ctx>>,
    scopes: Vec<HashMap<String, PointerValue<'ctx>>>,
    in_tour: bool,
    tour_base: usize,
    perception: bool,
    pm_ids: HashMap<String, u64>,
    in_main: bool,
}

impl<'ctx> Cg<'ctx> {
    fn new(ctx: &'ctx Context) -> Self {
        Cg {
            ctx,
            module: ctx.create_module("cadernis"),
            builder: ctx.create_builder(),
            rt_fns: HashMap::new(),
            bot_fns: HashMap::new(),
            bot_meta: HashMap::new(),
            cur_rt: None,
            cur_fn: None,
            scopes: Vec::new(),
            in_tour: false,
            tour_base: 0,
            perception: false,
            pm_ids: HashMap::new(),
            in_main: false,
        }
    }

    // ------------------------------------------------------------- déclarations

    fn declare_runtime(&mut self) {
        let i64t = self.ctx.i64_type();
        let ptr = self.ctx.ptr_type(AddressSpace::default());
        let void = self.ctx.void_type();

        // (nom, type)
        let new_t = ptr.fn_type(&[i64t.into(), i64t.into()], false);
        self.add_rt("cdc_rt_new", new_t);
        self.add_rt("cdc_rt_free", void.fn_type(&[ptr.into()], false));
        self.add_rt("cdc_rt_banner", void.fn_type(&[], false));
        self.add_rt("cdc_rt_start_turn", void.fn_type(&[ptr.into()], false));
        self.add_rt("cdc_rt_end_turn", void.fn_type(&[ptr.into()], false));
        self.add_rt("cdc_rt_pa", i64t.fn_type(&[ptr.into()], false));
        self.add_rt("cdc_rt_pm", i64t.fn_type(&[ptr.into()], false));
        self.add_rt("cdc_rt_suspicion", i64t.fn_type(&[ptr.into()], false));
        self.add_rt("cdc_rt_spend_pa", void.fn_type(&[ptr.into(), i64t.into()], false));
        self.add_rt("cdc_rt_spend_assign", void.fn_type(&[ptr.into()], false));
        self.add_rt("cdc_rt_spend_up", void.fn_type(&[ptr.into()], false));
        self.add_rt("cdc_rt_pm_touch", void.fn_type(&[ptr.into(), i64t.into()], false));
        self.add_rt("cdc_rt_afk", void.fn_type(&[ptr.into(), i64t.into()], false));
        let str_void = void.fn_type(&[ptr.into(), ptr.into(), i64t.into()], false);
        self.add_rt("cdc_rt_up", str_void);
        self.add_rt("cdc_rt_act_bot", str_void);
        self.add_rt("cdc_rt_cd_pret", i64t.fn_type(&[ptr.into(), ptr.into(), i64t.into()], false));
        self.add_rt("cdc_rt_guard_cd", str_void);
        self.add_rt(
            "cdc_rt_set_cooldown",
            void.fn_type(&[ptr.into(), ptr.into(), i64t.into(), i64t.into()], false),
        );
        let rng_t = i64t.fn_type(&[ptr.into(), i64t.into(), i64t.into()], false);
        self.add_rt("cdc_rt_rand", rng_t);
        self.add_rt("cdc_rt_butin", rng_t);
    }

    fn add_rt(&mut self, name: &'static str, ty: inkwell::types::FunctionType<'ctx>) {
        let f = self.module.add_function(name, ty, None);
        self.rt_fns.insert(name, f);
    }

    fn declare_bots(&mut self, program: &Program) {
        let i64t = self.ctx.i64_type();
        let ptr = self.ctx.ptr_type(AddressSpace::default());
        for item in &program.items {
            if let Item::Bot(b) = item {
                let mut params = vec![ptr.into()];
                params.extend(b.params.iter().map(|_| i64t.into()));
                let ft = i64t.fn_type(&params, false);
                let f = self.module.add_function(&format!("cdl_bot_{}", b.name), ft, None);
                self.bot_fns.insert(b.name.clone(), f);
                self.bot_meta
                    .insert(b.name.clone(), (b.cost_pa.unwrap_or(0), b.cd.unwrap_or(0)));
            }
        }
    }

    // ------------------------------------------------------------- fonctions

    fn gen_bots(&mut self, program: &Program) -> Result<(), String> {
        for item in &program.items {
            if let Item::Bot(b) = item {
                let f = self.bot_fns[&b.name];
                let entry = self.ctx.append_basic_block(f, "entry");
                self.builder.position_at_end(entry);
                self.cur_fn = Some(f);
                self.cur_rt = Some(f.get_nth_param(0).unwrap().into_pointer_value());
                self.in_main = false;
                self.in_tour = false; // corps de bot : hors budget (SPEC §1.1)
                self.perception = false;
                self.scopes = vec![HashMap::new()];

                // lier les paramètres à des allocas
                for (i, p) in b.params.iter().enumerate() {
                    let v = f.get_nth_param((i + 1) as u32).unwrap().into_int_value();
                    let slot = self.alloca(&p.name);
                    self.builder.build_store(slot, v).unwrap();
                }

                let terminated = self.gen_block(&b.body)?;
                if !terminated {
                    // bot sans `gg` explicite → renvoie 0 (afk_total)
                    self.builder
                        .build_return(Some(&self.ctx.i64_type().const_zero()))
                        .unwrap();
                }
            }
        }
        Ok(())
    }

    fn gen_main(&mut self, program: &Program) -> Result<(), String> {
        let i32t = self.ctx.i32_type();
        let i64t = self.ctx.i64_type();
        let main = self.module.add_function("main", i32t.fn_type(&[], false), None);
        let entry = self.ctx.append_basic_block(main, "entry");
        let exit = self.ctx.append_basic_block(main, "exit");
        self.builder.position_at_end(entry);

        self.cur_fn = Some(main);
        self.in_main = true;
        self.in_tour = false;
        self.perception = false;
        self.scopes = vec![HashMap::new()];

        // bannière + création du runtime (seed via CDC_SEED → has_seed=0)
        self.call_void("cdc_rt_banner", &[]);
        let rt = self
            .builder
            .build_call(
                self.rt_fns["cdc_rt_new"],
                &[i64t.const_zero().into(), i64t.const_zero().into()],
                "rt",
            )
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();
        self.cur_rt = Some(rt);

        let connexion = program.items.iter().find_map(|it| match it {
            Item::Connexion(b) => Some(b),
            _ => None,
        });
        let connexion = connexion.ok_or("aucune connexion (point d'entrée)")?;

        let terminated = self.gen_block(connexion)?;
        if !terminated {
            self.builder.build_unconditional_branch(exit).unwrap();
        }

        // bloc de sortie : libère le runtime et renvoie 0.
        self.builder.position_at_end(exit);
        self.call_void("cdc_rt_free", &[rt.into()]);
        self.builder.build_return(Some(&i32t.const_zero())).unwrap();
        Ok(())
    }

    // ------------------------------------------------------------- scopes & vars

    fn alloca(&mut self, name: &str) -> PointerValue<'ctx> {
        let slot = self.builder.build_alloca(self.ctx.i64_type(), name).unwrap();
        self.scopes.last_mut().unwrap().insert(name.to_string(), slot);
        slot
    }

    /// Retourne (slot, scope_index) pour une variable.
    fn lookup(&self, name: &str) -> Option<(PointerValue<'ctx>, usize)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, s)| s.get(name).map(|p| (*p, i)))
    }

    fn pm_id(&mut self, name: &str) -> u64 {
        if let Some(id) = self.pm_ids.get(name) {
            return *id;
        }
        let id = self.pm_ids.len() as u64;
        self.pm_ids.insert(name.to_string(), id);
        id
    }

    // ------------------------------------------------------------- statements

    /// Génère un bloc. Retourne `true` si le bloc s'est terminé par un `gg` (branche/return émis).
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
        let rt = self.cur_rt.unwrap();
        match s {
            Stmt::Loot { name, value, .. } | Stmt::Ban { name, value, .. } => {
                let v = self.gen_expr(value)?;
                if self.in_tour {
                    self.call_void("cdc_rt_spend_assign", &[rt.into()]);
                }
                let slot = self.alloca(name);
                self.builder.build_store(slot, v).unwrap();
                Ok(false)
            }
            Stmt::Assign { name, op, value, .. } => {
                let rhs = self.gen_expr(value)?;
                let newv = match op {
                    AssignOp::Set => rhs,
                    AssignOp::Add | AssignOp::Sub => {
                        let cur = self.read_var(name)?;
                        if matches!(op, AssignOp::Add) {
                            self.builder.build_int_add(cur, rhs, "add").unwrap()
                        } else {
                            self.builder.build_int_sub(cur, rhs, "sub").unwrap()
                        }
                    }
                };
                if self.in_tour {
                    self.call_void("cdc_rt_spend_assign", &[rt.into()]);
                }
                let (slot, _) = self
                    .lookup(name)
                    .ok_or_else(|| format!("variable indéfinie « {name} »"))?;
                self.builder.build_store(slot, newv).unwrap();
                Ok(false)
            }
            Stmt::Tour(block) => {
                self.call_void("cdc_rt_start_turn", &[rt.into()]);
                let (si, sb) = (self.in_tour, self.tour_base);
                self.in_tour = true;
                self.tour_base = self.scopes.len();
                let term = self.gen_block(block)?;
                self.in_tour = si;
                self.tour_base = sb;
                Ok(term)
            }
            Stmt::Passer => {
                self.call_void("cdc_rt_end_turn", &[rt.into()]);
                Ok(false)
            }
            Stmt::Farm { cond, body } => {
                let f = self.cur_fn.unwrap();
                let cond_bb = self.ctx.append_basic_block(f, "farm.cond");
                let body_bb = self.ctx.append_basic_block(f, "farm.body");
                let end_bb = self.ctx.append_basic_block(f, "farm.end");
                self.builder.build_unconditional_branch(cond_bb).unwrap();
                self.builder.position_at_end(cond_bb);
                let c = self.gen_cond(cond)?;
                self.builder.build_conditional_branch(c, body_bb, end_bb).unwrap();
                self.builder.position_at_end(body_bb);
                let term = self.gen_block(body)?;
                if !term {
                    self.builder.build_unconditional_branch(cond_bb).unwrap();
                }
                self.builder.position_at_end(end_bb);
                Ok(false)
            }
            Stmt::Grind { var, from, to, body } => {
                let f = self.cur_fn.unwrap();
                let from_v = self.gen_expr(from)?;
                let to_v = self.gen_expr(to)?;
                self.scopes.push(HashMap::new());
                let slot = self.alloca(var);
                self.builder.build_store(slot, from_v).unwrap();
                let cond_bb = self.ctx.append_basic_block(f, "grind.cond");
                let body_bb = self.ctx.append_basic_block(f, "grind.body");
                let end_bb = self.ctx.append_basic_block(f, "grind.end");
                self.builder.build_unconditional_branch(cond_bb).unwrap();
                self.builder.position_at_end(cond_bb);
                let cur = self.load(slot);
                let cmp = self
                    .builder
                    .build_int_compare(IntPredicate::SLE, cur, to_v, "grind.le")
                    .unwrap();
                self.builder.build_conditional_branch(cmp, body_bb, end_bb).unwrap();
                self.builder.position_at_end(body_bb);
                let term = self.gen_block(body)?;
                if !term {
                    let cur = self.load(slot);
                    let one = self.ctx.i64_type().const_int(1, false);
                    let next = self.builder.build_int_add(cur, one, "grind.inc").unwrap();
                    self.builder.build_store(slot, next).unwrap();
                    self.builder.build_unconditional_branch(cond_bb).unwrap();
                }
                self.builder.position_at_end(end_bb);
                self.scopes.pop();
                Ok(false)
            }
            Stmt::Detect { cond, then_branch, else_branch } => {
                let f = self.cur_fn.unwrap();
                let c = self.gen_cond(cond)?;
                let then_bb = self.ctx.append_basic_block(f, "then");
                let else_bb = self.ctx.append_basic_block(f, "else");
                let merge_bb = self.ctx.append_basic_block(f, "merge");
                self.builder.build_conditional_branch(c, then_bb, else_bb).unwrap();

                self.builder.position_at_end(then_bb);
                let t1 = self.gen_block(then_branch)?;
                if !t1 {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }
                self.builder.position_at_end(else_bb);
                let t2 = if let Some(eb) = else_branch {
                    self.gen_block(eb)?
                } else {
                    false
                };
                if !t2 {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }
                self.builder.position_at_end(merge_bb);
                Ok(false)
            }
            Stmt::Gg(opt) => {
                if self.in_main {
                    // gg dans connexion → fin de programme (branche vers le bloc de sortie)
                    let exit = self.cur_fn.unwrap().get_last_basic_block().unwrap();
                    self.builder.build_unconditional_branch(exit).unwrap();
                } else {
                    let v = match opt {
                        Some(e) => self.gen_expr(e)?,
                        None => self.ctx.i64_type().const_zero(),
                    };
                    self.builder.build_return(Some(&v)).unwrap();
                }
                Ok(true)
            }
            Stmt::Up(e) => {
                let (ptr, len) = self.gen_str(e)?;
                if self.in_tour {
                    self.call_void("cdc_rt_spend_up", &[rt.into()]);
                }
                self.call_void("cdc_rt_up", &[rt.into(), ptr.into(), len.into()]);
                Ok(false)
            }
            Stmt::Afk(e) => {
                let ms = self.gen_expr(e)?;
                self.call_void("cdc_rt_afk", &[rt.into(), ms.into()]);
                Ok(false)
            }
            Stmt::Expr(e) => {
                self.gen_expr(e)?;
                Ok(false)
            }
        }
    }

    // ------------------------------------------------------------- expressions

    fn gen_cond(&mut self, e: &Expr) -> Result<IntValue<'ctx>, String> {
        let saved = self.perception;
        self.perception = true; // lectures gratuites en condition (SPEC §1.1)
        let v = self.gen_expr(e);
        self.perception = saved;
        let v = v?;
        // « truthy » : v != 0
        Ok(self
            .builder
            .build_int_compare(IntPredicate::NE, v, self.ctx.i64_type().const_zero(), "truthy")
            .unwrap())
    }

    fn read_var(&mut self, name: &str) -> Result<IntValue<'ctx>, String> {
        let (slot, idx) = self
            .lookup(name)
            .ok_or_else(|| format!("variable indéfinie « {name} »"))?;
        if self.in_tour && !self.perception && idx < self.tour_base {
            let id = self.pm_id(name);
            let rt = self.cur_rt.unwrap();
            let idv = self.ctx.i64_type().const_int(id, false);
            self.call_void("cdc_rt_pm_touch", &[rt.into(), idv.into()]);
        }
        Ok(self.load(slot))
    }

    fn load(&self, slot: PointerValue<'ctx>) -> IntValue<'ctx> {
        self.builder
            .build_load(self.ctx.i64_type(), slot, "load")
            .unwrap()
            .into_int_value()
    }

    fn gen_expr(&mut self, e: &Expr) -> Result<IntValue<'ctx>, String> {
        let i64t = self.ctx.i64_type();
        let rt = self.cur_rt.unwrap();
        match &e.kind {
            ExprKind::Int(n) => Ok(i64t.const_int(*n as u64, true)),
            ExprKind::Bool(b) => Ok(i64t.const_int(*b as u64, false)),
            ExprKind::Str(_) => Err("chaîne hors d'un « up » non supportée par le backend".into()),
            ExprKind::Pa => Ok(self.call_i64("cdc_rt_pa", &[rt.into()])),
            ExprKind::Pm => Ok(self.call_i64("cdc_rt_pm", &[rt.into()])),
            ExprKind::Suspicion => Ok(self.call_i64("cdc_rt_suspicion", &[rt.into()])),
            ExprKind::Var(name) => self.read_var(name),
            ExprKind::Unary(op, x) => {
                let v = self.gen_expr(x)?;
                match op {
                    UnOp::Neg => Ok(self.builder.build_int_neg(v, "neg").unwrap()),
                    UnOp::Not => {
                        let isz = self
                            .builder
                            .build_int_compare(IntPredicate::EQ, v, i64t.const_zero(), "notz")
                            .unwrap();
                        Ok(self.zext(isz))
                    }
                }
            }
            ExprKind::Binary(op, l, r) => self.gen_binary(*op, l, r),
            ExprKind::Call(name, args) => self.gen_call(name, args),
        }
    }

    fn gen_binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Result<IntValue<'ctx>, String> {
        let lv = self.gen_expr(l)?;
        let rv = self.gen_expr(r)?;
        let b = &self.builder;
        let zero = self.ctx.i64_type().const_zero();
        let res = match op {
            BinOp::Add => b.build_int_add(lv, rv, "add").unwrap(),
            BinOp::Sub => b.build_int_sub(lv, rv, "sub").unwrap(),
            BinOp::Mul => b.build_int_mul(lv, rv, "mul").unwrap(),
            BinOp::Div => b.build_int_signed_div(lv, rv, "div").unwrap(),
            BinOp::Lt => return Ok(self.cmp(IntPredicate::SLT, lv, rv)),
            BinOp::Le => return Ok(self.cmp(IntPredicate::SLE, lv, rv)),
            BinOp::Gt => return Ok(self.cmp(IntPredicate::SGT, lv, rv)),
            BinOp::Ge => return Ok(self.cmp(IntPredicate::SGE, lv, rv)),
            BinOp::Eq => return Ok(self.cmp(IntPredicate::EQ, lv, rv)),
            BinOp::Ne => return Ok(self.cmp(IntPredicate::NE, lv, rv)),
            BinOp::And => {
                // non court-circuit : OK car les opérandes de condition sont sans effet de bord
                let la = b.build_int_compare(IntPredicate::NE, lv, zero, "la").unwrap();
                let ra = b.build_int_compare(IntPredicate::NE, rv, zero, "ra").unwrap();
                let a = b.build_and(la, ra, "and").unwrap();
                return Ok(self.zext(a));
            }
            BinOp::Or => {
                let la = b.build_int_compare(IntPredicate::NE, lv, zero, "la").unwrap();
                let ra = b.build_int_compare(IntPredicate::NE, rv, zero, "ra").unwrap();
                let o = b.build_or(la, ra, "or").unwrap();
                return Ok(self.zext(o));
            }
        };
        Ok(res)
    }

    fn cmp(&self, pred: IntPredicate, l: IntValue<'ctx>, r: IntValue<'ctx>) -> IntValue<'ctx> {
        let c = self.builder.build_int_compare(pred, l, r, "cmp").unwrap();
        self.zext(c)
    }

    fn zext(&self, b: IntValue<'ctx>) -> IntValue<'ctx> {
        self.builder
            .build_int_z_extend(b, self.ctx.i64_type(), "zext")
            .unwrap()
    }

    fn gen_call(&mut self, name: &str, args: &[Expr]) -> Result<IntValue<'ctx>, String> {
        let rt = self.cur_rt.unwrap();
        match name {
            "rand" | "butin" => {
                let a = self.gen_expr(&args[0])?;
                let bb = self.gen_expr(&args[1])?;
                let f = if name == "rand" { "cdc_rt_rand" } else { "cdc_rt_butin" };
                Ok(self.call_i64(f, &[rt.into(), a.into(), bb.into()]))
            }
            "cd_pret" => {
                let bot = match args.first().map(|a| &a.kind) {
                    Some(ExprKind::Var(b)) => b.clone(),
                    _ => return Err("cd_pret attend un nom de bot".into()),
                };
                let (ptr, len) = self.global_str(&bot);
                Ok(self.call_i64("cdc_rt_cd_pret", &[rt.into(), ptr.into(), len.into()]))
            }
            _ => {
                let f = *self
                    .bot_fns
                    .get(name)
                    .ok_or_else(|| format!("bot inconnu « {name} »"))?;
                let (cost, cd) = self.bot_meta[name];
                let (ptr, len) = self.global_str(name);
                // cooldown : appel d'un bot indisponible est fatal (SPEC §1.3)
                self.call_void("cdc_rt_guard_cd", &[rt.into(), ptr.into(), len.into()]);
                // arguments
                let mut argv: Vec<inkwell::values::BasicMetadataValueEnum> = vec![rt.into()];
                for a in args {
                    argv.push(self.gen_expr(a)?.into());
                }
                if self.in_tour {
                    let c = self.ctx.i64_type().const_int(cost as u64, true);
                    self.call_void("cdc_rt_spend_pa", &[rt.into(), c.into()]);
                }
                // action observable (suspicion, peut bannir) puis mise en cooldown
                self.call_void("cdc_rt_act_bot", &[rt.into(), ptr.into(), len.into()]);
                let cdv = self.ctx.i64_type().const_int(cd as u64, true);
                self.call_void("cdc_rt_set_cooldown", &[rt.into(), ptr.into(), len.into(), cdv.into()]);
                // appel du corps
                Ok(self
                    .builder
                    .build_call(f, &argv, "botcall")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value())
            }
        }
    }

    // ------------------------------------------------------------- helpers

    /// Construit (ptr i8*, len i64) pour un littéral chaîne d'un `up`.
    fn gen_str(&mut self, e: &Expr) -> Result<(PointerValue<'ctx>, IntValue<'ctx>), String> {
        match &e.kind {
            ExprKind::Str(s) => Ok(self.global_str(s)),
            _ => Err("« up » n'accepte qu'un littéral chaîne dans le backend natif".into()),
        }
    }

    fn global_str(&mut self, s: &str) -> (PointerValue<'ctx>, IntValue<'ctx>) {
        let g = self.builder.build_global_string_ptr(s, "str").unwrap();
        let len = self.ctx.i64_type().const_int(s.len() as u64, false);
        (g.as_pointer_value(), len)
    }

    fn call_void(&self, name: &str, args: &[inkwell::values::BasicMetadataValueEnum<'ctx>]) {
        self.builder.build_call(self.rt_fns[name], args, "").unwrap();
    }

    fn call_i64(&self, name: &str, args: &[inkwell::values::BasicMetadataValueEnum<'ctx>]) -> IntValue<'ctx> {
        self.builder
            .build_call(self.rt_fns[name], args, "call")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_int_value()
    }

    // ------------------------------------------------------------- émission objet

    fn emit_object(&self, obj: &Path) -> Result<(), String> {
        Target::initialize_native(&Default::default()).map_err(|e| e.to_string())?;
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let tm = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("création de la TargetMachine impossible")?;
        tm.write_to_file(&self.module, FileType::Object, obj)
            .map_err(|e| e.to_string())
    }
}

/// Lie l'objet et la staticlib `cdc-runtime` en un binaire natif (via `cc`).
fn link(obj: &Path, runtime_lib: &Path, out: &Path) -> Result<(), String> {
    let status = Command::new("cc")
        .arg(obj)
        .arg(runtime_lib)
        .arg("-o")
        .arg(out)
        // dépendances système de la std Rust (staticlib)
        .args(["-lpthread", "-ldl", "-lm"])
        .status()
        .map_err(|e| format!("échec du lancement de cc : {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cc a échoué (code {:?})", status.code()))
    }
}
