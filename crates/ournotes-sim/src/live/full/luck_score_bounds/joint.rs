//! The expectation of one order's native score with each note's score read jointly with its lottery class.
//!
//! [`luck_exact_score`] encloses a note's score in each lottery class by its scores over every native factor history of
//! that class. Lottery paths with different histories of Rush and probe filings make the score queries execute
//! different frames again, and a frame whose recorded sum is not the exact inverse of its additions leaves the factor
//! states of those histories a few units in the last place apart: a note whose score changes within those units takes
//! two integers in one class. Here the certified lottery law is propagated jointly with the factor histories
//! ([`LotteryRecording::joint`]), so each history carries the mass of the lottery paths that make it.
//!
//! Each joint mass belongs to a lottery state, a group and a member of the group. A group holds lottery paths with the
//! same schedule ahead ([`Track`](super::exact_paths::Track)) and the same classes read by the notes not yet final; a member holds one factor
//! history of the group: its factor state and the value of each such note at its last execution so far. A lottery
//! step keeps every path's group and member. The schedule's events change them where they read the lottery: a range
//! FINISH, a probe switch and a play frame's Rush filings change a path's track by the flags of its lottery state, a
//! note's read point sets the class the note reads, and a score query executes each group's members along its track.
//! A note whose last execution no later query can repeat and whose class is read is final: its expectation is the sum
//! over groups and members of their mass times the note's score in its class there. A rank bonus's range score is the
//! difference of two snapshots, each the sum of the final notes so far and the expectation of the others in the
//! classes the snapshot reads; the bonus keeps the enclosure of its truncation around that expectation.
//!
//! The members of a group are exact histories while they are few. Past twice [`MEMBERS`], the lightest members merge
//! with a near partner into the hull of their states, which rounding keeps the least and the greatest state of the
//! merged histories, as for the paths of [`ExactReplay`].

use super::exact::{ClassRead, Observers, OrderSchedule, Schedule, class_read, order_schedule, value_classes};
use super::exact_paths::{ExactReplay, NoteValue, Observed, TrackScratch, rush_mask, undo_floors};
use super::*;
use crate::live::full::luck_dp::{JointReader, JointView, LotteryFlags, LotteryRecording, Phase, RUSH_FINISH};
use crate::num::FxHashMap;
use members::{Group, Head, Member, Regroup, trim_members};
use ranks::Ranks;

mod members;
mod ranks;

/// The members a group keeps exact; the least massive merge once a group holds more than twice as many.
const MEMBERS: usize = 32;
/// The class of a note before its read point.
const UNREAD: u8 = u8::MAX;

/// [`luck_exact_score`] of `deck` in its order with every note's score read jointly with its lottery class: the final
/// note mean and the range scores are the expectations of the joint law, enclosed by those of [`luck_exact_score`].
#[allow(clippy::too_many_arguments)]
pub fn luck_joint_score(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LuckExactScore, Error> {
    if !super::exact::has_luck(setup) {
        return Err(refuse("the live has no LUCK range"));
    }
    let live = LotteryLive { master, skills, notes, events, params, setup, play, delta_times, ranking };
    let recording =
        LotteryRecording::record(master, skills, notes, events, params, setup, play, delta_times, deck, ranking)?;
    let order = order_schedule(&live, deck, Some(recording.law()), None)?;
    let mut score = order.evaluate(&live, false, Observers::default())?;
    let mut joint = Joint::new(&order, &score.ranges)?;
    recording.joint(&mut joint)?;
    let refined = joint.result()?;
    let old = |bounds: RealBounds| F64Interval::new(bounds.lower, bounds.upper);
    for (range, &(mean, bonus)) in score.ranges.iter_mut().zip(&refined.ranges) {
        (range.mean, range.bonus_mean) = (mean.into(), bonus.into());
    }
    let disagree = || refuse("the joint expectation leaves its enclosure");
    let note = refined.note.intersect(old(score.final_note_mean)?).ok_or_else(disagree)?;
    let rank = refined.rank.intersect(old(score.final_rank_mean)?).ok_or_else(disagree)?;
    let support = I32Interval::new(score.final_support.lower, score.final_support.upper)?.as_real();
    let mean = note
        .add(rank)
        .intersect(old(score.final_mean)?)
        .and_then(|mean| mean.intersect(support))
        .ok_or_else(disagree)?;
    (score.final_note_mean, score.final_rank_mean, score.final_mean) = (note.into(), rank.into(), mean.into());
    Ok(score)
}

fn misread() -> Error {
    refuse("a lottery state contradicts the track of its factor histories")
}

fn possible(mass: &ProbabilityMass) -> bool {
    mass.interval().upper() > 0.0
}

/// The total of disjoint masses.
fn total<'m>(masses: impl IntoIterator<Item = &'m ProbabilityMass>) -> ProbabilityMass {
    masses
        .into_iter()
        .filter(|mass| possible(mass))
        .fold(ProbabilityMass::ZERO, |total, &mass| total.merge_disjoint(mass))
}

