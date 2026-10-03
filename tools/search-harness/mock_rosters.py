"""Deterministic mock rosters for search cases.

A pool that owns every card at one cultivation level and no band items does not
represent real accounts. These profiles vary ownership, cultivation, skill levels,
character ranks, band items (furniture), VIP rank and the kinds of cards owned.
None is a real account.

Every random draw is keyed by the profile seed and the card or fact it decides,
so a card added to the master changes no other card's ownership or cultivation
draw; only a card added to reach five characters can drop out when the new card
covers its character.
The manifest and every roster record the provenance hashes of the master tables
the generator read (null for a table the data does not have); the solver CLI
rejects a roster whose tables differ from the deck data's.

Usage: python3 mock_rosters.py DECK_DATA OUT_DIR [--extended]
"""

import argparse
import copy
import hashlib
import json
import pathlib
import random
import sys
from collections import defaultdict

SCHEMA = "ournotes.mock-rosters/3"

PROFILES = [
    # name, member share, snap share, cultivation (lo, hi), skill (lo, hi), character rank (lo, hi),
    # band item level (lo, hi), VIP (lo, hi), band skew (None or number of favored bands), seed
    ("max-items", 1.0, 1.0, (1.0, 1.0), (5, 5), (50, 50), (30, 30), (21, 21), None, 11),
    ("veteran", 0.75, 0.65, (0.7, 1.0), (3, 5), (15, 45), (15, 30), (8, 21), None, 12),
    ("midcore", 0.45, 0.4, (0.4, 0.8), (1, 4), (5, 25), (5, 20), (3, 12), None, 13),
    ("newcomer", 0.25, 0.15, (0.1, 0.5), (1, 2), (1, 8), (0, 6), (1, 4), None, 14),
    ("band-skewed", 0.5, 0.5, (0.5, 1.0), (2, 5), (5, 35), (10, 30), (5, 15), 2, 15),
    ("snap-poor", 0.8, 0.08, (0.6, 1.0), (3, 5), (10, 40), (10, 25), (5, 15), None, 16),
]

# Converting judgements: Snap skill effect types that rewrite note judgements.
CONVERSION_EFFECTS = {12006, 13005}


def keyed(seed, *key):
    """The random stream of one decision; str seeds are hashed with SHA-512, stable across runs and versions."""
    return random.Random(":".join(str(part) for part in (seed, *key)))


def cultivate(rng, lo, hi, top):
    """Six-profile helper; other profiles select represented rows."""
    f = lo + (hi - lo) * rng.random()
    return max(1, min(top, round(1 + f * (top - 1))))


