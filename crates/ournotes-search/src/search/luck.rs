//! Request-local, ordered LUCK replay memoization. A failed optional replay keeps
//! the original envelope; neither capacity nor replay limits remove candidates.
use super::expectation::{FiniteSeedLaw, PhysicalDeck};
use super::snaps::{FullSetup, RushMasks, deck_performers};
use super::telemetry::{CacheUse, LuckReplay};
use super::{Pool, SearchRequest};
use crate::domain::CandidateDomain;
use ournotes_sim::Error;
use ournotes_sim::live::full::{luck_signature, rush_branches_why};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

/// Replay keys remembered; each holds a shared pointer, the masks themselves count against `MAX_DISTINCT`.
const MAX_CACHE: usize = 1 << 16;
const MAX_DISTINCT: usize = 1024;
const MAX_RUNS: usize = 64;
/// Positions buckets with more distinct branches keep only their union.
const MAX_BRANCHES: usize = 16;

/// Ordered variant IDs by native position, and the native positions of the physical slots.
type ReplayKey = ([u32; 5], [usize; 5]);
/// The union of a key's replays over its roots (or why there is none) and their distinct branches (at most
/// `MAX_BRANCHES + 1`), with the diagnostic outcome.
type Replayed =
    (Result<ournotes_sim::live::full::RushMasks, Unavailable>, Vec<ournotes_sim::live::full::RushMasks>, Outcome);
/// The diagnostic outcome label of a replay and its decline detail (diagnostics feature only; else "masked").
type Outcome = (&'static str, serde_json::Value);

/// The caller of the next queries, for the per-site counts.
#[derive(Clone, Copy, Default)]
pub(crate) enum Site {
    #[default]
    Fine,
    Cutoff,
    Prefix,
    /// Per-branch leaf fine bounds (buckets without branch lists read their union).
    Branch,
}

#[cfg(feature = "search-diagnostics")]
impl Site {
    fn label(self) -> &'static str {
        match self {
            Site::Fine => "fine",
            Site::Cutoff => "cutoff",
            Site::Prefix => "prefix",
            Site::Branch => "branch",
        }
    }
}

/// Why a query has no masks.
#[derive(Clone, Copy)]
enum Unavailable {
    NoVariant,
    NoRoots,
    Declined,
    RootShape,
    Cached,
}

/// Diagnostics only: replay queries by caller and outcome, and the first distinct declined keys.
#[cfg(feature = "search-diagnostics")]
#[derive(Default)]
pub(crate) struct LuckDiag {
    outcomes: HashMap<ReplayKey, &'static str>,
    by_site: BTreeMap<(&'static str, &'static str), u64>,
    declines: Vec<(ReplayKey, serde_json::Value, u64)>,
    /// Leaf fine checks by coverage ("all", "some", "none" masked orders, "skipped" without queries) and outcome.
    pub leaves: BTreeMap<(&'static str, bool), u64>,
    /// Every 64th leaf that survived the fine check (at most 256): card ids, snap ids, fine cap and threshold.
    pub survivors: Vec<serde_json::Value>,
    pub survivors_seen: u64,
}

#[cfg(feature = "search-diagnostics")]
impl LuckDiag {
    fn record(&mut self, site: Site, key: Option<ReplayKey>, outcome: &'static str) {
        *self.by_site.entry((site.label(), outcome)).or_default() += 1;
        if let Some(key) = key
            && let Some(row) = self.declines.iter_mut().find(|row| row.0 == key)
        {
            row.2 += 1;
        }
    }

