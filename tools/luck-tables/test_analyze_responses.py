"""Small independent checks of response-analysis math; no solver or chart simulation."""

import importlib.util
import itertools
import math
from pathlib import Path
import unittest
from fractions import Fraction


SPEC = importlib.util.spec_from_file_location("analyze_responses", Path(__file__).with_name("analyze_responses.py"))
analysis = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(analysis)


def curve(rows):
    return {"steps": [{"timeMs": time, "buckets": [[value, value] for value in values]} for time, values in rows]}


def skill(source, identifier, position=0):
    return [{"source": source, "id": identifier, "level": 2, "matched": None}, position]


def entry(key, rows):
    return {"key": key, "response": {"status": "success", "curve": curve(rows)}}


def exact_fit_value(fit, t):
    """Independent exact real interpolation of the stored binary64 ordinates."""
    times, values = fit["times"], fit["values"]
    if len(times) == 1:
        return [Fraction(value) for value in values[0]]
    for left in range(len(times) - 1):
        if times[left] <= t <= times[left + 1]:
            fraction = Fraction(t - times[left], times[left + 1] - times[left])
            return [Fraction(a) + (Fraction(b) - Fraction(a)) * fraction for a, b in zip(values[left], values[left + 1])]
    raise AssertionError("outside the declared fit domain")


