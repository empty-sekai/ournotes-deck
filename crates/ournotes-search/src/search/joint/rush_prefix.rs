//! Last-slot envelopes retain each future LUCK program until after root weighting.
//! Buckets bound physical choices; no representative is substituted in scoring.
use super::*;
use crate::search::luck::LuckOracle;
use crate::search::snaps::rush_linear::{MarginPacket, RushLinear};

const SCALES: usize = 9;
const MAX_VARIANTS: usize = 64;
const MAX_BUCKETS: usize = 128;

struct Bucket {
    alpha: Vec<f64>,
    intercept: [f64; SCALES],
    base: f64,
    power: i64,
}

/// Every field covers the union of physical pairs in this one future variant.
struct Future {
    buckets: Vec<Bucket>,
    margin: MarginPacket,
}

pub(super) struct RushPrefix {
    catalog: Vec<(usize, Option<usize>)>,
    linear: RushLinear,
    scales: [f64; SCALES],
    // profile -> future variant -> native position. Ignoring occupied resources
    // and slot restrictions here enlarges the set, without changing enumeration.
    future: Vec<Vec<[Future; 5]>>,
    variant_count: usize,
    future_bonus: i64,
}

#[derive(Clone)]
pub(crate) struct RushCaps {
    pub(crate) variants: Vec<Option<(i128, i64)>>,
    pub(crate) whole: Option<(i128, i64)>,
}

