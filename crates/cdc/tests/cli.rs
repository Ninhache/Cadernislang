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
fn build_affiche_banniere() {
    let f = write_temp("banner", "// gg wp\nconnexion {}\n");
    let out = cdc().arg("build").arg(&f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("cadernis compiler — gg wp"),
        "bannière attendue absente, stdout = {stdout:?}"
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
