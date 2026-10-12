//! Lottery kinds: what a performer brings to the lottery recordings of a live, apart from where it plays.
//!
//! The order classes of a live ([`LuckOrderClasses`]) key a formation by the text of the lottery condition skills
//! of its reduced performers ([`cache::class_text`]). A lottery kind reads that text one performer at a time:
//! the text of the performer's own lottery condition skills at each of the five positions, with blank performers
//! before it. A formation whose performers all have a kind takes a key, and two such formations with the same kind
//! at every position take the same key, so they share their order class and their law:
//!
//! - The build appends the condition skills of the Gekisou skills by mission list then position, then the Gekisou
//!   support skills by position, numbering the effect rows as it appends them. The condition skills of a performer
//!   are built from its own skill rows, its position, and the checkers of its conditions. A kind refuses a performer
//!   with a checker that reads the formation beyond its holder ([`LotteryBreak::ReadsFormation`]), so its condition
//!   skills depend on the performer and its position alone, up to the numbers of the rows appended before it. Blank
//!   performers hold no Gekisou skill and append nothing. So equal kinds at every position give the same mission
//!   lists, the same row counts and the same condition skills, appended in the same order with the same numbers:
//!   the same text.
//! - A formation takes a key when the performer checks of its recording pass and no judgement conversion it holds
//!   changes a judgement of the play ([`LuckOrderClasses::key`]). Each of these checks fails for a formation when it
//!   fails for one of its performers: they are disjunctions over the performers' skill lists, and the state
//!   identities of different performers never meet (an effect id holds the skill type and the position below the row
//!   id, and a kind refuses row ids whose effect ids wrap). A kind refuses a performer that fails them alone.
//! - The key folds the life comparisons of a formation over its life range ([`life_range`]): the range of the play,
//!   for a formation without a performer whose skills can take the life out of it. The kinds of a play with a life
//!   range fold their texts over it and refuse such a performer ([`LotteryBreak::LeavesLifeRange`]), so a formation
//!   whose performers all have a kind holds the range. A fold replaces comparisons inside one condition, so the folded
//!   text of the formation is made of the folded texts of its performers as the text is made of theirs.
//! - A recording that still reads the life of the formation keys no class ([`LotteryBreak::ReadsLife`]). The plan of
//!   the lottery condition skills makes its actions, and refuses a shape, effect by effect, so a kind refuses a
//!   performer whose own plan does either.
//!
//! The checks split further between the member's skills and the Snap's skills, which [`LotteryKinds`] reads apart
//! (see [`LotteryKinds::breaks`]).

use super::*;

/// The positions of a formation.
const POSITIONS: usize = 5;

/// Why the formations of a performer take no lottery kind ([`LotteryRole::Breaks`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LotteryBreak {
    /// A lottery recording refuses the performer, for the reason given.
    Unsupported(String),
    /// A judgement conversion the performer holds can change a judgement of the play.
    Converts,
    /// A skill of the performer can take the life out of the range the play keeps it in.
    LeavesLifeRange,
    /// A lottery action of the performer reads the life beyond what that range answers.
    ReadsLife,
    /// A lottery condition of the performer reads performers other than its holder.
    ReadsFormation,
}

/// What a performer is to the lottery recordings of a live ([`LotteryKinds::role`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LotteryRole {
    Kind(LotteryKind),
    Breaks(LotteryBreak),
}

/// A lottery kind of one [`LotteryKinds`]: formations whose performers have the same kinds at every position share
/// their order class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LotteryKind(u32);

impl LotteryKind {
    /// The kind's index among the kinds of its [`LotteryKinds`], in the order they were first met.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A skill and its level.
type Held = (i64, i64);

/// A member's live and Gekisou skills.
type MemberSkills = (Option<Held>, Option<Held>);

/// A Snap's support and Gekisou support skills, beside a Gekisou skill or not.
type SnapSkills = (Vec<Held>, Vec<Held>, bool);

/// The lottery kinds of the performers of one live: the master, the lottery skills, the chart, the play and the
/// skill target music type.
pub struct LotteryKinds<'a> {
    master: &'a Master,
    skills: &'a LuckSkills,
    notes: &'a [LiveNote],
    play: &'a LivePlay,
    skill_target_music_type: i64,
    /// The life range of the play ([`play_life_range`]).
    life: Option<(i32, i32)>,
    /// The checks of the member's skills, by live and Gekisou skill.
    members: FxHashMap<MemberSkills, Option<LotteryBreak>>,
    /// The checks of the Snap's skills, by support and Gekisou support skills, beside a Gekisou skill or not.
    snaps: FxHashMap<SnapSkills, Option<LotteryBreak>>,
    /// Whether a Gekisou support skill holds a lottery-related row.
    related: FxHashMap<Held, bool>,
    /// The role of each reduced performer ([`LotteryKinds::reduced`]).
    roles: FxHashMap<Performer, LotteryRole>,
    /// The kind of each text of the five positions.
    kinds: FxHashMap<Vec<String>, LotteryKind>,
}

