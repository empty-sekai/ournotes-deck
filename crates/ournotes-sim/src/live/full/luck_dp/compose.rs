//! Lottery laws composed from the contributions of performers ([`LotteryKinds`]).

use super::*;

/// The live of a lottery recording: everything it plays except the formation.
#[derive(Clone, Copy)]
pub struct LotteryLive<'a> {
    pub master: &'a Master,
    pub skills: &'a LuckSkills,
    pub notes: &'a [LiveNote],
    pub events: &'a [(i32, i32)],
    pub params: LiveParams,
    pub setup: &'a GekisouSetup,
    pub play: &'a LivePlay,
    pub delta_times: &'a [f32],
    pub ranking: Option<&'a [crate::replay::RankConfirmation]>,
}

impl LotteryLive<'_> {
    /// The skeleton of the reduced performers `reduced` without a life recorder ([`record_skeleton`]), which reads no
    /// skill event.
    pub(super) fn skeleton(&self, reduced: &[Performer]) -> Result<Skeleton, Error> {
        let Self { master, skills, notes, events, params, setup, play, delta_times, ranking } = *self;
        let recorded = record_skeleton(
            master,
            skills,
            notes,
            events,
            params,
            setup,
            play,
            delta_times,
            reduced,
            None,
            ranking,
            &mut || false,
        )?;
        Ok(recorded.expect("an uncancelled recording"))
    }
}

/// A copy of a [`LotteryLive`] without its master and its total power, binary32 values by bit pattern. The total
/// power sets the reduced score calculator alone, which a lottery recording never reads: the controller is given a
/// zero current score, no condition reads the score, and the appliers only file score commands into it.
#[derive(PartialEq)]
pub(super) struct HeldLive {
    skills: LuckSkills,
    notes: Vec<LiveNote>,
    events: Vec<(i32, i32)>,
    params: (i64, i32, i32, i32, Option<i32>, u32),
    setup: GekisouSetup,
    play: LivePlay,
    delta_times: Vec<u32>,
    ranking: Option<Vec<crate::replay::RankConfirmation>>,
}

impl HeldLive {
    fn params(params: LiveParams) -> (i64, i32, i32, i32, Option<i32>, u32) {
        let LiveParams {
            skill_target_music_type,
            total_power: _,
            music_level,
            converted_note_count,
            music_length_ms,
            score_music_length_ms,
            assist_factor,
        } = params;
        (
            skill_target_music_type,
            music_level,
            converted_note_count,
            music_length_ms,
            score_music_length_ms,
            assist_factor.to_bits(),
        )
    }

    pub(super) fn new(live: &LotteryLive<'_>) -> Self {
        Self {
            skills: live.skills.clone(),
            notes: live.notes.to_vec(),
            events: live.events.to_vec(),
            params: Self::params(live.params),
            setup: live.setup.clone(),
            play: live.play.clone(),
            delta_times: live.delta_times.iter().map(|delta| delta.to_bits()).collect(),
            ranking: live.ranking.map(<[_]>::to_vec),
        }
    }

    pub(super) fn is(&self, live: &LotteryLive<'_>) -> bool {
        self.params == Self::params(live.params)
            && self.notes == live.notes
            && self.events == live.events
            && self.setup == *live.setup
            && self.ranking.as_deref() == live.ranking
            && self.delta_times.len() == live.delta_times.len()
            && self.delta_times.iter().zip(live.delta_times).all(|(&bits, delta)| bits == delta.to_bits())
            && self.play == *live.play
            && self.skills == *live.skills
    }
}

/// The identities of the contributions an order composes, sorted ([`KindEntry::ids`]).
type OrderIds = [u32; POSITIONS];

/// The law, or none, an order's contributions compose to.
type Composed = Option<std::sync::Arc<LuckDpCertifiedResult>>;

/// The bytes one entry of [`ComposedLaws::orders`] holds, with its share of the table.
const ORDER_BYTES: usize = 2 * std::mem::size_of::<(OrderIds, Composed)>();