def build(domains, profile):
    name, m_share, s_share, cult, skill, crank, item, vip, skew, seed = profile
    members = [domains.members[i] for i in sorted(domains.members)]
    bands = sorted({c["_bandID"] for c in domains.characters.values()})
    favored = set(keyed(seed, "bands").sample(bands, skew)) if skew else None
    band_of = lambda m: domains.characters[m["_characterID"]]["_bandID"]
    weight = (lambda m: 1.6 if band_of(m) in favored else 0.4) if favored else (lambda m: 1.0)
    chosen = [m for m in members if m_share >= 1.0 or keyed(seed, "own-member", m["_id"]).random() < m_share * weight(m)]
    # Keep a legal deck: at least five characters.
    by_character = defaultdict(list)
    for m in members:
        by_character[m["_characterID"]].append(m)
    have = {m["_characterID"] for m in chosen}
    for c in sorted(by_character):
        if len(have) >= 5:
            break
        if c not in have:
            chosen.append(keyed(seed, "fill", c).choice(by_character[c]))
            have.add(c)
    owned_members = []
    for m in sorted(chosen, key=lambda m: m["_id"]):
        rng = keyed(seed, "member", m["_id"])
        awake = cultivate(rng, *cult, 5)
        level_cap = domains.member_caps[(m["_rarity"], awake)]
        owned_members.append({
            "id": m["_id"],
            "level": cultivate(rng, *cult, level_cap),
            "awake": awake,
            "rank": cultivate(rng, *cult, max(domains.all_ranks[m["_memberCardRankGroup"]])),
            "liveSkillLevel": rng.randint(*skill),
            "gekisouSkillLevel": rng.randint(*skill),
        })
    owned_snaps = []
    for s in [domains.snaps[i] for i in sorted(domains.snaps)]:
        if s_share < 1.0 and keyed(seed, "own-snap", s["_id"]).random() >= s_share:
            continue
        rng = keyed(seed, "snap", s["_id"])
        rank = cultivate(rng, *cult, 5)
        owned_snaps.append({
            "id": s["_id"],
            "level": cultivate(rng, *cult, domains.snap_caps[(s["_supportCardRankGroup"], rank)]),
            "rank": rank,
        })
    items = {}
    for it in [domains.items[i] for i in sorted(domains.items)]:
        lo, hi = item
        if favored and it["_bandId"] not in favored:
            hi = max(lo, hi // 2)
        level = keyed(seed, "item", it["_id"]).randint(lo, hi)
        if level > 0:
            items[str(it["_id"])] = level
    return {
        "player": {
            "characterRanks": {str(c): keyed(seed, "character", c).randint(*crank) for c in sorted(domains.characters)},
            "bandItems": items,
            "vipRank": keyed(seed, "vip").randint(*vip),
            "events": [1],
            "memory": None,
            "ownedMemberCardIds": [m["id"] for m in owned_members],
            "ownedSupportCardIds": [s["id"] for s in owned_snaps],
        },
        "members": owned_members,
        "snaps": owned_snaps,
    }


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=False).encode()).hexdigest()


def grouped(rows, key, value):
    result = defaultdict(set)
    for row in rows:
        result[row[key]].add(row[value])
    return {key: sorted(values) for key, values in result.items()}


