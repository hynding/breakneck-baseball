//! Unit tests for [`super`] — the count module.

use super::super::TagUp;
use super::super::test_support::*;
use super::*;
use crate::game::variant::CountRules;

// ── Count mutations ───────────────────────────────────────────────────────

#[test]
fn third_strike_is_a_strikeout_and_an_out() {
    let mut score = ScoreBoard {
        strikes: 2,
        balls: 3,
        ..Default::default()
    };
    let mut bases = empty();
    assert_eq!(
        call_strike(&mut score, &mut bases, &std_rules(), false),
        StrikeCall::Strikeout
    );
    assert_eq!((score.balls, score.strikes), (0, 0)); // fresh count
    assert_eq!(score.outs, 1);
}

#[test]
fn dropped_third_strike_puts_the_batter_on_first() {
    let mut score = ScoreBoard {
        strikes: 2,
        ..Default::default()
    };
    let mut bases = empty();
    assert_eq!(
        call_strike(&mut score, &mut bases, &std_rules(), true),
        StrikeCall::DroppedThird
    );
    assert_eq!(score.outs, 0); // the batter reached — no out
    assert_eq!((score.balls, score.strikes), (0, 0));
    assert_eq!(bases, with(&[0]));
}

#[test]
fn dropped_flag_before_strike_three_is_a_plain_strike() {
    let mut score = ScoreBoard::default();
    let mut bases = empty();
    assert_eq!(
        call_strike(&mut score, &mut bases, &std_rules(), true),
        StrikeCall::Strike
    );
    assert_eq!(score.strikes, 1);
    assert_eq!(bases, empty());
}

#[test]
fn foul_is_a_strike_but_never_the_third() {
    let mut score = ScoreBoard::default();
    foul(&mut score, &std_rules());
    assert_eq!(score.strikes, 1);
    foul(&mut score, &std_rules());
    assert_eq!(score.strikes, 2);
    foul(&mut score, &std_rules()); // would be strike three — stays at two
    assert_eq!(score.strikes, 2);
}

#[test]
fn fourth_ball_walks_and_forces_runners() {
    let mut score = ScoreBoard {
        balls: 3,
        strikes: 2,
        top_of_inning: true, // Away bats
        ..Default::default()
    };
    let mut bases = loaded();
    assert_eq!(
        call_ball(&mut score, &mut bases, &std_rules()),
        BallCall::Walk { runs: 1 }
    );
    assert_eq!(score.away_runs, 1); // forced run credited to batting team
    assert_eq!((score.balls, score.strikes), (0, 0));
    assert_eq!(bases, loaded()); // still loaded after the force
}

#[test]
fn three_outs_flip_the_half_inning_and_clear_bases() {
    let mut score = ScoreBoard {
        inning: 1,
        top_of_inning: true,
        outs: 2,
        ..Default::default()
    };
    let mut bases = loaded();

    // Third out of the top: flip to the bottom of the same inning.
    record_out(&mut score, &mut bases, &std_rules());
    assert_eq!(score.outs, 0);
    assert!(!score.top_of_inning);
    assert_eq!(score.inning, 1);
    assert_eq!(bases, empty());

    // Third out of the bottom: advance to the top of the next inning.
    score.outs = 2;
    record_out(&mut score, &mut bases, &std_rules());
    assert!(score.top_of_inning);
    assert_eq!(score.inning, 2);
}

#[test]
fn custom_out_threshold_flips_half_inning() {
    let rules = Ruleset {
        counts: CountRules {
            outs_per_half: 4,
            ..std_rules().counts
        },
        ..std_rules()
    };
    let mut score = ScoreBoard {
        inning: 1,
        top_of_inning: true,
        outs: 2,
        ..Default::default()
    };
    let mut bases = empty();
    // Out three of four: the half-inning continues.
    record_out(&mut score, &mut bases, &rules);
    assert_eq!((score.outs, score.top_of_inning), (3, true));
    // Out four retires the side.
    record_out(&mut score, &mut bases, &rules);
    assert_eq!((score.outs, score.top_of_inning), (0, false));
}

// ── Batted-ball outs with base-running consequences ───────────────────────

