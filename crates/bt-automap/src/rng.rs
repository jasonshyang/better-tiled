//! The randomness of a run: a pure function of the seed, the rule, the position and what the
//! number is for. Tiled draws from an unseeded generator in the order it scans, so its results
//! cannot be repeated; here the same seed gives the same map whatever order positions are tried
//! in, which is what lets a run be repeated, previewed and, later, recomputed in part.

use bt_utils::rng::Rng;
use glam::IVec2;

use crate::rule_ref::RuleRef;

/// What a random number decides, so two decisions at one position are independent.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// Whether a rule whose `Probability` is below 1 is skipped at a position.
    Skip,
    /// Which of a rule's random outputs is applied at a position.
    Output,
}

impl Purpose {
    fn key(self) -> u64 {
        match self {
            Purpose::Skip => 0x5EED_0001,
            Purpose::Output => 0x5EED_0002,
        }
    }
}

/// The SplitMix64 finalizer: every bit of the result depends on every bit of `z`.
fn mix(z: u64) -> u64 {
    let z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The 64 random bits of one decision.
pub(crate) fn bits(seed: u64, rule: RuleRef, position: IVec2, purpose: Purpose) -> u64 {
    // usize is at most 64 bits on every platform Rust supports.
    let rule_map = rule.rule_map as u64;
    let rule_index = rule.rule as u64;
    let cell =
        (u64::from(position.x.cast_unsigned()) << 32) | u64::from(position.y.cast_unsigned());
    let mut h = mix(seed);
    h = mix(h ^ rule_map.wrapping_mul(0xD1B5_4A32_D192_ED03));
    h = mix(h ^ rule_index.wrapping_mul(0xABC9_8388_FB8F_AC03));
    h = mix(h ^ cell);
    mix(h ^ purpose.key())
}

/// A number in `[0, 1)` for one decision, with 53 bits of precision.
pub fn unit(seed: u64, rule: RuleRef, position: IVec2, purpose: Purpose) -> f64 {
    const SCALE: f64 = 1.0 / (1u64 << 53) as f64;
    (bits(seed, rule, position, purpose) >> 11) as f64 * SCALE
}

/// A generator for a decision that draws through `bt_utils`' `RandomPicker`, so a weighted
/// choice is Tiled's.
pub(crate) fn generator(seed: u64, rule: RuleRef, position: IVec2, purpose: Purpose) -> Rng {
    Rng::new(bits(seed, rule, position, purpose))
}

#[cfg(test)]
mod tests {
    use bt_utils::rng::RandomPicker;

    use super::*;

    fn rule(rule_map: usize, rule: usize) -> RuleRef {
        RuleRef { rule_map, rule }
    }

    #[test]
    fn the_same_decision_gives_the_same_number() {
        let a = unit(7, rule(1, 2), IVec2::new(3, -4), Purpose::Skip);
        let b = unit(7, rule(1, 2), IVec2::new(3, -4), Purpose::Skip);
        assert_eq!(a, b);
        assert!((0.0..1.0).contains(&a));
    }

    #[test]
    fn every_part_of_a_decision_changes_the_number() {
        let base = unit(7, rule(1, 2), IVec2::new(3, -4), Purpose::Skip);
        for other in [
            unit(8, rule(1, 2), IVec2::new(3, -4), Purpose::Skip),
            unit(7, rule(0, 2), IVec2::new(3, -4), Purpose::Skip),
            unit(7, rule(1, 3), IVec2::new(3, -4), Purpose::Skip),
            unit(7, rule(1, 2), IVec2::new(4, -4), Purpose::Skip),
            unit(7, rule(1, 2), IVec2::new(3, -3), Purpose::Skip),
            unit(7, rule(1, 2), IVec2::new(-4, 3), Purpose::Skip),
            unit(7, rule(1, 2), IVec2::new(3, -4), Purpose::Output),
        ] {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn numbers_are_spread_evenly() {
        // A coarse check of uniformity over a grid of positions: each tenth of [0, 1) gets
        // its share within a few percent.
        let mut buckets = [0u32; 10];
        for y in 0..100 {
            for x in 0..100 {
                let u = unit(42, rule(0, 0), IVec2::new(x, y), Purpose::Skip);
                buckets[(u * 10.0) as usize] += 1;
            }
        }
        for count in buckets {
            assert!((900..1100).contains(&count), "{buckets:?}");
        }
    }

    #[test]
    fn weighted_choices_follow_their_weights() {
        let mut picker = RandomPicker::new();
        picker.add(0usize, 3.0);
        picker.add(1usize, 1.0);
        let mut counts = [0u32; 2];
        for x in 0..4000 {
            let mut rng = generator(1, rule(0, 0), IVec2::new(x, 0), Purpose::Output);
            counts[*picker.pick(&mut rng).unwrap()] += 1;
        }
        assert!((2850..3150).contains(&counts[0]), "{counts:?}");
    }
}
