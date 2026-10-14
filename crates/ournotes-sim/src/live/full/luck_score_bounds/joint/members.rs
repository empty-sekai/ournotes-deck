//! The groups of a joint evaluation and the factor histories they hold: how groups with equal heads merge, and how a
//! group past its member cap merges its members into hulls of their states.

use super::super::exact_paths::{FactorState, Track};
use super::*;

/// What sets a group apart: the track ahead of its lottery paths and, per pending note, the lottery class it read, or
/// [`UNREAD`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Head {
    pub(super) track: Track,
    pub(super) classes: Vec<u8>,
}

/// One factor history of a group: its state bounds and, per pending note, the value of its last execution so far.
#[derive(Clone, Debug)]
pub(super) struct Member {
    pub(super) low: FactorState,
    pub(super) high: FactorState,
    pub(super) values: Vec<Option<NoteValue>>,
}

impl Member {
    /// The history of one factor state, before any note.
    pub(super) fn at(state: FactorState) -> Self {
        Self { low: state, high: state, values: Vec::new() }
    }

    /// Words equal for equal members, into `out`.
    fn words(&self, out: &mut Vec<u32>) {
        out.clear();
        out.extend(self.low.iter().chain(&self.high).map(|value| value.to_bits()));
        for value in &self.values {
            match value {
                None => out.push(u32::MAX),
                Some(value) => {
                    out.extend([u32::from(value.class), value.combo.0, value.combo.1]);
                    out.extend(value.low.iter().chain(&value.high).map(|field| field.to_bits()));
                }
            }
        }
    }

    /// Whether the two members' pending notes have values of the same probe class and combo inputs, so that their
    /// hull scores each note as the scores of both enclose it.
    fn joins(&self, other: &Member) -> bool {
        self.values.iter().zip(&other.values).all(|pair| match pair {
            (None, None) => true,
            (Some(a), Some(b)) => (a.class, a.combo) == (b.class, b.combo),
            _ => false,
        })
    }

    /// The hull of the two members.
    fn join(&mut self, other: &Member) {
        widen(&mut self.low, &mut self.high, &other.low, &other.high);
        for (value, other) in self.values.iter_mut().zip(&other.values) {
            if let (Some(value), Some(other)) = (value, other) {
                widen(&mut value.low, &mut value.high, &other.low, &other.high);
            }
        }
    }

    /// The greatest distance in binary32 steps between the two members' state bounds, field by field.
    fn distance(&self, other: &Member) -> u64 {
        let pairs = self.low.iter().zip(&other.low).chain(self.high.iter().zip(&other.high));
        pairs.map(|(&a, &b)| u64::from(steps(a).abs_diff(steps(b)))).max().unwrap_or(0)
    }
}

/// The place of `value` among the binary32 values in their order, so that neighbours are one step apart.
fn steps(value: f32) -> u32 {
    let bits = value.to_bits();
    if bits >> 31 == 1 { !bits } else { bits | 1 << 31 }
}

/// Widens `low..=high` to cover `other_low..=other_high`, field by field.
fn widen(low: &mut FactorState, high: &mut FactorState, other_low: &FactorState, other_high: &FactorState) {
    for field in 0..low.len() {
        low[field] = low[field].min(other_low[field]);
        high[field] = high[field].max(other_high[field]);
    }
}

/// The lottery paths of one head and their factor histories.
pub(super) struct Group {
    pub(super) head: Head,
    pub(super) members: Vec<Member>,
}

/// New groups made of old ones: groups with the same head merge, and so do their equal members.
#[derive(Default)]
pub(super) struct Regroup {
    groups: Vec<Group>,
    heads: FxHashMap<Head, u32>,
    /// Per new group, the old groups moved into it with the index of their member map, and once a second one comes,
    /// its members by their words.
    sources: Vec<Vec<(u32, u32)>>,
    known: Vec<Option<FxHashMap<Vec<u32>, u32>>>,
    maps: Vec<Vec<u32>>,
    words: Vec<u32>,
}

