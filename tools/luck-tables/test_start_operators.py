import itertools
import unittest
from fractions import Fraction

import analyze_start_operators as operators


class StartOperatorsTest(unittest.TestCase):
    def test_native_binary32_cross_multiplicity_identity(self):
        q50, q60, q80 = map(operators.probability, (50, 60, 80))
        self.assertNotEqual(q60, Fraction(3, 5))
        self.assertEqual((1 - q60) * (1 - q50), 1 - q80)
        self.assertEqual(1 - q80, Fraction(3355443, 16777216))
        for minimum in (1, 2):
            self.assertEqual(operators.minimum_pmf([(minimum, q60), (minimum, q50)]),
                             operators.minimum_pmf([(minimum, q80)]))

    def test_polynomial_and_maximum_match_independent_branch_enumeration(self):
        chances = list(map(operators.probability, (10, 30, 50, 60, 80)))
        amounts = (1, 2, 1, 3, 2)
        count, maximum = [Fraction(0)] * 6, [Fraction(0)] * 4
        for enabled in itertools.product((False, True), repeat=5):
            mass = Fraction(1)
            for on, chance in zip(enabled, chances):
                mass *= chance if on else 1 - chance
            count[sum(enabled)] += mass
            maximum[max([0] + [amount for on, amount in zip(enabled, amounts) if on])] += mass
        self.assertEqual(tuple(count), operators.lot_polynomial(chances))
        self.assertEqual(tuple(maximum), operators.minimum_pmf(list(zip(amounts, chances))))
        self.assertEqual(sum(count), 1)
        self.assertEqual(sum(maximum), 1)

    def test_partial_gauge_percentages_are_not_composed_before_native_rounding(self):
        self.assertEqual(operators.native_gauge_delta(50, 500), 2)
        self.assertEqual(operators.native_gauge_delta(50, 1000), 5)
        self.assertNotEqual(2 * operators.native_gauge_delta(50, 500),
                            operators.native_gauge_delta(50, 1000))
        for maximum in (50, 100):
            self.assertEqual(operators.native_gauge_delta(maximum, 10000), maximum)
        with self.assertRaises(ValueError):
            operators.native_gauge_delta(100, 1 << 24)

    def test_layout_keeps_source_multiplicity_formation_and_capacity(self):
        def record(skill, band, matched):
            return {"key": {"source": "gekisouSupport", "id": skill, "level": 1, "matched": matched},
                    "band": band, "action": ("minimum", 2, Fraction(1, 2))}
        # One source cannot occur twice on one native holder; five owners remain distinct.
        single = [(None, [record(1, 1, True)])]
        layout, stopped = operators.bind_supports([0] * 5, single)
        self.assertFalse(stopped)
        self.assertEqual(sorted(position for _, position in layout), list(range(5)))
        self.assertEqual(operators.bind_supports([0] * 6, single), (None, False))
        # Different sources of the same matched band can share the two support slots.
        paired = [(None, [record(1, 1, True), record(2, 1, True)])]
        layout, stopped = operators.bind_supports([0] * 10, paired)
        self.assertFalse(stopped)
        self.assertEqual(len(layout), 10)
        self.assertTrue(all(sum(position == p for _, position in layout) == 2 for p in range(5)))
        self.assertEqual(operators.bind_supports([0] * 11, paired)[0], None)

    def test_unknown_condition_and_non_dyadic_percentage_input_are_refused(self):
        catalogue = operators.Catalogue.__new__(operators.Catalogue)
        catalogue.groups = {1: [[{"_conditionType": 7000, "_conditionValues": [0],
                                "_conditionTargetIDs": [], "_isPositive": True}]]}
        with self.assertRaises(ValueError):
            catalogue.chance(1, 1000, False)
        for value in (-1, 101, 0.5):
            with self.assertRaises(ValueError):
                operators.probability(value)


if __name__ == "__main__":
    unittest.main()
