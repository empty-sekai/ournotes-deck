//! Compiled coefficient envelope for candidates with no reachable judgement changes.
use super::*;

struct Prefix {
    lower: Vec<f64>,
    upper: Vec<f64>,
}
impl Prefix {
    fn new(terms: impl Iterator<Item = (f64, f64)>) -> Option<Self> {
        let (mut lower, mut upper) = (vec![0.0f64], vec![0.0f64]);
        for (lo, hi) in terms {
            if !lo.is_finite() || !hi.is_finite() || lo < 0.0 || hi < lo {
                return None;
            }
            lower.push((lower.last()? + lo).next_down().max(0.0));
            upper.push((upper.last()? + hi).next_up());
        }
        upper.last()?.is_finite().then_some(Self { lower, upper })
    }
    fn interval(&self, lo: usize, hi: usize) -> f64 {
        (self.upper[hi] - self.lower[lo]).next_up().max(0.0)
    }
}
#[derive(Clone, Copy, Default)]
struct Packet {
    gain: f64,
    ops: f64,
    peak: f64,
    near: f64,
    n: f64,
    applicable: bool,
}

/// One whole physical pair's mean reward and position-wise maxima of its complete command work.
/// The fields remain private: only all five applicable native-position packets can produce this value.
/// Component-wise maxima for an open slot deliberately allow different pairs to attain each component.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RawNodePacket {
    // mean gain, executions, time peak, closed-frame peak, lifetime commands
    values: [f64; 5],
}

impl RawNodePacket {
    pub(crate) fn add(self, rhs: Self) -> Self {
        Self { values: std::array::from_fn(|i| (self.values[i] + rhs.values[i]).next_up()) }
    }

    pub(crate) fn maximum(self, rhs: Self) -> Self {
        Self { values: std::array::from_fn(|i| self.values[i].max(rhs.values[i])) }
    }
}

// Outward overlap maximum of half-open time spans or closed native frame spans.
pub(super) fn peak(spans: &[(i64, i64, f64)], frames: Option<ScoreFrames>) -> Option<f64> {
    let mut events = Vec::new();
    for &(a, b, value) in spans {
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        if value == 0.0 {
            continue;
        }
        let (lo, hi) = match frames {
            Some(frames) => {
                let Some((lo, hi)) = frames.closed(a, b) else { continue };
                (lo, hi + 1)
            }
            None => (a, b),
        };
        if lo >= hi {
            continue;
        }
        events.push((lo, value));
        if hi < i64::MAX {
            events.push((hi, -value));
        }
    }
    events.sort_unstable_by_key(|e| e.0);
    let (mut i, mut value, mut best) = (0, 0.0f64, 0.0f64);
    while i < events.len() {
        let time = events[i].0;
        while i < events.len() && events[i].0 == time {
            value = (value + events[i].1).next_up();
            i += 1;
        }
        best = best.max(value);
    }
    best.is_finite().then_some(best)
}

