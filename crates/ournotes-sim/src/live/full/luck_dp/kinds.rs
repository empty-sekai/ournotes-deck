//! Lottery kinds: what a performer adds to the lottery recordings of a live, apart from the other performers.
//!
//! The lottery law of a formation is the propagation of its transcript: the native skeleton of its lot and gauge
//! factor skills with the actions of its plan filed in ([`record_skeleton`], [`file`]). A performer's contribution at
//! a position is what its own recording adds there, with blank performers before it: the gauge speed its factor skills
//! file at every hit, the actions of its plan, and the score probes it holds. Its kind is its contribution at each of
//! the five positions. [`LotteryKinds`] composes the transcript of a formation whose performers all have a kind from
//! the skeleton of the live without performers and the contribution of each performer at its position (see
//! [`LotteryKinds::order_laws`]), so formations with the same kinds at every position have the same law.
//!
//! A performer takes a kind when its contribution in a formation is its contribution alone:
//!
//! - The checks of a recording on its performers pass for it ([`LotteryBreak::Unsupported`]). Each of these checks
//!   fails for a formation when it fails for one of its performers: they are disjunctions over the performers' skill
//!   lists, and the state identities of different performers never meet (an effect id holds the skill type and the
//!   position below the row id, and a kind refuses row ids whose effect ids wrap).
//! - Its judgement conversions change no judgement of the play but Perfects and Justs into one another
//!   ([`LotteryBreak::Converts`]): whatever their conditions, they make no other change ([`play_conversions`]), or
//!   they change no judgement in a formation whose other performers' conversions change none ([`quiet_conversions`]).
//!   The kind records whether its conversions can turn a Perfect and a Just into one another, and whether they change
//!   none only while no other performer's conversions do ([`Conversions`]); a formation holding both takes no composed
//!   law ([`LotteryKinds::order_laws`]). Every other formation of performers with a kind changes no judgement of the
//!   play but Perfects and Justs into one another.
//! - Its lottery conditions read no performer other than their holder ([`LotteryBreak::ReadsFormation`]), so the
//!   condition skills it brings to a formation are those it brings alone at the same position.
//! - Its skills keep the life in the range the play keeps it in ([`LotteryBreak::LeavesLifeRange`]), and the life
//!   comparisons of its lottery conditions are folded over that range ([`fold_life_conditions`]); a lottery action
//!   that still reads the life refuses it ([`LotteryBreak::ReadsLife`]). A formation whose performers all have a kind
//!   holds the range, so its plan holds the folded actions of its performers, and no action reads a phase life.
//! - Its recording alone records the skeleton of the live without performers but for the gauge speeds of its hits:
//!   its factor skills file no lot factor and draw no random value, and the recording does not fail
//!   ([`LotteryBreak::Interacts`]).
//!
//! The checks split further between the member's skills and the Snap's skills, which [`LotteryKinds`] reads apart
//! (see [`LotteryKinds::breaks`]).

use super::*;

/// The positions of a formation.
pub(super) const POSITIONS: usize = 5;

/// Why the formations of a performer take no lottery kind ([`LotteryRole::Breaks`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LotteryBreak {
    /// A lottery recording refuses the performer, for the reason given.
    Unsupported(String),
    /// A judgement conversion the performer holds can change a judgement of the play in a formation, beyond turning
    /// a Perfect and a Just into one another.
    Converts,
    /// A skill of the performer can take the life out of the range the play keeps it in.
    LeavesLifeRange,
    /// A lottery action of the performer reads the life beyond what that range answers.
    ReadsLife,
    /// A lottery condition of the performer reads performers other than its holder.
    ReadsFormation,
    /// The recording of the performer's factor skills does more than add gauge speeds, for the reason given.
    Interacts(String),
}

/// What a performer is to the lottery recordings of a live ([`LotteryKinds::role`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LotteryRole {
    Kind(LotteryKind),
    Breaks(LotteryBreak),
}

/// A lottery kind of one [`LotteryKinds`]: formations whose performers have the same kinds at every position have the
/// same lottery law.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LotteryKind(pub(super) u32);

