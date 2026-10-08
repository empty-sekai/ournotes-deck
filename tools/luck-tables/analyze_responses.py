#!/usr/bin/env python3
"""Measure real generated LUCK responses, including bounded curve fitting.

This analyzes a generator output, never runs a synthetic performance workload.
Fit residuals enclose the supplied probability intervals on the complete integer
millisecond domain of that response. They are not whole-score or ranking bounds.
"""

from __future__ import annotations

import argparse
import bisect
from fractions import Fraction
import gzip
import hashlib
import itertools
import json
import lzma
import math
from pathlib import Path
import time
import zlib

try:
    import zstandard
except ImportError:
    zstandard = None


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def sizes(data):
    measured = {
        "raw": len(data),
        "gzip9": len(gzip.compress(data, compresslevel=9, mtime=0)),
        "zlib9": len(zlib.compress(data, level=9)),
        "xz6": len(lzma.compress(data, preset=6)),
    }
    if zstandard is not None:
        measured["zstd19"] = len(zstandard.ZstdCompressor(level=19).compress(data))
    return measured


def response_payload(curve):
    """Work counters do not change the probability response itself."""
    return {k: v for k, v in curve.items() if k not in ("peakStates", "transitions")}


def validated_steps(curve):
    """The numeric domain required by the interpolation rounding allowance."""
    steps = curve.get("steps")
    if not isinstance(steps, list):
        raise ValueError("response steps must be a list")
    previous = None
    for step in steps:
        if not isinstance(step, dict):
            raise ValueError("response step must be an object")
        t = step.get("timeMs")
        if type(t) is not int or not -(2**31) <= t < 2**31 or (previous is not None and t <= previous):
            raise ValueError("response times must be strictly increasing i32 integers")
        previous = t
        buckets = step.get("buckets")
        if not isinstance(buckets, list) or len(buckets) != 4:
            raise ValueError("response step must contain exactly four probability intervals")
        for bounds in buckets:
            if not isinstance(bounds, list) or len(bounds) != 2:
                raise ValueError("response probability interval must have two endpoints")
            if any(type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 1 for value in bounds):
                raise ValueError("response probability endpoints must be finite and lie in [0, 1]")
            if bounds[0] > bounds[1]:
                raise ValueError("response probability interval is reversed")
    return steps


def interval_points(curve):
    """Endpoints of every constant interval on an integer time domain.

    A fitted linear segment minus a constant response is affine between these
    points. Its extrema occur at endpoints. Including both sides of each step
    therefore checks every integer millisecond, not merely the stored knots.
    """
    steps = validated_steps(curve)
    points = []
    for i, step in enumerate(steps):
        t = step["timeMs"]
        points.append((t, step["buckets"]))
        if i + 1 < len(steps):
            end = steps[i + 1]["timeMs"] - 1
            if end > t:
                points.append((end, step["buckets"]))
    return points


def midpoint(bounds):
    return [(v[0] + v[1]) * 0.5 for v in bounds]


def interpolate(left_t, left, right_t, right, t):
    if right_t == left_t:
        return left
    fraction = (t - left_t) / (right_t - left_t)
    return [a + (b - a) * fraction for a, b in zip(left, right)]


def fit_curve(curve, tolerance):
    """Adaptive multivariate piecewise linear fit with exhaustive residuals."""
    if not math.isfinite(tolerance) or tolerance < 0:
        raise ValueError("fit tolerance must be finite and nonnegative")
    points = interval_points(curve)
    if not points:
        return {"times": [], "values": [], "residual": [0.0] * 4, "verifiedPoints": 0}
    centers = [midpoint(bounds) for _, bounds in points]
    selected = {0, len(points) - 1}
    pending = [(0, len(points) - 1)]
    while pending:
        left, right = pending.pop()
        if right <= left + 1:
            continue
        greatest, split = -1.0, None
        for i in range(left + 1, right):
            predicted = interpolate(points[left][0], centers[left], points[right][0], centers[right], points[i][0])
            error = max(abs(a - b) for a, b in zip(predicted, centers[i]))
            if error > greatest:
                greatest, split = error, i
        if greatest > tolerance:
            selected.add(split)
            pending.extend(((left, split), (split, right)))
    selected = sorted(selected)
    residual = [0.0] * 4
    for left, right in zip(selected, selected[1:]):
        for i in range(left, right + 1):
            predicted = interpolate(points[left][0], centers[left], points[right][0], centers[right], points[i][0])
            for bucket, (value, bounds) in enumerate(zip(predicted, points[i][1])):
                residual[bucket] = max(residual[bucket], abs(value - bounds[0]), abs(value - bounds[1]))
    if len(selected) == 1:
        residual = [max(abs(c - lo), abs(c - hi)) for c, (lo, hi) in zip(centers[0], points[0][1])]
    # Integer i32 times and their differences are exact in binary64. The affine
    # evaluation has one division, subtraction, multiplication and addition of
    # bounded probabilities. This conservative allowance also covers evaluation
    # at unstored integer times between the checked endpoints.
    rounding = 8 * math.ulp(1.0)
    residual = [math.nextafter(value + rounding, math.inf) for value in residual]
    return {
        "times": [points[i][0] for i in selected],
        "values": [centers[i] for i in selected],
        "residual": residual,
        "verifiedPoints": len(points),
        "domain": [points[0][0], points[-1][0]],
    }


