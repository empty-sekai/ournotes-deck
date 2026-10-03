//! Life of a played live: a time-stamped command log folded frame by frame, with the frame cache of the game.

use crate::error::Error;
use crate::live::score::get_frame;
use crate::master::Master;
use crate::num::FxHashMap;

const NOTE_DAMAGE: u8 = 0;
const SKILL_DAMAGE: u8 = 1;
const RECOVERY: u8 = 2;
const GUARD_START: u8 = 3;
const GUARD_END: u8 = 4;
const REDUCTION_START: u8 = 5;
const REDUCTION_END: u8 = 6;

/// Judgements that neither damage nor count (Wait, Pass).
const WAIT: i32 = 0;
const PASS: i32 = 7;

/// Frames kept after the music length.
const BUFFER_FRAMES: i32 = 2;

#[derive(Clone, Copy, Debug)]
struct LifeCommand {
    time_ms: i32,
    kind: u8,
    value: i32,
    allow_over_heal: bool,
    safety: bool,
}

/// `max(x, 0)`.
#[inline]
fn non_negative(x: i32) -> i32 {
    if x < 0 { 0 } else { x }
}

/// The life settings: base life and the damage of each note judgement.
pub(crate) fn life_settings(master: &Master) -> Result<(i32, FxHashMap<i64, i64>), Error> {
    let setting = |key: &str| -> Result<i64, Error> {
        let r = master
            .live_settings
            .iter()
            .find(|r| r.key == key)
            .ok_or_else(|| Error::Master(format!("MasterLiveSettings {key} missing")))?;
        r.value.trim().parse::<i64>().map_err(|_| Error::Master(format!("MasterLiveSettings {key} is not an integer")))
    };
    let base = setting("life_base")?;
    setting("life_denger")?;
    let mut damage = FxHashMap::default();
    for r in &master.judgement_parameters {
        if damage.insert(r.note_simulate_judgement, r.damage).is_some() {
            return Err(Error::Master("duplicate judgement parameter".into()));
        }
    }
    Ok((base as i32, damage))
}

/// The life controller.
#[derive(Clone, Debug)]
pub(crate) struct LifeController {
    initial_life: i32,
    internal_max_life: i32,
    pub current_life: i32,
    damage: FxHashMap<i64, i64>,
    guard_ids: Vec<i32>,
    guard_id_counter: i32,
    reduction_ids: FxHashMap<i32, i32>,
    reduction_id_counter: i32,
    max_life_limits: FxHashMap<i32, i32>,
    max_life_id_counter: i32,
    max_frame: i32,
    commands: Vec<Vec<LifeCommand>>,
    cached_complete_frame: i32,
    cached_life: i32,
    cached_guard: i32,
    cached_reduction: i32,
}

impl LifeController {
    pub(crate) fn new(life: i32, damage: FxHashMap<i64, i64>, music_length_ms: i32) -> Result<LifeController, Error> {
        let max_frame = get_frame(music_length_ms).wrapping_add(BUFFER_FRAMES);
        if max_frame < 0 {
            return Err(Error::Game("life command log: negative frame count".into()));
        }
        Ok(LifeController {
            initial_life: life,
            internal_max_life: life,
            current_life: life,
            damage,
            guard_ids: Vec::new(),
            guard_id_counter: 0,
            reduction_ids: FxHashMap::default(),
            reduction_id_counter: 0,
            max_life_limits: FxHashMap::default(),
            max_life_id_counter: 0,
            max_frame,
            commands: vec![Vec::new(); max_frame as usize],
            cached_complete_frame: -1,
            cached_life: 0,
            cached_guard: 0,
            cached_reduction: 0,
        })
    }

    pub(crate) fn max_life(&self) -> i32 {
        self.internal_max_life
    }

    fn frame_of(&self, ms: i32) -> i32 {
        let f = get_frame(ms);
        if self.max_frame <= f { self.max_frame.wrapping_sub(1) } else { f }
    }

    fn add_command(&mut self, cmd: LifeCommand) -> Result<(), Error> {
        let f = self.frame_of(cmd.time_ms);
        if f < 0 || f as usize >= self.commands.len() {
            return Err(Error::Game("life command frame out of range".into()));
        }
        let list = &mut self.commands[f as usize];
        let mut i = list.len();
        while i >= 1 && cmd.time_ms < list[i - 1].time_ms {
            i -= 1;
        }
        list.insert(i, cmd);
        if f <= self.cached_complete_frame {
            self.cached_complete_frame = f - 1;
        }
        Ok(())
    }

    /// Note damage of a judgement at the note's chart time.
    pub(crate) fn add_note_damage(&mut self, time_ms: i32, judgement: i32) -> Result<(), Error> {
        if judgement == WAIT || judgement == PASS {
            return Ok(());
        }
        let d = *self
            .damage
            .get(&(judgement as i64))
            .ok_or_else(|| Error::Game(format!("judgement {judgement} has no damage entry")))? as i32;
        if d > 0 {
            self.add_command(LifeCommand {
                time_ms,
                kind: NOTE_DAMAGE,
                value: d,
                allow_over_heal: false,
                safety: false,
            })?;
        }
        Ok(())
    }

