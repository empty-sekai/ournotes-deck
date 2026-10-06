import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from run_matrix import FORMAT, decode, run


class FrameMatrixTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.context = dict(region="synthetic", clientVersion="fixture", masterVersion="fixture",
                            scenario="free", play="perfect")
        self.capture = dict(chartId=1, inputIdentity="synthetic-input", context=self.context,
                            frames=[dict(frame=0, timeMs=0, score=0), dict(frame=1, timeMs=20, score=7)])
        self.model = {**copy.deepcopy(self.capture), "modelSourceSha256": "a" * 64}
        self.contract = dict(integerFields=["frame", "timeMs", "score"])
        self.case = dict(id="free-perfect", reference="reference.json", model="model.json", contract="contract.json",
                         inputIdentity="synthetic-input", context=self.context, frameCount=2, finalTimeMs=20)
        self.manifest = dict(format=FORMAT, modelSourceSha256="a" * 64, cases=[self.case])
        self.write()

    def write(self):
        for name, value in (("reference", self.capture), ("model", self.model), ("contract", self.contract)):
            (self.directory / f"{name}.json").write_text(json.dumps(value), encoding="utf-8")

    def test_json_fields_and_numbers_are_unambiguous(self):
        for text in ('{"score":1,"score":2}', '{"score":NaN}', '{"score":Infinity}'):
            with self.assertRaises(ValueError):
                decode(text)

    def test_complete_source_linked_trajectory(self):
        result = run(self.manifest, self.directory)
        self.assertEqual((result["status"], result["caseCount"], result["fieldComparisons"]), ("passed", 1, 6))
        self.assertEqual(set(result["cases"][0]["resources"]), {"reference", "model", "contract"})
        self.assertNotIn(str(self.directory), json.dumps(result))

    def test_changed_source_or_context_is_incomplete(self):
        for change in ("source", "input", "context"):
            with self.subTest(change=change):
                actual = copy.deepcopy(self.model)
                if change == "source":
                    actual["modelSourceSha256"] = "b" * 64
                elif change == "input":
                    actual["inputIdentity"] = "other-input"
                else:
                    actual["context"]["scenario"] = "mission"
                (self.directory / "model.json").write_text(json.dumps(actual), encoding="utf-8")
                self.assertEqual(run(self.manifest, self.directory)["status"], "incomplete")

    def test_every_case_is_required_and_visited(self):
        self.manifest["cases"].append({**self.case, "id": "second", "reference": "absent.json"})
        result = run(self.manifest, self.directory)
        self.assertEqual(result["counts"]["passed"], 1)
        self.assertEqual(result["counts"]["not_run"], 1)
        self.assertEqual(result["status"], "incomplete")

    def test_contract_validation_preserves_later_case_results(self):
        (self.directory / "valid-contract.json").write_text(json.dumps(self.contract), encoding="utf-8")
        self.manifest["cases"].append({**self.case, "id": "second", "contract": "valid-contract.json"})
        malformed = [
            None,
            [],
            {"integerFields": "frame,timeMs,score"},
            {"integerFields": ["frame", "timeMs", 123]},
            {**self.contract, "float32BitFields": "score"},
            {**self.contract, "float32BitFields": [123]},
            {**self.contract, "arrayLengths": []},
            {**self.contract, "arrayLengths": {"skills": True}},
        ]
        for contract in malformed:
            with self.subTest(contract=contract):
                (self.directory / "contract.json").write_text(json.dumps(contract), encoding="utf-8")
                result = run(self.manifest, self.directory)
                self.assertEqual(result["status"], "incomplete")
                self.assertEqual(result["counts"]["invalid"], 1)
                self.assertEqual(result["counts"]["passed"], 1)
                self.assertEqual([(case["id"], case["status"]) for case in result["cases"]],
                                 [("free-perfect", "invalid"), ("second", "passed")])

    def test_cli_writes_all_case_outcomes_for_an_invalid_contract(self):
        (self.directory / "valid-contract.json").write_text(json.dumps(self.contract), encoding="utf-8")
        self.manifest["cases"].append({**self.case, "id": "second", "contract": "valid-contract.json"})
        self.contract["arrayLengths"] = []
        self.write()
        manifest_path = self.directory / "manifest.json"
        output_path = self.directory / "result.json"
        manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")
        completed = subprocess.run(
            [sys.executable, str(Path(__file__).with_name("run_matrix.py")), str(manifest_path), str(output_path)],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(completed.returncode, 2, completed.stderr)
        report = json.loads(output_path.read_text(encoding="utf-8"))
        self.assertEqual(report["caseCount"], 2)
        self.assertEqual([case["status"] for case in report["cases"]], ["invalid", "passed"])
        self.assertEqual(json.loads(completed.stdout)["status"], "incomplete")

    def test_frame_sequence_and_terminal_coverage_are_checked(self):
        for frames in (self.capture["frames"][:1], [dict(frame=1, timeMs=0, score=0), dict(frame=2, timeMs=20, score=7)],
                       [dict(frame=0, timeMs=40, score=0), dict(frame=1, timeMs=20, score=7)]):
            self.capture["frames"] = frames
            self.model["frames"] = copy.deepcopy(frames)
            self.write()
            self.assertEqual(run(self.manifest, self.directory)["status"], "incomplete")

    def test_declared_state_fields_select_actual_observations(self):
        self.contract["integerFields"] = ["frame", "timeMs", "skills.*.state"]
        for capture in (self.capture, self.model):
            for frame in capture["frames"]:
                frame["skills"] = []
        self.write()
        self.assertEqual(run(self.manifest, self.directory)["status"], "incomplete")

    def test_numeric_difference_is_retained(self):
        self.model["frames"][-1]["score"] += 1
        self.write()
        result = run(self.manifest, self.directory)
        self.assertEqual(result["status"], "different")
        self.assertEqual(result["cases"][0]["differenceCount"], 1)

    def test_empty_duplicate_and_clock_only_contracts_have_explicit_outcomes(self):
        for cases in ([], [self.case, self.case]):
            with self.assertRaises(ValueError):
                run({**self.manifest, "cases": cases}, self.directory)
        self.contract["integerFields"] = ["frame", "timeMs"]
        self.write()
        self.assertEqual(run(self.manifest, self.directory)["status"], "incomplete")


if __name__ == "__main__":
    unittest.main()
