//! The lottery laws of every team of a domain, read from one formation per multiset of lottery kinds.
//!
//! [`LotteryKinds`] gives each performer its lottery kind, and formations whose performers have the same kind at
//! every position share their law. A team's orders place its performers in every order, so two teams whose
//! performers hold the same multiset of kinds have the same laws over their orders, and the laws of a domain's teams
//! are the laws of one formation per multiset of kinds those teams hold. The census reads a superset of those
//! multisets from the kinds of the domain's members with each Snap choice:
//!
//! - Members with the same kind under every choice form a profile. A team holds members of distinct characters: at
//!   most as many members of a profile as the profile has characters.
//! - Snaps with the same kind on every profile form a type. A team holds each Snap at most once: at most as many
//!   Snaps of a type as the type has. A type with the kinds of no Snap joins the no-Snap choice, open to every member.
//! - Members of a profile are interchangeable, so every team's multiset is reached with the choices of each profile's
//!   members in nondecreasing order.
//!
//! The formation of a multiset takes members of a profile with distinct characters while it has them, and distinct
//! Snaps of a type. Bounds compiled under the laws read ([`JointBounds::compile_with_laws`]) cover every team of the
//! domain; the leaf evaluation checks the laws of each team it evaluates under them ([`JointBounds::cover_laws`]).
//!
//! A domain that reads several laws also splits them into clusters of laws with similar curves, the laws of each
//! multiset of kinds in one cluster, and compiles bounds under each cluster's laws: they cover the teams of its
//! multisets. The domain is then searched one cluster at a time, each traversal under the bounds of its cluster and
//! over the teams of its multisets ([`LawClusters`]).

use super::*;
use crate::domain::CandidateDomain;
use crate::search::certified_search::LuckLaw;
use crate::search::expectation::LiveSetting;
use crate::search::joint::{JointBounds, LawClusters, LawTables, MAX_CLUSTERS, NO_KIND, pack};
use ournotes_sim::live::full::{
    LotteryBreak, LotteryKinds, LotteryLive, LotteryRole, LuckDpCache, LuckSkills, luck_order_laws,
};
use std::collections::HashMap;
use std::sync::Arc;

/// The lottery roles of the members of a domain with each choice: no Snap, or one of the domain's Snaps.
pub(super) struct DomainRoles {
    members: Vec<usize>,
    snaps: Vec<usize>,
    /// `roles[i][c]`: member `members[i]` with choice `c` (0: no Snap, `j + 1`: `snaps[j]`).
    roles: Vec<Vec<LotteryRole>>,
}

impl DomainRoles {
    /// The roles of `domain`'s members and Snaps in `live`, or why they cannot be read.
    fn new(
        pool: &Pool,
        domain: &CandidateDomain,
        kinds: &mut LotteryKinds<'_>,
        live: &LotteryLive<'_>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, String> {
        let mut roles = Vec::with_capacity(domain.members().len());
        for &m in domain.members() {
            if cancelled() {
                return Err("the search stopped".into());
            }
            let member = &pool.members[m];
            let row = std::iter::once(None)
                .chain(domain.snaps().iter().map(|&s| Some(&pool.snaps[s])))
                .map(|snap| Ok(kinds.role(live, &crate::search::snaps::performer(member, snap)?)))
                .collect::<Result<Vec<_>, Error>>()
                .map_err(|error| error.to_string())?;
            roles.push(row);
        }
        Ok(Self { members: domain.members().to_vec(), snaps: domain.snaps().to_vec(), roles })
    }

    /// The Snaps some member takes no kind with, though it takes one alone.
    pub(super) fn breaking_snaps(&self) -> Vec<usize> {
        let alone = |row: &[LotteryRole]| matches!(row[0], LotteryRole::Kind(_));
        (0..self.snaps.len())
            .filter(|&j| self.roles.iter().any(|row| alone(row) && matches!(row[j + 1], LotteryRole::Breaks(_))))
            .map(|j| self.snaps[j])
            .collect()
    }