def lookup(curve, time_ms):
    times, steps = curve
    index = bisect.bisect_right(times, time_ms) - 1
    return None if index < 0 else steps[index]["buckets"]


def composition_report(entries, order):
    """Evaluate the actual interaction left after all available lower orders.

    The ordered key retains repeated skills and the original holder/source order.
    Coverage is explicit: absent subsets cannot silently become zero response.
    """
    curves = {}
    for entry in entries:
        if entry["response"]["status"] == "success":
            steps = validated_steps(entry["response"]["curve"])
            curves[canonical(entry["key"])] = ([s["timeMs"] for s in steps], steps)
    outcomes = []
    for entry in entries:
        key = entry["key"]
        if len(key) <= order or entry["response"]["status"] != "success":
            continue
        terms = {}
        missing = []
        # Sum the Mobius interaction for every subset with size <= order.
        for n in range(order + 1):
            for subset_indices in itertools.combinations(range(len(key)), n):
                for k in range(n + 1):
                    for component_indices in itertools.combinations(subset_indices, k):
                        component = canonical([key[i] for i in component_indices])
                        terms[component] = terms.get(component, 0) + (-1) ** (n - k)
        terms = {k: v for k, v in terms.items() if v}
        missing = [json.loads(k) for k in terms if k not in curves]
        row = {"entries": key, "interactionOrder": order}
        if missing:
            row.update(status="missingSubsets", missing=missing)
            outcomes.append(row)
            continue
        target = curves[canonical(key)]
        active = [(curves[k], multiplier) for k, multiplier in terms.items()]
        all_curves = [target] + [curve for curve, _ in active]
        if any(not times for times, _ in all_curves):
            row.update(status="emptyResponse")
            outcomes.append(row)
            continue
        start = max(times[0] for times, _ in all_curves)
        end = max(times[-1] for times, _ in all_curves)
        times = sorted({t for points, _ in all_curves for t in points if start <= t <= end})
        maximum, square, weighted_square = [0.0] * 4, [0.0] * 4, [0.0] * 4
        milliseconds = 0
        rush_maximum = 0.0
        outside = 0.0
        for index, t in enumerate(times):
            width = times[index + 1] - t if index + 1 < len(times) else 1
            milliseconds += width
            observed = lookup(target, t)
            predictions = [
                math.fsum(multiplier * midpoint(lookup(curve, t))[bucket] for curve, multiplier in active)
                for bucket in range(4)
            ]
            for bucket, (prediction, bounds) in enumerate(zip(predictions, observed)):
                maximum[bucket] = max(maximum[bucket], abs(prediction - bounds[0]), abs(prediction - bounds[1]))
                error_square = (prediction - (bounds[0] + bounds[1]) * 0.5) ** 2
                square[bucket] += error_square
                weighted_square[bucket] += error_square * width
                outside = max(outside, -prediction, prediction - 1.0)
            predicted_rush = predictions[2] + predictions[3]
            rush_maximum = max(
                rush_maximum,
                abs(predicted_rush - observed[2][0] - observed[3][0]),
                abs(predicted_rush - observed[2][1] - observed[3][1]),
            )
        row.update(
            status="measured",
            checkedTimeSegments=len(times),
            domain=[start, end],
            maximumBucketResidual=maximum,
            breakpointBucketRms=[math.sqrt(value / len(times)) for value in square],
            timeWeightedBucketRms=[math.sqrt(value / milliseconds) for value in weighted_square],
            checkedIntegerMilliseconds=milliseconds,
            maximumRushResidual=rush_maximum,
            outsideProbabilityRange=outside,
            residualScope="this complete observed combination and its step-function time domain only",
        )
        outcomes.append(row)
    measured = [row for row in outcomes if row["status"] == "measured"]
    return {
        "interactionOrder": order,
        "measuredCombinations": len(measured),
        "unavailableCombinations": len(outcomes) - len(measured),
        "maximumRushResidual": max((row["maximumRushResidual"] for row in measured), default=None),
        "maximumBucketResidual": max((max(row["maximumBucketResidual"]) for row in measured), default=None),
        "combinations": outcomes,
    }