class Domains:
    """Represented model domains, not player ownership/resource prerequisite certification."""

    def __init__(self, master):
        self.read = []

        def rows(name):
            self.read.append(name)
            t = master.get(name)
            return [dict(zip(t["columns"], row)) for row in t["rows"]] if t else []

        self.members = {r["_id"]: r for r in rows("MasterMemberCard")}
        self.snaps = {r["_id"]: r for r in rows("MasterSupportCard")}
        self.characters = {r["_id"]: r for r in rows("MasterCharacter")}
        self.levels = grouped(rows("MasterMemberCardLevel"), "_group", "_level")
        self.awakes = grouped(rows("MasterMemberCardAwake"), "_group", "_awakeCount")
        rank_rows = rows("MasterMemberCardRank")
        self.all_ranks = grouped(rank_rows, "_group", "_rank")
        self.ranks = grouped([r for r in rank_rows if r["_rank"] > 0], "_group", "_rank")
        self.member_caps = {(r["_rarity"], r["_awakeCount"]): r["_limitLevel"]
                            for r in rows("MasterMemberCardLevelLimit")}
        self.snap_levels = grouped(rows("MasterSupportCardLevel"), "_group", "_level")
        snap_rank_rows = rows("MasterSupportCardRank")
        self.snap_ranks = grouped([r for r in snap_rank_rows if r["_rank"] > 0], "_group", "_rank")
        self.snap_caps = {(r["_group"], r["_rank"]): r["_limitLevel"] for r in snap_rank_rows}
        self.live_levels = grouped(rows("MasterLiveSkillEffect"), "_liveSkillID", "_level")
        gk_effects = rows("MasterGekisouSkillEffect")
        self.gk_levels = grouped(gk_effects, "_gekisouSkillID", "_level")
        self.gk_mission = {r["_id"]: r["_gekisouMissionType"] for r in rows("MasterGekisouSkill")}
        self.character_ranks = sorted({r["_rank"] for r in rows("MasterCharacterRank") if r["_rank"] > 0})
        self.vip_ranks = sorted({r["_vipRank"] for r in rows("MasterVip") if r["_vipRank"] > 0})
        self.events = sorted({r["_id"] for r in rows("MasterEvent")})
        self.items = {r["_id"]: r for r in rows("MasterBandItem")}
        effects = {(r["_bandItemId"], r["_level"]) for r in rows("MasterBandItemSkillEffect")}
        levels = [r for r in rows("MasterBandItemLevel") if r["_level"] > 0 and
                  (r["_bandItemId"], r["_level"]) in effects and r["_bandItemId"] in self.items]
        self.item_levels = grouped(levels, "_bandItemId", "_level")
        converting = {r["_supportSkillID"] for r in rows("MasterSupportSkillEffect")
                      if r["_skillEffectType"] in CONVERSION_EFFECTS}
        gk_converting = {r["_gekisouSupportSkillID"] for r in rows("MasterGekisouSupportSkillEffect")
                         if r["_skillEffectType"] in CONVERSION_EFFECTS}
        self.converting_snaps = {i for i, s in self.snaps.items()
                                 if {s["_supportSkillId01"], s["_supportSkillId02"]} & converting
                                 or {s["_gekisouSupportSkillId01"], s["_gekisouSupportSkillId02"]} & gk_converting}
        self.warnings = []
        if not self.vip_ranks:
            self.warnings.append("MasterVip absent: profiles other than the six legacy ones use model default rank 1; "
                                 "VIP legality is unverified; legacy VIP values are drawn from their fixed ranges")
        if not self.character_ranks:
            raise ValueError("MasterCharacterRank is required for legal mock rank selection")
        if self.items and set(self.item_levels) != set(self.items):
            raise ValueError("every mock band item needs represented MasterBandItemLevel and matching effect rows")

    def member_awakes(self, member):
        return [a for a in self.awakes.get(member["_memberCardAwakeGroup"], [])
                if a > 0 and (member["_rarity"], a) in self.member_caps]

    def member_levels(self, member, awake):
        cap = self.member_caps[(member["_rarity"], awake)]
        return [v for v in self.levels.get(member["_memberCardLevelGroup"], []) if 0 < v <= cap]

    def support_levels(self, snap, rank):
        cap = self.snap_caps[(snap["_supportCardRankGroup"], rank)]
        return [v for v in self.snap_levels.get(snap["_supportCardLevelGroup"], []) if 0 < v <= cap]

    def skill_levels(self, member, kind):
        key, mapping = (("_liveSkillID", self.live_levels) if kind == "live" else ("_gekisouSkillID", self.gk_levels))
        skill_id = member.get(key, 0)
        return [1] if not skill_id else [v for v in mapping.get(skill_id, []) if v > 0]

    def band_of(self, member):
        return self.characters[member["_characterID"]]["_bandID"]

    def validate(self, roster):
        errors = []

        def check(ok, label):
            if not ok:
                errors.append(label)
        for m in roster["members"]:
            source = self.members.get(m["id"])
            if source is None:
                errors.append(f"unknown member {m['id']}")
                continue
            check(m["awake"] in self.member_awakes(source), f"member {m['id']} awake")
            if m["awake"] in self.member_awakes(source):
                check(m["level"] in self.member_levels(source, m["awake"]), f"member {m['id']} level")
            check(m["rank"] in self.ranks.get(source["_memberCardRankGroup"], []), f"member {m['id']} rank")
            for kind, field in [("live", "liveSkillLevel"), ("gk", "gekisouSkillLevel")]:
                check(m[field] in self.skill_levels(source, kind), f"member {m['id']} {field}")
        for snap in roster["snaps"]:
            source = self.snaps.get(snap["id"])
            if source is None:
                errors.append(f"unknown Snap {snap['id']}")
                continue
            valid_rank = snap["rank"] in self.snap_ranks.get(source["_supportCardRankGroup"], [])
            check(valid_rank, f"Snap {snap['id']} rank")
            if valid_rank:
                check(snap["level"] in self.support_levels(source, snap["rank"]), f"Snap {snap['id']} level")
        player = roster["player"]
        for item, level in player["bandItems"].items():
            check(level in self.item_levels.get(int(item), []), f"item {item} level")
        for char, rank in player["characterRanks"].items():
            check(int(char) in self.characters and rank in self.character_ranks, f"character {char} rank")
        if self.vip_ranks:
            check(player["vipRank"] in self.vip_ranks, "VIP rank")
        check(all(e in self.events for e in player["events"]), "held event ID")
        ids = [m["id"] for m in roster["members"]]
        snaps = [s["id"] for s in roster["snaps"]]
        check(len(ids) == len(set(ids)), "duplicate members")
        check(len(snaps) == len(set(snaps)), "duplicate Snaps")
        check(ids == player["ownedMemberCardIds"], "member ownership facts differ")
        check(snaps == player["ownedSupportCardIds"], "Snap ownership facts differ")
        check(len({self.members[i]["_characterID"] for i in ids if i in self.members}) >= 5, "fewer than five characters")
        return errors


