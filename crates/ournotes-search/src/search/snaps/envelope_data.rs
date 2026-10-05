//! Per-entry bound data: coefficients, fine terms, windows and candidate records.
use super::*;

/// A class of snaps for one member: the snaps whose active rows are the same in the simulation.
#[derive(Clone, Debug)]
pub(super) struct Class {
    /// Allowed-snap indexes (into `Tables::snaps`).
    pub(super) snaps: Vec<usize>,
    pub(super) rows: Vec<ActiveRow>,
}

/// Per-entry coefficients of the bound, entries sorted by chart time.
#[derive(Clone, Debug, Default)]
pub(super) struct Coef {
    pub(super) times: Vec<i32>,
    /// Power-free part of the float chain before the floor: `adj * level factor * note% * combo / divisor`.
    pub(super) k: Vec<f64>,
    /// Largest judgement percent over the reachable judgements.
    pub(super) max_jp: Vec<f64>,
    /// Judgement percent of Good, Great, Perfect, Just when reachable, else 0.
    pub(super) jp: Vec<[f64; 4]>,
    /// The reachable judgements (bit `j` for judgement `j`) these read: without the conversions of rows with a
    /// conversion budget.
    pub(super) vmask: Vec<u8>,
    /// Factor after the floor (assist times the largest life factor).
    pub(super) z: Vec<f64>,
    /// Prefix sums of `z * k * max_jp` and of `z * k * jp[j]`.
    pub(super) pc: Vec<f64>,
    pub(super) pj: [Vec<f64>; 4],
    /// The same with the life-zero factor (assist times the life-zero factor) in place of `z`.
    pub(super) pcd: Vec<f64>,
    pub(super) pjd: [Vec<f64>; 4],
}

/// What the per-entry bound of one candidate reads, entries in chart-time order. A candidate's entries reach only
/// the judgements its own performers' conversions can give them (at its positions' events), so its combo breaks and
/// judgement percents are those of that reach, never above the pool-wide ones of `Coef`.
#[derive(Clone, Debug)]
pub(super) struct Fine {
    /// Every possible conversion preserves the LUCK input class in all LUCK ranges.
    pub(super) rush_eligible: bool,
    /// Raw judgement of each entry.
    pub(super) raw: Vec<u8>,
    /// Index of the first entry at the same chart time.
    pub(super) group: Vec<u32>,
    /// `adj * level factor * note%` of each entry.
    pub(super) pre: Vec<f64>,
    /// `pre` without the luck rush factor, where some entry has one (empty otherwise): an entry that no rush score
    /// bonus command of the candidate's root can cover reads it (see `RushMasks::spans`).
    pub(super) pre_plain: Vec<f64>,
    pub(super) cnc: f64,
    /// Largest combo factor at any combo up to `c`, for every combo the pool-wide coefficients read (a candidate's
    /// combo counts are never larger).
    pub(super) combo_max: Vec<f64>,
    /// By judgement mask (bit `j` for judgement `j`): the largest judgement percent, the percents of Good, Great,
    /// Perfect, Just when present (else 0), and whether no judgement above Bad is present (the entry breaks the
    /// combo whatever it ends as).
    pub(super) mjp: Vec<f64>,
    pub(super) jp4: Vec<[f64; 4]>,
    pub(super) breaks: Vec<bool>,
    /// Conversion source of each member and class (0: none).
    pub(super) src: Vec<Vec<u32>>,
    /// The judgements a source adds to each entry when its performer is at position `k`: `extra[source][k][entry]`,
    /// a mask; `extra_v` without the conversions of rows with a budget (the judgement percents read it, the combo
    /// breaks read `extra`).
    pub(super) extra: Vec<[Vec<u8>; 5]>,
    pub(super) extra_v: Vec<[Vec<u8>; 5]>,
    /// The rows of each source with a conversion budget: (judgement converted to, most conversions, the entries it
    /// can convert).
    pub(super) budget: Vec<Vec<(u8, f64, Vec<u32>)>>,
    /// With Gekisou on and a combo range: what the candidate's Gekisou combo factor reads (see `GkCombo`).
    pub(super) gcombo: Option<GkCombo>,
    /// Entries whose life is 0 in every play of a candidate that neither recovers life nor guards: the damage
    /// already filed when the entry reads its life empties it. `z_dead` is their factor after the floor.
    pub(super) dead: Vec<bool>,
    /// With Gekisou on (empty otherwise): the factor of each entry's floored bound for the rank bonuses whose range
    /// score contains it, and whether its combo is bounded from the first entry on (a rank bonus that reads it
    /// before a combo break is judged).
    pub(super) rank: Vec<f64>,
    /// With Gekisou on: per completing range with a rank bonus, (range end time, percent / 100, its entries).
    pub(super) rank_ranges: Vec<(i32, f64, Vec<u32>)>,
    /// Network rank bonuses retain historical frame snapshots, so their cap cannot shrink by settled prefix.
    pub(super) network_ranking: bool,
    pub(super) nobreak: Vec<bool>,
    pub(super) z_dead: f64,
    /// The rows of each member and class that can raise the life.
    pub(super) life: Vec<Vec<LifeKind>>,
    /// For candidates that recover life at their skill events: the base life; the slots of the life fold in time
    /// order (a run of life frames that the frame cache can fold twice, or one chart time outside such runs) with
    /// the last time each covers and its damage (the smallest damage of the reachable judgements of its entries);
    /// for each position, the slot of each of its skill events' recoveries and how many times the fold can apply
    /// it; for each entry, the smallest chart time of the entries judged after it.
    pub(super) base: i64,
    /// Diagnostics only: the executions bound of each score frame (`Exec::e`).
    #[cfg(feature = "search-diagnostics")]
    pub(super) exec_profile: Vec<u32>,
    pub(super) slot_end: Vec<i64>,
    pub(super) slot_dmg: Vec<i64>,
    pub(super) ev_slot: [Vec<(usize, i64)>; 5],
    pub(super) until: Vec<i64>,
    /// The smallest `until` from each entry on, and the first entry from which every entry is in `dead`.
    pub(super) until_min: Vec<i64>,
    pub(super) dead_from: usize,
}

