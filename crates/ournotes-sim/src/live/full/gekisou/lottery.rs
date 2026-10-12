//! The lottery machine of the luck ranges: its tables, draws, result guarantees and nominal probabilities.

use super::*;

/// One lottery item: `(weight, value)`.
pub(super) type Item = (i64, i64);

fn nominal_weight_overflow() -> Error {
    Error::Capacity("LUCK DP lottery weight overflow".into())
}

/// The nominal model requires a positive ordinary modulus. Reject native wrapping tables instead of
/// treating wrapped totals as a normalized probability distribution.
fn nominal_total(items: &[Item]) -> Result<i32, Error> {
    items.iter().try_fold(0i32, |sum, &(weight, _)| {
        let weight = i32::try_from(weight).map_err(|_| nominal_weight_overflow())?;
        if weight < 0 {
            return Err(Error::Unsupported("LUCK DP: negative lottery weight".into()));
        }
        sum.checked_add(weight).ok_or_else(nominal_weight_overflow)
    })
}

fn nominal_probabilities(weights: Vec<(u64, u64, i64)>) -> Vec<(f64, i64)> {
    let mut out: Vec<(f64, i64)> = Vec::new();
    for (weight, total, result) in weights {
        let probability = weight as f64 / total as f64;
        if let Some(item) = out.iter_mut().find(|item| item.1 == result) {
            item.0 += probability;
        } else {
            out.push((probability, result));
        }
    }
    out
}

/// A weighted draw over items in order: `|r| % total`, the first item whose running weight exceeds it.
fn lottery_table(items: &[Item], r: i32) -> Result<Option<i64>, Error> {
    if items.is_empty() {
        return Ok(None);
    }
    let total = items.iter().fold(0i32, |t, it| t.wrapping_add(it.0 as i32));
    if total < 1 {
        return Err(game("lottery table with a total weight below 1"));
    }
    let m = srem(r.wrapping_abs(), total);
    let mut cum = 0i32;
    for it in items {
        cum = cum.wrapping_add(it.0 as i32);
        if m < cum {
            return Ok(Some(it.1));
        }
    }
    Ok(None)
}

/// The luck bonus table of one lot type: items by descending result, weights scaled by the lot probability buff.
#[derive(Clone, Debug)]
pub(super) struct LuckSkillTable {
    pub(super) items: Vec<Item>,
    pub(super) buff: f32,
    pub(super) total_weight: i32,
}

impl LuckSkillTable {
    pub(super) fn new(mut items: Vec<Item>) -> Result<LuckSkillTable, Error> {
        items.sort_by_key(|it| std::cmp::Reverse(it.1));
        let total = items.iter().fold(0i32, |t, it| t.wrapping_add(it.0 as i32));
        if !items.is_empty() && total < 1 {
            return Err(game("luck bonus table with a total weight below 1"));
        }
        Ok(LuckSkillTable { items, buff: 0f32, total_weight: total })
    }

    pub(super) fn lottery(&self, r: i32) -> Option<i64> {
        if self.items.is_empty() {
            return None;
        }
        let m = srem(r.wrapping_abs(), self.total_weight);
        let mut cum = 0i32;
        for it in &self.items {
            cum = cum.wrapping_add(buffed(self.buff, it.0));
            if m < cum {
                return Some(it.1);
            }
        }
        None
    }

    /// The draw restricted to results `>= minimum`; the other items' weight is shared out over them.
    pub(super) fn lottery_with_minimum(&self, r: i32, minimum: i64) -> Option<i64> {
        if self.items.is_empty() {
            return None;
        }
        let (mut inc_n, mut inc_sum, mut exc_sum) = (0i32, 0i32, 0i32);
        for it in &self.items {
            let w = buffed(self.buff, it.0);
            if minimum <= it.1 {
                inc_n += 1;
                inc_sum = inc_sum.wrapping_add(w);
            } else {
                exc_sum = exc_sum.wrapping_add(w);
            }
        }
        if inc_n == 0 {
            return None;
        }
        let per = sdiv(exc_sum, inc_n);
        let total = inc_sum.wrapping_add(per.wrapping_mul(inc_n));
        let m = srem(r.wrapping_abs(), total);
        let mut cum = 0i32;
        for it in &self.items {
            if minimum <= it.1 {
                cum = cum.wrapping_add(per).wrapping_add(buffed(self.buff, it.0));
                if m < cum {
                    return Some(it.1);
                }
            }
        }
        None
    }
}

