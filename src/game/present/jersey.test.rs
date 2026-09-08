//! Unit tests for [`super`] — the jersey module.

use super::*;

#[test]
fn glyphs_exist_for_the_full_roster_alphabet() {
    for c in ('A'..='Z').chain('0'..='9') {
        assert_ne!(letter(c), [0; 7], "glyph for {c:?} is blank");
    }
    assert_eq!(letter(' '), [0; 7]);
}

#[test]
fn back_texture_draws_name_and_number_pixels() {
    let card = PlayerCard {
        name: "OKAFOR".to_string(),
        number: 23,
        appearance: Default::default(),
    };
    let image = build_texture(&card, JerseyFace::Back, [255, 255, 255, 255]);
    let data = image.data;
    let lit = data.chunks(4).filter(|px| px[3] == 255).count();
    // A six-letter name plus two big digits lights up plenty of pixels.
    assert!(lit > 200, "only {lit} opaque pixels drawn");
    // Everything else stays transparent (the jersey shows through).
    let clear = data.chunks(4).filter(|px| px[3] == 0).count();
    assert!(clear > lit);
}

#[test]
fn number_texture_scales_single_digits_up() {
    let card = PlayerCard {
        name: "PYE".to_string(),
        number: 8,
        appearance: Default::default(),
    };
    let one = build_texture(&card, JerseyFace::Number, [255, 255, 255, 255]);
    let lit = one.data.chunks(4).filter(|px| px[3] == 255).count();
    assert!(lit > 100, "a lone digit should be drawn large ({lit} px)");
}

#[test]
fn lettering_contrasts_with_the_jersey() {
    let on_dark = contrast_color(Color::srgb(0.1, 0.1, 0.3));
    let on_light = contrast_color(Color::srgb(0.9, 0.9, 0.85));
    assert!(on_dark[0] > 200);
    assert!(on_light[0] < 60);
}
