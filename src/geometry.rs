//! Zoom math shared by the magnifier and its tests.

use crate::config::clamp_zoom;

/// Multiplicative factor applied per zoom key step.
pub const ZOOM_STEP: f32 = 1.1;

pub fn step_zoom(zoom: f32, steps: i32) -> f32 {
    clamp_zoom(zoom * ZOOM_STEP.powi(steps))
}

/// Top-left corner of a `size`-pixel square centered on the cursor.
pub fn window_origin(cursor: (i32, i32), size: i32) -> (i32, i32) {
    (cursor.0 - size / 2, cursor.1 - size / 2)
}

/// Screen rectangle (left, top, right, bottom) the magnifier must capture so that
/// the cursor pixel lands exactly on the window's center pixel (`size / 2`).
pub fn source_rect(cursor: (i32, i32), size: i32, zoom: f32) -> (i32, i32, i32, i32) {
    let half = (size / 2) as f32 / zoom;
    let src = ((size as f32 / zoom).round() as i32).max(1);
    let left = (cursor.0 as f32 - half).round() as i32;
    let top = (cursor.1 as f32 - half).round() as i32;
    (left, top, left + src, top + src)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_multiplicative_and_clamped() {
        assert_eq!(step_zoom(2.0, 1), 2.2);
        assert_eq!(step_zoom(2.0, -1), 1.82);
        assert_eq!(step_zoom(1.15, -5), 1.1);
        assert_eq!(step_zoom(15.0, 10), 16.0);
        assert_eq!(step_zoom(2.0, 0), 2.0);
    }

    #[test]
    fn cursor_maps_to_center_pixel() {
        for &(size, zoom) in &[(350, 2.0f32), (400, 4.0), (351, 3.0), (300, 2.5)] {
            let cursor = (1000, 700);
            let (l, t, r, b) = source_rect(cursor, size, zoom);
            assert_eq!(r - l, b - t);
            // Window pixel `size / 2` shows source pixel `left + (size / 2) / zoom`.
            let center_src = l as f32 + (size / 2) as f32 / zoom;
            assert!(
                (center_src - cursor.0 as f32).abs() <= 0.5,
                "size {size} zoom {zoom}: {center_src}"
            );
            let (x, _) = window_origin(cursor, size);
            assert_eq!(x + size / 2, cursor.0);
        }
    }
}
