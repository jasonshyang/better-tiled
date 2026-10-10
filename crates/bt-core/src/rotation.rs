//! Turning on screen. Tiled's rotations are in degrees and clockwise as the screen shows them,
//! with y pointing down (Qt's `QTransform::rotate`).

use glam::DVec2;

/// `v` turned clockwise on screen by `degrees`. Whole quarter turns are exact.
pub fn rotated(v: DVec2, degrees: f64) -> DVec2 {
    let quarters = degrees / 90.0;
    if quarters == quarters.round() {
        // A whole number of quarter turns; `rem_euclid` brings it into 0..4.
        let turn = quarters.rem_euclid(4.0);
        return if turn == 0.0 {
            v
        } else if turn == 1.0 {
            DVec2::new(-v.y, v.x)
        } else if turn == 2.0 {
            -v
        } else {
            DVec2::new(v.y, -v.x)
        };
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    DVec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn quarter_turns_are_exact_and_clockwise() {
        let x = v(1.0, 0.0);
        assert_eq!(rotated(x, 90.0), v(0.0, 1.0), "down the screen");
        assert_eq!(rotated(x, 180.0), v(-1.0, 0.0));
        assert_eq!(rotated(x, -90.0), v(0.0, -1.0));
        assert_eq!(rotated(x, 630.0), v(0.0, -1.0));
        assert_eq!(rotated(x, 0.0), x);
        let turned = rotated(v(2.0, 0.0), 30.0);
        assert!((turned - v(3f64.sqrt(), 1.0)).length() < 1e-12);
        let back = rotated(turned, -30.0);
        assert!((back - v(2.0, 0.0)).length() < 1e-12);
    }
}
