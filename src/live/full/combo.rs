//! The score's combo counter: judgements kept sorted by chart time (then by arrival), with the running combo state.

use crate::error::Error;

const WAIT: i32 = 0;
const PASS: i32 = 7;

#[derive(Clone, Copy, Debug)]
struct Entry {
    time_ms: i32,
    judgement: i32,
    order: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct State {
    combo: i32,
    max_combo: i32,
    all_perfect: bool,
    full_combo: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ComboCounter {
    capacity: i32,
    entries: Vec<Entry>,
    states: Vec<State>,
    add_order: i32,
}

fn out_of_range() -> Error {
    Error::Game("combo counter index out of range".into())
}

impl ComboCounter {
    pub(crate) fn new(total_note_count: usize) -> ComboCounter {
        ComboCounter {
            capacity: total_note_count as i32,
            entries: Vec::with_capacity(total_note_count),
            states: Vec::with_capacity(total_note_count),
            add_order: 0,
        }
    }

    fn precedes(x: &Entry, y: &Entry) -> bool {
        (x.time_ms, x.order) < (y.time_ms, y.order)
    }

    pub(crate) fn add_judgement(&mut self, time_ms: i32, judgement: i32) -> Result<(), Error> {
        if judgement == WAIT || judgement == PASS {
            return Ok(());
        }
        let n = self.entries.len();
        if n as i64 >= self.capacity as i64 {
            self.capacity = self.capacity.wrapping_shl(1);
            if self.capacity < 0 {
                return Err(Error::Game("combo counter capacity overflow".into()));
            }
        }
        let order = self.add_order;
        self.add_order = order.wrapping_add(1);
        let new = Entry { time_ms, judgement, order };
        let mut i = n;
        while i >= 1 {
            if (i - 1) as i64 >= self.capacity as i64 {
                return Err(out_of_range());
            }
            if !Self::precedes(&new, &self.entries[i - 1]) {
                break;
            }
            i -= 1;
        }
        if i as i64 >= self.capacity as i64 {
            return Err(out_of_range());
        }
        self.entries.insert(i, new);
        self.states.insert(i, State::default());
        self.recompute_from(i);
        Ok(())
    }

    fn recompute_from(&mut self, start: usize) {
        let mut s = if start == 0 {
            State { combo: 0, max_combo: 0, all_perfect: true, full_combo: true }
        } else {
            self.states[start - 1]
        };
        for k in start..self.entries.len() {
            let j = self.entries[k].judgement;
            let keep = (j.wrapping_sub(3) as u32) < 0xFFFF_FFFE;
            s.combo = if keep { s.combo.wrapping_add(1) } else { 0 };
            if s.combo > s.max_combo {
                s.max_combo = s.combo;
            }
            s.full_combo = s.full_combo && keep;
            s.all_perfect = s.all_perfect && (j.wrapping_sub(5) as u32) < 2;
            self.states[k] = s;
        }
    }

    /// Index of the last entry with a time before `t`, or -1.
    fn find_last_index_before(&self, t: i32) -> Result<isize, Error> {
        let n = self.entries.len();
        if n == 0 {
            return Ok(-1);
        }
        if self.capacity == 0 {
            return Err(out_of_range());
        }
        if self.entries[0].time_ms >= t {
            return Ok(-1);
        }
        let mut hi = n - 1;
        if hi < 1 {
            return Ok(0);
        }
        let mut lo = 0usize;
        loop {
            let mid = lo + (hi - lo).div_ceil(2);
            if self.entries[mid].time_ms < t {
                lo = mid;
            } else {
                hi = mid - 1;
            }
            if lo >= hi {
                return Ok(lo as isize);
            }
        }
    }

    /// The combo the score reads for a note at `t`: the state of the last judgement before `t`.
    pub(crate) fn timing_combo(&self, t: i32) -> Result<i32, Error> {
        if self.entries.is_empty() {
            return Ok(0);
        }
        let i = self.find_last_index_before(t)?;
        Ok(if i >= 0 { self.states[i as usize].combo } else { 0 })
    }
}
