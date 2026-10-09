//! Measures of decoded images: where their visible pixels are.

use bt_utils::IRect;
use glam::{DVec2, IVec2};

use crate::decode::DecodedImage;

/// The smallest rectangle holding every pixel whose alpha is above `threshold`, or `None` when
/// no pixel's is.
pub fn alpha_bounds(image: &DecodedImage, threshold: u8) -> Option<IRect> {
    IRect::bounding(visible_pixels(image, threshold))
}

/// Where a sprite's feet are: the middle of the lowest row of pixels whose alpha is above
/// `threshold`, on that row's bottom edge, in pixels from the image's top-left. `None` when no
/// pixel is visible.
pub fn bottom_center(image: &DecodedImage, threshold: u8) -> Option<DVec2> {
    let lowest = visible_pixels(image, threshold).map(|p| p.y).max()?;
    let row = visible_pixels(image, threshold).filter(|p| p.y == lowest);
    let (left, right) = row.fold((i32::MAX, i32::MIN), |(l, r), p| (l.min(p.x), r.max(p.x)));
    Some(DVec2::new(
        f64::from(left + right + 1) / 2.0,
        f64::from(lowest + 1),
    ))
}

/// The positions of the pixels whose alpha is above `threshold`.
fn visible_pixels(image: &DecodedImage, threshold: u8) -> impl Iterator<Item = IVec2> + '_ {
    let width = usize::try_from(image.width).unwrap_or(0).max(1);
    image
        .rgba
        .chunks_exact(4)
        .enumerate()
        .filter(move |(_, p)| p.get(3).is_some_and(|&a| a > threshold))
        .filter_map(move |(i, _)| {
            let x = i32::try_from(i % width).ok()?;
            let y = i32::try_from(i / width).ok()?;
            Some(IVec2::new(x, y))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16×16 tree: a wide crown, and a two-pixel trunk at x 5–6 ending two pixels above the
    /// bottom, with a faint shadow below it.
    fn tree() -> DecodedImage {
        let mut rgba = vec![0u8; 16 * 16 * 4];
        let mut set = |x: usize, y: usize, a: u8| rgba[(y * 16 + x) * 4 + 3] = a;
        for y in 2..9 {
            for x in 1..15 {
                set(x, y, 255);
            }
        }
        for y in 9..14 {
            for x in 5..7 {
                set(x, y, 255);
            }
        }
        for x in 2..12 {
            set(x, 15, 60);
        }
        DecodedImage {
            width: 16,
            height: 16,
            rgba,
        }
    }

    #[test]
    fn bounds_and_feet_skip_faint_pixels() {
        let image = tree();
        assert_eq!(
            alpha_bounds(&image, 128),
            Some(IRect {
                x: 1,
                y: 2,
                w: 14,
                h: 12
            })
        );
        assert_eq!(
            alpha_bounds(&image, 0),
            Some(IRect {
                x: 1,
                y: 2,
                w: 14,
                h: 14
            }),
            "the shadow counts above zero"
        );
        assert_eq!(
            bottom_center(&image, 128),
            Some(DVec2::new(6.0, 14.0)),
            "the middle of the trunk's lowest row"
        );
        let empty = DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
        };
        assert_eq!(
            (alpha_bounds(&empty, 0), bottom_center(&empty, 0)),
            (None, None)
        );
    }
}
