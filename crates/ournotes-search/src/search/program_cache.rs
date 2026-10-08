//! Request-local exact native score programs, keyed by complete Performer identity and the member set.
//!
//! The owning Engine fixes master/chart/play/events/clocks/ranking and every LiveParams field except power.
//! Snap IDs and leader identity are deliberately absent: native execution sees their complete Performer
//! programs and total power. Event payoff is NOT cached and must be recalculated for each physical candidate.
//! Rows may retain completed subsets of the 120 orders; missing orders remain explicit. Identity relabelling
//! never merges physical decks or canonical ties.

use crate::search::telemetry::CacheUse;
use crate::search::uniform::{ORDERS, all_orders, order_index};
use ournotes_sim::live::full::{Performer, RecordedOrder, ScoreProgram};
use std::collections::{HashMap, HashSet, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Leave most of a 256 MiB request budget for the pool, bounds, native models and the search frontier.
pub(crate) const DEFAULT_PROGRAM_CACHE_BYTES: usize = 64 * 1024 * 1024;
const ENTRY_OVERHEAD: usize = 1024;
const ADMISSION_BUDGET: usize = 2 * 1024 * 1024;

/// An existing legal deck has another legal leader with exactly the same member/Snap pairs.
/// This is only a recording admission hint: no sibling is evaluated or removed from the traversal here.
pub(super) fn has_legal_leader_sibling(
    pool: &ournotes_sim::pool::Pool,
    domain: &crate::domain::CandidateDomain,
    physical: &crate::search::expectation::PhysicalDeck,
) -> bool {
    if domain.leader().is_some() || domain.check_fixed(pool, physical).is_err() {
        return false;
    }
    crate::search::uniform::NONLEADER.iter().any(|&other| {
        let mut sibling = *physical;
        sibling.members.swap(2, other);
        sibling.snaps.swap(2, other);
        domain.check_fixed(pool, &sibling).is_ok()
    })
}

struct Identity<'a> {
    members: [usize; 5],
    performers: &'a [Performer],
    /// Canonical member index -> physical input slot.
    canonical: [usize; 5],
    /// Physical input slot -> canonical member index.
    slots: [usize; 5],
}

impl<'a> Identity<'a> {
    fn new(members: [usize; 5], performers: &'a [Performer]) -> Option<Self> {
        if performers.len() != 5 {
            return None;
        }
        let mut canonical = [0, 1, 2, 3, 4];
        canonical.sort_unstable_by_key(|&slot| members[slot]);
        if canonical.windows(2).any(|pair| members[pair[0]] == members[pair[1]]) {
            return None;
        }
        let slots = std::array::from_fn(|slot| canonical.iter().position(|&v| v == slot).expect("permutation"));
        Some(Self { members, performers, canonical, slots })
    }

    fn hash(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        for &slot in &self.canonical {
            self.members[slot].hash(&mut hash);
            let p = &self.performers[slot];
            p.live_skill.hash(&mut hash);
            p.support_skills.hash(&mut hash);
            p.band_id.hash(&mut hash);
            p.character_id.hash(&mut hash);
            p.card_type.hash(&mut hash);
            p.tag_ids.hash(&mut hash);
            p.live_skill_categories.hash(&mut hash);
            p.gekisou_skill_categories.hash(&mut hash);
            p.gekisou_mission_type.hash(&mut hash);
            p.gekisou_skill.hash(&mut hash);
            p.gekisou_support_skills.hash(&mut hash);
        }
        hash.finish()
    }
}

struct Key {
    members: [usize; 5],
    performers: [Performer; 5],
}

impl Key {
    fn capture(identity: &Identity<'_>) -> Self {
        Self {
            members: identity.canonical.map(|slot| identity.members[slot]),
            performers: identity.canonical.map(|slot| identity.performers[slot].clone()),
        }
    }

    fn matches(&self, identity: &Identity<'_>) -> bool {
        identity.canonical.iter().enumerate().all(|(canonical, &slot)| {
            self.members[canonical] == identity.members[slot] && self.performers[canonical] == identity.performers[slot]
        })
    }

    fn heap_bytes(&self) -> usize {
        self.performers
            .iter()
            .map(|p| {
                (p.support_skills.capacity() + p.gekisou_support_skills.capacity()) * std::mem::size_of::<(i64, i64)>()
                    + (p.tag_ids.capacity()
                        + p.live_skill_categories.capacity()
                        + p.gekisou_skill_categories.capacity())
                        * std::mem::size_of::<i64>()
            })
            .sum()
    }
}

