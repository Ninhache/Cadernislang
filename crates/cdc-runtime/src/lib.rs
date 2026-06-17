//! Runtime de cadernislang : porte TOUTE la jouabilité (budget PA/PM, suspicion, cooldowns,
//! horloge, builtins), exposée en une seule source partagée interpréteur ↔ backend LLVM (§9.7).
//!
//! Phase 3 : moteur de suspicion (fenêtre glissante K, buckets, BAN) sur les **actions
//! observables** (bot/afk/up, Déviation 6) et cooldowns par `bot`.

pub mod cabi;
pub mod config;

pub use config::Config;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::{HashMap, HashSet, VecDeque};

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

/// Le compte a été banni : `suspicion >= SEUIL_BAN` (SPEC §1.2). Terminaison immédiate, code ≠ 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Banned;

/// Identité d'une action observable, à granularité **par type** (SPEC §1.2, Déviation 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ActionId {
    /// Un `bot` donné (par index stable).
    Bot(u32),
    /// N'importe quel `afk` (id partagé).
    Afk,
    /// N'importe quel `up` (id partagé).
    Up,
}

/// État d'exécution partagé. Toute la sémantique des mécaniques vit ici (invariant §9.7).
pub struct Runtime {
    cfg: Config,
    pa: i64,
    pm: i64,
    /// Numéro de tour courant (incrémenté par `passer`), base des cooldowns.
    turn: u64,
    rng: StdRng,

    // --- moteur de suspicion (SPEC §1.2) ---
    susp: u32,
    /// Fenêtre glissante des K derniers couples (id_action, bucket).
    window: VecDeque<(ActionId, u64)>,
    /// Délai virtuel accumulé (ms) depuis la dernière action consommatrice (bot/up).
    /// `afk` l'augmente ; bot/up le consomment (remise à 0). SPEC §1.2 (Déviation 9).
    acc_ms: u64,
    bot_ids: HashMap<String, u32>,
    next_bot_id: u32,
    /// Variables externes déjà payées (1 PM) ce tour — model B (SPEC §1.1.b, Déviation 8).
    /// Vidé à chaque `start_turn`. Partagé interp ↔ codegen via [`Runtime::pm_touch`] (§9.7).
    paid_pm: HashSet<u64>,