/// Composed laws by the words of their composition, and the law each order's contributions compose to; the laws are
/// dropped oldest first once the held bytes exceed the capacity, and then every order's with them. Laws that share
/// their curves ([`LuckDpCertifiedResult::steps`] and [`LuckDpCertifiedResult::frame_queries`]) count them once.
pub(super) struct ComposedLaws {
    entries: FxHashMap<std::sync::Arc<[u64]>, std::sync::Arc<LuckDpCertifiedResult>>,
    /// The held laws oldest first: key, bytes of their own and the address of their curves.
    order: std::collections::VecDeque<(std::sync::Arc<[u64]>, usize, usize)>,
    /// The held laws and the bytes of each curve held, by its address.
    curves: FxHashMap<usize, (usize, usize)>,
    orders: FxHashMap<OrderIds, Composed>,
    bytes: usize,
    capacity: usize,
}

impl ComposedLaws {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            entries: FxHashMap::default(),
            order: std::collections::VecDeque::new(),
            curves: FxHashMap::default(),
            orders: FxHashMap::default(),
            bytes: 0,
            capacity,
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.curves.clear();
        self.orders.clear();
        self.bytes = 0;
    }

    /// Make room for `bytes` more, dropping the oldest laws and every order's; false when they exceed the capacity.
    fn reserve(&mut self, bytes: usize, stats: &mut LotteryKindStats) -> bool {
        if bytes > self.capacity {
            return false;
        }
        while self.bytes + bytes > self.capacity {
            if !self.orders.is_empty() {
                self.bytes -= self.orders.len() * ORDER_BYTES;
                self.orders.clear();
                continue;
            }
            let (oldest, held, curve) = self.order.pop_front().expect("held laws account for the held bytes");
            self.bytes -= held;
            let (laws, bytes) = self.curves.get_mut(&curve).expect("a held law's curve");
            *laws -= 1;
            if *laws == 0 {
                self.bytes -= *bytes;
                self.curves.remove(&curve);
            }
            self.entries.remove(&oldest);
            stats.law_evictions += 1;
        }
        true
    }

    /// Hold `law` under its composition key; false when it exceeds the capacity.
    fn insert(
        &mut self,
        key: Vec<u64>,
        law: std::sync::Arc<LuckDpCertifiedResult>,
        stats: &mut LotteryKindStats,
    ) -> bool {
        let bytes = key.len() * std::mem::size_of::<u64>() + law_bytes(&law);
        let (curve, curve_bytes) = (curve_address(&law), curve_bytes(&law));
        // Room for the curves too unless held, also when making room drops the laws that held them.
        let held = self.curves.contains_key(&curve);
        if !self.reserve(bytes + if held { 0 } else { curve_bytes }, stats)
            || (held && !self.curves.contains_key(&curve) && !self.reserve(bytes + curve_bytes, stats))
        {
            return false;
        }
        let key: std::sync::Arc<[u64]> = key.into();
        let (laws, _) = self.curves.entry(curve).or_insert((0, curve_bytes));
        if *laws == 0 {
            self.bytes += curve_bytes;
        }
        *laws += 1;
        self.bytes += bytes;
        self.order.push_back((key.clone(), bytes, curve));
        self.entries.insert(key, law);
        stats.peak_laws = stats.peak_laws.max(self.entries.len());
        stats.peak_law_bytes = stats.peak_law_bytes.max(self.bytes);
        true
    }

    /// Hold what the contributions `ids` compose to: `composed`, a law held under its composition key, or none.
    fn alias(&mut self, ids: OrderIds, composed: Composed, stats: &mut LotteryKindStats) {
        if self.reserve(ORDER_BYTES, stats) {
            self.bytes += ORDER_BYTES;
            self.orders.insert(ids, composed);
            stats.peak_law_bytes = stats.peak_law_bytes.max(self.bytes);
        }
    }
}

/// The bytes a law holds besides its curves.
fn law_bytes(law: &LuckDpCertifiedResult) -> usize {
    use std::mem::size_of_val;
    std::mem::size_of::<LuckDpCertifiedResult>()
        + size_of_val(&law.probe_transitions[..])
        + size_of_val(&law.rush_transitions[..])
        + size_of_val(&law.probes[..])
        + size_of_val(&law.range_moments[..])
}

/// The bytes of a law's curves.
fn curve_bytes(law: &LuckDpCertifiedResult) -> usize {
    std::mem::size_of_val(&law.steps[..]) + std::mem::size_of_val(&law.frame_queries[..])
}

