//! Positive linear Rush integrals and candidate-domain command certificates.
//!
//! These are score envelopes, never simulation/program equivalence classes. A
//! physical pair keeps every non-Rush term in its base;
//! only positively identified Rush windows are rebuilt from a complete replay.
use super::*;

const MAX_SPECS: usize = 16;
const MAX_CELLS: usize = 1_000_000;
const MAX_CACHE: usize = 1024;

fn nonnegative(x: f64) -> bool {
    x.is_finite() && x >= 0.0
}
fn add(a: f64, b: f64) -> Option<f64> {
    if !nonnegative(a) || !nonnegative(b) {
        return None;
    }
    let value = if a == 0.0 {
        b
    } else if b == 0.0 {
        a
    } else {
        (a + b).next_up()
    };
    value.is_finite().then_some(value)
}
fn mul(a: f64, b: f64) -> Option<f64> {
    if !nonnegative(a) || !nonnegative(b) {
        return None;
    }
    let value = if a == 0.0 || b == 0.0 { 0.0 } else { (a * b).next_up() };
    value.is_finite().then_some(value)
}

/// Lower and upper endpoints are necessary: subtracting two upper prefix sums
/// can lose a small positive interval after a large accumulated total.
struct Prefix {
    lower: Vec<f64>,
    upper: Vec<f64>,
}
impl Prefix {
    fn new(terms: impl Iterator<Item = (f64, f64)>) -> Option<Self> {
        let (mut lower, mut upper) = (vec![0.0f64], vec![0.0f64]);
        for (lo, hi) in terms {
            if !nonnegative(lo) || !nonnegative(hi) || lo > hi {
                return None;
            }
            lower.push((lower.last()? + lo).next_down().max(0.0));
            upper.push(add(*upper.last()?, hi)?);
        }
        Some(Self { lower, upper })
    }
    fn interval(&self, lo: usize, hi: usize) -> Option<f64> {
        if lo > hi {
            return None;
        }
        if lo == hi {
            return self.upper.get(hi).map(|_| 0.0);
        }
        let value = (self.upper.get(hi)? - self.lower.get(lo)?).next_up().max(0.0);
        value.is_finite().then_some(value)
    }
}

#[derive(Clone, Default)]
struct RunCounts {
    ops: f64,
    cmds: f64,
    ops_per_run: f64,
    cmds_per_run: f64,
}

