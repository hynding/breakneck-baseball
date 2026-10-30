//! Fits the whole UI to small screens: one [`UiScale`] from the window's
//! logical size, so a phone (landscape ~844×390, portrait ~390×844) gets
//! the menu, settings, pause board and HUD shrunk to fit instead of cut
//! off. Every screen is laid out in UI pixels against a design canvas of
//! at least [`DESIGN_SIZE`]; below that the scale drops, never below
//! [`MIN_UI_SCALE`], and it never grows past 1 on big screens.
//!
//! Anything that converts between window pixels and UI pixels must go
//! through the scale: `input::node_screen_rect` for hit-tests, and the
//! touch chrome's painter for its window-pixel rects.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// The smallest logical window the UI was designed for at full size: the
/// main menu card is ~720 px tall, and a portrait phone needs ~480 px of
/// width for the HUD's two bottom cards to sit side by side.
pub const DESIGN_SIZE: Vec2 = Vec2::new(480.0, 720.0);

/// Floor so text stays legible on the smallest phones.
pub const MIN_UI_SCALE: f32 = 0.5;

/// The UI scale for a window of `size` logical pixels.
pub fn fit_scale(size: Vec2) -> f32 {
    if size.min_element() <= 0.0 {
        return 1.0;
    }
    (size / DESIGN_SIZE).min_element().clamp(MIN_UI_SCALE, 1.0)
}

/// Keeps [`UiScale`] fitted to the primary window. Runs in `First`, so
/// every reader (hit-tests in `Update`, layout in `PostUpdate`) sees one
/// value per frame; guarded, so a steady window never dirties the
/// resource and re-lays out the UI.
pub(super) fn fit_ui_scale(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let want = fit_scale(window.size());
    if ui_scale.0 != want {
        ui_scale.0 = want;
    }
}

#[cfg(test)]
#[path = "fit.test.rs"]
mod tests;
