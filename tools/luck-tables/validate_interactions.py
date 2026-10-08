#!/usr/bin/env python3
"""Compare real full-deck nominal responses with cold and warm dictionary lookups.

The supplied data, account snapshot, request and decks remain unchanged. The
native reference scores every original order, and its own ordered LUCK entries
become the dictionary requests. Differences are measured, never assumed away.
This is a predictor validation experiment, not a full-score expectation proof.
"""
from __future__ import annotations

import argparse
import importlib.util
import itertools
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import time


_spec = importlib.util.spec_from_file_location("luck_interaction_programs", Path(__file__).with_name("programs.py"))
programs = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(programs)
_basis_spec = importlib.util.spec_from_file_location("luck_interaction_basis", Path(__file__).with_name("basis.py"))
basis = importlib.util.module_from_spec(_basis_spec)
_basis_spec.loader.exec_module(basis)
pipeline = programs.pipeline
FORMAT = "ournotes-deck.luck-real-deck-validation/1"
NATIVE_FORMAT = "ournotes-deck.luck-response-generation/1"
ALGORITHM = "ournotes-luck-response/1/"
ORDERS = set(itertools.permutations(range(5)))
EMPTY = [[1.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]


def lookup_backend(name):
    if name == "programs":
        return programs, "programs", "uniquePrograms"
    if name == "basis":
        return basis, "basisPrograms", "uniqueBasisPrograms"
    raise ValueError("unknown lookup backend; choose programs or basis")


def query_statistics(query, backend, label, source, hashes, spec_sha, jobs):
    implementation, _, unique_field = lookup_backend(backend)
    if (query.get("format") != implementation.QUERY or query.get("status") != "success"
            or query.get("complete") is not True or query.get("inputsUnchanged") is not True
            or query.get("sourceVersion") != source or query.get("inputSha256") != {**hashes, "spec": spec_sha}
            or backend == "basis" and query.get("operatorContract") != basis.CONTRACT):
        raise ValueError(f"{label} {backend} lookup did not complete on the exact reference inputs and source")
    stats = query["stats"]
    for field in ("requestedJobs", unique_field, "reusedPrograms", "generatedPrograms", "propagationCalls"):
        if type(stats.get(field)) is not int or stats[field] < 0:
            raise ValueError(f"{label} {backend} lookup lacks its exact native work counts")
    if stats["requestedJobs"] != jobs or stats[unique_field] == 0:
        raise ValueError("lookup did not identify every requested original order")
    if label == "cold":
        if stats["reusedPrograms"] != 0 or not stats["generatedPrograms"] == stats["propagationCalls"] == stats[unique_field]:
            raise ValueError("cold lookup did not generate exactly its distinct native programs")
    elif label == "warm":
        if stats["generatedPrograms"] != 0 or stats["propagationCalls"] != 0 or stats["reusedPrograms"] != stats[unique_field]:
            raise ValueError("warm lookup performed propagation or failed to reuse every requested program")
    else:
        raise ValueError("unknown cold/warm query phase")
    return stats


def reference_spec(decks):
    return {"mode": "generate", "jobs": [{"name": "base", "entries": []}],
            "validationDecks": decks, "mcRuns": 0, "scoreSamples": 0}


def reference_bytes(decks):
    # Preserve a reproducible reference-spec byte identity, including its trailing newline.
    return (json.dumps(reference_spec(decks), ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode()


def source_version(report):
    algorithm = report["context"]["algorithmVersion"]
    if not isinstance(algorithm, str) or not algorithm.startswith(ALGORITHM):
        raise ValueError("reference uses an unknown probability algorithm")
    return programs.digest(algorithm[len(ALGORITHM):])


def validate_decks(decks):
    if not isinstance(decks, list) or not 0 < len(decks) <= 65536 // 120:
        raise ValueError("provide real decks covering 1..65536 requested order jobs")
    for deck in decks:
        if (not isinstance(deck, dict) or set(deck) - {"name", "members", "snaps"}
                or not isinstance(deck.get("name", ""), str)
                or not isinstance(deck.get("members"), list) or len(deck["members"]) != 5
                or any(type(value) is not int for value in deck["members"])
                or not isinstance(deck.get("snaps"), list) or len(deck["snaps"]) != 5
                or any(value is not None and type(value) is not int for value in deck["snaps"])):
            raise ValueError("each deck needs five native member IDs and five Snap IDs or nulls")


def validate_provenance(report, hashes, spec_sha):
    provenance = report["provenance"]
    inputs = provenance["inputs"]
    if (provenance.get("datasetSha256") != hashes["data"]
            or inputs.get("rosterSha256") != hashes["snapshot"]
            or inputs.get("requestSha256") != hashes["request"]
            or inputs.get("specSha256") != spec_sha):
        raise ValueError("reference provenance differs from the original inputs or exact deck specification")


def validate_curve(curve):
    if not isinstance(curve, dict):
        raise ValueError("missing native probability curve")
    previous = None
    for step in curve["steps"]:
        point = step["timeMs"]
        if type(point) is not int or not -(1 << 31) <= point < (1 << 31) or previous is not None and point <= previous:
            raise ValueError("curve times are not strictly increasing native milliseconds")
        previous = point
        buckets = step["buckets"]
        if not isinstance(buckets, list) or len(buckets) != 4:
            raise ValueError("a joint probability step needs four buckets")
        for interval in buckets:
            if (not isinstance(interval, list) or len(interval) != 2
                    or any(type(value) not in (int, float) or not math.isfinite(value) for value in interval)
                    or not 0 <= interval[0] <= interval[1] <= 1):
                raise ValueError("invalid probability interval")
    if (not isinstance(curve["probes"], list) or any(type(value) is not bool for value in curve["probes"])
            or not isinstance(curve["probeTransitions"], list)
            or any(type(value) is not int or not 0 <= value <= 15 for value in curve["probeTransitions"])
            or not isinstance(curve["rangeMoments"], list)):
        raise ValueError("invalid native curve observation metadata")


def ordered_rows(deck, expected):
    if any(deck.get(field, "" if field == "name" else None) != expected.get(field, "" if field == "name" else None)
           for field in ("name", "members", "snaps")):
        raise ValueError("native result changed a requested physical deck")
    rows = deck["orders"]
    if not isinstance(rows, list) or len(rows) != 120:
        raise ValueError("each real deck must have all 120 original orders")
    result, ordinals = {}, set()
    for row in rows:
        order = row["order"]
        ordinal = row["ordinal"]
        if (not isinstance(order, list) or any(type(value) is not int for value in order)
                or tuple(order) not in ORDERS or tuple(order) in result
                or type(ordinal) is not int or not 0 <= ordinal < 120 or ordinal in ordinals):
            raise ValueError("native orders are missing, duplicated or malformed")
        result[tuple(order)] = row
        ordinals.add(ordinal)
    return result


def validate_reference(report, decks, hashes, spec_sha, plan):
    if report.get("format") != NATIVE_FORMAT or report.get("mode") != "generate":
        raise ValueError("expected an original native generation reference")
    validate_provenance(report, hashes, spec_sha)
    source = source_version(report)
    if (source != source_version(plan) or report["context"] != plan["context"]
            or report["sharedFingerprint"] != plan["sharedFingerprint"]
            or report["dependencyDescriptor"] != plan["dependencyDescriptor"]
            or report["capabilities"] != plan["capabilities"]):
        raise ValueError("reference source or resolved native dependencies differ from the current generator")
    values = report["validationDecks"]
    if not isinstance(values, list) or len(values) != len(decks):
        raise ValueError("reference does not cover every supplied physical deck")
    output = []
    for value, deck in zip(values, decks):
        rows = ordered_rows(value, deck)
        for row in rows.values():
            if (row.get("actual", {}).get("status") != "success"
                    or row.get("scoring", {}).get("status") != "success"
                    or type(row["scoring"].get("scoreAtMean")) is not int
                    or row["scoring"].get("samples") is not None
                    or not isinstance(row.get("entries"), list)):
                raise ValueError("a native full-deck order lacks a successful unsampled reference")
            validate_curve(row["actual"]["curve"])
        output.append(rows)
    return source, output


def outward_sum(left, right):
    if left == [0.0, 0.0]:
        return right
    if right == [0.0, 0.0]:
        return left
    return [max(0.0, math.nextafter(left[0] + right[0], -math.inf)),
            min(1.0, math.nextafter(left[1] + right[1], math.inf))]


def weights(buckets):
    return [outward_sum(buckets[2], buckets[3]), outward_sum(buckets[1], buckets[3]), buckets[3]]


def same_endpoint(left, right):
    return struct.pack(">d", left) == struct.pack(">d", right)


def compare_curves(actual, lookup):
    validate_curve(actual)
    validate_curve(lookup)
    times = sorted({step["timeMs"] for curve in (actual, lookup) for step in curve["steps"]})
    positions = [0, 0]
    current = [EMPTY, EMPTY]
    result = {"timePoints": len(times), "jointIntervals": 4 * len(times), "weightedIntervals": 3 * len(times),
              "sameJointEndpoints": True, "jointIntervalsOverlap": True, "weightedIntervalsOverlap": True,
              "lookupEnclosesReferenceEndpoints": True, "maximumJointSeparatedGap": 0.0,
              "maximumWeightedSeparatedGap": 0.0,
              "sameTransitionMasks": actual["probeTransitions"] == lookup["probeTransitions"],
              "sameProbeFlags": actual["probes"] == lookup["probes"],
              "lookupCoversRequiredProbes": len(actual["probes"]) == len(lookup["probes"])
              and all(not needed or available for needed, available in zip(actual["probes"], lookup["probes"])),
              "sameRangeMoments": actual["rangeMoments"] == lookup["rangeMoments"]}
    for point in times:
        for index, curve in enumerate((actual, lookup)):
            steps = curve["steps"]
            while positions[index] < len(steps) and steps[positions[index]]["timeMs"] <= point:
                current[index] = steps[positions[index]]["buckets"]
                positions[index] += 1
        for before, after in zip(*current):
            gap = max(0.0, before[0] - after[1], after[0] - before[1])
            result["sameJointEndpoints"] &= all(same_endpoint(a, b) for a, b in zip(before, after))
            result["jointIntervalsOverlap"] &= gap == 0
            result["maximumJointSeparatedGap"] = max(result["maximumJointSeparatedGap"], gap)
            result["lookupEnclosesReferenceEndpoints"] &= after[0] <= before[0] <= before[1] <= after[1]
        for before, after in zip(*(weights(buckets) for buckets in current)):
            gap = max(0.0, before[0] - after[1], after[0] - before[1])
            result["weightedIntervalsOverlap"] &= gap == 0
            result["maximumWeightedSeparatedGap"] = max(result["maximumWeightedSeparatedGap"], gap)
    return result


def prediction_rows(prediction, decks, source, hashes):
    if (prediction.get("format") != "ournotes-deck.luck-response-prediction/1"
            or prediction.get("status") != "success" or prediction.get("ordersScored") != 120 * len(decks)
            or source_version(prediction) != source or len(prediction.get("decks", [])) != len(decks)):
        raise ValueError("lookup prediction did not score every original order with the same native source")
    provenance = prediction["provenance"]
    inputs = provenance["inputs"]
    if (provenance.get("datasetSha256") != hashes["data"] or inputs.get("rosterSha256") != hashes["snapshot"]
            or inputs.get("requestSha256") != hashes["request"] or inputs.get("decksSha256") != hashes["decks"]):
        raise ValueError("lookup prediction provenance differs from the physical input files")
    result = []
    for actual, expected in zip(prediction["decks"], decks):
        rows = ordered_rows(actual, expected)
        if any(row.get("status") != "success" or type(row.get("scoreAtLookup")) is not int for row in rows.values()):
            raise ValueError("lookup prediction contains a missing or failed original order")
        result.append(rows)
    return result


def materialized_curves(report, backend="programs", source=None):
    lookup_backend(backend)
    expected_format = {"programs": "ournotes-deck.luck-response-materialization/1",
                       "basis": "ournotes-deck.luck-response-materialized/1"}[backend]
    if report.get("format") != expected_format:
        raise ValueError(f"expected native {backend} response materialization")
    if backend == "basis":
        materialized_source = programs.digest(report.get("sourceVersion"))
        if (report.get("operatorContract") != basis.CONTRACT
                or source_version(report["table"]) != materialized_source
                or type(report.get("propagationCalls")) is not int or report["propagationCalls"] != 0
                or any(report.get(field) is not False for field in
                       ("nativeExpectationProven", "rankingProven", "usesMonteCarlo"))):
            raise ValueError("basis materialization source, operator contract or prediction scope differs")
    if source is not None and source_version(report["table"]) != source:
        raise ValueError("materialized response uses a different native source")
    result = {}
    for entry in report["table"]["entries"]:
        key = pipeline.canonical(entry["key"])
        if key in result or entry["response"].get("status") != "success":
            raise ValueError("materialization has a duplicate or unsuccessful requested entry")
        curve = entry["response"]["curve"]
        validate_curve(curve)
        result[key] = curve
    return result


def validate(data, snapshot, request, decks_path, generator, output, mode="u24", reference=None, timeout=None,
             backend="programs"):
    started = time.monotonic()
    implementation, query_mode, _ = lookup_backend(backend)
    paths = {name: Path(path).resolve() for name, path in
             (("data", data), ("snapshot", snapshot), ("request", request), ("decks", decks_path))}
    generator, output = Path(generator).resolve(), Path(output).resolve()
    if reference is not None:
        reference = Path(reference).resolve()
    protected = {**paths, "generator": generator, **({"reference": reference} if reference is not None else {})}
    if any(path.is_relative_to(output) for path in protected.values()):
        raise ValueError("experiment output must not contain an original input, generator or reused reference")
    if output.exists() and any(output.iterdir()):
        raise ValueError("use a new empty experiment output so the cold dictionary is genuinely empty")
    before = pipeline.provenance(protected)
    hashes = {name: before[name] for name in paths}
    decks = pipeline.read(paths["decks"])
    validate_decks(decks)
    output.mkdir(parents=True, exist_ok=True)
    result = {"format": FORMAT, "complete": False, "status": "running", "mode": mode,
              "backend": backend, "queryFormat": implementation.QUERY,
              "inputSha256": hashes, "generatorSha256": before["generator"], "decks": decks,
              "nativeExpectationProven": False, "rankingProven": False, "usesMonteCarlo": False,
              "scope": "Full native nominal joint curves and score-at-mean proxies for the supplied real decks and all 120 original orders; no full-score expectation or ranking certificate.",
              "files": {"reference": "reference.json", "referenceSpec": "reference-spec.json", "plan": "plan.json",
                        "querySpec": "query-spec.json", "cold": "cold/query.json", "warm": "warm/query.json"}}
    pipeline.write(output / "validation.json", result)
    try:
        raw_spec = reference_bytes(decks)
        pipeline.atomic_bytes(output / "reference-spec.json", raw_spec)
        spec_sha = pipeline.sha(raw_spec)
        plan_spec = {**reference_spec(decks), "mode": "plan", "validationDecks": []}
        pipeline.write(output / "plan-spec.json", plan_spec)
        base = [generator, paths["data"], paths["snapshot"], paths["request"]]
        programs.call(base + [output / "plan-spec.json", output / "plan.json"], output / "plan.log", timeout)
        plan = pipeline.read(output / "plan.json")
        if plan.get("format") != NATIVE_FORMAT or plan.get("mode") != "plan":
            raise ValueError("current generator did not produce a native dependency plan")
        validate_provenance(plan, hashes, pipeline.sha((output / "plan-spec.json").read_bytes()))
        generated_ms = None
        if reference is None:
            began = time.monotonic()
            programs.call(base + [output / "reference-spec.json", output / "reference.json"],
                          output / "reference.log", timeout)
            generated_ms = (time.monotonic() - began) * 1000
        else:
            pipeline.atomic_bytes(output / "reference.json", reference.read_bytes())
        report = pipeline.read(output / "reference.json")
        source, reference_rows = validate_reference(report, decks, hashes, spec_sha, plan)
        result["sourceVersion"] = source
        result["reference"] = {"reused": reference is not None, "sha256": pipeline.sha((output / "reference.json").read_bytes()),
                               "specSha256": spec_sha, "orders": 120 * len(decks), "generationWallMs": generated_ms,
                               "nativeElapsedMs": report.get("elapsedMs")}
        jobs = [{"name": f"deck-{index:03d}-order-{row['ordinal']:03d}", "entries": row["entries"]}
                for index, rows in enumerate(reference_rows) for row in sorted(rows.values(), key=lambda row: row["ordinal"])]
        query_spec = {"mode": query_mode, "mcRuns": 0, "scoreSamples": 0, "jobs": jobs}
        pipeline.write(output / "query-spec.json", query_spec)
        query_spec_sha = pipeline.sha((output / "query-spec.json").read_bytes())
        result["queries"] = {}
        predictions = {}
        for label in ("cold", "warm"):
            query = implementation.query(paths["data"], paths["snapshot"], paths["request"], output / "query-spec.json",
                                         generator, output / "store", output / label, mode=mode, timeout=timeout,
                                         decks=paths["decks"])
            result["queries"][label] = query
            query_statistics(query, backend, label, source, hashes, query_spec_sha, len(jobs))
            predictions[label] = prediction_rows(pipeline.read(output / label / "prediction.json"), decks, source, hashes)
        result["sameColdWarmArchiveBytes"] = (output / "cold/responses.onlrsp").read_bytes() == (output / "warm/responses.onlrsp").read_bytes()
        result["sameColdWarmMaterialization"] = (output / "cold/responses.json").read_bytes() == (output / "warm/responses.json").read_bytes()
        if not result["sameColdWarmArchiveBytes"] or not result["sameColdWarmMaterialization"]:
            raise ValueError("warm query changed the materialized response or encoded archive bytes")
        curves = materialized_curves(pipeline.read(output / "cold/responses.json"), backend, source)
        if set(curves) != {pipeline.canonical(job["entries"]) for job in jobs}:
            raise ValueError("materialization does not cover precisely the original ordered LUCK entries")
        comparisons, errors = [], {label: [] for label in predictions}
        for deck_index, rows in enumerate(reference_rows):
            for order, row in sorted(rows.items()):
                curve = compare_curves(row["actual"]["curve"], curves[pipeline.canonical(row["entries"])])
                scores = {"referenceScoreAtMean": row["scoring"]["scoreAtMean"]}
                for label, decks_rows in predictions.items():
                    predicted = decks_rows[deck_index][order]
                    if predicted.get("entries") != row["entries"]:
                        raise ValueError("prediction changed the original native order's LUCK entries")
                    error = predicted["scoreAtLookup"] - row["scoring"]["scoreAtMean"]
                    scores[label] = {"scoreAtLookup": predicted["scoreAtLookup"], "signedError": error}
                    errors[label].append(error)
                comparisons.append({"deckIndex": deck_index, "ordinal": row["ordinal"], "order": list(order),
                                    "curve": curve, "scores": scores})
        result["comparisons"] = comparisons
        total = len(comparisons)
        result["summary"] = {"orders": total,
            **{field + "Orders": sum(row["curve"][field] for row in comparisons) for field in
               ("sameJointEndpoints", "jointIntervalsOverlap", "weightedIntervalsOverlap",
                "lookupEnclosesReferenceEndpoints", "sameTransitionMasks", "sameProbeFlags",
                "lookupCoversRequiredProbes", "sameRangeMoments")},
            "maximumJointSeparatedGap": max(row["curve"]["maximumJointSeparatedGap"] for row in comparisons),
            "maximumWeightedSeparatedGap": max(row["curve"]["maximumWeightedSeparatedGap"] for row in comparisons),
            "scoreProxy": {label: {"exactOrders": sum(error == 0 for error in values),
                                  "differentOrders": sum(error != 0 for error in values),
                                  "maximumAbsoluteError": max(map(abs, values)),
                                  "meanAbsoluteError": sum(map(abs, values)) / total,
                                  "meanSignedError": sum(values) / total} for label, values in errors.items()}}
        probability_agreement = all(row["curve"]["jointIntervalsOverlap"] and row["curve"]["weightedIntervalsOverlap"]
                                    and row["curve"]["lookupCoversRequiredProbes"] for row in comparisons)
        result.update(complete=True, jointProbabilityAgreement=probability_agreement,
                      scoreProxyExactlyMatches=all(error == 0 for values in errors.values() for error in values))
        result["status"] = "matched" if probability_agreement and result["scoreProxyExactlyMatches"] else "differences"
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(complete=False, status="error", error=pipeline.error_text(error))
    finally:
        try:
            result["inputsUnchanged"] = pipeline.provenance(protected) == before
        except OSError:
            result["inputsUnchanged"] = False
        if not result["inputsUnchanged"]:
            result.update(complete=False, status="error", error="an original input, generator or reference changed")
        result["elapsedMs"] = (time.monotonic() - started) * 1000
        pipeline.write(output / "validation.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("data", "snapshot", "request", "decks"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--generator", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mode", choices=programs.MODES, default="u24")
    parser.add_argument("--backend", choices=("programs", "basis"), default="programs",
                        help="whole-program dictionary (default) or conditional minimum-basis dictionary")
    parser.add_argument("--reference", type=Path,
                        help="reuse only a source/input/spec-bound full native reference; copied into the new output")
    parser.add_argument("--timeout-seconds", type=float)
    args = parser.parse_args()
    result = validate(args.data, args.snapshot, args.request, args.decks, args.generator, args.output,
                      args.mode, args.reference, args.timeout_seconds, backend=args.backend)
    print(json.dumps({key: result[key] for key in ("complete", "status", "elapsedMs")}))
    # A completed experiment may honestly find differences; its detailed counts are the result.
    return 0 if result["complete"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"luck real-deck validation: {error}", file=sys.stderr)
        sys.exit(2)