/// Exact nominal intervals of the current native table, retaining its binary32 buff.
pub(super) fn nominal_bonus_weights(table: &LuckSkillTable, minimum: i64) -> Result<Vec<(u64, u64, i64)>, Error> {
    if table.items.is_empty() {
        return Err(game("no luck lottery item"));
    }
    if table.items.iter().any(|&(_, result)| !(0..=3).contains(&result)) {
        return Err(Error::Unsupported("LUCK DP: lottery result outside Miss/Hit/Super Hit/Critical".into()));
    }
    let (mut total, mut per) = (nominal_total(&table.items)?, 0i32);
    if minimum >= 1 {
        let (mut included, mut excluded, mut n) = (0i32, 0i32, 0i32);
        for &(weight, result) in &table.items {
            let weight = buffed(table.buff, weight);
            if weight < 0 {
                return Err(Error::Unsupported("LUCK DP: negative lottery weight".into()));
            }
            if result >= minimum {
                included = included.checked_add(weight).ok_or_else(nominal_weight_overflow)?;
                n += 1;
            } else {
                excluded = excluded.checked_add(weight).ok_or_else(nominal_weight_overflow)?;
            }
        }
        if n == 0 {
            return Err(game("no luck lottery item"));
        }
        per = sdiv(excluded, n);
        total = per.checked_mul(n).and_then(|v| included.checked_add(v)).ok_or_else(nominal_weight_overflow)?;
    }
    if total < 1 {
        return Err(game("lottery table with a total weight below 1"));
    }
    let (mut out, mut lo) = (Vec::new(), 0i32);
    for &(weight, result) in &table.items {
        if minimum >= 1 && result < minimum {
            continue;
        }
        let weight = per.checked_add(buffed(table.buff, weight)).ok_or_else(nominal_weight_overflow)?;
        if weight < 0 {
            return Err(Error::Unsupported("LUCK DP: negative lottery weight".into()));
        }
        let hi = lo.checked_add(weight).ok_or_else(|| Error::Capacity("LUCK DP lottery weight overflow".into()))?;
        // Without a minimum the modulus remains the ORIGINAL, unbuffed total. Buffed cumulative
        // intervals therefore truncate at that total; normalizing the buffed weights changes the game.
        let (a, b) = (lo.clamp(0, total), hi.clamp(0, total));
        if b > a {
            out.push(((b - a) as u64, total as u64, result));
        }
        lo = hi;
    }
    if lo < total {
        return Err(game("no luck lottery item"));
    }
    Ok(out)
}

/// The luck lottery: base points per judged note, bonus results per lot, minimum-result guarantees.
#[derive(Clone, Debug)]
pub(crate) struct LotteryMachine {
    pub(super) good: Option<Vec<Item>>,
    pub(super) great: Option<Vec<Item>>,
    pub(super) perfect: Option<Vec<Item>>,
    pub(super) hold: Vec<Item>,
    pub(super) tables: Vec<LuckSkillTable>,
    /// `(id, result, remaining)`; remaining -1 is unlimited.
    pub(super) minimum: Vec<(i32, i64, i64)>,
    pub(super) minimum_counter: i32,
    pub(super) last_consumed: FxHashMap<i32, i32>,
}

fn push_items(out: &mut Vec<u64>, items: &[Item]) {
    out.push(items.len() as u64);
    for &(weight, result) in items {
        out.extend([weight as u64, result as u64]);
    }
}

impl LotteryMachine {
    /// Every field as words (binary32 by bit pattern, every list after its length): equal words mean equal machines.
    pub(crate) fn push_words(&self, out: &mut Vec<u64>) {
        let Self { good, great, perfect, hold, tables, minimum, minimum_counter, last_consumed } = self;
        for items in [good, great, perfect] {
            match items {
                None => out.push(0),
                Some(items) => {
                    out.push(1);
                    push_items(out, items);
                }
            }
        }
        push_items(out, hold);
        out.push(tables.len() as u64);
        for LuckSkillTable { items, buff, total_weight } in tables {
            push_items(out, items);
            out.extend([u64::from(buff.to_bits()), u64::from(*total_weight as u32)]);
        }
        out.push(minimum.len() as u64);
        for &(id, result, remaining) in minimum {
            out.extend([u64::from(id as u32), result as u64, remaining as u64]);
        }
        out.push(u64::from(*minimum_counter as u32));
        let mut consumed: Vec<_> = last_consumed.iter().map(|(&id, &frame)| (id, frame)).collect();
        consumed.sort_unstable();
        out.push(consumed.len() as u64);
        out.extend(consumed.into_iter().map(|(id, frame)| u64::from(id as u32) << 32 | u64::from(frame as u32)));
    }

