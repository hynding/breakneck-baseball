//! Unit tests for [`super`] — the game module.

use super::*;

#[test]
fn default_config_innings_follow_the_default_variant() {
    assert_eq!(GameConfig::default().innings, 9);
}