def choose(rng, values, fraction=(0.0, 1.0)):
    if not values:
        raise ValueError("empty represented cultivation domain")
    f = rng.uniform(*fraction)
    return values[min(len(values) - 1, max(0, round(f * (len(values) - 1))))]


def assemble(domains, seed, owned_members, owned_snaps, member_growth, snap_growth, character_rank, items):
    """A roster from owned cards and per-card growth rules; every draw is keyed by seed and card."""
    owned = []
    for m in sorted(owned_members, key=lambda m: m["_id"]):
        cultivation, skill = member_growth(m)
        rng = keyed(seed, "member", m["_id"])
        awake = choose(rng, domains.member_awakes(m), cultivation)
        owned.append({"id": m["_id"], "level": choose(rng, domains.member_levels(m, awake), cultivation),
                      "awake": awake, "rank": choose(rng, domains.ranks[m["_memberCardRankGroup"]], cultivation),
                      "liveSkillLevel": choose(rng, domains.skill_levels(m, "live"), skill),
                      "gekisouSkillLevel": choose(rng, domains.skill_levels(m, "gk"), skill)})
    snaps = []
    for s in sorted(owned_snaps, key=lambda s: s["_id"]):
        cultivation, rank_fraction = snap_growth(s)
        rng = keyed(seed, "snap", s["_id"])
        rank = choose(rng, domains.snap_ranks[s["_supportCardRankGroup"]], rank_fraction)
        snaps.append({"id": s["_id"], "rank": rank, "level": choose(rng, domains.support_levels(s, rank), cultivation)})
    return {"members": owned, "snaps": snaps, "player": {
        "characterRanks": {str(c): choose(keyed(seed, "character", c), domains.character_ranks, character_rank(c))
                           for c in sorted(domains.characters)},
        "bandItems": {str(i): choose(keyed(seed, "item", i), levels, items(domains.items[i]))
                      for i, levels in sorted(domains.item_levels.items()) if items(domains.items[i]) is not None},
        "vipRank": choose(keyed(seed, "vip"), domains.vip_ranks, (0.0, 1.0)) if domains.vip_ranks else 1,
        "events": ([1] if 1 in domains.events else domains.events[:1]), "memory": None,
        "ownedMemberCardIds": [m["id"] for m in owned], "ownedSupportCardIds": [s["id"] for s in snaps],
    }}


def fill_characters(domains, chosen, preferred=lambda m: True):
    """Adds the lowest-ID card of further characters, preferred cards first, until five characters are owned."""
    have = {m["_characterID"] for m in chosen}
    for m in sorted(domains.members.values(), key=lambda m: (not preferred(m), m["_id"])):
        if len(have) >= 5:
            break
        if m["_characterID"] not in have:
            chosen.append(m)
            have.add(m["_characterID"])
    return chosen


FAMILIES = {
    # member share, Snap share, cultivation, skill
    "balanced": (0.55, 0.5, (0.35, 0.95), (0.15, 1.0)),
    "single-band": (0.9, 0.55, (0.45, 1.0), (0.3, 1.0)),
    "member-poor-snap-rich": (0.10, 0.95, (0.3, 0.9), (0.1, 0.9)),
    "no-snap": (0.65, 0.0, (0.4, 1.0), (0.2, 1.0)),
    "strong-power-weak-skills": (0.6, 0.55, (0.85, 1.0), (0.0, 0.15)),
    "weak-power-strong-skills": (0.6, 0.55, (0.05, 0.35), (0.8, 1.0)),
}