/// A fresh bottom-half scoreboard with the given outs.
fn batting_home(outs: u32) -> ScoreBoard {
    ScoreBoard {
        inning: 1,
        top_of_inning: false,
        outs,
        ..Default::default()
    }
}

#[test]
fn double_play_retires_the_batter_and_the_forced_runner() {
    let mut score = batting_home(0);
    let mut bases = with(&[0]);
    let play = apply_double_play(&mut score, &mut bases, &std_rules());
    assert!(play.double_play);
    assert_eq!(play.outs, 2);
    assert_eq!(score.outs, 2);
    assert_eq!(bases, empty()); // both the batter and the forced runner are gone
}

#[test]
fn double_play_still_scores_the_runner_from_third() {
    // 6-4-3 with runners on the corners and nobody out: the run counts
    // because the inning does not end on the play.
    let mut score = batting_home(0);
    let mut bases = with(&[0, 2]);
    let play = apply_double_play(&mut score, &mut bases, &std_rules());
    assert!(play.double_play);
    assert_eq!(play.runs, 1);
    assert_eq!(score.home_runs, 1);
    assert_eq!(bases, empty());
}

#[test]
fn inning_ending_double_play_scores_nothing() {
    // One out, runners on the corners: the DP is outs two and three — the
    // force ends the inning and the run never counts.
    let mut score = batting_home(1);
    let mut bases = with(&[0, 2]);
    let play = apply_double_play(&mut score, &mut bases, &std_rules());
    assert!(play.double_play);
    assert_eq!(play.runs, 0);
    assert_eq!(score.home_runs, 0);
    assert!(score.top_of_inning); // half flipped
    assert_eq!(score.inning, 2);
}

#[test]
fn double_play_with_two_outs_only_counts_the_force() {
    // The inning ends on the force at second; the relay out can't leak
    // into the next half.
    let mut score = batting_home(2);
    let mut bases = with(&[0]);
    let play = apply_double_play(&mut score, &mut bases, &std_rules());
    assert_eq!(play.outs, 1);
    assert_eq!(score.outs, 0); // half flipped cleanly
    assert!(score.top_of_inning);
}

#[test]
fn fielders_choice_trades_the_runner_for_the_batter() {
    let mut score = batting_home(0);
    let mut bases = with(&[0]);
    let play = apply_fielders_choice(&mut score, &mut bases, &std_rules(), 1);
    assert_eq!(play.outs, 1);
    assert_eq!(score.outs, 1);
    assert_eq!(bases, with(&[0])); // batter standing where the runner was
}

#[test]
fn fielders_choice_at_home_keeps_the_bases_loaded() {
    // Force at the plate with the bases full: the lead runner dies, the
    // rest move up behind the batter — still loaded, nobody scored.
    let mut score = batting_home(0);
    let mut bases = loaded();
    let count = bases.count();
    let play = apply_fielders_choice(&mut score, &mut bases, &std_rules(), count);
    assert_eq!(play.outs, 1);
    assert_eq!(play.runs, 0);
    assert_eq!(score.home_runs, 0);
    assert_eq!(bases, loaded());
}

#[test]
fn cut_down_stretching_keeps_the_runners_advance() {
    // Batter out trying to stretch a single with a runner on second: the
    // runner still moves up (and in from third would score).
    let mut score = batting_home(0);
    let mut bases = with(&[1]);
    let play = apply_batted_out(
        &mut score,
        &mut bases,
        &std_rules(),
        OutKind::Stretching { advanced: 1 },
        false,
    );
    assert_eq!(play.outs, 1);
    assert_eq!(bases, with(&[2]));
    assert_eq!(play.runs, 0);
}

#[test]
fn honest_ground_out_takes_one_and_advances_the_runner() {
    // The out at first is just the batter now — the runner moves up
    // behind the play instead of being doubled off by fiat.
    let mut score = batting_home(2);
    let mut bases = with(&[0]);
    let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, false);
    assert!(!play.double_play);
    assert_eq!(play.outs, 1);
    assert!(score.top_of_inning); // routine third out retires the side
}

