//! Couche d'analyse indépendante de LSP : transforme un source `.cdl` en diagnostics + rapport de
//! coût + libellés de complétion. Testable sans serveur (le backend LSP n'est qu'un adaptateur).

use cdc_ast::Item;
use cdc_sema::TourCost;
use std::collections::HashMap;

/// Diagnostic positionné (ligne/colonne 1-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub line: u32,
    pub col: u32,
    pub msg: String,
    pub code: Option<String>,
}

/// Résultat d'analyse d'un document (diagnostics). Les coûts/positions passent par [`symbols`].
pub struct Analysis {
    pub diags: Vec<Diag>,
}

/// Analyse un source : en-tête → parse → sema, produit les diagnostics.
pub fn analyze(src: &str) -> Analysis {
    if cdc_lexer::verify_header(src).is_err() {
        return Analysis {
            diags: vec![Diag {
                line: 1,
                col: 1,
                msg: "candidature contributeur refusée".to_string(),
                code: None,
            }],
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
            Analysis { diags }
        }
    }
}

/// Symboles d'un programme : position de définition + coût (pour hover / go-to-definition).
#[derive(Default)]
pub struct Symbols {
    /// nom de `bot` → (ligne, colonne de définition, coût PA effectif).
    pub bots: HashMap<String, (u32, u32, i64)>,
    /// nom de `pano`/`perso` → (ligne, colonne de définition).
    pub types: HashMap<String, (u32, u32)>,
    /// Usage par `tour` (pour le hover sur le mot-clé `tour`).
    pub tours: Vec<TourCost>,
    pub max_pa: i64,
    pub max_pm: i64,
}

/// Extrait les symboles (définitions + coûts) d'un source valide ; vide sinon.
pub fn symbols(src: &str) -> Symbols {
    let prog = match cdc_parser::parse(src) {
        Ok(p) => p,
        Err(_) => return Symbols::default(),
    };
    let report = cdc_sema::report(&prog);
    let costs: HashMap<&str, i64> = report
        .bots
        .iter()
        .map(|b| (b.name.as_str(), b.cost))
        .collect();
    let mut sy = Symbols {
        max_pa: report.max_pa,
        max_pm: report.max_pm,
        tours: report.tours.clone(),
        ..Default::default()
    };
    for item in &prog.items {
        match item {
            Item::Bot(b) => {
                let c = costs.get(b.name.as_str()).copied().unwrap_or(0);
                sy.bots.insert(b.name.clone(), (b.line, b.col, c));
            }
            Item::Pano(p) => {
                sy.types.insert(p.name.clone(), (p.line, p.col));
            }
            Item::Perso(p) => {
                sy.types.insert(p.name.clone(), (p.line, p.col));
            }
            _ => {}
        }
    }
    sy
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
    fn analyse_golden_sans_diag() {
        let src = include_str!("../../../examples/dopeuls.cdl");
        assert!(analyze(src).diags.is_empty());
    }

    #[test]
    fn symboles_golden() {
        let src = include_str!("../../../examples/dopeuls.cdl");
        let sy = symbols(src);
        // bot connu avec son coût ; un tour à 5 PA
        assert_eq!(sy.bots.get("tuer_dopeul").map(|(_, _, c)| *c), Some(4));
        assert_eq!(sy.tours.len(), 1);
        assert_eq!(sy.tours[0].pa, 5);
    }

    #[test]
    fn analyse_sans_entete() {
        let a = analyze("connexion {}");
        assert!(a.diags.iter().any(|d| d.msg.contains("candidature")));
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
