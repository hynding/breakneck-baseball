use super::*;
use crate::game::Team;
use crate::game::fatigue::STARTER_STAMINA;

fn bullpen_app(cpu_fields: bool) -> App {
    let mut app = App::new();
    let mut controllers = Controllers::default();
    // The home side fields in the top of the first.
    if cpu_fields {
        controllers.home = InputSource::Cpu;
    }
    app.insert_resource(controllers)
        .insert_resource(ScoreBoard {
            inning: 1,
            top_of_inning: true,
            ..Default::default()
        })
        .init_resource::<Play>()
        .init_resource::<Fatigue>()
        .init_resource::<Rosters>()
        .add_message::<PlayBanner>()
        .add_systems(Update, cpu_bullpen);
    app
}

fn tire_the_starter(app: &mut App) -> u32 {
    let number = app.world().resource::<Rosters>().home.card(0).number;
    let mut fatigue = app.world_mut().resource_mut::<Fatigue>();
    for _ in 0..STARTER_STAMINA {
        fatigue.record_pitch(Team::Home, number);
    }
    number
}

#[test]
fn the_cpu_goes_to_the_bullpen_when_its_starter_is_spent() {
    let mut app = bullpen_app(true);
    app.update();
    let starter = app.world().resource::<Rosters>().home.card(0).number;
    let first_reliever = app.world().resource::<Rosters>().home.bench[0].number;
    tire_the_starter(&mut app);
    app.update();
    let rosters = app.world().resource::<Rosters>();
    assert_eq!(rosters.home.card(0).number, first_reliever);
    assert_eq!(rosters.home.bench[0].number, starter, "the starter sits");
    // The fresh arm isn't pulled again on the next frame.
    app.update();
    assert_eq!(
        app.world().resource::<Rosters>().home.card(0).number,
        first_reliever
    );
}

#[test]
fn never_mid_count_and_never_for_a_human_manager() {
    let mut app = bullpen_app(true);
    let starter = tire_the_starter(&mut app);
    app.world_mut().resource_mut::<ScoreBoard>().balls = 1;
    app.update();
    assert_eq!(
        app.world().resource::<Rosters>().home.card(0).number,
        starter
    );

    let mut human = bullpen_app(false);
    let starter = tire_the_starter(&mut human);
    human.update();
    assert_eq!(
        human.world().resource::<Rosters>().home.card(0).number,
        starter
    );
}