/// Componentwise maxima deliberately relax the physical pair correlation.
/// Every field is nonnegative and the certified drift formula is monotone in
/// every field. Summing four fixed packets and one such maximum is therefore
/// safe for every physical completion in the future variant.
#[derive(Clone, Default)]
pub(crate) struct MarginPacket {
    source: Option<Rc<()>>,
    plain_ops: f64,
    plain_cmds: f64,
    peak: f64,
    near: f64,
    runs: Vec<RunCounts>,
}
impl MarginPacket {
    fn merge(&mut self, other: &Self, maximum: bool) -> Option<()> {
        if let (Some(a), Some(b)) = (&self.source, &other.source)
            && !Rc::ptr_eq(a, b)
        {
            return None;
        }
        // An identity-free default packet is zero. It adopts the first source
        // and its dimension count; subsequent packets must share that source.
        if self.source.is_none() {
            self.source = other.source.clone();
            self.runs.resize(other.runs.len(), RunCounts::default());
        }
        if other.source.is_some() && self.runs.len() != other.runs.len() {
            return None;
        }
        let combine = |a: f64, b: f64| -> Option<f64> {
            if maximum { (nonnegative(a) && nonnegative(b)).then_some(a.max(b)) } else { add(a, b) }
        };
        self.plain_ops = combine(self.plain_ops, other.plain_ops)?;
        self.plain_cmds = combine(self.plain_cmds, other.plain_cmds)?;
        self.peak = combine(self.peak, other.peak)?;
        self.near = combine(self.near, other.near)?;
        for (a, b) in self.runs.iter_mut().zip(&other.runs) {
            a.ops = combine(a.ops, b.ops)?;
            a.cmds = combine(a.cmds, b.cmds)?;
            a.ops_per_run = combine(a.ops_per_run, b.ops_per_run)?;
            a.cmds_per_run = combine(a.cmds_per_run, b.cmds_per_run)?;
        }
        Some(())
    }
    pub(crate) fn add_assign(&mut self, other: &Self) -> Option<()> {
        self.merge(other, false)
    }
    pub(crate) fn max_assign(&mut self, other: &Self) -> Option<()> {
        self.merge(other, true)
    }
    fn matches(&self, summary: &MaskSummary) -> bool {
        self.source.as_ref().is_none_or(|id| Rc::ptr_eq(id, &summary.source))
    }
    fn counts(&self, summary: &MaskSummary) -> Option<(f64, f64)> {
        if !self.matches(summary) || (!self.runs.is_empty() && self.runs.len() != summary.runs.len()) {
            return None;
        }
        let (mut ops, mut cmds) = (self.plain_ops, self.plain_cmds);
        for (row, &runs) in self.runs.iter().zip(&summary.runs) {
            // sum(min(a_i,b_i*r)) <= min(sum(a_i),r*sum(b_i)).
            // Ignoring each row's separate max_runs can only enlarge this cap.
            ops = add(ops, row.ops.min(mul(runs, row.ops_per_run)?))?;
            cmds = add(cmds, row.cmds.min(mul(runs, row.cmds_per_run)?))?;
        }
        Some((ops, cmds))
    }
    /// Same binary32 certificate as FineView, with per-part broad-window peaks
    /// summed instead of scanning the combined spans. This also covers transient
    /// start/end commands: `near` uses the closed 40-ms frame neighborhood.
    /// The integer-to-f32 cast AND division each pay one representation rounding.
    /// The result is the absolute score-up drift; the relative chain allowance is [`MaskSummary::chain`].
    pub(crate) fn drift(&self, summary: &MaskSummary) -> Option<f64> {
        let (ops, cmds) = self.counts(summary)?;
        let representation = mul(2.0, cmds)?;
        let weight = add(mul(3.0, ops)?, representation)?;
        let amplification = float_margin::amplification(weight, 2f64.powi(-24))?;
        let state = add(1.0, self.peak.max(self.near))?;
        let magnitude = add(mul(ops, add(mul(2.0, state)?, self.near)?)?, mul(representation, state)?)?;
        mul(mul(magnitude, 2f64.powi(-24))?, amplification)
    }
}

#[derive(Clone)]
pub(crate) struct LinearPart {
    pub(crate) base: f64,
    /// Exact stored bit patterns identify bound buckets, not runtime programs.
    pub(crate) alpha: Vec<f64>,
    pub(crate) margin: MarginPacket,
}
impl LinearPart {
    pub(crate) fn gain(&self, summary: &MaskSummary) -> Option<f64> {
        self.margin.matches(summary).then_some(())?;
        add(self.base, summary.dot(&self.alpha)?)
    }
}

pub(crate) struct MaskSummary {
    source: Rc<()>,
    coefficients: Vec<f64>,
    runs: Vec<f64>,
    chain_extra: f64,
}
impl MaskSummary {
    /// The relative allowance of the native chain after the score-up factor.
    pub(crate) fn chain(&self) -> Option<f64> {
        float_margin::with_chain(0.0, self.chain_extra)
    }
    /// `alpha` must originate in this compiler's LinearPart catalog. All terms
    /// are accumulated positively and outward, without a difference array or a
    /// subtraction of two score upper bounds.
    pub(crate) fn dot(&self, alpha: &[f64]) -> Option<f64> {
        if alpha.len() != self.coefficients.len() {
            return None;
        }
        alpha.iter().zip(&self.coefficients).try_fold(0.0, |sum, (&a, &c)| add(sum, mul(a, c)?))
    }
}

struct SummaryCache {
    _masks: RushMasks,
    summary: Rc<MaskSummary>,
}