/// A filed note that is not final yet: its index in the replay's notes, its score frame and command, and whether its
/// lottery class is read.
struct Pending {
    note: usize,
    frame: i32,
    command: NoteCommand,
    read: bool,
}

/// A note's index and the bits of one of its values.
type ValueKey = (usize, u8, (u32, u32), [u32; FIELDS], [u32; FIELDS]);

/// Note scores by value, read once.
struct Scorer<'s> {
    calc: &'s LiveScoreCalculator,
    rush_percent: i32,
    shared: bool,
    scores: FxHashMap<ValueKey, [(i32, i32); 2]>,
}

impl Scorer<'_> {
    /// The integer bounds of the native score of `note` with `value` in lottery class `class`.
    fn score(&mut self, note: &Pending, value: &NoteValue, class: u8) -> Result<(i32, i32), Error> {
        if class == UNREAD {
            return Err(refuse("a note is scored before its lottery class is read"));
        }
        if !self.shared && value.class != class & 1 {
            return Err(misread());
        }
        let key = (note.note, value.class, value.combo, value.low.map(f32::to_bits), value.high.map(f32::to_bits));
        let pair = match self.scores.get(&key) {
            Some(&pair) => pair,
            None => {
                let buckets = value_classes(self.calc, &note.command, value, self.rush_percent, self.shared)?;
                let probe = usize::from(value.class);
                let missing = || refuse("a note value has no score in its class");
                let pair = [buckets[probe].ok_or_else(missing)?, buckets[probe | 2].ok_or_else(missing)?];
                self.scores.insert(key, pair);
                pair
            }
        };
        Ok(pair[usize::from(class >> 1)])
    }
}

/// The note mean, the rank mean and each range score and bonus of a joint evaluation.
struct Refined {
    note: F64Interval,
    rank: F64Interval,
    ranges: Vec<(F64Interval, F64Interval)>,
}

/// The joint evaluation of one order's schedule: a [`JointReader`] that takes the schedule's events at the read
/// points of the propagation where they read the lottery.
struct Joint<'s> {
    schedule: &'s Schedule,
    law: &'s LuckDpCertifiedResult,
    /// Whether the order has probe rows, whose class each note value keeps.
    rows: bool,
    scorer: Scorer<'s>,
    replay: ExactReplay,
    scratch: TrackScratch,
    observed: Vec<Observed>,
    /// The next event of the schedule; the read point of the propagation: a play frame and whether its lotteries are
    /// drawn, and whether the propagation skipped it; whether the propagation ended.
    cursor: usize,
    at: (usize, bool),
    quiet: bool,
    closed: bool,
    combos: FxHashMap<(usize, usize), (f32, f32)>,
    /// Play frames whose lotteries have completed, whether the next one has started, and its lottery filing places.
    ready: usize,
    started: bool,
    lottery_frames: Vec<i32>,
    /// The next skill boundary's play frame, the play frame of the latest boundary and its time.
    boundary: usize,
    last_probe: Option<usize>,
    skills_at: i32,
    /// The latest play frame time whose lotteries completed.
    probability_ready: i32,
    ranks: Ranks<'s>,
    /// The score queries so far, and the last frame of the latest one.
    queries: usize,
    last_to: Option<i32>,
    groups: Vec<Group>,
    pending: Vec<Pending>,
    /// The expectation of each final note, with its score frame.
    finals: Vec<(i32, F64Interval)>,
}

