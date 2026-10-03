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

// Exact interval overlap with outward accumulation. Padding by 40 ms converts
// the fine bound's closed frame-neighborhood test into half-open integer times.
pub(super) fn peak(spans: &[(i64, i64, f64)], pad: i64) -> Option<f64> {
    let mut events = Vec::new();
    for &(a, b, value) in spans {
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        if value == 0.0 {
            continue;
        }
        let lo = a.saturating_sub(pad);
        let hi = if pad == 0 { b } else { b.saturating_add(pad).saturating_add(1) };
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
                        slots[pos].peak = peak(&part.spans, 0)?;
                        slots[pos].near = peak(&part.spans, 40)?;
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
        // max_t sum(slot factors) <= sum(slot maxima). This is a constant-cost
        // upper envelope of cand_eps, retaining the original command drift model.
        let one = (1.0 + peak.max(near)).next_up();
        let representation = (2.0 * n).next_up();
        let sum =
            ((ops * ((2.0 * one).next_up() + near).next_up()).next_up() + (representation * one).next_up()).next_up();
        let amplification =
            float_margin::amplification(((3.0 * ops).next_up() + representation).next_up(), 2f64.powi(-24))?;
        let drift = ((sum * 2f64.powi(-24)).next_up() * amplification).next_up();
        // The drift is absolute in score-up units: every entry's coefficient in `base` gains at most `drift` of
        // it, the rest of the chain stays relative. The pool margin `eps` is a relative margin of its own.
        let chain = float_margin::with_chain(0.0, self.chain_extra)?;
        let p = power.max(0) as f64;
        let additive = ((a + (drift * self.base).next_up()).next_up() * (1.0 + chain).next_up()).next_up();
        let relative = (a * (1.0 + self.eps).next_up()).next_up();
        let cap = (p * additive.min(relative)).next_up().ceil();
        cap.is_finite().then_some(cap as i128)
    }
}

#[cfg(test)]
mod tests {
    use super::{Prefix, peak};
    #[test]
    fn per_slot_peak_covers_all_other_slots_observation_times() {
        let spans = [(0, 0, 2.0), (80, 80, 3.0), (-20, 10, 4.0), (10, 50, 7.0)];
        for t in -100..=150 {
            let a: f64 = spans.iter().filter(|&&(a, b, _)| a <= t && t < b).map(|w| w.2).sum();
            let b: f64 = spans.iter().filter(|&&(a, b, _)| a <= t + 40 && t - 40 <= b).map(|w| w.2).sum();
            assert!(peak(&spans, 0).unwrap() >= a);
            assert!(peak(&spans, 40).unwrap() >= b);
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
