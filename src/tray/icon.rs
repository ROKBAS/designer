//! Programmatically generated tray icon, so the binary needs no asset files.

use tray_icon::{BadIcon, Icon};

/// Icon edge length in pixels. Windows scales this down for the notification
/// area and up on high-DPI displays.
pub const ICON_SIZE: u32 = 32;

/// Build the tray icon: an amber sun with eight rays on a transparent
/// background.
pub fn app_icon() -> Result<Icon, BadIcon> {
    Icon::from_rgba(brightness_rgba(ICON_SIZE), ICON_SIZE, ICON_SIZE)
}

/// Rasterise the sun glyph into an RGBA buffer.
///
/// Coverage is computed from distances so edges get partial alpha, which keeps
/// the small tray icon from looking jagged.
pub fn brightness_rgba(size: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    let center = size as f32 / 2.0;
    let core_radius = size as f32 * 0.21;
    let ray_inner = size as f32 * 0.30;
    let ray_outer = size as f32 * 0.47;
    let ray_thickness = (size as f32 * 0.055).max(1.0);
    let ray_step = std::f32::consts::PI / 4.0;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let distance = (dx * dx + dy * dy).sqrt();

            // Angular distance from the nearest of eight ray axes.
            let angle = dy.atan2(dx);
            let nearest_axis = (angle / ray_step).round() * ray_step;
            let angular_delta = (angle - nearest_axis).abs();
            let perpendicular = distance * angular_delta.sin();

            let core_coverage = (core_radius - distance + 0.5).clamp(0.0, 1.0);

            let ray_coverage = if distance >= ray_inner && distance <= ray_outer {
                let edge = (ray_outer - distance)
                    .min(distance - ray_inner)
                    .min(ray_thickness - perpendicular);
                (edge + 0.5).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let alpha = core_coverage.max(ray_coverage);
            data.extend_from_slice(&[255, 200, 61, (alpha * 255.0).round() as u8]);
        }
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(data: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * size + x) * 4) as usize;
        [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]
    }

    #[test]
    fn buffer_has_expected_size() {
        let data = brightness_rgba(ICON_SIZE);
        assert_eq!(data.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
    }

    #[test]
    fn center_is_opaque_and_corner_is_transparent() {
        let size = ICON_SIZE;
        let data = brightness_rgba(size);
        let center = pixel(&data, size, size / 2, size / 2);
        assert_eq!(center[3], 255, "center of the sun must be opaque");
        assert_eq!(pixel(&data, size, 0, 0)[3], 0, "corner must be transparent");
        assert_eq!(
            pixel(&data, size, size - 1, size - 1)[3],
            0,
            "corner must be transparent"
        );
    }

    #[test]
    fn glyph_covers_a_reasonable_area() {
        let data = brightness_rgba(ICON_SIZE);
        let opaque = data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] > 0)
            .count();
        let total = (ICON_SIZE * ICON_SIZE) as usize;
        assert!(
            opaque > total / 8 && opaque < total / 2,
            "unexpected glyph coverage: {opaque}/{total}"
        );
    }
}
