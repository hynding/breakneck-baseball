use super::*;

fn facts(runs: u32, inning: u32) -> PlayFacts {
    PlayFacts {
        runs,
        inning,
        innings: 9,
        go_ahead: false,
        walk_off: false,
        home_run: false,
    }
}

#[test]
fn a_play_that_moves_nothing_weighs_nothing() {
    assert_eq!(play_weight(&facts(0, 9)), 0.0);
}

#[test]
fn later_runs_go_ahead_runs_and_walk_offs_weigh_more() {
    assert!(play_weight(&facts(1, 9)) > play_weight(&facts(1, 1)));
    let go_ahead = PlayFacts {
        go_ahead: true,
        ..facts(1, 5)
    };
    assert!(play_weight(&go_ahead) > play_weight(&facts(1, 5)));
    // A walk-off single beats an early grand slam.
    let walk_off = PlayFacts {
        walk_off: true,
        go_ahead: true,
        ..facts(1, 9)
    };
    let slam = PlayFacts {
        home_run: true,
        ..facts(4, 1)
    };
    assert!(play_weight(&walk_off) > play_weight(&slam));
}

#[test]
fn the_best_play_is_kept_and_named() {
    let mut best = BestPlay::default();
    assert_eq!(best.line(), None);
    best.offer(&facts(1, 2), "SINGLE  +1", true);
    best.offer(&facts(1, 1), "DOUBLE  +1", false); // lighter: ignored
    assert_eq!(best.line().as_deref(), Some("TOP 2  -  SINGLE  +1"));
    let slam = PlayFacts {
        home_run: true,
        ..facts(4, 7)
    };
    best.offer(&slam, "HOME RUN!  +4", false);
    assert_eq!(best.line().as_deref(), Some("BOT 7  -  HOME RUN!  +4"));
}
