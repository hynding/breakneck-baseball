//! Heads-up display — scoreboard card, count dots, base ring, and banners.
//!
//! All live game data comes from the [`ScoreBoard`] and [`Bases`] resources
//! and from [`PlayBanner`] events fired by `game::flow`. Every colour and
//! styling knob comes from the active [`Theme`] — the HUD owns layout only.

use bevy::prelude::*;

use crate::game::GameState;

mod banner;
mod hud;

use banner::{
    BannerTimer, ContactStampTimer, fade_banner, fade_contact_stamp, show_banner,
    show_contact_stamp, update_duel_panels,
};
use hud::{
    spawn_hud, update_base_ring, update_count_dots, update_inning_text, update_meter_bar,
    update_score_text,
};

// ── Markers ───────────────────────────────────────────────────────────────────

#[derive(Component)]
struct InningText;

#[derive(Component)]
struct ScoreText;

/// Which at-bat counter a dot belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CountKind {
    Ball,
    Strike,
    Out,
}

/// One indicator dot: lights up while `index <` the current count.
#[derive(Component)]
struct CountDot {
    kind: CountKind,
    index: u32,
}

/// One base-occupancy pip (0-indexed base number).
#[derive(Component)]
struct BaseIndicator(usize);

/// The banner pill chrome (persistent; painted/cleared via child mutations).
#[derive(Component)]
struct BannerPill;

/// The Swing Meter's load bar: a slim vertical track beside the count HUD. The
/// [`MeterFill`] child's height follows the batting team's meter load; the
/// track shell stays painted (dim) per the wasm UI rule — never despawned.
#[derive(Component)]
struct MeterFill;

/// The banner text inside the pill.
#[derive(Component)]
struct BannerText;

/// The contact-quality stamp (PERFECT! / EARLY / LATE / FOUL TIP), painted at
/// spawn near the zone-box screen area and shown by text mutation only — see
/// the wasm UI rule on [`hidden_tint`]. Public so e2e tests can query its
/// `Text` content directly (the same pattern `player::CatcherRole` uses for
/// `e2e_camera_views`'s `Visibility` check).
#[derive(Component)]
pub struct ContactStampText;

/// Root of one of the two duel cards flanking the catcher's-eye pitch view.
#[derive(Component)]
struct DuelPanel;

/// One line of a duel card, updated (and shown/hidden) by phase.
#[derive(Component)]
struct DuelLine(DuelLineKind);

#[derive(Clone, Copy, PartialEq, Eq)]
enum DuelLineKind {
    BatterTitle,
    BatterTeam,
    BatterSlot,
    BatterRuns,
    PitcherTitle,
    PitcherTeam,
    LegendFast,
    LegendChange,
    LegendCurve,
    LegendSlider,
    LegendSinker,
}

/// A colour reduced to near-invisibility. Never fully transparent: on the
/// wasm target an element extracted with alpha 0 is culled for good.
pub(crate) fn hidden_tint(color: Color) -> Color {
    color.with_alpha(0.004)
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BannerTimer>()
            .init_resource::<ContactStampTimer>()
            .add_systems(crate::game::game_start(), spawn_hud)
            .add_systems(
                Update,
                (
                    update_inning_text,
                    update_score_text,
                    update_count_dots,
                    update_meter_bar,
                    update_base_ring,
                    update_duel_panels,
                    show_banner,
                    fade_banner,
                    show_contact_stamp,
                    fade_contact_stamp,
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
