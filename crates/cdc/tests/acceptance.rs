//! Suite d'acceptation §9 (issue #25) : chaque test correspond à un critère de `docs/SPEC.md` §9,
//! exécuté de bout en bout via le binaire `cdc` sur les exemples du dépôt.
//!
//! §9.6 (binaire natif via `cdc build`) et §9.7 (parité interp ↔ LLVM) relèvent de la Phase 5
//! (backend LLVM) et seront ajoutés là — ils nécessitent LLVM 18.

use std::path::PathBuf;
use std::process::Command;

fn cdc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cdc"))
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../../examples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// §9.1 — `cdc run examples/dopeuls.cdl` : le farm aboutit, `objectif atteint gg`, code retour 0.
#[test]
fn s9_1_golden_aboutit() {
    let out = cdc()
        .arg("run")
        .arg(example("dopeuls.cdl"))
        .env("CDC_SEED", "7")
        .output()
        .unwrap();
    assert!(out.status.success(), "code retour attendu 0");
    assert!(String::from_utf8_lossy(&out.stdout).contains("objectif atteint gg"));
}

/// §9.2 — variante `afk 3000` : `error: compte banni`, code ≠ 0, objectif non atteint.
#[test]
fn s9_2_afk_fixe_bannit() {
    let out = cdc()
        .arg("run")
        .arg(example("dopeuls_ban.cdl"))
        .env("CDC_SEED", "7")
        .output()
        .unwrap();
    assert!(!out.status.success(), "code retour attendu ≠ 0");
    assert!(String::from_utf8_lossy(&out.stderr).contains("compte banni"));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("objectif atteint gg"));
}

/// §9.3 — un `tour` statiquement > 6 PA : `cdc check` échoue avec `error[E-PA]`.
#[test]
fn s9_3_tour_gourmand_epa() {
    let out = cdc()
        .arg("check")
        .arg(example("trop_gourmand.cdl"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("error[E-PA]"));
}

/// §9.4 — appel d'un `bot` en cooldown : erreur runtime `sort en cooldown`.
#[test]
fn s9_4_bot_en_cooldown() {
    let out = cdc()
        .arg("run")
        .arg(example("cooldown.cdl"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("sort en cooldown"));
}

/// §9.5 — fichier sans `// gg wp` : `error: candidature contributeur refusée`, aucune compilation.
#[test]
fn s9_5_sans_entete_refuse() {
    let out = cdc()
        .arg("check")
        .arg(example("sans_entete.cdl"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("candidature contributeur refusée"));
}
