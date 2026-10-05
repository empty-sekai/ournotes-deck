//! Ideal factor magnitudes over every filing version and native execution prefix.
//!
//! The native calculator executes in (score frame, chart time, owner, filing order). A late filing can
//! change an earlier prefix, so the final command set alone is insufficient. Activate one leaf per
//! filing and retain the maximum absolute prefix of every version. This bounds REAL sums only;
//! native binary32 apply/diff/undo rounding is still charged separately by `Operations`.

use super::*;

#[derive(Clone, Copy)]
struct Prefix {
    sum: [F64Interval; FIELDS],
    min: [f64; FIELDS],
    max: [f64; FIELDS],
}

impl Prefix {
    const ZERO: Self = Self { sum: [F64Interval::ZERO; FIELDS], min: [0.0; FIELDS], max: [0.0; FIELDS] };

    fn leaf(command: &FactorCommand) -> Result<Self, Error> {
        let mut node = Self::ZERO;
        for (field, delta) in deltas(command).into_iter().enumerate() {
            node.sum[field] = F64Interval::point(delta)?;
            node.min[field] = delta.min(0.0);
            node.max[field] = delta.max(0.0);
        }
        Ok(node)
    }

    fn join(left: Self, right: Self) -> Result<Self, Error> {
        let mut out = Self::ZERO;
        for field in 0..FIELDS {
            out.sum[field] = left.sum[field].add(right.sum[field]);
            let suffix = left.sum[field].add(F64Interval::new(right.min[field], right.max[field])?);
            out.min[field] = left.min[field].min(suffix.lower());
            out.max[field] = left.max[field].max(suffix.upper());
        }
        Ok(out)
    }
}

pub(super) fn fixed_magnitudes(trace: &BoundsTrace) -> Result<[f64; FIELDS], Error> {
    let commands: Vec<_> = trace
        .events
        .iter()
        .enumerate()
        .filter_map(|(event, value)| match value {
            BoundsEvent::Factor { frame, command } if deltas(command).iter().any(|v| *v != 0.0) => {
                Some((event, *frame, command))
            }
            _ => None,
        })
        .collect();
    let mut sorted: Vec<_> = (0..commands.len()).collect();
    sorted.sort_by_key(|&i| {
        let (event, frame, command) = commands[i];
        (frame, command.time_ms, command.owner_id, event)
    });
    let size = commands.len().next_power_of_two().max(1);
    let mut tree = vec![Prefix::ZERO; size * 2];
    let mut positions = vec![0; commands.len()];
    for (position, &index) in sorted.iter().enumerate() {
        positions[index] = size + position;
    }
    let initial = initial_note_factors();
    let mut maximum = [0.0f64; FIELDS];
    maximum[1] = 1.0;
    for (index, &(_, _, command)) in commands.iter().enumerate() {
        let mut position = positions[index];
        tree[position] = Prefix::leaf(command)?;
        while position > 1 {
            position /= 2;
            tree[position] = Prefix::join(tree[position * 2], tree[position * 2 + 1])?;
        }
        for field in 0..FIELDS {
            let range = initial[field].add(F64Interval::new(tree[1].min[field], tree[1].max[field])?);
            maximum[field] = maximum[field].max(range.lower().abs()).max(range.upper().abs());
        }
    }
    Ok(maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(commands: Vec<FactorCommand>) -> BoundsTrace {
        BoundsTrace {
            events: commands
                .into_iter()
                .map(|command| BoundsEvent::Factor { frame: get_frame(command.time_ms).max(0) as usize, command })
                .collect(),
            frames: 8,
            queries: 0,
            optional_note_factors: Vec::new(),
            combo: Default::default(),
            has_luck: true,
        }
    }

    #[test]
    fn late_negative_filing_cannot_hide_the_earlier_positive_prefix() {
        let command =
            |time_ms, owner_id, note_mill| FactorCommand { time_ms, owner_id, note_mill, ..Default::default() };
        let trace = trace(vec![command(80, 1, 10_000_000), command(0, 1, -10_000_000)]);
        let bound = fixed_magnitudes(&trace).unwrap()[1];
        assert!((101.0..102.0).contains(&bound), "must cover first filing, not only final time-prefix");
        // The old total-absolute-value bound was 201, even though no version/prefix reaches it.
        assert!(bound < 201.0);
    }

    #[test]
    fn every_filing_version_and_partial_frame_owner_prefix_is_enclosed() {
        let commands: Vec<_> = (0..97)
            .map(|index| FactorCommand {
                time_ms: (index * 37 % 200) - 10,
                owner_id: index % 5,
                note_mill: [100_000, -200_000, 400_000, -100_000][index as usize % 4],
                combo_mill: [800_000, -400_000, -800_000][index as usize % 3],
                judgement: 5,
                judge_mill: (index % 9 - 4) * 100_000,
                ..Default::default()
            })
            .collect();
        let bound = fixed_magnitudes(&trace(commands.clone())).unwrap();
        for count in 0..=commands.len() {
            let mut prefix: Vec<_> = commands[..count].iter().enumerate().collect();
            prefix.sort_by_key(|&(index, c)| (get_frame(c.time_ms).max(0), c.time_ms, c.owner_id, index));
            let mut ideal = [0.0; FIELDS];
            ideal[1] = 1.0;
            for (_, command) in prefix {
                for (field, delta) in deltas(command).into_iter().enumerate() {
                    ideal[field] += delta; // these small integer deltas sum exactly in binary64
                    assert!(ideal[field].abs() <= bound[field], "filings={count}, field={field}");
                }
            }
        }
    }
}