/// Closed-frame peak with an absolute binary64 allowance for endpoint grouping,
/// prefix additions and the factor values supplied to that sweep.
pub(super) fn certified_frame_peak(spans: &[(i64, i64, f64)], frames: ScoreFrames) -> Option<f64> {
    let value = peak(spans, Some(frames))?;
    let mut norm = 0.0f64;
    for &(_, _, factor) in spans {
        if !factor.is_finite() || factor < 0.0 {
            return None;
        }
        norm = (norm + (4.0 * factor).next_up()).next_up();
    }
    let count = (((spans.len() as f64).next_up() * 8.0).next_up() + 8.0).next_up();
    let alpha = (count * 2f64.powi(-53)).next_up();
    if alpha >= 1.0 {
        return None;
    }
    let gamma = (alpha / (1.0 - alpha).next_down()).next_up();
    let result = (value + (gamma * norm).next_up()).next_up();
    result.is_finite().then_some(result)
}
pub(super) struct RawEnvelope {
    base: f64,
    gains: Vec<Vec<[Packet; 5]>>,
    eps: f64,
    chain_extra: f64,
}
impl RawEnvelope {
    pub(super) fn compile(
        coef: &Coef,
        fine: &Fine,
        contrib: &[Vec<[Contrib; 5]>],
        eps: f64,
        chain_extra: f64,
    ) -> Option<Self> {
        let prefix: Vec<_> = (0..5)
            .map(|kind| {
                Prefix::new((0..fine.raw.len()).map(|e| {
                    let raw = fine.raw[e];
                    let jp = if kind == 0 { fine.mjp[1usize << raw] } else { fine.jp4[1usize << raw][kind - 1] };
                    let lo = ((coef.k[e] * coef.z[e]).next_down().max(0.0) * jp).next_down().max(0.0);
                    let hi = ((coef.k[e] * coef.z[e]).next_up() * jp).next_up();
                    (lo, hi)
                }))
            })
            .collect::<Option<_>>()?;
        let base = prefix[0].interval(0, fine.raw.len());
        let mut gains = Vec::with_capacity(contrib.len());
        for (m, classes) in contrib.iter().enumerate() {
            let mut row = Vec::with_capacity(classes.len());
            for (c, parts) in classes.iter().enumerate() {
                let mut slots = [Packet::default(); 5];
                for (pos, part) in parts.iter().enumerate() {
                    let src = fine.src[m][c] as usize;
                    if src == 0 || fine.extra[src][pos].iter().all(|&mask| mask == 0) {
                        slots[pos].applicable = true;
                        slots[pos].ops = part.ops;
                        slots[pos].peak = peak(&part.spans, None)?;
                        slots[pos].near = peak(&part.spans, Some(fine.score_frames))?;
                        slots[pos].n = part.cmds;
                        for w in &part.windows {
                            for (kind, factor) in std::iter::once(w.note).chain(w.judge).enumerate() {
                                if factor < 0.0 || !factor.is_finite() {
                                    return None;
                                }
                                let term = (factor * prefix[kind].interval(w.lo as usize, w.hi as usize)).next_up();
                                slots[pos].gain = (slots[pos].gain + term).next_up();
                            }
                        }
                    }
                }
                if slots.iter().any(|p| [p.gain, p.ops, p.peak, p.near, p.n].iter().any(|v| !v.is_finite() || *v < 0.0))
                {
                    return None;
                }
                row.push(slots);
            }
            gains.push(row);
        }
        Some(Self { base, gains, eps, chain_extra })
    }

    pub(super) fn node_packet(&self, member: usize, class: usize) -> Option<RawNodePacket> {
        let positions = self.gains.get(member)?.get(class)?;
        let mut result = RawNodePacket::default();
        for packet in positions {
            // Validate BEFORE maxima: f64::max can silently discard NaN instead of refusing unknown work.
            if !packet.applicable
                || [packet.gain, packet.ops, packet.peak, packet.near, packet.n]
                    .iter()
                    .any(|value| !value.is_finite() || *value < 0.0)
            {
                return None;
            }
            result.values[0] = (result.values[0] + packet.gain).next_up();
            for (upper, value) in result.values[1..].iter_mut().zip([packet.ops, packet.peak, packet.near, packet.n]) {
                *upper = upper.max(value);
            }
        }
        result.values[0] = (result.values[0] / 5.0).next_up();
        result.values.iter().all(|v| v.is_finite() && *v >= 0.0).then_some(result)
    }