    /// The members with each choice that take no kind, by the reason.
    fn breaks(&self) -> telemetry::RoleBreaks {
        let mut counts = telemetry::RoleBreaks::default();
        for why in self.roles.iter().flatten().filter_map(|role| match role {
            LotteryRole::Breaks(why) => Some(why),
            LotteryRole::Kind(_) => None,
        }) {
            match why {
                LotteryBreak::Unsupported(text) => {
                    counts.unsupported += 1;
                    counts.first_unsupported.get_or_insert_with(|| text.clone());
                }
                LotteryBreak::Converts => counts.converts += 1,
                LotteryBreak::LeavesLifeRange => counts.leaves_life_range += 1,
                LotteryBreak::ReadsLife => counts.reads_life += 1,
                LotteryBreak::ReadsFormation => counts.reads_formation += 1,
                LotteryBreak::Interacts(text) => {
                    counts.interacts += 1;
                    counts.first_interacts.get_or_insert_with(|| text.clone());
                }
            }
        }
        counts
    }
}

/// A multiset of kinds: the (profile, Snap type) of each of its five performers (type 0: no Snap).
type Slots = [(usize, usize); 5];

/// A formation: the member and the Snap (None: no Snap) of each performer, as indexes of the census's lists.
type Formation = [(usize, Option<usize>); 5];

/// The multisets of kinds of a census, each with a formation holding it, and the profiles and Snap types read.
struct KindClasses {
    formations: Vec<Formation>,
    profiles: usize,
    snap_types: usize,
}

/// The multisets of kinds the teams of a domain can hold (a superset; see the module documentation): `kinds[i][c]`
/// is the kind of member `i`, of character `characters[i]`, with choice `c` (0: no Snap, `j + 1`: Snap `j`). None
/// once `cancelled`.
fn kind_classes<K: Copy + Ord + std::hash::Hash>(
    characters: &[i64],
    kinds: &[Vec<K>],
    cancelled: &mut dyn FnMut() -> bool,
) -> Option<KindClasses> {
    let choices = kinds.first().map_or(1, Vec::len);
    // Profiles, by the kinds of a member under every choice; members of distinct characters first.
    let mut profile_of = HashMap::<&[K], usize>::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (i, row) in kinds.iter().enumerate() {
        let next = members.len();
        let p = *profile_of.entry(row).or_insert(next);
        if p == next {
            members.push(Vec::new());
        }
        members[p].push(i);
    }
    let mut caps = Vec::with_capacity(members.len());
    for list in &mut members {
        let mut seen = HashSet::new();
        let (first, rest): (Vec<usize>, Vec<usize>) = list.iter().partition(|&&i| seen.insert(characters[i]));
        caps.push(first.len().min(5));
        *list = first.into_iter().chain(rest).collect();
    }
    // Snap types, by the kinds of every profile with the Snap; type 0 holds no Snap.
    let none: Vec<K> = members.iter().map(|list| kinds[list[0]][0]).collect();
    let mut type_of = HashMap::<Vec<K>, usize>::new();
    let mut snaps: Vec<Vec<usize>> = vec![Vec::new()];
    let mut table: Vec<Vec<K>> = none.iter().map(|&kind| vec![kind]).collect();
    for j in 0..choices - 1 {
        let column: Vec<K> = members.iter().map(|list| kinds[list[0]][j + 1]).collect();
        if column == none {
            continue;
        }
        let next = snaps.len();
        let t = *type_of.entry(column.clone()).or_insert(next);
        if t == next {
            snaps.push(Vec::new());
            for (row, &kind) in table.iter_mut().zip(&column) {
                row.push(kind);
            }
        }
        snaps[t].push(j);
    }
    let mut walk = Classes {
        kinds: table,
        caps,
        left: snaps.iter().map(Vec::len).collect(),
        slots: Vec::with_capacity(5),
        seen: HashSet::new(),
        found: Vec::new(),
        visits: 0,
    };
    if !walk.walk(0, 0, 0, cancelled) {
        return None;
    }
    let formations = walk
        .found
        .iter()
        .map(|slots| {
            let (mut taken, mut worn) = (vec![0; members.len()], vec![0; snaps.len()]);
            slots.map(|(p, t)| {
                taken[p] += 1;
                let snap = (t != 0).then(|| {
                    worn[t] += 1;
                    snaps[t][worn[t] - 1]
                });
                (members[p][taken[p] - 1], snap)
            })
        })
        .collect();
    Some(KindClasses { formations, profiles: members.len(), snap_types: snaps.len() - 1 })
}