impl LotteryKind {
    /// The kind's index among the kinds of its [`LotteryKinds`], in the order they were first met.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// How the judgement conversions of a performer with a kind act in a formation: whether they can turn a Perfect and a
/// Just of the play into one another ([`PlayConversions::Relabel`]), and whether they change no judgement only while no
/// other performer's conversions do so ([`quiet_conversions`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Conversions {
    pub(super) relabels: bool,
    pub(super) read_relabels: bool,
}

/// A skill and its level.
type Held = (i64, i64);

/// A member's live and Gekisou skills.
type MemberSkills = (Option<Held>, Option<Held>);

/// A Snap's support and Gekisou support skills, beside a Gekisou skill or not.
type SnapSkills = (Vec<Held>, Vec<Held>, bool);

/// What a performer at one position adds to the lottery recording of a formation: its factor part (the gauge speeds
/// its factor skills file at the hits, [`LotteryKinds::parts`]; 0 for none), the actions of its plan by
/// [`action_words`], and the score probes it holds.
#[derive(Clone, Debug)]
pub(super) struct Contribution {
    pub(super) part: u32,
    pub(super) actions: Vec<Action<ProbabilityMass>>,
    pub(super) probes: Vec<bool>,
}

impl Contribution {
    fn push_words(&self, out: &mut Vec<u64>) {
        out.extend([u64::from(self.part), self.actions.len() as u64, self.probes.len() as u64]);
        self.actions.iter().for_each(|action| out.extend(action_words(action)));
        out.extend(self.probes.iter().map(|&probe| u64::from(probe)));
    }
}

/// What a kind holds: the contribution of its performers at each position, the identity of each contribution among
/// the contributions of the live held (equal contributions share one), and their conversions.
pub(super) struct KindEntry {
    pub(super) contributions: [Contribution; POSITIONS],
    pub(super) ids: [u32; POSITIONS],
    pub(super) conversions: Conversions,
}

/// The gauge speeds a performer's factor skills file at the hits of the live: the speed at each hit, and the factor
/// commands, whose binary32 sums decide whether the speeds of several performers add exactly.
pub(super) struct FactorPart {
    pub(super) speeds: Vec<f32>,
    pub(super) commands: Vec<(i32, f32)>,
}

/// Use of a [`LotteryKinds`].
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LotteryKindStats {
    /// Lives that replaced the held one.
    pub lives: u64,
    /// Roles asked, and the reduced performers whose kind was read.
    pub roles: u64,
    pub performers: u64,
    /// Kinds and factor parts met, and the native recordings of factor skills made for them.
    pub kinds: usize,
    pub parts: usize,
    pub factor_recordings: u64,
    /// Formations whose laws were asked, those with a performer without a kind, those whose gauge speeds may round or
    /// whose gauge actions may wrap, and those where conversions that turn a Perfect and a Just into one another meet
    /// conversions that read such a change ([`Conversions`]).
    pub teams: u64,
    pub unkinded: u64,
    pub uncomposed: u64,
    pub relabel_conflicts: u64,
    /// Laws of orders asked, those found held for the same contributions or the same composition, and the composed
    /// transcripts propagated, with the transitions and the largest state count of their propagations.
    pub law_lookups: u64,
    pub law_hits: u64,
    pub composed: u64,
    /// Composed laws read from the propagation of a composition that differs from theirs only in its score probes.
    pub probe_variants: u64,
    pub composed_transitions: u64,
    pub composed_peak_states: usize,
    pub law_evictions: u64,
    pub peak_laws: usize,
    pub peak_law_bytes: usize,
}

