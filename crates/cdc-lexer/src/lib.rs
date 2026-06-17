//! Front-end lexical de cadernislang.
//!
//! Phase 0 : seule la vérification de l'en-tête easter egg est implémentée.
//! La tokenisation (`logos`) arrive en Phase 1.

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
        // espace en trop, casse, etc. → refusé
        assert_eq!(verify_header("//gg wp\n"), Err(HeaderError));
        assert_eq!(verify_header("// GG WP\n"), Err(HeaderError));
    }

    #[test]
    fn source_vide() {
        assert_eq!(verify_header(""), Err(HeaderError));
    }
}
