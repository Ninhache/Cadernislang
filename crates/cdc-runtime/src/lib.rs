//! Runtime de cadernislang : porte TOUTE la jouabilité (budget PA/PM, horloge, builtins),
//! exposée en une seule source partagée interpréteur ↔ backend LLVM (SPEC §9.7).
//!
//! Phase 2 : budget PA/PM (compteurs, régénération, faute de dépassement), horloge virtuelle,
//! builtins `up`/`afk`/`rand`/`butin`. La suspicion (toujours 0 ici) et les cooldowns
//! (`cd_pret` ⇒ toujours prêt) arrivent en Phase 3.

pub mod config;

pub use config::Config;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Faute runtime : dépassement de budget dans un `tour` (SPEC §1.1, §12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Plus assez de PA pour l'action demandée.
    PaInsuffisant { demande: i64, restant: i64 },
    /// Plus assez de PM pour le déplacement vers la donnée.
    PmInsuffisant { demande: i64, restant: i64 },
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::PaInsuffisant { demande, restant } => {
                write!(
                    f,
                    "PaInsuffisant — {demande} PA demandés, {restant} restant(s)"
                )
            }
            Fault::PmInsuffisant { demande, restant } => {
                write!(
                    f,
                    "PmInsuffisant — {demande} PM demandés, {restant} restant(s)"
                )
            }
        }
    }
}

/// État d'exécution partagé. Toute la sémantique des mécaniques vit ici (invariant §9.7).
pub struct Runtime {
    cfg: Config,
    pa: i64,
    pm: i64,
    suspicion: u32,
    /// Horloge virtuelle en ms, alimentée par `afk` (SPEC §1.2, Déviation 3).
    clock_ms: u64,
    /// Numéro de tour courant (incrémenté par `passer`), pour les cooldowns (Phase 3).
    turn: u64,
    rng: StdRng,
}

impl Runtime {
    /// Crée un runtime ; le budget démarre plein. RNG seedé si `cfg.seed` est défini.
    pub fn new(cfg: Config) -> Self {
        let rng = match cfg.seed {
            Some(s) => StdRng::seed_from_u64(s),
            None => StdRng::from_entropy(),
        };
        Runtime {
            pa: cfg.max_pa,
            pm: cfg.max_pm,
            suspicion: 0,
            clock_ms: 0,
            turn: 0,
            rng,
            cfg,
        }
    }

    // ---- perception (lecture seule, coût 0) ----

    pub fn pa(&self) -> i64 {
        self.pa
    }
    pub fn pm(&self) -> i64 {
        self.pm
    }
    pub fn suspicion(&self) -> u32 {
        self.suspicion
    }
    pub fn turn(&self) -> u64 {
        self.turn
    }
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    // ---- budget PA/PM (SPEC §1.1) ----

    /// Régénère le budget au plein (début de tour). SPEC §1.1.
    pub fn start_turn(&mut self) {
        self.pa = self.cfg.max_pa;
        self.pm = self.cfg.max_pm;
    }

    /// Fin de tour : avance le compteur de tours (régénération via `start_turn`). SPEC §1.1.
    pub fn end_turn(&mut self) {
        self.turn += 1;
    }

    /// Débite `n` PA. SPEC §1.1.
    ///
    /// # Erreurs
    /// [`Fault::PaInsuffisant`] si le budget est insuffisant.
    pub fn spend_pa(&mut self, n: i64) -> Result<(), Fault> {
        if n > self.pa {
            return Err(Fault::PaInsuffisant {
                demande: n,
                restant: self.pa,
            });
        }
        self.pa -= n;
        Ok(())
    }

    /// Débite `n` PM (déplacement vers la donnée). SPEC §1.1.b.
    ///
    /// # Erreurs
    /// [`Fault::PmInsuffisant`] si le budget est insuffisant.
    pub fn spend_pm(&mut self, n: i64) -> Result<(), Fault> {
        if n > self.pm {
            return Err(Fault::PmInsuffisant {
                demande: n,
                restant: self.pm,
            });
        }
        self.pm -= n;
        Ok(())
    }

    /// Coût PA d'une affectation (constante de config).
    pub fn assign_pa(&self) -> i64 {
        self.cfg.assign_pa
    }
    /// Coût PA d'un `up` (constante de config).
    pub fn up_pa(&self) -> i64 {
        self.cfg.up_pa
    }

    // ---- builtins (SPEC §3) ----

    /// `up <txt>` : action observable, affiche la chaîne. (Suspicion branchée en Phase 3.)
    pub fn up(&mut self, msg: &str) {
        println!("{msg}");
    }

    /// `afk <ms>` : avance l'horloge virtuelle (aucun sleep réel). SPEC §1.2 (Déviation 3).
    pub fn afk(&mut self, ms: i64) {
        if ms > 0 {
            self.clock_ms = self.clock_ms.saturating_add(ms as u64);
        }
    }

    /// `rand(a, b) -> kamas` : entier aléatoire dans `[a, b]` (inclus). Bornes inversées tolérées.
    pub fn rand(&mut self, a: i64, b: i64) -> i64 {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        self.rng.gen_range(lo..=hi)
    }

    /// `butin(min, max) -> kamas` : stub RNG de butin (même loi que [`rand`]).
    pub fn butin(&mut self, min: i64, max: i64) -> i64 {
        self.rand(min, max)
    }

    /// `cd_pret(bot) -> flag` : Phase 2 ⇒ toujours prêt (cooldowns en Phase 3, issue #15).
    pub fn cd_pret(&self, _bot: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> Runtime {
        Runtime::new(Config {
            seed: Some(1),
            ..Config::default()
        })
    }

    #[test]
    fn budget_initial_plein() {
        let r = rt();
        assert_eq!(r.pa(), 6);
        assert_eq!(r.pm(), 3);
    }

    #[test]
    fn depense_et_regen() {
        let mut r = rt();
        r.spend_pa(4).unwrap();
        assert_eq!(r.pa(), 2);
        r.start_turn();
        assert_eq!(r.pa(), 6);
    }

    #[test]
    fn pa_insuffisant() {
        let mut r = rt();
        let e = r.spend_pa(7).unwrap_err();
        assert!(matches!(e, Fault::PaInsuffisant { .. }));
    }

    #[test]
    fn rand_seedable_deterministe() {
        let mut a = Runtime::new(Config {
            seed: Some(42),
            ..Config::default()
        });
        let mut b = Runtime::new(Config {
            seed: Some(42),
            ..Config::default()
        });
        assert_eq!(a.rand(0, 1000), b.rand(0, 1000));
    }

    #[test]
    fn rand_dans_bornes() {
        let mut r = rt();
        for _ in 0..100 {
            let v = r.rand(50, 200);
            assert!((50..=200).contains(&v));
        }
    }

    #[test]
    fn pragma_surcharge_config() {
        let mut c = Config::default();
        assert!(c.apply("max_pa", 8));
        assert_eq!(c.max_pa, 8);
        assert!(!c.apply("inconnu", 1));
    }
}
