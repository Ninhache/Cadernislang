//! Parser à descente récursive : source → AST (`cdc_ast::Program`).
//!
//! Implémente la grammaire de `docs/SPEC.md` §6, avec la précédence d'opérateurs et les formes
//! `up`/`afk` à mot-clé (Déviation 7) et `grind … de A a B` (bornes incluses).

use cdc_ast::*;
use cdc_lexer::{Spanned, Token};

/// Erreur de parsing localisée (ligne/colonne 1-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub msg: String,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (ligne {}, colonne {})",
            self.msg, self.line, self.col
        )
    }
}

type PResult<T> = Result<T, ParseError>;

/// Parse un source complet en [`Program`].
///
/// # Erreurs
/// Erreur lexicale (caractère invalide) ou syntaxique, avec position.
pub fn parse(src: &str) -> PResult<Program> {
    let toks = cdc_lexer::tokenize(src).map_err(|e| ParseError {
        msg: "caractère invalide".to_string(),
        line: e.line,
        col: e.col,
    })?;
    let mut p = Parser { toks, pos: 0 };
    p.program()
}

struct Parser {
    toks: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos).map(|s| &s.tok)
    }

    fn peek_at(&self, n: usize) -> Option<&Token> {
        self.toks.get(self.pos + n).map(|s| &s.tok)
    }

    /// Position du jeton courant (ou du dernier si EOF) pour les messages d'erreur.
    fn here(&self) -> (u32, u32) {
        match self.toks.get(self.pos).or_else(|| self.toks.last()) {
            Some(s) => (s.line, s.col),
            None => (1, 1),
        }
    }

    fn bump(&mut self) -> Option<Spanned> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn err<T>(&self, msg: impl Into<String>) -> PResult<T> {
        let (line, col) = self.here();
        Err(ParseError {
            msg: msg.into(),
            line,
            col,
        })
    }

    /// Consomme un jeton unitaire attendu (comparaison par variante, payload ignoré).
    fn expect(&mut self, want: &Token, what: &str) -> PResult<()> {
        match self.peek() {
            Some(t) if std::mem::discriminant(t) == std::mem::discriminant(want) => {
                self.bump();
                Ok(())
            }
            _ => self.err(format!("attendu {what}")),
        }
    }

    /// `true` si le jeton courant a la même variante que `t`.
    fn at(&self, t: &Token) -> bool {
        matches!(self.peek(), Some(x) if std::mem::discriminant(x) == std::mem::discriminant(t))
    }

    fn eat(&mut self, t: &Token) -> bool {
        if self.at(t) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> PResult<String> {
        match self.peek() {
            Some(Token::Ident(s)) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            _ => self.err("identifiant attendu"),
        }
    }

    /// Consomme un identifiant contextuel dont le texte doit valoir `kw` (p. ex. `de`, `a`).
    fn keyword_ident(&mut self, kw: &str) -> PResult<()> {
        match self.peek() {
            Some(Token::Ident(s)) if s == kw => {
                self.bump();
                Ok(())
            }
            _ => self.err(format!("attendu « {kw} »")),
        }
    }

    fn int(&mut self) -> PResult<i64> {
        match self.peek() {
            Some(Token::Int(n)) => {
                let n = *n;
                self.bump();
                Ok(n)
            }
            _ => self.err("entier attendu"),
        }
    }

    // ---------------------------------------------------------------- programme

    fn program(&mut self) -> PResult<Program> {
        let mut pragmas = Vec::new();
        while self.at(&Token::Hash) {
            let (line, col) = self.here();
            self.bump(); // #
            let key = self.ident()?;
            let value = self.int()?;
            pragmas.push(Pragma {
                key,
                value,
                line,
                col,
            });
        }

        let mut items = Vec::new();
        while self.peek().is_some() {
            items.push(self.item()?);
        }
        Ok(Program { pragmas, items })
    }

    fn item(&mut self) -> PResult<Item> {
        match self.peek() {
            Some(Token::Serveur) => {
                self.bump();
                let name = self.ident()?;
                Ok(Item::Serveur(name))
            }
            Some(Token::Bot) => Ok(Item::Bot(self.bot()?)),
            Some(Token::Connexion) => {
                self.bump();
                Ok(Item::Connexion(self.block()?))
            }
            Some(Token::Ident(s)) if s == "pano" => Ok(Item::Pano(self.pano()?)),
            _ => self.err("attendu « serveur », « bot », « pano » ou « connexion »"),
        }
    }

    fn bot(&mut self) -> PResult<Bot> {
        let (line, col) = self.here();
        self.expect(&Token::Bot, "« bot »")?;
        let name = self.ident()?;
        self.expect(&Token::LParen, "« ( »")?;
        let mut params = Vec::new();
        if !self.at(&Token::RParen) {
            loop {
                let pname = self.ident()?;
                self.expect(&Token::Colon, "« : »")?;
                let ty = self.parse_type()?;
                params.push(Param { name: pname, ty });
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(&Token::RParen, "« ) »")?;

        let ret = if self.eat(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        // suffixes optionnels: `, coute N pa` et `, cd M`
        let mut cost_pa = None;
        let mut cd = None;
        while self.eat(&Token::Comma) {
            match self.peek() {
                Some(Token::Coute) => {
                    self.bump();
                    cost_pa = Some(self.int()?);
                    self.expect(&Token::Pa, "« pa »")?;
                }
                Some(Token::Cd) => {
                    self.bump();
                    cd = Some(self.int()?);
                }
                _ => return self.err("attendu « coute » ou « cd » après « , »"),
            }
        }

        let body = self.block()?;
        Ok(Bot {
            name,
            params,
            ret,
            cost_pa,
            cd,
            body,
            line,
            col,
        })
    }

    /// `pano Nom { [@N] Variant , … }` (le mot `pano` est contextuel).
    fn pano(&mut self) -> PResult<Pano> {
        let (line, col) = self.here();
        self.bump(); // pano
        let name = self.ident()?;
        self.expect(&Token::LBrace, "« { »")?;
        let mut variants = Vec::new();
        while !self.at(&Token::RBrace) {
            if self.peek().is_none() {
                return self.err("« } » manquant");
            }
            let pin = if self.eat(&Token::At) {
                Some(self.int()?)
            } else {
                None
            };
            let vname = self.ident()?;
            variants.push(Variant { name: vname, pin });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace, "« } »")?;
        Ok(Pano {
            name,
            variants,
            line,
            col,
        })
    }

    fn parse_type(&mut self) -> PResult<Type> {
        let name = self.ident()?;
        Ok(match name.as_str() {
            "kamas" => Type::Kamas,
            "flag" => Type::Flag,
            "txt" => Type::Txt,
            "afk_total" => Type::AfkTotal,
            _ => Type::Named(name),
        })
    }

    // ---------------------------------------------------------------- blocs & stmts

    fn block(&mut self) -> PResult<Block> {
        self.expect(&Token::LBrace, "« { »")?;
        let mut stmts = Vec::new();
        while !self.at(&Token::RBrace) {
            if self.peek().is_none() {
                return self.err("« } » manquant");
            }
            stmts.push(self.stmt()?);
        }
        self.expect(&Token::RBrace, "« } »")?;
        Ok(Block { stmts })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        match self.peek() {
            Some(Token::Loot) => self.decl(false),
            Some(Token::Ban) => self.decl(true),
            Some(Token::Tour) => {
                self.bump();
                Ok(Stmt::Tour(self.block()?))
            }
            Some(Token::Passer) => {
                self.bump();
                Ok(Stmt::Passer)
            }
            Some(Token::Farm) => {
                self.bump();
                let cond = self.expr()?;
                let body = self.block()?;
                Ok(Stmt::Farm { cond, body })
            }
            Some(Token::Grind) => {
                self.bump();
                let var = self.ident()?;
                self.keyword_ident("de")?;
                let from = self.expr()?;
                self.keyword_ident("a")?;
                let to = self.expr()?;
                let body = self.block()?;
                Ok(Stmt::Grind {
                    var,
                    from,
                    to,
                    body,
                })
            }
            Some(Token::Detect) => {
                self.bump();
                let cond = self.expr()?;
                let then_branch = self.block()?;
                let else_branch = if self.eat(&Token::Sinon) {
                    Some(self.block()?)
                } else {
                    None
                };
                Ok(Stmt::Detect {
                    cond,
                    then_branch,
                    else_branch,
                })
            }
            Some(Token::Gg) => {
                self.bump();
                let value = if self.starts_expr() {
                    Some(self.expr()?)
                } else {
                    None
                };
                Ok(Stmt::Gg(value))
            }
            Some(Token::Up) => {
                self.bump();
                Ok(Stmt::Up(self.expr()?))
            }
            Some(Token::Afk) => {
                self.bump();
                Ok(Stmt::Afk(self.expr()?))
            }
            // affectation `IDENT op expr`, sinon expression-instruction
            Some(Token::Ident(_)) if self.is_assign_ahead() => {
                let (line, col) = self.here();
                let name = self.ident()?;
                let op = match self.bump().map(|s| s.tok) {
                    Some(Token::Eq) => AssignOp::Set,
                    Some(Token::PlusEq) => AssignOp::Add,
                    Some(Token::MinusEq) => AssignOp::Sub,
                    _ => unreachable!("vérifié par is_assign_ahead"),
                };
                let value = self.expr()?;
                Ok(Stmt::Assign {
                    name,
                    op,
                    value,
                    line,
                    col,
                })
            }
            Some(_) => Ok(Stmt::Expr(self.expr()?)),
            None => self.err("instruction attendue"),
        }
    }

    /// `loot`/`ban IDENT [: type] = expr`.
    fn decl(&mut self, is_const: bool) -> PResult<Stmt> {
        self.bump(); // loot | ban
        let name = self.ident()?;
        let ty = if self.eat(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&Token::Eq, "« = »")?;
        let value = self.expr()?;
        Ok(if is_const {
            Stmt::Ban { name, ty, value }
        } else {
            Stmt::Loot { name, ty, value }
        })
    }

    fn is_assign_ahead(&self) -> bool {
        matches!(
            self.peek_at(1),
            Some(Token::Eq) | Some(Token::PlusEq) | Some(Token::MinusEq)
        )
    }

    fn starts_expr(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                Token::Int(_)
                    | Token::Str(_)
                    | Token::Ident(_)
                    | Token::Legit
                    | Token::Cheat
                    | Token::Pa
                    | Token::Pm
                    | Token::Suspicion
                    | Token::LParen
                    | Token::Pas
                    | Token::Minus
            )
        )
    }

    // ---------------------------------------------------------------- expressions
    // Précédence (faible → fort) : ou < et < comparaisons < +,- < *,/ < unaire.

    fn expr(&mut self) -> PResult<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_and()?;
        while self.at(&Token::Ou) {
            self.bump();
            let rhs = self.parse_and()?;
            lhs = bin(BinOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_cmp()?;
        while self.at(&Token::Et) {
            self.bump();
            let rhs = self.parse_cmp()?;
            lhs = bin(BinOp::And, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Some(Token::Lt) => BinOp::Lt,
                Some(Token::Le) => BinOp::Le,
                Some(Token::Gt) => BinOp::Gt,
                Some(Token::Ge) => BinOp::Ge,
                Some(Token::EqEq) => BinOp::Eq,
                Some(Token::NotEq) => BinOp::Ne,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_add()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_mul()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Star) => BinOp::Mul,
                Some(Token::Slash) => BinOp::Div,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let (line, col) = self.here();
        match self.peek() {
            Some(Token::Pas) => {
                self.bump();
                let e = self.parse_unary()?;
                Ok(Expr {
                    kind: ExprKind::Unary(UnOp::Not, Box::new(e)),
                    line,
                    col,
                })
            }
            Some(Token::Minus) => {
                self.bump();
                let e = self.parse_unary()?;
                Ok(Expr {
                    kind: ExprKind::Unary(UnOp::Neg, Box::new(e)),
                    line,
                    col,
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let (line, col) = self.here();
        let kind = match self.peek() {
            Some(Token::Int(n)) => {
                let n = *n;
                self.bump();
                ExprKind::Int(n)
            }
            Some(Token::Str(s)) => {
                let s = s.clone();
                self.bump();
                ExprKind::Str(s)
            }
            Some(Token::Legit) => {
                self.bump();
                ExprKind::Bool(true)
            }
            Some(Token::Cheat) => {
                self.bump();
                ExprKind::Bool(false)
            }
            Some(Token::Pa) => {
                self.bump();
                ExprKind::Pa
            }
            Some(Token::Pm) => {
                self.bump();
                ExprKind::Pm
            }
            Some(Token::Suspicion) => {
                self.bump();
                ExprKind::Suspicion
            }
            Some(Token::LParen) => {
                self.bump();
                let e = self.expr()?;
                self.expect(&Token::RParen, "« ) »")?;
                return Ok(e);
            }
            Some(Token::Ident(_)) => {
                let name = self.ident()?;
                if self.at(&Token::LParen) {
                    let args = self.call_args()?;
                    ExprKind::Call(name, args)
                } else if self.eat(&Token::Dot) {
                    let member = self.ident()?;
                    ExprKind::Path(name, member)
                } else {
                    ExprKind::Var(name)
                }
            }
            _ => return self.err("expression attendue"),
        };
        Ok(Expr { kind, line, col })
    }

    fn call_args(&mut self) -> PResult<Vec<Expr>> {
        self.expect(&Token::LParen, "« ( »")?;
        let mut args = Vec::new();
        if !self.at(&Token::RParen) {
            loop {
                args.push(self.expr()?);
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(&Token::RParen, "« ) »")?;
        Ok(args)
    }
}

/// Construit une expression binaire en héritant de la position de l'opérande gauche.
fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    let (line, col) = (lhs.line, lhs.col);
    Expr {
        kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
        line,
        col,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> Program {
        parse(src).unwrap_or_else(|e| panic!("parse échoué: {e}"))
    }

    #[test]
    fn golden_parse() {
        let src = include_str!("../../../examples/dopeuls.cdl");
        let prog = parse_ok(src);
        // serveur incarnam + bot tuer_dopeul + connexion
        assert_eq!(prog.items.len(), 3);
        assert!(matches!(prog.items[0], Item::Serveur(ref n) if n == "incarnam"));
        match &prog.items[1] {
            Item::Bot(b) => {
                assert_eq!(b.name, "tuer_dopeul");
                assert_eq!(b.cost_pa, Some(4));
                assert_eq!(b.cd, Some(2));
                assert_eq!(b.ret, Some(Type::Kamas));
            }
            _ => panic!("attendu un bot"),
        }
        assert!(matches!(prog.items[2], Item::Connexion(_)));
    }

    #[test]
    fn somme_parse() {
        let src = include_str!("../../../examples/somme.cdl");
        let prog = parse_ok(src);
        assert_eq!(prog.items.len(), 1);
    }

    #[test]
    fn precedence_arith() {
        // 1 + 2 * 3 => Add(1, Mul(2,3))
        let prog = parse_ok("// gg wp\nconnexion { loot x = 1 + 2 * 3 }");
        let Item::Connexion(b) = &prog.items[0] else {
            panic!()
        };
        let Stmt::Loot { value, .. } = &b.stmts[0] else {
            panic!()
        };
        match &value.kind {
            ExprKind::Binary(BinOp::Add, _l, r) => {
                assert!(matches!(r.kind, ExprKind::Binary(BinOp::Mul, _, _)));
            }
            other => panic!("attendu Add au sommet, eu {other:?}"),
        }
    }

    #[test]
    fn precedence_et_ou_comparaison() {
        // a < b et c => And(Lt(a,b), c)
        let prog = parse_ok("// gg wp\nconnexion { loot x = 1 < 2 et legit }");
        let Item::Connexion(b) = &prog.items[0] else {
            panic!()
        };
        let Stmt::Loot { value, .. } = &b.stmts[0] else {
            panic!()
        };
        assert!(matches!(value.kind, ExprKind::Binary(BinOp::And, _, _)));
    }

    #[test]
    fn afk_rand_est_afk_dun_appel() {
        // Déviation 7 : `afk rand(2000,5000)` = Afk(Call("rand", ...))
        let prog = parse_ok("// gg wp\nconnexion { tour { afk rand(2000, 5000) } }");
        let Item::Connexion(b) = &prog.items[0] else {
            panic!()
        };
        let Stmt::Tour(t) = &b.stmts[0] else { panic!() };
        match &t.stmts[0] {
            Stmt::Afk(e) => assert!(matches!(e.kind, ExprKind::Call(ref n, _) if n == "rand")),
            other => panic!("attendu Afk(Call), eu {other:?}"),
        }
    }

    #[test]
    fn pragma_en_tete() {
        let prog = parse_ok("// gg wp\n#max_pa 8\n#seed 42\nconnexion {}");
        assert_eq!(prog.pragmas.len(), 2);
        assert_eq!(prog.pragmas[0].key, "max_pa");
        assert_eq!(prog.pragmas[0].value, 8);
    }

    #[test]
    fn grind_bornes() {
        let prog = parse_ok("// gg wp\nconnexion { grind i de 0 a 10 {} }");
        let Item::Connexion(b) = &prog.items[0] else {
            panic!()
        };
        assert!(matches!(b.stmts[0], Stmt::Grind { .. }));
    }

    #[test]
    fn erreur_position() {
        // `connexion` sans bloc → erreur localisée
        let e = parse("// gg wp\nconnexion").unwrap_err();
        assert_eq!(e.line, 2);
    }

    #[test]
    fn erreur_brace_manquante() {
        let e = parse("// gg wp\nconnexion { loot x = 1").unwrap_err();
        assert!(e.msg.contains('}'));
    }
}
