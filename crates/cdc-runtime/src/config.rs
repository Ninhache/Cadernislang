//! Constantes de gameplay, centralisées et surchargeables par pragma (SPEC §5, §11).

/// Configuration runtime. Les valeurs par défaut sont les constantes normatives de la SPEC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Budget PA par tour (SPEC §1.1). Défaut 6.
    pub max_pa: i64,
    /// Budget PM par tour (SPEC §1.1). Défaut 3.
    pub max_pm: i64,
    /// Seuil de bannissement (SPEC §1.2). Défaut 80.
    pub seuil_ban: u32,
    /// Pénalité de suspicion sur collision (SPEC §1.2). Défaut 7.
    pub penalite: u32,
    /// Décroissance de suspicion hors collision (SPEC §1.2). Défaut 3.
    pub decay: u32,
    /// Taille de la fenêtre glissante d'actions observables (SPEC §1.2, Déviation 4). Défaut 8.
    pub fenetre: usize,
    /// Largeur d'un bucket de délai, en ms (SPEC §1.2, Déviation 9). Défaut 100 : `afk rand`
    /// doit disperser sur ≥ ~12 buckets pour que le golden survive (sinon dérive → ban).
    pub bucket_ms: u64,
    /// Coût PA d'une affectation (SPEC §1.1). Défaut 1.
    pub assign_pa: i64,
    /// Coût PA d'un `up` (SPEC §1.1). Défaut 1.
    pub up_pa: i64,
    /// Graine RNG. `None` ⇒ aléatoire (entropie système).
    pub seed: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_pa: 6,
            max_pm: 3,
            seuil_ban: 80,
            penalite: 7,
            decay: 3,
            fenetre: 8,
            bucket_ms: 25,
            assign_pa: 1,
            up_pa: 1,
            seed: None,
        }
    }
}

impl Config {
    /// Applique une surcharge pragma `#clé valeur` (SPEC §5).
    ///
    /// Retourne `false` si la clé est inconnue (le driver peut alors avertir).
    pub fn apply(&mut self, key: &str, value: i64) -> bool {
        match key {
            "max_pa" => self.max_pa = value,
            "max_pm" => self.max_pm = value,
            "seuil_ban" => self.seuil_ban = value as u32,
            "penalite" => self.penalite = value as u32,
            "decay" => self.decay = value as u32,
            "fenetre" => self.fenetre = value as usize,
            "bucket_ms" => self.bucket_ms = value as u64,
            "seed" => self.seed = Some(value as u64),
            _ => return false,
        }
        true
    }
}
