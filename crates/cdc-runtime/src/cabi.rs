//! C ABI du runtime : surface `extern "C"` consommée par le code généré par LLVM (backend
//! `cdc-codegen`). Mêmes mécaniques que l'interpréteur — source unique (invariant §9.7).
//!
//! **Contrat de sûreté commun** : tout `rt` est un pointeur issu de [`cdc_rt_new`], valide et non
//! libéré ; les couples `(ptr, len)` décrivent une chaîne UTF-8 vivante. Les conditions fatales
//! (budget épuisé, ban, cooldown) impriment le message normatif (SPEC §12) et terminent le process
//! avec un code ≠ 0 — le code généré reste ainsi trivial (pas de propagation d'erreur dans l'IR).
#![allow(clippy::missing_safety_doc)]

use crate::{Config, Fault, Runtime};

/// Reconstruit un `&str` depuis un pointeur/longueur fournis par le code généré.
unsafe fn as_str<'a>(ptr: *const u8, len: i64) -> &'a str {
    let bytes = std::slice::from_raw_parts(ptr, len as usize);
    std::str::from_utf8(bytes).unwrap_or("")
}

fn fatal(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn check_fault(r: Result<(), Fault>) {
    if let Err(f) = r {
        fatal(&f.to_string());
    }
}

fn check_ban(r: Result<(), crate::Banned>) {
    if r.is_err() {
        fatal("compte banni");
    }
}

/// Crée le runtime. `has_seed != 0` ⇒ seed explicite ; sinon on lit `CDC_SEED` (comme l'interp).
#[no_mangle]
pub extern "C" fn cdc_rt_new(has_seed: i64, seed: i64) -> *mut Runtime {
    let mut cfg = Config::default();
    if has_seed != 0 {
        cfg.seed = Some(seed as u64);
    } else if let Ok(s) = std::env::var("CDC_SEED") {
        if let Ok(n) = s.parse::<u64>() {
            cfg.seed = Some(n);
        }
    }
    Box::into_raw(Box::new(Runtime::new(cfg)))
}

#[no_mangle]
pub unsafe extern "C" fn cdc_rt_free(rt: *mut Runtime) {
    if !rt.is_null() {
        drop(Box::from_raw(rt));
    }
}

/// Bannière obligatoire en en-tête de run du binaire produit par `cdc build` (SPEC §4).
#[no_mangle]
pub extern "C" fn cdc_rt_banner() {
    println!("cadernis compiler — gg wp");
}

#[no_mangle]
pub unsafe extern "C" fn cdc_rt_start_turn(rt: *mut Runtime) {
    (*rt).start_turn();
}

#[no_mangle]
pub unsafe extern "C" fn cdc_rt_end_turn(rt: *mut Runtime) {
    (*rt).end_turn();
}

#[no_mangle]
pub unsafe extern "C" fn cdc_rt_pa(rt: *const Runtime) -> i64 {
    (*rt).pa()
}

#[no_mangle]
pub unsafe extern "C" fn cdc_rt_pm(rt: *const Runtime) -> i64 {
    (*rt).pm()
}

#[no_mangle]
pub unsafe extern "C" fn cdc_rt_suspicion(rt: *const Runtime) -> i64 {
    (*rt).suspicion() as i64
}

/// Débite `n` PA (coût d'un appel de bot).
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_spend_pa(rt: *mut Runtime, n: i64) {
    check_fault((*rt).spend_pa(n));
}

/// Débite le coût PA d'une affectation.
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_spend_assign(rt: *mut Runtime) {
    let n = (*rt).assign_pa();
    check_fault((*rt).spend_pa(n));
}

/// Débite le coût PA d'un `up`.
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_spend_up(rt: *mut Runtime) {
    let n = (*rt).up_pa();
    check_fault((*rt).spend_pa(n));
}

/// Déplacement vers une variable externe (model B).
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_pm_touch(rt: *mut Runtime, var_id: i64) {
    check_fault((*rt).pm_touch(var_id as u64));
}

/// `afk <ms>`.
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_afk(rt: *mut Runtime, ms: i64) {
    check_ban((*rt).afk(ms));
}

/// `up <txt>`.
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_up(rt: *mut Runtime, ptr: *const u8, len: i64) {
    let s = as_str(ptr, len);
    check_ban((*rt).up(s));
}

/// Action observable « appel de bot » (suspicion).
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_act_bot(rt: *mut Runtime, ptr: *const u8, len: i64) {
    let s = as_str(ptr, len);
    check_ban((*rt).act_bot(s));
}

/// `cd_pret(bot) -> flag`.
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_cd_pret(rt: *const Runtime, ptr: *const u8, len: i64) -> i64 {
    let s = as_str(ptr, len);
    (*rt).cd_pret(s) as i64
}

/// Vérifie qu'un bot est appelable, sinon termine avec « sort en cooldown ».
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_guard_cd(rt: *const Runtime, ptr: *const u8, len: i64) {
    let s = as_str(ptr, len);
    if !(*rt).cd_pret(s) {
        fatal("sort en cooldown");
    }
}

/// Met un bot en cooldown après appel.
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_set_cooldown(rt: *mut Runtime, ptr: *const u8, len: i64, cd: i64) {
    let s = as_str(ptr, len);
    (*rt).set_cooldown(s, cd);
}

/// `rand(a, b)`.
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_rand(rt: *mut Runtime, a: i64, b: i64) -> i64 {
    (*rt).rand(a, b)
}

/// `butin(min, max)`.
#[no_mangle]
pub unsafe extern "C" fn cdc_rt_butin(rt: *mut Runtime, a: i64, b: i64) -> i64 {
    (*rt).butin(a, b)
}