/// The rows of one performer that can raise the life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LifeKind {
    None,
    /// Only life recovery rows triggered by the performer's own skill event, recovering this much in total there.
    Recovery(i64),
    /// Anything else (a guard, a recovery with another trigger, a live skill row).
    Other,
}

/// What the per-entry bound of one candidate knows about its life.
#[derive(Clone, Copy, Debug)]
pub(super) enum CandLife {
    /// Nothing raises the life: `Fine::dead` applies.
    NoRise,
    /// Only recoveries at skill events: an entry reads life 0 when its chart time is at least this time and the entries
    /// at chart times up to it are judged no later than the entry (see `Fine::zero_from`).
    ZeroFrom(i64),
    /// No life bound.
    Unknown,
}

impl Fine {
    /// The last time of the first slot at which the life is 0 in the slot-ordered fold of every damage (smallest
    /// damage per entry) and of the recoveries `rec[k]` at position `k`'s skill events, each as many times as the
    /// frame cache can apply it (`i64::MAX`: never). The commands of one slot are folded in an order that is not
    /// known, so a slot with both damage and recovery gives `clamp(life + recovery - damage, 0, 2 * base)`, at least
    /// what any order gives.
    pub(super) fn zero_from(&self, rec: [i64; 5]) -> i64 {
        let mut ups: Vec<(usize, i64)> = Vec::new();
        for (k, slots) in self.ev_slot.iter().enumerate() {
            if rec[k] > 0 {
                ups.extend(slots.iter().map(|&(slot, times)| (slot, rec[k].saturating_mul(times))));
            }
        }
        ups.sort_unstable();
        let cap = 2 * self.base;
        let mut life = self.base;
        let mut j = 0usize;
        for (slot, (&end, &dmg)) in self.slot_end.iter().zip(&self.slot_dmg).enumerate() {
            let mut up = 0i64;
            while j < ups.len() && ups[j].0 == slot {
                up = up.saturating_add(ups[j].1);
                j += 1;
            }
            life = if up == 0 {
                (life - dmg).max(0)
            } else if dmg == 0 {
                life.saturating_add(up).min(cap)
            } else {
                (life.saturating_add(up) - dmg).clamp(0, cap)
            };
            if life <= 0 {
                return end;
            }
        }
        i64::MAX
    }
}