    pub(crate) fn report(&self, variants: usize, distinct: usize) -> serde_json::Value {
        let by_site: Vec<_> =
            self.by_site.iter().map(|((site, outcome), n)| serde_json::json!([site, outcome, n])).collect();
        let leaves: Vec<_> =
            self.leaves.iter().map(|((cover, pruned), n)| serde_json::json!([cover, pruned, n])).collect();
        let declines: Vec<_> = self
            .declines
            .iter()
            .map(|((ids, positions), detail, n)| {
                serde_json::json!({"variants": ids, "positions": positions, "queries": n, "detail": detail})
            })
            .collect();
        serde_json::json!({
            "variantCount": variants,
            "distinctMasks": distinct,
            "distinctKeys": self.outcomes.len(),
            "bySiteOutcome": by_site,
            "leafCoverage": leaves,
            "declines": declines,
            "fineSurvivorsSeen": self.survivors_seen,
            "fineSurvivors": self.survivors,
        })
    }
}

pub(crate) struct LuckOracle {
    setup: FullSetup,
    // Pair identities index the original pool, independent of conversion partitions.
    pairs: HashMap<(usize, Option<usize>), Option<u32>>,
    representatives: Vec<(usize, Option<usize>)>,
    roots: BTreeMap<[usize; 5], Vec<i32>>,
    cache: HashMap<ReplayKey, Option<RushMasks>>,
    /// Distinct replay results, shared by every key that produced them. Equal content gives equal bounds, and the
    /// shared pointer also keys the downstream window and summary caches.
    interned: HashSet<RushMasks>,
    /// The distinct branch masks of the keys asked for them (two to `MAX_BRANCHES`, else None), interned apart
    /// from the unions so that rare branch queries do not evict the unions.
    branch_cache: HashMap<ReplayKey, Option<Rc<Vec<RushMasks>>>>,
    branch_interned: HashSet<RushMasks>,
    /// Every life a live of the domain can reach (compiled bounds), deciding the replay's life conditions.
    life: Option<(i64, i64)>,
    pub site: Site,
    pub stats: LuckReplay,
    pub cache_use: CacheUse,
    pub branch_cache_use: CacheUse,
    #[cfg(feature = "search-diagnostics")]
    pub diag: LuckDiag,
}

impl LuckOracle {
    pub fn new(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        law: &FiniteSeedLaw,
        life: Option<(i64, i64)>,
    ) -> Result<Option<Self>, Error> {
        let Some(setup) = super::full_setup(pool, &request.objective)?.filter(|s| s.gk.is_some()) else {
            return Ok(None);
        };
        if !setup.gk.as_ref().expect("Gekisou setup").setup.missions.contains(&2) {
            return Ok(None);
        }
        if domain.members().len().saturating_mul(domain.snaps().len().saturating_add(1)) > 1_000_000 {
            return Ok(None);
        }
        let mut pairs = HashMap::new();
        let mut signatures = HashMap::new();
        let mut representatives = Vec::new();
        for &m in domain.members() {
            for s in std::iter::once(None).chain(domain.snaps().iter().copied().map(Some)) {
                // Reuse performer construction, retaining all static context for admission.
                let deck =
                    ournotes_sim::pool::Deck { members: [m; 5], snaps: [s; 5], performance_order: [0, 1, 2, 3, 4] };
                let performers = deck_performers(pool, &deck)?;
                let signature = luck_signature(pool.master, &performers[0])?;
                let id = signature.map(|key| {
                    let next = signatures.len() as u32;
                    *signatures.entry(key).or_insert_with(|| {
                        representatives.push((m, s));
                        next
                    })
                });
                pairs.insert((m, s), id);
            }
        }
        let mut roots: BTreeMap<_, Vec<i32>> = BTreeMap::new();
        for &(root, _) in law.atoms() {
            let entry = roots.entry(super::joint::positions(root)?).or_default();
            if !entry.contains(&root) {
                entry.push(root);
            }
        }
        Ok(Some(Self {
            setup,
            pairs,
            representatives,
            roots,
            cache: HashMap::new(),
            interned: HashSet::new(),
            branch_cache: HashMap::new(),
            branch_interned: HashSet::new(),
            life,
            site: Site::default(),
            stats: LuckReplay { enabled: true, ..LuckReplay::default() },
            cache_use: CacheUse::default(),
            branch_cache_use: CacheUse::default(),
            #[cfg(feature = "search-diagnostics")]
            diag: LuckDiag::default(),
        }))
    }

    fn query(&mut self) {
        self.stats.queries += 1;
        self.site_counts().queries += 1;
    }

    fn unavailable(&mut self, why: Unavailable) {
        let u = &mut self.stats.unavailable;
        u.total += 1;
        *match why {
            Unavailable::NoVariant => &mut u.no_variant,
            Unavailable::NoRoots => &mut u.no_roots,
            Unavailable::Declined => &mut u.declined,
            Unavailable::RootShape => &mut u.root_shape,
            Unavailable::Cached => &mut u.cached,
        } += 1;
        self.site_counts().unavailable += 1;
    }

    fn site_counts(&mut self) -> &mut super::telemetry::Site {
        let s = &mut self.stats.sites;
        match self.site {
            Site::Fine => &mut s.fine,
            Site::Cutoff => &mut s.cutoff,
            Site::Prefix => &mut s.prefix,
            Site::Branch => &mut s.branch,
        }
    }

    pub(crate) fn catalog(&self) -> &[(usize, Option<usize>)] {
        &self.representatives
    }

    pub(crate) fn variant_count(&self) -> usize {
        self.representatives.len()
    }

    /// Diagnostics only: distinct replay results currently shared.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn distinct_masks(&self) -> usize {
        self.interned.len()
    }

    pub(crate) fn variant(&self, member: usize, snap: Option<usize>) -> Option<u32> {
        self.pairs.get(&(member, snap)).copied().flatten()
    }

    pub(crate) fn deck_variants(&self, physical: &PhysicalDeck) -> Option<[u32; 5]> {
        let mut ids = [0; 5];
        for (slot, id) in ids.iter_mut().enumerate() {
            *id = self.variant(physical.members[slot], physical.snaps[slot])?;
        }
        Some(ids)
    }