pub(crate) struct RushLinear {
    source: Rc<()>,
    base: f64,
    chain_extra: f64,
    times: Vec<i32>,
    prefix: Vec<Prefix>,
    specs: Vec<Rc<rush::RushSpec>>,
    /// (spec index, note=0 or judgement=1..4).
    dimensions: Vec<(usize, usize)>,
    parts: Vec<Vec<[LinearPart; 5]>>,
    class_of: Vec<Vec<u16>>,
    windows: RefCell<rush::WindowCache>,
    summaries: RefCell<HashMap<usize, SummaryCache>>,
}
impl RushLinear {
    pub(crate) fn compile(fine: &JointFineBounds) -> Option<Self> {
        if !fine.rush_eligible
            || !nonnegative(fine.chain_extra)
            // The legacy conversion budget is constructed with nominal f64
            // differences and top sums. A smaller native drift certificate must
            // not silently pay for that unquantified arithmetic. Keep the old
            // search on such domains until its budget is rebuilt outward.
            || fine.contrib.iter().flatten().flatten().any(|p| p.budget != 0.0)
        {
            return None;
        }
        let coef = &fine.coef;
        let ne = coef.times.len();
        if [coef.k.len(), coef.z.len(), coef.max_jp.len(), coef.jp.len()].iter().any(|&len| len != ne) {
            return None;
        }
        let prefix: Vec<_> = (0..5)
            .map(|kind| {
                Prefix::new((0..ne).map(|e| {
                    let jp = if kind == 0 { coef.max_jp[e] } else { coef.jp[e][kind - 1] };
                    // These enclose products of the represented nonnegative
                    // Coef values; their native-chain allowance remains in eps.
                    if !nonnegative(coef.k[e]) || !nonnegative(coef.z[e]) || !nonnegative(jp) {
                        return (f64::NAN, f64::NAN);
                    }
                    let lo = ((coef.k[e] * coef.z[e]).next_down().max(0.0) * jp).next_down().max(0.0);
                    let hi = mul(coef.k[e], coef.z[e]).and_then(|v| mul(v, jp)).unwrap_or(f64::INFINITY);
                    (lo, hi)
                }))
            })
            .collect::<Option<_>>()?;
        let base = prefix[0].interval(0, ne)?;
        let mut specs = Vec::new();
        let mut indexes = HashMap::new();
        let mut used: Vec<[bool; 5]> = Vec::new();
        for part in fine.contrib.iter().flatten().flatten() {
            for row in &part.rush {
                let key = Rc::as_ptr(&row.spec) as usize;
                let i = *indexes.entry(key).or_insert_with(|| {
                    let i = specs.len();
                    specs.push(row.spec.clone());
                    used.push([false; 5]);
                    i
                });
                for (kind, factor) in std::iter::once(row.note).chain(row.judge).enumerate() {
                    if !nonnegative(factor) {
                        return None;
                    }
                    used[i][kind] |= factor != 0.0;
                }
            }
        }
        if specs.is_empty() || specs.len() > MAX_SPECS || specs.iter().any(|s| !(1..=4).contains(&s.gate)) {
            return None;
        }
        let dimensions: Vec<_> = used
            .iter()
            .enumerate()
            .flat_map(|(spec, kinds)| {
                kinds.iter().enumerate().filter_map(move |(kind, &yes)| yes.then_some((spec, kind)))
            })
            .collect();
        let part_count = fine.contrib.iter().try_fold(0usize, |sum, row| sum.checked_add(row.len().checked_mul(5)?))?;
        if part_count.checked_mul(dimensions.len().checked_add(specs.len().checked_mul(4)?)?.checked_add(5)?)?
            > MAX_CELLS
        {
            return None;
        }
        let source = Rc::new(());
        let mut parts = Vec::with_capacity(fine.contrib.len());
        for classes in &fine.contrib {
            let mut compiled = Vec::with_capacity(classes.len());
            for slots in classes {
                let mut outputs = Vec::with_capacity(5);
                for p in slots {
                    let mut h = p.budget;
                    if !nonnegative(h) {
                        return None;
                    }
                    for w in &p.windows {
                        if w.rush != 0 {
                            p.rush.get(w.rush as usize - 1)?;
                            continue;
                        }
                        for (kind, factor) in std::iter::once(w.note).chain(w.judge).enumerate() {
                            h = add(h, mul(factor, prefix[kind].interval(w.lo as usize, w.hi as usize)?)?)?;
                        }
                    }
                    let mut alpha = vec![0.0; dimensions.len()];
                    let mut margin = MarginPacket {
                        source: Some(source.clone()),
                        plain_ops: p.ops_plain,
                        plain_cmds: p.cmds_plain,
                        peak: raw::peak(&p.spans, 0)?,
                        near: raw::peak(&p.spans, 40)?,
                        runs: vec![RunCounts::default(); specs.len()],
                    };
                    if !nonnegative(margin.plain_ops) || !nonnegative(margin.plain_cmds) {
                        return None;
                    }
                    for row in &p.rush {
                        let spec = *indexes.get(&(Rc::as_ptr(&row.spec) as usize))?;
                        for (d, &(s, kind)) in dimensions.iter().enumerate() {
                            if s == spec {
                                let factor = if kind == 0 { row.note } else { row.judge[kind - 1] };
                                alpha[d] = add(alpha[d], factor)?;
                            }
                        }
                        if row.run_cap {
                            let counts = &mut margin.runs[spec];
                            counts.ops = add(counts.ops, row.ops)?;
                            counts.cmds = add(counts.cmds, row.cmds)?;
                            counts.ops_per_run = add(counts.ops_per_run, row.ops_per_run)?;
                            counts.cmds_per_run = add(counts.cmds_per_run, row.cmds_per_run)?;
                        } else {
                            margin.plain_ops = add(margin.plain_ops, row.ops)?;
                            margin.plain_cmds = add(margin.plain_cmds, row.cmds)?;
                        }
                    }
                    outputs.push(LinearPart { base: h, alpha, margin });
                }
                compiled.push(outputs.try_into().ok()?);
            }
            parts.push(compiled);
        }
        Some(Self {
            source,
            base,
            chain_extra: fine.chain_extra,
            times: coef.times.clone(),
            prefix,
            specs,
            dimensions,
            parts,
            class_of: fine.class_of.clone(),
            windows: RefCell::new(rush::WindowCache::default()),
            summaries: RefCell::new(HashMap::new()),
        })
    }
    /// Raw no-skill coefficient. In particular, this does not reuse a0 after the
    /// optional pool-wide additive drift transformation.
    pub(crate) fn base(&self) -> f64 {
        self.base
    }
    pub(crate) fn part(&self, member: usize, choice: usize, position: usize) -> Option<&LinearPart> {
        let class = if choice == 0 { 0 } else { *self.class_of.get(member)?.get(choice - 1)? as usize };
        self.parts.get(member)?.get(class)?.get(position)
    }
    pub(crate) fn mask_summary(&self, masks: &RushMasks) -> Option<Rc<MaskSummary>> {
        let key = Rc::as_ptr(masks) as usize;
        if let Some(found) = self.summaries.borrow().get(&key) {
            return Some(found.summary.clone());
        }
        let mut integrals = Vec::with_capacity(self.specs.len());
        let mut runs = Vec::with_capacity(self.specs.len());
        for spec in &self.specs {
            let windows = rush::cached_windows(&mut self.windows.borrow_mut(), spec, masks, &self.times)?;
            let mut values = [0.0; 5];
            for &(lo, hi, mult) in windows.iter() {
                for (kind, value) in values.iter_mut().enumerate() {
                    *value = add(*value, mul(mult, self.prefix[kind].interval(lo as usize, hi as usize)?)?)?;
                }
            }
            integrals.push(values);
            // A rounded u64->f64 cast is enclosed by one upward successor.
            runs.push((masks.max_runs[(spec.gate - 1) as usize] as f64).next_up());
        }
        let summary = Rc::new(MaskSummary {
            source: self.source.clone(),
            coefficients: self.dimensions.iter().map(|&(spec, kind)| integrals[spec][kind]).collect(),
            runs,
            chain_extra: self.chain_extra,
        });
        let mut cache = self.summaries.borrow_mut();
        if cache.len() >= MAX_CACHE {
            cache.clear();
        }
        cache.insert(key, SummaryCache { _masks: masks.clone(), summary: summary.clone() });
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(source: &Rc<()>, runs: Vec<f64>, coefficients: Vec<f64>) -> MaskSummary {
        MaskSummary { source: source.clone(), coefficients, runs, chain_extra: GK_CHAIN_EPS }
    }

    #[test]
    fn positive_integrals_cover_small_intervals_and_overlapping_windows() {
        let exact = [1u64 << 53, 1, 3, 2, 7, 1];
        let prefix = Prefix::new(exact.iter().map(|&n| (n as f64, n as f64))).unwrap();
        for lo in 0..exact.len() {
            for hi in lo..=exact.len() {
                let sum: u64 = exact[lo..hi].iter().sum();
                assert!(prefix.interval(lo, hi).unwrap() >= sum as f64);
            }
        }
        // Independent integer evaluation of two overlapping score windows.
        let c = add(prefix.interval(1, 5).unwrap(), mul(2.0, prefix.interval(3, 6).unwrap()).unwrap()).unwrap();
        let exact_sum: u64 = exact[1..5].iter().sum::<u64>() + 2 * exact[3..6].iter().sum::<u64>();
        assert!(c >= exact_sum as f64);
        assert_eq!(prefix.interval(3, 3), Some(0.0));
        assert!(prefix.interval(5, 3).is_none());
        assert!(Prefix::new([(0.0, f64::INFINITY)].into_iter()).is_none());
    }

    #[test]
    fn future_component_max_and_fixed_sum_cover_each_possible_command_program() {
        let source = Rc::new(());
        let packet = |ops, cmds, per, peak, near| MarginPacket {
            source: Some(source.clone()),
            plain_ops: ops,
            plain_cmds: cmds,
            peak,
            near,
            runs: vec![RunCounts { ops: 1000.0, cmds: 200.0, ops_per_run: per, cmds_per_run: per / 5.0 }],
        };
        let fixed = packet(12.0, 6.0, 30.0, 1.0, 1.5);
        let alternatives = [packet(3.0, 8.0, 11.0, 4.0, 5.0), packet(20.0, 2.0, 40.0, 0.5, 1.0)];
        let mut future = MarginPacket::default();
        for part in &alternatives {
            future.max_assign(part).unwrap();
        }
        let mut cap = fixed.clone();
        cap.add_assign(&future).unwrap();
        for runs in [0.0, 1.0, 3.0, 50.0] {
            let mask = summary(&source, vec![runs], vec![]);
            for part in &alternatives {
                let mut actual = fixed.clone();
                actual.add_assign(part).unwrap();
                assert!(cap.drift(&mask).unwrap() >= actual.drift(&mask).unwrap());
            }
        }
        let mask = summary(&source, vec![3.0], vec![]);
        let counts = alternatives[0].counts(&mask).unwrap();
        assert!(counts.0 >= 3.0 + 3.0 * 11.0);
        assert!(counts.1 >= 8.0 + 3.0 * 11.0 / 5.0);
    }

    #[test]
    fn packet_margin_covers_native_multirun_accumulation_and_rejects_unbounded_feedback() {
        use ournotes_sim::live::score::ScoreFactorState;
        use ournotes_sim::live::skill::{FactorCommand, apply_factor};
        let source = Rc::new(());
        let mut state = ScoreFactorState::new(1000);
        let count = 2048;
        for _ in 0..count {
            apply_factor(&mut state, &FactorCommand { note_mill: 140001, ..Default::default() });
            apply_factor(&mut state, &FactorCommand { note_mill: -140001, ..Default::default() });
        }
        let packet = MarginPacket {
            source: Some(source.clone()),
            plain_ops: f64::from(2 * count),
            plain_cmds: f64::from(2 * count),
            peak: 1.40001,
            near: 2.80002,
            runs: vec![],
        };
        let mask = summary(&source, vec![], vec![]);
        assert!(packet.drift(&mask).unwrap() >= (state.note_score_up as f64 - 1.0).abs());
        let huge = MarginPacket { plain_ops: 2f64.powi(25), ..packet };
        assert!(huge.drift(&mask).is_none());
    }

    #[test]
    fn linear_part_keeps_intercept_and_alpha_positive_and_refuses_cross_domain_packets() {
        let source = Rc::new(());
        let mask = summary(&source, vec![], vec![3.0, 5.0]);
        let part = LinearPart {
            base: 7.0,
            alpha: vec![2.0, 4.0],
            margin: MarginPacket { source: Some(source.clone()), ..Default::default() },
        };
        assert!(part.gain(&mask).unwrap() >= 33.0);
        assert!(mask.dot(&[1.0]).is_none());
        assert!(mask.dot(&[-1.0, 0.0]).is_none());
        assert!(mask.dot(&[f64::INFINITY, 0.0]).is_none());
        let foreign = summary(&Rc::new(()), vec![], vec![3.0, 5.0]);
        assert!(part.gain(&foreign).is_none());
        assert!(part.margin.drift(&foreign).is_none());
        let mut other = MarginPacket { source: Some(foreign.source), ..Default::default() };
        assert!(other.add_assign(&part.margin).is_none());
    }
}
