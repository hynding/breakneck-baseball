use super::*;

#[test]
fn an_arm_is_fresh_until_the_fade_window() {
    assert_eq!(tiredness(0, STARTER_STAMINA), 0.0);
    assert_eq!(
        tiredness(STARTER_STAMINA - FADE_PITCHES, STARTER_STAMINA),
        0.0
    );
    assert!((tiredness(STARTER_STAMINA, STARTER_STAMINA) - 1.0).abs() < 1e-6);
    assert!(tiredness(STARTER_STAMINA + 10, STARTER_STAMINA) > 1.0);
    // A reliever's whole outing sits inside his fade window.
    assert!(tiredness(10, RELIEVER_STAMINA) > 0.0);
}

#[test]
fn a_spent_arm_loses_about_a_mph_and_some_control() {
    let heater = 95.0 * velocity_scale(1.0);
    assert!((93.8..=94.2).contains(&heater), "{heater}");
    assert_eq!(velocity_scale(0.0), 1.0);
    assert_eq!(control_wander(0.0), 0.0);
    assert!(control_wander(1.0) > 0.0);
    // Never slower than the floor, however long he stays out there.
    assert_eq!(velocity_scale(100.0), 0.8);
}

#[test]
fn counts_are_per_team_and_number_and_the_first_arm_starts() {
    let mut f = Fatigue::default();
    for _ in 0..3 {
        f.record_pitch(Team::Home, 7);
    }
    f.record_pitch(Team::Away, 7);
    assert_eq!(f.pitches(Team::Home, 7), 3);
    assert_eq!(f.pitches(Team::Away, 7), 1);
    assert_eq!(f.stamina(Team::Home, 7), STARTER_STAMINA);
    f.record_pitch(Team::Home, 60);
    assert_eq!(f.stamina(Team::Home, 60), RELIEVER_STAMINA);
    // Before anyone pitches, whoever takes the ball is the starter.
    assert_eq!(Fatigue::default().stamina(Team::Home, 60), STARTER_STAMINA);
}

#[test]
fn the_starter_is_spent_at_his_stamina() {
    let mut f = Fatigue::default();
    for _ in 0..STARTER_STAMINA - 1 {
        f.record_pitch(Team::Away, 21);
    }
    assert!(!f.spent(Team::Away, 21));
    f.record_pitch(Team::Away, 21);
    assert!(f.spent(Team::Away, 21));
}

#[test]
fn the_bullpen_brings_in_an_arm_that_has_not_pitched() {
    let mut f = Fatigue::default();
    f.record_pitch(Team::Home, 7);
    f.record_pitch(Team::Home, 51);
    assert_eq!(pick_reliever(&f, Team::Home, &[51, 8, 60]), Some(1));
    f.record_pitch(Team::Home, 8);
    f.record_pitch(Team::Home, 60);
    assert_eq!(pick_reliever(&f, Team::Home, &[51, 8, 60]), None);
}
