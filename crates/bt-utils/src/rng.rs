//! Seeded randomness. Every random choice in better-tiled comes from a [`Rng`] made from a seed,
//! so a preview is what gets painted and a run can be reproduced. [`cell_rng`] keys a generator
//! by a position, so a result depends on where it is, not on the order cells are visited in.
//! [`RandomPicker`] is Tiled's weighted choice.

use glam::IVec2;

/// SplitMix64 (Steele, Lea and Flood, 2014): small, fast, and the same on every platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator whose sequence is fixed by `seed`.
    pub const fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    /// The next number of the sequence, advancing it.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `[0, 1)` with 53 bits of precision, advancing the sequence.
    pub fn next_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / (1u64 << 53) as f64;
        (self.next_u64() >> 11) as f64 * SCALE
    }

    /// A number in `[0, n)`, advancing the sequence, or `None` when `n` is 0.
    pub fn below(&mut self, n: u64) -> Option<u64> {
        if n == 0 {
            return None;
        }
        // Lemire's multiply-shift: the high half of a 128-bit product is below `n`.
        let wide = u128::from(self.next_u64()) * u128::from(n);
        u64::try_from(wide >> 64).ok()
    }
}

/// The generator for the cell at `position` under `seed`: the same for the same cell and seed
/// whatever else is computed in the same call, so a preview matches the paint and a growing
/// stroke does not flicker.
pub fn cell_rng(seed: u64, position: IVec2) -> Rng {
    let mut seeded = Rng::new(seed);
    // Each coordinate's bits, unchanged, in one half of the key.
    let key = (u64::from(position.x.cast_unsigned()) << 32) | u64::from(position.y.cast_unsigned());
    let mut keyed = Rng::new(seeded.next_u64() ^ key.wrapping_mul(0xD1B5_4A32_D192_ED03));
    Rng::new(keyed.next_u64())
}

/// A weighted random choice, with the semantics of Tiled's `RandomPicker`: values are kept with
/// cumulative thresholds, a draw is uniform in `[0, sum)`, and the first value whose threshold
/// is not below the draw is chosen. A value whose weight is not positive is never added.
#[derive(Clone, Debug)]
pub struct RandomPicker<T> {
    sum: f64,
    thresholds: Vec<Threshold<T>>,
}

#[derive(Clone, Debug)]
struct Threshold<T> {
    /// The sum of the weights up to and including this value's.
    cumulative: f64,
    value: T,
}

impl<T> Default for RandomPicker<T> {
    fn default() -> RandomPicker<T> {
        RandomPicker {
            sum: 0.0,
            thresholds: Vec::new(),
        }
    }
}

impl<T> RandomPicker<T> {
    /// An empty picker.
    pub fn new() -> RandomPicker<T> {
        RandomPicker::default()
    }

    /// Adds `value` with `weight`. A weight that is not positive (or not a number) adds nothing.
    pub fn add(&mut self, value: T, weight: f64) {
        if weight > 0.0 {
            self.sum += weight;
            match self.thresholds.last_mut() {
                // Like Qt's map that Tiled keeps them in, an equal threshold (a weight too small
                // to change a large sum) replaces the value before.
                Some(last) if last.cumulative == self.sum => last.value = value,
                _ => self.thresholds.push(Threshold {
                    cumulative: self.sum,
                    value,
                }),
            }
        }
    }

    /// Whether there is nothing to pick.
    pub fn is_empty(&self) -> bool {
        self.thresholds.is_empty()
    }

    /// How many values there are.
    pub fn len(&self) -> usize {
        self.thresholds.len()
    }

    /// Removes every value.
    pub fn clear(&mut self) {
        self.sum = 0.0;
        self.thresholds.clear();
    }

    /// A value chosen by weight, or `None` when there is none. A single value is returned without
    /// drawing, as Tiled does, so it leaves `rng` where it was.
    pub fn pick(&self, rng: &mut Rng) -> Option<&T> {
        if let [only] = self.thresholds.as_slice() {
            return Some(&only.value);
        }
        let index = self.draw(rng)?;
        self.thresholds.get(index).map(|t| &t.value)
    }