    /// Nominal independent draw probabilities, with `|r| % total` treated as uniform. This is not a
    /// distribution over the correlated output of every possible seeded System.Random instance.
    pub(crate) fn bonus_probabilities(
        &self,
        lot_type: usize,
        buff: i32,
        minimum: i64,
    ) -> Result<Vec<(f64, i64)>, Error> {
        Ok(nominal_probabilities(self.bonus_probability_weights(lot_type, buff, minimum)?))
    }

    /// Integer interval lengths over the native modulus. Repeated results remain separate, in table order,
    /// so callers can accumulate either binary64 estimates or outward probability certificates.
    pub(crate) fn bonus_probability_weights(
        &self,
        lot_type: usize,
        buff: i32,
        minimum: i64,
    ) -> Result<Vec<(u64, u64, i64)>, Error> {
        let mut table = self.tables.get(lot_type).ok_or_else(|| game("lot type out of range"))?.clone();
        table.buff = buff as f32 / 100f32;
        nominal_bonus_weights(&table, minimum)
    }

    /// Nominal base-point probabilities using the same note and judgement dispatch as `base_point`.
    pub(crate) fn base_point_probabilities(&self, note_type: i32, j: i32) -> Result<Vec<(f64, i64)>, Error> {
        Ok(nominal_probabilities(self.base_point_probability_weights(note_type, j)?))
    }

    /// Exact nominal base-point masses before any division or aggregation.
    pub(crate) fn base_point_probability_weights(&self, note_type: i32, j: i32) -> Result<Vec<(u64, u64, i64)>, Error> {
        if NON_LUCK_NOTE_TYPES.contains(&note_type) {
            return Ok(vec![(1, 1, 0)]);
        }
        let u = j.wrapping_add(1) as u32;
        if u < 9 && (0x107u32 >> (u & 31)) & 1 != 0 {
            return Ok(vec![(1, 1, 0)]);
        }
        let items = if is_sub_note(note_type) {
            &self.hold
        } else if (j.wrapping_sub(5) as u32) < 2 {
            self.perfect.as_ref().ok_or_else(|| game("no Perfect base point table"))?
        } else if j == J_GREAT {
            self.great.as_ref().ok_or_else(|| game("no Great base point table"))?
        } else if j == J_GOOD {
            self.good.as_ref().ok_or_else(|| game("no Good base point table"))?
        } else {
            return Ok(vec![(1, 1, 0)]);
        };
        let total = nominal_total(items)?;
        if items.is_empty() || total < 1 {
            return Err(game("empty base point table"));
        }
        let mut out = Vec::new();
        for &(weight, point) in items {
            let weight = i32::try_from(weight).map_err(|_| Error::Capacity("LUCK DP base weight overflow".into()))?;
            if weight < 0 {
                return Err(Error::Unsupported("LUCK DP: negative base-point weight".into()));
            }
            out.push((weight as u64, total as u64, point));
        }
        Ok(out)
    }

    pub(super) fn from_master(master: &Master) -> Result<LotteryMachine, Error> {
        let (mut good, mut great, mut perfect, mut hold) = (None, None, None, Vec::new());
        for r in master.gekisou_luck_base_points.iter().filter(|r| r.weight > 0) {
            let it = (r.weight, r.base_point);
            if r.note_category == 0 {
                match r.note_simulate_judgement {
                    3 => good.get_or_insert_with(Vec::new).push(it),
                    4 => great.get_or_insert_with(Vec::new).push(it),
                    5 => perfect.get_or_insert_with(Vec::new).push(it),
                    _ => {}
                }
            } else if r.note_category == 1 && r.note_simulate_judgement == J_PERFECT as i64 {
                hold.push(it);
            }
        }
        let mut bonus: Vec<Vec<Item>> = vec![Vec::new(); 5];
        for r in master.gekisou_luck_bonus_lots.iter().filter(|r| r.weight > 0) {
            if (0..5).contains(&r.chance_lot_type) {
                bonus[r.chance_lot_type as usize].push((r.weight, r.lot_result));
            }
        }
        let tables = bonus.into_iter().map(LuckSkillTable::new).collect::<Result<Vec<_>, _>>()?;
        Ok(LotteryMachine {
            good,
            great,
            perfect,
            hold,
            tables,
            minimum: Vec::new(),
            minimum_counter: 0,
            last_consumed: FxHashMap::default(),
        })
    }