impl Regroup {
    /// Moves the members of old group `old`, `members()`, under `head`. Returns the new group and the index of the map
    /// of the members to its members, `u32::MAX` when they keep their places; with `dedupe`, equal members of the group
    /// merge as well.
    pub(super) fn add(
        &mut self,
        old: u32,
        head: Head,
        members: impl FnOnce() -> Vec<Member>,
        dedupe: bool,
    ) -> (u32, u32) {
        let Self { groups, heads, sources, known, maps, words } = self;
        let index = match heads.get(&head) {
            Some(&index) => {
                if let Some(&(_, map)) = sources[index as usize].iter().find(|(source, _)| *source == old) {
                    return (index, map);
                }
                index
            }
            None if !dedupe => {
                let index = groups.len() as u32;
                heads.insert(head.clone(), index);
                groups.push(Group { head, members: members() });
                sources.push(vec![(old, u32::MAX)]);
                known.push(None);
                return (index, u32::MAX);
            }
            None => {
                let index = groups.len() as u32;
                heads.insert(head.clone(), index);
                groups.push(Group { head, members: Vec::new() });
                sources.push(Vec::new());
                known.push(Some(FxHashMap::default()));
                index
            }
        };
        let group = &mut groups[index as usize];
        let known = known[index as usize].get_or_insert_with(|| {
            let mut known = FxHashMap::default();
            for (at, member) in group.members.iter().enumerate() {
                member.words(words);
                known.entry(words.clone()).or_insert(at as u32);
            }
            known
        });
        let (mut map, mut moved) = (Vec::new(), true);
        for member in members() {
            member.words(words);
            let at = match known.get(words) {
                Some(&at) => at,
                None => {
                    let at = group.members.len() as u32;
                    known.insert(words.clone(), at);
                    group.members.push(member);
                    at
                }
            };
            moved &= at as usize == map.len();
            map.push(at);
        }
        let map = if moved && sources[index as usize].is_empty() {
            u32::MAX
        } else {
            maps.push(map);
            maps.len() as u32 - 1
        };
        sources[index as usize].push((old, map));
        (index, map)
    }

    /// The new groups, their numbers of members and the member maps.
    pub(super) fn finish(self) -> (Vec<Group>, Vec<u32>, Vec<Vec<u32>>) {
        let widths = self.groups.iter().map(|group| group.members.len() as u32).collect();
        (self.groups, widths, self.maps)
    }
}

/// The cost of merging two members of masses `a` and `b` whose states are `distance` binary32 steps apart: the hull
/// widens the scores of their later notes on their mass, the more often the farther apart they are. The distance
/// enters by its bit length, so the cost is the same on every platform.
fn merge_cost(a: f64, b: f64, distance: u64) -> f64 {
    (a + b) * f64::from(1 + u64::BITS - distance.leading_zeros())
}

/// `members` of masses `masses` without those of no possible mass, which hold no lottery path, and merged down to
/// `keep` ([`merge_lightest`]) when more than twice as many remain. Returns the members and the place of each old one
/// among them; a member of no possible mass takes the first place, which receives none of its masses.
pub(super) fn trim_members(members: Vec<Member>, masses: &[ProbabilityMass], keep: usize) -> (Vec<Member>, Vec<u32>) {
    let count = members.len();
    let mut merged: Vec<Merged> = members
        .into_iter()
        .zip(masses)
        .enumerate()
        .map(|(at, (member, mass))| possible(mass).then(|| (member, mass.interval().upper(), vec![at])))
        .collect();
    let live = merged.iter().flatten().count();
    if live > 2 * keep {
        merge_lightest(&mut merged, live, keep);
    }
    let mut map = vec![0u32; count];
    let mut out = Vec::with_capacity(keep);
    for (member, _, old) in merged.into_iter().flatten() {
        for at in old {
            map[at] = out.len() as u32;
        }
        out.push(member);
    }
    (out, map)
}

/// A member, its mass and the old members in it; None once merged into another.
type Merged = Option<(Member, f64, Vec<usize>)>;

