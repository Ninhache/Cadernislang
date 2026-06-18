//! Couche d'analyse indépendante de LSP : transforme un source `.cdl` en diagnostics + rapport de
//! coût + libellés de complétion. Testable sans serveur (le backend LSP n'est qu'un adaptateur).

use cdc_ast::Item;
use cdc_sema::CostReport;

/// Diagnostic positionné (ligne/colonne 1-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub line: u32,
    pub col: u32,
    pub msg: String,
    pub code: Option<String>,
}

/// Résultat d'analyse d'un document.
pub struct Analysis {
    pub diags: Vec<Diag>,
    pub report: Option<CostReport>,
}

/// Analyse un source : en-tête → parse → sema, et calcule le rapport de coût si valide.
pub fn analyze(src: &str) -> Analysis {
    if cdc_lexer::verify_header(src).is_err() {
        return Analysis {
            diags: vec![Diag {
                line: 1,
                col: 1,
                msg: "candidature contributeur refusée".to_string(),
                code: None,
            }],
            report: None,
        };
    }
    match cdc_parser::parse(src) {
        Err(e) => Analysis {
            diags: vec![Diag {
                line: e.line,
                col: e.col,
                msg: e.msg,
                code: None,
            }],
            report: None,
        },
        Ok(prog) => {
            let diags = cdc_sema::check(&prog)
                .into_iter()
                .map(|d| Diag {
                    line: d.line,
                    col: d.col,
                    msg: d.msg,
                    code: d.code.map(|c| c.to_string()),
                })
                .collect();
            let report = Some(cdc_sema::report(&prog));
            Analysis { diags, report }
        }
    }
}

const MOTS_CLES: &[&str] = &[
    "bot",
    "coute",
    "cd",
    "connexion",
    "serveur",
    "tour",
    "passer",
    "farm",
    "grind",
    "loot",
    "ban",
    "gg",
    "detect",
    "sinon",
    "et",
    "ou",
    "pas",
    "up",
    "afk",
    "pa",
    "pm",
    "suspicion",
    "legit",
    "cheat",
    "pano",
    "perso",
    "kamas",
    "flag",
    "txt",
];
const BUILTINS: &[&str] = &["rand", "butin", "cd_pret"];

/// Libellés de complétion : mots-clés + builtins + noms déclarés (bots, pano/perso + membres).
pub fn completion_labels(src: &str) -> Vec<String> {
    let mut out: Vec<String> = MOTS_CLES
        .iter()
        .chain(BUILTINS)
        .map(|s| s.to_string())
        .collect();
    if let Ok(prog) = cdc_parser::parse(src) {
        for item in &prog.items {
            match item {
                Item::Bot(b) => out.push(b.name.clone()),
                Item::Pano(p) => {
                    out.push(p.name.clone());
                    out.extend(p.variants.iter().map(|v| v.name.clone()));
                }
                Item::Perso(p) => {
                    out.push(p.name.clone());
                    out.extend(p.fields.iter().map(|f| f.name.clone()));
                }
                _ => {}
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyse_golden_sans_diag_avec_rapport() {
        let src = include_str!("../../../examples/dopeuls.cdl");
        let a = analyze(src);
        assert!(a.diags.is_empty(), "diags={:?}", a.diags);
        let r = a.report.unwrap();
        assert_eq!(r.tours.len(), 1);
        assert_eq!(r.tours[0].pa, 5);
    }

    #[test]
    fn analyse_sans_entete() {
        let a = analyze("connexion {}");
        assert!(a.diags.iter().any(|d| d.msg.contains("candidature")));
        assert!(a.report.is_none());
    }

    #[test]
    fn analyse_e_pa() {
        let src = include_str!("../../../examples/trop_gourmand.cdl");
        let a = analyze(src);
        assert!(a.diags.iter().any(|d| d.code.as_deref() == Some("E-PA")));
    }

    #[test]
    fn completions_incluent_keywords_et_bots() {
        let c = completion_labels("// gg wp\nbot frappe() {}\nconnexion {}");
        assert!(c.contains(&"detect".to_string()));
        assert!(c.contains(&"butin".to_string()));
        assert!(c.contains(&"frappe".to_string()));
    }
}
