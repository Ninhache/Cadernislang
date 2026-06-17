//! Tests d'intégration de la CLI `cdc` pour la Phase 0.
//!
//! Couvre le critère d'acceptation §9.5 (en-tête `// gg wp`) et le câblage des sous-commandes.

use std::path::PathBuf;
use std::process::Command;

/// Écrit un `.cdl` temporaire unique et renvoie son chemin.
fn write_temp(name: &str, content: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    // Unicité sans dépendance externe : pid + nom de test.
    p.push(format!("cdc_test_{}_{}.cdl", std::process::id(), name));
    std::fs::write(&p, content).expect("écriture du fichier temporaire");
    p
}

fn cdc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cdc"))
}

#[test]
fn header_present_check_ok() {
    let f = write_temp("hdr_ok", "// gg wp\nconnexion {}\n");
    let out = cdc().arg("check").arg(&f).output().unwrap();
    assert!(
        out.status.success(),
        "check devrait réussir avec un en-tête valide"
    );
    let _ = std::fs::remove_file(&f);
}

#[test]
fn header_absent_refuse() {
    let f = write_temp("hdr_ko", "connexion {}\n");
    let out = cdc().arg("check").arg(&f).output().unwrap();
    assert!(
        !out.status.success(),
        "check devrait échouer sans en-tête (§9.5)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("candidature contributeur refusée"),
        "message attendu absent, stderr = {stderr:?}"
    );
    let _ = std::fs::remove_file(&f);
}

#[test]
fn build_sans_llvm_signale_backend_indisponible() {
    // La bannière est émise par le binaire PRODUIT (§4.2), testée dans l'acceptation Phase 5.
    // Sans la feature `llvm`, `cdc build` doit échouer proprement (backend non compilé).
    let f = write_temp("banner", "// gg wp\nconnexion {}\n");
    let out = cdc().arg("build").arg(&f).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("backend LLVM non compilé"));
    let _ = std::fs::remove_file(&f);
}

#[test]
fn check_tour_trop_gourmand_e_pa() {
    // §9.3 : un tour statiquement > MAX_PA → cdc check échoue avec error[E-PA].
    let f = write_temp(
        "epa",
        "// gg wp\nconnexion {\n loot x = 0\n tour {\n  x = 1\n  x = 2\n  x = 3\n  x = 4\n  x = 5\n  x = 6\n  x = 7\n }\n passer\n}\n",
    );
    let out = cdc().arg("check").arg(&f).output().unwrap();
    assert!(!out.status.success(), "check doit échouer (§9.3)");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error[E-PA]"),
        "diagnostic E-PA attendu, stderr = {stderr:?}"
    );
    let _ = std::fs::remove_file(&f);
}

#[test]
fn sous_commande_inconnue_echoue() {
    let out = cdc().arg("farmle").output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn sans_argument_echoue() {
    let out = cdc().output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn run_golden_aboutit() {
    // §9.1 (partiel Phase 2 : sans suspicion/cd) — le farm aboutit, code retour 0.
    let golden = format!("{}/../../examples/dopeuls.cdl", env!("CARGO_MANIFEST_DIR"));
    let out = cdc()
        .arg("run")
        .arg(&golden)
        .env("CDC_SEED", "7")
        .output()
        .unwrap();
    assert!(out.status.success(), "run devrait réussir (rc 0)");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("objectif atteint gg"),
        "sortie attendue absente, stdout = {stdout:?}"
    );
}

#[test]
fn run_variante_afk_fixe_bannit() {
    // §9.2 : afk 3000 fixe → « error: compte banni », code retour ≠ 0, objectif non atteint.
    let ban = format!(
        "{}/../../examples/dopeuls_ban.cdl",
        env!("CARGO_MANIFEST_DIR")
    );
    let out = cdc()
        .arg("run")
        .arg(&ban)
        .env("CDC_SEED", "7")
        .output()
        .unwrap();
    assert!(!out.status.success(), "doit terminer avec un code ≠ 0");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("compte banni"),
        "message de ban attendu, stderr = {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("objectif atteint gg"),
        "l'objectif ne doit pas être atteint"
    );
}

#[test]
fn run_calcul_pur() {
    // Couche calcul (§1.5) : exécution déterministe sans action observable.
    let somme = format!("{}/../../examples/somme.cdl", env!("CARGO_MANIFEST_DIR"));
    let out = cdc()
        .arg("run")
        .arg(&somme)
        .env("CDC_SEED", "7")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("fin du calcul"));
}