    /// Removes and returns a value chosen by weight, or `None` when there is none. As in Tiled,
    /// the sum is not lowered, so the removed value's weight goes to the value after it.
    pub fn take(&mut self, rng: &mut Rng) -> Option<T> {
        let index = self.draw(rng)?;
        (index < self.thresholds.len()).then(|| self.thresholds.remove(index).value)
    }

    /// The index a draw lands on: the first threshold not below it, or the last one.
    fn draw(&self, rng: &mut Rng) -> Option<usize> {
        let last = self.thresholds.len().checked_sub(1)?;
        let random = rng.next_f64() * self.sum;
        Some(
            self.thresholds
                .partition_point(|t| t.cumulative < random)
                .min(last),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_is_splitmix64() {
        let mut rng = Rng::new(1_234_567);
        assert_eq!(rng.next_u64(), 6_457_827_717_110_365_317);
        assert_eq!(rng.next_u64(), 3_203_168_211_198_807_973);
        assert_eq!(rng.next_u64(), 9_817_491_932_198_370_423);
        let f = Rng::new(9).next_f64();
        assert!((0.0..1.0).contains(&f));
    }

    #[test]
    fn below_stays_below_and_refuses_zero() {
        let mut rng = Rng::new(3);
        assert_eq!(rng.below(0), None);
        assert!((0..1000).all(|_| rng.below(7).is_some_and(|v| v < 7)));
        assert_eq!(Rng::new(5).below(1), Some(0));
    }

    #[test]
    fn cell_generators_depend_on_the_cell_and_the_seed_only() {
        let first = cell_rng(1, IVec2::new(3, 4)).next_u64();
        assert_eq!(first, cell_rng(1, IVec2::new(3, 4)).next_u64());
        assert_ne!(first, cell_rng(1, IVec2::new(4, 3)).next_u64());
        assert_ne!(first, cell_rng(2, IVec2::new(3, 4)).next_u64());
        assert_ne!(
            cell_rng(1, IVec2::new(-1, 0)).next_u64(),
            cell_rng(1, IVec2::new(0, -1)).next_u64()
        );
    }

    #[test]
    fn picks_follow_the_weights() {
        let mut picker = RandomPicker::new();
        picker.add('a', 1.0);
        picker.add('b', 3.0);
        picker.add('z', 0.0);
        picker.add('n', f64::NAN);
        assert_eq!(picker.len(), 2);
        let mut rng = Rng::new(7);
        let mut counts = [0u32; 2];
        for _ in 0..20_000 {
            match picker.pick(&mut rng) {
                Some('a') => counts[0] += 1,
                Some('b') => counts[1] += 1,
                other => panic!("unexpected {other:?}"),
            }
        }
        let ratio = f64::from(counts[1]) / f64::from(counts[0]);
        assert!((2.7..3.3).contains(&ratio), "ratio {ratio}");
    }

    #[test]
    fn a_single_value_is_picked_without_drawing() {
        let mut picker = RandomPicker::new();
        assert_eq!(picker.pick(&mut Rng::new(1)), None);
        picker.add(5, 0.25);
        let mut rng = Rng::new(3);
        assert_eq!(picker.pick(&mut rng), Some(&5));
        assert_eq!(rng, Rng::new(3));
    }

    #[test]
    fn taking_drains_every_value_once() {
        let mut picker = RandomPicker::new();
        for value in 0..5 {
            picker.add(value, 1.0);
        }
        let mut rng = Rng::new(11);
        let mut taken: Vec<i32> = std::iter::from_fn(|| picker.take(&mut rng)).collect();
        assert!(picker.is_empty());
        taken.sort_unstable();
        assert_eq!(taken, [0, 1, 2, 3, 4]);
        picker.add(1, 1.0);
        picker.clear();
        assert_eq!(picker.take(&mut rng), None);
    }
}