#[derive(Clone)]
struct ProgramOrder {
    program: Arc<ScoreProgram>,
    final_life: i32,
}

struct Entry {
    id: u64,
    key: Key,
    /// Lexicographic orders of the canonical member layout.
    orders: Vec<Option<ProgramOrder>>,
    bytes: usize,
}

struct Seen {
    id: u64,
    key: Key,
    bytes: usize,
}

/// A hash only chooses a bucket; a hit always compares the complete Performer values, including Vec order.
pub(crate) struct ProgramCache {
    // Entry contains five complete Performers; boxing avoids Vec's spare capacity replicating that large key.
    #[allow(clippy::vec_box)]
    rows: HashMap<u64, Vec<Box<Entry>>>,
    fifo: VecDeque<(u64, u64)>,
    used: usize,
    budget: usize,
    next_id: u64,
    #[allow(clippy::vec_box)]
    seen: HashMap<u64, Vec<Box<Seen>>>,
    seen_fifo: VecDeque<(u64, u64)>,
    seen_bytes: usize,
    recorded_nodes: u64,
    recorded_bytes: u64,
    evaluation_ms: f64,
}

impl ProgramCache {
    pub(crate) fn new(bytes: usize) -> Self {
        Self {
            rows: HashMap::new(),
            fifo: VecDeque::new(),
            used: 0,
            budget: bytes,
            next_id: 0,
            seen: HashMap::new(),
            seen_fifo: VecDeque::new(),
            seen_bytes: 0,
            recorded_nodes: 0,
            recorded_bytes: 0,
            evaluation_ms: 0.0,
        }
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        self.used + self.seen_bytes
    }

    /// Pass this to OrderedLive's recorded variant. It limits retained exports before cache admission.
    pub(crate) fn capture_budget(&self) -> usize {
        self.budget.saturating_sub(self.seen_bytes).saturating_sub(ENTRY_OVERHEAD)
    }

    pub(crate) fn recorded_work(&self) -> (u64, u64) {
        (self.recorded_nodes, self.recorded_bytes)
    }

    pub(crate) fn evaluation_ms(&self) -> f64 {
        self.evaluation_ms
    }

    /// Whether every order of this exact paired-performer set has a complete native program. This query
    /// evaluates no power and supplies no score; scheduling can use it to avoid speculative native siblings.
    pub(crate) fn has_complete(&self, members: [usize; 5], performers: &[Performer]) -> bool {
        if self.budget == 0 {
            return false;
        }
        let Some(identity) = Identity::new(members, performers) else { return false };
        self.rows.get(&identity.hash()).is_some_and(|rows| {
            rows.iter().any(|row| {
                row.key.matches(&identity) && row.orders.len() == ORDERS && row.orders.iter().all(Option::is_some)
            })
        })
    }

