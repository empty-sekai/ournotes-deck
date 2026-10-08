//! Optional integer command-work evidence from an already completed controller transcript.
use super::*;

/// No held probe is neutral. Otherwise every holder has a fixed-true condition and the same legal phase.
/// The caller intersects this check across the complete physical pair domain, not only the requested writers.
pub(super) fn phase(model: &LiveModel, skills: &LuckSkills) -> Option<Option<i64>> {
    if !super::super::super::luck_score_bounds::bind_probe_phase(model, skills) {
        return None;
    }
    let rows = model.luck_score_rows(skills);
    let mut phase = None;
    for effect in model.cond.iter().flat_map(|skill| skill.updater.effects()) {
        if rows.iter().any(|row| row.row == effect.row && row.may_hold) {
            if !matches!(effect.phase, 1 | 2) || phase.is_some_and(|old| old != effect.phase) {
                return None;
            }
            phase = Some(effect.phase);
        }
    }
    Some(phase)
}

/// The edge at each ORIGINAL play frame is observed at its skill boundary. Several notes or pending draws in
/// one frame do not create extra probe observations; quiet/repeated frames retain their own identity edges.
/// Native probes begin false. A direct untimed holder with no release/reset/limit starts at most once at each
/// false-to-true edge and files at most one start and one end command for that activation. Ending false closes
/// those pairs. Requiring only false-to-false edges after the music clamp excludes backdated inverse lifetimes.
/// These edges retain possible mass, not probabilities: max-plus stitching deliberately admits extra paths.
pub(super) fn maximum(edges: &[u8], expected_frames: usize, first_late_frame: usize) -> Option<u64> {
    if expected_frames == 0 || edges.len() != expected_frames || first_late_frame > expected_frames {
        return None;
    }
    let mut best = [Some(0u64), None];
    for (frame, &mask) in edges.iter().enumerate() {
        if !(1..=15).contains(&mask) || (frame >= first_late_frame && mask != 0b0001) {
            return None;
        }
        let mut next = [None, None];
        for (old, value) in best.into_iter().enumerate() {
            let Some(value) = value else { continue };
            for (new, target) in next.iter_mut().enumerate() {
                if mask & (1 << (2 * old + new)) != 0 {
                    let value = value.checked_add(u64::from(old == 0 && new == 1))?;
                    *target = Some(target.map_or(value, |old: u64| old.max(value)));
                }
            }
        }
        if next == [None, None] {
            return None;
        }
        best = next;
    }
    if best[1].is_some() { None } else { best[0] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enumerate(edges: &[u8]) -> Option<u64> {
        let mut maximum = None;
        for states in 0..1usize << edges.len() {
            let (mut old, mut runs) = (0, 0);
            let mut valid = true;
            for (frame, &mask) in edges.iter().enumerate() {
                let new = (states >> frame) & 1;
                valid &= mask & (1 << (2 * old + new)) != 0;
                runs += u64::from(old == 0 && new == 1);
                old = new;
            }
            if valid {
                if old != 0 {
                    return None;
                }
                maximum = Some(maximum.map_or(runs, |old: u64| old.max(runs)));
            }
        }
        maximum
    }

    #[test]
    fn profile_probe_runs_max_plus_matches_independent_complete_binary_paths() {
        for a in 1..=15 {
            for b in 1..=15 {
                for c in 1..=15 {
                    for d in 1..=15 {
                        let edges = [a, b, c, d];
                        assert_eq!(maximum(&edges, 4, 4), enumerate(&edges), "{edges:?}");
                    }
                }
            }
        }
        assert_eq!(maximum(&[2, 8, 4, 1], 4, 4), Some(1));
        assert_eq!(maximum(&[2, 4, 2, 4], 4, 4), Some(2));
        // The union is true throughout both paths, yet an individual path can have two starts.
        assert_eq!(maximum(&[3, 15, 15, 5], 4, 4), Some(2));
    }

    #[test]
    fn profile_probe_runs_require_complete_clock_closed_lifetimes_and_inactive_clamped_tail() {
        assert_eq!(maximum(&[2, 4, 1, 1], 4, 2), Some(1));
        for edges in [&[][..], &[0][..], &[16][..], &[4][..], &[2][..], &[3][..]] {
            assert_eq!(maximum(edges, edges.len(), edges.len()), None, "{edges:?}");
        }
        assert_eq!(maximum(&[2, 4, 1, 1], 3, 2), None);
        assert_eq!(maximum(&[2, 4, 1, 1], 4, 5), None);
        assert_eq!(maximum(&[2, 4, 1, 1], 4, 1), None, "late inverse may be backdated");
        assert_eq!(maximum(&[2, 4, 9, 1], 4, 2), None, "every late edge must stay off");
    }
}
