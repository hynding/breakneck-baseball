//! Heads-up display — scoreboard card, count dots, base ring, and banners.
//!
//! All live game data comes from the [`ScoreBoard`] and [`Bases`] resources
//! and from [`PlayBanner`] events fired by `game::flow`. Every colour and
//! styling knob comes from the active [`Theme`] — the HUD owns layout only.

use bevy::prelude::*;

use crate::game::GameState;
use crate::game::theme::UiTheme;

mod banner;
mod hud;
mod touch;

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

// ── Overlay screens ───────────────────────────────────────────────────────────

/// [`GlobalZIndex`] tiers for the full-screen overlays, ordered bottom-up.
///
/// Stacking used to be spawn-order luck (TODO 67). Every overlay root reads
/// its tier from here so the ladder is one list instead of four literals with
/// four copies of the ladder in prose — a new screen picks its rung by
/// reading this module, and no site can disagree with another about where a
/// neighbour sits.
///
/// **There is deliberately no banner tier.** The play banner and contact
/// stamp sit at the *default* tier (see `hud::spawn_hud`): a cycle-2 attempt
/// to merge them under a `GlobalZIndex` stopped extracting on wasm and was
/// reverted. They render above gameplay by spawn order alone, and the
/// comments that once claimed "banners (40)" were describing a tier that was
/// tried and abandoned. Anything added here must survive the wasm UI rule.
pub(crate) mod z {
    /// Main menu — the bottom overlay; every other screen opens over it.
    pub const MENU: i32 = 10;
    /// Settings screen, opened from the menu with **S**.
    pub const SETTINGS: i32 = 20;
    /// Pause / substitutions board.
    pub const PAUSE: i32 = 30;
    /// The touch pause button, one rung above the pause board: it stays a
    /// live resume target while Paused, so it must draw over that board's
    /// full-screen dim. An invisible-but-live control is exactly the class
    /// the shared visibility predicate exists to prevent.
    pub const TOUCH_CHROME: i32 = PAUSE + 1;
}

/// How an overlay is made to appear and disappear — the choice that decides
/// whether its chrome may be painted with real colours at spawn.
///
/// This is the wasm UI rule in enum form. Spelling the two answers out once,
/// here, is what stops a screen from picking the paint that happens to look
/// right natively and silently never rendering in the browser.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayPaint {
    /// The screen exists only while it is visible — it is spawned on open and
    /// despawned on close (or rebuilt per press, like the menu), so its chrome
    /// is painted with real colours straight away.
    Opaque,
    /// The screen's entities outlive its visibility: it is shown and hidden by
    /// mutating children of a root painted at spawn, so that root must be
    /// spawned near-invisible rather than transparent (see [`hidden_tint`]).
    /// Such roots generally also want [`KeepAliveUi`].
    Hidden,
}

/// The scrim behind an [`OverlayPaint::Opaque`] screen. Not fully opaque: the
/// field stays faintly readable behind the menu.
const OVERLAY_SCRIM_ALPHA: f32 = 0.97;

impl OverlayPaint {
    /// The full-screen backdrop colour behind the card.
    fn scrim(self, color: Color) -> Color {
        match self {
            Self::Opaque => color.with_alpha(OVERLAY_SCRIM_ALPHA),
            Self::Hidden => hidden_tint(color),
        }
    }

    /// A colour on the card itself (background or border).
    fn panel(self, color: Color) -> Color {
        match self {
            Self::Opaque => color,
            Self::Hidden => hidden_tint(color),
        }
    }
}

/// The full-screen, centred root every overlay screen spawns: tier, layout,
/// and the backdrop paint its [`OverlayPaint`] implies.
///
/// Returned as loose components rather than spawned here so a caller can add
/// its own marker (and `KeepAliveUi`/`GameplayEntity` where those apply), and
/// can tweak the [`Node`] before spawning — the pause board stacks its
/// children in a column. Same "build it, adjust it, spawn it" shape the duel
/// cards in `banner.rs` already use.
pub(crate) fn overlay_root(
    tier: i32,
    paint: OverlayPaint,
    ui: &UiTheme,
) -> (GlobalZIndex, Node, BackgroundColor) {
    (
        GlobalZIndex(tier),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(paint.scrim(ui.panel_bg)),
    )
}

