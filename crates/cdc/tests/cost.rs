//! `cdc cost` (Phase 8) : coût PA effectif des bots + usage PA/PM par tour.

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

#[test]
fn cost_golden() {
    let out = cdc()
        .arg("cost")
        .arg(example("dopeuls.cdl"))
        .env("CDC_SEED", "7")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("tuer_dopeul : 4 PA"), "coût bot attendu : {s:?}");
    assert!(s.contains("5/6 PA"), "usage PA du tour attendu : {s:?}");
    assert!(s.contains("1/3 PM"), "usage PM du tour attendu : {s:?}");
}
