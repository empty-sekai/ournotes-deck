//! The transcript of a recording: everything the lottery-state propagation reads.

use super::*;

/// Everything the lottery-state propagation reads: the native frames of a recording ([`Skeleton`]) with the actions
/// of its plan ([`file`]). [`propagate`] is a function of this value alone: equal transcripts give the same curve,
/// bit for bit.
pub(super) struct Transcript<M> {
    pub(super) collect_moments: bool,
    pub(super) templates: Vec<LuckScore>,
    pub(super) machine: LotteryMachine,
    /// One flag per range: whether it is a Luck range.
    pub(super) luck: Vec<bool>,
    pub(super) probes: Vec<bool>,
    /// Whether the plan has a Miss gauge row. Their used flags are committed after every gated frame.
    pub(super) miss_rows: bool,
    pub(super) frames: Vec<Frame>,
    pub(super) notes: Vec<Judged>,
    pub(super) hits: Vec<Hit>,
    pub(super) actions: Vec<Action<M>>,
    /// `(range, lot buff at the frame time)` of every playing Luck range, in range order.
    pub(super) pending: Vec<(usize, i32)>,
    /// A recording failure after the last recorded frame. It is reported once the recorded frames propagate, in
    /// the order the interleaved computation meets the two kinds of failure.
    pub(super) failure: Option<Error>,
}

/// One frame, or `repeat` consecutive quiet frames (no note, no Luck range transition) with equal records.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub(super) time_ms: i32,
    pub(super) repeat: u32,
    /// The Luck range that starts in this frame.
    pub(super) start: Option<usize>,
    pub(super) complete: bool,
    pub(super) finish: bool,
    pub(super) gate: bool,
    /// Whether the current playing range is a Luck range.
    pub(super) current_luck: bool,
    pub(super) target: i32,
    /// Exclusive ends of the frame's entries in the note, action and pending lists.
    pub(super) notes: usize,
    pub(super) actions: usize,
    pub(super) pending: usize,
}

impl Frame {
    pub(super) fn quiet(&self, notes_from: usize) -> bool {
        self.notes == notes_from && self.start.is_none() && !self.complete && !self.finish
    }
}

/// A judged note in chart-time order, with its judgement after any deterministic conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Judged {
    pub(super) time_ms: i32,
    pub(super) note_type: i32,
    pub(super) judgement: i32,
    /// Exclusive end of the note's entries in the hit list.
    pub(super) hits: usize,
}

/// A Luck range containing a note, with the lot buff and gauge speed filed at the note time.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hit {
    pub(super) range: usize,
    pub(super) buff: i32,
    pub(super) speed: f32,
    pub(super) consumes: bool,
}

fn push_i32(out: &mut Vec<u64>, value: i32) {
    out.push(u64::from(value as u32));
}

pub(super) fn action_words<M: Mass>(action: &Action<M>) -> [u64; 4] {
    match *action {
        Action::StartGauge { value, chance } => {
            let [lower, upper] = chance.bits();
            [0, value as u64, lower, upper]
        }
        Action::StartMinimum { result, chance } => {
            let [lower, upper] = chance.bits();
            [1, u64::from(result as u8), lower, upper]
        }
        Action::MissGauge { value } => [2, value as u64, 0, 0],
        Action::CriticalPoints { value, chance } => {
            let [lower, upper] = chance.bits();
            [3, u64::from(value as u32), lower, upper]
        }
        Action::StartPoints { value, chance } => {
            let [lower, upper] = chance.bits();
            [4, u64::from(value as u32), lower, upper]
        }
    }
}

fn same_actions<M: Mass>(a: &[Action<M>], b: &[Action<M>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| action_words(a) == action_words(b))
}

impl<M: Mass> Transcript<M> {
    /// Append a recorded frame (`repeat` frames with equal records), merged into the previous frame when both are
    /// quiet with equal records. The merged frame's later times are never read: a quiet frame files no weights.
    pub(super) fn push_frame(&mut self, frame: Frame) {
        let n = self.frames.len();
        let notes_from = self.frames.last().map_or(0, |f| f.notes);
        if n > 0 && frame.quiet(notes_from) {
            let from = |i: usize| {
                if i == 0 {
                    (0, 0, 0)
                } else {
                    (self.frames[i - 1].notes, self.frames[i - 1].actions, self.frames[i - 1].pending)
                }
            };
            let previous = self.frames[n - 1];
            let (previous_notes, previous_actions, previous_pending) = from(n - 1);
            if previous.quiet(previous_notes)
                && previous.repeat.checked_add(frame.repeat).is_some()
                && (previous.gate, previous.current_luck, previous.target)
                    == (frame.gate, frame.current_luck, frame.target)
                && same_actions(
                    &self.actions[previous_actions..previous.actions],
                    &self.actions[previous.actions..frame.actions],
                )
                && self.pending[previous_pending..previous.pending] == self.pending[previous.pending..frame.pending]
            {
                self.actions.truncate(previous.actions);
                self.pending.truncate(previous.pending);
                self.frames[n - 1].repeat += frame.repeat;
                return;
            }
        }
        self.frames.push(frame);
    }

    /// Every field as words (binary32 and binary64 values by bit pattern, every list after its length), or None
    /// for a failed recording. Equal words mean equal transcripts.
    pub(super) fn key(&self) -> Option<Vec<u64>> {
        let Self {
            collect_moments,
            templates,
            machine,
            luck,
            probes,
            miss_rows,
            frames,
            notes,
            hits,
            actions,
            pending,
            failure,
        } = self;
        if failure.is_some() {
            return None;
        }
        let mut out = Vec::with_capacity(64 + 4 * frames.len() + 4 * notes.len() + 4 * hits.len() + 4 * actions.len());
        if *collect_moments {
            out.push(0x6d6f6d656e747331);
        }
        out.push(templates.len() as u64);
        templates.iter().for_each(|template| template.push_words(&mut out));
        machine.push_words(&mut out);
        out.push(luck.len() as u64);
        out.extend(luck.iter().map(|&flag| u64::from(flag)));
        out.push(probes.len() as u64);
        out.extend(probes.iter().map(|&flag| u64::from(flag)));
        out.push(u64::from(*miss_rows));
        out.push(frames.len() as u64);
        for frame in frames {
            let Frame { time_ms, repeat, start, complete, finish, gate, current_luck, target, notes, actions, pending } =
                *frame;
            push_i32(&mut out, time_ms);
            out.push(u64::from(repeat));
            out.push(start.map_or(0, |range| range as u64 + 1));
            out.push(
                u64::from(complete) | u64::from(finish) << 1 | u64::from(gate) << 2 | u64::from(current_luck) << 3,
            );
            push_i32(&mut out, target);
            out.extend([notes as u64, actions as u64, pending as u64]);
        }
        out.push(notes.len() as u64);
        for &Judged { time_ms, note_type, judgement, hits } in notes {
            push_i32(&mut out, time_ms);
            push_i32(&mut out, note_type);
            push_i32(&mut out, judgement);
            out.push(hits as u64);
        }
        out.push(hits.len() as u64);
        for &Hit { range, buff, speed, consumes } in hits {
            out.push(range as u64);
            push_i32(&mut out, buff);
            out.push(u64::from(speed.to_bits()));
            out.push(u64::from(consumes));
        }
        out.push(actions.len() as u64);
        actions.iter().for_each(|action| out.extend(action_words(action)));
        out.push(pending.len() as u64);
        for &(range, buff) in pending {
            out.push(range as u64);
            push_i32(&mut out, buff);
        }
        Some(out)
    }
}
