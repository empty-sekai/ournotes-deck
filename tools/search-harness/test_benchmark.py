"""Mutation checks for paired completion reports and exact certificate comparison."""
import json
import os
import subprocess
import tempfile
import textwrap
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


class BenchmarkModes(unittest.TestCase):
    def test_paired_arguments_keep_the_default_repeats_and_alternating_order(self):
        script = """
const assert = require('node:assert/strict');
const {parseArguments, runOrder} = require('./benchmark.cjs');
const paired = parseArguments(['suite.json', 'baseline', 'candidate', 'out']);
assert.equal(paired.repeats, 2);
assert.equal(paired.candidateOnly, false);
assert.deepEqual(runOrder(false, 0, 0), ['baseline', 'candidate']);
assert.deepEqual(runOrder(false, 0, 1), ['candidate', 'baseline']);
const single = parseArguments(['--candidate-only', 'suite.json', 'candidate', 'out', '--repeats', '3']);
assert.equal(single.repeats, 3);
assert.equal(single.baseline, null);
assert.throws(() => parseArguments(['suite.json', 'candidate', 'out', '--candidate-only', '--repeats']));
"""
        result = subprocess.run(["node", "-e", script], cwd=Path(__file__).parent,
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipIf(os.name == "nt", "uses a POSIX native executable fixture")
    def test_candidate_only_executes_each_case_once_and_preserves_incomplete_status(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "native-fixture"
            binary.write_text(textwrap.dedent("""\
                #!/usr/bin/env python3
                import hashlib
                import json
                import sys
                from pathlib import Path
                data, snapshot, request, output = map(Path, sys.argv[1:])
                with (Path(__file__).parent / "calls.jsonl").open("a") as calls:
                    calls.write(json.dumps(request.name) + "\\n")
                complete = request.stem == "complete"
                request_value = json.loads(request.read_text())
                result = {
                    "metric": request_value["metric"], "strategy": request_value["strategy"],
                    "completion": "Complete" if complete else "TimedOut",
                    "optimality": "proven" if complete else "unproven",
                    "exitReason": "exhausted" if complete else "timeLimit",
                    "results": [{"members": [1, 2, 3, 4, 5], "snaps": [None] * 5,
                                 "power": 100, "rankCertified": complete}],
                    "telemetry": {"proof": {"complete": complete}, "memory": {"peakBytes": 100}, "nodes": {},
                                  "leaves": {"visited": 1, "simulations": 1}, "caches": {},
                                  "lotteryRefinement": {"frames": 0}, "phases": {}, "time": {}}
                }
                output.write_text(json.dumps({"datasetId": hashlib.sha256(data.read_bytes()).hexdigest(),
                                             "status": "ok", "errors": [], "missing": [], "result": result}))
                print(json.dumps({"elapsedMs": 10 if complete else 60}))
            """), encoding="utf-8")
            binary.chmod(0o755)
            (root / "data.json").write_text("{}\n", encoding="utf-8")
            (root / "snapshot.json").write_text("{}\n", encoding="utf-8")
            cases = []
            for name in ["complete", "timeout"]:
                (root / f"{name}.json").write_text(json.dumps({
                    "k": 1, "limits": {"timeLimitMs": 50, "cacheEntries": 0},
                    "metric": {"kind": "score"}, "strategy": {"kind": "branchAndBound"},
                }), encoding="utf-8")
                cases.append({"name": name, "family": "fixture", "metric": "score",
                              "data": "data.json", "snapshot": "snapshot.json", "request": f"{name}.json"})
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"format": "ournotes-deck.search-benchmark/1",
                                            "requestTimeLimitMs": 50, "cases": cases}), encoding="utf-8")
            result = subprocess.run(["node", str(Path(__file__).with_name("benchmark.cjs")),
                                     str(manifest), str(binary), str(root / "out"), "--candidate-only"],
                                    capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / "calls.jsonl").read_text().splitlines(), ['"complete.json"', '"timeout.json"'])
            report = json.loads((root / "out/report.json").read_text())
            self.assertEqual(report["mode"], "candidateOnly")
            self.assertEqual(report["repeats"], 1)
            self.assertEqual(list(report["variants"]), ["candidate"])
            self.assertEqual(report["comparisons"], [])
            self.assertEqual([row["completion"] for row in report["rows"]], ["Complete", "TimedOut"])
            summary = report["summary"]["candidate"]
            self.assertEqual((summary["requests"], summary["complete"], summary["provenWithinBudget"]), (2, 1, 1))
            self.assertFalse(summary["allComplete"])
            self.assertFalse(summary["allProvenWithinBudget"])
            self.assertTrue(report["passed"])
            self.assertEqual(report["passedScope"], "executionAndAnswerContracts")
            self.assertEqual(report["independentOracle"], "notRun")


if __name__ == "__main__":
    unittest.main()