    pub(crate) fn recovery(&mut self, time_ms: i32, amount: i64, allow_over_heal: bool) -> Result<(), Error> {
        let amount = amount as i32;
        if amount > 0 {
            self.add_command(LifeCommand { time_ms, kind: RECOVERY, value: amount, allow_over_heal, safety: false })?;
        }
        Ok(())
    }

    pub(crate) fn damage(&mut self, time_ms: i32, amount: i64, safety: bool) -> Result<(), Error> {
        let amount = amount as i32;
        if amount > 0 {
            self.add_command(LifeCommand {
                time_ms,
                kind: SKILL_DAMAGE,
                value: amount,
                allow_over_heal: false,
                safety,
            })?;
        }
        Ok(())
    }

    pub(crate) fn enable_damage_reduction(&mut self, time_ms: i32, bp: i64) -> Result<i32, Error> {
        self.reduction_id_counter = self.reduction_id_counter.wrapping_add(1);
        let id = self.reduction_id_counter;
        if self.reduction_ids.contains_key(&id) {
            return Err(Error::Game("duplicate damage reduction id".into()));
        }
        self.reduction_ids.insert(id, bp as i32);
        self.add_command(LifeCommand {
            time_ms,
            kind: REDUCTION_START,
            value: bp as i32,
            allow_over_heal: false,
            safety: false,
        })?;
        Ok(id)
    }

    pub(crate) fn disable_damage_reduction(&mut self, time_ms: i32, id: i32) -> Result<(), Error> {
        let Some(bp) = self.reduction_ids.remove(&id) else { return Ok(()) };
        self.add_command(LifeCommand { time_ms, kind: REDUCTION_END, value: bp, allow_over_heal: false, safety: false })
    }

    pub(crate) fn add_life_limit(&mut self, amount: i64) -> Result<i32, Error> {
        self.max_life_id_counter = self.max_life_id_counter.wrapping_add(1);
        let id = self.max_life_id_counter;
        if self.max_life_limits.contains_key(&id) {
            return Err(Error::Game("duplicate life limit id".into()));
        }
        self.max_life_limits.insert(id, amount as i32);
        self.update_life_max();
        Ok(id)
    }

    pub(crate) fn subtract_life_limit(&mut self, id: i32) {
        if self.max_life_limits.remove(&id).is_some() {
            self.update_life_max();
        }
    }

    fn update_life_max(&mut self) {
        // This updates the current maximum, not the initial maximum, and is not a time-stamped command.
        let sum = self.max_life_limits.values().fold(0i32, |a, &v| a.wrapping_add(v));
        self.internal_max_life = self.internal_max_life.wrapping_add(sum);
    }

    pub(crate) fn enable_guard(&mut self, time_ms: i32) -> Result<i32, Error> {
        self.guard_id_counter = self.guard_id_counter.wrapping_add(1);
        let id = self.guard_id_counter;
        self.guard_ids.push(id);
        self.add_command(LifeCommand { time_ms, kind: GUARD_START, value: 0, allow_over_heal: false, safety: false })?;
        Ok(id)
    }

    pub(crate) fn disable_guard(&mut self, time_ms: i32, id: i32) -> Result<(), Error> {
        let Some(pos) = self.guard_ids.iter().position(|&g| g == id) else { return Ok(()) };
        self.guard_ids.remove(pos);
        self.add_command(LifeCommand { time_ms, kind: GUARD_END, value: 0, allow_over_heal: false, safety: false })
    }

    fn apply_command(&self, life: i32, cmd: &LifeCommand, guard: i32, reduction: i32) -> (i32, i32, i32) {
        let (mut life, mut guard, mut reduction) = (life, guard, reduction);
        match cmd.kind {
            NOTE_DAMAGE => {
                if guard < 1 {
                    life = non_negative(life.wrapping_sub(apply_damage_reduction(cmd.value, reduction)));
                }
            }
            SKILL_DAMAGE => {
                if guard < 1 && (life > 0 || !cmd.safety) {
                    life = life.wrapping_sub(apply_damage_reduction(cmd.value, reduction));
                    life = life.max(i32::from(cmd.safety));
                }
            }
            RECOVERY => {
                if life > 0 {
                    let cap = if cmd.allow_over_heal {
                        self.internal_max_life.wrapping_shl(1)
                    } else {
                        self.internal_max_life
                    };
                    let s = cmd.value.wrapping_add(life);
                    life = if s < cap { s } else { cap };
                }
            }
            GUARD_START => guard = guard.wrapping_add(1),
            GUARD_END => guard = non_negative(guard.wrapping_sub(1)),
            REDUCTION_START => reduction = reduction.wrapping_add(cmd.value),
            REDUCTION_END => reduction = non_negative(reduction.wrapping_sub(cmd.value)),
            _ => {}
        }
        (life, guard, reduction)
    }

