use super::*;
use rank_residues::{Law, Output, Request};
use std::collections::BTreeMap;

type Fixture = (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>);

struct Case {
    master: Master,
    notes: Vec<LiveNote>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    delta: Vec<f32>,
    deck: Vec<Performer>,
}

impl Case {
    fn new((master, notes, params, setup, play, delta): Fixture) -> Self {
        Self { master, notes, params, setup, play, delta, deck: Vec::new() }
    }

    fn run(&self, requests: &[Request], cancelled: impl FnMut() -> bool) -> Result<Option<Output>, Error> {
        let skills = luck_skills(&self.master)?;
        rank_residues::probabilities(
            &self.master,
            &skills,
            &self.notes,
            &[],
            self.params,
            &self.setup,
            &self.play,
            &self.delta,
            &self.deck,
            None,
            requests,
            cancelled,
        )
    }

    fn complete(&self, requests: &[Request]) -> Output {
        self.run(requests, || false).unwrap().unwrap()
    }
}

fn request(modulus: u8, rewards: &[(i32, [Option<u8>; 4])]) -> Request {
    Request { modulus, rewards: rewards.iter().copied().collect() }
}

fn assert_point(law: &Law, residue: usize) {
    assert_eq!(law.unresolved, ProbabilityMass::ZERO);
    for (index, &mass) in law.residues.iter().enumerate() {
        assert_eq!(mass, if index == residue { ProbabilityMass::ONE } else { ProbabilityMass::ZERO });
    }
}

fn law_words(law: &Law) -> Vec<u64> {
    law.residues.iter().chain(std::iter::once(&law.unresolved)).flat_map(|mass| mass.bits()).collect()
}

#[test]
fn residue_observer_sees_both_weight_sites_even_when_every_marginal_is_unchanged() {
    let input = Case::new(fixture(3, 0));
    let skills = luck_skills(&input.master).unwrap();
    let transcript = record_frames::<ProbabilityMass>(
        &input.master,
        &skills,
        &input.notes,
        &[],
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        &input.deck,
        None,
        None,
    )
    .unwrap();
    struct Observations(Vec<(i32, bool)>);
    impl NoteObserver<ProbabilityMass> for Observations {
        fn observe(
            &mut self,
            time: i32,
            at_frame: bool,
            dp: &mut Dp<'_, ProbabilityMass>,
            _cancelled: &mut impl FnMut() -> bool,
        ) -> Result<bool, Error> {
            assert!(dp.dist.keys().all(|state| state.residue == 0));
            self.0.push((time, at_frame));
            Ok(true)
        }
    }
    let mut observer = Observations(Vec::new());
    let watched = propagate_observed_cancellable(&transcript, &mut || false, None, &mut observer).unwrap().unwrap();
    let ordinary = propagate(&transcript).unwrap();
    let expected: Vec<_> = input
        .play
        .frames
        .iter()
        .flat_map(|frame| {
            frame.judged.iter().map(|note| (note.judgement_time_ms, note.judgement_time_ms == frame.time_ms))
        })
        .collect();
    assert_eq!(observer.0, expected);
    assert!(observer.0.iter().any(|entry| entry.1) && observer.0.iter().any(|entry| !entry.1));
    assert_eq!(ordinary.steps.len(), 1);
    assert_eq!(watched.steps, ordinary.steps);
    assert_eq!((watched.peak_states, watched.transitions), (ordinary.peak_states, ordinary.transitions));
    let output = input.complete(&[request(7, &[(100, [Some(1); 4]), (110, [Some(2); 4]), (200, [Some(3); 4])])]);
    assert_point(output.laws[0].as_ref().unwrap(), 6);
}