/// The lottery kinds of the performers of one live at a time, and the laws composed from them. A live whose recordings
/// fail without performers composes none: every performer is then refused.
pub struct LotteryKinds<'m> {
    pub(super) master: &'m Master,
    /// The live of the kinds held.
    pub(super) live: Option<HeldLive>,
    /// The skeleton of the live without performers, or why it has none.
    pub(super) base: Result<std::sync::Arc<Skeleton>, String>,
    /// The life range of the play ([`play_life_range`]).
    life: Option<(i32, i32)>,
    /// The checks of the member's skills, by live and Gekisou skill.
    members: FxHashMap<MemberSkills, Option<LotteryBreak>>,
    /// The checks of the Snap's skills, by support and Gekisou support skills, beside a Gekisou skill or not.
    snaps: FxHashMap<SnapSkills, Option<LotteryBreak>>,
    /// Whether a Gekisou support skill holds a lottery-related row.
    related: FxHashMap<Held, bool>,
    /// The conversions of a performer whose skill lists pass every check but those of their conversions, or why its
    /// formations take no kind ([`LotteryKinds::admit`]).
    conversions: FxHashMap<Performer, Result<Conversions, LotteryBreak>>,
    /// The role of each reduced performer ([`LotteryKinds::reduced`]) with its conversions.
    roles: FxHashMap<(Performer, Conversions), LotteryRole>,
    /// The kind of each word list of five contributions and conversions, and what each kind holds.
    kinds: FxHashMap<Vec<u64>, LotteryKind>,
    pub(super) entries: Vec<KindEntry>,
    /// The identity of each contribution, by its words.
    contribution_ids: FxHashMap<Vec<u64>, u32>,
    /// The factor parts; part 0 files no gauge speed.
    pub(super) parts: Vec<FactorPart>,
    /// The part of each speed and command list, and of each factor text ([`factor_text`]).
    part_ids: FxHashMap<Vec<u64>, u32>,
    part_texts: FxHashMap<Vec<u8>, Result<u32, LotteryBreak>>,
    pub(super) laws: ComposedLaws,
    pub(super) stats: LotteryKindStats,
}

impl<'m> LotteryKinds<'m> {
    /// Kinds of performers of lives played with `master`, holding composed laws of at most `capacity_bytes` in total
    /// with their keys; zero holds none.
    pub fn new(master: &'m Master, capacity_bytes: usize) -> Self {
        Self {
            master,
            live: None,
            base: Err("no live".into()),
            life: None,
            members: FxHashMap::default(),
            snaps: FxHashMap::default(),
            related: FxHashMap::default(),
            conversions: FxHashMap::default(),
            roles: FxHashMap::default(),
            kinds: FxHashMap::default(),
            entries: Vec::new(),
            contribution_ids: FxHashMap::default(),
            parts: Vec::new(),
            part_ids: FxHashMap::default(),
            part_texts: FxHashMap::default(),
            laws: ComposedLaws::new(capacity_bytes),
            stats: LotteryKindStats::default(),
        }
    }