/// The address of a law's curves, which the laws that share them share.
fn curve_address(law: &LuckDpCertifiedResult) -> usize {
    law.steps.as_ptr() as usize
}

impl LotteryKinds<'_> {
    /// The lottery law of each of `orders` of the five performers of `deck` in `live`, composed from the
    /// contributions of their kinds: `order[q]` is the index in `deck` of the performer at position `q`. None when a
    /// performer has no kind, when conversions of the performers that turn a Perfect and a Just into one another meet
    /// conversions that read such a change ([`Conversions`]), or when the gauge speeds or gauge actions of an order may
    /// not compose.
    ///
    /// The law of an order is the propagation of its transcript ([`record_skeleton`], [`file`]); the composed law
    /// encloses it:
    ///
    /// - The formations composed change no judgement of the play but Perfects and Justs into one another (see the
    ///   [`kinds`](super::kinds) module). The propagation reads a judgement only through the base points of the
    ///   lottery machine, which take Perfect and Just alike, and the skeleton reads none beyond that: the triggers,
    ///   releases and resets a lottery plan admits read the range states and the lottery results, and its conditions
    ///   only chances, the holder and the life ([`compile`]). So the order's law is that of its transcript with the
    ///   judgements of the play.
    /// - No action reads the life (see the [`kinds`](super::kinds) module), so the skeleton is the native run of the
    ///   performers' factor skills alone. Its controller's range states follow the fevers and the frame times. Each
    ///   performer's own recording
    ///   records the skeleton of the live without performers but for the gauge speeds of its hits, and files no lot
    ///   factor; so does the order, with the lot buffs of the live, and its factor skills file their gauge factor
    ///   commands in the same frames and phases and with the same times as in their own recordings: their triggers
    ///   and releases read the range states, and their conditions read only their holder or fold to constants.
    /// - The gauge speed of a hit is the binary32 sum, in filing order, of the gauge factor commands filed before the
    ///   hit is recorded with times no later than its note. Those are the commands each performer's own recording
    ///   has filed by then. When every sum of a subset of all the performers' commands is a binary32 value
    ///   ([`exact_sums`]), every such sum is exact in any order, and the speed is the sum of the performers' own
    ///   speeds at the hit.
    /// - The plan of the order holds the actions of each performer's plan at its position, each filed with its own
    ///   chance, its score probes are those of any performer, and its Miss gauge rows those of any. The propagation
    ///   applies a frame's actions one after another, and their law is the same in every order: a range-start or
    ///   Miss gauge adds a nonnegative amount to the gauge, rolling full gauges into lots, and two such additions
    ///   reach the gauge and lots of adding their total when no sum wraps ([`gauge_actions_commute`]); a result
    ///   guarantee raises the guarantee to its result; and whether a Miss gauge applies depends on the previous
    ///   frame's result and the used flag, which no action changes. The composed transcript files them in the order
    ///   of their words, so its law and the order's are one law, which each propagation encloses.
    ///
    /// The composition reads the contributions of the order's positions alone, and the multiset of them: orders and
    /// formations with the same contributions share their composed law.
    pub(crate) fn order_laws(
        &mut self,
        live: &LotteryLive<'_>,
        deck: &[Performer],
        orders: &[[usize; POSITIONS]],
        curves: &mut LuckDpCache,
    ) -> Result<Option<Vec<std::sync::Arc<LuckDpCertifiedResult>>>, Error> {
        if deck.len() != POSITIONS {
            return Err(Error::Input("an order evaluation takes five performers".into()));
        }
        if !self.enter(live) {
            return Ok(None);
        }
        let mut kinds = [LotteryKind(0); POSITIONS];
        for (kind, performer) in kinds.iter_mut().zip(deck) {
            match self.held_role(live, performer) {
                LotteryRole::Kind(found) => *kind = found,
                LotteryRole::Breaks(_) => {
                    self.stats.teams += 1;
                    self.stats.unkinded += 1;
                    return Ok(None);
                }
            }
        }
        self.kind_laws(live, kinds, orders, curves)
    }

    /// The laws of `orders` of a formation of performers with `kinds` in `live`, whose kinds these are
    /// ([`Self::role`]), but for the orders whose contributions `seen` holds: each law once, and the contributions of
    /// the orders read join `seen`. None as [`Self::order_laws`], `seen` then unchanged. Orders with the same
    /// contributions share their law, so the laws of formations read with one `seen` are those of all their orders.
    pub fn formation_laws(
        &mut self,
        live: &LotteryLive<'_>,
        kinds: [LotteryKind; POSITIONS],
        orders: &[[usize; POSITIONS]],
        curves: &mut LuckDpCache,
        seen: &mut FxHashSet<[u32; POSITIONS]>,
    ) -> Result<Option<Vec<std::sync::Arc<LuckDpCertifiedResult>>>, Error> {
        if !self.live.as_ref().is_some_and(|held| held.is(live)) {
            return Err(Error::Input("lottery kinds are read in the live they were found in".into()));
        }
        if kinds.iter().any(|kind| kind.index() >= self.entries.len()) {
            return Err(Error::Input("a lottery kind of another live".into()));
        }
        if !self.composes(kinds, orders)? {
            return Ok(None);
        }
        let mut fresh: Vec<(OrderIds, usize)> =
            orders.iter().enumerate().map(|(o, order)| (self.order_ids(kinds, order), o)).collect();
        fresh.sort_unstable();
        fresh.dedup_by_key(|(ids, _)| *ids);
        fresh.retain(|(ids, _)| !seen.contains(ids));
        let mut laws: Vec<std::sync::Arc<LuckDpCertifiedResult>> = Vec::with_capacity(fresh.len());
        for &(ids, o) in &fresh {
            match self.order_law(live, kinds, &orders[o], ids, curves)? {
                Some(law) => {
                    if !laws.iter().any(|held| std::sync::Arc::ptr_eq(held, &law)) {
                        laws.push(law);
                    }
                }
                None => {
                    self.stats.uncomposed += 1;
                    return Ok(None);
                }
            }
        }
        seen.extend(fresh.iter().map(|&(ids, _)| ids));
        Ok(Some(laws))
    }

    fn kind_laws(
        &mut self,
        live: &LotteryLive<'_>,
        kinds: [LotteryKind; POSITIONS],
        orders: &[[usize; POSITIONS]],
        curves: &mut LuckDpCache,
    ) -> Result<Option<Vec<std::sync::Arc<LuckDpCertifiedResult>>>, Error> {
        if !self.composes(kinds, orders)? {
            return Ok(None);
        }
        let mut laws: Vec<std::sync::Arc<LuckDpCertifiedResult>> = Vec::with_capacity(orders.len());
        // The contributions met in this formation, with the index of their first order.
        let mut met = FxHashMap::<OrderIds, usize>::default();
        for order in orders {
            let ids = self.order_ids(kinds, order);
            if let Some(&index) = met.get(&ids) {
                laws.push(laws[index].clone());
                continue;
            }
            met.insert(ids, laws.len());
            match self.order_law(live, kinds, order, ids, curves)? {
                Some(law) => laws.push(law),
                None => {
                    self.stats.uncomposed += 1;
                    return Ok(None);
                }
            }
        }
        Ok(Some(laws))
    }

    /// Whether the orders of a formation with `kinds` may compose (counting the formation): false when conversions
    /// that turn a Perfect and a Just into one another meet conversions that read such a change.
    fn composes(&mut self, kinds: [LotteryKind; POSITIONS], orders: &[[usize; POSITIONS]]) -> Result<bool, Error> {
        if orders.iter().flatten().any(|&member| member >= POSITIONS) {
            return Err(Error::Input("an order is a permutation of the five performers".into()));
        }
        self.stats.teams += 1;
        let held =
            |flag: fn(&Conversions) -> bool| kinds.iter().any(|kind| flag(&self.entries[kind.index()].conversions));
        if held(|conversions| conversions.relabels) && held(|conversions| conversions.read_relabels) {
            self.stats.relabel_conflicts += 1;
            return Ok(false);
        }
        Ok(true)
    }

    /// The contributions of `order` of a formation with `kinds`, sorted.
    fn order_ids(&self, kinds: [LotteryKind; POSITIONS], order: &[usize; POSITIONS]) -> OrderIds {
        let mut ids: OrderIds =
            std::array::from_fn(|position| self.entries[kinds[order[position]].index()].ids[position]);
        ids.sort_unstable();
        ids
    }

    /// The law the contributions `ids` of `order` of a formation with `kinds` compose to, or none.
    fn order_law(
        &mut self,
        live: &LotteryLive<'_>,
        kinds: [LotteryKind; POSITIONS],
        order: &[usize; POSITIONS],
        ids: OrderIds,
        curves: &mut LuckDpCache,
    ) -> Result<Composed, Error> {
        self.stats.law_lookups += 1;
        if let Some(composed) = self.laws.orders.get(&ids) {
            self.stats.law_hits += 1;
            return Ok(composed.clone());
        }
        let composition = self.composition(live, std::array::from_fn(|position| (kinds[order[position]], position)));
        let composed = match self.laws.entries.get(&composition.key[..]) {
            Some(law) => {
                self.stats.law_hits += 1;
                Some(law.clone())
            }
            None => {
                let (key, unprobed) = (composition.key.clone(), composition.unprobed_key());
                let law = match self.laws.entries.get(&unprobed[..]) {
                    Some(propagated) => {
                        self.stats.probe_variants += 1;
                        let probes = composition.probes;
                        Some(std::sync::Arc::new(LuckDpCertifiedResult { probes, ..(**propagated).clone() }))
                    }
                    None => match self.transcript(composition) {
                        None => None,
                        Some(transcript) => {
                            self.stats.composed += 1;
                            let law =
                                curves.propagated(&transcript, &mut || false)?.expect("an uncancelled propagation");
                            self.stats.composed_transitions += law.transitions;
                            self.stats.composed_peak_states = self.stats.composed_peak_states.max(law.peak_states);
                            self.laws.insert(unprobed, law.clone(), &mut self.stats);
                            Some(law)
                        }
                    },
                };
                match law {
                    Some(law) if !self.laws.insert(key, law.clone(), &mut self.stats) => return Ok(Some(law)),
                    law => law,
                }
            }
        };
        self.laws.alias(ids, composed.clone(), &mut self.stats);
        Ok(composed)
    }

    /// The composition of the contributions of `(kind, position)` at each position.
    pub(super) fn composition(&self, live: &LotteryLive<'_>, at: [(LotteryKind, usize); POSITIONS]) -> Composition {
        let contributions = at.map(|(kind, position)| &self.entries[kind.index()].contributions[position]);
        let mut parts: Vec<u32> =
            contributions.iter().map(|contribution| contribution.part).filter(|&part| part != 0).collect();
        parts.sort_unstable();
        let mut actions: Vec<Action<ProbabilityMass>> =
            contributions.iter().flat_map(|contribution| contribution.actions.iter().copied()).collect();
        actions.sort_by_key(action_words);
        let mut probes = vec![false; live.skills.shapes.len()];
        for contribution in contributions {
            for (probe, &held) in probes.iter_mut().zip(&contribution.probes) {
                *probe |= held;
            }
        }
        let mut key = Vec::with_capacity(3 + parts.len() + 4 * actions.len() + probes.len());
        key.push(parts.len() as u64);
        key.extend(parts.iter().map(|&part| u64::from(part)));
        key.push(actions.len() as u64);
        actions.iter().for_each(|action| key.extend(action_words(action)));
        key.extend(probes.iter().map(|&probe| u64::from(probe)));
        Composition { parts, actions, probes, key }
    }

    /// The composed transcript of `composition`, or None when its gauge speeds may round or its gauge actions may
    /// wrap.
    pub(super) fn transcript(&self, composition: Composition) -> Option<Transcript<ProbabilityMass>> {
        let base = self.base.as_ref().expect("a live whose performers have a kind");
        let Composition { parts, actions, probes, key: _ } = composition;
        let commands: Vec<(i32, f32)> =
            parts.iter().flat_map(|&part| self.parts[part as usize].commands.iter().copied()).collect();
        if !exact_sums(&commands) || !gauge_actions_commute(base, &actions) {
            return None;
        }
        let mut hits = base.hits.clone();
        for &part in &parts {
            for (hit, &speed) in hits.iter_mut().zip(&self.parts[part as usize].speeds) {
                hit.speed += speed;
            }
        }
        let actions = actions.into_iter().map(|action| (1, action, None)).collect();
        Some(file_hits(base, hits, &Plan { actions, probes, order_pairs: 0 }, false))
    }
}