class ResponseAnalysisTests(unittest.TestCase):
    def test_piecewise_fit_encloses_every_integer_millisecond_and_both_sides_of_jumps(self):
        original = curve([
            (-8, [0.0, 0.125, 0.25, 0.625]),
            (-7, [0.5, 0.0, 0.375, 0.125]),
            (-1, [0.0, 1.0, 0.0, 0.0]),
            (0, [0.125, 0.25, 0.25, 0.375]),
            (2, [0.0, 0.0, 1.0, 0.0]),
            (13, [0.1, 0.2, 0.3, 0.4]),
            (31, [1.0, 0.0, 0.0, 0.0]),
        ])
        original["steps"][5]["buckets"][2] = [0.3, 0.3 + 1e-10]
        for tolerance in [0.0, 0.03, 0.2, 1.0]:
            fit = analysis.fit_curve(original, tolerance)
            self.assertEqual(fit["domain"], [-8, 31])
            for t in range(-8, 32):
                bounds = next(step["buckets"] for step in reversed(original["steps"]) if step["timeMs"] <= t)
                exact = exact_fit_value(fit, t)
                for bucket in range(4):
                    radius = Fraction(fit["residual"][bucket])
                    self.assertLessEqual(exact[bucket] - radius, Fraction(bounds[bucket][0]))
                    self.assertGreaterEqual(exact[bucket] + radius, Fraction(bounds[bucket][1]))
            # A fit carries no implicit certificate outside its explicit domain.
            with self.assertRaises(AssertionError):
                exact_fit_value(fit, 32)

    def test_fit_empty_single_point_i32_extremes_and_rejected_numeric_domains(self):
        self.assertEqual(analysis.fit_curve(curve([]), 0.1)["times"], [])
        single = analysis.fit_curve(curve([(7, [0.1, 0.2, 0.3, 0.4])]), 0.1)
        self.assertEqual(single["domain"], [7, 7])
        large = curve([(-(2**31), [0.0] * 4), (2**31 - 1, [1.0] * 4)])
        fit = analysis.fit_curve(large, 1.0)
        for t in [-(2**31), -(2**31) + 1, -1, 0, 1, 2**31 - 2, 2**31 - 1]:
            observed = 1.0 if t == 2**31 - 1 else 0.0
            for value, radius in zip(exact_fit_value(fit, t), fit["residual"]):
                self.assertLessEqual(abs(value - Fraction(observed)), Fraction(radius))
        for time in [True, 0.5, -(2**31) - 1, 2**31]:
            with self.assertRaises(ValueError):
                analysis.fit_curve(curve([(time, [0.0] * 4)]), 0.1)
        for rows in [[(1, [0.0] * 4), (1, [0.0] * 4)], [(2, [0.0] * 4), (1, [0.0] * 4)]]:
            with self.assertRaises(ValueError):
                analysis.fit_curve(curve(rows), 0.1)
        for bounds in [[math.nan, 1.0], [0.0, math.inf], [-0.1, 0.1], [0.0, 1.1], [0.2, 0.1], [False, 1.0]]:
            invalid = curve([(0, [0.0] * 4)])
            invalid["steps"][0]["buckets"][0] = bounds
            with self.assertRaises(ValueError):
                analysis.fit_curve(invalid, 0.1)

    def test_mobius_single_and_pair_terms_match_independent_quadratic_response(self):
        keys = [skill("gekisou", identifier, identifier - 1) for identifier in [1, 2, 3]]
        entries = []
        for size in range(4):
            for subset in itertools.combinations(range(3), size):
                # Dyadic polynomial: base 1/8, individual effects 1/32, pair effects 1/64.
                p = 1 / 8 + size / 32 + math.comb(size, 2) / 64
                entries.append(entry([keys[i] for i in subset], [(0, [1 - p, 0, p, 0]), (10, [1 - p, 0, p, 0])]))
        first = analysis.composition_report(entries, 1)
        triple = next(row for row in first["combinations"] if len(row["entries"]) == 3)
        self.assertEqual(triple["maximumRushResidual"], 3 / 64)
        second = analysis.composition_report(entries, 2)
        self.assertEqual(second["measuredCombinations"], 1)
        self.assertEqual(second["maximumBucketResidual"], 0.0)

    def test_repeated_skills_keep_multiplicity_and_source_order_missing_subsets_stay_unknown(self):
        repeated = skill("gekisouSupport", 69)
        entries = [entry([repeated] * n, [(0, [1 - n / 8, 0, n / 8, 0])]) for n in range(4)]
        self.assertEqual(analysis.composition_report(entries, 1)["maximumRushResidual"], 0.0)
        self.assertEqual(analysis.composition_report(entries, 2)["maximumRushResidual"], 0.0)
        a, b, c = [skill("gekisouSupport", identifier) for identifier in [1, 2, 3]]
        ordered = [entry([], [(0, [1, 0, 0, 0])])]
        ordered += [entry([key], [(0, [1, 0, 0, 0])]) for key in [a, b, c]]
        ordered += [entry(key, [(0, [1, 0, 0, 0])]) for key in [[b, a], [a, c], [b, c], [a, b, c]]]
        result = analysis.composition_report(ordered, 2)
        self.assertEqual(result["combinations"][0]["status"], "missingSubsets")
        self.assertIn([a, b], result["combinations"][0]["missing"])

    def test_breakpoint_and_time_weighted_rms_use_different_measures(self):
        a, b = skill("gekisou", 7), skill("gekisouSupport", 69)
        entries = [entry(key, [(0, [1, 0, 0, 0]), (100, [1, 0, 0, 0])]) for key in [[], [a], [b]]]
        entries.append(entry([a, b], [(0, [1, 0, 0, 0]), (100, [0, 0, 1, 0])]))
        row = analysis.composition_report(entries, 1)["combinations"][0]
        self.assertEqual(row["domain"], [0, 100])
        self.assertEqual(row["checkedIntegerMilliseconds"], 101)
        self.assertEqual(row["breakpointBucketRms"][2], math.sqrt(1 / 2))
        self.assertEqual(row["timeWeightedBucketRms"][2], math.sqrt(1 / 101))
        self.assertNotIn("bucketRms", row)

    def test_scalar_constant_cannot_represent_two_ordinary_timings_with_same_writer_key(self):
        writer = skill("gekisou", 7, 0)
        orders = []
        # The same holder/key sees two ordinary-skill timings. Their implied c
        # values are 1/4 and 3/4; full-score LS weights them by 100^2 and 200^2.
        for order, s1, score in [([0, 1, 2, 3, 4], 200, 125), ([0, 2, 1, 3, 4], 300, 250)]:
            orders.append({"order": order, "entries": [writer], "scoring": {
                "status": "success", "s0": 100, "s1": s1, "scoreAtMean": score,
                "scoreAtMeanIsExactExpectation": False,
            }})
        deck = {"name": "timing observations", "members": [1, 2, 3, 4, 5], "snaps": [None] * 5, "orders": orders}
        report = analysis.scalar_coefficient_report([deck])
        self.assertEqual(report["scoredOrders"], 2)
        self.assertFalse(report["allDecksHave120ScoredOrders"])
        group = report["groups"][0]
        self.assertEqual(group["entries"], [writer])
        self.assertEqual(group["minimumCoefficient"], 1 / 4)
        self.assertEqual(group["maximumCoefficient"], 3 / 4)
        self.assertEqual(group["exactFittedFraction"], {"numerator": 13, "denominator": 20})
        self.assertEqual(group["maximumAbsoluteProxyScoreResidual"], 40)
        self.assertEqual(group["meanAbsoluteProxyScoreResidual"], 30)
        self.assertEqual(group["maximumRelativeProxyScoreResidual"], 40 / 125)
        self.assertIsNone(analysis.scalar_coefficient_report([]))
        with self.assertRaises(ValueError):
            analysis.scalar_coefficient_report([deck, deck])
        support = skill("gekisouSupport", 69, 0)
        reordered = [dict(orders[0], order=order, entries=entries) for order, entries in [
            ([0, 1, 3, 2, 4], [writer, support]),
            ([0, 1, 4, 2, 3], [support, writer]),
        ]]
        groups = analysis.scalar_coefficient_report([dict(deck, orders=orders + reordered)])["groups"]
        self.assertEqual(len(groups), 3, "same-holder source order must not collapse into one coefficient key")


if __name__ == "__main__":
    unittest.main()