    /// The kinds met in the live held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn stats(&self) -> LotteryKindStats {
        LotteryKindStats { kinds: self.entries.len(), parts: self.parts.len(), ..self.stats }
    }

    /// Hold `live`, dropping the kinds of another; false for a live of another master. The skeleton of the live
    /// without performers is recorded once the checks of a recording on the live pass.
    pub(super) fn enter(&mut self, live: &LotteryLive<'_>) -> bool {
        // The held master stays borrowed, so no other master can be at its address.
        if !std::ptr::eq(live.master, self.master) {
            return false;
        }
        if self.live.as_ref().is_some_and(|held| held.is(live)) {
            return true;
        }
        self.stats.lives += 1;
        self.members.clear();
        self.snaps.clear();
        self.related.clear();
        self.conversions.clear();
        self.roles.clear();
        self.kinds.clear();
        self.entries.clear();
        self.contribution_ids.clear();
        self.part_texts.clear();
        self.laws.clear();
        self.life = play_life_range(self.master, live.play);
        self.base = check_recording(self.master, live.skills, live.play, live.delta_times, None, &[])
            .and_then(|()| live.skeleton(&[]))
            .map_err(|error| error.to_string())
            .and_then(|skeleton| match (&skeleton.failure, skeleton.pairs) {
                (Some(failure), _) => Err(failure.to_string()),
                (None, None) => Err("the recording without performers draws a random value".into()),
                (None, Some(_)) => Ok(std::sync::Arc::new(skeleton)),
            });
        let hits = self.base.as_ref().map_or(0, |base| base.hits.len());
        self.parts = vec![FactorPart { speeds: vec![0.0; hits], commands: Vec::new() }];
        self.part_ids = FxHashMap::default();
        self.part_ids.insert(part_words(&self.parts[0]), 0);
        self.live = Some(HeldLive::new(live));
        true
    }

    /// The kind of `performer` in `live`, or why its formations take none.
    pub fn role(&mut self, live: &LotteryLive<'_>, performer: &Performer) -> LotteryRole {
        if !self.enter(live) {
            return LotteryRole::Breaks(LotteryBreak::Unsupported("the live is played with another master".into()));
        }
        self.held_role(live, performer)
    }

    /// [`Self::role`] in the live held, which is `live`.
    pub(super) fn held_role(&mut self, live: &LotteryLive<'_>, performer: &Performer) -> LotteryRole {
        self.stats.roles += 1;
        if let Err(why) = &self.base {
            let why = format!("the live composes no lottery law: {why}");
            return LotteryRole::Breaks(LotteryBreak::Unsupported(why));
        }
        let conversions = match self.admit(live, performer) {
            Ok(conversions) => conversions,
            Err(why) => return LotteryRole::Breaks(why),
        };
        let key = (self.reduced(live, performer), conversions);
        if let Some(role) = self.roles.get(&key) {
            return role.clone();
        }
        self.stats.performers += 1;
        let role = match self.kind_of(live, &key.0, conversions) {
            Ok(kind) => LotteryRole::Kind(kind),
            Err(why) => LotteryRole::Breaks(why),
        };
        self.roles.insert(key, role.clone());
        role
    }

    /// The conversions of `performer` ([`Conversions`]), or why the performer checks of a recording, its conversions
    /// or its effects on the life keep its formations from a kind.
    ///
    /// The checks of the live and Gekisou skills of the member and those of the support and Gekisou support skills
    /// of the Snap are made apart: every check is a disjunction over the skill lists, the Gekisou support list read
    /// only beside a Gekisou skill, and state identities of different lists never meet. The Snap's lists are
    /// checked beside the performer's own Gekisou skill once that skill passes, which then adds nothing to them. A
    /// part whose held conversion can change a judgement of the play leaves the conversions to the complete
    /// performer: their changes ([`play_conversions`]), and the run whose holder conditions they read
    /// ([`quiet_conversions`]).
    pub(super) fn admit(&mut self, live: &LotteryLive<'_>, performer: &Performer) -> Result<Conversions, LotteryBreak> {
        let member = (performer.live_skill, performer.gekisou_skill);
        if !self.members.contains_key(&member) {
            let part = Performer {
                live_skill: performer.live_skill,
                gekisou_skill: performer.gekisou_skill,
                ..Default::default()
            };
            let why = self.performer_breaks(live, &part);
            self.members.insert(member, why);
        }
        let member = self.members[&member].clone();
        if let Some(why) = member.as_ref().filter(|why| **why != LotteryBreak::Converts) {
            return Err(why.clone());
        }
        let snap = (
            performer.support_skills.clone(),
            performer.gekisou_support_skills.clone(),
            performer.gekisou_skill.is_some(),
        );
        if !self.snaps.contains_key(&snap) {
            let part = Performer {
                support_skills: performer.support_skills.clone(),
                gekisou_skill: performer.gekisou_skill,
                gekisou_support_skills: performer.gekisou_support_skills.clone(),
                ..Default::default()
            };
            let why = self.performer_breaks(live, &part);
            self.snaps.insert(snap.clone(), why);
        }
        let snap = self.snaps[&snap].clone();
        if let Some(why) = snap.as_ref().filter(|why| **why != LotteryBreak::Converts) {
            return Err(why.clone());
        }
        if member.is_none() && snap.is_none() {
            return Ok(Conversions::default());
        }
        if let Some(found) = self.conversions.get(performer) {
            return found.clone();
        }
        let LotteryLive { notes, events, params, setup, play, delta_times, ranking, .. } = *live;
        let found = match play_conversions(self.master, std::slice::from_ref(performer), notes, play) {
            PlayConversions::None => Ok(Conversions::default()),
            PlayConversions::Relabel => Ok(Conversions { relabels: true, read_relabels: false }),
            PlayConversions::Change => {
                quiet_conversions(self.master, performer, notes, events, params, setup, play, delta_times, ranking)
                    .map(|blind| Conversions { relabels: false, read_relabels: !blind })
                    .ok_or(LotteryBreak::Converts)
            }
        };
        self.conversions.insert(performer.clone(), found.clone());
        found
    }

    /// The performer checks of a recording, the life range and the conversions, for `performer` alone; a conversion
    /// that can change a judgement of the play by the targets of its effects ([`luck_play_converts`]) is reported
    /// last.
    fn performer_breaks(&self, live: &LotteryLive<'_>, performer: &Performer) -> Option<LotteryBreak> {
        let unsupported = |error: Error| Some(LotteryBreak::Unsupported(error.to_string()));
        match luck::luck_signature(self.master, performer) {
            Err(error) => return unsupported(error),
            Ok(None) => {
                return unsupported(Error::Unsupported(
                    "LUCK DP: an ordinary lottery effect or cumulative lottery condition".into(),
                ));
            }
            Ok(Some(_)) => {}
        }
        if let Err(error) = check_held_state_identities(self.master, std::slice::from_ref(performer)) {
            return unsupported(error);
        }
        if self.effect_ids_wrap(performer) {
            return unsupported(Error::Unsupported("a condition effect id wraps".into()));
        }
        if self.life.is_some_and(|range| leaves_life_range(self.master, performer, range)) {
            return Some(LotteryBreak::LeavesLifeRange);
        }
        luck_play_converts(self.master, std::slice::from_ref(performer), live.notes, live.play)
            .then_some(LotteryBreak::Converts)
    }

    /// Whether the effect id of a condition skill row of `performer` (row id × 100 + skill type × 10 + position)
    /// wraps at some position.
    fn effect_ids_wrap(&self, performer: &Performer) -> bool {
        let wraps = |id: i64| id.checked_mul(100).and_then(|id| id.checked_add(99)).is_none();
        let master = self.master;
        performer.support_skills.iter().any(|&(id, level)| {
            master.support_skill_effects.iter().any(|r| r.support_skill_id == id && r.level == level && wraps(r.id))
        }) || performer.gekisou_skill.is_some_and(|(id, level)| {
            master.gekisou_skill_effects.iter().any(|r| r.skill_id == id && r.level == level && wraps(r.id))
        }) || performer.gekisou_support_skills.iter().any(|&(id, level)| {
            master.gekisou_support_skill_effects.iter().any(|r| r.skill_id == id && r.level == level && wraps(r.id))
        })
    }

    /// The performer as its lottery condition skills read it: without its live and support skills, and with only
    /// the Gekisou support skills that hold a lottery-related row, beside a Gekisou skill. A dropped skill builds a
    /// condition skill without an effect, which [`lottery_conditions`] leaves out, and appends no row.
    fn reduced(&mut self, live: &LotteryLive<'_>, performer: &Performer) -> Performer {
        let mut reduced = Performer {
            live_skill: None,
            support_skills: Vec::new(),
            gekisou_support_skills: Vec::new(),
            ..performer.clone()
        };
        if performer.gekisou_skill.is_some() {
            let (master, skills) = (self.master, live.skills);
            for &(id, level) in &performer.gekisou_support_skills {
                let related = *self.related.entry((id, level)).or_insert_with(|| {
                    master
                        .gekisou_support_skill_effects
                        .iter()
                        .any(|r| r.skill_id == id && r.level == level && skills.related(LuckSource::GekisouSupport, r))
                });
                if related {
                    reduced.gekisou_support_skills.push((id, level));
                }
            }
        }
        reduced
    }

    /// The kind of a reduced performer with `conversions`: its contribution at each position, with blank performers
    /// before it, and the conversions.
    fn kind_of(
        &mut self,
        live: &LotteryLive<'_>,
        reduced: &Performer,
        conversions: Conversions,
    ) -> Result<LotteryKind, LotteryBreak> {
        let mut contributions = Vec::with_capacity(POSITIONS);
        for position in 0..POSITIONS {
            let mut deck = vec![Performer::default(); position];
            deck.push(reduced.clone());
            contributions.push(self.contribution(live, &deck)?);
        }
        let mut words = Vec::new();
        let mut ids = [0u32; POSITIONS];
        for (id, contribution) in ids.iter_mut().zip(&contributions) {
            let from = words.len();
            contribution.push_words(&mut words);
            let next = self.contribution_ids.len() as u32;
            *id = *self.contribution_ids.entry(words[from..].to_vec()).or_insert(next);
        }
        words.extend([u64::from(conversions.relabels), u64::from(conversions.read_relabels)]);
        let next = LotteryKind(self.entries.len() as u32);
        let kind = *self.kinds.entry(words).or_insert(next);
        if kind == next {
            let contributions = contributions.try_into().expect("one contribution per position");
            self.entries.push(KindEntry { contributions, ids, conversions });
        }
        Ok(kind)
    }

    /// The contribution of the last performer of `deck`, after blank performers.
    fn contribution(&mut self, live: &LotteryLive<'_>, deck: &[Performer]) -> Result<Contribution, LotteryBreak> {
        let unsupported = |error: Error| LotteryBreak::Unsupported(error.to_string());
        let (mut cond, rows, formation) =
            lottery_conditions_read(self.master, deck, live.params.skill_target_music_type, live.skills)
                .map_err(unsupported)?;
        if formation {
            return Err(LotteryBreak::ReadsFormation);
        }
        fold_life_conditions(&mut cond, &rows, || self.life);
        let plan =
            compile::<ProbabilityMass>(self.master, &cond, &rows, live.skills, None, false).map_err(unsupported)?;
        if plan.actions.iter().any(|(_, _, checker)| checker.as_ref().is_some_and(reads_life)) {
            return Err(LotteryBreak::ReadsLife);
        }
        let mut actions: Vec<_> = plan.actions.iter().map(|&(_, action, _)| action).collect();
        actions.sort_by_key(action_words);
        let part = match factor_text(cond, rows) {
            None => 0,
            Some(text) => match self.part_texts.get(&text) {
                Some(found) => found.clone()?,
                None => {
                    let found = self.factor_part(live, deck);
                    self.part_texts.insert(text, found.clone());
                    found?
                }
            },
        };
        Ok(Contribution { part, actions, probes: plan.probes })
    }

    /// The factor part of the reduced performers `deck`, from the native recording of their factor skills.
    fn factor_part(&mut self, live: &LotteryLive<'_>, deck: &[Performer]) -> Result<u32, LotteryBreak> {
        self.stats.factor_recordings += 1;
        let interacts = |why: &str| Err(LotteryBreak::Interacts(why.into()));
        let skeleton = live.skeleton(deck).map_err(|error| LotteryBreak::Unsupported(error.to_string()))?;
        let base = self.base.as_ref().expect("a live with a skeleton");
        if skeleton.failure.is_some() {
            return interacts("its recording fails");
        }
        if skeleton.pairs.is_none() {
            return interacts("its recording draws a random value");
        }
        if !skeleton.factors[0].is_empty() {
            return interacts("it files a lot factor");
        }
        if !same_but_speeds(base, &skeleton) {
            return interacts("its factor skills change the recording beyond the gauge speeds");
        }
        let part = FactorPart {
            speeds: skeleton.hits.iter().map(|hit| hit.speed).collect(),
            commands: skeleton.factors[1].clone(),
        };
        let next = self.parts.len() as u32;
        let id = *self.part_ids.entry(part_words(&part)).or_insert(next);
        if id == next {
            self.parts.push(part);
        }
        Ok(id)
    }
}

