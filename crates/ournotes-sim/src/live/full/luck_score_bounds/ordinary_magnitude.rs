//! Optional exact-real ordinary-factor magnitude from all historical filed command sets.
//!
//! Native rounding is deliberately absent here: this supplies only the ideal-state M used by the existing
//! A/U/N/H roundoff proof. A failed optional construction leaves the lifetime-L1 magnitude valid. The caller
//! must add its unchanged unconditional direct-probe run amplitude and must propagate cancellation.

use super::terminal_prefix::TimedEvent;
use super::trace_drift::Decline;

const FIELDS: usize = 6;

#[derive(Clone, Copy)]
struct Command {
    key: (usize, i32, i32, usize),
    deltas: [f32; FIELDS],
}

/// Sum and extrema over all prefixes, including the empty prefix. The endpoints enclose exact real sums.
#[derive(Clone, Copy, Debug, Default)]
struct Node {
    sum_lower: f64,
    sum_upper: f64,
    prefix_lower: f64,
    prefix_upper: f64,
}

fn down_add(a: f64, b: f64) -> Result<f64, Decline> {
    let value = if a == 0.0 {
        b
    } else if b == 0.0 {
        a
    } else {
        (a + b).next_down()
    };
    value.is_finite().then_some(value).ok_or(Decline::Nonfinite)
}

fn up_add(a: f64, b: f64) -> Result<f64, Decline> {
    let value = if a == 0.0 {
        b
    } else if b == 0.0 {
        a
    } else {
        (a + b).next_up()
    };
    value.is_finite().then_some(value).ok_or(Decline::Nonfinite)
}

impl Node {
    fn point(value: f32) -> Result<Self, Decline> {
        if !value.is_finite() {
            return Err(Decline::Nonfinite);
        }
        let value = f64::from(value);
        Ok(Self { sum_lower: value, sum_upper: value, prefix_lower: value.min(0.0), prefix_upper: value.max(0.0) })
    }

    fn is_zero(self) -> bool {
        self.sum_lower == 0.0 && self.sum_upper == 0.0 && self.prefix_lower == 0.0 && self.prefix_upper == 0.0
    }

    fn join(left: Self, right: Self) -> Result<Self, Decline> {
        if left.is_zero() {
            return Ok(right);
        }
        if right.is_zero() {
            return Ok(left);
        }
        Ok(Self {
            sum_lower: down_add(left.sum_lower, right.sum_lower)?,
            sum_upper: up_add(left.sum_upper, right.sum_upper)?,
            prefix_lower: left.prefix_lower.min(down_add(left.sum_lower, right.prefix_lower)?),
            prefix_upper: left.prefix_upper.max(up_add(left.sum_upper, right.prefix_upper)?),
        })
    }

    fn magnitude(self, initial: f32) -> Result<f64, Decline> {
        let initial = f64::from(initial);
        let lower = down_add(initial, self.prefix_lower)?;
        let upper = up_add(initial, self.prefix_upper)?;
        Ok(lower.abs().max(upper.abs()))
    }
}

fn storage_bytes(commands: usize, order: usize, nodes: usize) -> Result<usize, Decline> {
    commands
        .checked_mul(std::mem::size_of::<Command>())
        .and_then(|bytes| order.checked_mul(std::mem::size_of::<usize>()).and_then(|next| bytes.checked_add(next)))
        .and_then(|bytes| nodes.checked_mul(std::mem::size_of::<Node>()).and_then(|next| bytes.checked_add(next)))
        .ok_or(Decline::Capacity)
}