def monte_carlo_report(document, entries):
    curves = {canonical(entry["key"]): entry["response"] for entry in entries}
    results = []
    for job in document.get("jobs", []):
        sampled = job.get("mc")
        response = curves.get(canonical(job["entries"]))
        if not sampled or sampled.get("status") != "sampled" or not response or response["status"] != "success":
            continue
        reference = response["curve"]["steps"]
        estimate = sampled["steps"]
        if not reference or not estimate:
            continue
        reference_times = [row["timeMs"] for row in reference]
        sample_times = [row[0] for row in estimate]
        start = max(reference_times[0], sample_times[0])
        times = sorted({t for t in reference_times + sample_times if t >= start})
        maximum = [0.0, 0.0, 0.0]
        squared = [0.0, 0.0, 0.0]
        for t in times:
            bucket = midpoint(reference[bisect.bisect_right(reference_times, t) - 1]["buckets"])
            observed = estimate[bisect.bisect_right(sample_times, t) - 1][1]
            expected = [bucket[2] + bucket[3], bucket[1] + bucket[3], bucket[3]]
            # All supported direct-7021 probes share the same predicate. Check
            # every returned probe, not just the first one in the catalogue.
            for index, value in enumerate(observed):
                column = 0 if index == 0 else 1 + (index - 1) % 2
                error = abs(value - expected[column])
                maximum[column] = max(maximum[column], error)
                squared[column] += error * error
        probe_count = (len(estimate[0][1]) - 1) // 2
        counts = [len(times), len(times) * probe_count, len(times) * probe_count]
        before, after = job.get("cacheBefore", {}), job.get("cacheAfter", {})
        results.append({
            "name": job["name"], "entries": job["entries"], "runs": sampled["runs"],
            "monteCarloMs": sampled["elapsedMs"],
            "recordMs": after.get("recordMs", 0) - before.get("recordMs", 0),
            "propagateMs": after.get("propagateMs", 0) - before.get("propagateMs", 0),
            "maximumAbsoluteError": dict(zip(("rush", "probe", "both"), maximum)),
            "breakpointRms": dict(zip(("rush", "probe", "both"),
                [math.sqrt(value / count) if count else None for value, count in zip(squared, counts)])),
            "checkedTimeSegments": len(times),
            "interpretation": "empirical finite-seed comparison to independent nominal DP, not a confidence or score certificate",
        })
    return results