def build_extended(domains, family, seed):
    m_share, s_share, cultivation, skill = FAMILIES[family]
    bands = sorted({c["_bandID"] for c in domains.characters.values()})
    favored = keyed(seed, "bands").choice(bands) if family == "single-band" else None
    in_band = lambda m: favored is None or domains.band_of(m) == favored
    chosen = [m for m in domains.members.values()
              if in_band(m) and keyed(seed, "own-member", m["_id"]).random() < m_share]
    chosen = fill_characters(domains, chosen, in_band)
    owned_snaps = [s for s in domains.snaps.values() if keyed(seed, "own-snap", s["_id"]).random() < s_share]
    rank_fraction = skill if family in {"strong-power-weak-skills", "weak-power-strong-skills"} else cultivation
    return assemble(domains, seed, chosen, owned_snaps, lambda m: (cultivation, skill),
                    lambda s: (cultivation, rank_fraction), lambda c: cultivation, lambda item: (0.15, 0.8))


def build_kind(domains, kind, seed):
    """Profiles that concentrate one kind of card or one constraint (see KINDS)."""
    members = list(domains.members.values())
    snaps = list(domains.snaps.values())
    own = lambda what, card, p: keyed(seed, what, card["_id"]).random() < p
    growth = lambda m: ((0.5, 1.0), (0.4, 1.0))
    snap_growth = lambda s: ((0.5, 1.0), (0.5, 1.0))
    character_rank = lambda c: (0.3, 0.9)
    items = lambda item: (0.2, 0.8)
    if kind == "conversion-rich":
        chosen = [m for m in members if own("own-member", m, 0.55)]
        owned_snaps = [s for s in snaps if own("own-snap", s, 1.0 if s["_id"] in domains.converting_snaps else 0.15)]
    elif kind in ("luck-heavy", "combo-heavy"):
        mission = 2 if kind == "luck-heavy" else 1
        focus = lambda m: domains.gk_mission.get(m["_gekisouSkillID"]) == mission
        chosen = fill_characters(domains, [m for m in members if own("own-member", m, 0.95 if focus(m) else 0.12)], focus)
        owned_snaps = [s for s in snaps if own("own-snap", s, 0.5)]
        growth = lambda m: ((0.6, 1.0), (0.8, 1.0)) if focus(m) else ((0.3, 0.8), (0.2, 0.6))
    elif kind == "single-attribute":
        attribute = keyed(seed, "attribute").choice(sorted({m["_cardType"] for m in members}))
        match = lambda card: card["_cardType"] == attribute
        chosen = fill_characters(domains, [m for m in members if match(m) and own("own-member", m, 0.95)], match)
        owned_snaps = [s for s in snaps if own("own-snap", s, 0.9 if match(s) else 0.08)]
    elif kind in ("five-characters", "six-characters"):
        count = 5 if kind == "five-characters" else 6
        characters = set(keyed(seed, "characters").sample(sorted(domains.characters), count))
        chosen = [m for m in members if m["_characterID"] in characters and own("own-member", m, 0.85)]
        chosen = fill_characters(domains, chosen, lambda m: m["_characterID"] in characters)
        owned_snaps = [s for s in snaps if own("own-snap", s, 0.55)]
    elif kind == "near-ties":
        # One cultivation point and skill level for every card, equal character ranks, no furniture.
        chosen = [m for m in members if own("own-member", m, 0.75)]
        owned_snaps = [s for s in snaps if own("own-snap", s, 0.6)]
        growth = lambda m: ((0.6, 0.6), (1.0, 1.0))
        snap_growth = lambda s: ((0.6, 0.6), (0.6, 0.6))
        character_rank = lambda c: (0.5, 0.5)
        items = lambda item: None
    elif kind == "gacha-newcomer":
        # A few top-rarity cards fully grown, the rest of a young account barely grown.
        top = max(m["_rarity"] for m in members)
        high = lambda card: card["_rarity"] >= top
        chosen = fill_characters(domains, [m for m in members if own("own-member", m, 0.3 if high(m) else 0.35)])
        owned_snaps = [s for s in snaps if own("own-snap", s, 0.2 if high(s) else 0.3)]
        growth = lambda m: ((0.9, 1.0), (0.8, 1.0)) if high(m) else ((0.0, 0.3), (0.0, 0.3))
        snap_growth = lambda s: ((0.8, 1.0), (0.8, 1.0)) if high(s) else ((0.0, 0.3), (0.0, 0.2))
        character_rank = lambda c: (0.0, 0.15)
        items = lambda item: (0.0, 0.2)
    elif kind == "rank-skewed":
        # One band's characters at the top character rank with its furniture maxed, the others at the bottom.
        band = keyed(seed, "bands").choice(sorted({c["_bandID"] for c in domains.characters.values()}))
        chosen = [m for m in members if own("own-member", m, 0.6)]
        owned_snaps = [s for s in snaps if own("own-snap", s, 0.5)]
        character_rank = lambda c: (1.0, 1.0) if domains.characters[c]["_bandID"] == band else (0.0, 0.0)
        items = lambda item: (1.0, 1.0) if item["_bandId"] == band else None
    else:
        raise ValueError(f"unknown kind {kind}")
    chosen = fill_characters(domains, chosen)
    return assemble(domains, seed, chosen, owned_snaps, growth, snap_growth, character_rank, items)


