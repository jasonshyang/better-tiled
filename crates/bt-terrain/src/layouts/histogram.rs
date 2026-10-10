//! Colour histograms: how a terrain looks, in a form that tells terrains apart where their mean
//! colours do not (dirt and rooty dirt have nearly the same mean, not the same colours).

use std::collections::BTreeMap;

/// The key of see-through pixels (alpha under one half), which count as one colour.
const SEE_THROUGH: u16 = u16::MAX;

/// How often each colour shows in some pixels, the colours cut to 4 bits a channel; see-through
/// pixels all count as one colour.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Histogram {
    counts: BTreeMap<u16, u32>,
}

impl Histogram {
    /// The histogram of straight-alpha RGBA pixels.
    pub(crate) fn of_pixels(pixels: impl IntoIterator<Item = [u8; 4]>) -> Histogram {
        let mut counts: BTreeMap<u16, u32> = BTreeMap::new();
        for [r, g, b, a] in pixels {
            let key = if a < 128 {
                SEE_THROUGH
            } else {
                (u16::from(r >> 4) << 8) | (u16::from(g >> 4) << 4) | u16::from(b >> 4)
            };
            *counts.entry(key).or_default() += 1;
        }
        Histogram { counts }
    }

    /// Adds the counts of `other` to these.
    pub(crate) fn add(&mut self, other: &Histogram) {
        for (&key, &count) in &other.counts {
            *self.counts.entry(key).or_default() += count;
        }
    }

    /// How many pixels were counted.
    pub(crate) fn pixel_count(&self) -> u64 {
        self.counts.values().map(|&n| u64::from(n)).sum()
    }

    /// How much two histograms overlap: the sum over colours of the smaller of the two shares,
    /// from 0 (no colour in common, or nothing counted) to 1 (the same colours, as often).
    pub fn overlap(&self, other: &Histogram) -> f64 {
        let (total, other_total) = (self.pixel_count(), other.pixel_count());
        if total == 0 || other_total == 0 {
            return 0.0;
        }
        self.counts
            .iter()
            .map(|(key, &count)| {
                let share = f64::from(count) / total as f64;
                let other_share = other
                    .counts
                    .get(key)
                    .map_or(0.0, |&n| f64::from(n) / other_total as f64);
                share.min(other_share)
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_count_at_four_bits_a_channel() {
        // These two greens differ only below the top four bits of each channel.
        let a = Histogram::of_pixels([[60, 170, 60, 255], [61, 175, 62, 255], [0, 0, 0, 0]]);
        let b = Histogram::of_pixels([[63, 168, 50, 255], [62, 171, 55, 255], [9, 9, 9, 100]]);
        assert_eq!(a.pixel_count(), 3);
        assert!((a.overlap(&b) - 1.0).abs() < 1e-9, "{}", a.overlap(&b));
        let red = Histogram::of_pixels([[200, 0, 0, 255]]);
        assert_eq!(a.overlap(&red), 0.0);
        assert_eq!(a.overlap(&Histogram::default()), 0.0, "nothing counted");
        let mut both = red.clone();
        both.add(&Histogram::of_pixels([[60, 170, 60, 255]]));
        assert!((both.overlap(&red) - 0.5).abs() < 1e-9);
    }
}
