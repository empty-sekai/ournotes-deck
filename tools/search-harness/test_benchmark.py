"""Mutation checks for paired completion reports and exact certificate comparison."""
import json
import subprocess
import unittest
from pathlib import Path


class CompletionComparison(unittest.TestCase):
    def invoke(self, left, right):
        script = """
const fs = require('node:fs');
const { compareAnswers } = require('./benchmark.cjs');
const [left, right] = JSON.parse(fs.readFileSync(0, 'utf8'));
console.log(JSON.stringify(compareAnswers(left, right)));
"""
        result = subprocess.run(
            ["node", "-e", script], input=json.dumps([left, right]), text=True,
            capture_output=True, cwd=Path(__file__).parent, check=False,
        )
        return result.returncode, result.stdout, result.stderr

    @staticmethod
    def answer(completion="Complete", member=1, lower=1, upper=2):
        value = {
            "result": {
                "completion": completion,
                "results": [{
                    "members": [member, 2, 3, 4, 5], "snaps": [None] * 5, "power": 100,
                    "payoffInterval": {
                        "lower": {"numerator": str(lower), "denominator": "3"},
                        "upper": {"numerator": str(upper), "denominator": "3"},
                    },
                }],
            },
        }
        return json.dumps(value)

    def test_distinct_large_member_ids_cannot_compare_equal(self):
        code, _, error = self.invoke(self.answer(member=9007199254740992), self.answer(member=9007199254740993))
        self.assertNotEqual(code, 0)
        self.assertIn("canonical Top-K changed", error)

    def test_narrower_certificates_preserve_complete_order(self):
        code, output, error = self.invoke(self.answer(lower=1, upper=3), self.answer(lower=2, upper=2))
        self.assertEqual(code, 0, error)
        self.assertTrue(json.loads(output)["canonicalTopKEqual"])

    def test_disjoint_large_rational_endpoints_are_rejected(self):
        code, _, error = self.invoke(
            self.answer(lower=9007199254740992, upper=9007199254740992),
            self.answer(lower=9007199254740993, upper=9007199254740993),
        )
        self.assertNotEqual(code, 0)
        self.assertIn("disjoint payoff certificates", error)

    def test_equal_incumbents_do_not_certify_an_incomplete_run(self):
        code, output, error = self.invoke(self.answer("TimedOut"), self.answer())
        self.assertEqual(code, 0, error)
        result = json.loads(output)
        self.assertFalse(result["bothComplete"])
        self.assertIsNone(result["canonicalTopKEqual"])
        self.assertEqual(result["sharedTeams"], 1)

    def test_incomplete_shared_teams_still_require_compatible_certificates(self):
        code, _, error = self.invoke(self.answer("TimedOut", lower=0, upper=1), self.answer(lower=2, upper=3))
        self.assertNotEqual(code, 0)
        self.assertIn("disjoint payoff certificates", error)

    def test_probability_law_changes_are_rejected_for_incomplete_runs(self):
        left = json.loads(self.answer("TimedOut"))
        right = json.loads(self.answer("TimedOut"))
        left["result"]["probabilityLaw"] = {"orders": 120, "lottery": "certifiedNativeLotteryIntervals"}
        right["result"]["probabilityLaw"] = {"orders": 120, "lottery": "none"}
        code, _, error = self.invoke(json.dumps(left), json.dumps(right))
        self.assertNotEqual(code, 0)
        self.assertIn("probabilityLaw semantics changed", error)

    def test_exact_expectations_override_coarse_enclosures(self):
        left = json.loads(self.answer(lower=0, upper=3))
        right = json.loads(self.answer(lower=0, upper=3))
        left["result"]["results"][0]["expectedPayoff"] = {"numerator": "1", "denominator": "3"}
        right["result"]["results"][0]["expectedPayoff"] = {"numerator": "2", "denominator": "3"}
        code, _, error = self.invoke(json.dumps(left), json.dumps(right))
        self.assertNotEqual(code, 0)
        self.assertIn("disjoint payoff certificates", error)


if __name__ == "__main__":
    unittest.main()
