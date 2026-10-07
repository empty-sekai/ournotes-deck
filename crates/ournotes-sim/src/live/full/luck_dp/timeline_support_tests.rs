// Compare the complete support's actual edges with an independent unweighted native recording.
use super::*;
use crate::live::full::luck_dp::timeline_support::{TimelineKind, complete_timeline_support};
use crate::live::full::luck_score_bounds::BoundsEvent;

#[test]
fn timeline_support_keeps_native_same_frame_edges_probe_delay_and_finish() {
    for result in [0, 3] {
        let (master, notes, params, setup, play, delta) = fixture(result, 60);
        let skills = luck_skills(&master).unwrap();
        let deck = [Performer {
            gekisou_skill: Some((1, 1)),
            gekisou_support_skills: vec![(31, 1), (96, 1)],
            ..Default::default()
        }];
        let transcript =
            record::<ProbabilityMass>(&master, &skills, &notes, &[], params, &setup, &play, &delta, &deck, None, None)
                .unwrap();
        let times: Vec<_> = play.frames.iter().map(|frame| frame.time_ms).collect();
        let support = complete_timeline_support(&transcript, &times, true, &mut || false).unwrap().unwrap();
        assert_eq!(support.len(), 1, "the original nominal tables are deterministic");
        let path = support.path(0).unwrap();
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        native.score.begin_bounds(Vec::new(), true);
        let mut cursor = 0;
        let mut rush = Vec::new();
        let mut probes = Vec::new();
        for (frame, (input, &dt)) in play.frames.iter().zip(&delta).enumerate() {
            native.frame_timed(input.time_ms, &input.judged, dt).unwrap();
            let trace = native.score.bounds_trace.as_ref().unwrap();
            for event in &trace.events[cursor..] {
                if let BoundsEvent::Factor { command, .. } = event {
                    if command.owner_id == -1 && command.luck != 0 {
                        rush.push((frame, command.time_ms, command.luck > 0));
                    }
                    if command.owner_id == 2 && command.note_mill != 0 {
                        probes.push((frame, command.time_ms, command.note_mill > 0));
                    }
                }
            }
            cursor = trace.events.len();
        }
        let observed = |kind| {
            path.iter()
                .filter(|edge| edge.kind == kind)
                .map(|edge| (edge.frame, edge.chart_time, edge.on))
                .collect::<Vec<_>>()
        };
        assert_eq!(observed(TimelineKind::Rush), rush);
        assert_eq!(observed(TimelineKind::Probe), probes);
        if result == 3 {
            assert!(rush.iter().any(|edge| edge.2));
            assert!(probes.iter().any(|edge| edge.2));
            assert!(path.iter().any(|edge| edge.kind == TimelineKind::Rush && edge.stage == 0 && !edge.on));
        }
    }
}

#[test]
fn timeline_support_never_publishes_cancelled_or_incomplete_clock_coverage() {
    let (master, notes, params, setup, play, delta) = fixture(3, 60);
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((1, 1)), ..Default::default() }];
    let transcript =
        record::<ProbabilityMass>(&master, &skills, &notes, &[], params, &setup, &play, &delta, &deck, None, None)
            .unwrap();
    let times: Vec<_> = play.frames.iter().map(|frame| frame.time_ms).collect();
    assert!(complete_timeline_support(&transcript, &times[..times.len() - 1], false, &mut || false).is_err());
    let mut bad_origin = times.clone();
    bad_origin[0] += 1;
    assert!(complete_timeline_support(&transcript, &bad_origin, false, &mut || false).is_err());
    let mut polls = 0;
    assert!(
        complete_timeline_support(&transcript, &times, false, &mut || {
            polls += 1;
            polls >= 6
        })
        .unwrap()
        .is_none()
    );
    assert!(polls >= 6);
    assert!(complete_timeline_support(&transcript, &times, false, &mut || false).unwrap().is_some());
}