def scalar_coefficient_report(validation_decks):
    """Fit a score-prediction coefficient per complete ordered writer key.

    Fit c in s0 + c*(s1-s0) to scoreAtMean with exact rational accumulation.
    This is equivalent to averaging each defined c with weight (s1-s0)^2;
    it does not replace a time-dependent response or estimate E[native score].
    Partial validations remain usable observations, with explicit coverage.
    """
    if not validation_decks:
        return None
    all_orders = set(itertools.permutations(range(5)))
    groups, decks, seen, unavailable = {}, [], set(), {}
    for deck in validation_decks:
        members, snaps = tuple(deck["members"]), tuple(deck["snaps"])
        if len(members) != 5 or len(snaps) != 5:
            raise ValueError("scalar analysis requires five paired deck positions")
        orders, scored = set(), 0
        for row in deck.get("orders", []):
            order = tuple(row["order"])
            if any(type(value) is not int for value in order) or order not in all_orders:
                raise ValueError("scalar analysis requires an original five-position order")
            identity = (members, snaps, order)
            if identity in seen:
                raise ValueError("scalar analysis contains a duplicate physical deck/order")
            seen.add(identity)
            orders.add(order)
            scoring = row.get("scoring") or {}
            if scoring.get("status") != "success":
                status = scoring.get("status", "missingScoring")
                unavailable[status] = unavailable.get(status, 0) + 1
                continue
            if scoring.get("scoreAtMeanIsExactExpectation") is not False:
                raise ValueError("scalar analysis requires explicitly labelled scoreAtMean predictions")
            s0, s1, score = (scoring[field] for field in ("s0", "s1", "scoreAtMean"))
            if any(type(value) is not int or not -(2**31) <= value < 2**31 for value in (s0, s1, score)):
                raise ValueError("scalar analysis scores must be native i32 integers")
            key = canonical(row["entries"])
            groups.setdefault(key, []).append((identity, s1 - s0, score - s0, score))
            scored += 1
        decks.append({"name": deck.get("name", ""), "members": members, "snaps": snaps,
                      "observedOrders": len(orders), "scoredOrders": scored,
                      "all120Labels": orders == all_orders, "all120Scored": orders == all_orders and scored == 120})
    reports = []
    for key, samples in sorted(groups.items()):
        denominator = sum(x * x for _, x, _, _ in samples)
        fitted = Fraction(sum(x * y for _, x, y, _ in samples), denominator) if denominator else Fraction(0)
        coefficients = [Fraction(y, x) for _, x, y, _ in samples if x]
        residuals = [(identity, fitted * x - y, score) for identity, x, y, score in samples]
        worst = max(residuals, key=lambda item: abs(item[1]))
        relative = [abs(residual / score) for _, residual, score in residuals if score]
        reports.append({
            "entries": json.loads(key), "orders": len(samples),
            "physicalDecks": len({identity[:2] for identity, _, _, _ in samples}),
            "coefficientDefinedOrders": len(coefficients),
            "zeroDenominatorOrders": sum(x == 0 for _, x, _, _ in samples),
            "zeroDenominatorNonzeroSignalOrders": sum(x == 0 and y != 0 for _, x, y, _ in samples),
            "minimumCoefficient": float(min(coefficients)) if coefficients else None,
            "maximumCoefficient": float(max(coefficients)) if coefficients else None,
            "coefficientSpan": float(max(coefficients) - min(coefficients)) if coefficients else None,
            "weightedLeastSquaresCoefficient": float(fitted) if denominator else None,
            "exactFittedFraction": {"numerator": fitted.numerator, "denominator": fitted.denominator} if denominator else None,
            "maximumAbsoluteProxyScoreResidual": float(abs(worst[1])),
            "meanAbsoluteProxyScoreResidual": float(sum(abs(r) for _, r, _ in residuals) / len(residuals)),
            "maximumRelativeProxyScoreResidual": float(max(relative)) if relative else None,
            "zeroReferenceScoreOrders": sum(score == 0 for _, _, score in residuals),
            "worstOrder": {"members": worst[0][0], "snaps": worst[0][1], "order": worst[0][2],
                           "signedProxyScoreResidual": float(worst[1]), "referenceScore": worst[2]},
        })
    return {
        "reference": "native scoreAtMean predictor; not Monte Carlo expectation, E[native score], or a score certificate",
        "fit": "argmin_c sum_orders ((s1-s0)*c-(scoreAtMean-s0))^2; exact rational accumulation; no coefficient clipping",
        "key": "ordered complete entries, including source, level, matched, holder position and multiplicity",
        "observedOrders": len(seen), "scoredOrders": sum(row["scoredOrders"] for row in decks),
        "unavailableScoring": unavailable, "decks": decks, "groups": reports,
        "allDecksHave120ScoredOrders": all(row["all120Scored"] for row in decks),
        "groupsWithCoefficientVariation": sum((row["coefficientSpan"] or 0) > 0 for row in reports),
        "maximumAbsoluteProxyScoreResidual": max((row["maximumAbsoluteProxyScoreResidual"] for row in reports), default=None),
        "maximumRelativeProxyScoreResidual": max((row["maximumRelativeProxyScoreResidual"] for row in reports
                                                  if row["maximumRelativeProxyScoreResidual"] is not None), default=None),
    }


