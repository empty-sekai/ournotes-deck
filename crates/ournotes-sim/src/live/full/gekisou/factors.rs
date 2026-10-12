//! The factor commands of the controller: lot probability ups and gauge ups.

use super::*;

/// Timed lot probability and gauge factors added by skills.
#[derive(Clone, Debug, Default)]
pub(super) struct FactorStorage {
    pub(super) lot_cmds: Vec<(i32, f32)>,
    pub(super) gauge_cmds: Vec<(i32, f32)>,
    lot_ids: FxHashMap<i32, f32>,
    gauge_ids: FxHashMap<i32, f32>,
    current_id: i32,
}

/// Whether every sum of a subset of `cmds`' factors is a binary32 value, so that adding them in any order rounds
/// nowhere: each factor is an integer multiple of one power of two, and their absolute values sum to at most 2^24
/// of it.
pub(super) fn exact_sums(cmds: &[(i32, f32)]) -> bool {
    // (odd integer, exponent) with factor = ±integer * 2^exponent.
    let mut parts = Vec::with_capacity(cmds.len());
    for &(_, f) in cmds {
        if !f.is_finite() {
            return false;
        }
        let bits = f.to_bits();
        let (biased, fraction) = (((bits >> 23) & 0xff) as i32, bits & 0x7f_ffff);
        let (mantissa, exponent) = if biased == 0 { (fraction, -149) } else { (fraction | 0x80_0000, biased - 150) };
        if mantissa != 0 {
            let zeros = mantissa.trailing_zeros();
            parts.push((mantissa >> zeros, exponent + zeros as i32));
        }
    }
    let Some(least) = parts.iter().map(|&(_, exponent)| exponent).min() else {
        return true;
    };
    // A sum of at most 2^24 units of 2^least is finite.
    if least > 100 {
        return false;
    }
    let mut units = 0u64;
    for (integer, exponent) in parts {
        let shift = exponent - least;
        if shift > 24 {
            return false;
        }
        units += u64::from(integer) << shift;
        if units > 1 << 24 {
            return false;
        }
    }
    true
}

impl FactorStorage {
    pub(super) fn at(cmds: &[(i32, f32)], t: i32) -> f32 {
        let mut s = 0f32;
        for &(ct, d) in cmds {
            if ct <= t {
                s += d;
            }
        }
        s
    }

    pub(super) fn add(&mut self, gauge: bool, t: i32, f: f32) -> i32 {
        self.current_id = self.current_id.wrapping_add(1);
        let (cmds, ids) =
            if gauge { (&mut self.gauge_cmds, &mut self.gauge_ids) } else { (&mut self.lot_cmds, &mut self.lot_ids) };
        ids.insert(self.current_id, f);
        cmds.push((t, f));
        self.current_id
    }

    pub(super) fn subtract(&mut self, gauge: bool, t: i32, id: i32) -> Result<(), Error> {
        let (cmds, ids) =
            if gauge { (&mut self.gauge_cmds, &mut self.gauge_ids) } else { (&mut self.lot_cmds, &mut self.lot_ids) };
        let f = ids.remove(&id).ok_or_else(|| game("luck factor id not found"))?;
        cmds.push((t, -f));
        Ok(())
    }
}
