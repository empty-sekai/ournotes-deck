#!/usr/bin/env python3
"""Declare reproducible minimum-basis experiments from actual published master keys.

The 1,250 grid jobs are isolated master-skill kernels: five GK8 level-3
writers and ten actual GS66/GS71 support writers. They are not owned-deck
benchmarks. Native holder validation, complete recording and conditional-term
identity remain authoritative; this script neither invents a chart nor replaces
a requested chart's missions. Run the large grid in basisIdentify mode, then
generate only the small warmup and query subsets.
"""
from __future__ import annotations

import argparse
import copy
import itertools
import json
from pathlib import Path

import catalogue
from analyze_start_operators import Catalogue, rational


PATTERNS = {
    "all-matched": (True, True, True, True, True),
    "mixed-matches": (False, False, True, True, True),
}
UNSEEN_LEVELS = ((1, 2, 3, 4), (2, 3, 4, 5), (5, 4, 3, 2),
                 (4, 3, 2, 1), (1, 3, 5, 2), (5, 1, 4, 2))


def key(source, skill, level, matched=None):
    return {"source": source, "id": skill, "level": level, "matched": matched}


def job(pattern, levels):
    levels = tuple(levels) + (5,)
    entries = []
    for position, matched in enumerate(PATTERNS[pattern]):
        entries.extend([
            [key("gekisou", 8, 3), position],
            [key("gekisouSupport", 66, levels[position], matched), position],
            [key("gekisouSupport", 71, 3, matched), position],
        ])
    suffix = "".join(map(str, levels[:4]))
    return {"name": f"minimum-grid-{pattern}-{suffix}", "entries": entries}


def spec(jobs, mode="basisPrograms", verify=False):
    return {"mode": mode, "basisMaxTerms": 64, "verifyBasis": verify,
            "mcRuns": 0, "scoreSamples": 0, "jobs": jobs}


def validate_selected_sources(data):
    if data.get("format") != "nnnotes.deck-data/1":
        raise ValueError("expected actual nnnotes.deck-data/1")
    keys, unsupported = catalogue.all_chain_keys(data)
    available = {catalogue.canonical(item) for item in keys}
    selected = [key("gekisou", skill, 3) for skill in (8, 20)]
    selected += [key("gekisouSupport", skill, level, matched)
                 for skill in (66, 71) for level in range(1, 6) for matched in (False, True)]
    for item in selected:
        if catalogue.canonical(item) not in available:
            raise ValueError(f"required actual native catalogue key is missing: {item}")
        if any(all(row.get(field) == item[field] for field in ("source", "id", "level"))
               for row in unsupported):
            raise ValueError(f"required source has conflicting formation targets: {item}")
    operators = Catalogue(data["master"])
    supports = []
    for item in selected:
        if item["source"] != "gekisouSupport":
            continue
        original = operators.start(item["source"], item["id"], item["level"], item["matched"], 1000)
        kind, minimum, probability = original["action"]
        if kind != "minimum" or minimum != (2 if item["matched"] else 1) or not 0 < probability < 1:
            raise ValueError(f"required source no longer has the declared minimum shape: {item}")
        supports.append({"key": item, "band": original["band"], "minimum": minimum,
                         "probability": rational(probability), "rowIds": original["rowIds"]})
    # Every holder combines GS66 and GS71, so the formation targets must be jointly constructible.
    if len({item["band"] for item in supports}) != 1 or supports[0]["band"] is None:
        raise ValueError("selected support sources no longer have one shared formation-band target")
    fixed = operators.sources["gekisou", 8, 3][1]
    if not fixed or any(row["_skillEffectType"] != 11001 for row in fixed):
        raise ValueError("the fixed actual GK8 level-3 source is no longer gauge speed only")
    control = operators.start("gekisou", 20, 3, None, 1000)
    if control["action"][0] != "lot":
        raise ValueError("the actual GK20 level-3 control no longer changes the non-minimum tape")
    return selected, supports, available