KINDS = {
    "conversion-rich": "every converting Snap, few others",
    "luck-heavy": "mostly members with LUCK Gekisou skills, grown further than the rest",
    "combo-heavy": "mostly members with COMBO Gekisou skills, grown further than the rest",
    "single-attribute": "members and Snaps of one attribute",
    "five-characters": "members of exactly five characters (one formation per member choice)",
    "six-characters": "members of exactly six characters",
    "near-ties": "uniform cultivation, equal character ranks and no furniture, so many decks nearly tie",
    "gacha-newcomer": "a few fully grown top-rarity cards in an otherwise young account",
    "rank-skewed": "one band at the top character rank with its furniture, the others at the bottom",
}
KIND_SEEDS = [101, 202]


def without_items(roster):
    result = copy.deepcopy(roster)
    result["player"].pop("bandItems")
    return result


def table_hashes(data, read):
    """Provenance hashes of the tables read: a missing table must be absent from the provenance too."""
    tables = data["provenance"]["master"]["tables"]
    hashes = {}
    for name in sorted(set(read)):
        if name in data["master"]:
            if name not in tables:
                raise ValueError(f"{name} has no provenance hash")
            hashes[name] = tables[name]["sha256"]
        else:
            if name in tables:
                raise ValueError(f"{name} has a provenance hash but no rows")
            hashes[name] = None
    return hashes


