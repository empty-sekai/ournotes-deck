use super::factors::network_rank_factors;
use super::*;

#[test]
fn network_snapshot_envelope_covers_negative_retroactive_range_scores() {
    // Snapshot raw values can vary independently inside the per-note cap. Include negative early bonuses
    // read by later start snapshots, late packets, two ranks applied together, and notes judged after an end.
    let frames = [0, 2, 4, 7];
    let snapshots = [(3, 0, Some(1), Some(2), 150), (6, 1, Some(4), Some(5), 70), (6, 2, Some(2), Some(4), 30)];
    let caps = [31.0, 67.0, 43.0, 101.0];
    let factors = network_rank_factors(&frames, &[0; 4], &[i32::MIN; 8], &snapshots);
    let bound: f64 = factors.iter().zip(caps).map(|(r, v)| r * v).sum();
    for bits in 0..4096u64 {
        let mut bonus = Vec::new();
        for (i, &(_, _, start, end, pct)) in snapshots.iter().enumerate() {
            let raw = |at: Option<usize>, start: bool, shift: usize| -> f64 {
                frames
                    .iter()
                    .zip(caps)
                    .enumerate()
                    .filter(|(e, (frame, _))| {
                        at.is_some_and(|at| if start { **frame < at } else { **frame <= at })
                            && (bits >> ((shift + e) % 12)) & 1 != 0
                    })
                    .map(|(_, (_, cap))| cap)
                    .sum()
            };
            let mut s0 = raw(start, true, i * 4);
            let mut s1 = raw(end, false, i * 4 + 2);
            for (j, &(applied, ..)) in snapshots[..i].iter().enumerate() {
                if start.is_some_and(|start| applied < start) {
                    s0 += bonus[j];
                }
                if end.is_some_and(|end| applied < end) {
                    s1 += bonus[j];
                }
            }
            bonus.push(((s1 - s0) * pct as f64 / 100.0).trunc());
        }
        let score = caps.iter().sum::<f64>() + bonus.iter().sum::<f64>();
        assert!(score <= bound, "{bits}: {score} > {bound}");
    }
    assert!(factors[3] < 1.000001, "notes after every end receive no rank contribution");
}

#[test]
fn network_snapshot_envelope_cancels_held_notes_and_filed_bonuses() {
    // A note changes only in its judgement frame and in play frames whose calculations re-execute its score
    // frame. A bonus files with the calculation after its application, or (a stronger adversary) at once.
    let judged = [0, 1, 3, 5, 8];
    let score_frames = [0, 2, 4, 6, 9];
    let floors = [0, 0, 0, 3, 3, 3, 5, 5, 5, 8];
    let snapshots = [(2, 0, Some(1), Some(2), 150), (6, 1, Some(5), Some(6), 80), (7, 2, Some(6), Some(7), 40)];
    let caps = [31.0, 67.0, 43.0, 101.0, 59.0];
    let factors = network_rank_factors(&judged, &score_frames, &floors, &snapshots);
    let bound: f64 = factors.iter().zip(caps).map(|(r, v)| r * v).sum();
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..20_000 {
        // value[e][k]: the note's value after the calculations of play frame k
        let mut value = vec![vec![0.0f64; floors.len()]; caps.len()];
        for (e, row) in value.iter_mut().enumerate() {
            let mut v = 0.0;
            for (k, slot) in row.iter_mut().enumerate() {
                let can = k == judged[e] || (k > judged[e] && floors[k] <= score_frames[e]);
                if can {
                    v = caps[e] * (next() % 5) as f64 / 4.0;
                }
                *slot = if k >= judged[e] { v } else { 0.0 };
            }
        }
        let at_once: Vec<bool> = snapshots.iter().map(|_| next() & 1 != 0).collect();
        let mut bonus: Vec<f64> = Vec::new();
        for (i, &(_, _, start, end, pct)) in snapshots.iter().enumerate() {
            let (s, e) = (start.unwrap(), end.unwrap());
            // The start snapshot opens play frame `s`, after the applications of frame `s - 1`; the end
            // snapshot precedes the applications of frame `e`.
            let total = |k: usize, filed: &dyn Fn(usize) -> bool| -> f64 {
                value.iter().map(|row| row[k]).sum::<f64>()
                    + (0..i).filter(|&j| filed(j)).map(|j| bonus[j]).sum::<f64>()
            };
            let s0 = total(s - 1, &|j| s - 1 > snapshots[j].0 || (at_once[j] && s - 1 == snapshots[j].0));
            let s1 = total(e, &|j| e > snapshots[j].0);
            bonus.push(((s1 - s0) * pct as f64 / 100.0).trunc());
        }
        let last = floors.len() - 1;
        let score = value.iter().map(|row| row[last]).sum::<f64>() + bonus.iter().sum::<f64>();
        assert!(score <= bound, "{score} > {bound}");
    }
    // The first note moves only within the first range; the first bonus is in both snapshots of the later ones.
    assert!(factors[0] < 2.51, "{factors:?}");
    assert!(factors[4] < 1.000001);
}

