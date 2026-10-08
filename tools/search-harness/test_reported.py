"""Reported source preservation and export gating. Native audit is mocked, never claimed to run here."""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import reported
from matrix import digest, read_json, write_json


class ReportedContracts(unittest.TestCase):
    def test_original_fixture_bytes_and_explicit_facts(self):
        roster, provenance = reported.original_inputs()
        self.assertFalse(provenance["originalDatasetIdentityProvided"])
        self.assertIn("synthetic reproduction roster; not a player account", provenance["inventoryKind"])
        data = {"master": {"MasterCharacter": {"columns": ["_id"], "rows": [[i] for i in range(1, 26)]}}}
        value = reported.derive_snapshot(roster, data, "a" * 64)
        self.assertEqual(value["eligible"], {k: roster[k] for k in ["members", "snaps"]})
        self.assertEqual(value["ownedFacts"]["memberIds"], roster["player"]["ownedMemberCardIds"])
        self.assertEqual(value["ownedFacts"]["snapIds"], roster["player"]["ownedSupportCardIds"])
        self.assertEqual(value["ownedFacts"]["memberCoverage"], "partial")
        self.assertEqual(value["ownedFacts"]["snapCoverage"], "partial")
        self.assertEqual(sum(row["value"] for row in value["player"]["characterRanks"]["values"]), 916)
        self.assertIsNone(value["player"]["characterTotalRank"])
        self.assertEqual(value["player"]["memory"], {"musicRanks": [], "unlockedMembers": [], "unlockedSnaps": []})
        self.assertEqual(value["assumptions"], [])
        data["master"]["MasterCharacter"]["rows"].append([26])
        with self.assertRaises(ValueError):
            reported.derive_snapshot(roster, data, "a" * 64)
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory)
            for name in ["request", "roster", "provenance"]:
                (fixture / f"{name}.json").write_bytes((reported.FIXTURE / f"{name}.json").read_bytes())
            original = (fixture / "request.json").read_bytes()
            for changed in [original + b"\n", original.replace(b'"k": 5', b'"k": 3'),
                            original.replace(b'"cacheEntries": 2048', b'"cacheEntries": 1024')]:
                (fixture / "request.json").write_bytes(changed)
                with self.assertRaises(ValueError):
                    reported.original_inputs(fixture)

    def test_manifest_only_published_after_complete_native_audit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = root / "data.json"
            write_json(data, {"master": {"MasterCharacter": {"columns": ["_id"], "rows": [[i] for i in range(1, 26)]}}})
            binary = root / "audit"
            binary.write_bytes(b"unit-test stub, not a native audit")
            args = argparse.Namespace(data=data, offline=True, cache_dir=root, audit_binary=str(binary),
                                      no_build=True, out=root / "success")
            roster, _ = reported.original_inputs()

            def audit_stub(command, **_kwargs):
                output = Path(command[-1])
                write_json(output, {"format": "ournotes-deck.reported-projection/1", "datasetId": digest(data),
                                    "strictResolution": True, "parsedRosterEqual": True, "poolFieldsEqual": True,
                                    "candidateDomainEqual": True, "memberIds": roster["player"]["ownedMemberCardIds"],
                                    "snapIds": roster["player"]["ownedSupportCardIds"],
                                    "leaderIds": roster["player"]["ownedMemberCardIds"]})

            with patch.object(reported, "acquire", return_value=(data, {"datasetId": digest(data)})), \
                    patch.object(reported.subprocess, "run", side_effect=audit_stub) as invoked:
                manifest = reported.prepare(args)
                self.assertTrue(manifest.is_file())
                self.assertEqual(invoked.call_count, 1)
                for name in ["request", "roster"]:
                    self.assertEqual((args.out / f"{name}.json").read_bytes(),
                                     (reported.FIXTURE / f"{name}.json").read_bytes())
                self.assertEqual(read_json(manifest)["matrixId"], "reported")
                self.assertEqual(len(read_json(manifest)["cases"]), 1)
                self.assertTrue(read_json(args.out / "preparation-receipt.json")["nativeProjectionPassed"])
                with self.assertRaises(FileExistsError):
                    reported.prepare(args)
            for failure in ["exit", "domain", "leaders"]:
                failed = copy.copy(args)
                failed.out = root / failure

                def failed_audit(command, **kwargs):
                    if failure == "exit":
                        raise subprocess.CalledProcessError(1, command)
                    audit_stub(command, **kwargs)
                    value = read_json(command[-1])
                    value["candidateDomainEqual" if failure == "domain" else "leaderIds"] = False
                    write_json(Path(command[-1]), value)

                with patch.object(reported, "acquire", return_value=(data, {"datasetId": digest(data)})), \
                        patch.object(reported.subprocess, "run", side_effect=failed_audit):
                    with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                        reported.prepare(failed)
                    self.assertFalse((failed.out / "benchmark.json").exists())


if __name__ == "__main__":
    unittest.main()