    // --- cooldowns (SPEC §1.3) ---
    cd_table: HashMap<String, u64>,
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
            turn: 0,
            rng,
            susp: 0,
            window: VecDeque::new(),
            acc_ms: 0,
            bot_ids: HashMap::new(),
            next_bot_id: 0,
            paid_pm: HashSet::new(),
            cd_table: HashMap::new(),
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
        self.susp
    }
    pub fn turn(&self) -> u64 {
        self.turn
    }
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    // ---- budget PA/PM (SPEC §1.1) ----

    /// Régénère le budget au plein (début de tour) et vide le cache PM du tour. SPEC §1.1.
    pub fn start_turn(&mut self) {
        self.pa = self.cfg.max_pa;
        self.pm = self.cfg.max_pm;
        self.paid_pm.clear();
    }

    /// Déplacement vers une variable externe (model B) : débite **1 PM la première fois** que
    /// `var_id` est touchée dans le tour courant ; gratuit ensuite. SPEC §1.1.b (Déviation 8).
    ///
    /// # Erreurs
    /// [`Fault::PmInsuffisant`] si le budget PM est épuisé.
    pub fn pm_touch(&mut self, var_id: u64) -> Result<(), Fault> {
        if self.paid_pm.insert(var_id) {
            self.spend_pm(1)?;
        }
        Ok(())
    }

    /// Fin de tour : avance le compteur de tours. SPEC §1.1 / §1.3.
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

    // ---- moteur de suspicion (SPEC §1.2, Déviations 4/6/9) ----

    fn bot_id(&mut self, name: &str) -> u32 {
        if let Some(id) = self.bot_ids.get(name) {
            return *id;
        }
        let id = self.next_bot_id;
        self.next_bot_id += 1;
        self.bot_ids.insert(name.to_string(), id);
        id
    }

    /// Enregistre une action observable et met à jour la suspicion (fenêtre glissante K).
    ///
    /// `reset` = l'action consomme le délai accumulé (bot/up) ; `afk` ne le consomme pas.
    fn record(&mut self, id: ActionId, reset: bool) -> Result<(), Banned> {
        let bucket = self.acc_ms / self.cfg.bucket_ms.max(1);
        let entry = (id, bucket);
        if self.window.contains(&entry) {
            self.susp = self.susp.saturating_add(self.cfg.penalite);
        } else {
            self.susp = self.susp.saturating_sub(self.cfg.decay);
        }
        self.window.push_back(entry);
        while self.window.len() > self.cfg.fenetre {
            self.window.pop_front();
        }
        if reset {
            self.acc_ms = 0;
        }
        if self.susp >= self.cfg.seuil_ban {
            Err(Banned)
        } else {
            Ok(())
        }
    }

    /// `afk <ms>` : avance l'horloge virtuelle (aucun sleep réel) et enregistre l'action.
    /// Le délai s'accumule (il alimentera aussi le bucket de la prochaine action). SPEC §1.2.
    ///
    /// # Erreurs
    /// [`Banned`] si la suspicion atteint le seuil.
    pub fn afk(&mut self, ms: i64) -> Result<(), Banned> {
        if ms > 0 {
            self.acc_ms = self.acc_ms.saturating_add(ms as u64);
        }
        self.record(ActionId::Afk, false)
    }

    /// `up <txt>` : affiche la chaîne puis enregistre l'action observable.
    ///
    /// # Erreurs
    /// [`Banned`] si la suspicion atteint le seuil.
    pub fn up(&mut self, msg: &str) -> Result<(), Banned> {
        println!("{msg}");
        self.record(ActionId::Up, true)
    }

    /// Enregistre l'appel d'un `bot` comme action observable (suspicion).
    ///
    /// # Erreurs
    /// [`Banned`] si la suspicion atteint le seuil.
    pub fn act_bot(&mut self, name: &str) -> Result<(), Banned> {
        let id = self.bot_id(name);
        self.record(ActionId::Bot(id), true)
    }

    // ---- cooldowns (SPEC §1.3) ----

    /// `cd_pret(bot) -> flag` : vrai si `bot` est appelable ce tour (perception, 0 PA).
    pub fn cd_pret(&self, name: &str) -> bool {
        self.turn >= self.cd_table.get(name).copied().unwrap_or(0)
    }

    /// Met `bot` en cooldown après un appel : rappelable au tour `turn + cd` (SPEC §1.3).
    pub fn set_cooldown(&mut self, name: &str, cd: i64) {
        if cd > 0 {
            self.cd_table
                .insert(name.to_string(), self.turn + cd as u64);
        }
    }

    // ---- builtins purs (SPEC §3) ----

    /// `rand(a, b) -> kamas` : entier aléatoire dans `[a, b]` (inclus). Bornes inversées tolérées.
    pub fn rand(&mut self, a: i64, b: i64) -> i64 {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        self.rng.gen_range(lo..=hi)
    }

    /// `butin(min, max) -> kamas` : stub RNG de butin (même loi que [`rand`]).
    pub fn butin(&mut self, min: i64, max: i64) -> i64 {
        self.rand(min, max)
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
        assert!(matches!(
            r.spend_pa(7).unwrap_err(),
            Fault::PaInsuffisant { .. }
        ));
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
    fn suspicion_fixe_bannit() {
        // afk 3000 fixe + kill alternés → buckets répétés → BAN (SPEC §9.2).
        let mut r = rt();
        let mut banned = false;
        for _ in 0..50 {
            if r.afk(3000).is_err() {
                banned = true;
                break;
            }
            if r.act_bot("f").is_err() {
                banned = true;
                break;
            }
        }
        assert!(banned, "afk fixe doit finir par bannir");
    }

    #[test]
    fn suspicion_variee_survit() {
        // afk rand(2000,5000) + kill → buckets dispersés → pas de ban sur une longue session.
        let mut r = rt();
        for _ in 0..2000 {
            let d = r.rand(2000, 5000);
            assert!(
                r.afk(d).is_ok(),
                "ne devrait pas bannir avec de la variabilité"
            );
            assert!(r.act_bot("f").is_ok());
        }
        assert!(r.suspicion() < r.config().seuil_ban);
    }

    #[test]
    fn cooldown_indisponible_puis_pret() {
        let mut r = rt();
        assert!(r.cd_pret("f"), "prêt au départ");
        r.set_cooldown("f", 2); // appelé au tour 0 → dispo tour 2
        assert!(!r.cd_pret("f")); // tour 0
        r.end_turn(); // tour 1
        assert!(!r.cd_pret("f"));
        r.end_turn(); // tour 2
        assert!(r.cd_pret("f"));
    }

    #[test]
    fn pragma_surcharge_config() {
        let mut c = Config::default();
        assert!(c.apply("max_pa", 8));
        assert_eq!(c.max_pa, 8);
        assert!(!c.apply("inconnu", 1));
    }
}