    /// Life at a music time. The frame cache is invalidated only by lowering its complete frame when a command lands
    /// at or before it; the cached state is kept, so a later query above the lowered frame folds the frames in between
    /// again from that state (the game's behaviour).
    pub(crate) fn get_life_at_ms(&mut self, ms: i32) -> Result<i32, Error> {
        let f = self.frame_of(ms);
        let c = self.cached_complete_frame;
        let (full, mut life, mut guard, mut reduction, start) = if c < 0 || f <= c {
            (c >= 0 && f <= c, self.initial_life, 0, 0, 0)
        } else {
            (false, self.cached_life, self.cached_guard, self.cached_reduction, c + 1)
        };
        for k in start..f.max(start) {
            let cmds = self.commands.get(k as usize).ok_or_else(|| Error::Game("life frame out of range".into()))?;
            for cmd in cmds {
                (life, guard, reduction) = self.apply_command(life, cmd, guard, reduction);
            }
        }
        if f < 0 {
            return Ok(life);
        }
        if !full {
            self.cached_complete_frame = f - 1;
            self.cached_life = life;
            self.cached_guard = guard;
            self.cached_reduction = reduction;
        }
        if f >= self.max_frame {
            return Ok(life);
        }
        let cmds = self.commands.get(f as usize).ok_or_else(|| Error::Game("life frame out of range".into()))?;
        for cmd in cmds {
            if cmd.time_ms > ms {
                break;
            }
            (life, guard, reduction) = self.apply_command(life, cmd, guard, reduction);
        }
        Ok(life)
    }

    /// Stores the life at the frame time as the current life.
    pub(crate) fn sync_current_life(&mut self, t: i32) -> Result<i32, Error> {
        let life = self.get_life_at_ms(t)?;
        self.current_life = life;
        Ok(life)
    }
}

/// Damage after a reduction in basis points.
fn apply_damage_reduction(damage: i32, reduction_bp: i32) -> i32 {
    if reduction_bp < 1 {
        return damage;
    }
    if (reduction_bp as u32 >> 4) > 0x270 {
        return 0;
    }
    non_negative(10000i32.wrapping_sub(reduction_bp).wrapping_mul(damage) / 10000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> LifeController {
        LifeController::new(1000, FxHashMap::from_iter([(1, 100)]), 1000).unwrap()
    }

    #[test]
    fn safe_skill_damage_does_not_kill_or_revive() {
        let mut c = controller();
        c.damage(0, 5000, true).unwrap();
        assert_eq!(c.get_life_at_ms(0).unwrap(), 1);
        c.add_note_damage(1, 1).unwrap();
        c.damage(2, 5000, true).unwrap();
        c.recovery(3, 5000, true).unwrap();
        assert_eq!(c.get_life_at_ms(3).unwrap(), 0);
        let mut c = controller();
        c.damage(0, 5000, false).unwrap();
        assert_eq!(c.get_life_at_ms(0).unwrap(), 0);
    }

    #[test]
    fn reductions_stack_and_expire_by_id() {
        let mut c = controller();
        let a = c.enable_damage_reduction(0, 2000).unwrap();
        let b = c.enable_damage_reduction(0, 3000).unwrap();
        c.add_note_damage(1, 1).unwrap();
        c.disable_damage_reduction(2, a).unwrap();
        c.disable_damage_reduction(2, a).unwrap();
        c.damage(3, 100, true).unwrap();
        c.disable_damage_reduction(4, b).unwrap();
        c.add_note_damage(5, 1).unwrap();
        assert_eq!(c.get_life_at_ms(5).unwrap(), 1000 - 50 - 70 - 100);
    }

    #[test]
    fn guards_block_skill_damage_and_negative_amounts_add_no_commands() {
        let mut c = controller();
        let guard = c.enable_guard(0).unwrap();
        c.damage(1, 5000, true).unwrap();
        c.disable_guard(2, guard).unwrap();
        c.damage(3, -1, false).unwrap();
        c.damage(4, 1i64 << 32, false).unwrap();
        assert_eq!(c.get_life_at_ms(4).unwrap(), 1000);
    }

    #[test]
    fn life_limit_updates_accumulate_on_the_current_maximum() {
        let mut c = controller();
        let a = c.add_life_limit(100).unwrap();
        assert_eq!(c.internal_max_life, 1100);
        let b = c.add_life_limit(200).unwrap();
        assert_eq!(c.internal_max_life, 1400);
        c.subtract_life_limit(a);
        assert_eq!(c.internal_max_life, 1600);
        c.subtract_life_limit(b);
        c.subtract_life_limit(b);
        assert_eq!(c.internal_max_life, 1600);
        c.recovery(0, 5000, false).unwrap();
        assert_eq!(c.get_life_at_ms(0).unwrap(), 1600);
        c.recovery(1, 5000, true).unwrap();
        assert_eq!(c.get_life_at_ms(1).unwrap(), 3200);
    }

    #[test]
    fn reduction_saturates_at_full_and_wraps_its_integer_product() {
        assert_eq!(apply_damage_reduction(100, 10000), 0);
        assert_eq!(apply_damage_reduction(100, 20000), 0);
        assert_eq!(apply_damage_reduction(100, -100), 100);
        assert_eq!(apply_damage_reduction(i32::MAX, 5000), 0);
    }
}
