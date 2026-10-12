//! Zoom, shared by every viewer.
//!
//! One notch of the wheel and one press of a zoom button have to mean the same
//! thing wherever they are used, so the multipliers, the limits and the
//! percentages the menus offer live here rather than in each viewer. The
//! viewers differ in what they can show (a page, an image, a diagram), not in
//! how far a step takes them.

/// Multiplier for one notch of the mouse wheel, or a trackpad pinch.
pub const WHEEL_STEP: f32 = 1.2;
/// Multiplier for the zoom in / out buttons and the zoom shortcuts.
pub const BUTTON_STEP: f32 = 1.2;
/// Smallest zoom, as a factor.
pub const MIN: f32 = 0.1;
/// Largest zoom, as a factor.
pub const MAX: f32 = 8.0;
/// The factor a viewer opens at.
pub const DEFAULT: f32 = 1.0;

/// The factors the zoom percentage menus offer, in the order they are listed.
pub const PRESETS: &[(f32, &str)] = &[
    (0.25, "25%"),
    (0.5, "50%"),
    (0.75, "75%"),
    (1.0, "100%"),
    (1.25, "125%"),
    (1.5, "150%"),
    (2.0, "200%"),
    (4.0, "400%"),
    (8.0, "800%"),
];

/// The label for a zoom factor: `120%` for 1.2.
pub fn percent(zoom: f32) -> String {
    format!("{:.0}%", zoom * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_and_a_preset_agree_with_the_limits() {
        assert!(WHEEL_STEP > 1.0 && BUTTON_STEP > 1.0, "steps zoom in");
        assert!(MIN < DEFAULT && DEFAULT < MAX);
        // Every preset has to be reachable, or the menu would offer a factor
        // the viewers clamp away.
        for &(factor, label) in PRESETS {
            assert!(
                (MIN..=MAX).contains(&factor),
                "{label} is outside {MIN}..{MAX}"
            );
        }
    }

    #[test]
    fn the_percentage_label_rounds() {
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(1.25), "125%");
        assert_eq!(percent(0.876), "88%");
    }
}