    pub fn masks(
        &mut self,
        pool: &Pool,
        physical: &PhysicalDeck,
        positions: &[usize; 5],
    ) -> Result<Option<RushMasks>, Error> {
        let Some(ids) = self.deck_variants(physical) else {
            self.query();
            self.unavailable(Unavailable::NoVariant);
            #[cfg(feature = "search-diagnostics")]
            self.diag.record(self.site, None, "noVariant");
            return Ok(None);
        };
        self.masks_for_variants(pool, ids, positions)
    }

    /// The distinct branch masks of a candidate's positions bucket (see [`LuckOracle::masks`]); None when the
    /// replay is unavailable or the bucket has one branch or more than `MAX_BRANCHES`. Computed on first request
    /// (one more replay of the key), then cached.
    pub fn branches(
        &mut self,
        pool: &Pool,
        physical: &PhysicalDeck,
        positions: &[usize; 5],
    ) -> Result<Option<Rc<Vec<RushMasks>>>, Error> {
        let Some(physical_ids) = self.deck_variants(physical) else { return Ok(None) };
        let Some((key, order)) = self.key(physical_ids, positions) else { return Ok(None) };
        self.branch_cache_use.lookups += 1;
        if let Some(found) = self.branch_cache.get(&key) {
            self.branch_cache_use.hits += 1;
            return Ok(found.clone());
        }
        let Some(roots) = self.roots.get(positions).cloned() else { return Ok(None) };
        let (combined, branches, _) = self.replay(pool, physical_ids, order, &roots)?;
        let list = (combined.is_ok() && (2..=MAX_BRANCHES).contains(&branches.len())).then_some(branches);
        let fresh = list.as_ref().map_or(0, |b| b.iter().filter(|m| !self.branch_interned.contains(*m)).count());
        if self.branch_cache.len() >= MAX_CACHE || self.branch_interned.len() + fresh > MAX_DISTINCT {
            self.branch_cache_use.evictions += self.branch_cache.len() as u64;
            self.branch_cache.clear();
            self.branch_interned.clear();
        }
        let list = list.map(|list| {
            Rc::new(
                list.into_iter()
                    .map(|masks| match self.branch_interned.get(&masks) {
                        Some(found) => found.clone(),
                        None => {
                            let masks = Rc::new(masks);
                            self.branch_interned.insert(masks.clone());
                            masks
                        }
                    })
                    .collect(),
            )
        });
        self.branch_cache.insert(key, list.clone());
        self.branch_cache_use.peak_entries = self.branch_cache_use.peak_entries.max(self.branch_cache.len());
        Ok(list)
    }

    /// The replay key of a candidate (variant IDs by native position, and the positions) and its performance
    /// order; None for an unknown variant.
    fn key(&self, physical_ids: [u32; 5], positions: &[usize; 5]) -> Option<(ReplayKey, [usize; 5])> {
        let mut ids = [0; 5];
        let mut order = [0; 5];
        for s in 0..5 {
            if physical_ids[s] as usize >= self.representatives.len() {
                return None;
            }
            ids[positions[s]] = physical_ids[s];
            order[positions[s]] = s;
        }
        // Do not sort: binary32 command accumulation observes performer order.
        Some(((ids, *positions), order))
    }

    /// Representatives are interchangeable only for the admitted reduced LUCK
    /// replay, with all formation predicates conservatively scripted. They are
    /// never scored as substitutes for a physical candidate.
    pub(crate) fn masks_for_variants(
        &mut self,
        pool: &Pool,
        physical_ids: [u32; 5],
        positions: &[usize; 5],
    ) -> Result<Option<RushMasks>, Error> {
        self.query();
        let Some((key, order)) = self.key(physical_ids, positions) else {
            self.unavailable(Unavailable::NoVariant);
            #[cfg(feature = "search-diagnostics")]
            self.diag.record(self.site, None, "noVariant");
            return Ok(None);
        };
        self.cache_use.lookups += 1;
        if let Some(masks) = self.cache.get(&key) {
            let masks = masks.clone();
            self.stats.cache_hits += 1;
            self.cache_use.hits += 1;
            if masks.is_none() {
                self.unavailable(Unavailable::Cached);
            }
            #[cfg(feature = "search-diagnostics")]
            {
                let outcome = self.diag.outcomes[&key];
                self.diag.record(self.site, Some(key), outcome);
            }
            return Ok(masks);
        }
        let Some(roots) = self.roots.get(positions).cloned() else {
            self.unavailable(Unavailable::NoRoots);
            #[cfg(feature = "search-diagnostics")]
            self.diag.record(self.site, None, "noRoots");
            return Ok(None);
        };
        #[cfg_attr(not(feature = "search-diagnostics"), allow(unused_variables))]
        let (combined, _, outcome) = self.replay(pool, physical_ids, order, &roots)?;
        // A positions bucket includes every declared root, even when roots collide
        // under member shuffle. One unavailable root invalidates the entire union.
        let combined = match combined {
            Ok(masks) => Some(masks),
            Err(why) => {
                self.unavailable(why);
                None
            }
        };
        let shared = combined.as_ref().and_then(|masks| self.interned.get(masks).cloned());
        let fresh = combined.is_some() && shared.is_none();
        if self.cache.len() >= MAX_CACHE || (fresh && self.interned.len() >= MAX_DISTINCT) {
            self.cache_use.evictions += self.cache.len() as u64;
            self.cache.clear();
            self.interned.clear();
        }
        let value = shared.or_else(|| {
            let masks = Rc::new(combined?);
            self.interned.insert(masks.clone());
            Some(masks)
        });
        self.cache.insert(key, value.clone());
        self.cache_use.peak_entries = self.cache_use.peak_entries.max(self.cache.len());
        #[cfg(feature = "search-diagnostics")]
        {
            let site = self.site;
            let diag = &mut self.diag;
            diag.outcomes.insert(key, outcome.0);
            if outcome.0 != "masked" && diag.declines.len() < 256 && !diag.declines.iter().any(|row| row.0 == key) {
                diag.declines.push((key, outcome.1, 0));
            }
            diag.record(site, Some(key), outcome.0);
        }
        Ok(value)
    }