impl<'a> LotteryKinds<'a> {
    pub fn new(
        master: &'a Master,
        skills: &'a LuckSkills,
        notes: &'a [LiveNote],
        play: &'a LivePlay,
        skill_target_music_type: i64,
    ) -> Self {
        Self {
            master,
            skills,
            notes,
            play,
            skill_target_music_type,
            life: play_life_range(master, play),
            members: FxHashMap::default(),
            snaps: FxHashMap::default(),
            related: FxHashMap::default(),
            roles: FxHashMap::default(),
            kinds: FxHashMap::default(),
        }
    }

    /// The kinds met so far.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// The kind of `performer`, or why its formations take none.
    pub fn role(&mut self, performer: &Performer) -> LotteryRole {
        if let Some(why) = self.breaks(performer) {
            return LotteryRole::Breaks(why);
        }
        let reduced = self.reduced(performer);
        if let Some(role) = self.roles.get(&reduced) {
            return role.clone();
        }
        let role = self.kind_of(&reduced);
        self.roles.insert(reduced, role.clone());
        role
    }

    /// Why the performer checks of a recording, the conversions of `performer` or its effects on the life keep its
    /// formations from a key folded as the kinds are.
    ///
    /// The checks of the live and Gekisou skills of the member and those of the support and Gekisou support skills
    /// of the Snap are made apart: every check is a disjunction over the skill lists, the Gekisou support list read
    /// only beside a Gekisou skill, and state identities of different lists never meet. The Snap's lists are
    /// checked beside the performer's own Gekisou skill once that skill passes, which then adds nothing to them.
    pub(super) fn breaks(&mut self, performer: &Performer) -> Option<LotteryBreak> {
        let member = (performer.live_skill, performer.gekisou_skill);
        if !self.members.contains_key(&member) {
            let part = Performer {
                live_skill: performer.live_skill,
                gekisou_skill: performer.gekisou_skill,
                ..Default::default()
            };
            let why = self.performer_breaks(&part);
            self.members.insert(member, why);
        }
        if let Some(why) = &self.members[&member] {
            return Some(why.clone());
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
            let why = self.performer_breaks(&part);
            self.snaps.insert(snap.clone(), why);
        }
        self.snaps[&snap].clone()
    }

    /// The performer checks of a recording, the conversions and the life range, for `performer` alone.
    fn performer_breaks(&self, performer: &Performer) -> Option<LotteryBreak> {
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
        if luck_play_converts(self.master, std::slice::from_ref(performer), self.notes, self.play) {
            return Some(LotteryBreak::Converts);
        }
        self.life
            .is_some_and(|range| leaves_life_range(self.master, performer, range))
            .then_some(LotteryBreak::LeavesLifeRange)
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
    fn reduced(&mut self, performer: &Performer) -> Performer {
        let mut reduced = Performer {
            live_skill: None,
            support_skills: Vec::new(),
            gekisou_support_skills: Vec::new(),
            ..performer.clone()
        };
        if performer.gekisou_skill.is_some() {
            let (master, skills) = (self.master, self.skills);
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

    /// The kind of a reduced performer: the text of its lottery condition skills at each position, with blank
    /// performers before it, folded over the life range of the play.
    fn kind_of(&mut self, reduced: &Performer) -> LotteryRole {
        let mut texts = Vec::with_capacity(POSITIONS);
        for position in 0..POSITIONS {
            let mut deck = vec![Performer::default(); position];
            deck.push(reduced.clone());
            let unsupported = |error: Error| LotteryRole::Breaks(LotteryBreak::Unsupported(error.to_string()));
            let (mut cond, rows, formation) =
                match lottery_conditions_read(self.master, &deck, self.skill_target_music_type, self.skills) {
                    Ok(built) => built,
                    Err(error) => return unsupported(error),
                };
            if formation {
                return LotteryRole::Breaks(LotteryBreak::ReadsFormation);
            }
            fold_life_conditions(&mut cond, &rows, || self.life);
            let plan = match compile::<ProbabilityMass>(self.master, &cond, &rows, self.skills, None, false) {
                Ok(plan) => plan,
                Err(error) => return unsupported(error),
            };
            if plan.actions.iter().any(|(_, _, checker)| checker.as_ref().is_some_and(reads_life)) {
                return LotteryRole::Breaks(LotteryBreak::ReadsLife);
            }
            texts.push(format!("{rows:?}/{cond:?}"));
        }
        let next = LotteryKind(self.kinds.len() as u32);
        LotteryRole::Kind(*self.kinds.entry(texts).or_insert(next))
    }
}
