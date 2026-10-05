"""Mutation checks for source identities and report comparison; no game model claims."""
import argparse
import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock
import run


class SourceIdentity(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.write("harness-binary", "unchanged executable")
        for name in ("Cargo.toml", "Cargo.lock", "tools/search-harness/Cargo.toml", "tools/search-harness/Cargo.lock",
                     "tools/search-harness/src/main.rs", "crates/ournotes-sim/Cargo.toml",
                     "crates/ournotes-sim/src/lib.rs", "crates/ournotes-search/Cargo.toml",
                     "crates/ournotes-search/src/lib.rs", "crates/ournotes-search/tests/proof.rs"):
            self.write(name, name)
        # A worktree edit does not change HEAD or the previously built executable.
        patcher = mock.patch("run.subprocess.run", return_value=mock.Mock(returncode=0, stdout="fixed-head\n"))
        patcher.start()
        self.addCleanup(patcher.stop)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def identity(self):
        return run.identity(self.binary, self.root)

    def test_workspace_search_and_scorer_edits_change_the_manifest_at_the_same_head(self):
        for name in ("crates/ournotes-sim/src/lib.rs", "crates/ournotes-search/src/lib.rs"):
            with self.subTest(name=name):
                before = self.identity()
                self.write(name, "changed implementation")
                after = self.identity()
                self.assertEqual(before["sourceHead"], after["sourceHead"])
                self.assertEqual(before["binarySha256"], after["binarySha256"])
                self.assertNotEqual(before["sourceManifest"][name], after["sourceManifest"][name])

    def test_workspace_module_additions_and_deletions_change_the_manifest(self):
        before = self.identity()["sourceManifest"]
        name = "crates/ournotes-search/src/new_bound.rs"
        path = self.write(name, "new source")
        added = self.identity()["sourceManifest"]
        self.assertEqual(added[name], run.sha(path))
        self.assertNotEqual(before, added)
        path.unlink()
        self.assertEqual(before, self.identity()["sourceManifest"])

    def test_workspace_manifests_tests_and_harness_locks_are_recorded(self):
        manifest = self.identity()["sourceManifest"]
        for name in ("Cargo.toml", "Cargo.lock", "tools/search-harness/Cargo.toml", "tools/search-harness/Cargo.lock",
                     "tools/search-harness/src/main.rs", "crates/ournotes-sim/Cargo.toml",
                     "crates/ournotes-search/Cargo.toml", "crates/ournotes-search/tests/proof.rs"):
            with self.subTest(name=name):
                self.assertEqual(manifest[name], run.sha(self.root / name))

    def test_build_outputs_and_python_caches_do_not_change_the_source_manifest(self):
        before = self.identity()["sourceManifest"]
        for name in ("crates/ournotes-search/target/output", "tools/search-harness/target/output",
                     "tools/search-harness/__pycache__/run.pyc"):
            self.write(name, "generated output")
        self.assertEqual(before, self.identity()["sourceManifest"])


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
