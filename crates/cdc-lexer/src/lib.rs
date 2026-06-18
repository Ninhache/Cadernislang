//! Front-end lexical de cadernislang : vérification de l'en-tête + tokenisation (`logos`).

use logos::Logos;

/// En-tête obligatoire en tête de tout fichier `.cdl` (easter egg, SPEC §4).
pub const HEADER: &str = "// gg wp";

/// Échec de la vérification de l'en-tête : la ligne 1 n'est pas `// gg wp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderError;

/// Vérifie que la **première ligne** du source est exactement `// gg wp` (SPEC §4 / §5).
///
/// Les commentaires `//` sont des trivia ignorés partout ailleurs, mais la ligne 1 est
/// imposée comme rite d'entrée. Un éventuel `\r` final (fichiers CRLF) est toléré.
///
/// # Erreurs
/// Retourne [`HeaderError`] si la ligne 1 diffère. Le driver émet alors
/// `error: candidature contributeur refusée` et n'effectue aucune compilation.
pub fn verify_header(source: &str) -> Result<(), HeaderError> {
    let first = source.lines().next().unwrap_or("");
    if first.trim_end_matches('\r') == HEADER {
        Ok(())
    } else {
        Err(HeaderError)
    }
}

/// Retire les underscores et parse un littéral entier (`1_000_000` → `1000000`, SPEC §2).
fn lex_int(lex: &mut logos::Lexer<Token>) -> Option<i64> {
    let cleaned: String = lex.slice().chars().filter(|c| *c != '_').collect();
    cleaned.parse().ok()
}

/// Extrait le contenu d'un littéral chaîne et déséchappe `\" \\ \n \t`.
fn lex_str(lex: &mut logos::Lexer<Token>) -> String {
    let raw = lex.slice();
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Jeton lexical. Commentaires (`// …`) et espaces sont ignorés (trivia, SPEC §6).
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
pub enum Token {
    // --- mots-clés (réservés) ---
    #[token("bot")]
    Bot,
    #[token("coute")]
    Coute,
    #[token("cd")]
    Cd,
    #[token("connexion")]
    Connexion,
    #[token("serveur")]
    Serveur,
    #[token("tour")]
    Tour,
    #[token("passer")]
    Passer,
    #[token("farm")]
    Farm,
    #[token("grind")]
    Grind,
    #[token("loot")]
    Loot,
    #[token("ban")]
    Ban,
    #[token("gg")]
    Gg,
    #[token("detect")]
    Detect,
    #[token("sinon")]
    Sinon,
    #[token("et")]
    Et,
    #[token("ou")]
    Ou,
    #[token("pas")]
    Pas,
    #[token("up")]
    Up,
    #[token("afk")]
    Afk,
    // pseudo-variables runtime (SPEC §3) ; `pa` sert aussi de suffixe à `coute N pa`.
    #[token("pa")]
    Pa,
    #[token("pm")]
    Pm,
    #[token("suspicion")]
    Suspicion,
    // littéraux booléens
    #[token("legit")]
    Legit,
    #[token("cheat")]
    Cheat,

    // --- ponctuation ---
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("#")]
    Hash,
    #[token(".")]
    Dot,
    #[token("@")]
    At,

    // --- opérateurs (les plus longs en premier pour le longest-match) ---
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,

    // --- littéraux & identifiants ---
    #[regex(r"[0-9][0-9_]*", lex_int)]
    Int(i64),
    #[regex(r#""([^"\\]|\\.)*""#, lex_str)]
    Str(String),
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),
}

/// Jeton accompagné de sa position source (1-based).
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub tok: Token,
    pub line: u32,
    pub col: u32,
}

/// Caractère invalide rencontré pendant la tokenisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexError {
    pub line: u32,
    pub col: u32,
}

/// Convertit un offset octet en (ligne, colonne) 1-based.
fn line_col(src: &str, off: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, c) in src.char_indices() {
        if i >= off {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Tokenise le source complet (l'en-tête `// gg wp` est ignoré comme commentaire).
///
/// # Erreurs
/// Retourne [`LexError`] au premier caractère non reconnu.
pub fn tokenize(src: &str) -> Result<Vec<Spanned>, LexError> {
    let mut out = Vec::new();
    let mut lex = Token::lexer(src);
    while let Some(res) = lex.next() {
        let (line, col) = line_col(src, lex.span().start);
        match res {
            Ok(tok) => out.push(Spanned { tok, line, col }),
            Err(()) => return Err(LexError { line, col }),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_valide() {
        assert!(verify_header("// gg wp\nconnexion {}\n").is_ok());
    }

    #[test]
    fn header_valide_crlf() {
        assert!(verify_header("// gg wp\r\nconnexion {}\r\n").is_ok());
    }

    #[test]
    fn header_absent() {
        assert_eq!(verify_header("connexion {}\n"), Err(HeaderError));
    }

    #[test]
    fn header_presque_bon() {
        assert_eq!(verify_header("//gg wp\n"), Err(HeaderError));
        assert_eq!(verify_header("// GG WP\n"), Err(HeaderError));
    }

    #[test]
    fn source_vide() {
        assert_eq!(verify_header(""), Err(HeaderError));
    }

    #[test]
    fn underscores_dans_entiers() {
        let toks = tokenize("// gg wp\n1_000_000").unwrap();
        assert_eq!(toks.last().unwrap().tok, Token::Int(1_000_000));
    }

    #[test]
    fn mots_cles_vs_identifiants() {
        let toks = tokenize("// gg wp\nbot total").unwrap();
        assert_eq!(toks[0].tok, Token::Bot);
        assert_eq!(toks[1].tok, Token::Ident("total".to_string()));
    }

    #[test]
    fn operateurs_longest_match() {
        let toks = tokenize("// gg wp\n<= < += =").unwrap();
        let kinds: Vec<_> = toks.into_iter().map(|s| s.tok).collect();
        assert_eq!(kinds, vec![Token::Le, Token::Lt, Token::PlusEq, Token::Eq]);
    }

    #[test]
    fn commentaires_ignores_et_position() {
        // le commentaire de ligne 2 est sauté ; `passer` est en ligne 3.
        let toks = tokenize("// gg wp\n// note\npasser").unwrap();
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].tok, Token::Passer);
        assert_eq!(toks[0].line, 3);
    }

    #[test]
    fn chaine_avec_echappement() {
        let toks = tokenize("// gg wp\n\"a\\nb\"").unwrap();
        assert_eq!(toks.last().unwrap().tok, Token::Str("a\nb".to_string()));
    }

    #[test]
    fn caractere_invalide() {
        // `%` n'est pas un token reconnu.
        assert!(tokenize("// gg wp\n%").is_err());
    }
}
