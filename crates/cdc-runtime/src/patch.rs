//! Dérive de patch (SPEC §1.4, Phase 6) : attribution des **tags** de variants `pano`.
//!
//! Les tags non épinglés sont permutés par un *seed de patch* — simulant la renumérotation
//! interne d'une MAJ Dofus. Les variants épinglés (`@N`) gardent leur tag. Logique unique,
//! partagée par l'interpréteur et le backend natif (invariant §9.7).

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

/// Calcule le tag de chaque variant à partir de ses épinglages et du seed de patch.
///
/// `pins[i]` = `Some(n)` si le variant *i* est épinglé `@n`, sinon `None`. Les variants non
/// épinglés reçoivent les tags libres (0..n privés des tags épinglés), permutés par `seed`.
///
/// # Erreurs
/// Si un tag épinglé est hors de `0..n` ou dupliqué (collision).
pub fn layout(pins: &[Option<i64>], seed: u64) -> Result<Vec<i64>, String> {
    let n = pins.len() as i64;
    let mut used = vec![false; pins.len()];
    for (i, p) in pins.iter().enumerate() {
        if let Some(tag) = p {
            if *tag < 0 || *tag >= n {
                return Err(format!(
                    "tag épinglé @{tag} hors plage 0..{} (variant #{i})",
                    pins.len()
                ));
            }
            let t = *tag as usize;
            if used[t] {
                return Err(format!("tag épinglé @{tag} en double"));
            }
            used[t] = true;
        }
    }
    // tags libres, permutés par le seed
    let mut free: Vec<i64> = (0..n).filter(|t| !used[*t as usize]).collect();
    free.shuffle(&mut StdRng::seed_from_u64(seed));

    let mut tags = Vec::with_capacity(pins.len());
    let mut next_free = 0usize;
    for p in pins {
        match p {
            Some(tag) => tags.push(*tag),
            None => {
                tags.push(free[next_free]);
                next_free += 1;
            }
        }
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epingle_stable_libre_derive() {
        // 3 variants, le #2 épinglé @2. Les #0/#1 (libres) doivent dériver selon le seed.
        let pins = [None, None, Some(2)];
        let a = layout(&pins, 1).unwrap();
        let b = layout(&pins, 999).unwrap();
        assert_eq!(a[2], 2, "variant épinglé stable");
        assert_eq!(b[2], 2);
        // une permutation existe où les libres diffèrent entre deux seeds
        let mut differ = false;
        for s in 0..50 {
            if layout(&pins, s).unwrap()[0] != a[0] {
                differ = true;
                break;
            }
        }
        assert!(differ, "les tags libres doivent dériver selon le seed");
    }

    #[test]
    fn tags_sont_une_permutation() {
        let pins = [None, Some(0), None, None];
        let mut t = layout(&pins, 7).unwrap();
        t.sort();
        assert_eq!(t, vec![0, 1, 2, 3]);
    }

    #[test]
    fn epingle_hors_plage_erreur() {
        assert!(layout(&[Some(5), None], 0).is_err());
    }

    #[test]
    fn epingle_double_erreur() {
        assert!(layout(&[Some(0), Some(0)], 0).is_err());
    }
}