fn finite(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn product(a: f64, b: f64) -> Option<f64> {
    finite((a * b).next_up())
}

fn sum(a: f64, b: f64) -> Option<f64> {
    finite((a + b).next_up())
}

fn integer_cap(value: f64) -> Option<i128> {
    // Native admitted total scores fit i32; retaining this much larger explicit
    // numeric guard avoids saturating casts being mistaken for certificates.
    (value.is_finite() && (0.0..=1e30).contains(&value)).then(|| value.ceil() as i128)
}

impl RushPrefix {
    pub(super) fn compile(
        b: &JointBounds,
        domain: &CandidateDomain,
        oracle: &LuckOracle,
        budget: SearchBudget,
    ) -> Option<Self> {
        let fine = b.fine.as_ref().filter(|f| f.rush_eligible())?;
        let linear = RushLinear::compile(fine)?;
        let nv = oracle.variant_count();
        if nv == 0 || nv > MAX_VARIANTS || b.lead.len().checked_mul(nv)?.checked_mul(5)? > 25_000 {
            return None;
        }
        // Denser scales than the generic prefix relaxation, still immutable and
        // shared by all queries of this compiled domain.
        let scales = std::array::from_fn(|i| b.correlation_scales[1] * 2f64.powf((i as f64 - 4.0) * 0.5));
        if scales.iter().any(|&r| !r.is_finite() || r <= 0.0) {
            return None;
        }
        let mut future = Vec::with_capacity(b.lead.len());
        for profile in 0..b.lead.len() {
            if budget.expired() {
                return None;
            }
            let mut groups: Vec<[Future; 5]> = (0..nv)
                .map(|_| std::array::from_fn(|_| Future { buckets: Vec::new(), margin: MarginPacket::default() }))
                .collect();
            let mut indexes: Vec<[HashMap<Vec<u64>, usize>; 5]> =
                (0..nv).map(|_| std::array::from_fn(|_| HashMap::new())).collect();
            for &m in domain.members() {
                if budget.expired() {
                    return None;
                }
                for choice in 0..=domain.snaps().len() {
                    let snap = (choice > 0).then(|| domain.snaps()[choice - 1]);
                    let Some(v) = oracle.variant(m, snap).map(|v| v as usize) else {
                        continue;
                    };
                    let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                    if power < 0 {
                        return None;
                    }
                    for pos in 0..5 {
                        let part = linear.part(m, choice, pos)?;
                        let key: Vec<_> = part.alpha.iter().map(|v| v.to_bits()).collect();
                        let group = &mut groups[v][pos];
                        group.margin.max_assign(&part.margin)?;
                        let next = group.buckets.len();
                        let index = *indexes[v][pos].entry(key).or_insert(next);
                        if index == next {
                            if next >= MAX_BUCKETS {
                                return None;
                            }
                            group.buckets.push(Bucket {
                                alpha: part.alpha.clone(),
                                intercept: [0.0; SCALES],
                                base: 0.0,
                                power: 0,
                            });
                        }
                        let bucket = &mut group.buckets[index];
                        bucket.power = bucket.power.max(power);
                        bucket.base = bucket.base.max(part.base);
                        for (i, &r) in scales.iter().enumerate() {
                            let value = sum(power as f64, product(r, part.base)?)?;
                            bucket.intercept[i] = bucket.intercept[i].max(value);
                        }
                    }
                }
            }
            future.push(groups);
        }
        let future_bonus = b.points.as_ref().map_or(0, |pt| {
            domain
                .members()
                .iter()
                .flat_map(|&m| {
                    (0..=domain.snaps().len())
                        .map(move |choice| pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] })
                })
                .max()
                .unwrap_or(0)
        });
        Some(Self { catalog: oracle.catalog().to_vec(), linear, scales, future, variant_count: nv, future_bonus })
    }

    fn at_root(
        &self,
        b: &JointBounds,
        domain: &CandidateDomain,
        deck: &PhysicalDeck,
        positions: &[usize; 5],
        variant: usize,
        masks: &super::super::snaps::RushMasks,
    ) -> Option<(i128, i64)> {
        let profile = b.profile[deck.members[2]];
        let group = &self.future[profile][variant][positions[4]];
        if group.buckets.is_empty() {
            return None;
        }
        let summary = self.linear.mask_summary(masks)?;
        let mut coefficient = self.linear.base();
        let mut fixed_power = 0i64;
        let mut margin = group.margin.clone();
        let mut bonus = 0i64;
        for &slot in &SLOTS[..4] {
            let m = deck.members[slot];
            let choice = match deck.snaps[slot] {
                None => 0,
                Some(s) => domain.snaps().iter().position(|&v| v == s)? + 1,
            };
            let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
            fixed_power = fixed_power.checked_add(power)?;
            let part = self.linear.part(m, choice, positions[slot])?;
            coefficient = sum(coefficient, part.gain(&summary)?)?;
            margin.add_assign(&part.margin)?;
            if let Some(pt) = &b.points {
                bonus = bonus.checked_add(pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] })?;
            }
        }
        // The drift is absolute in score-up units: each entry's no-skill coefficient (summed in `base`) gains at
        // most `drift` of it; the chain allowance stays relative.
        coefficient = sum(coefficient, product(margin.drift(&summary)?, self.linear.base())?)?;
        let factor = sum(1.0, summary.chain()?)?;
        let mut future_gain = 0f64;
        let mut future_power = 0;
        let mut future_weight = [0f64; SCALES];
        for bucket in &group.buckets {
            let dot = summary.dot(&bucket.alpha)?;
            future_gain = future_gain.max(sum(bucket.base, dot)?);
            future_power = future_power.max(bucket.power);
            for (i, &r) in self.scales.iter().enumerate() {
                future_weight[i] = future_weight[i].max(sum(bucket.intercept[i], product(r, dot)?)?);
            }
        }
        let power = fixed_power.checked_add(future_power)?;
        let mut score = product(product(power as f64, sum(coefficient, future_gain)?)?, factor)?;
        for (i, &r) in self.scales.iter().enumerate() {
            let w = sum(sum(fixed_power as f64, product(r, coefficient)?)?, future_weight[i])?;
            let denominator = (4.0 * r).next_down();
            if denominator <= 0.0 {
                continue;
            }
            let square = product(w, w)?;
            score = score.min(product((square / denominator).next_up(), factor)?);
        }
        let score = integer_cap(score)?;
        let cap = if let Some(pt) = &b.points {
            // A separate unrestricted last-pair bonus maximum is conservative.
            // Convert each root cap to PT before weighting its declared mass.
            let percentage = bonus.checked_add(self.future_bonus)?.checked_add(10000)?;
            (percentage as i128).checked_mul(pt.multiplier_at(score) as i128)? / 10000
        } else {
            score
        };
        Some((cap, power))
    }

    #[allow(clippy::too_many_arguments)]
    fn caps(
        &self,
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        deck: &PhysicalDeck,
        orders: &[([usize; 5], u128)],
        oracle: &mut LuckOracle,
        budget: SearchBudget,
    ) -> Result<Option<RushCaps>, Error> {
        // Equal physical representatives mean the deterministic pair catalogue
        // assigned the same IDs. This permits a fresh request-local oracle for
        // the same immutable compiled problem, without pointer-identity aliasing.
        if self.catalog != oracle.catalog() {
            return Ok(None);
        }
        let mut ids = [0u32; 5];
        for &slot in &SLOTS[..4] {
            let Some(v) = oracle.variant(deck.members[slot], deck.snaps[slot]) else { return Ok(None) };
            ids[slot] = v;
        }
        // Establish which future variants still have a legal physical pair.
        // This scans the original choice domain once, without ranking or
        // truncation. Avoid replaying a rare program whose character or only
        // compatible Snap resource is already occupied by the fixed prefix.
        let mut feasible = vec![false; self.variant_count];
        let mut unsupported = false;
        let missing: Vec<_> =
            domain.required().iter().copied().filter(|m| !SLOTS[..4].iter().any(|&s| deck.members[s] == *m)).collect();
        for &(m, choice) in &b.choices {
            let character = pool.members[m].character_id;
            if missing.len() > 1
                || missing.first().is_some_and(|&r| r != m)
                || SLOTS[..4].iter().any(|&s| pool.members[deck.members[s]].character_id == character)
                || domain.required().iter().any(|&r| r != m && pool.members[r].character_id == character)
                || !b.allows(SLOTS[4], choice)
            {
                continue;
            }
            let snap = (choice > 0).then(|| domain.snaps()[choice - 1]);
            if snap.is_some() && SLOTS[..4].iter().any(|&s| deck.snaps[s] == snap) {
                continue;
            }
            match oracle.variant(m, snap) {
                Some(v) => feasible[v as usize] = true,
                None => unsupported = true,
            }
        }
        let mut caps = vec![None; self.variant_count];
        for (v, cap) in caps.iter_mut().enumerate().filter(|&(v, _)| feasible[v]) {
            if budget.expired() {
                break;
            }
            ids[4] = v as u32;
            let mut total = Some(0i128);
            let mut power = 0;
            for (positions, weight) in orders {
                if budget.expired() {
                    total = None;
                    break;
                }
                let Some(masks) = oracle.masks_for_variants(pool, ids, positions)? else {
                    total = None;
                    break;
                };
                let Some((value, p)) = self.at_root(b, domain, deck, positions, v, &masks) else {
                    total = None;
                    break;
                };
                power = power.max(p);
                total = total.and_then(|n| value.checked_mul(i128::try_from(*weight).ok()?)?.checked_add(n));
            }
            *cap = total.map(|value| (value, power));
        }
        let whole = if !unsupported && feasible.iter().enumerate().all(|(v, &yes)| !yes || caps[v].is_some()) {
            caps.iter().flatten().copied().max()
        } else {
            None
        };
        Ok(Some(RushCaps { variants: caps, whole }))
    }
}

