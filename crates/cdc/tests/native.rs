//! Acceptation §9.6 : `cdc build` produit un binaire natif au comportement identique à `cdc run`.
//!
//! Nécessite `clang` au runtime. Si `clang` est absent, le test est ignoré proprement (le backend
//! natif émet de l'IR textuel compilé par clang — voir ADR-002). Nécessite aussi la staticlib
//! `libcdc_runtime.a` (produite par cargo à côté du binaire `cdc`).

use std::path::PathBuf;
use std::process::Command;

fn clang_present() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../../examples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// Construit `src` nativement dans un répertoire temporaire et renvoie le chemin du binaire.
fn build_native(src: &str, tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cdc_native_{}_{}", std::process::id(), tag));
    let _ = std::fs::create_dir_all(&dir);
    let status = Command::new(env!("CARGO_BIN_EXE_cdc"))
        .current_dir(&dir)
        .arg("build")
        .arg(example(src))
        .status()
        .expect("lancement de cdc build");
    assert!(status.success(), "cdc build a échoué pour {src}");
    let stem = std::path::Path::new(src)
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap();
    dir.join(stem)
}

#[test]
fn s9_6_golden_natif_identique() {
    if !clang_present() {
        eprintln!("clang absent — test §9.6 ignoré");
        return;
    }
    let bin = build_native("dopeuls.cdl", "golden");
    let out = Command::new(&bin)
        .env("CDC_SEED", "7")
        .output()
        .expect("exécution du binaire natif");
    assert!(
        out.status.success(),
        "le binaire natif devrait réussir (§9.1/§9.6)"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // bannière obligatoire (§4) + même résultat que `cdc run`
    assert!(
        stdout.contains("cadernis compiler — gg wp"),
        "bannière absente : {stdout:?}"
    );
    assert!(
        stdout.contains("objectif atteint gg"),
        "résultat absent : {stdout:?}"
    );
    let _ = std::fs::remove_file(&bin);
}

#[test]
fn s9_6_variante_ban_natif() {
    if !clang_present() {
        eprintln!("clang absent — test §9.6 ignoré");
        return;
    }
    let bin = build_native("dopeuls_ban.cdl", "ban");
    let out = Command::new(&bin)
        .env("CDC_SEED", "7")
        .output()
        .expect("exécution du binaire natif");
    assert!(
        !out.status.success(),
        "le binaire natif doit bannir (code ≠ 0)"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("compte banni"));
    let _ = std::fs::remove_file(&bin);
}