#[test]
fn gated_bonus_opens_once_the_time_group_can_reach_the_threshold() {
    // one ordered combo range of four entries, two of them at the same chart time
    let times = [10, 20, 20, 30];
    let gc = GkCombo {
        range: vec![Some(0); 4],
        upto: vec![0; 4],
        entries: vec![vec![0, 1, 2, 3]],
        ordered: vec![true],
        tab: Vec::new(),
        gmax: 1.0,
        frame: vec![0; 4],
        read: Vec::new(),
        run: Vec::new(),
    };
    // a +5 bonus gated at a count of 3 in range 0
    let w: ComboBonusRow = (0, 100, 5.0, Some((3, 0)));
    let (mut out, mut sums) = (Vec::new(), Vec::new());
    gc.fill(
        &times,
        |ri, _, acc, group| if gate_open(w.3, ri, acc, group, gate_step(w.2, w.2)) { w.2 } else { 0.0 },
        &mut out,
        &mut sums,
    );
    // its own bonus never counts toward its threshold: closed for the first entry (0 + 1 < 3); the pair at 20 can
    // reach it (1 + 2 * 1), so both count the bonus
    assert_eq!(sums, vec![0.0, 1.0, 7.0, 13.0, 19.0]);
    assert_eq!(gate_step(5.0, 5.0), 1.0);
    assert_eq!(gate_step(7.5, 5.0), 3.0);
    // a gate naming another range never closes
    assert!(gate_open(Some((1000, 1)), 0, 0.0, 1, 1.0));
    assert!(gate_open(None, 0, 0.0, 1, 1.0));
}

