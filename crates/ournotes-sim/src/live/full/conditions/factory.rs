//! Conditions and cumulative conditions built from the master data.

use super::*;

pub(super) fn life_percent_threshold(value: i64, initial_life: i32) -> i32 {
    ceil_to_i32((value as f32 / 10f32) * initial_life as f32)
}

pub(super) fn no_value() -> Error {
    Error::Master("skill condition without a value".into())
}

fn missing_target(id: i64) -> Error {
    Error::Master(format!("unknown skill target {id}"))
}

/// The live member-target predicate deliberately ignores the target's discriminator.
pub(super) fn target_matches(tg: &SkillTargetRow, p: &Performer) -> bool {
    p.matches_skill_target(tg)
}

/// Builds checkers for the performer at `member` of `deck`.
pub(crate) struct Factory<'a> {
    pub master: &'a Master,
    pub deck: &'a [Performer],
    pub initial_life: i32,
    pub initial_time_ms: i32,
    pub skill_target_music_type: i64,
}

impl Factory<'_> {
    pub(super) fn one(&self, cid: i64, k: usize) -> Result<Option<Checker>, Error> {
        let m = self.master;
        let c = m.skill_condition(cid).ok_or_else(|| Error::Master(format!("unknown skill condition {cid}")))?;
        let v0 = c.condition_values.first().copied();
        let targets: Vec<Option<&SkillTargetRow>> = c.condition_target_ids.iter().map(|&i| m.skill_target(i)).collect();
        let target = |i: usize| targets[i].ok_or_else(|| missing_target(c.condition_target_ids[i]));
        let missions = || -> Result<Vec<i64>, Error> {
            let mut out = Vec::new();
            for i in 0..targets.len() {
                let t = target(i)?;
                if t.skill_target_type == TARGET_MISSION && t.gekisou_mission_type != 0 {
                    out.push(t.gekisou_mission_type);
                }
            }
            Ok(out)
        };
        let ch = match c.condition_type {
            0 => return Ok(None),
            2000 => Checker::LifeGreater(v0),
            2001 => Checker::LifeAtLeast(v0),
            2002 => Checker::LifeLess(v0),
            2003 => Checker::LifeAtMost(v0),
            2004 => Checker::LifeChanged { previous: self.initial_life },
            4007 => Checker::LifePercent {
                threshold: life_percent_threshold(v0.ok_or_else(no_value)?, self.initial_life),
                previous: 0,
            },
            4008 | 4009 => Checker::LifeDelta {
                threshold: v0.ok_or_else(no_value)? as i32,
                increase: c.condition_type == 4008,
                count: 0,
                previous: self.initial_life,
            },
            3000 | 3001 => {
                let mut count = 0;
                for member in self.deck {
                    for i in 0..targets.len() {
                        if target_matches(target(i)?, member) {
                            count += 1;
                            break;
                        }
                    }
                }
                Checker::Fixed(if c.condition_type == 3000 { count > 0 } else { count == self.deck.len() })
            }
            4001 => Checker::LiveComboMultiple { threshold: v0.ok_or_else(no_value)? as i32, previous: 0 },
            4002 => Checker::LiveComboAtLeast(v0.ok_or_else(no_value)? as i32),
            4000 => Checker::ElapsedTime {
                period: (v0.ok_or_else(no_value)? as i32).wrapping_mul(1000),
                elapsed: 0,
                previous: self.initial_time_ms,
            },
            4010 | 5020 => Checker::SameMemberLiveSkill(k as i32),
            5021 => Checker::SnapGekisouStart { previous: -1 },
            4012 => {
                let mut hit = false;
                for i in 0..targets.len() {
                    if target(i)?.live_music_type == self.skill_target_music_type {
                        hit = true;
                        break;
                    }
                }
                Checker::Fixed(hit)
            }
            4011 => Checker::Probability(v0.ok_or_else(no_value)? as f32 / 100f32),
            5000 => {
                let mut fixed = false;
                if let Some(p) = self.deck.get(k) {
                    for i in 0..targets.len() {
                        if target_matches(target(i)?, p) {
                            fixed = true;
                            break;
                        }
                    }
                }
                Checker::Fixed(fixed)
            }
            8000 => Checker::Fixed(false),
            1000 | 1010 | 1020 | 1030 | 1040 => {
                let mut judgements = Vec::with_capacity(targets.len());
                for i in 0..targets.len() {
                    let j = target(i)?.judgement;
                    if j != NO_JUDGEMENT {
                        judgements.push(j);
                    }
                }
                if matches!(c.condition_type, 1030 | 1040) {
                    Checker::NoteJudgementCount {
                        n: v0.ok_or_else(no_value)?,
                        consecutive: c.condition_type == 1040,
                        targets: judgements,
                        count: 0,
                        override_ms: None,
                    }
                } else {
                    Checker::NoteJudgementMatch { kind: c.condition_type, targets: judgements, override_ms: None }
                }
            }
            7000 => Checker::LuckLotResult { target: v0.ok_or_else(no_value)?, override_ms: None },
            7001 | 7002 | 7004 => Checker::GkInterval {
                kind: c.condition_type,
                n: v0.ok_or_else(no_value)? as i32,
                previous: 0,
                override_ms: None,
            },
            7003 => Checker::JustAtLeast { threshold: v0.ok_or_else(no_value)? as i32, override_ms: None },
            7006 | 7007 => Checker::GkJustEdge {
                raw: c.condition_type == 7007,
                threshold: v0.ok_or_else(no_value)? as i32,
                previous: 0,
                last_range: -1,
                override_ms: None,
            },
            7011 => Checker::GkLiveStart(false),
            7012 => Checker::GkReachRank { threshold: v0.unwrap_or(1) as i32, triggered: false },
            7005 => Checker::ComboAtLeast { threshold: v0.ok_or_else(no_value)?, override_ms: None },
            7010 => Checker::RangeStart { missions: missions()?, triggered: Vec::new(), override_ms: None },
            7013 => Checker::RangeComplete,
            7020 => Checker::RangePlaying { missions: missions()?, active: Vec::new(), override_ms: None },
            7021 => Checker::LuckRushPlaying(false),
            t => return Err(Error::Unsupported(format!("skill condition type {t}"))),
        };
        Ok(Some(if c.is_positive { ch } else { Checker::Not(Box::new(ch)) }))
    }

    /// The checker of a condition group (`None` for group 0 or a group without a non-empty set).
    pub(crate) fn group(&self, gid: i64, k: usize) -> Result<Option<Checker>, Error> {
        self.group_from(gid, |cid| self.one(cid, k))
    }

    pub(crate) fn luck_group(
        &self,
        gid: i64,
        k: usize,
        script: &crate::live::full::luck::SharedScript,
    ) -> Result<Option<Checker>, Error> {
        self.group_from(gid, |cid| {
            let row = self
                .master
                .skill_condition(cid)
                .ok_or_else(|| Error::Master(format!("unknown skill condition {cid}")))?;
            match row.condition_type {
                2000..=2004 | 4011 | 5000 | 3000 | 3001 => {
                    let checker = Checker::Scripted {
                        script: script.clone(),
                        sticky: matches!(row.condition_type, 5000 | 3000 | 3001),
                        memo: None,
                        true_hits: if matches!(row.condition_type, 2004 | 4011) { 0 } else { 1 },
                        life: (2000..=2003)
                            .contains(&row.condition_type)
                            .then(|| row.condition_values.first().map(|&v| (row.condition_type, v)))
                            .flatten(),
                    };
                    Ok(Some(if row.is_positive { checker } else { Checker::Not(Box::new(checker)) }))
                }
                0 | 7000 | 7010 | 7013 | 7020 | 7021 => self.one(cid, k),
                ty => Err(Error::Unsupported(format!("LUCK replay condition {ty}"))),
            }
        })
    }

    fn group_from(
        &self,
        gid: i64,
        mut one: impl FnMut(i64) -> Result<Option<Checker>, Error>,
    ) -> Result<Option<Checker>, Error> {
        if gid == 0 {
            return Ok(None);
        }
        let mut ors = Vec::new();
        for s in self.master.skill_condition_sets.iter().filter(|s| s.group == gid) {
            let mut items = Vec::new();
            for &cid in &s.condition_ids {
                if let Some(c) = one(cid)? {
                    items.push(c);
                }
            }
            match items.len() {
                0 => {}
                1 => ors.push(items.pop().expect("one item")),
                _ => {
                    let resettable = items.iter().map(Checker::count_resettable).collect();
                    ors.push(Checker::And { items, resettable });
                }
            }
        }
        Ok(match ors.len() {
            0 => None,
            1 => ors.pop(),
            _ => Some(Checker::Or(ors)),
        })
    }

    /// The cumulative condition of an effect (id 0: none).
    pub(crate) fn cumulative(&self, cid: i64, k: usize) -> Result<Option<Cumulative>, Error> {
        if cid == 0 {
            return Ok(None);
        }
        let m = self.master;
        let c =
            m.cumulative_condition(cid).ok_or_else(|| Error::Master(format!("unknown cumulative condition {cid}")))?;
        let max = if c.max_cumulative_count < 1 { i32::MAX as i64 } else { c.max_cumulative_count };
        let n = c.condition_values.first().copied().unwrap_or(0);
        let values_empty = c.condition_values.is_empty();
        match c.condition_type {
            7000 | 7001 => Ok(Some(Cumulative::ComboPerN { n, values_empty, max, just: c.condition_type == 7000 })),
            2000 | 2001 => {
                if c.condition_type == 2000 && values_empty {
                    return Err(no_value());
                }
                let init =
                    if c.condition_type == 2000 { floor_to_i32(self.initial_life as f32 / n as f32) as i64 } else { 0 };
                Ok(Some(Cumulative::LifePerN { init, n, values_empty, max, limit: c.condition_type == 2001 }))
            }
            1000..=1002 => {
                let mut targets = Vec::with_capacity(c.condition_target_ids.len());
                for &i in &c.condition_target_ids {
                    let j = m.skill_target(i).ok_or_else(|| missing_target(i))?.judgement;
                    if j != NO_JUDGEMENT {
                        targets.push(j);
                    }
                }
                Ok(Some(Cumulative::JudgementPerN { n, values_empty, targets, max, count: 0, kind: c.condition_type }))
            }
            3000..=3005 => {
                let count = match c.condition_type {
                    3000 | 3001 => {
                        let targets = c
                            .condition_target_ids
                            .iter()
                            .map(|&id| m.skill_target(id).ok_or_else(|| missing_target(id)))
                            .collect::<Result<Vec<_>, _>>()?;
                        self.deck
                            .iter()
                            .enumerate()
                            .filter(|(index, member)| {
                                (c.condition_type != 3001 || *index != k)
                                    && targets.iter().any(|target| member.matches_skill_target(target))
                            })
                            .count()
                    }
                    3002 | 3003 => {
                        let band = self
                            .deck
                            .get(k)
                            .ok_or_else(|| Error::Game("cumulative self member index out of range".into()))?
                            .band_id;
                        if c.condition_type == 3002 {
                            self.deck.iter().filter(|p| p.band_id == band).count()
                        } else {
                            self.deck
                                .iter()
                                .map(|p| p.band_id)
                                .filter(|&b| b != band)
                                .collect::<std::collections::HashSet<_>>()
                                .len()
                        }
                    }
                    3004 => self.deck.iter().map(|p| p.band_id).collect::<std::collections::HashSet<_>>().len(),
                    _ => self.deck.iter().map(|p| p.card_type).collect::<std::collections::HashSet<_>>().len(),
                };
                Ok(Some(Cumulative::Fixed((count as i64).min(max))))
            }
            6000 => {
                let n = c.condition_values.first().copied().ok_or_else(no_value)? as i32;
                Ok(Some(Cumulative::ElapsedTime {
                    period: n.wrapping_mul(1000),
                    elapsed: 0,
                    previous: self.initial_time_ms,
                    count: 0,
                    max,
                }))
            }
            // The client's checker conversion throws for any other value, 0 included.
            t => Err(Error::Game(format!("ArgumentOutOfRangeException: cumulative condition type {t}"))),
        }
    }
}
