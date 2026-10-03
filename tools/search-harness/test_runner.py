"""Mutation checks for the report comparison; no game model claims."""
import argparse
import copy
import json
import tempfile
import unittest
from pathlib import Path
import run


class ComparisonGate(unittest.TestCase):
    def invoke(self, before, after):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run.dump(root / "a.json", before)
            run.dump(root / "b.json", after)
            run.compare(argparse.Namespace(baseline=root / "a.json",candidate=root / "b.json",out=root / "out.json"))
            return json.loads((root / "out.json").read_text())

    def fixture(self):
        result = {"members":[1,2,3,4,5],"snaps":[1,None,None,None,None],
                  "expectedPayoff":{"numerator":"9007199254740993","denominator":"7"}}
        return {"case":"mutation-check","inputIdentity":"same", "oracle":{"topK":[result]},
                "experiments":[{"name":"exact","patch":{},"samples":[{"wallMs":2,
                    "outcome":{"results":[copy.deepcopy(result)]}}]}]}

    def test_same_score_different_snap_is_a_wrong_result(self):
        a=self.fixture(); b=copy.deepcopy(a)
        b["experiments"][0]["samples"][0]["outcome"]["results"][0]["snaps"][0]=2
        with self.assertRaisesRegex(ValueError,"returned results differ"):
            self.invoke(a,b)

    def test_one_integer_above_js_safe_range_is_not_rounded_away(self):
        a=self.fixture(); b=copy.deepcopy(a)
        b["oracle"]["topK"][0]["expectedPayoff"]["numerator"]="9007199254740992"
        with self.assertRaisesRegex(ValueError,"reference changed"):
            self.invoke(a,b)

    def test_changed_inputs_are_rejected(self):
        a=self.fixture(); b=copy.deepcopy(a); b["inputIdentity"]="new-law"
        with self.assertRaisesRegex(ValueError,"input identities differ"):
            self.invoke(a,b)

    def test_timing_change_alone_passes(self):
        a=self.fixture(); b=copy.deepcopy(a); b["experiments"][0]["samples"][0]["wallMs"]=1
        self.assertTrue(self.invoke(a,b)["passed"])

    def test_changed_schedule_is_rejected(self):
        a=self.fixture(); b=copy.deepcopy(a)
        b["experiments"][0]["schedule"]="classesWithResource"
        with self.assertRaisesRegex(ValueError,"experiment contracts differ"):
            self.invoke(a,b)


if __name__ == "__main__":
    unittest.main()