/// Merges the `live` members of `merged` down to `keep`: repeatedly the lightest member with the partner of least
/// [`merge_cost`] whose hull scores their pending notes. A member without such a partner stays as it is.
fn merge_lightest(merged: &mut [Merged], mut live: usize, keep: usize) {
    let count = merged.len();
    let mut alone = vec![false; count];
    while live > keep {
        let lightest = (0..count)
            .filter(|&at| !alone[at])
            .filter_map(|at| merged[at].as_ref().map(|(_, mass, _)| (at, *mass)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((a, mass)) = lightest else { break };
        let (member, _, _) = merged[a].as_ref().expect("a live member");
        let partner = (0..count)
            .filter(|&b| b != a)
            .filter_map(|b| {
                let (other, other_mass, _) = merged[b].as_ref()?;
                member.joins(other).then(|| (b, merge_cost(mass, *other_mass, member.distance(other))))
            })
            .min_by(|x, y| x.1.total_cmp(&y.1));
        let Some((b, _)) = partner else {
            alone[a] = true;
            continue;
        };
        let (member, mass, old) = merged[a].take().expect("a live member");
        let into = merged[b].as_mut().expect("a live partner");
        into.0.join(&member);
        into.1 += mass;
        into.2.extend(old);
        live -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A history at state `x` in every field, with one pending note of probe class `class` at the same state.
    fn member(x: f32, class: u8) -> Member {
        let state = [x; FIELDS];
        let value = NoteValue { orders: 1, class, combo: (0, 0), low: state, high: state };
        Member { values: vec![Some(value)], ..Member::at(state) }
    }

    fn mass(weight: u64) -> ProbabilityMass {
        ProbabilityMass::from_ratio(weight, 1000).unwrap()
    }

    /// Whether `outer` scores every pending note of `inner` as a hull of it.
    fn holds(outer: &Member, inner: &Member) -> bool {
        let within = |low: &FactorState, high: &FactorState, other_low: &FactorState, other_high: &FactorState| {
            (0..FIELDS).all(|field| low[field] <= other_low[field] && other_high[field] <= high[field])
        };
        within(&outer.low, &outer.high, &inner.low, &inner.high)
            && outer.values.iter().zip(&inner.values).all(|pair| match pair {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    (a.class, a.combo) == (b.class, b.combo) && within(&a.low, &a.high, &b.low, &b.high)
                }
                _ => false,
            })
    }

    /// Past twice the cap, members merge down to the cap, and the member each old one goes to holds it.
    #[test]
    fn merged_members_hold_their_histories() {
        let members: Vec<_> = (0..9).map(|at| member(1.0 + at as f32 * 1e-6, 0)).collect();
        let masses: Vec<_> = (1..=9).map(mass).collect();
        let (out, map) = trim_members(members.clone(), &masses, 4);
        assert_eq!(out.len(), 4);
        for (old, &at) in members.iter().zip(&map) {
            assert!(holds(&out[at as usize], old), "{old:?} outside {:?}", out[at as usize]);
        }
        let (out, map) = trim_members(members.clone(), &masses, 5);
        assert_eq!((out.len(), map), (9, (0..9).collect::<Vec<_>>()), "up to twice the cap, members stay");
    }

    /// Members whose pending notes have values of different probe classes never merge.
    #[test]
    fn members_of_different_note_classes_stay_apart() {
        let members = vec![member(1.0, 0), member(1.5, 1), member(2.0, 0)];
        let (out, map) = trim_members(members.clone(), &[mass(1), mass(2), mass(3)], 1);
        assert_eq!(out.len(), 2);
        assert_eq!(map[0], map[2]);
        assert_ne!(map[0], map[1]);
        for (old, &at) in members.iter().zip(&map) {
            assert!(holds(&out[at as usize], old));
        }
    }

    /// Members of no possible mass go without widening the others, which keep their order.
    #[test]
    fn members_without_mass_go_alone() {
        let members = vec![member(1.0, 0), member(100.0, 0), member(2.0, 0)];
        let (out, map) = trim_members(members.clone(), &[mass(5), ProbabilityMass::ZERO, mass(5)], 2);
        assert_eq!(out.len(), 2);
        assert_eq!((map[0], map[2]), (0, 1));
        for (old, new) in [(&members[0], &out[0]), (&members[2], &out[1])] {
            assert_eq!((old.low, old.high), (new.low, new.high));
        }
    }

    /// Neighbouring binary32 values are one step apart, across zero as well.
    #[test]
    fn steps_count_binary32_neighbours() {
        assert_eq!(steps(1.0f32.next_up()) - steps(1.0), 1);
        assert_eq!(steps(0.0) - steps(-0.0), 1);
        assert_eq!(steps(f32::from_bits(1)) - steps(0.0), 1);
        assert_eq!(steps(-0.0) - steps(-f32::from_bits(1)), 1);
        assert!(steps(-1.0) < steps(-0.5) && steps(0.5) < steps(1.0));
    }
}
