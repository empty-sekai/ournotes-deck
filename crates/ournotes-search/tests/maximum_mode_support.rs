#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, roster, short_chart, synth_snaps};
use ournotes_search::{
    search::{
        Completion, Constraints, GekisouObjective, Objective, PlayInput, SearchRequest, SeedSet,
        solve_physical_with_aggregation,
    },
    types::{Aggregation, Limits, Metric, SimulationInput, Strategy},
};
use ournotes_sim::{
    Error,
    live::model::JudgementStream,
    pool::Pool,
    scenario::{ResolvedContext, Scenario},
};

#[test]
fn public_physical_search_rejects_maximum_gekisou_with_or_without_scenario() {
    let mut rng = Rng::new(8613);
    let mut master = synth_snaps(&mut rng, 5, 0, &[2000]).master();
    for member in &mut master.member_cards {
        member.character_id = member.id;
    }
    for music in &mut master.live_musics {
        music.gekisou_mission_1 = 0;
        music.gekisou_mission_2 = 0;
        music.gekisou_mission_3 = 0;
    }
    master.reindex().unwrap();
    let roster = roster(&mut rng, &master);
    let pool = Pool::new(&master, &roster).unwrap();
    let (chart, judgement_types) = short_chart(&mut rng, 8, false);
    let objective = Objective::LiveScore {
        score_id: 1004,
        play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types },
        chart,
        event: false,
        exclude_snap_skills: false,
        gekisou: Some(GekisouObjective { seeds: SeedSet::List(vec![1]), fevers: Vec::new() }),
    };
    let context =
        ResolvedContext::resolve(&master, Scenario::Mission(master.live_musics[0].id), Some(1004), &[], Vec::new())
            .unwrap();
    let mut ordinary = objective.clone();
    let Objective::LiveScore { gekisou, .. } = &mut ordinary else { unreachable!() };
    *gekisou = None;
    let run = |request: &SearchRequest, aggregation| {
        solve_physical_with_aggregation(
            &pool,
            request,
            &Metric::Score,
            None,
            &Limits { time_limit_ms: None, max_candidates: None, cache_entries: 0 },
            &Strategy::Exhaustive,
            None,
            &SimulationInput::default(),
            aggregation,
        )
    };
    for objective in [objective.clone(), objective.in_scenario(context)] {
        let request = SearchRequest { objective, k: 1, constraints: Constraints::default(), time_limit: None };
        assert!(
            matches!(run(&request, Aggregation::Maximum), Err(Error::Unsupported(message)) if message.contains("Gekisou lives"))
        );
    }
    let request = SearchRequest { objective: ordinary, k: 1, constraints: Constraints::default(), time_limit: None };
    let expected = run(&request, Aggregation::Expected).expect("ordinary Expected live");
    assert_eq!(expected.completion, Completion::Complete);
    assert_eq!(expected.results.len(), 1);
}
