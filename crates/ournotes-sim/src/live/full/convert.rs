//! Judgement conversion: the note judgement converter and the target judgement convert applier (effect types 12006
//! and 13005).

use super::StateKey;
use super::engine::{END_FRAME, EXECUTE_FRAME};
use crate::error::Error;
use crate::num::{FxHashMap, FxHashSet};

/// Just judgement.
const JUST: i32 = 6;
/// The convert-to-Just effect type.
const CONVERT_TO_JUST: i64 = 13005;

/// The judgement a convert effect converts to: Just for 13005, else the effect value when it is 1..=6, else -1.
fn resolve_convert_to(effect_type: i64, effect_value: i64) -> i32 {
    if effect_type == CONVERT_TO_JUST {
        return JUST;
    }
    let v = effect_value as i32;
    if (v.wrapping_sub(1) as u32) > 5 { -1 } else { v }
}

#[derive(Clone, Debug)]
struct ConvertParam {
    key: StateKey,
    count: i32,
    max_count: i64,
    targets: Vec<i64>,
    convert_to: i32,
}

/// The effect fields the applier reads.
pub(crate) struct ConvertEffect<'a> {
    pub effect_type: i64,
    pub effect_value: i64,
    pub effect_limit_count: i64,
    pub targets: &'a [i64],
    pub effect_id: i64,
}

/// The converter with its only function kind, the convert applier's `TryConvert`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Conversion {
    /// Registered function ids, head first.
    order: Vec<i32>,
    registered: FxHashSet<i32>,
    func_id: i32,
    id_map: FxHashMap<StateKey, i32>,
    params: FxHashMap<i32, ConvertParam>,
    limit_finished: FxHashMap<StateKey, i32>,
    context: FxHashMap<i64, Vec<i64>>,
    no_just: FxHashSet<i64>,
    /// Number of conversions made.
    pub converted: u64,
}

impl Conversion {
    pub(crate) fn new(no_just: FxHashSet<i64>) -> Conversion {
        Conversion { no_just, ..Default::default() }
    }

    fn register(&mut self) -> i32 {
        self.func_id = self.func_id.wrapping_add(1);
        let k = self.func_id;
        self.registered.insert(k);
        self.order.insert(0, k);
        k
    }

    fn unregister(&mut self, id: i32) -> Result<(), Error> {
        if id <= 0 || !self.registered.remove(&id) {
            return Err(Error::Game(format!("judgement convert function {id} is not registered")));
        }
        if let Some(p) = self.order.iter().position(|&x| x == id) {
            self.order.remove(p);
        }
        Ok(())
    }

    /// Converts a raw judgement: the registered functions from the head; the first that changes the judgement wins.
    pub(crate) fn convert(
        &mut self,
        judgement: i32,
        judgement_type: i32,
        judgement_time_ms: i32,
    ) -> Result<i32, Error> {
        let mut i = 0isize;
        while (i as usize) < self.order.len() {
            let key = self.order[i as usize];
            if self.registered.contains(&key) {
                let out = self.try_convert(key, judgement, judgement_type, judgement_time_ms)?;
                if out != judgement {
                    return Ok(out);
                }
                if !((i as usize) < self.order.len() && self.order[i as usize] == key) {
                    i -= 1;
                }
            }
            i += 1;
        }
        Ok(judgement)
    }

    fn try_convert(&mut self, fid: i32, cur: i32, judgement_type: i32, judgement_time_ms: i32) -> Result<i32, Error> {
        let Some(p) = self.params.get_mut(&fid) else { return Ok(cur) };
        let to = p.convert_to;
        if to == -1 || !p.targets.contains(&(cur as i64)) || cur == to {
            return Ok(cur);
        }
        if to == JUST && self.no_just.contains(&(judgement_type as i64)) {
            return Ok(cur);
        }
        p.count = p.count.wrapping_add(1);
        if p.max_count > 0 && p.max_count <= p.count as i64 {
            let key = p.key;
            self.unregister(fid)?;
            self.id_map.remove(&key);
            self.params.remove(&fid);
            self.limit_finished.insert(key, judgement_time_ms);
        }
        self.converted += 1;
        Ok(to)
    }

    /// The applier's per-frame update of one effect state. Returns the time at which the effect reached its limit
    /// when that is reported now (the caller ends the effect state at that time).
    pub(crate) fn update(&mut self, key: StateKey, state: u8, effect: &ConvertEffect) -> Result<Option<i32>, Error> {
        let mut state = state;
        let finished = self.limit_finished.remove(&key);
        if finished.is_some() {
            state = END_FRAME;
        }
        if state == END_FRAME {
            if let Some(fid) = self.id_map.remove(&key) {
                self.unregister(fid)?;
                self.params.remove(&fid);
            }
        } else if state == EXECUTE_FRAME {
            let targets = self.context.entry(effect.effect_id).or_insert_with(|| effect.targets.to_vec()).clone();
            let convert_to = resolve_convert_to(effect.effect_type, effect.effect_value);
            let fid = self.register();
            if self.id_map.contains_key(&key) {
                return Err(Error::Game("judgement convert state registered twice".into()));
            }
            self.id_map.insert(key, fid);
            if self.params.contains_key(&fid) {
                return Err(Error::Game("judgement convert function registered twice".into()));
            }
            self.params
                .insert(fid, ConvertParam { key, count: 0, max_count: effect.effect_limit_count, targets, convert_to });
        }
        Ok(finished)
    }
}
