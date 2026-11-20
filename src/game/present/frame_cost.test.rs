//! Unit tests for [`super`] — the frame-cost probe.

use super::*;

#[test]
fn a_window_closes_after_five_seconds_with_its_mean_and_worst() {
    let mut w = FrameWindow::default();
    // 299 frames of 1/60 s, then one 50 ms hitch: 4.983 + 0.05 > 5 s.
    for _ in 0..299 {
        assert_eq!(w.observe(1.0 / 60.0), None);
    }
    let s = w.observe(0.05).expect("window closes past 5 s");
    assert_eq!(s.frames, 300);
    assert!((s.worst_ms - 50.0).abs() < 1e-3);
    let expected_mean = (299.0 / 60.0 + 0.05) / 300.0 * 1000.0;
    assert!((s.mean_ms - expected_mean).abs() < 1e-3);
    assert_eq!(w, FrameWindow::default(), "a new window starts empty");
}

#[test]
fn the_probe_is_off_by_default() {
    assert!(!FrameCostProbe::default().enabled);
}
