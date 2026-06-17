//! Phase 6 — dérive de patch (SPEC §1.4) : un variant `pano` non épinglé voit son tag dériver
//! d'un « build » (patch_seed) à l'autre, cassant le code paresseux ; un variant épinglé `@N`
//! survit. Démontré via l'interpréteur ET le backend natif.

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
        .arg(example("patch.cdl"))
        .env("CDC_PATCH_SEED", patch_seed)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn epingle_survit_non_epingle_derive_interp() {
    // seed 0 : Connecte a le tag 0 ; seed 2 : il a dérivé.
    let s0 = run_interp("0");
    let s2 = run_interp("2");
    // variant épinglé @2 : stable dans les deux « builds »
    assert!(s0.contains("banni epingle ok"));
    assert!(s2.contains("banni epingle ok"));
    // variant non épinglé : le comportement diffère entre les deux seeds (dérive)
    assert!(
        s0.contains("connecte tag 0"),
        "seed 0 attendu tag 0 : {s0:?}"
    );
    assert!(
        s2.contains("connecte a derive"),
        "seed 2 attendu dérive : {s2:?}"
    );
    assert_ne!(s0, s2, "le code non épinglé doit casser au rebuild");
}

fn clang_present() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn build_run_native(patch_seed: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cdc_patch_{}_{}", std::process::id(), patch_seed));
    let _ = std::fs::create_dir_all(&dir);
    let st = cdc()
        .current_dir(&dir)
        .arg("build")
        .arg(example("patch.cdl"))
        .env("CDC_PATCH_SEED", patch_seed)
        .status()
        .unwrap();
    assert!(st.success());
    let bin = dir.join("patch");
    let out = Command::new(&bin).output().unwrap();
    let _ = std::fs::remove_file(&bin);
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn epingle_survit_non_epingle_derive_natif() {
    if !clang_present() {
        eprintln!("clang absent — test §1.4 natif ignoré");
        return;
    }
    // deux « builds » avec des patch_seeds différents → le binaire change de comportement.
    let s0 = build_run_native("0");
    let s2 = build_run_native("2");
    assert!(s0.contains("banni epingle ok") && s2.contains("banni epingle ok"));
    assert!(s0.contains("connecte tag 0"), "build seed 0 : {s0:?}");
    assert!(s2.contains("connecte a derive"), "build seed 2 : {s2:?}");
}
