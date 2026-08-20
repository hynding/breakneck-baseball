//! The floating strike-zone overlay and PCI aiming cursor — the catcher's-eye
//! duel view's aiming aids.

use bevy::prelude::*;

use crate::game::batting::{PciState, style_for};
use crate::game::flow::{ContactEvent, Phase, Play};
use crate::game::input::Controllers;
use crate::game::rules;
use crate::game::rules::ContactQuality;
use crate::game::settings::{BattingStyle, Settings};
use crate::game::{GameplayEntity, ScoreBoard};

use super::{PciCursorMarker, StrikeZoneOverlay, ZoneFlash};

/// The drawn zone is the *plate-width* rulebook zone; the umpire's calls
/// extend one ball radius past the frame (see [`rules::ZONE_HALF_WIDTH`]'s
/// doc — the "any part of the ball" allowance, docs/BASEBALL.md).
const ZONE_DRAWN_HALF_WIDTH: f32 = rules::PLATE_HALF_WIDTH_M;
/// The zone volume is as deep as home plate (17 in front edge to point,
/// docs/BASEBALL.md) — the rulebook zone is a prism *over the plate*.
const ZONE_DEPTH: f32 = super::diamond::PLATE_WIDTH;
/// Darker wireframe per the design ask: near-black steel, nearly
/// transparent — a ghost of a K-zone that never competes with the ball or
/// the PCI cursor.
const ZONE_FRAME_COLOR: Color = Color::srgba(0.10, 0.11, 0.14, 0.20);
/// A whisper of dark tint on the near face only — enough for the PCI
/// cursor to read against, never a bright pane.
const ZONE_FILL_COLOR: Color = Color::srgba(0.05, 0.06, 0.08, 0.10);
/// Wireframe bar thickness — hairline rails (halved from the first
/// designer pass, and halved again on review).
const ZONE_BAR: f32 = 0.004;

/// How long the zone-frame flash pulse holds before fading back.
const ZONE_FLASH_SECS: f32 = 0.18;

/// A floating 3D wireframe box over the plate showing the zone the umpire
/// calls ([`rules::ZONE_LOW`]..[`rules::ZONE_HIGH`], plate-wide and
/// plate-deep per docs/BASEBALL.md "Strike zone"; calls get the ball-radius
/// edge allowance past the drawn frame) — the catcher's-eye duel view's
/// aiming aid. Visible only during the duel (see
/// [`strike_zone_visibility`]); no colliders, the ball flies through it.
pub(super) fn spawn_strike_zone(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    theme: &crate::game::theme::Theme,
) {
    let height = rules::ZONE_HIGH - rules::ZONE_LOW;
    let mid_y = (rules::ZONE_HIGH + rules::ZONE_LOW) / 2.0;
    let mut translucent = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        })
    };
    let frame_base_color = ZONE_FRAME_COLOR;
    let fill = translucent(ZONE_FILL_COLOR);
    let frame = translucent(frame_base_color);

    // Task B4: a Solid/Perfect contact pulses the frame bars toward the
    // theme accent, then fades back to `frame_base_color` — presentation
    // only, teaching the timing windows without touching the rules.
    commands.insert_resource(ZoneFlash {
        material: frame.clone(),
        base_color: frame_base_color,
        flash_color: theme.ui.accent.with_alpha(1.0),
        timer: None,
    });

    let mut part = |size: Vec3, pos: Vec3, mat: &Handle<StandardMaterial>| {
        commands.spawn((
            StrikeZoneOverlay,
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(pos),
        ));
    };

    let hw = ZONE_DRAWN_HALF_WIDTH;
    let hd = ZONE_DEPTH / 2.0;
    let bar = ZONE_BAR;
    // Subtle fill on the near (catcher-side) face only, for PCI contrast.
    part(
        Vec3::new(hw * 2.0, height, 0.004),
        Vec3::new(0.0, mid_y, -hd),
        &fill,
    );
    // The 12 edges of the zone prism: horizontals and verticals on the near
    // and far faces...
    for z in [-hd, hd] {
        for y in [rules::ZONE_LOW, rules::ZONE_HIGH] {
            part(
                Vec3::new(hw * 2.0 + bar, bar, bar),
                Vec3::new(0.0, y, z),
                &frame,
            );
        }
        for x in [-hw, hw] {
            part(
                Vec3::new(bar, height + bar, bar),
                Vec3::new(x, mid_y, z),
                &frame,
            );
        }
    }
    // ...and the four depth rails connecting them.
    for x in [-hw, hw] {
        for y in [rules::ZONE_LOW, rules::ZONE_HIGH] {
            part(
                Vec3::new(bar, bar, ZONE_DEPTH + bar),
                Vec3::new(x, y, 0.0),
                &frame,
            );
        }
    }

    // The PCI aiming cursor: a small unlit quad sitting a hair off the zone
    // box's near face (toward the behind-home camera) so it reads over the
    // fill. Hidden at spawn, revealed only for a human PCI batter (wasm UI
    // rule: the scene spawns at game start and shows/hides — never respawns).
    let cursor_mat = translucent(theme.ui.accent.with_alpha(0.9));
    commands.spawn((
        PciCursorMarker,
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(0.06, 0.06, 0.004))),
        MeshMaterial3d(cursor_mat),
        Transform::from_translation(Vec3::new(0.0, mid_y, -hd - 0.02)),
        Visibility::Hidden,
    ));
}