    /// Uniform 120-order sum, without evaluating any order. Every member occupies every position 24 times.
    /// Gain therefore averages linearly; command work uses a deterministic maximum over ALL those positions.
    /// Both complete additive/relative envelopes bound the mean independently, so their minimum also does.
    /// One extra score unit per label covers the original per-order ceiling before taking the uniform mean.
    pub(super) fn node_upper(&self, power: i64, packet: RawNodePacket) -> Option<i128> {
        if !(0..=i64::from(i32::MAX)).contains(&power) || packet.values.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return None;
        }
        let [gain, ops, peak, near, n] = packet.values;
        let coefficient = self.coefficient_upper((self.base + gain).next_up(), ops, peak, near, n)?;
        let mean = (((power as f64 * coefficient).next_up()) + 1.0).next_up();
        let cap = (mean * crate::search::uniform::ORDERS as f64).next_up().ceil();
        (cap.is_finite() && cap >= 0.0 && cap < i128::MAX as f64).then_some(cap as i128)
    }
    pub(super) fn upper(
        &self,
        power: i64,
        members: [usize; 5],
        classes: [usize; 5],
        positions: &[usize; 5],
    ) -> Option<i128> {
        let mut a = self.base;
        let (mut ops, mut peak, mut near, mut n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for slot in 0..5 {
            let p = self.gains[members[slot]][classes[slot]][positions[slot]];
            if !p.applicable {
                return None;
            }
            a = (a + p.gain).next_up();
            ops = (ops + p.ops).next_up();
            peak = (peak + p.peak).next_up();
            near = (near + p.near).next_up();
            n = (n + p.n).next_up();
        }
        let coefficient = self.coefficient_upper(a, ops, peak, near, n)?;
        let cap = (power.max(0) as f64 * coefficient).next_up().ceil();
        cap.is_finite().then_some(cap as i128)
    }

    fn coefficient_upper(&self, a: f64, ops: f64, peak: f64, near: f64, n: f64) -> Option<f64> {
        // max_t sum(slot factors) <= sum(slot maxima). This is a constant-cost
        // upper envelope of cand_eps, retaining the original command drift model.
        let one = (1.0 + peak.max(near)).next_up();
        let representation = (2.0 * n).next_up();
        let sum =
            ((ops * ((2.0 * one).next_up() + near).next_up()).next_up() + (representation * one).next_up()).next_up();
        let amplification =
            float_margin::amplification(((3.0 * ops).next_up() + representation).next_up(), 2f64.powi(-24))?;
        let drift = ((sum * 2f64.powi(-24)).next_up() * amplification).next_up();
        let drift = float_margin::snapshot_allowance(drift, self.chain_extra > 0.0);
        // The drift is absolute in score-up units: every entry's coefficient in `base` gains at most `drift` of
        // it, the rest of the chain stays relative. The pool margin `eps` is a relative margin of its own.
        let chain = float_margin::with_chain(0.0, self.chain_extra)?;
        let additive = ((a + (drift * self.base).next_up()).next_up() * (1.0 + chain).next_up()).next_up();
        let relative = (a * (1.0 + self.eps).next_up()).next_up();
        let upper = additive.min(relative);
        (upper.is_finite() && upper >= 0.0).then_some(upper)
    }
}

#[cfg(test)]
mod tests {
    use super::{LiveParams, Packet, Prefix, RawEnvelope, RawNodePacket, ScoreFrames, get_frame, peak};

    #[test]
    fn raw_node_mean_retains_ceil_slack_and_both_complete_arithmetic_envelopes() {
        // Fractional gains intentionally put the original order caps on different integer boundaries.
        // Each of the independent error envelopes is the tighter one in one of these two cases.
        for eps in [0.0, 0.25] {
            let packets = std::array::from_fn(|position| Packet {
                gain: 0.13 * (position + 1) as f64,
                ops: 300.0 + 30.0 * position as f64,
                peak: 0.4 + position as f64 * 0.1,
                near: 0.5 + position as f64 * 0.1,
                n: 20.0 + position as f64,
                applicable: true,
            });
            let raw = RawEnvelope { base: 0.37, gains: vec![vec![packets]; 5], eps, chain_extra: super::GK_CHAIN_EPS };
            let mut packet = RawNodePacket::default();
            for member in 0..5 {
                packet = packet.add(raw.node_packet(member, 0).unwrap());
            }
            for power in [1, 7, 101, 12_345] {
                let original: i128 = crate::search::uniform::all_orders()
                    .iter()
                    .map(|order| {
                        raw.upper(power, [0, 1, 2, 3, 4], [0; 5], &crate::search::uniform::positions_of(order)).unwrap()
                    })
                    .sum();
                let node = raw.node_upper(power, packet).unwrap();
                assert!(node >= original);
                assert!(node - original < 600, "the ceiling allowance is per label, not per note or command");
            }
        }
    }