impl JointBounds {
    /// The caller has fixed the first four physical slots in SLOTS order.
    pub(crate) fn rush_prefix_caps(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        deck: &PhysicalDeck,
        orders: &[([usize; 5], u128)],
        oracle: &mut LuckOracle,
        budget: SearchBudget,
    ) -> Result<Option<RushCaps>, Error> {
        if budget.expired() || !self.rush_eligible() {
            return Ok(None);
        }
        let table = self.rush_prefix.get_or_init(|| RushPrefix::compile(self, domain, oracle, budget));
        match table {
            Some(table) => table.caps(self, pool, domain, deck, orders, oracle, budget),
            None => Ok(None),
        }
    }
}

#[cfg(feature = "search-diagnostics")]
impl JointBounds {
    /// Diagnostics: the cheap and LUCK prefix caps along one complete deck's search path (prefixes in SLOTS order),
    /// per native order, with the joint relaxation split into power, the placed slots' gains, the free part and the
    /// relative margin. Never used for pruning.
    pub(crate) fn prefix_profile(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        deck: &PhysicalDeck,
        orders: &[([usize; 5], u128)],
        mut oracle: Option<&mut LuckOracle>,
    ) -> Result<serde_json::Value, Error> {
        let choices: [usize; 5] = std::array::from_fn(|s| {
            deck.snaps[s].map_or(0, |x| domain.snaps().iter().position(|&v| v == x).expect("compiled Snap") + 1)
        });
        let text = |v: Option<i128>| v.map(|v| v.to_string());
        let mut depths = Vec::new();
        for depth in 1..=5 {
            let mut roots = Vec::new();
            for (positions, weight) in orders {
                let (power, gain, bonus) = self.relax(pool, domain, deck, depth, &SLOTS[depth..], positions, None);
                let fixed = SLOTS[..depth]
                    .iter()
                    .fold(0.0, |a, &s| add_up(a, self.gains[deck.members[s]][choices[s]][positions[s]]));
                let coefficient = add_up(self.a0, gain).min(self.global);
                let mut row = serde_json::json!({
                    "positions": positions, "weight": weight.to_string(), "power": power, "gain": gain,
                    "fixedGain": fixed, "coefficient": coefficient,
                    "capWithoutMargin": ((power as f64) * coefficient).ceil(),
                    "cap": self.payoff_cap(power, gain, bonus).to_string(),
                });
                if depth < 5 {
                    row["correlated"] = self.correlated_upper(pool, domain, deck, depth, positions).to_string().into();
                    row["character"] = text(self.character_prefix_upper(pool, domain, deck, depth, positions)).into();
                    row["resource"] = text(self.resource_prefix_upper(pool, domain, deck, depth, positions)).into();
                    if let Some(state) = self.tail_state(pool, domain, deck, depth, &[(*positions, 1)]) {
                        let slot = SLOTS[depth];
                        row["pairNext"] =
                            self.pair_upper(&state, deck.members[slot], choices[slot])?.0.to_string().into();
                    }
                } else if let Some(fine) = &self.fine {
                    let masks = match oracle.as_deref_mut() {
                        Some(o) if self.rush_eligible() => o.masks(pool, deck, positions)?,
                        _ => None,
                    };
                    let mut scratch = JointScratch::default();
                    row["fineUnion"] =
                        text(self.fine_upper(domain, deck, power, positions, &mut scratch, masks.as_ref())).into();
                    if let Some(o) = oracle.as_deref_mut()
                        && let Some(branches) = o.branches(pool, deck, positions)?
                    {
                        let caps: Vec<_> = branches
                            .iter()
                            .map(|m| text(self.fine_upper(domain, deck, power, positions, &mut scratch, Some(m))))
                            .collect();
                        row["fineBranches"] = caps.into();
                    }
                    row["raw"] = text(self.raw_upper(domain, deck, power, positions)).into();
                    row["linear"] = fine.linear_profile(deck.members, choices, positions, masks.as_ref());
                }
                roots.push(row);
            }
            let mut entry = serde_json::json!({"depth": depth, "roots": roots,
                "expected": self.expected_upper(pool, domain, deck, depth, orders)?.0.to_string()});
            if depth < 5 {
                entry["correlatedExpected"] =
                    self.correlated_expected_upper(pool, domain, deck, depth, orders)?.to_string().into();
                entry["resourceExpected"] =
                    text(self.resource_expected_upper(pool, domain, deck, depth, orders)).into();
            }
            if depth == 4
                && let Some(o) = oracle.as_deref_mut()
            {
                let budget = SearchBudget::new(Instant::now(), None)?;
                if let Some(caps) = self.rush_prefix_caps(pool, domain, deck, orders, o, budget)? {
                    let own = o.variant(deck.members[SLOTS[4]], deck.snaps[SLOTS[4]]);
                    entry["rushPrefixWhole"] = text(caps.whole.map(|c| c.0)).into();
                    entry["rushPrefixOwnVariant"] =
                        text(own.and_then(|v| caps.variants[v as usize]).map(|c| c.0)).into();
                    entry["rushPrefixVariants"] = caps.variants.iter().filter(|v| v.is_some()).count().into();
                }
            }
            depths.push(entry);
        }
        Ok(serde_json::json!({"a0": self.a0, "global": self.global, "relativeMargin": self.eps,
            "poolMargin": self.fine.as_ref().map(|f| f.pool_margin_profile()), "depths": depths}))
    }
}
