//! `cdc` — Cadernis Compiler : driver CLI.
//!
//! Phase 0 (scaffold) : parse les sous-commandes `run` / `build` / `check`, lit le fichier,
//! applique le check de l'en-tête easter egg (`// gg wp`), affiche la bannière sur `build`.
//! Le lexer/parser/interp/codegen réels sont câblés aux phases suivantes.

use std::process::ExitCode;

/// Bannière affichée en en-tête de `cdc build` (easter egg, SPEC §4).
const BANNER: &str = "cadernis compiler — gg wp";
/// Message fatal quand l'en-tête `// gg wp` manque (SPEC §4 / §9.5).
const ERR_CANDIDATURE: &str = "error: candidature contributeur refusée";

fn usage() -> String {
    format!(
        "cdc — Cadernis Compiler\n\n\
         usage:\n  \
         cdc run   <fichier.cdl>   # interprète (cdc-interp)\n  \
         cdc build <fichier.cdl>   # compile via LLVM + link cdc-runtime → binaire natif\n  \
         cdc check <fichier.cdl>   # lexer + parser + sema, sans exécution\n\n\
         {BANNER}"
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

/// Aiguille la sous-commande. Retourne `Err(())` sur toute erreur (code retour ≠ 0, SPEC §12).
fn dispatch(args: &[String]) -> Result<(), ()> {
    let cmd = match args.first() {
        Some(c) => c.as_str(),
        None => {
            eprintln!("{}", usage());
            return Err(());
        }
    };

    if matches!(cmd, "-h" | "--help" | "help") {
        println!("{}", usage());
        return Ok(());
    }

    if !matches!(cmd, "run" | "build" | "check") {
        eprintln!("error: sous-commande inconnue « {cmd} »\n\n{}", usage());
        return Err(());
    }

    let path = match args.get(1) {
        Some(p) => p,
        None => {
            eprintln!(
                "error: la commande « {cmd} » attend un fichier .cdl\n\n{}",
                usage()
            );
            return Err(());
        }
    };

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: lecture impossible de « {path} » : {e}");
            return Err(());
        }
    };

    // Rite d'entrée : pas d'en-tête `// gg wp` → aucune compilation (SPEC §4 / §9.5).
    if cdc_lexer::verify_header(&source).is_err() {
        eprintln!("{ERR_CANDIDATURE}");
        return Err(());
    }

    // Phase 0 : pipeline non encore câblé. On confirme juste que l'en-tête passe.
    match cmd {
        "build" => {
            println!("{BANNER}");
            println!("note: backend LLVM non encore implémenté (Phase 5) — en-tête validé.");
        }
        "run" => {
            println!("note: interpréteur non encore implémenté (Phase 2) — en-tête validé.");
        }
        "check" => {
            println!("ok: en-tête validé (lexer/parser/sema arrivent en Phases 1 & 4).");
        }
        _ => unreachable!("commande déjà validée"),
    }
    Ok(())
}
