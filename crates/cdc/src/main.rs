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
         cdc check <fichier.cdl>   # lexer + parser + sema, sans exécution\n  \
         cdc cost  <fichier.cdl>   # coût PA des bots + usage PA/PM par tour\n\n\
         {BANNER}"
    )
}

/// Localise la staticlib `cdc-runtime` (produite dans le même répertoire que le binaire `cdc`).
fn runtime_staticlib() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("libcdc_runtime.a")))
        .unwrap_or_else(|| std::path::PathBuf::from("libcdc_runtime.a"))
}

/// Affiche le rapport de coût (`cdc cost`).
fn print_cost(r: &cdc_sema::CostReport) {
    println!("bots — coût PA effectif :");
    if r.bots.is_empty() {
        println!("  (aucun)");
    }
    for b in &r.bots {
        let src = if b.declared {
            "déclaré"
        } else {
            "auto-dérivé"
        };
        println!("  {} : {} PA ({src})", b.name, b.cost);
    }
    println!(
        "tours — usage par tour (budget {} PA / {} PM) :",
        r.max_pa, r.max_pm
    );
    if r.tours.is_empty() {
        println!("  (aucun)");
    }
    for t in &r.tours {
        if t.dynamic {
            println!(
                "  ligne {}, col {} : dynamique (boucle/tour imbriqué → vérif au runtime)",
                t.line, t.col
            );
        } else {
            let fpa = if t.pa > r.max_pa { "  ⚠️ E-PA" } else { "" };
            let fpm = if t.pm > r.max_pm { "  ⚠️ E-PM" } else { "" };
            println!(
                "  ligne {}, col {} : {}/{} PA, {}/{} PM{fpa}{fpm}",
                t.line, t.col, t.pa, r.max_pa, t.pm, r.max_pm
            );
        }
    }
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

    if !matches!(cmd, "run" | "build" | "check" | "cost") {
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

    // Front-end commun à toutes les commandes : lexer + parser → AST (Phase 1).
    let program = match cdc_parser::parse(&source) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return Err(());
        }
    };

    // Analyse sémantique : résolution, typage, budget statique PA/PM (Phase 4).
    let diags = cdc_sema::check(&program);
    if !diags.is_empty() {
        for d in &diags {
            eprintln!("{d}");
        }
        return Err(());
    }

    match cmd {
        "build" => {
            let stem = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("a");
            let out = std::path::PathBuf::from(stem);
            let rt_lib = runtime_staticlib();
            match cdc_codegen::build(&program, &out, &rt_lib) {
                Ok(()) => println!("binaire natif produit : {}", out.display()),
                Err(e) => {
                    eprintln!("error: {e}");
                    return Err(());
                }
            }
        }
        "run" => {
            if let Err(e) = cdc_interp::run(&program) {
                eprintln!("error: {e}");
                return Err(());
            }
        }
        "check" => {
            // SPEC §8 / issue #8 : `cdc check` affiche l'AST en debug.
            println!("{program:#?}");
        }
        "cost" => print_cost(&cdc_sema::report(&program)),
        _ => unreachable!("commande déjà validée"),
    }
    Ok(())
}
