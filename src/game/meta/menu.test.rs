//! Unit tests for [`super`] — the menu module.

use super::*;

#[test]
fn row_labels_show_hotkeys_only_to_the_keyboard() {
    assert_eq!(
        row_label("F   Field", false, ActiveDevice::Keyboard),
        "  F   Field"
    );
    assert_eq!(
        row_label("F   Field", true, ActiveDevice::Gamepad),
        "> Field"
    );
    // Touch has no cursor, so no marker column either.
    assert_eq!(row_label("F   Field", true, ActiveDevice::Touch), "Field");
}

#[test]
fn each_device_reads_only_its_own_controls() {
    let pad = controls_hint(ActiveDevice::Gamepad);
    let keys = controls_hint(ActiveDevice::Keyboard);
    let touch = controls_hint(ActiveDevice::Touch);
    assert!(!pad.contains("WASD") && !pad.contains("Tap"));
    assert!(!keys.contains("D-pad") && !keys.contains("Tap"));
    assert!(!touch.contains("WASD") && !touch.contains("D-pad"));
}