/// The walk of [`kind_classes`] over the profiles and Snap types.
struct Classes<K> {
    /// `kinds[p][t]`: the kind of a member of profile `p` with a choice of type `t`.
    kinds: Vec<Vec<K>>,
    /// The most members of each profile one team holds, and the Snaps of each type left (type 0 unbounded).
    caps: Vec<usize>,
    left: Vec<usize>,
    slots: Vec<(usize, usize)>,
    seen: HashSet<[K; 5]>,
    found: Vec<Slots>,
    visits: u64,
}

impl<K: Copy + Ord + std::hash::Hash> Classes<K> {
    /// Fills the slots from profile `p` on, the next member of `p` taking a type from `from` on. False once
    /// `cancelled`.
    fn walk(&mut self, p: usize, taken: usize, from: usize, cancelled: &mut dyn FnMut() -> bool) -> bool {
        self.visits += 1;
        if self.visits.is_multiple_of(4096) && cancelled() {
            return false;
        }
        if self.slots.len() == 5 {
            let slots: Slots = std::array::from_fn(|k| self.slots[k]);
            let mut class = slots.map(|(p, t)| self.kinds[p][t]);
            class.sort_unstable();
            if self.seen.insert(class) {
                self.found.push(slots);
            }
            return true;
        }
        if p == self.kinds.len() {
            return true;
        }
        if taken < self.caps[p] {
            for t in from..self.left.len() {
                if t != 0 && self.left[t] == 0 {
                    continue;
                }
                self.left[t] -= usize::from(t != 0);
                self.slots.push((p, t));
                let more = self.walk(p, taken + 1, t, cancelled);
                self.slots.pop();
                self.left[t] += usize::from(t != 0);
                if !more {
                    return false;
                }
            }
        }
        self.walk(p + 1, 0, 0, cancelled)
    }
}

/// The census of the lottery laws of the plan's domain and its parts.
pub(super) struct LotteryCensus {
    live: LiveSetting,
    skills: Arc<LuckSkills>,
    pub(super) roles: DomainRoles,
    read: ReadLaws,
}

/// The laws the censuses of a plan read. A formation's orders place its performers in every order, so the multiset
/// of its kinds gives its laws.
#[derive(Default)]
struct ReadLaws {
    /// The laws read, distinct by address.
    laws: Vec<LuckLaw>,
    /// The index in `laws` of each law, by its address.
    index: ournotes_sim::num::FxHashMap<usize, u32>,
    /// The indices of the laws of each multiset of kinds (sorted kind indices), or None when its kinds compose none.
    multisets: ournotes_sim::num::FxHashMap<[usize; 5], Option<Box<[u32]>>>,
    /// The luck weights of the bounds last compiled under the laws of a census.
    weights: crate::search::snaps::WeightsMemo,
}

impl ReadLaws {
    /// The index of `law`, held from now on.
    fn hold(laws: &mut Vec<LuckLaw>, index: &mut ournotes_sim::num::FxHashMap<usize, u32>, law: LuckLaw) -> u32 {
        *index.entry(Arc::as_ptr(&law) as usize).or_insert_with(|| {
            laws.push(law);
            (laws.len() - 1) as u32
        })
    }
}

impl LotteryCensus {
    /// The kind index of pool member `member` with pool Snap `snap` (None: no Snap), when the census's roles hold it
    /// and it takes a kind.
    pub(super) fn kind_index(&self, member: usize, snap: Option<usize>) -> Option<usize> {
        let roles = &self.roles;
        let row = roles.members.iter().position(|&m| m == member)?;
        let column = match snap {
            None => 0,
            Some(s) => 1 + roles.snaps.iter().position(|&r| r == s)?,
        };
        match &roles.roles[row][column] {
            LotteryRole::Kind(kind) => Some(kind.index()),
            LotteryRole::Breaks(_) => None,
        }
    }

    /// The laws of the orders of team `p`, read from the multiset of its performers' kinds: two teams whose performers
    /// hold the same multiset of kinds have the same laws over their orders, and the census holds the laws of every
    /// order of one formation of each multiset it read. None when a performer of `p` is outside the census's roles or
    /// takes no kind, or when the census read no laws composed from the multiset.
    pub(super) fn team_laws(&self, p: &PhysicalDeck) -> Option<Vec<LuckLaw>> {
        let mut multiset = [0usize; 5];
        for (slot, kind) in multiset.iter_mut().enumerate() {
            *kind = self.kind_index(p.members[slot], p.snaps[slot])?;
        }
        multiset.sort_unstable();
        self.multiset_laws(&multiset)
    }

