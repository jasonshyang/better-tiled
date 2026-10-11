//! The random choices operations make with their generator (`10-agents.md`, §8.1, §8.4): one of
//! several names, a shape's cells in random order, how many a `scatter` wants, and the note when
//! it runs out of room.

use bt_utils::rng::Rng;
use glam::IVec2;

use crate::script::Amount;
use crate::words::number_text;

/// One of `items`, at random.
pub(super) fn pick<T: Clone>(items: &[T], rng: &mut Rng) -> Option<T> {
    let count = u64::try_from(items.len()).ok()?;
    let index = usize::try_from(rng.below(count)?).ok()?;
    items.get(index).cloned()
}

/// Shuffles `cells` with `rng` (Fisher and Yates).
pub(super) fn shuffle(cells: &mut [IVec2], rng: &mut Rng) {
    for i in (1..cells.len()).rev() {
        let bound = u64::try_from(i + 1).unwrap_or(u64::MAX);
        if let Some(j) = rng.below(bound).and_then(|j| usize::try_from(j).ok()) {
            cells.swap(i, j);
        }
    }
}

/// How many `scatter` places: the count, or the density times the cells, rounded.
pub(super) fn wanted_of(amount: Amount, cells: usize) -> usize {
    match amount {
        Amount::Count(count) => usize::try_from(count).unwrap_or(usize::MAX),
        Amount::Density(density) => {
            let wanted = (density * cells as f64).round();
            // A density is between 0 and 1, so this is at most the number of cells.
            if wanted > 0.0 { wanted as usize } else { 0 }
        }
    }
}

/// The note for a scatter that ran out of room: `scatter placed 3 of 10 (no room left 2 cells
/// apart)`.
pub(super) fn short_of_room(verb: &str, placed: usize, wanted: usize, spacing: f64) -> String {
    let apart = if spacing > 0.0 {
        format!(" {} cells apart", number_text(spacing))
    } else {
        String::new()
    };
    format!("{verb} placed {placed} of {wanted} (no room left{apart})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_follow_the_generator_and_counts_round() {
        let mut cells: Vec<IVec2> = (0..10).map(|x| IVec2::new(x, 0)).collect();
        let mut again = cells.clone();
        shuffle(&mut cells, &mut Rng::new(3));
        shuffle(&mut again, &mut Rng::new(3));
        assert_eq!(cells, again, "the same generator, the same order");
        let mut sorted = cells.clone();
        sorted.sort_by_key(|c| c.x);
        assert_eq!(sorted.len(), 10);
        assert_eq!(wanted_of(Amount::Density(0.15), 100), 15);
        assert_eq!(wanted_of(Amount::Density(0.0), 100), 0);
        assert_eq!(wanted_of(Amount::Count(7), 3), 7);
        assert_eq!(pick::<u8>(&[], &mut Rng::new(1)), None);
        assert_eq!(
            short_of_room("scatter", 3, 10, 2.5),
            "scatter placed 3 of 10 (no room left 2.5 cells apart)"
        );
    }
}