    pub(super) fn set_weight_buff(&mut self, percent: i32) {
        let mul = percent as f32 / 100f32;
        for t in self.tables.iter_mut() {
            t.buff = mul;
        }
    }

    fn max_minimum_result(&self) -> i64 {
        let mut m = 0i64;
        for &(_, res, _) in &self.minimum {
            if m <= res {
                m = res;
            }
        }
        m
    }

    fn consume_minimum(&mut self, applied: i64, t: i32) {
        let last = &mut self.last_consumed;
        self.minimum.retain_mut(|e| {
            if e.1 > applied || e.2 < 0 {
                return true;
            }
            e.2 = (e.2 as i32).wrapping_sub(1) as i64;
            if e.2 != 0 {
                return true;
            }
            last.insert(e.0, t);
            false
        });
    }

    pub(crate) fn enable_minimum(&mut self, result: i64, limit: i64) -> i32 {
        self.minimum_counter = self.minimum_counter.wrapping_add(1);
        self.minimum.push((self.minimum_counter, result, if limit >= 1 { limit } else { -1 }));
        self.minimum_counter
    }

    pub(crate) fn disable_minimum(&mut self, id: i32) {
        self.minimum.retain(|e| e.0 != id);
    }

    pub(crate) fn is_minimum_active(&self, id: i32) -> bool {
        self.minimum.iter().any(|e| e.0 == id)
    }

    pub(crate) fn take_last_consumed(&mut self, id: i32) -> i32 {
        self.last_consumed.remove(&id).unwrap_or(-1)
    }

    pub(super) fn luck_bonus(&mut self, lot_type: usize, t: i32, random: &mut LiveRandom) -> Result<i64, Error> {
        if random.is_nominal() {
            let applied = self.max_minimum_result();
            let table = self.tables.get(lot_type).ok_or_else(|| game("lot type out of range"))?;
            let result = random.nominal_lottery(nominal_bonus_weights(table, applied)?)?;
            if applied >= 1 {
                self.consume_minimum(applied, t);
            }
            return Ok(result);
        }
        let r = random.next_int(LUCK);
        let applied = self.max_minimum_result();
        let table = self.tables.get(lot_type).ok_or_else(|| game("lot type out of range"))?;
        if applied < 1 {
            return table.lottery(r).ok_or_else(|| game("no luck lottery item"));
        }
        let it = table.lottery_with_minimum(r, applied).ok_or_else(|| game("no luck lottery item"))?;
        self.consume_minimum(applied, t);
        Ok(it)
    }

    pub(super) fn base_point(&mut self, note_type: i32, j: i32, random: &mut LiveRandom) -> Result<i64, Error> {
        if NON_LUCK_NOTE_TYPES.contains(&note_type) {
            return Ok(0);
        }
        let u = j.wrapping_add(1) as u32;
        if u < 9 && (0x107u32 >> (u & 31)) & 1 != 0 {
            return Ok(0);
        }
        if random.is_nominal() {
            return random.nominal_lottery(self.base_point_probability_weights(note_type, j)?);
        }
        let r = random.next_int(LUCK);
        let items = if is_sub_note(note_type) {
            &self.hold
        } else if (j.wrapping_sub(5) as u32) < 2 {
            self.perfect.as_ref().ok_or_else(|| game("no Perfect base point table"))?
        } else if j == J_GREAT {
            self.great.as_ref().ok_or_else(|| game("no Great base point table"))?
        } else if j == J_GOOD {
            self.good.as_ref().ok_or_else(|| game("no Good base point table"))?
        } else {
            return Ok(0);
        };
        lottery_table(items, r)?.ok_or_else(|| game("empty base point table"))
    }
}