/// The conversions of one performer: its live skill's (with their target judgement and targets) and its snap
/// class's (also with activation time bits and whether the trigger is its own skill event).
pub(super) type ConvSource = (Vec<(i32, Vec<i64>)>, Vec<SnapConv>);

/// A snap class's conversion: judgement converted to, targets, activation time bits, whether the trigger is the
/// performer's own skill event, the frame ranges of a Gekisou row, and its conversion budget (bits).
pub(super) type SnapConv = (i32, Vec<i64>, u32, bool, Option<Vec<(i64, i64)>>, Option<u64>);

/// One window of score factors of a performer: entries `lo..hi` (time order), note factor and judgement factors.
#[derive(Clone, Copy, Debug)]
pub(super) struct Window {
    pub(super) lo: u32,
    pub(super) hi: u32,
    pub(super) note: f64,
    pub(super) judge: [f64; 4],
    /// 0: a flat window; else 1 + index into the contribution's combo ramps, which the candidate cap may evaluate
    /// per entry instead of the flat note factor.
    pub(super) ramp: u32,
    /// Zero for an ordinary window; otherwise one plus its contribution's RushRef index.
    pub(super) rush: u32,
}

/// A Gekisou cumulative note score up counting the playing range's combo (7001): its note factor by unit count
/// (the last value repeats) and its concurrent executions in the window.
#[derive(Clone, Debug)]
pub(super) struct ComboRamp {
    pub(super) unit: i64,
    pub(super) max_count: i64,
    pub(super) table: Rc<Vec<f64>>,
    pub(super) mult: f64,
}

/// The bound data of one (member, class, position).
#[derive(Clone, Debug, Default)]
pub(super) struct Contrib {
    pub(super) windows: Vec<Window>,
    pub(super) gain: f64,
    pub(super) judge: bool,
    /// Score-frame executions that apply this performer's factor commands (see `cand_eps`).
    pub(super) ops: f64,
    /// Command executions excluding rows represented by `rush`.
    pub(super) ops_plain: f64,
    pub(super) cmds: f64,
    pub(super) cmds_plain: f64,
    /// The largest factor of the commands it files (see `factor_drift`).
    pub(super) fac: f64,
    /// Every factor window in chart time (with or without notes): start, end, largest factor on one note.
    pub(super) spans: Vec<(i64, i64, f64)>,
    /// The part of `gain` for the conversions of rows with a conversion budget (see `SnapLive::new`).
    pub(super) budget: f64,
    /// The class's Gekisou combo bonus windows (`combo_windows`).
    pub(super) cb: Vec<ComboBonusRow>,
    /// Combo ramps referenced by `windows`.
    pub(super) ramps: Vec<ComboRamp>,
    pub(super) rush: Vec<rush::RushRef>,
}

/// A judgement conversion some allowed card can register: the judgement it converts to, its target judgements, and
/// the play-frame index ranges `(a, b]` of the frames whose judgements it can see.
pub(super) type Conv = (i32, Vec<i64>, Vec<(i64, i64)>);

/// A snap assignment and its weight.
pub(super) type Assignment = (i64, [Option<usize>; 5]);

/// One candidate of a leaf.
#[derive(Clone, Debug)]
pub(super) struct Cand {
    pub(super) bound: i64,
    pub(super) power: i64,
    pub(super) snaps: [Option<usize>; 5],
    pub(super) snap_ids: [i64; 5],
    pub(super) order: [usize; 5],
    pub(super) classes: [usize; 5],
}

/// The best representative of a member set.
#[derive(Clone, Debug)]
pub(crate) struct LeafBest {
    pub score: i64,
    pub power: i64,
    /// Pool snap index of each slot.
    pub snaps: [Option<usize>; 5],
    pub order: [usize; 5],
}