#[test]
fn carrier_factors_and_counts_cover_every_deck_with_that_many_carriers() {
    // two ordered combo ranges, entries at repeated chart times, a table rising to 1.3
    let times: Vec<i32> = (0..40).map(|i| 100 + 40 * (i / 2) + if i >= 20 { 1000 } else { 0 }).collect();
    let gc = GkCombo {
        range: (0..40).map(|i| Some(u32::from(i >= 20))).collect(),
        upto: (0..40).map(|i| (i % 20) as u32 / 2 * 2).collect(),
        entries: vec![(0..20).collect(), (20..40).collect()],
        ordered: vec![true, true],
        tab: vec![(5, 1.05), (20, 1.1), (60, 1.2), (150, 1.3)],
        gmax: 1.3,
        frame: vec![0; 40],
        read: Vec::new(),
        run: Vec::new(),
    };
    // per member, its window lists: plain and gated bonuses of different lengths
    let mut seed = 7u64;
    let mut next = move |m: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % m
    };
    let members: Vec<Vec<Vec<ComboBonusRow>>> = (0..7)
        .map(|_| {
            (0..1 + next(2))
                .map(|_| {
                    (0..1 + next(2))
                        .map(|_| {
                            let a = 100 + 40 * next(25) as i64;
                            let gate = (next(2) == 0).then(|| (1 + next(12) as i64, next(2) as u32));
                            (a, a + 40 * next(12) as i64, (1 + next(5)) as f64, gate)
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    let (bound, sums): (Vec<Vec<f64>>, Vec<Vec<f64>>) = (0..=5)
        .map(|n| {
            let (mut v, mut c) = (Vec::new(), Vec::new());
            carrier_factors(&gc, &times, &members, n, &mut v, &mut c);
            (v, c)
        })
        .unzip();
    for n in 0..5 {
        assert!(bound[n].iter().zip(&bound[n + 1]).all(|(a, b)| a <= b), "{n}");
        assert!(sums[n].iter().zip(&sums[n + 1]).all(|(a, b)| a <= b), "{n}");
    }
    // every deck: a subset of the members, each with one of its lists, through the candidate fill
    for mask in 0usize..1 << members.len() {
        if mask.count_ones() > 5 {
            continue;
        }
        let picks: Vec<usize> = (0..members.len()).filter(|&m| mask & (1 << m) != 0).collect();
        for choice in 0..1usize << picks.len() {
            let windows: Vec<ComboBonusRow> = picks
                .iter()
                .enumerate()
                .flat_map(|(i, &m)| members[m][((choice >> i) & 1).min(members[m].len() - 1)].iter().copied())
                .collect();
            let (mut own, mut own_sums) = (Vec::new(), Vec::new());
            gc.fill(
                &times,
                |ri, q, acc, group| {
                    let t = times[gc.entries[ri][q] as usize] as i64;
                    let running = || windows.iter().filter(|w| w.0 <= t && t <= w.1);
                    let all = running().map(|w| w.2).sum::<f64>();
                    running().filter(|w| gate_open(w.3, ri, acc, group, gate_step(all, w.2))).map(|w| w.2).sum()
                },
                &mut own,
                &mut own_sums,
            );
            let level = &bound[picks.len()];
            assert!(own.iter().zip(level).all(|(a, b)| a <= b), "{picks:?} {choice}");
            assert!(own_sums.iter().zip(&sums[picks.len()]).all(|(a, b)| a <= b), "{picks:?} {choice}");
            // any of its carriers placed with their lists, the others among the slots to fill
            for placed in 0usize..1 << picks.len() {
                let lists: Vec<&[ComboBonusRow]> = picks
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| placed & (1 << i) != 0)
                    .map(|(i, &m)| &members[m][((choice >> i) & 1).min(members[m].len() - 1)][..])
                    .collect();
                let (mut keyed, mut keyed_sums) = (Vec::new(), Vec::new());
                let r = picks.len() - lists.len();
                keyed_factors(&gc, &times, &members, &lists, r, &mut keyed, &mut keyed_sums);
                assert!(own.iter().zip(&keyed).all(|(a, b)| a <= b), "{picks:?} {choice} {placed}");
                assert!(own_sums.iter().zip(&keyed_sums).all(|(a, b)| a <= b), "{picks:?} {choice} {placed}");
            }
        }
    }
    // fewer carriers really count less here
    assert!(bound[1].iter().sum::<f64>() < bound[5].iter().sum::<f64>());
    assert!(sums[1].iter().sum::<f64>() < sums[5].iter().sum::<f64>());
}

#[test]
fn ramp_count_reads_the_count_of_the_frame_the_factor_comes_from() {
    // entries at 100, 200, 300, 400 judged in the first frame at or after their time; frames every 60 ms
    let frames: Vec<i32> = (0..9).map(|f| 60 * f).collect();
    let times = [100, 200, 300, 400];
    let judged: Vec<usize> = times.iter().map(|&t| frames.partition_point(|&x| x < t)).collect();
    assert_eq!(judged, vec![2, 4, 5, 7]);
    let entries = vec![vec![0u32, 1, 2, 3]];
    let current = vec![Some(0); frames.len()];
    let (frame, read, run) = GkCombo::reads(&entries, &[true], &times, &judged, &frames, &current);
    let gc = GkCombo {
        range: vec![Some(0); 4],
        upto: vec![0; 4],
        entries,
        ordered: vec![true],
        tab: Vec::new(),
        gmax: 1.0,
        frame,
        read,
        run,
    };
    let sums = [0.0, 1.0, 2.0, 3.0, 4.0];
    let at = |starts: &RampStarts| -> Vec<f64> {
        (0..4).map(|e| gc.ramp_count(&sums, starts, e, times[e] as i64)).collect()
    };
    // one execution running from the start: the count of the last frame up to each entry, never the entry itself
    assert_eq!(at(&RampStarts::new(vec![(i64::MIN, 0)])), vec![0.0, 1.0, 2.0, 3.0]);
    // a first factor backdated to 100 from frame 7 carries the count of frame 7 to the earlier entries
    assert_eq!(at(&RampStarts::new(vec![(100, 7)])), vec![3.0, 3.0, 3.0, 3.0]);
    // nothing started by the entry's time is read as zero
    assert_eq!(at(&RampStarts::new(vec![(250, 6)])), vec![0.0, 0.0, 3.0, 3.0]);
}

#[test]
fn ramp_count_covers_every_start_it_may_read() {
    let mut seed = 11u64;
    let mut next = move |m: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % m
    };
    for _ in 0..300 {
        // two ordered combo ranges with sums from random steps; playing ranges in runs, some without one
        let nf = 30usize;
        let frames: Vec<i32> = (0..nf as i32).map(|f| 50 * f).collect();
        let ne = 20usize;
        let mut times: Vec<i32> = (0..ne).map(|_| next(1450) as i32).collect();
        times.sort_unstable();
        let judged: Vec<usize> =
            times.iter().map(|&t| (frames.partition_point(|&x| x < t) + next(2) as usize).min(nf - 1)).collect();
        let mut judged_sorted = judged.clone();
        judged_sorted.sort_unstable();
        let judged = judged_sorted;
        let split = 1 + next(ne as u64 - 1) as usize;
        let entries: Vec<Vec<u32>> = vec![(0..split as u32).collect(), (split as u32..ne as u32).collect()];
        let mut current = vec![None; nf];
        let mut f = 0;
        while f < nf {
            let len = 1 + next(6) as usize;
            let r = match next(4) {
                0 => None,
                1 => Some(0),
                _ => Some(1),
            };
            for c in current.iter_mut().skip(f).take(len) {
                *c = r;
            }
            f += len;
        }
        let (frame, read, run) = GkCombo::reads(&entries, &[true, true], &times, &judged, &frames, &current);
        let gc = GkCombo {
            range: Vec::new(),
            upto: Vec::new(),
            entries: entries.clone(),
            ordered: vec![true, true],
            tab: Vec::new(),
            gmax: 1.0,
            frame,
            read,
            run,
        };
        let mut sums = Vec::new();
        for list in &entries {
            let mut acc = 0.0;
            sums.push(acc);
            for _ in list {
                acc += (1 + next(4)) as f64;
                sums.push(acc);
            }
        }
        // the count frame `f` reads: the playing range's sum over its entries judged before `f`
        let count = |f: usize| -> f64 {
            match current[f] {
                None => 0.0,
                Some(r) => {
                    let at = if r == 0 { 0 } else { entries[0].len() + 1 };
                    let k = entries[r].iter().rposition(|&e| judged[e as usize] < f).map_or(0, |q| q + 1);
                    sums[at + k]
                }
            }
        };
        let starts: Vec<(i64, usize)> =
            (0..1 + next(4)).map(|_| (next(1500) as i64 - 50, next(nf as u64) as usize)).collect();
        let rs = RampStarts::new(starts.clone());
        for e in 0..ne {
            let t = times[e] as i64;
            let last = frames.partition_point(|&x| x as i64 <= t) as i64 - 1;
            let exact = starts
                .iter()
                .filter(|s| s.0 <= t)
                .map(|&(_, f)| count((f as i64).max(last) as usize))
                .fold(0f64, f64::max);
            let bound = gc.ramp_count(&sums, &rs, e, t);
            assert!(bound >= exact, "{e}: {bound} < {exact}");
            if starts.len() == 1 {
                assert_eq!(bound, exact, "{e}");
            }
        }
    }
}
