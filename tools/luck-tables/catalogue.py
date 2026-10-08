#!/usr/bin/env python3
"""Acquire bdon's complete published replay catalogues and declare every chart.

This reads actual master rows and embedded notes. It does not synthesize charts,
pretend an anchor inventory owns every skill level, or enumerate team products.
Native planning must validate the complete requested writer catalogue separately.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import importlib.util
import json
from pathlib import Path
from urllib.parse import urljoin

BUILD_URLS = {
    "tw": "https://storage.bdon.moe/moenotes/music-data/build.json",
    "jp": "https://storage.bdon.moe/moenotes/jp/music-data/build.json",
}
DIFFICULTIES = ("easy", "normal", "hard", "expert")
SOURCES = {"gekisou": ("MasterGekisouSkillEffect", "_gekisouSkillID"),
           "gekisouSupport": ("MasterGekisouSupportSkillEffect", "_gekisouSupportSkillID")}
INVENTORY_FORMAT = "ournotes-deck.luck-catalogue/1"
_FETCH_SPEC = importlib.util.spec_from_file_location(
    "luck_catalogue_fetch", Path(__file__).parents[1] / "search-harness" / "fetch_dataset.py")
fetch = importlib.util.module_from_spec(_FETCH_SPEC)
_FETCH_SPEC.loader.exec_module(fetch)


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def write(path, value):
    fetch._atomic_write(Path(path), canonical(value) + b"\n")


def rows(data, name):
    table = data["master"][name]
    columns = table["columns"]
    if len(columns) != len(set(columns)) or any(len(row) != len(columns) for row in table["rows"]):
        raise ValueError(f"invalid master table {name}")
    return [dict(zip(columns, row)) for row in table["rows"]]


def unique(values, field, label):
    result = {}
    for row in values:
        key = row[field]
        if key in result:
            raise ValueError(f"duplicate {label} identity {key}")
        result[key] = row
    return result


def all_chain_keys(data):
    """Raw scheduling inventory, independently checked by native LuckSkills later.

    Every level containing 11000..11005 is retained, even when no owned card uses
    it. Formation variants follow native trigger/condition groups, not release
    or reset groups. Conflicting targets are reported rather than silently lost.
    """
    conditions = unique(rows(data, "MasterSkillCondition"), "_id", "condition")
    groups = defaultdict(list)
    for row in rows(data, "MasterSkillConditionSet"):
        groups[row["_group"]].extend(row["_conditionIds"])
    selected = defaultdict(list)
    for source, (name, field) in SOURCES.items():
        for row in rows(data, name):
            selected[(source, row[field], row["_level"])].append(row)
    keys, unsupported = [], []
    for (source, skill, level), effect_rows in sorted(selected.items()):
        if not any(11000 <= row["_skillEffectType"] <= 11005 for row in effect_rows):
            continue
        targets = set()
        for row in effect_rows:
            for field in ("_skillTriggerConditionGroup", "_skillConditionGroup"):
                group = row[field]
                if not group:
                    continue
                if group not in groups:
                    raise ValueError(f"missing formation condition group {group}")
                for condition_id in groups[group]:
                    if condition_id not in conditions:
                        raise ValueError(f"missing condition {condition_id}")
                    condition = conditions[condition_id]
                    if condition["_conditionType"] == 5000:
                        targets.add(tuple(sorted(condition["_conditionTargetIDs"])))
        if len(targets) > 1:
            unsupported.append({"source": source, "id": skill, "level": level,
                                "reason": "native formation targets differ across selected rows"})
        for matched in ([False, True] if targets else [None]):
            keys.append({"source": source, "id": skill, "level": level, "matched": matched})
    return keys, unsupported


def inventory(data, dataset_id=None, region=None):
    if data.get("format") != "nnnotes.deck-data/1":
        raise ValueError("expected nnnotes.deck-data/1")
    music = unique(rows(data, "MasterLiveMusic"), "_id", "music")
    scores = unique(rows(data, "MasterLiveMusicScore"), "_id", "score")
    charts = unique(data["charts"], "scoreId", "embedded chart")
    if set(scores) != set(charts):
        raise ValueError(f"master/embedded chart mismatch: missing={sorted(set(scores)-set(charts))}, "
                         f"extra={sorted(set(charts)-set(scores))}")
    declared = {}
    for music_id, song in sorted(music.items()):
        for difficulty in DIFFICULTIES:
            score_id = song["_" + difficulty + "ID"]
            if not score_id:
                continue
            if score_id in declared or score_id not in charts:
                raise ValueError(f"ambiguous or missing music chart {score_id}")
            chart = charts[score_id]
            for name, required in (("notes", "id"), ("fevers", "startMs")):
                columns = chart[name]
                if required not in columns or any(not isinstance(v, list) for v in columns.values()):
                    raise ValueError(f"invalid embedded {name} columns for {score_id}")
                if len({len(v) for v in columns.values()}) != 1:
                    raise ValueError(f"embedded {name} columns differ for {score_id}")
            count = len(chart["fevers"]["startMs"])
            if count > 3:
                raise ValueError(f"unknown mission range schema for {score_id}")
            missions = [song[f"_gekisouMission{i+1}"] for i in range(count)]
            declared[score_id] = {"scoreId": score_id, "musicId": music_id, "difficulty": difficulty,
                                  "level": scores[score_id]["_musicScoreLevel"], "missions": missions,
                                  "hasLuckMission": 2 in missions,
                                  "noteRows": len(chart["notes"]["id"]),
                                  "chartSha256": sha(canonical(chart)), "asset": chart.get("asset")}
    if set(declared) != set(charts):
        raise ValueError(f"charts without declared music/difficulty: {sorted(set(charts)-set(declared))}")
    keys, unsupported = all_chain_keys(data)
    source_levels = {(key["source"], key["id"], key["level"]) for key in keys}
    return {"format": INVENTORY_FORMAT, "datasetId": dataset_id, "region": region,
            "scope": "all charts and all four declared difficulties in this published regional deck-data snapshot",
            "masterTables": len(data["master"]), "songs": len(music), "charts": list(declared.values()),
            "counts": {"charts": len(charts), "difficulties": dict(Counter(x["difficulty"] for x in declared.values())),
                       "luckCharts": sum(x["hasLuckMission"] for x in declared.values()),
                       "writerSourceLevels": len(source_levels), "writerFormationKeys": len(keys),
                       "sourceLevelsByKind": dict(Counter(key[0] for key in source_levels)),
                       "singlePositionJobsPerChart": 1 + 5 * len(keys),
                       "allChartBaseAndSingleJobs": len(charts) * (1 + 5 * len(keys)),
                       "applicableBaseAndSingleJobs": sum(x["hasLuckMission"] for x in declared.values()) * (1 + 5 * len(keys))},
            "luckCatalogueKeys": keys, "structuralUnsupported": unsupported,
            "combinationCoverage": "No Cartesian product implied; explicit combinations are separate jobs.",
            "nativeCatalogueValidated": False}


def acquire(region, output, download=fetch._download):
    """Discover a fresh pointer, then pin and verify both manifest and data bytes."""
    output = Path(output).resolve()
    url = BUILD_URLS[region]
    build_raw = download(url, fetch.MANIFEST_LIMIT)
    build = fetch._json(build_raw, "build")
    if build.get("format") != "moenotes.music-data-build/1":
        raise ValueError("unknown bdon music build format")
    if build.get("provenance", {}).get("region") != region:
        raise ValueError("published build region differs")
    replay = build["replay"]
    manifest_url = urljoin(url, replay["manifestUrl"])
    fetch._url(manifest_url, "replay manifest")
    manifest_raw = download(manifest_url, fetch.MANIFEST_LIMIT)
    fetch._verify(manifest_raw, replay, "replay manifest")
    manifest = fetch._json(manifest_raw, "replay manifest")
    if manifest.get("format") != fetch.MANIFEST_FORMAT:
        raise ValueError("unknown replay manifest format")
    entry = {**manifest["deckData"], "url": urljoin(manifest_url, manifest["deckData"]["url"])}
    fetch._entry(entry, "deck data", size=True)
    if entry["format"] != fetch.DATA_FORMAT:
        raise ValueError("unknown deck data format")
    if entry["bytes"] > 256 * 1024 * 1024:
        raise ValueError("deck data exceeds acquisition limit")
    raw = download(entry["url"], entry["bytes"])
    fetch._dataset(raw, entry)
    result = inventory(fetch._json(raw, "deck data"), entry["sha256"], region)
    replay_charts = unique(manifest["charts"], "scoreId", "replay chart")
    ids = {row["scoreId"] for row in result["charts"]}
    if set(replay_charts) != ids or build.get("charts") != result["counts"]["charts"]:
        raise ValueError("published build/manifest/deck chart coverage differs")
    if build.get("songs") != result["songs"] or replay.get("charts") != len(ids):
        raise ValueError("published song/replay counts differ")
    source = {"format": fetch.SOURCE_FORMAT, "name": "bdon.moe", "region": region,
              "buildUrl": url, "replayManifest": {"url": manifest_url, "sha256": sha(manifest_raw)},
              "deckData": entry}
    receipt = {"format": "ournotes-deck.luck-catalogue-acquisition/1", "source": source,
               "build": {"url": url, "sha256": sha(build_raw), "bytes": len(build_raw),
                         "builtAt": build.get("builtAt"), "inputs": build.get("inputs"), "run": build.get("run")},
               "counts": result["counts"], "songs": result["songs"],
               "freshness": "Freshly fetched published regional replay build; not a claim about unreleased or later master rows.",
               "embeddedNotesComplete": True}
    for path, contents in (("build.json", build_raw), ("replay-manifest.json", manifest_raw), ("deck-data.json", raw)):
        fetch._atomic_write(output / path, contents)
    write(output / "source.json", source)
    write(output / "catalogue.json", result)
    write(output / "acquisition.json", receipt)
    return receipt


def build_anchor(data_path, output):
    """Declared minimum represented parameters, explicitly not a player account."""
    spec = importlib.util.spec_from_file_location(
        "catalogue_roster_domains", Path(__file__).parents[1] / "search-harness" / "mock_rosters.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    raw = Path(data_path).read_bytes()
    data = json.loads(raw)
    domains = module.Domains(data["master"])
    selected, characters = [], set()
    for member in sorted(domains.members.values(), key=lambda row: row["_id"]):
        if member["_characterID"] in characters:
            continue
        awake = min(domains.member_awakes(member))
        selected.append({"id": member["_id"], "level": min(domains.member_levels(member, awake)),
                         "awake": awake, "rank": min(domains.ranks[member["_memberCardRankGroup"]]),
                         "liveSkillLevel": min(domains.skill_levels(member, "live")),
                         "gekisouSkillLevel": min(domains.skill_levels(member, "gk"))})
        characters.add(member["_characterID"])
        if len(selected) == 5:
            break
    if len(selected) != 5 or not domains.vip_ranks:
        raise ValueError("cannot construct five distinct represented anchors and a represented VIP rank")
    rank, vip = min(domains.character_ranks), min(domains.vip_ranks)
    ids = [row["id"] for row in selected]
    roster = {"members": selected, "snaps": [], "player": {
        "characterRanks": {str(c): rank for c in sorted(domains.characters)}, "bandItems": {},
        "vipRank": vip, "events": [], "memory": None, "ownedMemberCardIds": ids, "ownedSupportCardIds": []}}
    errors = domains.validate(roster)
    if errors:
        raise ValueError(f"invalid catalogue anchors: {errors}")
    snapshot = {"format": "ournotes.owned-snapshot/1", "datasetId": sha(raw),
                "revision": "catalogue-anchor/nonaccount/minimum-represented-parameters", "ownedFacts": {
                    "memberIds": ids, "snapIds": [], "memberCoverage": "complete", "snapCoverage": "complete"},
                "eligible": {"members": selected, "snaps": []}, "player": {
                    "characterRanks": {"coverage": "complete", "values": [
                        {"id": c, "value": rank} for c in sorted(domains.characters)]},
                    "characterTotalRank": rank * len(domains.characters), "vipRank": vip, "bandItems": [],
                    "memory": {"musicRanks": [], "unlockedMembers": [], "unlockedSnaps": []}, "eventIds": []},
                "assumptions": [{"path": "$", "reason": "Not a player account: five actual master cards with minimum represented cultivation and player ranks provide native kernel parameters only. Skill coverage is the independent full master catalogue."}]}
    write(output, snapshot)
    return snapshot


def build_manifest(data_path, snapshot_path, output, region=None):
    """Declare every real chart; caller supplies an explicit native anchor inventory.

    The anchor is not the skill catalogue and is copied unchanged. Scheduling all
    source levels requires pipeline's explicit catalogue mode, not owned selection.
    """
    data_path, snapshot_path, output = Path(data_path).resolve(), Path(snapshot_path).resolve(), Path(output).resolve()
    data_raw, snapshot_raw = data_path.read_bytes(), snapshot_path.read_bytes()
    data, snapshot = json.loads(data_raw), json.loads(snapshot_raw)
    inv = inventory(data, sha(data_raw), region)
    if snapshot.get("format") != "ournotes.owned-snapshot/1" or snapshot.get("datasetId") != inv["datasetId"]:
        raise ValueError("anchor snapshot does not identify exact dataset bytes")
    if output.exists() and any(output.iterdir()):
        raise ValueError("manifest output must be new or empty")
    member_ids = set(unique(rows(data, "MasterMemberCard"), "_id", "member"))
    snap_ids = set(unique(rows(data, "MasterSupportCard"), "_id", "snap"))
    if not {r["id"] for r in snapshot["eligible"]["members"]} <= member_ids:
        raise ValueError("anchor contains unknown member")
    if not {r["id"] for r in snapshot["eligible"]["snaps"]} <= snap_ids:
        raise ValueError("anchor contains unknown snap")
    fetch._atomic_write(output / "data.json", data_raw)
    fetch._atomic_write(output / "snapshot.json", snapshot_raw)
    cases = []
    for chart in inv["charts"]:
        name = f"{region or 'catalogue'}-{chart['scoreId']}-{chart['difficulty']}"
        request = {"format": "ournotes-deck.search-request/1",
                   "execution": {"kind": "live", "scoreId": chart["scoreId"], "gekisou": True,
                                 "play": {"kind": "theoreticalBest"}},
                   "scenario": {"kind": "mission", "musicId": chart["musicId"]}, "metric": {"kind": "score"},
                   "constraints": {}, "k": 3, "strategy": {"kind": "branchAndBound"},
                   "limits": {"timeLimitMs": 60000, "maxCandidates": None, "cacheEntries": 1024}}
        request_path = f"requests/{name}.json"
        write(output / request_path, request)
        cases.append({"name": name, "data": "data.json", "snapshot": "snapshot.json", "request": request_path,
                      "scoreId": chart["scoreId"], "difficulty": chart["difficulty"],
                      "hasLuckMission": chart["hasLuckMission"], "profile": "declared-anchor"})
    manifest = {"format": "ournotes-deck.search-benchmark/1", "matrixId": "whole-catalogue",
                "datasetId": inv["datasetId"], "cases": cases,
                "luckCatalogue": {"format": INVENTORY_FORMAT, "selection": "all-master-source-levels",
                                  "includeNonLuckCharts": True, "keys": inv["luckCatalogueKeys"],
                                  "counts": inv["counts"], "nativeCatalogueValidated": False},
                "coverage": {"charts": "all-master-score-ids", "play": "theoreticalBest",
                             "otherPlayProfilesCovered": False, "allTeamCombinationsCovered": False,
                             "anchorSnapshotSha256": sha(snapshot_raw)}}
    write(output / "benchmark.json", manifest)
    write(output / "catalogue.json", inv)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    get = commands.add_parser("acquire", help="fetch fresh regional build pointers and verify complete data")
    get.add_argument("--region", choices=("tw", "jp", "all"), default="all")
    get.add_argument("--output", type=Path, required=True)
    inspect = commands.add_parser("inspect", help="inventory complete local deck-data without native work")
    inspect.add_argument("data", type=Path)
    inspect.add_argument("--region", choices=("tw", "jp"))
    inspect.add_argument("--output", type=Path, required=True)
    anchor = commands.add_parser("anchor", help="declare five real-card native parameters; explicitly not an account")
    anchor.add_argument("data", type=Path)
    anchor.add_argument("--output", type=Path, required=True)
    manifest = commands.add_parser("manifest", help="declare all charts using an explicitly supplied anchor")
    manifest.add_argument("data", type=Path)
    manifest.add_argument("snapshot", type=Path)
    manifest.add_argument("--region", choices=("tw", "jp"))
    manifest.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "acquire":
            regions = tuple(BUILD_URLS) if args.region == "all" else (args.region,)
            result = {region: acquire(region, args.output / region) for region in regions}
            write(args.output / "acquisition.json", {"regions": result})
            print(json.dumps({r: v["counts"] for r, v in result.items()}, indent=2))
        elif args.command == "inspect":
            raw = args.data.read_bytes()
            result = inventory(json.loads(raw), sha(raw), args.region)
            write(args.output, result)
            print(json.dumps(result["counts"], indent=2))
        elif args.command == "anchor":
            result = build_anchor(args.data, args.output)
            print(json.dumps({"snapshot": str(args.output), "datasetId": result["datasetId"]}))
        else:
            result = build_manifest(args.data, args.snapshot, args.output, args.region)
            print(json.dumps({"manifest": str(args.output / "benchmark.json"), "cases": len(result["cases"])}))
    except (KeyError, OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
