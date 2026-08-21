//! Shared test fixtures for `core/rules`'s submodule test modules.
#![cfg(test)]

use bevy::math::Vec3;

use crate::game::variant::{FieldSpec, PaceTuning, Ruleset, VariantId};

use super::Bases;

pub(super) fn std_rules() -> Ruleset {
    VariantId::Standard.rules()
}

pub(super) fn pace() -> PaceTuning {
    PaceTuning::default()
}

pub(super) fn empty() -> Bases {
    Bases::default()
}

/// A standard diamond with the given (0-indexed) bases occupied.
pub(super) fn with(occupied: &[usize]) -> Bases {
    let mut b = Bases::default();
    for &base in occupied {
        b.set(base, true);
    }
    b
}

pub(super) fn loaded() -> Bases {
    with(&[0, 1, 2])
}

pub(super) fn std_field() -> FieldSpec {
    VariantId::Standard.field()
}

/// Straightaway-centre launch velocity from angle (degrees) and speed.
pub(super) fn vel_at(launch_deg: f32, speed: f32) -> Vec3 {
    vel_spray(launch_deg, speed, 0.0)
}

/// Launch velocity sprayed `spray_deg` degrees off the centre-field axis.
pub(super) fn vel_spray(launch_deg: f32, speed: f32, spray_deg: f32) -> Vec3 {
    let launch = launch_deg.to_radians();
    let spray = spray_deg.to_radians();
    let horizontal = speed * launch.cos();
    Vec3::new(
        horizontal * spray.sin(),
        speed * launch.sin(),
        horizontal * spray.cos(),
    )
}