    #[test]
    fn raw_node_nonfinite_or_incomplete_packets_decline_without_publishing_a_cap() {
        let mut raw = RawEnvelope {
            base: 1.0,
            gains: vec![vec![[Packet { applicable: true, ..Default::default() }; 5]]],
            eps: 0.0,
            chain_extra: 0.0,
        };
        assert!(raw.node_packet(0, 0).is_some());
        raw.gains[0][0][4].applicable = false;
        assert!(raw.node_packet(0, 0).is_none(), "all five positions are required, not merely the best one");
        for field in 0..5 {
            for invalid in [f64::NAN, f64::INFINITY, -1.0] {
                let packet = &mut raw.gains[0][0][4];
                *packet = Packet { applicable: true, ..Default::default() };
                match field {
                    0 => packet.gain = invalid,
                    1 => packet.ops = invalid,
                    2 => packet.peak = invalid,
                    3 => packet.near = invalid,
                    4 => packet.n = invalid,
                    _ => unreachable!(),
                }
                assert!(raw.node_packet(0, 0).is_none(), "field {field} must refuse before max ignores {invalid}");
            }
        }
        for value in [f64::INFINITY, f64::NAN, -1.0, f64::MAX] {
            assert!(raw.node_upper(10, RawNodePacket { values: [value; 5] }).is_none());
        }
        assert!(raw.node_upper(i64::MAX, RawNodePacket::default()).is_none());
    }
    #[test]
    fn per_slot_peak_covers_all_other_slots_observation_times() {
        let spans = [(0, 0, 2.0), (80, 80, 3.0), (-20, 10, 4.0), (10, 50, 7.0)];
        for t in -100..=150 {
            let a: f64 = spans.iter().filter(|&&(a, b, _)| a <= t && t < b).map(|w| w.2).sum();
            assert!(peak(&spans, None).unwrap() >= a);
        }
    }

    #[test]
    fn native_frame_peaks_cover_every_projected_factor_span() {
        let a = 134_217_880;
        let b = 134_217_928;
        assert_eq!(get_frame(a), get_frame(b));
        assert!(b - a > 40);
        let cases = [
            (0, vec![(-5, 0, 1.0), (0, 0, 2.0), (40, 80, 3.0), (2000, 2010, 5.0), (3000, 3010, 7.0)]),
            (b, vec![(a as i64, a as i64, 2.0), (b as i64, b as i64, 3.0), (a as i64 - 40, b as i64 + 40, 5.0)]),
            (b, vec![(145_000_000, 145_000_010, 7.0), (i64::MAX, i64::MAX, 11.0), (i64::MIN, i64::MIN, 13.0)]),
        ];
        for (length, spans) in cases {
            let params = LiveParams {
                skill_target_music_type: 0,
                total_power: 1,
                music_level: 1,
                converted_note_count: 1,
                music_length_ms: length,
                score_music_length_ms: None,
                assist_factor: 1.0,
            };
            let frames = ScoreFrames::new(&params);
            let last = get_frame(length) + 49;
            let mut reference = std::collections::BTreeMap::<i32, f64>::new();
            for &(a, b, value) in &spans {
                let touched: std::collections::BTreeSet<_> = (a..=b)
                    .map(|t| get_frame(t.clamp(i32::MIN as i64, i32::MAX as i64) as i32).min(last).max(0))
                    .collect();
                for frame in touched {
                    *reference.entry(frame).or_default() += value;
                }
            }
            let compiled = peak(&spans, Some(frames)).unwrap();
            let candidate = spans
                .iter()
                .map(|&(a, _, _)| {
                    let frame = frames.at(a);
                    spans.iter().filter(|&&(a, b, _)| frames.meets(a, b, frame)).map(|w| w.2).sum::<f64>()
                })
                .fold(0.0f64, f64::max);
            for expected in reference.values() {
                assert!(compiled >= *expected);
                assert!(candidate >= *expected);
            }
        }
    }
    #[test]
    fn subtracting_prefixes_preserves_small_intervals_after_large_totals() {
        let values = [1e16, 1.0, 1.0, 0.0, 2.0, 1e-10];
        let p = Prefix::new(values.into_iter().map(|v| (v, v))).unwrap();
        for a in 0..values.len() {
            for b in a..=values.len() {
                assert!(p.interval(a, b) >= values[a..b].iter().sum::<f64>());
            }
        }
        assert!(Prefix::new([(0.0, f64::INFINITY)].into_iter()).is_none());
    }
}