impl<'s> Joint<'s> {
    /// The joint evaluation of `order`, whose exact evaluation has range scores and bonuses `enclosures`.
    fn new(order: &'s OrderSchedule, enclosures: &'s [LuckExactRange]) -> Result<Self, Error> {
        let (schedule, law) = (&order.schedule, &*order.law);
        if schedule.lanes != 1 {
            return Err(Error::Input("a joint evaluation follows one order".into()));
        }
        let floors = undo_floors(schedule.events.iter().map(|(event, _)| event), &law.rush_transitions)?;
        let replay = ExactReplay::new(schedule.frames, &schedule.configs, floors, 1)?;
        let (track, state) = replay.start_track()?;
        let rows = schedule.configs.iter().any(|(_, rows)| !rows.is_empty());
        Ok(Self {
            schedule,
            law,
            rows,
            scorer: Scorer {
                calc: &order.calc,
                rush_percent: order.rush_percent,
                shared: !rows,
                scores: FxHashMap::default(),
            },
            replay,
            scratch: TrackScratch::default(),
            observed: Vec::new(),
            cursor: 0,
            at: (0, false),
            quiet: false,
            closed: false,
            combos: FxHashMap::default(),
            ready: 0,
            started: false,
            lottery_frames: Vec::new(),
            boundary: 0,
            last_probe: None,
            skills_at: i32::MIN,
            probability_ready: i32::MIN,
            ranks: Ranks::new(schedule, enclosures),
            queries: 0,
            last_to: None,
            groups: vec![Group { head: Head { track, classes: Vec::new() }, members: vec![Member::at(state)] }],
            pending: Vec::new(),
            finals: Vec::new(),
        })
    }

    /// The read point `event` needs the propagation at, if it reads the lottery: the start of the play frame it
    /// belongs to, or the point where that frame's lotteries are drawn.
    fn point(&self, event: &BoundsEvent) -> Option<(usize, bool)> {
        match event {
            BoundsEvent::Potential { start: true, .. } | BoundsEvent::Probe { .. } => Some((self.ready, false)),
            // A query before the next frame starts belongs to the frame whose lotteries completed last.
            BoundsEvent::Query { .. } if self.started || self.ready == 0 => Some((self.ready, false)),
            BoundsEvent::Query { .. } => Some((self.ready - 1, true)),
            BoundsEvent::ProbabilityReady(_) => Some((self.ready, true)),
            _ => None,
        }
    }

    /// Takes the schedule's events up to the read point the propagation is at.
    fn advance(&mut self, view: &mut JointView<'_, '_>) -> Result<(), Error> {
        let schedule = self.schedule;
        while let Some((event, _)) = schedule.events.get(self.cursor) {
            if let Some(point) = self.point(event)
                && (self.closed || point != self.at)
            {
                if !self.closed && point > self.at {
                    return Ok(());
                }
                return Err(refuse("a schedule event reads the lottery away from its read point"));
            }
            self.cursor += 1;
            self.event(view, event)?;
        }
        Ok(())
    }