def analyze(path, tolerances, fit_outputs):
    source = path.read_bytes()
    document = json.loads(source)
    table = document.get("table", document)
    entries = table["entries"]
    successes = [entry for entry in entries if entry["response"]["status"] == "success"]
    failures = [entry for entry in entries if entry["response"]["status"] != "success"]
    payloads = {}
    for entry in successes:
        curve = entry["response"]["curve"]
        identity = digest(response_payload(curve))
        item = payloads.setdefault(identity, {"curve": curve, "keys": []})
        item["keys"].append(entry["key"])
    fit_rows = []
    for tolerance in tolerances:
        started = time.perf_counter()
        fitted = []
        for identity, item in payloads.items():
            curve = item["curve"]
            fit = fit_curve(curve, tolerance)
            fitted.append({
                "responseSha256": identity,
                "keys": item["keys"],
                "fit": fit,
                "probeTransitions": curve["probeTransitions"],
                "probes": curve["probes"],
                "rangeMoments": curve["rangeMoments"],
            })
        packed = {"context": table["context"], "tolerance": tolerance, "responses": fitted}
        fit_outputs.append(packed)
        fit_rows.append({
            "requestedTolerance": tolerance,
            "uniqueResponses": len(fitted),
            "knots": sum(len(item["fit"]["times"]) for item in fitted),
            "verifiedConstantIntervalEndpoints": sum(item["fit"]["verifiedPoints"] for item in fitted),
            "maximumStoredResidual": max((max(item["fit"]["residual"]) for item in fitted), default=0.0),
            "bytes": sizes(canonical(packed)),
            "elapsedMs": (time.perf_counter() - started) * 1000,
        })
    archives = []
    for mode in ("lossless", "u16", "u24", "u32"):
        archive = Path(str(path) + "." + mode + ".onlrsp")
        if archive.is_file():
            data = archive.read_bytes()
            archives.append({"mode": mode, "sha256": hashlib.sha256(data).hexdigest(), "bytes": sizes(data)})
    job_rows = document.get("jobs", [])
    report = {
        "input": path.name,
        "inputSha256": hashlib.sha256(source).hexdigest(),
        "context": table["context"],
        "entries": len(entries),
        "successfulEntries": len(successes),
        "failedEntries": [{"key": row["key"], "response": row["response"]} for row in failures],
        "uniqueCompletePayloads": len({digest(row["response"]) for row in successes}),
        "uniqueProbabilityResponses": len(payloads),
        "responseAliases": [{"responseSha256": identity, "keys": item["keys"]} for identity, item in payloads.items()],
        "originalSteps": sum(len(row["response"]["curve"]["steps"]) for row in successes),
        "uniqueSteps": sum(len(item["curve"]["steps"]) for item in payloads.values()),
        "generationMs": sum(row.get("elapsedMs", 0.0) for row in job_rows),
        "generatorCache": document.get("cacheStats", job_rows[-1].get("cacheAfter") if job_rows else None),
        "jsonBytes": sizes(canonical(table)),
        "archives": archives,
        "fits": fit_rows,
        "composition": [composition_report(entries, order) for order in (1, 2)],
        "monteCarlo": monte_carlo_report(document, entries),
    }
    scalar = scalar_coefficient_report(document.get("validationDecks"))
    if scalar is not None:
        report["scalarCoefficientAnalysis"] = scalar
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", type=Path, nargs="+")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fits", type=Path, help="optional complete bounded fits; reference response domain only")
    parser.add_argument("--tolerances", default="0.0001,0.001,0.01")
    args = parser.parse_args()
    tolerances = [float(value) for value in args.tolerances.split(",") if value]
    if any(not math.isfinite(value) or value < 0 for value in tolerances):
        parser.error("tolerances must be finite and nonnegative")
    fit_outputs = []
    report = {
        "format": "ournotes-deck.luck-response-analysis/1",
        "scope": "generated real-chart kernel probability data; no whole-score or full-domain ranking certificate",
        "compressors": {"zlib": zlib.ZLIB_VERSION,
                        "zstandard": zstandard.__version__ if zstandard is not None else "unavailable"},
        "contexts": [analyze(path, tolerances, fit_outputs) for path in args.inputs],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(canonical(report) + b"\n")
    if args.fits:
        args.fits.parent.mkdir(parents=True, exist_ok=True)
        args.fits.write_bytes(canonical({"format": "ournotes-deck.luck-response-fits/1", "contexts": fit_outputs}) + b"\n")
    print(json.dumps({
        "contexts": len(report["contexts"]),
        "entries": sum(row["entries"] for row in report["contexts"]),
        "successfulEntries": sum(row["successfulEntries"] for row in report["contexts"]),
        "uniqueProbabilityResponses": sum(row["uniqueProbabilityResponses"] for row in report["contexts"]),
        "generationMs": sum(row["generationMs"] for row in report["contexts"]),
        "output": str(args.output),
    }))


if __name__ == "__main__":
    main()