/// The zone box belongs to the duel: shown while a pitch is coming, hidden
/// the moment the ball is in play — except a live Task-B4 flash pulse holds
/// it up a beat longer (`flash.timer.is_some()`) so the pulse that fires the
/// same frame contact flips the phase to `InPlay` actually gets a chance to
/// render, instead of being hidden the very frame it's set. Players can
/// switch the overlay off entirely from the pause board
/// ([`Settings::show_strike_zone`], toggled with **Z** in `subs.rs`).
pub(super) fn strike_zone_visibility(
    play: Res<Play>,
    flash: Res<ZoneFlash>,
    settings: Res<Settings>,
    mut overlay: Query<&mut Visibility, With<StrikeZoneOverlay>>,
) {
    let visible = settings.show_strike_zone
        && (matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch)
            || flash.timer.is_some());
    for mut visibility in &mut overlay {
        let desired = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
        }
    }
}

/// The PCI cursor belongs to the batting side's duel: shown only while a
/// *human* batter set to [`BattingStyle::PciCursor`] is up and a pitch is
/// coming, and parked at the live cursor position ([`PciState::cursor`]) each
/// frame. A CPU batter always routes Classic (so [`style_for`] never returns
/// PCI for it), which keeps the marker hidden on defense/CPU at-bats.
pub(super) fn pci_cursor_visibility(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    controllers: Res<Controllers>,
    settings: Res<Settings>,
    pci: Res<PciState>,
    mut marker: Query<(&mut Visibility, &mut Transform), With<PciCursorMarker>>,
) {
    let batting = score.batting_team();
    let show = controllers.player_index(batting).is_some()
        && style_for(batting, &controllers, &settings) == BattingStyle::PciCursor
        && matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    let cursor = pci.cursor(batting);
    for (mut visibility, mut transform) in &mut marker {
        let desired = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
        }
        if show {
            transform.translation.x = cursor.x;
            transform.translation.y = cursor.y;
        }
    }
}

/// A judged Solid/Perfect swing pulses the zone frame toward the theme
/// accent — `FoulTip`/`Whiff` leave it alone (a whiff already has its own
/// strike banner; `Weak` is bucketed with `Solid`, see [`ZoneFlash`]'s doc).
pub(super) fn trigger_zone_flash(
    mut contact_ev: EventReader<ContactEvent>,
    mut flash: ResMut<ZoneFlash>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for ev in contact_ev.read() {
        if !matches!(
            ev.quality,
            ContactQuality::Perfect | ContactQuality::Solid | ContactQuality::Weak
        ) {
            continue;
        }
        if let Some(mat) = materials.get_mut(&flash.material) {
            mat.base_color = flash.flash_color;
        }
        flash.timer = Some(Timer::from_seconds(ZONE_FLASH_SECS, TimerMode::Once));
    }
}

/// Restores the zone frame's resting tint once the flash pulse timer expires.
pub(super) fn restore_zone_flash(
    time: Res<Time>,
    mut flash: ResMut<ZoneFlash>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let done = match flash.timer.as_mut() {
        Some(timer) => timer.tick(time.delta()).finished(),
        None => false,
    };
    if !done {
        return;
    }
    let (material, base_color) = (flash.material.clone(), flash.base_color);
    flash.timer = None;
    if let Some(mat) = materials.get_mut(&material) {
        mat.base_color = base_color;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// The zone overlay is a 3D wireframe the size of the rulebook zone:
    /// plate width, plate depth, knee-to-midpoint tall (docs/BASEBALL.md
    /// "Strike zone") — darker than the old washed-out white frame (every
    /// channel below 0.5), with nonzero alpha per the wasm UI rule.
    /// Deliberately asserts on constants: it pins design-reviewed values
    /// against silent drift.
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn zone_wireframe_matches_rulebook_dimensions() {
        assert!((ZONE_DRAWN_HALF_WIDTH - rules::PLATE_HALF_WIDTH_M).abs() < 1e-6);
        assert!((ZONE_DEPTH - super::super::diamond::PLATE_WIDTH).abs() < 1e-6);
        let c = ZONE_FRAME_COLOR.to_srgba();
        assert!(
            c.red < 0.5 && c.green < 0.5 && c.blue < 0.5,
            "the wireframe should read dark, got {c:?}"
        );
        // Designer-reviewed look: hairline rails, nearly transparent — but
        // never alpha 0 (wasm rule).
        assert!(
            c.alpha > 0.0 && c.alpha <= 0.25,
            "frame should be nearly transparent, got alpha {}",
            c.alpha
        );
        assert!(
            ZONE_BAR <= 0.005,
            "rails should stay hairline, got {ZONE_BAR}"
        );
        let f = ZONE_FILL_COLOR.to_srgba();
        assert!(f.alpha > 0.0 && f.alpha < 0.2, "fill stays a whisper");
    }
}