#[test]
fn routine_ground_out_advances_the_runners() {
    // Runner on second, nobody out: the defense takes the out at first
    // and the runner moves up to third.
    let mut score = batting_home(0);
    let mut bases = with(&[1]);
    let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, false);
    assert_eq!((play.outs, play.runs), (1, 0));
    assert_eq!(bases, with(&[2]));
}

#[test]
fn deep_fly_is_a_sacrifice() {
    // Runners on second and third, nobody out: both tag up — third
    // scores, second takes third.
    let mut score = batting_home(0);
    let mut bases = with(&[1, 2]);
    let play = apply_batted_out(
        &mut score,
        &mut bases,
        &std_rules(),
        OutKind::Fly {
            deep: true,
            tag: TagUp::Scores,
        },
        false,
    );
    assert_eq!((play.outs, play.runs), (1, 1));
    assert_eq!(score.home_runs, 1);
    assert_eq!(bases, with(&[2]));
}

#[test]
fn shallow_fly_holds_the_runners() {
    let mut score = batting_home(0);
    let mut bases = with(&[2]);
    let play = apply_batted_out(
        &mut score,
        &mut bases,
        &std_rules(),
        OutKind::Fly {
            deep: false,
            tag: TagUp::Hold,
        },
        false,
    );
    assert_eq!((play.outs, play.runs), (1, 0));
    assert_eq!(bases, with(&[2]));
}

#[test]
fn runner_tagging_from_third_can_be_cut_down_at_the_plate() {
    // Runners on second and third, one out: the runner from third loses
    // the race home (two outs, the side retired), so nobody scores.
    let mut score = batting_home(1);
    let mut bases = with(&[1, 2]);
    let play = apply_batted_out(
        &mut score,
        &mut bases,
        &std_rules(),
        OutKind::Fly {
            deep: true,
            tag: TagUp::ThrownOut,
        },
        false,
    );
    assert!(play.thrown_out_home);
    assert_eq!((play.outs, play.runs), (2, 0));
    assert!(
        score.top_of_inning,
        "the second out on the play ends the half"
    );

    // Nobody out: the trailing runner moves up on the throw home.
    let mut score = batting_home(0);
    let mut bases = with(&[1, 2]);
    let play = apply_batted_out(
        &mut score,
        &mut bases,
        &std_rules(),
        OutKind::Fly {
            deep: true,
            tag: TagUp::ThrownOut,
        },
        false,
    );
    assert_eq!((play.outs, play.runs, score.outs), (2, 0, 2));
    assert_eq!(bases, with(&[2]));
}

#[test]
fn deep_fly_runner_on_second_holds_behind_a_held_third() {
    let mut score = batting_home(0);
    let mut bases = with(&[1, 2]);
    let play = apply_batted_out(
        &mut score,
        &mut bases,
        &std_rules(),
        OutKind::Fly {
            deep: true,
            tag: TagUp::Hold,
        },
        false,
    );
    assert_eq!((play.outs, play.runs), (1, 0));
    assert_eq!(bases, with(&[1, 2]));
}

#[test]
fn runner_scoring_from_second_on_a_single() {
    // Runners on first and second: the lead runner takes the extra base
    // home, the trailing runner and batter take the single's base.
    let mut score = batting_home(0);
    let mut bases = with(&[0, 1]);
    let (runs, outs) = apply_extra_base(&mut score, &mut bases, &std_rules(), 1, 1, true);
    assert_eq!((runs, outs), (1, 0));
    assert_eq!(score.home_runs, 1);
    assert_eq!(bases, with(&[0, 1]));
}

#[test]
fn runner_cut_down_going_first_to_third() {
    let mut score = batting_home(0);
    let mut bases = with(&[0]);
    let (runs, outs) = apply_extra_base(&mut score, &mut bases, &std_rules(), 1, 0, false);
    assert_eq!((runs, outs, score.outs), (0, 1, 1));
    assert_eq!(bases, with(&[0]), "the batter holds first");
}

