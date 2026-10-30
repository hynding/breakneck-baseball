use super::*;

#[test]
fn desktop_windows_keep_full_size() {
    assert_eq!(fit_scale(Vec2::new(1280.0, 720.0)), 1.0);
    assert_eq!(fit_scale(Vec2::new(2560.0, 1440.0)), 1.0);
}

#[test]
fn phones_shrink_to_fit_in_either_orientation() {
    let landscape = fit_scale(Vec2::new(844.0, 390.0));
    let portrait = fit_scale(Vec2::new(390.0, 844.0));
    assert!((landscape - 390.0 / 720.0).abs() < 1e-4, "{landscape}");
    assert!((portrait - 390.0 / 480.0).abs() < 1e-4, "{portrait}");
}

#[test]
fn tiny_or_degenerate_windows_are_clamped() {
    assert_eq!(fit_scale(Vec2::new(200.0, 200.0)), MIN_UI_SCALE);
    assert_eq!(fit_scale(Vec2::ZERO), 1.0);
}