/// The full event ordinal disambiguates equal time/owner commands in their native stable ordering. At every
/// original ordinary filing, insert exactly that native f32 delta into its final execution position and retain
/// all prefix extrema. Historical maxima cover the old set during undo as well as the new set during execute.
/// One scalar-field tree is reused for all six fields; no probability branch or factor-history path is built.
///
/// `byte_limit` is only a temporary-storage allowance for this optional tightening, never a search or DP budget.
/// Its actual command/order/tree capacities are charged. Numeric or capacity failure must retain the old L1 M;
/// cancellation must keep its existing stopped status instead of silently continuing this optional work.
pub(super) fn bound(
    initial: [f32; FIELDS],
    events: &[TimedEvent],
    byte_limit: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<[f64; FIELDS], Decline> {
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    if initial.iter().any(|value| !value.is_finite()) {
        return Err(Decline::Nonfinite);
    }
    let mut count = 0usize;
    for (index, event) in events.iter().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Err(Decline::Cancelled);
        }
        if matches!(event, TimedEvent::Factor { .. }) {
            count = count.checked_add(1).ok_or(Decline::CountOverflow)?;
        }
    }
    if count == 0 {
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        return Ok(initial.map(|value| f64::from(value).abs()));
    }
    let leaves = count.checked_next_power_of_two().ok_or(Decline::Capacity)?;
    let node_count = leaves.checked_mul(2).ok_or(Decline::Capacity)?;
    if storage_bytes(count, count, node_count)? > byte_limit {
        return Err(Decline::Capacity);
    }
    let mut commands = Vec::new();
    commands.try_reserve_exact(count).map_err(|_| Decline::Capacity)?;
    for (ordinal, event) in events.iter().copied().enumerate() {
        if ordinal.is_multiple_of(64) && cancelled() {
            return Err(Decline::Cancelled);
        }
        if let TimedEvent::Factor { frame, time_ms, owner, deltas } = event {
            if deltas.iter().any(|value| !value.is_finite()) {
                return Err(Decline::Nonfinite);
            }
            commands.push(Command { key: (frame, time_ms, owner, ordinal), deltas });
        }
    }
    commands.sort_unstable_by_key(|command| command.key);
    let mut order = Vec::new();
    order.try_reserve_exact(count).map_err(|_| Decline::Capacity)?;
    order.extend(0..count);
    order.sort_unstable_by_key(|&position| commands[position].key.3);
    let mut tree = Vec::new();
    tree.try_reserve_exact(node_count).map_err(|_| Decline::Capacity)?;
    tree.resize(node_count, Node::default());
    if storage_bytes(commands.capacity(), order.capacity(), tree.capacity())? > byte_limit {
        return Err(Decline::Capacity);
    }

    let mut magnitude = initial.map(|value| f64::from(value).abs());
    for field in 0..FIELDS {
        tree.fill(Node::default());
        for (index, &position) in order.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            let value = commands[position].deltas[field];
            // Numeric zero changes no exact-real prefix magnitude, including a signed-zero command.
            if value == 0.0 {
                continue;
            }
            let mut node = leaves + position;
            tree[node] = Node::point(value)?;
            while node > 1 {
                node /= 2;
                tree[node] = Node::join(tree[2 * node], tree[2 * node + 1])?;
            }
            magnitude[field] = magnitude[field].max(tree[1].magnitude(initial[field])?);
        }
    }
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    Ok(magnitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(frame: usize, time_ms: i32, owner: i32, deltas: [f32; FIELDS]) -> TimedEvent {
        TimedEvent::Factor { frame, time_ms, owner, deltas }
    }

    // These fixtures use binary dyadics small enough that the reference f64 sums are exact. Enumerate every
    // historical filed set and every internal native command prefix directly, independently of the tree.
    fn enumerate(initial: [f32; FIELDS], events: &[TimedEvent]) -> [f64; FIELDS] {
        let mut commands = Vec::new();
        let mut out = initial.map(|value| f64::from(value).abs());
        for (ordinal, event) in events.iter().copied().enumerate() {
            if let TimedEvent::Factor { frame, time_ms, owner, deltas } = event {
                commands.push(((frame, time_ms, owner, ordinal), deltas));
                commands.sort_unstable_by_key(|&(key, _)| key);
                let mut prefix = initial.map(f64::from);
                for (_, delta) in &commands {
                    for field in 0..FIELDS {
                        prefix[field] += f64::from(delta[field]);
                        out[field] = out[field].max(prefix[field].abs());
                    }
                }
            }
        }
        out
    }

    #[test]
    fn historical_prefix_tree_contains_independent_enumeration_in_all_fields() {
        for seed in 0..24 {
            let initial = [0.0, 1.0, -0.0, -1.0, 0.5, -0.5];
            let mut events = Vec::new();
            for ordinal in 0..48 {
                let deltas = std::array::from_fn(|field| {
                    let signed = ((seed * 7 + ordinal * 13 + field * 11) % 33) as i32 - 16;
                    signed as f32 / 16.0
                });
                events.push(command(
                    (ordinal * 7 + seed) % 9,
                    ((ordinal * 5) % 7) as i32,
                    (ordinal % 3) as i32,
                    deltas,
                ));
                events.push(TimedEvent::Other);
            }
            let expected = enumerate(initial, &events);
            let bounded = bound(initial, &events, 1024 * 1024, || false).unwrap();
            for field in 0..FIELDS {
                assert!(bounded[field] >= expected[field], "seed={seed} field={field}: {bounded:?} {expected:?}");
                assert!(bounded[field] <= expected[field] + 1e-9);
            }
        }
    }

    #[test]
    fn history_and_internal_command_prefixes_cannot_be_replaced_by_the_final_sum() {
        let initial = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let mut plus = [0.0; FIELDS];
        plus[1] = 64.0;
        let mut minus = [0.0; FIELDS];
        minus[1] = -64.0;
        // The first state reaches 65. After the backdated negative filing, the complete prefix maximum is
        // only 1 and its minimum is -63. Retaining only the final filed set would lose the earlier state.
        let events = [command(2, 20, 0, plus), command(1, 10, 0, minus)];
        let bounded = bound(initial, &events, 4096, || false).unwrap();
        assert!(bounded[1] >= 65.0 && bounded[1] < 65.00001);
        // Both commands in one frame have zero net contribution, but an internal prefix still reaches 65.
        let events = [command(1, 10, 0, plus), command(1, 10, 1, minus)];
        assert!(bound(initial, &events, 4096, || false).unwrap()[1] >= 65.0);
    }

    #[test]
    fn alternating_native_pulses_do_not_pay_their_complete_lifetime_l1() {
        let mut events = Vec::new();
        for index in 0..80 {
            let mut value = [0.0; FIELDS];
            value[1] = if index % 2 == 0 { 0.25 } else { -0.25 };
            events.push(command(index, index as i32, 0, value));
        }
        let bounded = bound([0.0, 1.0, 0.0, 0.0, 0.0, 0.0], &events, 1024 * 1024, || false).unwrap();
        assert!(bounded[1] >= 1.25 && bounded[1] < 1.250001);
        assert!(bounded[1] < 1.0 + 80.0 * 0.25);
    }

    #[test]
    fn zero_signed_zero_and_subnormal_magnitudes_keep_the_empty_prefix() {
        let smallest = f32::from_bits(1);
        let events = [command(0, 0, 0, [smallest, -smallest, -0.0, 0.0, smallest, -smallest])];
        let bounded = bound([0.0; FIELDS], &events, 4096, || false).unwrap();
        for field in [0, 1, 4, 5] {
            assert!(bounded[field] >= f64::from(smallest) && bounded[field] < 2.0 * f64::from(smallest));
        }
        assert_eq!(bounded[2], 0.0);
        assert_eq!(bounded[3], 0.0);
        let empty = bound([-0.0, 1.0, -1.0, 0.5, -0.5, 0.0], &[], 0, || false).unwrap();
        assert_eq!(empty, [0.0, 1.0, 1.0, 0.5, 0.5, 0.0]);
    }

    #[test]
    fn optional_capacity_numeric_and_cancellation_failures_return_no_new_magnitude() {
        let events = [command(0, 0, 0, [1.0; FIELDS])];
        assert_eq!(bound([0.0; FIELDS], &events, 0, || false), Err(Decline::Capacity));
        assert_eq!(storage_bytes(usize::MAX, 1, 2), Err(Decline::Capacity));
        assert_eq!(bound([f32::NAN; FIELDS], &events, 4096, || false), Err(Decline::Nonfinite));
        let invalid = [command(0, 0, 0, [f32::INFINITY; FIELDS])];
        assert_eq!(bound([0.0; FIELDS], &invalid, 4096, || false), Err(Decline::Nonfinite));
        assert_eq!(bound([0.0; FIELDS], &events, 4096, || true), Err(Decline::Cancelled));
        let events = vec![events[0]; 140];
        let mut calls = 0;
        assert_eq!(
            bound([0.0; FIELDS], &events, 1024 * 1024, || {
                calls += 1;
                calls > 8
            }),
            Err(Decline::Cancelled)
        );
    }
}