def make_specs(data):
    selected, supports, available = validate_selected_sources(data)
    grid = [job(pattern, levels) for pattern in PATTERNS
            for levels in itertools.product(range(1, 6), repeat=4)]
    warmup = [job("all-matched", (5, 5, 5, 5)), job("mixed-matches", (1, 1, 1, 1))]
    unseen = [job(pattern, levels) for pattern in PATTERNS for levels in UNSEEN_LEVELS]
    new = []
    for pattern in PATTERNS:
        for variant in ("gs71-levels-12345", "one-support-per-holder", "alternate-support-sources"):
            entry = job(pattern, (1, 2, 3, 4))
            entry["name"] = f"minimum-new-{pattern}-{variant}"
            if variant == "gs71-levels-12345":
                for item, position in entry["entries"]:
                    if item["source"] == "gekisouSupport" and item["id"] == 71:
                        item["level"] = position + 1
            elif variant == "one-support-per-holder":
                entry["entries"] = [[item, position] for item, position in entry["entries"]
                                    if item["source"] == "gekisou" or item["id"] == 66]
            else:
                entry["entries"] = [[item, position] for item, position in entry["entries"]
                                    if item["source"] == "gekisou" or item["id"] == (66 if position % 2 == 0 else 71)]
            new.append(entry)
    verification = [copy.deepcopy(unseen[index]) for index in (0, 2, 4, 6, 8, 10)]
    for entry in verification:
        entry["name"] = entry["name"].replace("minimum-grid-", "minimum-verify-")
    control = job("mixed-matches", (1, 2, 3, 4))
    control["name"] = "minimum-control-native-start-gauge"
    for item, position in control["entries"]:
        if item["source"] == "gekisou" and position == 4:
            item["id"] = 20
    files = {
        "grid-identify.json": spec(grid, "basisIdentify"),
        "warmup.json": spec(warmup),
        "unseen-levels.json": spec(unseen),
        "new-combinations.json": spec(new),
        "verification.json": spec(verification, verify=True),
        "nonminimum-control.json": spec([control], "basisIdentify"),
    }
    if len(grid) != 1250 or len({entry["name"] for entry in grid}) != 1250:
        raise ValueError("grid construction did not retain all 2 x 5^4 profiles")
    warm_keys = {catalogue.canonical(entry["entries"]) for entry in warmup}
    if any(catalogue.canonical(entry["entries"]) in warm_keys for entry in unseen + new):
        raise ValueError("a query subset reuses a warmup source key")
    for document in files.values():
        for entry in document["jobs"]:
            held = [[] for _ in range(5)]
            for item, position in entry["entries"]:
                if catalogue.canonical(item) not in available or type(position) is not int or not 0 <= position < 5:
                    raise ValueError("generated entry is not an actual catalogue key at an original holder position")
                held[position].append(item)
            for items in held:
                if sum(item["source"] == "gekisou" for item in items) != 1:
                    raise ValueError("generated holder does not have exactly one actual main writer")
                if sum(item["source"] == "gekisouSupport" for item in items) > 2:
                    raise ValueError("generated holder exceeds native support capacity")
    return files, selected, supports


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", type=Path, required=True, help="published regional deck-data JSON")
    parser.add_argument("--out", type=Path, required=True, help="directory for portable native request specs")
    args = parser.parse_args(argv)
    raw = args.data.read_bytes()
    data = json.loads(raw)
    files, selected, supports = make_specs(data)
    args.out.mkdir(parents=True, exist_ok=True)
    receipts = {}
    for name, document in files.items():
        path = args.out / name
        catalogue.write(path, document)
        receipts[name] = {"jobs": len(document["jobs"]), "mode": document["mode"],
                          "sha256": catalogue.sha(path.read_bytes()), "bytes": path.stat().st_size}
    receipt = {
        "format": "ournotes-deck.minimum-basis-specs/1",
        "scope": "isolated full-capacity kernels from actual master skills; not an owned-deck benchmark",
        "dataSha256": catalogue.sha(raw),
        "region": data.get("provenance", {}).get("region"),
        "masterVersion": data.get("provenance", {}).get("master", {}).get("version"),
        "sourceSha256": {name: catalogue.sha((Path(__file__).parent / name).read_bytes()) for name in (
            Path(__file__).name, "catalogue.py", "analyze_start_operators.py")},
        "selectedKeys": selected, "supportOperatorsAtDeclaredPhaseLife1000": supports,
        "grid": {"jobs": 1250, "formationPatterns": PATTERNS, "variable": "GS66 levels at holders0..3",
                 "levelsPerVariable": [1, 2, 3, 4, 5], "fixedFifthGS66Level": 5, "fixedGS71Level": 3,
                 "mainWriters": 5, "supportWriters": 10},
        "analyticalBound": {"maximumNativeLuckStarts": 3, "minimumValuesIncludingDisabled": [0, 1, 2],
                            "maximumConditionalProgramsPerUnchangedNonminimumTape": 27,
                            "nativeEqualityAndSuccessNotYetAssumed": True},
        "nativeValidationRequired": True, "changesChartsOrMissions": False, "usesMonteCarlo": False,
        "isRankingCertificate": False, "files": receipts,
    }
    catalogue.write(args.out / "receipt.json", receipt)
    print(json.dumps({"dataSha256": receipt["dataSha256"], "files": receipts}, sort_keys=True))


if __name__ == "__main__":
    main()