    /// Replays every root of a positions bucket: the union of all their branches (None when any root declines
    /// or the shapes differ; one unavailable root invalidates the bucket) and their distinct branches.
    fn replay(
        &mut self,
        pool: &Pool,
        physical_ids: [u32; 5],
        order: [usize; 5],
        roots: &[i32],
    ) -> Result<Replayed, Error> {
        let deck = ournotes_sim::pool::Deck {
            members: physical_ids.map(|v| self.representatives[v as usize].0),
            snaps: physical_ids.map(|v| self.representatives[v as usize].1),
            performance_order: order,
        };
        let performers = deck_performers(pool, &deck)?;
        let g = self.setup.gk.as_ref().expect("Gekisou replay setup");
        let mut combined: Option<ournotes_sim::live::full::RushMasks> = None;
        let mut branches: Vec<ournotes_sim::live::full::RushMasks> = Vec::new();
        #[cfg_attr(not(feature = "search-diagnostics"), allow(unused_mut))]
        let mut outcome = ("masked", serde_json::Value::Null);
        for &root in roots {
            self.stats.runs += 1;
            let started = super::budget::now();
            let replayed = rush_branches_why(
                pool.master,
                &self.setup.notes,
                self.setup.params,
                &g.setup,
                &self.setup.play,
                &g.dt,
                &performers,
                root,
                MAX_RUNS,
                self.life,
            )?;
            self.stats.replay_ms += started.elapsed().as_secs_f64() * 1000.0;
            let root_branches = match replayed {
                Ok(list) => list,
                Err(_why) => {
                    #[cfg(feature = "search-diagnostics")]
                    {
                        outcome = (_why.label(), decline_detail(&_why, root, &performers));
                    }
                    return Ok((Err(Unavailable::Declined), Vec::new(), outcome));
                }
            };
            for masks in root_branches {
                match &mut combined {
                    Some(all) => {
                        if !all.union_with(&masks) {
                            #[cfg(feature = "search-diagnostics")]
                            {
                                outcome = ("rootShape", serde_json::Value::Null);
                            }
                            return Ok((Err(Unavailable::RootShape), Vec::new(), outcome));
                        }
                    }
                    None => combined = Some(masks.clone()),
                }
                if branches.len() <= MAX_BRANCHES && !branches.contains(&masks) {
                    branches.push(masks);
                }
            }
        }
        // Every positions bucket holds a root.
        Ok((Ok(combined.expect("a replayed root")), branches, outcome))
    }
}

#[cfg(feature = "search-diagnostics")]
fn decline_detail(
    why: &ournotes_sim::live::full::RushDecline,
    root: i32,
    performers: &[ournotes_sim::live::full::Performer],
) -> serde_json::Value {
    use ournotes_sim::live::full::RushDecline;
    let skills: Vec<_> = performers.iter().map(|p| (p.gekisou_skill, p.gekisou_support_skills.clone())).collect();
    let detail = match why {
        RushDecline::Runs(times) => {
            serde_json::json!({"answers": times.len(), "times": times.iter().take(32).collect::<Vec<_>>()})
        }
        RushDecline::Build(why) | RushDecline::Branch(why) => serde_json::json!(why),
        _ => serde_json::Value::Null,
    };
    serde_json::json!({"reason": why.label(), "root": root, "skills": format!("{skills:?}"), "why": detail})
}