def generate(data_path, out, extended=False):
    data_path, out = pathlib.Path(data_path), pathlib.Path(out)
    data_bytes = data_path.read_bytes()
    data = json.loads(data_bytes)
    if data.get("format") != "nnnotes.deck-data/1":
        raise ValueError(f"{data_path}: expected nnnotes.deck-data/1, got {data.get('format')}")
    domains = Domains(data["master"])
    tables = table_hashes(data, domains.read)
    source = {"schema": SCHEMA, "tables": tables}
    out.mkdir(parents=True, exist_ok=True)
    records = []

    def save(name, roster, family, variant="sample", paired_group=None, seed=None, **metadata):
        errors = domains.validate(roster)
        if errors:
            raise ValueError(f"{name}: {'; '.join(errors)}")
        text = json.dumps({**roster, "mockSource": source}, ensure_ascii=False, indent=1) + "\n"
        path = out / f"mock-{name}-roster.json"
        path.write_text(text, encoding="utf-8", newline="\n")
        records.append({"name": name, "file": path.name, "sha256": hashlib.sha256(text.encode()).hexdigest(),
                        "semanticSha256": digest(roster), "family": family, "variant": variant,
                        "pairedGroup": paired_group, "seed": seed, "fixedFactsSha256": digest(without_items(roster)),
                        "members": len(roster["members"]), "snaps": len(roster["snaps"]),
                        "characters": len({domains.members[m["id"]]["_characterID"] for m in roster["members"]}),
                        "convertingSnaps": sum(s["id"] in domains.converting_snaps for s in roster["snaps"]),
                        "bandItems": len(roster["player"]["bandItems"]), **metadata})

    for profile in PROFILES:
        try:
            roster = build(domains, profile)
            errors = domains.validate(roster)
        except (KeyError, ValueError) as error:
            errors = [str(error)]
        preserved = not errors
        if errors:
            roster = build_extended(domains, "balanced", profile[-1])
        save(profile[0], roster, "legacy", seed=profile[-1], legacyPreserved=preserved,
             legacyAdjustmentReasons=errors, parameters=dict(zip(
                 ["memberShare", "snapShare", "cultivation", "skillLevels", "characterRanks",
                  "bandItemLevels", "vipRanks", "favoredBands", "seed"], profile[1:])))
    for kind, description in KINDS.items():
        for seed in KIND_SEEDS:
            save(f"{kind}-s{seed}", build_kind(domains, kind, seed), kind, seed=seed, description=description)
    if extended:
        for family in FAMILIES:
            for seed in [101, 202, 303]:
                save(f"{family}-s{seed}", build_extended(domains, family, seed), family, seed=seed)
        for family in FAMILIES:
            base = build_extended(domains, family, 707)
            group = f"items-{family}-s707"
            for variant in ["baseline", "none", "low", "high"]:
                roster = copy.deepcopy(base)
                if variant == "none":
                    roster["player"]["bandItems"] = {}
                elif variant != "baseline":
                    fraction = 0.25 if variant == "low" else 1.0
                    roster["player"]["bandItems"] = {
                        str(i): levels[round(fraction * (len(levels) - 1))]
                        for i, levels in sorted(domains.item_levels.items())}
                save(f"{group}-{variant}", roster, family, variant, group, 707)
    provenance = data["provenance"]
    manifest = {"schema": SCHEMA, "scope": "synthetic deterministic benchmark inputs; not player accounts",
                "validationScope": "represented cultivation rows/caps; ownership, resources and item player-rank prerequisites are not certified",
                "vipDomainVerified": bool(domains.vip_ranks), "warnings": domains.warnings,
                "data": {"sha256": hashlib.sha256(data_bytes).hexdigest(), "region": provenance["region"],
                         "masterVersion": provenance["master"]["version"]},
                "tables": tables,
                "generatorSha256": hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
                "pythonVersion": sys.version, "extended": extended, "rosterCount": len(records),
                "uniqueSemanticRosters": len({r["semanticSha256"] for r in records}), "rosters": records}
    (out / "mock-manifest.json").write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8", newline="\n")
    return manifest


def check_tables(manifest, data):
    """Errors when the deck data's master tables differ from those the rosters were generated from."""
    tables = data["provenance"]["master"]["tables"]
    for name, sha in manifest["tables"].items():
        actual = tables.get(name, {}).get("sha256")
        if actual != sha:
            raise ValueError(f"mock rosters were generated from {name} {sha}, the deck data has {actual}; "
                             "regenerate the mock rosters")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("data", type=pathlib.Path)
    parser.add_argument("out_dir", type=pathlib.Path)
    parser.add_argument("--extended", action="store_true", help="also generate 48 survey rosters including paired item controls")
    args = parser.parse_args()
    manifest = generate(args.data, args.out_dir, args.extended)
    for row in manifest["rosters"]:
        print(row["name"], row["members"], row["snaps"], row["characters"], row["convertingSnaps"], row["bandItems"])


if __name__ == "__main__":
    main()