/// The contributions of the positions of one order composed: the factor parts other than 0 and the actions, each in
/// the order of their words, the score probes any position holds, and the words of all three.
pub(super) struct Composition {
    parts: Vec<u32>,
    actions: Vec<Action<ProbabilityMass>>,
    probes: Vec<bool>,
    key: Vec<u64>,
}

impl Composition {
    /// The words of the parts and actions alone, marked apart from every [`Self::key`]. The propagation of a
    /// composed transcript reads its score probes only to label its result ([`LuckDpCertifiedResult::probes`]):
    /// the certified class masses read the direct-probe class of each state, which follows the Rush at the gated
    /// frames whoever holds a probe. Compositions with the same parts and actions have one propagation, up to that
    /// label.
    fn unprobed_key(&self) -> Vec<u64> {
        let mut key = self.key[..self.key.len() - self.probes.len()].to_vec();
        key.push(u64::MAX);
        key
    }
}

/// Whether the range-start and Miss gauge actions `actions` commute in every state of the Luck ranges of `base`: in
/// each range, under each gauge maximum it can take, every amount they add is nonnegative, and their total added to
/// the most the gauge can hold before them stays a native integer.
///
/// A gauge below the maximum keeps an addition that stays below it, and a gauge that reaches it keeps the remainder
/// and adds the quotient to the lots, so from a gauge `g`, nonnegative amounts `a` and `b` reach the gauge and lots
/// of adding `a + b` in either order when no sum wraps. Before any action the gauge is the range's template gauge
/// or below the current maximum: an addition leaves it below, and a maximum change leaves it at most the new one.
fn gauge_actions_commute(base: &Skeleton, actions: &[Action<ProbabilityMass>]) -> bool {
    let mut luck = base.templates.iter().zip(&base.luck).filter(|&(_, &luck)| luck);
    luck.all(|(template, _)| {
        template.gauge_maxima().into_iter().filter(|&maximum| maximum >= 1).all(|maximum| {
            let mut gauge = i64::from(template.gauge).max(maximum);
            actions.iter().all(|action| match *action {
                Action::StartGauge { value, .. } | Action::MissGauge { value } => {
                    let added = floor_to_i32((maximum as i128 * value as i128) as i32 as f32 / 10000f32);
                    gauge += i64::from(added);
                    added >= 0 && gauge <= i64::from(i32::MAX)
                }
                _ => true,
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn law(probe: bool) -> LuckDpCertifiedResult {
        LuckDpCertifiedResult {
            probe_transitions: Vec::new(),
            rush_transitions: Vec::new(),
            steps: vec![(0, [ProbabilityMass::ONE; 4]); 100].into(),
            frame_queries: vec![(0, [[ProbabilityMass::ONE; 4]; 3]); 100].into(),
            probes: vec![probe],
            probe_starts: F64Interval::ZERO,
            range_moments: Vec::new(),
            peak_states: 1,
            transitions: 1,
        }
    }

    #[test]
    fn laws_that_share_their_curves_hold_them_once() {
        let base = law(false);
        let variant = LuckDpCertifiedResult { probes: vec![true], ..base.clone() };
        let own = |law: &LuckDpCertifiedResult| std::mem::size_of::<u64>() + law_bytes(law);
        let curves = curve_bytes(&base);
        let capacity = own(&base) + own(&variant) + curves;
        let mut stats = LotteryKindStats::default();
        let mut laws = ComposedLaws::new(capacity);
        assert!(laws.insert(vec![1], Arc::new(base), &mut stats));
        assert!(laws.insert(vec![2], Arc::new(variant), &mut stats));
        assert_eq!((stats.peak_laws, stats.law_evictions, laws.bytes), (2, 0, capacity));
        // A law with curves of its own drops both, and their curves with the second.
        let other = law(false);
        let held = own(&other) + curve_bytes(&other);
        assert!(laws.insert(vec![3], Arc::new(other), &mut stats));
        assert_eq!((laws.entries.len(), stats.law_evictions, laws.bytes), (1, 2, held));
        assert_eq!(laws.curves.len(), 1);
    }
}
