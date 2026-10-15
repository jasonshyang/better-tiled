//! Animated tiles (docs/design/12-godot-export.md). Godot steps an atlas tile's animation through cells of its
//! atlas laid out from the tile's own cell: along its row, or in rows of `animation_columns`
//! frames, with `animation_separation` cells between frames. So an animated tile goes in an atlas
//! source of its own on the same texture, at its first frame's cell, and its frames stay plain
//! tiles of the tileset's source. Animations whose cells overlap go in further sources. An image
//! collection's animation goes in its tile's own source when its frames are evenly spaced parts
//! of one image.

use std::collections::HashSet;

use bt_core::{Frame, Tileset};
use glam::IVec2;

use super::text;

/// How Godot finds an animation's frames from its first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Layout {
    /// Frames per row before the next row starts; 0 for one row however long.
    pub columns: i32,
    /// Cells skipped between one frame and the next, across and down.
    pub separation: IVec2,
}

impl Layout {
    /// The cell of frame `i` of an animation whose first frame is at `head`.
    pub fn cell(&self, head: IVec2, i: usize) -> IVec2 {
        let i = i as i32;
        let place = if self.columns > 0 {
            IVec2::new(i % self.columns, i / self.columns)
        } else {
            IVec2::new(i, 0)
        };
        head + (IVec2::ONE + self.separation) * place
    }
}

/// An animated tile of an animation source.
pub(super) struct Animation {
    pub layout: Layout,
    /// Each frame's duration, in seconds.
    pub durations: Vec<f64>,
}

impl Animation {
    pub fn new(layout: Layout, frames: &[Frame]) -> Animation {
        // Godot refuses frames of no duration; Tiled never shows them anyway.
        Animation {
            layout,
            durations: frames
                .iter()
                .map(|f| f.duration_ms.max(1) as f64 / 1000.0)
                .collect(),
        }
    }
}

/// The layout putting frame i at `cells[i]`, if Godot has one: a fixed stride along a row or down
/// a column, or rows of frames (a run that wraps whole rows of the atlas, a block). The simplest
/// that fits wins.
pub(super) fn solve(cells: &[IVec2]) -> Option<Layout> {
    let head = *cells.first()?;
    let Some(&second) = cells.get(1) else {
        return Some(Layout::default());
    };
    let across = second - head;
    let mut candidates = Vec::new();
    if across.y == 0 && across.x > 0 {
        candidates.push(Layout {
            columns: 0,
            separation: IVec2::new(across.x - 1, 0),
        });
    }
    if across.x == 0 && across.y > 0 {
        candidates.push(Layout {
            columns: 1,
            separation: IVec2::new(0, across.y - 1),
        });
    }
    if across.y == 0 && across.x > 0 {
        for (columns, &cell) in cells.iter().enumerate().skip(2) {
            let down = cell - head;
            if down.x == 0 && down.y > 0 {
                candidates.push(Layout {
                    columns: columns as i32,
                    separation: IVec2::new(across.x - 1, down.y - 1),
                });
            }
        }
    }
    candidates.into_iter().find(|layout| {
        cells
            .iter()
            .enumerate()
            .all(|(i, &cell)| layout.cell(head, i) == cell)
    })
}

/// An image collection's animation as frames of one size evenly spaced in one image, if it is:
/// the pixels between frames, and the layout of the frames from the first.
pub(super) fn strip(ts: &Tileset, frames: &[Frame]) -> Option<(IVec2, Layout)> {
    let rects = frames
        .iter()
        .map(|f| {
            ts.tile_image(f.tile)
                .map(|tile| (tile.image.path(), tile.rect))
        })
        .collect::<Option<Vec<_>>>()?;
    let (image, head) = *rects.first()?;
    let size = IVec2::new(head.w, head.h);
    if size.x <= 0
        || size.y <= 0
        || rects
            .iter()
            .any(|(i, r)| *i != image || r.w != size.x || r.h != size.y)
    {
        return None;
    }
    let offsets: Vec<IVec2> = rects
        .iter()
        .map(|(_, r)| IVec2::new(r.x - head.x, r.y - head.y))
        .collect();
    // From one frame to the next, across and down: the nearest frame that way.
    let nearest = |d: fn(IVec2) -> i32, size: i32| {
        offsets
            .iter()
            .map(|&o| d(o))
            .filter(|&d| d > 0)
            .min()
            .unwrap_or(size)
    };
    let pitch = IVec2::new(nearest(|o| o.x, size.x), nearest(|o| o.y, size.y));
    let even = |o: &IVec2| o.x >= 0 && o.y >= 0 && o.x % pitch.x == 0 && o.y % pitch.y == 0;
    if pitch.x < size.x || pitch.y < size.y || !offsets.iter().all(even) {
        return None;
    }
    let cells: Vec<IVec2> = offsets.iter().map(|&o| o / pitch).collect();
    Some((pitch - size, solve(&cells)?))
}

/// Packs animations, each given by the cells it covers, into sources where no two share a cell:
/// each goes in the source `was` gives it if it has room there (so a shared TileSet's animated
/// tiles stay where scenes exported before have them), the others in the first that has room.
/// Returns each one's source, counted from 0; a source `was` names may be left empty.
pub(super) fn pack(animations: &[Vec<IVec2>], was: &[Option<usize>]) -> Vec<usize> {
    let mut taken: Vec<HashSet<IVec2>> = Vec::new();
    let fits = |taken: &HashSet<IVec2>, cells: &[IVec2]| cells.iter().all(|c| !taken.contains(c));
    let mut sources: Vec<Option<usize>> = vec![None; animations.len()];
    for (i, cells) in animations.iter().enumerate() {
        let Some(n) = was.get(i).copied().flatten() else {
            continue;
        };
        if taken.len() <= n {
            taken.resize_with(n + 1, HashSet::new);
        }
        if taken.get(n).is_some_and(|t| fits(t, cells)) {
            if let Some(taken) = taken.get_mut(n) {
                taken.extend(cells.iter().copied());
            }
            if let Some(source) = sources.get_mut(i) {
                *source = Some(n);
            }
        }
    }
    for (i, cells) in animations.iter().enumerate() {
        if sources.get(i).is_some_and(Option::is_some) {
            continue;
        }
        let n = taken
            .iter()
            .position(|t| fits(t, cells))
            .unwrap_or_else(|| {
                taken.push(HashSet::new());
                taken.len() - 1
            });
        if let Some(taken) = taken.get_mut(n) {
            taken.extend(cells.iter().copied());
        }
        if let Some(source) = sources.get_mut(i) {
            *source = Some(n);
        }
    }
    sources.into_iter().map(|n| n.unwrap_or_default()).collect()
}

/// An animated tile's lines under its `x:y/`: the layout, then the frames' durations. Godot
/// applies each frame as it reads it, so a frame read before the separation would claim the next
/// cell, which may be another animation's. Godot works out the frame count from the durations.
pub(super) fn lines(animation: &Animation) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    if animation.layout.columns != 0 {
        lines.push((
            "animation_columns".to_owned(),
            animation.layout.columns.to_string(),
        ));
    }
    if animation.layout.separation != IVec2::ZERO {
        lines.push((
            "animation_separation".to_owned(),
            text::vector2i(animation.layout.separation),
        ));
    }
    if animation.durations.len() > 1 {
        for (i, seconds) in animation.durations.iter().enumerate() {
            lines.push((
                format!("animation_frame_{i}/duration"),
                text::float(*seconds),
            ));
        }
    }
    lines
}
