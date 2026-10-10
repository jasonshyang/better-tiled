//! Reading parts of a tileset image: the pixels of a rectangle and their mean colour.

use bt_image::DecodedImage;
use bt_utils::IRect;

/// A mean colour: red, green and blue premultiplied by alpha, then alpha, each from 0 to 1. A
/// see-through region's mean is near zero in every channel.
pub(crate) type MeanColor = [f64; 4];

/// The pixels of `rect` that lie inside `image`, row by row.
pub(crate) fn region_pixels(image: &DecodedImage, rect: IRect) -> impl Iterator<Item = [u8; 4]> {
    let width = i32::try_from(image.width).unwrap_or(i32::MAX);
    let height = i32::try_from(image.height).unwrap_or(i32::MAX);
    let left = rect.x.max(0);
    let right = rect.x.saturating_add(rect.w).min(width);
    let top = rect.y.max(0);
    let bottom = rect.y.saturating_add(rect.h).min(height);
    (top..bottom).flat_map(move |y| {
        (left..right)
            .filter_map(move |x| image.pixel(u32::try_from(x).ok()?, u32::try_from(y).ok()?))
    })
}

/// The mean colour of the pixels of `rect` inside `image`; see-through when none are.
pub(crate) fn mean_color(image: &DecodedImage, rect: IRect) -> MeanColor {
    let mut sum = [0.0; 4];
    let mut count = 0_u32;
    for [r, g, b, a] in region_pixels(image, rect) {
        let alpha = f64::from(a) / 255.0;
        for (total, channel) in sum.iter_mut().zip([r, g, b]) {
            *total += f64::from(channel) / 255.0 * alpha;
        }
        if let Some(total) = sum.get_mut(3) {
            *total += alpha;
        }
        count += 1;
    }
    if count == 0 {
        return [0.0; 4];
    }
    sum.map(|total| total / f64::from(count))
}

/// The squared distance between two mean colours.
pub(crate) fn distance(a: &MeanColor, b: &MeanColor) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4×2 image: the left half opaque red, the right half see-through.
    fn image() -> DecodedImage {
        let mut rgba = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                rgba.extend(if x < 2 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 0, 0]
                });
            }
        }
        DecodedImage {
            width: 4,
            height: 2,
            rgba,
        }
    }

    #[test]
    fn means_are_premultiplied_and_clipped_to_the_image() {
        let image = image();
        assert_eq!(
            mean_color(
                &image,
                IRect {
                    x: 0,
                    y: 0,
                    w: 2,
                    h: 2
                }
            ),
            [1.0, 0.0, 0.0, 1.0]
        );
        assert_eq!(
            mean_color(
                &image,
                IRect {
                    x: 0,
                    y: 0,
                    w: 4,
                    h: 2
                }
            ),
            [0.5, 0.0, 0.0, 0.5]
        );
        // Past the right edge only the pixels inside count: they do not wrap to the next row.
        assert_eq!(
            mean_color(
                &image,
                IRect {
                    x: 3,
                    y: 0,
                    w: 5,
                    h: 1
                }
            ),
            [0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            region_pixels(
                &image,
                IRect {
                    x: -1,
                    y: -1,
                    w: 2,
                    h: 2
                }
            )
            .count(),
            1
        );
        assert_eq!(
            mean_color(
                &image,
                IRect {
                    x: 10,
                    y: 10,
                    w: 2,
                    h: 2
                }
            ),
            [0.0; 4],
            "nothing inside"
        );
        assert_eq!(distance(&[1.0, 0.0, 0.0, 1.0], &[0.0, 0.0, 0.0, 1.0]), 1.0);
    }
}
