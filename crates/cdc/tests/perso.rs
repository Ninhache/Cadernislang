//! Phase 6b — `perso` (structs) : accès de champ par nom (stable) + dérive du **tag** de champ
//! (SPEC §1.4). Le backend natif accepte les tags `Perso.champ` mais rejette la construction et
//! l'accès d'instance (interp uniquement) — c'est vérifié ici aussi.

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

fn run_interp(patch_seed: &str) -> String {
    let out = cdc()
        .arg("run")
        .arg(example("perso.cdl"))
        .env("CDC_PATCH_SEED", patch_seed)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn acces_stable_tag_champ_derive() {
    let s0 = run_interp("0");
    let s2 = run_interp("2");
    // accès par nom : stable
    assert!(s0.contains("vie ok") && s2.contains("vie ok"));
    // tag de champ épinglé @0 : stable
    assert!(s0.contains("niveau tag 0 epingle") && s2.contains("niveau tag 0 epingle"));
    // tag de champ non épinglé : dérive entre deux patchs
    assert!(s0.contains("vie tag 1"), "seed 0 : {s0:?}");
    assert!(s2.contains("vie tag autre"), "seed 2 : {s2:?}");
}

#[test]
fn construction_perso_rejetee_en_natif() {
    let clang = Command::new("clang")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !clang {
        eprintln!("clang absent — test ignoré");
        return;
    }
    let mut dir = std::env::temp_dir();
    dir.push(format!("cdc_perso_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let out = cdc()
        .current_dir(&dir)
        .arg("build")
        .arg(example("perso.cdl"))
        .output()
        .unwrap();
    // construction de perso non supportée par le backend natif → échec clair
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("backend natif"),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}