/// The words of a factor part: its speeds, and its command changes in a canonical order. Equal words mean equal
/// speeds and equal multisets of changes, which is all [`LotteryKinds::order_laws`] reads of a part.
fn part_words(part: &FactorPart) -> Vec<u64> {
    let mut changes: Vec<u32> = part.commands.iter().map(|&(_, change)| change.to_bits()).collect();
    changes.sort_unstable();
    let mut words = Vec::with_capacity(1 + part.speeds.len() + changes.len());
    words.push(part.speeds.len() as u64);
    words.extend(part.speeds.iter().map(|speed| u64::from(speed.to_bits())));
    words.extend(changes.into_iter().map(u64::from));
    words
}

/// Whether two skeletons agree in everything but the gauge speeds of their hits.
fn same_but_speeds(a: &Skeleton, b: &Skeleton) -> bool {
    let words = |skeleton: &Skeleton| {
        let mut out = Vec::new();
        skeleton.templates.iter().for_each(|template| template.push_words(&mut out));
        skeleton.machine.push_words(&mut out);
        out
    };
    let hit = |hit: &Hit| (hit.range, hit.buff, hit.consumes);
    words(a) == words(b)
        && a.luck == b.luck
        && a.frames == b.frames
        && a.notes == b.notes
        && a.pending == b.pending
        && a.hits.len() == b.hits.len()
        && a.hits.iter().zip(&b.hits).all(|(x, y)| hit(x) == hit(y))
}