    fn event(&mut self, view: &mut JointView<'_, '_>, event: &BoundsEvent) -> Result<(), Error> {
        let schedule = self.schedule;
        match *event {
            BoundsEvent::Note { frame, index, note } => {
                let at = self.replay.file_note(frame, note.time_ms, note.note_id, (frame, index))?;
                self.pending.push(Pending { note: at, frame: frame as i32, command: note, read: false });
                for group in &mut self.groups {
                    group.head.classes.push(UNREAD);
                    group.members.iter_mut().for_each(|member| member.values.push(None));
                }
            }
            BoundsEvent::Factor { frame, ref command } => {
                if command.band_total_power != 0 {
                    return Err(refuse("recorder produced an unproved power command"));
                }
                self.replay.file_command(frame, command, 1)?;
            }
            BoundsEvent::Potential { frame, start: true } => {
                self.started = true;
                if rush_mask(&self.law.rush_transitions, self.ready)? & RUSH_FINISH != 0 {
                    self.relabel(view, |flags, head| {
                        if head.track.rush != flags.query_rush {
                            return Err(misread());
                        }
                        let mut head = head.clone();
                        head.track.finish(frame);
                        Ok(head)
                    })?;
                }
            }
            BoundsEvent::Potential { frame, start: false } => self.lottery_frames.push(frame as i32),
            BoundsEvent::Probe { frame, time_ms } => {
                let frame_times = &schedule.frame_times;
                // One boundary per play frame; a frame after music end files its clamped inverse at music end.
                let index = if frame_times.get(self.boundary) == Some(&time_ms) {
                    self.boundary += 1;
                    self.boundary - 1
                } else if time_ms == schedule.music_length_ms
                    && self.boundary > 0
                    && frame_times[self.boundary - 1] > time_ms
                {
                    self.boundary - 1
                } else {
                    return Err(refuse("a probe filing is not at a native skill boundary"));
                };
                if index != self.ready {
                    return Err(refuse("a probe filing is not at its play frame"));
                }
                if self.rows && self.last_probe == Some(index) {
                    // The clamped filing place of a boundary: the joint law follows one place per switch.
                    let switches = view
                        .blocks()
                        .any(|(flags, _, _, masses)| flags.score != flags.score_before && masses.iter().any(possible));
                    if switches {
                        return Err(refuse("a probe switch has two filing places"));
                    }
                } else if self.rows {
                    self.relabel(view, |flags, head| {
                        if head.track.now != u8::from(flags.score_before) {
                            return Err(misread());
                        }
                        let mut head = head.clone();
                        if head.track.now != u8::from(flags.score) {
                            head.track.switch(frame, time_ms);
                        }
                        Ok(head)
                    })?;
                }
                self.last_probe = Some(index);
                self.skills_at = frame_times[index];
            }
            BoundsEvent::Combo { frame, index, ordinary, gekisou } => {
                self.combos.insert((frame, index), (ordinary, gekisou));
            }
            BoundsEvent::ProbabilityReady(time) => {
                if schedule.frame_times.get(self.ready) != Some(&time) {
                    return Err(refuse("a lottery completion is not at its play frame"));
                }
                // A skipped play frame draws no lottery: every state keeps its Rush. Otherwise a lottery that changed
                // the Rush after the last read point filed its command at the frame's time, the latest filing place
                // of the frame, which an earlier filing of the frame precedes.
                if !self.quiet {
                    let place = self.filing_place(view, time)?;
                    self.relabel(view, |flags, head| {
                        if head.track.rush != flags.rush_start {
                            return Err(misread());
                        }
                        let mut head = head.clone();
                        head.track.rush = flags.rush;
                        if let Some(place) = place.filter(|_| flags.rush_filed) {
                            head.track.file(place);
                        }
                        Ok(head)
                    })?;
                }
                self.lottery_frames.clear();
                self.ready += 1;
                self.started = false;
                self.probability_ready = self.probability_ready.max(time);
            }
            BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                self.ranks.rank(range, time_ms, percent, start, end)?
            }
            BoundsEvent::Query { time_ms, to } => self.query(view, time_ms, to)?,
        }
        Ok(())
    }

    /// The score frame of a Rush command a lottery at chart time `time` of the current play frame files, when some
    /// possible state's lotteries changed the Rush in the frame.
    fn filing_place(&self, view: &JointView<'_, '_>, time: i32) -> Result<Option<i32>, Error> {
        if !view.blocks().any(|(flags, _, _, masses)| flags.rush_filed && masses.iter().any(possible)) {
            return Ok(None);
        }
        let frame = get_frame(time).min(self.schedule.frames as i32 - 1);
        if !self.lottery_frames.contains(&frame) {
            return Err(refuse("a lottery Rush command files outside its play frame's filing places"));
        }
        Ok(Some(frame))
    }

    /// Moves each group's lottery paths under the head `change` gives for their lottery flags; their members follow.
    fn relabel(
        &mut self,
        view: &mut JointView<'_, '_>,
        mut change: impl FnMut(LotteryFlags, &Head) -> Result<Head, Error>,
    ) -> Result<(), Error> {
        let mut seen: Vec<(u32, LotteryFlags)> = view
            .blocks()
            .filter(|(_, _, _, masses)| masses.iter().any(possible))
            .map(|(flags, group, _, _)| (group, flags))
            .collect();
        seen.sort_unstable_by_key(|&(group, flags)| (group, flags.word()));
        seen.dedup();
        let mut heads = Vec::with_capacity(seen.len());
        for &(group, flags) in &seen {
            let old = &self.groups.get(group as usize).ok_or_else(misread)?.head;
            heads.push(change(flags, old)?);
        }
        if seen.iter().zip(&heads).all(|(&(group, _), head)| *head == self.groups[group as usize].head) {
            return Ok(());
        }
        // A group whose lottery paths all take one head keeps its masses and members under it.
        if seen.windows(2).zip(heads.windows(2)).all(|(seen, heads)| seen[0].0 != seen[1].0 || heads[0] == heads[1]) {
            for (&(group, _), head) in seen.iter().zip(heads) {
                self.groups[group as usize].head = head;
            }
            return Ok(());
        }
        let old = std::mem::take(&mut self.groups);
        let mut regroup = Regroup::default();
        let mut places = FxHashMap::default();
        for (&(group, flags), head) in seen.iter().zip(heads) {
            let members = &old[group as usize].members;
            places.insert((group, flags.word()), regroup.add(group, head, || members.clone(), false));
        }
        let (groups, widths, maps) = regroup.finish();
        view.remap(&widths, &maps, |flags, group| places.get(&(group, flags.word())).copied().ok_or_else(misread))?;
        self.groups = groups;
        Ok(())
    }

    /// Merges the groups with equal heads and the equal members of each group.
    fn dedupe(&mut self, view: &mut JointView<'_, '_>) -> Result<(), Error> {
        let old = std::mem::take(&mut self.groups);
        let count = (old.len(), old.iter().map(|group| group.members.len()).sum::<usize>());
        let mut regroup = Regroup::default();
        let mut places = Vec::with_capacity(old.len());
        for (index, Group { head, members }) in old.into_iter().enumerate() {
            places.push(regroup.add(index as u32, head, || members, true));
        }
        let (groups, widths, maps) = regroup.finish();
        if (groups.len(), groups.iter().map(|group| group.members.len()).sum::<usize>()) != count {
            view.remap(&widths, &maps, |_, group| places.get(group as usize).copied().ok_or_else(misread))?;
        }
        self.groups = groups;
        Ok(())
    }

    /// The total mass of each member of each group.
    fn member_masses(&self, view: &JointView<'_, '_>) -> Vec<Vec<ProbabilityMass>> {
        let mut totals: Vec<Vec<_>> =
            self.groups.iter().map(|group| vec![ProbabilityMass::ZERO; group.members.len()]).collect();
        for (_, group, width, masses) in view.blocks() {
            let totals = &mut totals[group as usize];
            for row in masses.chunks_exact(width) {
                for (total, mass) in totals.iter_mut().zip(row).filter(|(_, mass)| possible(mass)) {
                    *total = total.merge_disjoint(*mass);
                }
            }
        }
        totals
    }

    /// A score query up to frame `to` at play frame time `time_ms`: every group's members execute along its track.
    fn query(&mut self, view: &mut JointView<'_, '_>, time_ms: i32, to: i32) -> Result<(), Error> {
        if !self.lottery_frames.is_empty() {
            return Err(refuse("a score query precedes the completion of a play frame's lotteries"));
        }
        let plan = self.replay.begin_query(to)?;
        let places_of: FxHashMap<usize, usize> =
            self.pending.iter().enumerate().map(|(at, pending)| (pending.note, at)).collect();
        let old = std::mem::take(&mut self.groups);
        let mut regroup = Regroup::default();
        let mut places = Vec::with_capacity(old.len());
        let (mut lows, mut highs) = (Vec::new(), Vec::new());
        for (index, mut group) in old.into_iter().enumerate() {
            lows.clear();
            highs.clear();
            lows.extend(group.members.iter().map(|member| member.low));
            highs.extend(group.members.iter().map(|member| member.high));
            self.observed.clear();
            let track = self.replay.query_track(
                &plan,
                &group.head.track,
                &mut lows,
                &mut highs,
                &self.combos,
                &mut self.observed,
                &mut self.scratch,
            )?;
            for (member, (low, high)) in group.members.iter_mut().zip(lows.iter().zip(&highs)) {
                (member.low, member.high) = (*low, *high);
            }
            for observed in &self.observed {
                let at = *places_of.get(&observed.note).ok_or_else(|| refuse("a query executes a final note"))?;
                group.members[observed.member as usize].values[at] = Some(observed.value);
            }
            group.head.track = track;
            let Group { head, members } = group;
            places.push(regroup.add(index as u32, head, || members, false));
        }
        let (groups, widths, maps) = regroup.finish();
        view.remap(&widths, &maps, |_, group| places.get(group as usize).copied().ok_or_else(misread))?;
        self.groups = groups;
        self.replay.end_query(&plan);
        self.ranks.file(to);
        let query = self.queries;
        self.queries += 1;
        self.last_to = Some(to);
        self.finalize(view)?;
        if self.ranks.reads(query) {
            let sum = self.snapshot(view, time_ms, to)?;
            self.ranks.snapshot(query, to, sum);
        }
        self.cap(view)
    }

    /// The notes at chart time `time` read the lottery class they keep, in `phase`. Before the frame's time, the
    /// lotteries of those notes are drawn: a state whose lottery changed the Rush in the frame so far filed its first
    /// Rush command at `time` or earlier.
    fn read(&mut self, view: &mut JointView<'_, '_>, time: i32, phase: Phase) -> Result<(), Error> {
        if phase == Phase::BeforeFrame
            && let Some(place) = self.filing_place(view, time)?
        {
            self.relabel(view, |flags, head| {
                let mut head = head.clone();
                if flags.rush_filed {
                    head.track.file(place);
                }
                Ok(head)
            })?;
        }
        let targets: Vec<usize> = (0..self.pending.len())
            .filter(|&at| !self.pending[at].read && self.pending[at].command.time_ms == time)
            .collect();
        if targets.is_empty() {
            return Ok(());
        }
        self.relabel(view, |flags, head| {
            let mut head = head.clone();
            for &at in &targets {
                head.classes[at] = flags.class(phase);
            }
            Ok(head)
        })?;
        for &at in &targets {
            self.pending[at].read = true;
        }
        self.finalize(view)
    }

    /// Counts the notes that are final: read, up to the latest query's last frame, and before every frame a later
    /// query executes; once the propagation ended, every read note up to the latest query's last frame.
    fn finalize(&mut self, view: &mut JointView<'_, '_>) -> Result<(), Error> {
        let to = self.last_to.unwrap_or(i32::MIN);
        let base = if self.closed { i32::MAX } else { self.replay.base() };
        let done: Vec<usize> = (0..self.pending.len())
            .filter(|&at| {
                let pending = &self.pending[at];
                pending.read && pending.frame <= to && pending.frame < base
            })
            .collect();
        if done.is_empty() {
            return Ok(());
        }
        let masses = self.member_masses(view);
        for &at in &done {
            let note = &self.pending[at];
            let mut mean = F64Interval::ZERO;
            for (group, masses) in self.groups.iter().zip(&masses) {
                let class = group.head.classes[at];
                for (member, mass) in group.members.iter().zip(masses).filter(|(_, mass)| possible(mass)) {
                    let value = member.values[at].as_ref().ok_or_else(|| refuse("a final note was not executed"))?;
                    let (lo, hi) = self.scorer.score(note, value, class)?;
                    mean = mean.add(mass.interval().multiply(I32Interval::new(lo, hi)?.as_real()));
                }
            }
            self.finals.push((note.frame, mean));
        }
        for &at in done.iter().rev() {
            self.pending.remove(at);
            for group in &mut self.groups {
                group.head.classes.remove(at);
                group.members.iter_mut().for_each(|member| {
                    member.values.remove(at);
                });
            }
        }
        self.dedupe(view)
    }

    /// The expectation of the final notes up to frame `to`.
    fn finals_to(&self, to: i32) -> F64Interval {
        self.finals.iter().filter(|(frame, _)| *frame <= to).fold(F64Interval::ZERO, |sum, &(_, mean)| sum.add(mean))
    }

    /// The expectation of the note sum a score query at `time_ms` up to frame `to` reads: the final notes, and the
    /// others of its frames in the classes the query reads them in.
    fn snapshot(&mut self, view: &JointView<'_, '_>, time_ms: i32, to: i32) -> Result<F64Interval, Error> {
        let frame_times = &self.schedule.frame_times;
        let mut reads = Vec::new();
        for (at, pending) in self.pending.iter().enumerate().filter(|(_, pending)| pending.frame <= to) {
            let read =
                class_read(frame_times, self.probability_ready, self.skills_at, time_ms, pending.command.time_ms)?;
            reads.push((
                at,
                match read {
                    ClassRead::Kept(_) if !pending.read => {
                        return Err(refuse("a snapshot reads a kept class before its read point"));
                    }
                    ClassRead::Kept(_) => None,
                    ClassRead::Query { phase, .. } => {
                        Some([Phase::BeforeFrame, Phase::AtFrame, Phase::BeforeSkills][phase])
                    }
                },
            ));
        }
        let mut sum = self.finals_to(to);
        for (flags, group, width, masses) in view.blocks() {
            let group = &self.groups[group as usize];
            for (index, member) in group.members.iter().enumerate().take(width) {
                let mass = total(masses.iter().skip(index).step_by(width));
                if !possible(&mass) {
                    continue;
                }
                let (mut lo, mut hi) = (0i64, 0i64);
                for &(at, phase) in &reads {
                    let class = phase.map_or(group.head.classes[at], |phase| flags.class(phase));
                    let value = member.values[at].as_ref().ok_or_else(|| refuse("a snapshot note was not executed"))?;
                    let (a, b) = self.scorer.score(&self.pending[at], value, class)?;
                    (lo, hi) = (lo + i64::from(a), hi + i64::from(b));
                }
                sum = sum.add(mass.interval().multiply(F64Interval::new(lo as f64, hi as f64)?));
            }
        }
        Ok(sum)
    }

    /// Trims each group that holds more than twice [`MEMBERS`] members ([`trim_members`]).
    fn cap(&mut self, view: &mut JointView<'_, '_>) -> Result<(), Error> {
        if self.groups.iter().all(|group| group.members.len() <= 2 * MEMBERS) {
            return Ok(());
        }
        let masses = self.member_masses(view);
        let mut maps = Vec::new();
        let mut places = Vec::with_capacity(self.groups.len());
        for (index, (group, masses)) in self.groups.iter_mut().zip(&masses).enumerate() {
            if group.members.len() <= 2 * MEMBERS {
                places.push((index as u32, u32::MAX));
                continue;
            }
            let (members, map) = trim_members(std::mem::take(&mut group.members), masses, MEMBERS);
            group.members = members;
            maps.push(map);
            places.push((index as u32, maps.len() as u32 - 1));
        }
        let widths: Vec<u32> = self.groups.iter().map(|group| group.members.len() as u32).collect();
        view.remap(&widths, &maps, |_, group| places.get(group as usize).copied().ok_or_else(misread))
    }

    fn result(&self) -> Result<Refined, Error> {
        let prev = self.last_to.ok_or_else(|| refuse("the recorder made no score query"))?;
        // The notes after the terminal query's last frame are not in the final score.
        if !self.closed
            || self.cursor != self.schedule.events.len()
            || self.pending.iter().any(|pending| pending.frame <= prev)
            || self.ready != self.schedule.frame_times.len()
        {
            return Err(refuse("the joint propagation did not take the whole schedule"));
        }
        let (rank, ranges) = self.ranks.finish(prev)?;
        Ok(Refined { note: self.finals_to(prev), rank, ranges })
    }
}

impl JointReader for Joint<'_> {
    fn started(&mut self, view: &mut JointView<'_, '_>, play: usize, quiet: bool) -> Result<(), Error> {
        (self.at, self.quiet) = ((play, false), quiet);
        self.advance(view)
    }

    fn kept(&mut self, view: &mut JointView<'_, '_>, time: i32, phase: Phase) -> Result<(), Error> {
        self.read(view, time, phase)
    }

    fn lotteries(&mut self, view: &mut JointView<'_, '_>, play: usize, quiet: bool) -> Result<(), Error> {
        (self.at, self.quiet) = ((play, true), quiet);
        self.advance(view)
    }

    fn close(&mut self, view: &mut JointView<'_, '_>) -> Result<(), Error> {
        self.closed = true;
        self.advance(view)?;
        self.finalize(view)
    }
}