    /// Called only after the leaf bounds admit a native order-tree evaluation. Usually the first encounter
    /// records only full identity and the second may record a program. The caller may request first-use
    /// capture for an initial Score incumbent with a legal paired-leader sibling, before a full cutoff exists.
    /// That already-required complete run can then serve the sibling's different power without another live.
    /// The hint changes neither the capacity nor the complete-identity lookup, and never removes a candidate.
    pub(crate) fn capture_budget_for(
        &mut self,
        members: [usize; 5],
        performers: &[Performer],
        first_use_leader_reuse: bool,
        telemetry: &mut CacheUse,
    ) -> usize {
        if self.budget == 0 {
            return 0;
        }
        telemetry.lookups += 1;
        let Some(identity) = Identity::new(members, performers) else { return 0 };
        let hash = identity.hash();
        if self.rows.get(&hash).is_some_and(|rows| rows.iter().any(|row| row.key.matches(&identity)))
            || self.seen.get(&hash).is_some_and(|rows| rows.iter().any(|row| row.key.matches(&identity)))
        {
            telemetry.hits += 1;
            return self.capture_budget();
        }
        let key = Key::capture(&identity);
        let bytes = ENTRY_OVERHEAD + std::mem::size_of::<Seen>() + key.heap_bytes();
        let limit = ADMISSION_BUDGET.min(self.budget);
        if bytes > limit || self.next_id == u64::MAX {
            return 0;
        }
        while self.seen_bytes.saturating_add(bytes) > limit
            || self.allocated_bytes().saturating_add(bytes) > self.budget
        {
            let Some((hash, id)) = self.seen_fifo.pop_front() else { return 0 };
            let rows = self.seen.get_mut(&hash).expect("seen FIFO bucket");
            let index = rows.iter().position(|row| row.id == id).expect("seen FIFO identity");
            self.seen_bytes -= rows.swap_remove(index).bytes;
            if rows.is_empty() {
                self.seen.remove(&hash);
            }
            telemetry.evictions += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.seen_bytes += bytes;
        self.seen.entry(hash).or_default().push(Box::new(Seen { id, key, bytes }));
        self.seen_fifo.push_back((hash, id));
        telemetry.peak_entries = telemetry.peak_entries.max(self.seen_fifo.len());
        if first_use_leader_reuse { self.capture_budget() } else { 0 }
    }

    #[cfg(test)]
    pub(crate) fn get(
        &self,
        members: [usize; 5],
        performers: &[Performer],
        power: i32,
        telemetry: &mut CacheUse,
    ) -> Option<Vec<(i32, i32)>> {
        if self.budget == 0 {
            return None;
        }
        telemetry.lookups += 1;
        let identity = Identity::new(members, performers)?;
        let row = self.rows.get(&identity.hash())?.iter().find(|row| row.key.matches(&identity))?;
        if row.orders.iter().any(Option::is_none) {
            return None;
        }
        telemetry.hits += 1;
        let capacity = row.orders.iter().flatten().map(|order| order.program.node_count()).max().unwrap_or(0);
        let mut scratch = Vec::with_capacity(capacity);
        Some(
            all_orders()
                .into_iter()
                .map(|order| {
                    let cached = row.orders[order_index(&order.map(|slot| identity.slots[slot]))]
                        .as_ref()
                        .expect("complete row");
                    (cached.program.evaluate_into(power, &mut scratch), cached.final_life)
                })
                .collect(),
        )
    }

    /// Exact completed orders of an otherwise partial class. Missing orders remain None and must still be
    /// evaluated or pruned by the ordinary search proof. The result uses the caller's physical slot labels.
    pub(crate) fn get_partial(
        &mut self,
        members: [usize; 5],
        performers: &[Performer],
        power: i32,
        telemetry: &mut CacheUse,
    ) -> Option<Vec<Option<(i32, i32)>>> {
        if self.budget == 0 {
            return None;
        }
        telemetry.lookups += 1;
        let identity = Identity::new(members, performers)?;
        let row = self.rows.get(&identity.hash())?.iter().find(|row| row.key.matches(&identity))?;
        telemetry.hits += 1;
        let started = crate::clock::Instant::now();
        let mut scratch =
            Vec::with_capacity(row.orders.iter().flatten().map(|order| order.program.node_count()).max().unwrap_or(0));
        let evaluated = all_orders()
            .into_iter()
            .map(|order| {
                row.orders[order_index(&order.map(|slot| identity.slots[slot]))]
                    .as_ref()
                    .map(|cached| (cached.program.evaluate_into(power, &mut scratch), cached.final_life))
            })
            .collect();
        self.evaluation_ms += started.elapsed().as_secs_f64() * 1000.0;
        Some(evaluated)
    }

    /// Admit only a complete lottery-free 120-order group. Eviction/oversize/incomplete groups merely miss.
    #[cfg(test)]
    pub(crate) fn insert(
        &mut self,
        members: [usize; 5],
        performers: &[Performer],
        orders: &[[usize; 5]],
        programs: Vec<RecordedOrder>,
        telemetry: &mut CacheUse,
    ) -> bool {
        if self.budget == 0 || orders.len() != ORDERS || programs.len() != ORDERS {
            return false;
        }
        self.insert_partial(members, performers, orders, programs, telemetry)
    }

    /// Retain every independently completed order even when its surrounding native tree was pruned.
    pub(crate) fn insert_partial(
        &mut self,
        members: [usize; 5],
        performers: &[Performer],
        orders: &[[usize; 5]],
        programs: Vec<RecordedOrder>,
        telemetry: &mut CacheUse,
    ) -> bool {
        if self.budget == 0 || orders.len() > ORDERS || programs.is_empty() || programs.len() > orders.len() {
            return false;
        }
        let Some(identity) = Identity::new(members, performers) else {
            return false;
        };
        let hash = identity.hash();
        let old = self.rows.get(&hash).and_then(|rows| rows.iter().find(|row| row.key.matches(&identity)));
        let old_id = old.map(|row| row.id);
        let old_bytes = old.map_or(0, |row| row.bytes);
        let key = Key::capture(&identity);
        let mut canonical: Vec<Option<ProgramOrder>> =
            old.map_or_else(|| (0..ORDERS).map(|_| None).collect(), |row| row.orders.clone());
        let mut seen = [false; ORDERS];
        let mut added = 0;
        let mut unique = HashSet::new();
        let mut exported = HashSet::new();
        let mut bytes = ENTRY_OVERHEAD
            + std::mem::size_of::<Entry>()
            + key.heap_bytes()
            + ORDERS * std::mem::size_of::<Option<ProgramOrder>>();
        for recorded in programs {
            if exported.insert(Arc::as_ptr(&recorded.program) as usize) {
                self.recorded_nodes += recorded.program.node_count() as u64;
                self.recorded_bytes += recorded.program.allocated_bytes() as u64;
            }
            let Some(&order) = orders.get(recorded.index) else {
                return false;
            };
            let mut sorted = order;
            sorted.sort_unstable();
            if sorted != [0, 1, 2, 3, 4] || recorded.random_draws != 0 {
                return false;
            }
            let index = order_index(&order.map(|slot| identity.slots[slot]));
            if std::mem::replace(&mut seen[index], true) {
                return false;
            }
            if canonical[index].is_none() {
                canonical[index] = Some(ProgramOrder { program: recorded.program, final_life: recorded.final_life });
                added += 1;
            }
        }
        if added == 0 {
            return false;
        }
        for recorded in canonical.iter().flatten() {
            if unique.insert(Arc::as_ptr(&recorded.program) as usize) {
                bytes = bytes.saturating_add(recorded.program.allocated_bytes() + 2 * std::mem::size_of::<usize>());
            }
        }
        if bytes > self.budget.saturating_sub(self.seen_bytes) {
            return false;
        }
        while self.allocated_bytes().saturating_sub(old_bytes).saturating_add(bytes) > self.budget {
            let Some((evicted_hash, id)) = self.fifo.pop_front() else {
                return false;
            };
            if old_id == Some(id) {
                self.fifo.push_back((evicted_hash, id));
                continue;
            }
            let bucket = self.rows.get_mut(&evicted_hash).expect("cached FIFO key");
            let index = bucket.iter().position(|row| row.id == id).expect("cached FIFO entry");
            self.used -= bucket.swap_remove(index).bytes;
            if bucket.is_empty() {
                self.rows.remove(&evicted_hash);
            }
            telemetry.evictions += 1;
        }
        if let Some(id) = old_id {
            let row = self
                .rows
                .get_mut(&hash)
                .expect("updated cache bucket")
                .iter_mut()
                .find(|row| row.id == id)
                .expect("updated row");
            row.orders = canonical;
            row.bytes = bytes;
            self.used = self.used - old_bytes + bytes;
            return true;
        }
        if self.next_id == u64::MAX {
            return false;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.used += bytes;
        self.rows.entry(hash).or_default().push(Box::new(Entry { id, key, orders: canonical, bytes }));
        self.fifo.push_back((hash, id));
        telemetry.peak_entries = telemetry.peak_entries.max(self.fifo.len());
        true
    }

    /// Optional class-local score proof on exactly this explicit power interval. Event payoff equivalence
    /// is a separate premise; callers may not reuse these endpoints as payoff bounds without that proof.
    #[allow(dead_code)]
    pub(crate) fn certify_nondecreasing(
        &self,
        members: [usize; 5],
        performers: &[Performer],
        low_power: i32,
        high_power: i32,
    ) -> Option<Vec<(i32, i32)>> {
        let identity = Identity::new(members, performers)?;
        let row = self.rows.get(&identity.hash())?.iter().find(|row| row.key.matches(&identity))?;
        all_orders()
            .into_iter()
            .map(|order| {
                row.orders[order_index(&order.map(|slot| identity.slots[slot]))]
                    .as_ref()?
                    .program
                    .certify_nondecreasing(low_power, high_power)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ournotes_sim::live::full::{JudgedNote, LiveModel, LiveNote, LiveParams, LivePlay, PlayFrame};
    use ournotes_sim::live::random::LiveRandom;
    use ournotes_sim::master::Master;
    use serde_json::json;

    fn program() -> Arc<ScoreProgram> {
        let tables = json!({
            "MasterLiveSettings":[
                {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
                {"_id":3,"_key":"life_base","_value":"1000"},
                {"_id":4,"_key":"life_denger","_value":"300"}],
            "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
            "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
            "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}]
        });
        let texts: Vec<_> = tables
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, rows)| (key.clone(), json!({"_allData":rows}).to_string()))
            .collect();
        let master =
            Master::from_json_tables(|key| texts.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str()))
                .unwrap();
        let notes = [LiveNote { note_id: 1, note_operate_type: 1, judgement_type: 1, time_ms: 40 }];
        let params = LiveParams {
            total_power: 100,
            music_level: 5,
            converted_note_count: 1,
            music_length_ms: 200,
            score_music_length_ms: None,
            skill_target_music_type: 0,
            assist_factor: 1.0,
        };
        let play = LivePlay {
            frames: vec![PlayFrame {
                time_ms: 40,
                judged: vec![JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 40 }],
            }],
            base_seed: 0,
        };
        let model = LiveModel::new(&master, &[], &notes, &[], params).unwrap();
        Arc::new(model.compile_score_program(&play, &[0.04], LiveRandom::new(0)).unwrap().0)
    }

    fn records(program: &Arc<ScoreProgram>) -> Vec<RecordedOrder> {
        // Distinct life sentinels isolate the canonical order remapping test from the scalar scorer.
        (0..ORDERS)
            .map(|index| RecordedOrder { index, program: program.clone(), final_life: index as i32, random_draws: 0 })
            .collect()
    }

    #[test]
    fn program_cache_preserves_all_120_order_identities_and_varies_only_power() {
        let members = [4, 1, 7, 2, 8];
        let performers: [Performer; 5] = std::array::from_fn(|slot| Performer {
            character_id: members[slot] as i64,
            tag_ids: vec![1, 2, 1],
            ..Default::default()
        });
        let program = program();
        let orders = all_orders();
        let mut cache = ProgramCache::new(DEFAULT_PROGRAM_CACHE_BYTES);
        let mut telemetry = CacheUse::default();
        assert!(cache.insert(members, &performers, &orders, records(&program), &mut telemetry));
        for layout in all_orders() {
            let moved_members = layout.map(|slot| members[slot]);
            let moved_performers = layout.map(|slot| performers[slot].clone());
            assert!(cache.has_complete(moved_members, &moved_performers));
            let results = cache.get(moved_members, &moved_performers, 137, &mut telemetry).unwrap();
            for (order, &(score, life)) in orders.iter().zip(&results) {
                assert_eq!(score, program.evaluate(137));
                assert_eq!(life, order_index(&order.map(|slot| layout[slot])) as i32);
            }
        }
        assert_ne!(
            cache.get(members, &performers, 100, &mut telemetry).unwrap()[0].0,
            cache.get(members, &performers, 137, &mut telemetry).unwrap()[0].0
        );
        assert!(cache.certify_nondecreasing(members, &performers, 0, 1000).is_some());
    }

    #[test]
    fn program_cache_compares_every_performer_field_and_ordered_vector() {
        let members = [0, 1, 2, 3, 4];
        let base = Performer {
            live_skill: Some((1, 2)),
            support_skills: vec![(3, 1), (4, 2)],
            band_id: 5,
            character_id: 6,
            card_type: 7,
            tag_ids: vec![8, 9],
            live_skill_categories: vec![10, 11],
            gekisou_skill_categories: vec![12, 13],
            gekisou_mission_type: 2,
            gekisou_skill: Some((14, 3)),
            gekisou_support_skills: vec![(15, 1), (16, 2)],
        };
        let performers = [base.clone(), base.clone(), base.clone(), base.clone(), base.clone()];
        let mut variants = Vec::new();
        let mut changed = base.clone();
        changed.live_skill = Some((1, 3));
        variants.push(changed);
        let mut changed = base.clone();
        changed.support_skills.reverse();
        variants.push(changed);
        let mut changed = base.clone();
        changed.band_id += 1;
        variants.push(changed);
        let mut changed = base.clone();
        changed.character_id += 1;
        variants.push(changed);
        let mut changed = base.clone();
        changed.card_type += 1;
        variants.push(changed);
        let mut changed = base.clone();
        changed.tag_ids.reverse();
        variants.push(changed);
        let mut changed = base.clone();
        changed.live_skill_categories.reverse();
        variants.push(changed);
        let mut changed = base.clone();
        changed.gekisou_skill_categories.reverse();
        variants.push(changed);
        let mut changed = base.clone();
        changed.gekisou_mission_type += 1;
        variants.push(changed);
        let mut changed = base.clone();
        changed.gekisou_skill = Some((14, 4));
        variants.push(changed);
        let mut changed = base.clone();
        changed.gekisou_support_skills.reverse();
        variants.push(changed);
        let mut cache = ProgramCache::new(DEFAULT_PROGRAM_CACHE_BYTES);
        let mut telemetry = CacheUse::default();
        assert!(cache.insert(members, &performers, &all_orders(), records(&program()), &mut telemetry));
        for changed in variants {
            let mut other = performers.clone();
            other[2] = changed;
            assert!(!cache.has_complete(members, &other));
            assert!(cache.get(members, &other, 100, &mut telemetry).is_none());
        }
        assert!(!cache.has_complete([0, 1, 2, 3, 5], &performers));
        assert!(!cache.has_complete([0, 1, 2, 3, 3], &performers));
        assert!(!cache.has_complete(members, &performers[..4]));
        assert!(cache.get([0, 1, 2, 3, 5], &performers, 100, &mut telemetry).is_none());
    }

    #[test]
    fn program_cache_budget_and_incomplete_groups_only_cause_misses() {
        let members = [0, 1, 2, 3, 4];
        let performers: [Performer; 5] = Default::default();
        let orders = all_orders();
        let program = program();
        let mut telemetry = CacheUse::default();
        let mut tiny = ProgramCache::new(1);
        assert!(!tiny.insert(members, &performers, &orders, records(&program), &mut telemetry));
        let mut cache = ProgramCache::new(DEFAULT_PROGRAM_CACHE_BYTES);
        let mut incomplete = records(&program);
        incomplete.pop();
        assert!(!cache.insert(members, &performers, &orders, incomplete, &mut telemetry));
        let mut duplicate = records(&program);
        duplicate[119].index = 0;
        assert!(!cache.insert(members, &performers, &orders, duplicate, &mut telemetry));
        let mut random = records(&program);
        random[0].random_draws = 1;
        assert!(!cache.insert(members, &performers, &orders, random, &mut telemetry));
        assert!(cache.insert(members, &performers, &orders, records(&program), &mut telemetry));
        cache.budget = cache.allocated_bytes();
        assert!(cache.insert([0, 1, 2, 3, 5], &performers, &orders, records(&program), &mut telemetry));
        assert!(cache.get(members, &performers, 100, &mut telemetry).is_none());
        assert_eq!(telemetry.evictions, 1);
        assert!(cache.allocated_bytes() <= cache.budget);
    }

    #[test]
    fn partial_exact_orders_are_reused_before_a_class_is_complete() {
        let members = [9, 3, 7, 1, 5];
        let performers: [Performer; 5] = Default::default();
        let orders = all_orders();
        let program = program();
        let mut cache = ProgramCache::new(DEFAULT_PROGRAM_CACHE_BYTES);
        let mut telemetry = CacheUse::default();
        assert!(cache.insert_partial(
            members,
            &performers,
            &orders,
            records(&program).into_iter().take(45).collect(),
            &mut telemetry
        ));
        assert!(cache.get(members, &performers, 317, &mut telemetry).is_none());
        let partial = cache.get_partial(members, &performers, 317, &mut telemetry).unwrap();
        for (index, value) in partial.iter().enumerate() {
            assert_eq!(*value, (index < 45).then(|| (program.evaluate(317), index as i32)));
        }
        // Relative indices from a subsequent native evaluation of only the missing orders.
        let missing = &orders[45..];
        let rest = records(&program)
            .into_iter()
            .skip(45)
            .map(|mut record| {
                record.index -= 45;
                record
            })
            .collect();
        assert!(cache.insert_partial(members, &performers, missing, rest, &mut telemetry));
        let complete = cache.get(members, &performers, 733, &mut telemetry).unwrap();
        for (index, value) in complete.iter().enumerate() {
            assert_eq!(*value, (program.evaluate(733), index as i32));
        }
    }

    #[test]
    fn recording_admission_counts_only_native_starts_and_preserves_full_identity() {
        let members = [9, 3, 7, 1, 5];
        let mut performers: [Performer; 5] = Default::default();
        performers[0].support_skills = vec![(11, 1), (12, 2)];
        let mut cache = ProgramCache::new(DEFAULT_PROGRAM_CACHE_BYTES);
        let mut lookups = CacheUse::default();
        let mut admissions = CacheUse::default();
        // Fine-bound-only encounters never warm the recording admission policy.
        for _ in 0..3 {
            assert!(cache.get_partial(members, &performers, 317, &mut lookups).is_none());
        }
        assert_eq!(cache.capture_budget_for(members, &performers, false, &mut admissions), 0);
        assert_eq!(admissions.hits, 0);
        let layout = [4, 2, 0, 1, 3];
        let moved = layout.map(|slot| performers[slot].clone());
        assert!(cache.capture_budget_for(layout.map(|slot| members[slot]), &moved, false, &mut admissions) > 0);
        assert_eq!(admissions.hits, 1);
        assert!(cache.get_partial(members, &performers, 411, &mut lookups).is_none());
        assert_eq!(lookups.hits, 0, "seen identity is not a program hit");
        let retained = cache.allocated_bytes();
        cache.budget = retained;
        performers[0].support_skills.reverse();
        assert_eq!(cache.capture_budget_for(members, &performers, false, &mut admissions), 0);
        assert_eq!(admissions.hits, 1, "Vec order remains part of complete program identity");
        assert_eq!(admissions.evictions, 1);
        assert!(cache.allocated_bytes() <= cache.budget);
        // FIFO admission eviction only loses a recording opportunity; a fresh first encounter stays native.
        performers[0].support_skills.reverse();
        assert_eq!(cache.capture_budget_for(members, &performers, false, &mut admissions), 0);
    }

    #[test]
    fn first_use_leader_recording_requires_a_legal_paired_sibling() {
        use crate::domain::CandidateDomain;
        use crate::search::Constraints;
        use crate::search::expectation::PhysicalDeck;
        use crate::search::gate_tests::common::{Rng, roster, set_column, synth_snaps};
        use ournotes_sim::pool::Pool;

        let mut source = synth_snaps(&mut Rng::new(81), 5, 2, &[]);
        set_column(&mut source, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
        let master = source.master();
        let owned = roster(&mut Rng::new(82), &master);
        let pool = Pool::new(&master, &owned).unwrap();
        let constraints = Constraints { include_members: vec![pool.members[0].id], ..Default::default() };
        let domain = CandidateDomain::build(&pool, &constraints).unwrap();
        let deck = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [Some(0), None, Some(1), None, None] };
        for order in all_orders() {
            let current = PhysicalDeck {
                members: order.map(|slot| deck.members[slot]),
                snaps: order.map(|slot| deck.snaps[slot]),
            };
            assert!(has_legal_leader_sibling(&pool, &domain, &current));
            let fixed = Constraints { leader: Some(pool.members[current.members[2]].id), ..constraints.clone() };
            let fixed = CandidateDomain::build(&pool, &fixed).unwrap();
            assert!(!has_legal_leader_sibling(&pool, &fixed, &current));
        }
        let excluded = Constraints { exclude_snaps: vec![pool.snaps[0].id], ..constraints };
        let excluded = CandidateDomain::build(&pool, &excluded).unwrap();
        assert!(!has_legal_leader_sibling(&pool, &excluded, &deck));
        let mut duplicate_resource = deck;
        duplicate_resource.snaps[1] = duplicate_resource.snaps[0];
        assert!(!has_legal_leader_sibling(&pool, &domain, &duplicate_resource));
    }

    #[test]
    fn first_use_leader_recording_keeps_seen_identity_and_only_reuses_complete_orders() {
        let members = [9, 3, 7, 1, 5];
        let performers: [Performer; 5] = std::array::from_fn(|slot| Performer {
            character_id: members[slot] as i64,
            support_skills: vec![(11, 1), (12, 2)],
            ..Default::default()
        });
        let mut cache = ProgramCache::new(DEFAULT_PROGRAM_CACHE_BYTES);
        let mut admissions = CacheUse::default();
        let mut lookups = CacheUse::default();
        let capture = cache.capture_budget_for(members, &performers, true, &mut admissions);
        assert!(capture > 0);
        assert!(capture < DEFAULT_PROGRAM_CACHE_BYTES);
        assert_eq!(capture, cache.capture_budget());
        assert_eq!(cache.seen_fifo.len(), 1);
        assert_eq!(admissions.hits, 0, "first-use capture is not a reuse hit");
        assert!(!cache.has_complete(members, &performers));
        assert!(cache.get_partial(members, &performers, 317, &mut lookups).is_none());
        // A cancelled or otherwise unsuccessful recording leaves only Seen, never an order certificate.
        assert_eq!(cache.capture_budget_for(members, &performers, false, &mut admissions), capture);
        assert_eq!(admissions.hits, 1);
        assert!(cache.get_partial(members, &performers, 317, &mut lookups).is_none());

        let program = program();
        let orders = all_orders();
        assert!(cache.insert_partial(
            members,
            &performers,
            &orders,
            records(&program).into_iter().take(45).collect(),
            &mut lookups,
        ));
        let layout = [0, 2, 1, 3, 4];
        let moved_members = layout.map(|slot| members[slot]);
        let moved = layout.map(|slot| performers[slot].clone());
        assert!(!cache.has_complete(moved_members, &moved));
        assert!(cache.get(moved_members, &moved, 733, &mut lookups).is_none());
        let partial = cache.get_partial(moved_members, &moved, 733, &mut lookups).unwrap();
        for (order, actual) in orders.iter().zip(&partial) {
            let origin = order_index(&order.map(|slot| layout[slot]));
            assert_eq!(*actual, (origin < 45).then(|| (program.evaluate(733), origin as i32)));
        }
        let remaining = records(&program)
            .into_iter()
            .skip(45)
            .map(|mut row| {
                row.index -= 45;
                row
            })
            .collect();
        assert!(cache.insert_partial(members, &performers, &orders[45..], remaining, &mut lookups));
        assert!(cache.has_complete(moved_members, &moved));
        let complete = cache.get(moved_members, &moved, 733, &mut lookups).unwrap();
        for (order, &(score, life)) in orders.iter().zip(&complete) {
            assert_eq!(score, program.evaluate(733));
            assert_eq!(life, order_index(&order.map(|slot| layout[slot])) as i32);
        }
        let mut changed = moved;
        changed[2].support_skills.reverse();
        assert!(cache.get_partial(moved_members, &changed, 733, &mut lookups).is_none());
        assert!(cache.allocated_bytes() <= DEFAULT_PROGRAM_CACHE_BYTES);
    }

    #[test]
    fn first_use_leader_recording_never_bypasses_capacity_or_identity_admission() {
        let members = [0, 1, 2, 3, 4];
        let performers: [Performer; 5] = Default::default();
        for budget in [0, 1, ENTRY_OVERHEAD] {
            let mut cache = ProgramCache::new(budget);
            let mut telemetry = CacheUse::default();
            assert_eq!(cache.capture_budget_for(members, &performers, true, &mut telemetry), 0);
            assert_eq!(cache.allocated_bytes(), 0);
            assert!(cache.rows.is_empty());
            assert!(cache.seen.is_empty());
            assert!(!cache.has_complete(members, &performers));
        }
        let mut cache = ProgramCache::new(DEFAULT_PROGRAM_CACHE_BYTES);
        let mut telemetry = CacheUse::default();
        assert_eq!(cache.capture_budget_for([0, 1, 2, 3, 3], &performers, true, &mut telemetry), 0);
        assert_eq!(cache.capture_budget_for(members, &performers[..4], true, &mut telemetry), 0);
        assert_eq!(cache.allocated_bytes(), 0);
        assert_eq!(cache.capture_budget_for(members, &performers, false, &mut telemetry), 0);
        let retained = cache.allocated_bytes();
        cache.budget = retained;
        assert_eq!(cache.capture_budget_for(members, &performers, true, &mut telemetry), 0);
        assert_eq!(cache.allocated_bytes(), retained);
        assert!(cache.get_partial(members, &performers, 100, &mut telemetry).is_none());
    }
}