    /// The laws the census read for the multiset of sorted kind indices `multiset`, distinct by address. None when it
    /// read none composed from it.
    fn multiset_laws(&self, multiset: &[usize; 5]) -> Option<Vec<LuckLaw>> {
        let mut ids = self.read.multisets.get(multiset)?.as_ref()?.to_vec();
        ids.sort_unstable();
        ids.dedup();
        Some(ids.into_iter().map(|i| self.read.laws[i as usize].clone()).collect())
    }
}

/// The live of the lottery recordings of `live`; None without a Gekisou range.
fn lottery_live<'a>(
    master: &'a ournotes_sim::master::Master,
    skills: &'a LuckSkills,
    live: &'a LiveSetting,
) -> Option<LotteryLive<'a>> {
    Some(LotteryLive {
        master,
        skills,
        notes: &live.notes,
        events: &live.events,
        params: live.params,
        setup: live.gekisou.as_ref()?,
        play: &live.play,
        delta_times: &live.delta_times,
        ranking: live.rank_confirmations.as_deref(),
    })
}

/// A multiset of kinds read (sorted kind indices) with the indices of its laws in the census.
type Multiset = ([usize; 5], Box<[u32]>);

/// The laws of the teams of `domain`, whose members and Snaps the census's roles hold, distinct by address, with every
/// multiset of kinds read; or why the census reads none.
#[allow(clippy::too_many_arguments)]
fn domain_laws(
    pool: &Pool,
    census: &mut LotteryCensus,
    domain: &CandidateDomain,
    curves: &mut LuckDpCache,
    lottery: &mut LotteryKinds<'_>,
    stats: &mut telemetry::DomainLaws,
    mut cancelled: impl FnMut() -> bool,
) -> Result<(Vec<LuckLaw>, Vec<Multiset>), String> {
    let LotteryCensus { live: setting, skills, roles, read } = census;
    let mut multisets: Vec<Multiset> = Vec::new();
    let live = lottery_live(pool.master, skills, setting).ok_or("the live has no Gekisou range")?;
    let column = |s: usize| roles.snaps.iter().position(|&r| r == s).map(|j| j + 1);
    let columns: Vec<usize> = std::iter::once(Some(0))
        .chain(domain.snaps().iter().map(|&s| column(s)))
        .collect::<Option<_>>()
        .ok_or("a Snap outside the census")?;
    let mut kinds = Vec::with_capacity(domain.members().len());
    for &m in domain.members() {
        let i = roles.members.iter().position(|&r| r == m).ok_or("a member outside the census")?;
        let row = columns
            .iter()
            .map(|&c| match &roles.roles[i][c] {
                LotteryRole::Kind(kind) => Ok(*kind),
                LotteryRole::Breaks(why) => Err(format!(
                    "member {} with {}: {why:?}",
                    pool.members[m].id,
                    c.checked_sub(1).map_or("no Snap".into(), |j| format!("Snap {}", pool.snaps[roles.snaps[j]].id)),
                )),
            })
            .collect::<Result<Vec<_>, String>>()?;
        kinds.push(row);
    }
    let characters: Vec<i64> = domain.members().iter().map(|&m| pool.members[m].character_id).collect();
    let started = Instant::now();
    let found = kind_classes(&characters, &kinds, &mut cancelled).ok_or("the search stopped")?;
    stats.classes_ms += started.elapsed().as_secs_f64() * 1000.0;
    (stats.profiles, stats.snap_types, stats.classes) = (found.profiles, found.snap_types, found.formations.len());
    stats.formations += found.formations.len();
    let orders = crate::search::uniform::all_orders();
    // Whether the domain's teams hold each law read.
    let mut held: Vec<bool> = Vec::new();
    let mark = |held: &mut Vec<bool>, i: u32| {
        let i = i as usize;
        if held.len() <= i {
            held.resize(i + 1, false);
        }
        held[i] = true;
    };
    // Formations whose kinds compose no law, read by their performers once the census is done.
    let mut separate = Vec::new();
    let mut census = lottery.census(&live).map_err(|error| error.to_string())?;
    for formation in &found.formations {
        if cancelled() {
            return Err("the search stopped".into());
        }
        let formation_kinds: [_; 5] = std::array::from_fn(|k| {
            let (i, j) = formation[k];
            kinds[i][j.map_or(0, |j| j + 1)]
        });
        let mut multiset = formation_kinds.map(|kind| kind.index());
        multiset.sort_unstable();
        let ids = match read.multisets.entry(multiset) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let found =
                    census.formation_laws(formation_kinds, &orders, curves).map_err(|error| error.to_string())?;
                let ids = found.map(|laws| {
                    laws.into_iter().map(|law| ReadLaws::hold(&mut read.laws, &mut read.index, law)).collect()
                });
                entry.insert(ids)
            }
        };
        match ids {
            Some(ids) => {
                ids.iter().for_each(|&i| mark(&mut held, i));
                multisets.push((multiset, ids.clone()));
            }
            None => separate.push(*formation),
        }
    }
    for formation in separate {
        if cancelled() {
            return Err("the search stopped".into());
        }
        let mut multiset: [usize; 5] = std::array::from_fn(|k| {
            let (i, j) = formation[k];
            kinds[i][j.map_or(0, |j| j + 1)].index()
        });
        multiset.sort_unstable();
        let started = Instant::now();
        let deck = formation
            .iter()
            .map(|&(i, j)| {
                let snap = j.map(|j| &pool.snaps[domain.snaps()[j]]);
                crate::search::snaps::performer(&pool.members[domain.members()[i]], snap)
            })
            .collect::<Result<Vec<_>, Error>>()
            .map_err(|error| error.to_string())?;
        stats.performers_ms += started.elapsed().as_secs_f64() * 1000.0;
        let mut group = Vec::new();
        for law in luck_order_laws(&live, &deck, &orders, curves, Some(&mut *lottery)).map_err(|e| e.to_string())? {
            let i = ReadLaws::hold(&mut read.laws, &mut read.index, law);
            mark(&mut held, i);
            group.push(i);
        }
        multisets.push((multiset, group.into()));
    }
    let laws: Vec<LuckLaw> =
        held.iter().zip(&read.laws).filter(|&(&held, _)| held).map(|(_, law)| law.clone()).collect();
    stats.laws = laws.len();
    Ok((laws, multisets))
}