/// The centred card an overlay's contents sit in: a bordered, rounded column
/// whose padding and row spacing are the only things screens vary.
pub(crate) fn overlay_card(
    paint: OverlayPaint,
    padding: Vec2,
    row_gap: f32,
    ui: &UiTheme,
) -> (Node, BackgroundColor, BorderColor, BorderRadius) {
    (
        Node {
            padding: UiRect::axes(Val::Px(padding.x), Val::Px(padding.y)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(row_gap),
            border: UiRect::all(Val::Px(CARD_BORDER_PX)),
            ..default()
        },
        BackgroundColor(paint.panel(ui.panel_bg)),
        BorderColor(paint.panel(ui.panel_border)),
        BorderRadius::all(Val::Px(CARD_RADIUS_PX)),
    )
}

/// Hairline border on every overlay card.
const CARD_BORDER_PX: f32 = 1.5;
/// Corner rounding on every overlay card.
const CARD_RADIUS_PX: f32 = 16.0;

/// Marker for near-transparent UI roots that must stay extraction-alive on
/// wasm: [`keep_ui_roots_alive`] re-touches their background change tick
/// every frame. The 2026-08-27 bisect showed a transparent, never-repainted
/// root stops extracting its whole subtree on wasm — this is the one shared
/// mechanism for that rule, so no screen has to hand-roll (or remember) a
/// per-frame repaint.
///
/// When to add it: a root (or card) that is BOTH (a) hidden-tinted /
/// near-transparent and (b) repainted only by change-gated systems — i.e.
/// nothing else touches its ticks on quiet frames. Carriers: the settings
/// screen (root + card), the touch overlay (root + hint container), and
/// the pause board (root + card + controls dialog — `subs::update_board`
/// IS change-gated, so quiet stretches leave its hidden z-30 root
/// untouched, the exact 2026-08-27 stall shape). Screens that don't need
/// it: the HUD and menu, painted with real colors at spawn (and the menu
/// rebuilds per press). When a screen's per-frame repaint gets
/// change-gated, that's the moment its hidden roots take this marker.
#[derive(Component)]
pub struct KeepAliveUi;

/// See [`KeepAliveUi`]. Ungated: the rule applies in every state a marked
/// root exists in (menus, gameplay, pause). wasm-only — the extraction
/// stall this works around does not exist natively, so native builds skip
/// the per-frame change-tick churn.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn keep_ui_roots_alive(mut roots: Query<&mut BackgroundColor, With<KeepAliveUi>>) {
    for mut bg in &mut roots {
        bg.set_changed();
    }
}

/// Writes a `Text` only when different — glyph re-shaping is the expensive
/// part of a Text write, and `Text` lacks `PartialEq` so `set_if_neq` can't
/// serve. Takes the `Mut` so the compare itself never dirties the change
/// tick. The one guard every painter calls, instead of seven hand-rolled
/// copies that can each silently lose it.
pub(crate) fn set_text_if_neq(text: &mut Mut<Text>, want: &str) {
    if text.as_str() != want {
        ***text = want.to_string();
    }
}

/// [`set_text_if_neq`]'s sibling for `TextColor`, which lacks `PartialEq`
/// (so `set_if_neq` can't serve) — same rationale: one guard, not hand-
/// rolled copies that can each silently lose it.
pub(crate) fn set_color_if_neq(color: &mut Mut<TextColor>, want: Color) {
    if color.0 != want {
        color.0 = want;
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BannerTimer>()
            .init_resource::<ContactStampTimer>();
        #[cfg(target_arch = "wasm32")]
        app.add_systems(Update, keep_ui_roots_alive);
        app.add_systems(
            crate::game::game_start(),
            // Chained: both roots sit at the default tier, and bevy_ui
            // breaks ties between same-tier roots by spawn order — so
            // unordered, whether the SWING button drew over the HUD's
            // bottom-right cluster (they overlap on a portrait phone) or
            // under it was a per-schedule-build tie-break. Touch chrome is
            // interactive; it goes on top, so it spawns last.
            (spawn_hud, touch::spawn_touch_overlay).chain(),
        )
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
        )
        // The touch chrome lives through pauses (its pause button is the
        // touch resume control; `KeepAliveUi` keeps the root extracting on
        // wasm), and `tap_pause` runs before the pause board's consumers so
        // a tap and its evaluation land on the same frame. Both skip until
        // a touch is ever seen: nothing paints or accepts presses before
        // then, and the overlay's `LastPainted` default can never match a
        // real state, so the first paint still fires when the bit flips.
        .add_systems(
            Update,
            (
                touch::paint_touch_overlay
                    // After the Paused-state `TouchGestures` writers (the
                    // pause-region claim and the quit row's finger binds,
                    // both inside or before `PauseInputSet`): unordered,
                    // a mid-pause seen/ownership flip painted the chrome
                    // a frame later on some schedule builds than others —
                    // cosmetic, but a per-build tie-break all the same.
                    .after(crate::game::subs::PauseInputSet),
                touch::tap_pause
                    .before(crate::game::subs::PauseInputSet)
                    // Before the Paused-state liveness recompute, so this
                    // reads PRE-frame liveness — the same reveal discipline
                    // as `read_touch`; unordered, the pair was a schedule
                    // ambiguity with per-build outcomes.
                    .before(crate::game::touch::paused_pause_region_taps),
            )
                .run_if(
                    in_state(GameState::Playing)
                        .or(in_state(GameState::Paused))
                        .and(crate::game::touch::touchscreen_seen),
                ),
        );
    }
}