#[test]
fn residue_classes_keep_native_frame_probe_delay_and_chart_time_rush() {
    let mut input = Case::new(fixture(3, 60));
    input.deck =
        vec![Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let skills = luck_skills(&input.master).unwrap();
    let native = luck::luck_rush_samples(
        &input.master,
        &skills,
        &input.notes,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        &input.deck,
        None,
        &[0, 1, 7],
    )
    .unwrap();
    let class_at = |time| {
        let values = &native[native.partition_point(|entry| entry.0 <= time) - 1].1;
        assert!(values.iter().all(|&value| value == 0.0 || value == 1.0));
        2 * usize::from(values[0] == 1.0) + usize::from(values[1] == 1.0)
    };
    assert_ne!(class_at(110), class_at(200), "the fixture must exercise the delayed score-probe update");
    let times = [100, 110, 200];
    let requests: Vec<_> =
        times.iter().map(|&time| request(7, &[(time, [Some(1), Some(2), Some(3), Some(4)])])).collect();
    let output = input.complete(&requests);
    for (index, time) in times.into_iter().enumerate() {
        assert_point(output.laws[index].as_ref().unwrap(), class_at(time) + 1);
    }
}

#[test]
fn residue_laws_preserve_joint_rush_history_against_all_native_branches() {
    let mut input = Case::new(fixture(0, 140));
    for kind in 0..5 {
        input.master.gekisou_luck_bonus_lots.push(crate::master::LuckBonusLotRow {
            id: 100 + kind,
            chance_lot_type: kind,
            lot_result: 3,
            weight: 1,
        });
    }
    input.master.note_parameters.push(crate::master::NoteParameterRow {
        id: 122,
        note_operate_type: 122,
        score_percent: 100,
    });
    input.master.reindex().unwrap();
    input.notes.retain(|note| [100, 200, 300].contains(&note.time_ms));
    for note in &mut input.notes {
        if note.time_ms != 100 {
            note.note_operate_type = 122;
        }
    }
    let ids: FxHashSet<_> = input.notes.iter().map(|note| note.note_id).collect();
    for frame in &mut input.play.frames {
        frame.judged.retain(|note| ids.contains(&note.note_id));
    }
    input.params.converted_note_count = input.notes.len() as i32;
    let requests = [
        request(5, &[(200, [Some(0), Some(0), Some(1), Some(1)]), (300, [Some(0), Some(0), Some(1), Some(1)])]),
        request(5, &[(200, [Some(0), Some(0), None, None]), (300, [Some(2); 4])]),
    ];
    let output = input.complete(&requests);
    let known = output.laws[0].as_ref().unwrap();
    let partial = output.laws[1].as_ref().unwrap();
    let mut counts = [0u32; 5];
    let mut unresolved = 0;
    // The first actual result and its prepared successor are two independent fair nominal draws. There are
    // no later gauge-producing notes; the two observed Rush flags retain that same first random outcome.
    for first in 0..2 {
        for next in 0..2 {
            let mut native =
                LiveModel::new_gekisou(&input.master, &input.deck, &input.notes, &[], input.params, &input.setup)
                    .unwrap();
            native
                .run_with_random(
                    &input.play,
                    &input.delta,
                    crate::live::random::LiveRandom::with_nominal_prefix(vec![first, next]),
                )
                .unwrap();
            assert!(native.random.nominal_prefix_consumed() && native.random.nominal_covers_draws());
            let spans = native.rush_spans();
            let flags = [200, 300].map(|time| spans.iter().any(|&(start, end)| start <= time && time < end));
            assert_eq!(flags[0], flags[1]);
            counts[usize::from(flags[0]) + usize::from(flags[1])] += 1;
            unresolved += u32::from(flags[0]);
        }
    }
    assert_eq!(counts, [2, 0, 2, 0, 0]);
    assert_eq!(unresolved, 2);
    assert_eq!(known.unresolved, ProbabilityMass::ZERO);
    for (index, &count) in counts.iter().enumerate() {
        assert!(known.residues[index].interval().contains(f64::from(count) / 4.0));
        if count == 0 {
            assert_eq!(known.residues[index], ProbabilityMass::ZERO);
        }
    }
    assert!(partial.unresolved.interval().contains(0.5));
    for (index, &mass) in partial.residues.iter().enumerate() {
        if index == 2 {
            assert!(mass.interval().contains(0.5));
        } else {
            assert_eq!(mass, ProbabilityMass::ZERO);
        }
    }
}

#[test]
fn residue_windows_decline_independently_and_keep_unknown_rewards_absorbing() {
    let input = Case::new(fixture(3, 0));
    let requests = [
        request(7, &[(100, [Some(2), None, None, None]), (200, [Some(3); 4])]),
        request(7, &[(100, [None, Some(2), Some(3), Some(4)]), (200, [Some(3); 4])]),
        request(7, &[(150, [Some(1); 4])]),
        request(0, &[(100, [Some(0); 4])]),
        request(3, &[(100, [Some(3); 4])]),
        request(255, &[(100, [Some(254); 4]), (200, [Some(254); 4])]),
        request(255, &[(100, [None; 4]), (200, [Some(254); 4])]),
        Request { modulus: 7, rewards: BTreeMap::new() },
    ];
    let output = input.complete(&requests);
    assert_point(output.laws[0].as_ref().unwrap(), 5);
    let unresolved = output.laws[1].as_ref().unwrap();
    assert!(unresolved.residues.iter().all(|&mass| mass == ProbabilityMass::ZERO));
    assert_eq!(unresolved.unresolved, ProbabilityMass::ONE);
    assert!(output.laws[2..5].iter().all(Option::is_none));
    assert_point(output.laws[5].as_ref().unwrap(), 253);
    assert_eq!(output.laws[6].as_ref().unwrap().unresolved, ProbabilityMass::ONE);
    assert_point(output.laws[7].as_ref().unwrap(), 0);
}

#[test]
fn residue_bins_are_saved_at_the_last_observation_before_future_rounding() {
    let mut input = Case::new(random_fixture());
    input.master.reindex().unwrap();
    input.deck =
        vec![Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let requests = [request(5, &[(100, [Some(0), Some(1), Some(2), Some(3)]), (200, [Some(1); 4])])];
    let full = input.complete(&requests);
    let last = input.play.frames.iter().position(|frame| frame.time_ms == 200).unwrap() + 1;
    input.play.frames.truncate(last);
    input.delta.truncate(last);
    let prefix = input.complete(&requests);
    assert_eq!(law_words(full.laws[0].as_ref().unwrap()), law_words(prefix.laws[0].as_ref().unwrap()));
    assert!(full.transitions > prefix.transitions);
}

#[test]
fn residue_cancellation_never_returns_a_partial_law_and_native_errors_remain_errors() {
    let mut input = Case::new(random_fixture());
    input.master.reindex().unwrap();
    let requests = [request(7, &[(100, [Some(1); 4]), (200, [Some(2); 4]), (500, [Some(3); 4])])];
    let mut checks = 0;
    let completed = input
        .run(&requests, || {
            checks += 1;
            false
        })
        .unwrap()
        .unwrap();
    assert!(checks > 8);
    for stop in [1, 2, 3, checks / 2, checks - 1, checks] {
        let mut seen = 0;
        assert!(
            input
                .run(&requests, || {
                    seen += 1;
                    seen >= stop
                })
                .unwrap()
                .is_none()
        );
    }
    let retry = input.complete(&requests);
    assert_eq!(law_words(completed.laws[0].as_ref().unwrap()), law_words(retry.laws[0].as_ref().unwrap()));
    input.play.frames.iter_mut().find(|frame| !frame.judged.is_empty()).unwrap().judged[0].note_id = i32::MAX;
    assert!(matches!(input.run(&requests, || false), Err(Error::Input(_))));
}
