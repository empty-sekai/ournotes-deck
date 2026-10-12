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

/// A copy of a [`LotteryLive`] without its master, its skill events and its total power, binary32 values by bit
/// pattern. The skill events drive only the life recorder, which no recording of a performer with a kind runs. The
/// total power sets the reduced score calculator alone, which a lottery recording never reads: the controller is given
/// a zero current score, no condition reads the score, and the appliers only file score commands into it.
#[derive(PartialEq)]
pub(super) struct HeldLive {
    skills: LuckSkills,
    notes: Vec<LiveNote>,
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
            && self.setup == *live.setup
            && self.ranking.as_deref() == live.ranking
            && self.delta_times.len() == live.delta_times.len()
            && self.delta_times.iter().zip(live.delta_times).all(|(&bits, delta)| bits == delta.to_bits())
            && self.play == *live.play
            && self.skills == *live.skills
    }
}

/// Composed laws by the words of their composition, dropped oldest first once their keys and laws exceed the capacity.
pub(super) struct ComposedLaws {
    entries: FxHashMap<std::sync::Arc<[u64]>, std::sync::Arc<LuckDpCertifiedResult>>,
    order: std::collections::VecDeque<(std::sync::Arc<[u64]>, usize)>,
    bytes: usize,
    capacity: usize,
}

impl ComposedLaws {
    pub(super) fn new(capacity: usize) -> Self {
        Self { entries: FxHashMap::default(), order: std::collections::VecDeque::new(), bytes: 0, capacity }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }

    fn insert(&mut self, key: Vec<u64>, law: std::sync::Arc<LuckDpCertifiedResult>, stats: &mut LotteryKindStats) {
        let bytes = key.len() * std::mem::size_of::<u64>() + law_bytes(&law);
        if bytes > self.capacity {
            return;
        }
        while self.bytes + bytes > self.capacity {
            let (oldest, held) = self.order.pop_front().expect("held laws account for the held bytes");
            self.bytes -= held;
            self.entries.remove(&oldest);
            stats.law_evictions += 1;
        }
        let key: std::sync::Arc<[u64]> = key.into();
        self.bytes += bytes;
        self.order.push_back((key.clone(), bytes));
        self.entries.insert(key, law);
        stats.peak_laws = stats.peak_laws.max(self.entries.len());
        stats.peak_law_bytes = stats.peak_law_bytes.max(self.bytes);
    }
}

/// The bytes a law holds.
fn law_bytes(law: &LuckDpCertifiedResult) -> usize {
    use std::mem::size_of_val;
    std::mem::size_of::<LuckDpCertifiedResult>()
        + size_of_val(&law.probe_transitions[..])
        + size_of_val(&law.rush_transitions[..])
        + size_of_val(&law.steps[..])
        + size_of_val(&law.frame_queries[..])
        + size_of_val(&law.probes[..])
        + size_of_val(&law.range_moments[..])
}

impl LotteryKinds<'_> {
    /// The lottery law of each of `orders` of the five performers of `deck` in `live`, composed from the
    /// contributions of their kinds: `order[q]` is the index in `deck` of the performer at position `q`. None when a
    /// performer has no kind, or when the gauge speeds or gauge actions of an order may not compose.
    ///
    /// The law of an order is the propagation of its transcript ([`record_skeleton`], [`file`]); the composed law
    /// encloses it:
    ///
    /// - No performer converts a judgement and no action reads the life (see the [`kinds`](super::kinds) module), so
    ///   the skeleton is the native run of the performers' factor skills alone. Its controller's range states follow
    ///   the fevers and the frame times, and its judgements are those of the play. Each performer's own recording
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
    /// The composition reads the contributions of the order's positions alone, so orders and formations with the same
    /// contributions at every position share their composed law.
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
        self.stats.teams += 1;
        let mut kinds = [LotteryKind(0); POSITIONS];
        for (kind, performer) in kinds.iter_mut().zip(deck) {
            match self.held_role(live, performer) {
                LotteryRole::Kind(found) => *kind = found,
                LotteryRole::Breaks(_) => {
                    self.stats.unkinded += 1;
                    return Ok(None);
                }
            }
        }
        let mut laws: Vec<std::sync::Arc<LuckDpCertifiedResult>> = Vec::with_capacity(orders.len());
        let mut keys = Vec::<Vec<u64>>::with_capacity(orders.len());
        for order in orders {
            let composition =
                self.composition(live, std::array::from_fn(|position| (kinds[order[position]], position)));
            if let Some(index) = keys.iter().position(|known| *known == composition.key) {
                laws.push(laws[index].clone());
                keys.push(composition.key);
                continue;
            }
            self.stats.law_lookups += 1;
            if let Some(law) = self.laws.entries.get(&composition.key[..]) {
                self.stats.law_hits += 1;
                laws.push(law.clone());
                keys.push(composition.key);
                continue;
            }
            let key = composition.key.clone();
            let Some(transcript) = self.transcript(composition) else {
                self.stats.uncomposed += 1;
                return Ok(None);
            };
            self.stats.composed += 1;
            let law = curves.law_of(&transcript, &mut || false)?.expect("an uncancelled propagation");
            self.laws.insert(key.clone(), law.clone(), &mut self.stats);
            laws.push(law);
            keys.push(key);
        }
        Ok(Some(laws))
    }

    /// The composition of the contributions of `(kind, position)` at each position.
    pub(super) fn composition(&self, live: &LotteryLive<'_>, at: [(LotteryKind, usize); POSITIONS]) -> Composition {
        let contributions = at.map(|(kind, position)| &self.contributions[kind.index()][position]);
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