/// The laws of a domain split into at most this many clusters (see [`cluster_laws`]).
const LAW_CLUSTERS: usize = 8;

/// Points of the time grid on which [`cluster_laws`] compares the curves of the laws.
const CURVE_POINTS: i64 = 64;

/// The laws of the multisets of kinds read in at most `k` clusters of laws with similar Rush and probe curves: the law
/// set of each cluster and the cluster of each multiset. Each multiset goes to the cluster of its first law's curve with
/// all its laws, so bounds compiled under a cluster's laws cover every team holding one of its multisets. None when a
/// multiset reads no law.
///
/// The curves are compared by their luck weights (Rush factor and probe weight) at a single note on a grid over the
/// laws' times, and grouped by k-means from the farthest-point seeding.
fn cluster_laws(multisets: &[Multiset], laws: &[LuckLaw], k: usize) -> Option<(Vec<Vec<LuckLaw>>, Vec<usize>)> {
    let key = |law: &LuckLaw| (law.steps.as_ptr() as usize, law.frame_queries.as_ptr() as usize);
    let held = |law: &LuckLaw| law.probes.iter().any(|&p| p);
    // one law per curve, one that holds a probe when some law of the curve does
    let mut index = HashMap::<(usize, usize), usize>::new();
    let mut curves: Vec<LuckLaw> = Vec::new();
    for (_, ids) in multisets {
        for &i in ids.iter() {
            let law = &laws[i as usize];
            match index.get(&key(law)) {
                Some(&c) => {
                    if held(law) && !held(&curves[c]) {
                        curves[c] = law.clone();
                    }
                }
                None => {
                    index.insert(key(law), curves.len());
                    curves.push(law.clone());
                }
            }
        }
    }
    let n = curves.len();
    let k = k.clamp(1, n.max(1));
    let mut label = vec![0usize; n];
    if k > 1 {
        let times =
            curves.iter().flat_map(|law| law.steps.iter().map(|s| s.0).chain(law.frame_queries.iter().map(|q| q.0)));
        let (lo, hi) = times.fold((i32::MAX, i32::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
        let (lo, hi) = if lo > hi { (0, 1) } else { (lo, hi.max(lo + 1)) };
        let span = hi as i64 - lo as i64;
        let grid: Vec<(i32, f64)> =
            (0..CURVE_POINTS).map(|i| ((lo as i64 + span * i / (CURVE_POINTS - 1)) as i32, 2.0)).collect();
        let features: Vec<Vec<f64>> = curves
            .iter()
            .map(|law| {
                let one = [law.clone()];
                let weights = crate::search::snaps::LuckLaws::new(&one).expect("one law").weights(&grid);
                let probe = held(law);
                let rush = weights.iter().map(|w| w.0 - 1.0);
                rush.chain(weights.iter().map(|w| if probe { w.1 } else { 0.0 })).collect()
            })
            .collect();
        let d2 = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>();
        let first = (0..n)
            .max_by(|&a, &b| features[a].iter().sum::<f64>().total_cmp(&features[b].iter().sum::<f64>()))
            .expect("a curve");
        let mut centers = vec![features[first].clone()];
        let mut dist = vec![f64::INFINITY; n];
        while centers.len() < k {
            let last = centers.last().expect("a center");
            for i in 0..n {
                dist[i] = dist[i].min(d2(&features[i], last));
            }
            let far = (0..n).max_by(|&a, &b| dist[a].total_cmp(&dist[b])).expect("a curve");
            centers.push(features[far].clone());
        }
        for _ in 0..30 {
            let mut moved = false;
            for i in 0..n {
                let best = (0..k)
                    .min_by(|&a, &b| d2(&features[i], &centers[a]).total_cmp(&d2(&features[i], &centers[b])))
                    .expect("a center");
                moved |= best != label[i];
                label[i] = best;
            }
            for (c, center) in centers.iter_mut().enumerate() {
                let members: Vec<usize> = (0..n).filter(|&i| label[i] == c).collect();
                if members.is_empty() {
                    continue;
                }
                for (d, value) in center.iter_mut().enumerate() {
                    *value = members.iter().map(|&i| features[i][d]).sum::<f64>() / members.len() as f64;
                }
            }
            if !moved {
                break;
            }
        }
    }
    let mut taken = vec![vec![false; laws.len()]; k];
    let mut sets: Vec<Vec<LuckLaw>> = vec![Vec::new(); k];
    let mut cluster: Vec<usize> = Vec::with_capacity(multisets.len());
    for (_, ids) in multisets {
        let c = label[index[&key(&laws[*ids.first()? as usize])]];
        for &i in ids.iter() {
            if !std::mem::replace(&mut taken[c][i as usize], true) {
                sets[c].push(laws[i as usize].clone());
            }
        }
        cluster.push(c);
    }
    // the clusters without a multiset are dropped
    let mut renumber = vec![0; k];
    let mut kept = Vec::new();
    for (c, set) in sets.into_iter().enumerate() {
        if !set.is_empty() {
            renumber[c] = kept.len();
            kept.push(set);
        }
    }
    Some((kept, cluster.into_iter().map(|c| renumber[c]).collect()))
}

/// The places of the multisets read among their clusters (`cluster`: the cluster of each multiset): the clusters
/// (bits) of the multisets that contain each sorted multiset of one to four kinds, and the cluster each multiset
/// belongs to (its first), by their packed keys. None when a kind index does not pack.
fn multiset_places(multisets: &[Multiset], cluster: &[usize]) -> Option<(HashMap<u64, u64>, HashMap<u64, u8>)> {
    let mut sets = HashMap::<u64, u64>::new();
    let mut teams = HashMap::<u64, u8>::new();
    let mut sub = Vec::with_capacity(5);
    for ((multiset, _), &c) in multisets.iter().zip(cluster) {
        // every nonempty subset of the five positions, the whole multiset last
        for pick in 1u32..32 {
            sub.clear();
            sub.extend((0..5).filter(|&i| pick >> i & 1 != 0).map(|i| multiset[i]));
            let key = pack(&sub)?;
            if pick == 31 {
                teams.entry(key).or_insert(c as u8);
            } else {
                *sets.entry(key).or_default() |= 1 << c;
            }
        }
    }
    Some((sets, teams))
}

impl Engine<'_, '_> {
    /// The lottery roles of the plan's domain when its bounds can weigh lottery laws: a certified score search of a
    /// Gekisou live in its declared clocks. None otherwise, or when the roles cannot be read (the reason then in the
    /// telemetry).
    pub(super) fn lottery_census(&mut self, domain: &CandidateDomain) -> Option<LotteryCensus> {
        if self.certified.is_none()
            || !matches!(self.metric, Metric::Score)
            || self.simulation.music_length_ms.is_some()
            || self.simulation.score_music_length_ms.is_some()
        {
            return None;
        }
        let live = crate::search::expectation::live_setting(self.pool, &self.request.objective).ok()?;
        live.gekisou.as_ref()?;
        let started = Instant::now();
        let mut stats = telemetry::DomainLaws::default();
        let roles = match self.certified_luck_skills() {
            Ok(skills) => {
                let pool = self.pool;
                let mut kinds = self.take_luck_kinds();
                let lottery = lottery_live(pool.master, &skills, &live).expect("a live with a Gekisou range");
                let roles = DomainRoles::new(pool, domain, &mut kinds, &lottery, || self.expired());
                stats.kinds = kinds.len();
                self.keep_luck_kinds(kinds);
                roles.map(|roles| (skills.clone(), roles))
            }
            Err(error) => Err(error.to_string()),
        };
        stats.roles_ms = started.elapsed().as_secs_f64() * 1000.0;
        let census = match roles {
            Ok((skills, roles)) => {
                stats.separated_snaps = roles.breaking_snaps().len();
                stats.breaks = roles.breaks();
                Some(LotteryCensus { live, skills, roles, read: ReadLaws::default() })
            }
            Err(why) => {
                stats.unavailable = Some(why);
                None
            }
        };
        self.tel.environment.bounds.domain_laws = Some(stats);
        census
    }

    /// The joint bounds of `domain` weighing the lottery laws of its teams, or None when the census reads none or
    /// they do not compile (the reason then in the telemetry).
    pub(super) fn weighed_bounds(
        &mut self,
        census: &mut LotteryCensus,
        domain: &CandidateDomain,
    ) -> Option<JointBounds> {
        let pool = self.pool;
        let mut kinds = self.take_luck_kinds();
        let mut curves = std::mem::take(&mut self.certified.as_mut().expect("certified request").luck_curves);
        let mut stats = self.tel.environment.bounds.domain_laws.take().unwrap_or_default();
        stats.censuses += 1;
        let started = Instant::now();
        let laws = domain_laws(pool, census, domain, &mut curves, &mut kinds, &mut stats, || self.expired());
        stats.laws_ms += started.elapsed().as_secs_f64() * 1000.0;
        self.tel.caches.luck_curves.record(curves.stats());
        self.keep_luck_kinds(kinds);
        self.certified.as_mut().expect("certified request").luck_curves = curves;
        let started = Instant::now();
        let memo = Some(&census.read.weights);
        let (laws, multisets) = match laws {
            Ok((laws, multisets)) => (Ok(laws), multisets),
            Err(error) => (Err(error), Vec::new()),
        };
        let mut bounds = laws.as_ref().map_err(Clone::clone).and_then(|laws| {
            let (request, metric, event_input, simulation) =
                (self.request, self.metric, self.event_input, self.simulation);
            JointBounds::compile_with_laws(pool, request, domain, metric, event_input, simulation, laws, memo)
                .map_err(|error| format!("the weighed bounds do not compile: {error}"))
        });
        stats.compile_ms += started.elapsed().as_secs_f64() * 1000.0;
        stats.law_clusters = 0;
        if let Ok(bounds) = &mut bounds {
            let started = Instant::now();
            if let Some(clusters) = self.law_clusters(census, domain, &multisets) {
                stats.law_clusters = clusters.bounds.len();
                bounds.set_law_clusters(clusters);
            }
            stats.clusters_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        stats.unavailable = bounds.as_ref().err().cloned();
        stats.weighed += usize::from(bounds.is_ok());
        self.tel.environment.bounds.domain_laws = Some(stats);
        bounds.ok()
    }

    /// The bounds of `domain` under clusters of the laws of the multisets of kinds `multisets` read by `census` (see
    /// [`cluster_laws`]), or None when they form one cluster or the bounds of a cluster do not compile.
    fn law_clusters(
        &self,
        census: &LotteryCensus,
        domain: &CandidateDomain,
        multisets: &[Multiset],
    ) -> Option<LawClusters> {
        let (sets, cluster) = cluster_laws(multisets, &census.read.laws, LAW_CLUSTERS.min(MAX_CLUSTERS))?;
        if sets.len() < 2 {
            return None;
        }
        let (prefixes, teams) = multiset_places(multisets, &cluster)?;
        let (pool, request, metric, event_input, simulation) =
            (self.pool, self.request, self.metric, self.event_input, self.simulation);
        let bounds = sets
            .iter()
            .map(|set| {
                JointBounds::compile_with_laws(pool, request, domain, metric, event_input, simulation, set, None)
            })
            .collect::<Result<Vec<_>, Error>>()
            .ok()?;
        let mut kinds = vec![Box::<[u32]>::default(); pool.members.len()];
        for &m in domain.members() {
            let choices = std::iter::once(None).chain(domain.snaps().iter().map(|&s| Some(s)));
            kinds[m] = choices
                .map(|snap| census.kind_index(m, snap).and_then(|k| u32::try_from(k).ok()).unwrap_or(NO_KIND))
                .collect();
        }
        Some(LawClusters::new(bounds, LawTables::new(kinds, prefixes, teams)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The sorted kinds of every team: five members of distinct characters, each with no Snap or a Snap no other
    /// member holds.
    fn team_classes(characters: &[i64], kinds: &[Vec<u8>]) -> BTreeSet<[u8; 5]> {
        fn teams(
            from: usize,
            team: &mut Vec<(usize, usize)>,
            characters: &[i64],
            kinds: &[Vec<u8>],
            out: &mut BTreeSet<[u8; 5]>,
        ) {
            if team.len() == 5 {
                let mut class: [u8; 5] = std::array::from_fn(|k| kinds[team[k].0][team[k].1]);
                class.sort_unstable();
                out.insert(class);
                return;
            }
            for i in from..kinds.len() {
                if team.iter().any(|&(other, _)| characters[other] == characters[i]) {
                    continue;
                }
                for c in 0..kinds[i].len() {
                    if c != 0 && team.iter().any(|&(_, other)| other == c) {
                        continue;
                    }
                    team.push((i, c));
                    teams(i + 1, team, characters, kinds, out);
                    team.pop();
                }
            }
        }
        let mut out = BTreeSet::new();
        teams(0, &mut Vec::new(), characters, kinds, &mut out);
        out
    }

    #[test]
    fn kind_classes_hold_every_team_multiset_once_with_a_formation_of_it() {
        let mut seed = 11u64;
        let mut next = |below: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % below
        };
        let mut exact = 0;
        for _ in 0..200 {
            let members = 5 + next(5) as usize;
            let snaps = next(5) as usize;
            let characters: Vec<i64> = (0..members).map(|_| next(7) as i64).collect();
            // Kinds from a member group and a Snap group, so that profiles and Snap types gather several of each.
            let groups: Vec<u64> = (0..members).map(|_| next(3)).collect();
            let snap_groups: Vec<u64> = std::iter::once(0).chain((0..snaps).map(|_| next(3))).collect();
            let table: Vec<Vec<u8>> = (0..3).map(|_| (0..3).map(|_| next(4) as u8).collect()).collect();
            let kinds: Vec<Vec<u8>> = (0..members)
                .map(|i| snap_groups.iter().map(|&g| table[groups[i] as usize][g as usize]).collect())
                .collect();
            let teams = team_classes(&characters, &kinds);
            let found = kind_classes(&characters, &kinds, &mut || false).unwrap();
            let mut classes = BTreeSet::new();
            for formation in &found.formations {
                let mut class = formation.map(|(i, c)| kinds[i][c.map_or(0, |j| j + 1)]);
                class.sort_unstable();
                assert!(classes.insert(class), "{class:?} read twice");
                let snaps: Vec<usize> = formation.iter().filter_map(|&(_, j)| j).collect();
                assert_eq!(snaps.iter().collect::<BTreeSet<_>>().len(), snaps.len(), "{formation:?}");
            }
            assert!(teams.is_subset(&classes), "{characters:?} {kinds:?}");
            exact += usize::from(teams == classes);
        }
        assert!(exact > 100, "{exact}");
    }

    #[test]
    fn kind_classes_stop_when_cancelled() {
        let kinds: Vec<Vec<u8>> = (0..12).map(|i| (0..9).map(|c| (i * 9 + c) as u8).collect()).collect();
        let characters: Vec<i64> = (0..12).collect();
        assert!(kind_classes(&characters, &kinds, &mut || true).is_none());
    }
}
