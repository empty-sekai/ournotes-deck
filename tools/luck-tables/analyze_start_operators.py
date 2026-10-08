#!/usr/bin/env python3
"""Exact, deliberately narrow analysis of real range-start lottery operators.

This does not execute or replace the native simulator. It reads original master
rows, refuses unrecognised shapes, and conditions LIFE readers on a stated phase
LIFE value. Fraction arithmetic models independent nominal probabilities, not a
finite-seed distribution or the bit pattern of an outward floating DP interval.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
from fractions import Fraction
import hashlib
import itertools
import json
import math
from pathlib import Path
import struct


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def rational(value):
    return f"{value.numerator}/{value.denominator}"


def binary32(value):
    """Nearest-even binary32, verified against adjacent representable values."""
    value = Fraction(value)
    if not 0 <= value <= 2**24:
        raise ValueError("binary32 analyser only accepts nonnegative values up to 2**24")
    candidate = struct.unpack("<I", struct.pack("<f", float(value)))[0]
    choices = []
    for bits in range(max(0, candidate - 1), candidate + 2):
        number = struct.unpack("<f", struct.pack("<I", bits))[0]
        if math.isfinite(number):
            exact = Fraction.from_float(number)
            choices.append((abs(exact - value), bits & 1, bits, exact))
    return min(choices)[3]


def probability(percent):
    if type(percent) is not int or not 0 <= percent <= 100:
        raise ValueError("probability outside the accepted integer percentage domain")
    # Factory: (value as f32) / 100f32, not the ideal rational value/100.
    return binary32(binary32(percent) / 100)


def native_gauge_delta(maximum, value):
    product = maximum * value
    if not 0 < maximum <= 2**24 or not 0 <= product <= 2**24:
        raise ValueError("gauge product leaves the proved nonnegative nonwrapping binary32 integer domain")
    return math.floor(binary32(binary32(product) / 10000))


def table(master, name):
    raw = master[name]
    columns = raw["columns"]
    if len(set(columns)) != len(columns) or any(len(row) != len(columns) for row in raw["rows"]):
        raise ValueError(f"ambiguous table {name}")
    return [dict(zip(columns, row)) for row in raw["rows"]]


def unique(rows, field):
    out = {}
    for row in rows:
        if row[field] in out:
            raise ValueError(f"duplicate {field}: {row[field]}")
        out[row[field]] = row
    return out


class Catalogue:
    def __init__(self, master):
        self.conditions = unique(table(master, "MasterSkillCondition"), "_id")
        self.targets = unique(table(master, "MasterSkillTarget"), "_id")
        self.groups = defaultdict(list)
        for row in table(master, "MasterSkillConditionSet"):
            self.groups[row["_group"]].append([self.conditions[i] for i in row["_conditionIds"]])
        self.phases = unique(table(master, "MasterSkillEffectSetting"), "_skillEffectType")
        self.sources = {}
        for source, metadata, effects, field in [
            ("gekisou", "MasterGekisouSkill", "MasterGekisouSkillEffect", "_gekisouSkillID"),
            ("gekisouSupport", "MasterGekisouSupportSkill", "MasterGekisouSupportSkillEffect", "_gekisouSupportSkillID"),
        ]:
            metas = unique(table(master, metadata), "_id")
            selected = defaultdict(list)
            effect_rows = table(master, effects)
            unique(effect_rows, "_id")
            for row in effect_rows:
                selected[row[field], row["_level"]].append(row)
            for (skill, level), rows in selected.items():
                if any(11000 <= row["_skillEffectType"] <= 11005 for row in rows):
                    self.sources[source, skill, level] = (metas[skill], sorted(rows, key=lambda r: r["_id"]))

    def group(self, number):
        if number == 0:
            return []
        groups = self.groups[number]
        if len(groups) != 1:
            raise ValueError("only one original conjunction is analysed; OR/absent groups are refused")
        return groups[0]

    def direct(self, number, kind):
        group = self.group(number)
        if len(group) != 1:
            raise ValueError("expected one direct native trigger/release")
        row = group[0]
        if row["_conditionType"] != kind or not row["_isPositive"] or row["_conditionValues"]:
            raise ValueError("trigger/release differs from the accepted direct shape")
        if kind == 7010:
            for target in row["_conditionTargetIDs"]:
                target = self.targets[target]
                if target["_skillTargetType"] != 5 or target["_gekisouMissionType"] != 2:
                    raise ValueError("start target is not the LUCK mission")
        elif row["_conditionTargetIDs"]:
            raise ValueError("release unexpectedly has targets")

    def formation_band(self, rows):
        bands = set()
        for row in rows:
            for condition in self.group(row["_skillConditionGroup"]):
                if condition["_conditionType"] != 5000:
                    continue
                ids = condition["_conditionTargetIDs"]
                if len(ids) != 1:
                    raise ValueError("only one simple formation target is analysed")
                target = self.targets[ids[0]]
                if target["_bandID"] <= 0 or any(target.get(k, 0) for k in [
                    "_characterID", "_cardType", "_tagID", "_gekisouMissionType",
                ]) or any(target.get(k, []) for k in ["_liveSkillCategories", "_gekisouSkillCategories"]):
                    raise ValueError("formation requires more than a positive band target")
                bands.add(target["_bandID"])
        if len(bands) > 1:
            raise ValueError("selected source has conflicting formation targets")
        return next(iter(bands), None)

    def chance(self, number, life, matched):
        value, random_conditions = Fraction(1), 0
        for row in self.group(number):
            kind, values = row["_conditionType"], row["_conditionValues"]
            if kind == 4011:
                if len(values) != 1 or row["_conditionTargetIDs"]:
                    raise ValueError("probability condition has unexpected arguments")
                result = probability(values[0])
                random_conditions += 1
            elif kind in (2000, 2001, 2002, 2003):
                if len(values) != 1 or row["_conditionTargetIDs"]:
                    raise ValueError("LIFE comparator has unexpected arguments")
                threshold = values[0]
                result = Fraction({2000: life > threshold, 2001: life >= threshold,
                                   2002: life < threshold, 2003: life <= threshold}[kind])
            elif kind == 5000 and matched is not None and not values:
                result = Fraction(matched)
            else:
                raise ValueError(f"unrecognised start condition {kind}")
            if not row["_isPositive"]:
                result = 1 - result
            value *= result
        if random_conditions > 1:
            raise ValueError("more than one independent probability predicate per row")
        return value

    def start(self, source, skill, level, matched, life):
        metadata, rows = self.sources[source, skill, level]
        if metadata["_gekisouMissionType"] != 2:
            raise ValueError("non-LUCK source")
        if source == "gekisouSupport" and metadata["_gekisouSupportSkillExecTiming"] != 1:
            raise ValueError("unrecognised support execution timing")
        band = self.formation_band(rows)
        if (band is None) != (matched is None):
            raise ValueError("formation key does not match the source")
        actions = []
        for row in rows:
            kind = row["_skillEffectType"]
            if kind not in (11003, 11005) or self.phases[kind]["_phase"] != 2:
                raise ValueError("source is outside the phase-2 start gauge/minimum subset")
            if row["_skillTriggerType"] != 1 or row["_activationTimeSecond"] != 0 or any(row[k] for k in [
                "_skillCumulativeConditionID", "_effectExecuteLimitCount", "_effectExecuteLimitResetConditionGroup",
                "_maxEffectValue",
            ]) or row["_skillTargetIDs"]:
                raise ValueError("source has a lifetime/cumulative/target/limit dependency")
            self.direct(row["_skillTriggerConditionGroup"], 7010)
            self.direct(row["_skillReleaseConditionGroup"], 7013)
            chance = self.chance(row["_skillConditionGroup"], life, matched)
            value = row["_effectValue"]
            if kind == 11003:
                if value != 10000 or row["_effectLimitCount"] != 0:
                    raise ValueError("start gauge is not a complete 100% increment")
                if any(native_gauge_delta(m, value) != m for m in (50, 100)):
                    raise ValueError("native gauge conversion is not one complete lot")
                action = ("lot", 1, chance)
            else:
                if row["_effectLimitCount"] != 1 or value not in (2, 3, 4):
                    raise ValueError("minimum is not a supported one-draw guarantee")
                action = ("minimum", value - 1, chance)
            if chance:
                actions.append(action)
        if len(actions) != 1:
            raise ValueError("source must reduce to exactly one active start action in this declared phase state")
        key = {"source": source, "id": skill, "level": level, "matched": matched}
        return {"key": key, "band": band, "action": actions[0], "rowIds": [r["_id"] for r in rows]}


def lot_polynomial(chances):
    out = (Fraction(1),)
    for chance in chances:
        next_row = [Fraction(0)] * (len(out) + 1)
        for n, mass in enumerate(out):
            next_row[n] += mass * (1 - chance)
            next_row[n + 1] += mass * chance
        out = tuple(next_row)
    return out


def minimum_pmf(actions):
    cdf = [math.prod((1 - chance for result, chance in actions if result > m), start=Fraction(1)) for m in range(4)]
    return (cdf[0],) + tuple(cdf[i] - cdf[i - 1] for i in range(1, 4))


def operator(actions):
    return (lot_polynomial(chance for kind, _, chance in actions if kind == "lot"),
            minimum_pmf([(value, chance) for kind, value, chance in actions if kind == "minimum"]))


def source_classes(records, kind):
    groups = defaultdict(list)
    for record in records:
        if record["action"][0] == kind:
            groups[record["action"]].append(record)
    return [(action, groups[action]) for action in sorted(groups)]


def bind_supports(indices, classes, node_limit=2000):
    """Find an actual <=5-holder, <=2-support layout; unresolved is not illegal.

    Each holder's matched band constraints and duplicate original source rows are
    preserved. This is native probe-holder legality, not inventory-deck legality.
    """
    if len(indices) > 10:
        return None, False
    holders = [[] for _ in range(5)]
    nodes = 0

    def compatible(items, record):
        key = record["key"]
        if len(items) == 2 or any(item["key"] == key for item in items):
            return False
        trial = items + [record]
        positive = {item["band"] for item in trial if item["key"]["matched"] is True}
        if len(positive) > 1:
            return False
        actual = next(iter(positive), 0)
        return all((actual == item["band"]) == item["key"]["matched"] for item in trial)

    def place(at):
        nonlocal nodes
        nodes += 1
        if nodes > node_limit:
            return None
        if at == len(indices):
            return [[record["key"], position] for position, items in enumerate(holders) for record in items]
        used_states = set()
        for position, items in enumerate(holders):
            state = canonical([record["key"] for record in items])
            if state in used_states:
                continue
            used_states.add(state)
            for record in classes[indices[at]][1]:
                if compatible(items, record):
                    items.append(record)
                    result = place(at + 1)
                    items.pop()
                    if result is not None:
                        return result
        return None

    return place(0), nodes > node_limit


def count_operators(classes, maximum, limit, support):
    levels, seen, witnesses = [], {}, []
    for n in range(maximum + 1):
        combinations = math.comb(len(classes) + n - 1, n) if n else 1
        if combinations > limit:
            levels.append({"count": n, "algebraicMultisets": combinations,
                           "orderedClassSequences": len(classes) ** n,
                           "status": "enumerationLimit", "uniqueOperators": None})
            continue
        current, legal, unresolved, no_layout = set(), 0, 0, 0
        for indices in itertools.combinations_with_replacement(range(len(classes)), n):
            if support:
                binding, stopped = bind_supports(indices, classes)
                if binding is None:
                    unresolved += stopped
                    no_layout += not stopped
                    continue
            else:
                binding = [[classes[index][1][0]["key"], position] for position, index in enumerate(indices)]
            legal += 1
            image = operator([classes[index][0] for index in indices])
            current.add(image)
            if image in seen and len(witnesses) < 12 and seen[image][0] != n:
                witnesses.append({"leftCount": seen[image][0], "rightCount": n,
                                  "leftEntries": seen[image][1], "rightEntries": binding,
                                  "lotPmf": list(map(rational, image[0])), "minimumPmf": list(map(rational, image[1]))})
            seen.setdefault(image, (n, binding))
        levels.append({"count": n, "algebraicMultisets": combinations, "status": "enumerated",
                       "orderedClassSequences": len(classes) ** n,
                       "nativeHolderLayoutsFound": legal, "noLayoutFound": no_layout,
                       "layoutSearchLimit": unresolved, "uniqueOperators": len(current)})
    return {"semanticClasses": len(classes), "levels": levels,
            "uniqueOperatorsAcrossEnumeratedLegalLayouts": len(seen), "crossMultiplicityWitnesses": witnesses,
            "allRequestedMultisetsEnumerated": all(row["status"] == "enumerated" and not row["layoutSearchLimit"] for row in levels)}


def analyse(data, life, max_main, max_support, enumeration_limit):
    catalogue = Catalogue(data["master"])
    accepted, refused, miss = [], [], []
    for (source, skill, level), (metadata, rows) in sorted(catalogue.sources.items()):
        try:
            band = catalogue.formation_band(rows)
            variants = [None] if band is None else [False, True]
        except (KeyError, ValueError) as error:
            refused.append({"source": source, "id": skill, "level": level, "reason": str(error)})
            continue
        for matched in variants:
            try:
                accepted.append(catalogue.start(source, skill, level, matched, life))
            except (KeyError, ValueError) as error:
                refused.append({"source": source, "id": skill, "level": level, "matched": matched, "reason": str(error)})
        for row in rows:
            if source == "gekisouSupport" and row["_skillEffectType"] == 11003:
                miss.append({"source": source, "id": skill, "level": level, "rowId": row["_id"],
                             "value": row["_effectValue"], "triggerGroup": row["_skillTriggerConditionGroup"],
                             "trigger": catalogue.groups[row["_skillTriggerConditionGroup"]],
                             "deltaAtMaximum50": native_gauge_delta(50, row["_effectValue"]),
                             "deltaAtMaximum100": native_gauge_delta(100, row["_effectValue"]),
                             "includedInStartOperator": False})
    main = source_classes(accepted, "lot")
    support = source_classes(accepted, "minimum")
    if any(record["key"]["source"] != "gekisou" for _, records in main for record in records):
        raise ValueError("start lot source no longer has one-main-per-holder capacity")
    if any(record["key"]["source"] != "gekisouSupport" for _, records in support for record in records):
        raise ValueError("minimum source no longer has two-support-per-holder capacity")
    def describe(classes):
        return [{"kind": action[0], "value": action[1], "chance": rational(action[2]),
                 "keys": [record["key"] for record in records], "originalRowIds": [record["rowIds"] for record in records]}
                for action, records in classes]
    result = {"format": "ournotes-luck-start-operator-analysis/1", "phaseLifeAssumption": life,
              "rangeStartPhase": 2, "defaultGaugeMaximumAssumption": 100, "rushGaugeMaximumAssumption": 50,
              "isNativeValidation": False, "isScorePrediction": False, "isRankingCertificate": False,
              "nominalArithmetic": "exact Fraction of native nearest-even binary32 probabilities",
              "legacyIntervalBitsPreserved": False, "finiteSeedDistributionProven": False,
              "mainClasses": describe(main), "minimumClasses": describe(support),
              "writerSources": [{"source": source, "id": skill, "level": level,
                                 "metadata": metadata, "originalEffects": rows}
                                for (source, skill, level), (metadata, rows) in sorted(catalogue.sources.items())],
              "mainCounts": count_operators(main, max_main, enumeration_limit, False),
              "minimumCounts": count_operators(support, max_support, enumeration_limit, True),
              "outsideSubset": refused, "supportGaugeRows": miss,
              "requirements": [
                  "Every range uses the original fully admitted native constructor and deterministic phase LIFE; this script does not prove that LIFE stays constant.",
                  "All start actions precede the first note/lot consumption; no intervening draw or mutable predicate may observe an intermediate action.",
                  "Each minimum is independent, lasts exactly one draw, and shares the original range-complete release.",
                  "A full-gauge increment needs the native initial template and a nonwrapping lot/gauge domain; this analysis checks conversion only for maxima 50 and 100.",
                  "11001 native speed command timing/order remains in the surrounding transcript. 11002 is omitted only for probability-only curves, as in the existing compiler.",
                  "Support Miss-triggered 11003 rows are listed separately; their rounded deltas must not be added as raw percentages or treated as start rows.",
                  "Counts are per-type dimensions; their Cartesian product is not a count of legal inventory decks or a claim of all-controller coverage.",
              ]}
    witnesses = result["minimumCounts"]["crossMultiplicityWitnesses"]
    jobs = []
    for ordinal, witness in enumerate(witnesses):
        for side in ("left", "right"):
            jobs.append({"name": f"exact-minimum-{ordinal}-{side}", "entries": witness[side + "Entries"]})
    spec = {"mode": "programs", "mcRuns": 0, "scoreSamples": 0,
            "identityBytes": 32 << 20, "programBytes": 32 << 20, "jobs": jobs}
    return result, spec


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("data", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--phase-life", type=int, default=1000)
    parser.add_argument("--max-main", type=int, choices=range(6), default=5)
    parser.add_argument("--max-support", type=int, choices=range(11), default=10)
    parser.add_argument("--enumeration-limit", type=int, default=1000,
                        help="Maximum algebraic multisets per degree; higher degrees remain explicit, not approximated")
    args = parser.parse_args()
    if args.enumeration_limit < 1:
        parser.error("enumeration limit must be positive")
    raw = args.data.read_bytes()
    report, spec = analyse(json.loads(raw), args.phase_life, args.max_main, args.max_support, args.enumeration_limit)
    report["provenance"] = {"inputName": args.data.name, "dataSha256": hashlib.sha256(raw).hexdigest(),
                            "analyzerSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    args.output.mkdir(parents=True, exist_ok=True)
    for filename, value in [("analysis.json", report), ("equivalent-minimum-spec.json", spec)]:
        path = args.output / filename
        path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"acceptedMainClasses": len(report["mainClasses"]),
                      "acceptedMinimumClasses": len(report["minimumClasses"]),
                      "nativeComparisonJobs": len(spec["jobs"]), "output": str(args.output)}))


if __name__ == "__main__":
    main()