#[test]
fn runs_ahead_of_a_third_out_at_the_plate_count() {
    // Two out, runners on first and third, a double: the runner from third
    // scores well ahead of the play, then the runner from first is cut
    // down trying to score — the run counts.
    let mut score = batting_home(2);
    let mut bases = with(&[0, 2]);
    let (runs, outs) = apply_extra_base(&mut score, &mut bases, &std_rules(), 2, 0, false);
    assert_eq!((runs, outs), (1, 1));
    assert_eq!(score.home_runs, 1);
    assert!(score.top_of_inning, "the out at the plate ends the half");
}

#[test]
fn two_out_deep_fly_ends_the_half_scoreless() {
    let mut score = batting_home(2);
    let mut bases = with(&[2]);
    let play = apply_batted_out(
        &mut score,
        &mut bases,
        &std_rules(),
        OutKind::Fly {
            deep: true,
            tag: TagUp::Hold,
        },
        false,
    );
    assert_eq!(play.runs, 0);
    assert_eq!(score.home_runs, 0);
    assert!(score.top_of_inning);
}

#[test]
fn charge_out_keeps_the_count_mid_at_bat() {
    // A runner thrown out on the bases is not the batter's at-bat ending.
    let mut score = ScoreBoard {
        balls: 2,
        strikes: 1,
        ..batting_home(0)
    };
    let mut bases = empty();
    charge_out(&mut score, &mut bases, &std_rules());
    assert_eq!((score.balls, score.strikes, score.outs), (2, 1, 1));
}

#[test]
fn runner_going_on_a_caught_fly_is_doubled_off() {
    let mut score = batting_home(0);
    let mut bases = with(&[0]);
    let play = apply_batted_out(
        &mut score,
        &mut bases,
        &std_rules(),
        OutKind::Fly {
            deep: false,
            tag: TagUp::Hold,
        },
        true,
    );
    assert!(play.doubled_off);
    assert_eq!(play.outs, 2);
    assert_eq!(bases, empty());
}

#[test]
fn runners_going_beat_the_double_play() {
    // The point of the hit-and-run: the grounder can't be turned two,
    // and the runner moves up.
    let mut score = batting_home(0);
    let mut bases = with(&[0]);
    let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, true);
    assert!(!play.double_play);
    assert_eq!(play.outs, 1);
    assert_eq!(bases, with(&[1]));
}

// ── Game end ──────────────────────────────────────────────────────────────

#[test]
fn walkoff_when_home_leads_in_bottom_of_final() {
    let score = ScoreBoard {
        home_runs: 3,
        away_runs: 2,
        inning: 9,
        top_of_inning: false,
        ..Default::default()
    };
    assert!(is_game_over(&score, 9));
}

#[test]
fn tie_after_regulation_goes_to_extras() {
    let score = ScoreBoard {
        home_runs: 2,
        away_runs: 2,
        inning: 10,
        top_of_inning: true,
        ..Default::default()
    };
    assert!(!is_game_over(&score, 9));
}

#[test]
fn one_inning_walkoff_ends_immediately() {
    let score = ScoreBoard {
        home_runs: 1,
        away_runs: 0,
        inning: 1,
        top_of_inning: false,
        ..Default::default()
    };
    assert!(is_game_over(&score, 1));
}

#[test]
fn one_inning_tie_goes_to_extras() {
    // Still tied in the bottom of the 1st: play on.
    let bottom = ScoreBoard {
        inning: 1,
        top_of_inning: false,
        ..Default::default()
    };
    assert!(!is_game_over(&bottom, 1));
    // Tied after a full inning: extras.
    let extras = ScoreBoard {
        inning: 2,
        top_of_inning: true,
        ..Default::default()
    };
    assert!(!is_game_over(&extras, 1));
}

#[test]
fn home_lead_entering_bottom_of_final_skips_the_half() {
    // Home led 2-0 when the top of the 6th ended: the bottom is never played.
    let score = ScoreBoard {
        home_runs: 2,
        away_runs: 0,
        inning: 6,
        top_of_inning: false,
        ..Default::default()
    };
    assert!(is_game_over(&score, 6));
}

#[test]
fn away_leads_after_bottom_nine_ends_game() {
    let score = ScoreBoard {
        home_runs: 1,
        away_runs: 4,
        inning: 10,
        top_of_inning: true,
        ..Default::default()
    };
    assert!(is_game_over(&score, 9));
}
