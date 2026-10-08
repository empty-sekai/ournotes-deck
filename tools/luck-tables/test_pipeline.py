"""Small orchestration tests; no native simulation, benchmark, or build."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("luck_pipeline", Path(__file__).with_name("pipeline.py"))
pipeline = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pipeline)
KEY = {"source": "gekisou", "id": 7, "level": 1, "matched": None}


def master_table(rows):
    if not rows:
        return {"columns": [], "rows": []}
    columns = list(rows[0])
    return {"columns": columns, "rows": [[row[column] for column in columns] for row in rows]}


def tiny_inputs():
    effect = {"_id": 1, "_gekisouSkillID": 7, "_level": 1, "_skillEffectType": 11001,
              "_skillTriggerConditionGroup": 0, "_skillConditionGroup": 0,
              "_skillReleaseConditionGroup": 0, "_effectExecuteLimitResetConditionGroup": 0}
    tables = {
        "MasterMemberCard": [{"_id": 10, "_gekisouSkillID": 7}],
        "MasterSupportCard": [], "MasterSupportCardRank": [],
        "MasterSkillCondition": [{"_id": 2, "_conditionType": 5000}],
        "MasterSkillConditionSet": [{"_id": 4, "_group": 5, "_conditionIds": [2]}],
        "MasterGekisouSkillEffect": [effect], "MasterGekisouSupportSkillEffect": [],
        "MasterLiveMusic": [{"_id": 100, "_easyID": 10000, "_normalID": 10001,
                             "_hardID": 10002, "_expertID": 10003,
                             "_gekisouMission1": 2, "_gekisouMission2": 1, "_gekisouMission3": 3}],
    }
    data = {"master": {name: master_table(rows) for name, rows in tables.items()},
            "charts": [{"scoreId": 10003, "fevers": {"startMs": [100, 200, 300]}}]}
    snapshot = {"format": "ournotes.owned-snapshot/1", "eligible": {
        "members": [{"id": 10, "gekisouSkillLevel": 1}], "snaps": []}}
    request = {"execution": {"kind": "live", "gekisou": True, "scoreId": 10003},
               "scenario": {"kind": "mission", "musicId": 100}}
    return data, snapshot, request


class SelectionTests(unittest.TestCase):
    def test_selected_levels_formation_and_positions(self):
        data, snapshot, _ = tiny_inputs()
        self.assertEqual(pipeline.selected_keys(data, snapshot), [KEY])
        self.assertEqual(len(pipeline.base_jobs([KEY])), 6)
        t = data["master"]["MasterGekisouSkillEffect"]
        t["rows"][0][t["columns"].index("_skillConditionGroup")] = 5
        self.assertEqual([k["matched"] for k in pipeline.selected_keys(data, snapshot)], [False, True])
        del data["master"]["MasterSkillCondition"]
        with self.assertRaises(KeyError):
            pipeline.selected_keys(data, snapshot)

    def test_luck_uses_real_missions_and_chart_relation(self):
        data, _, request = tiny_inputs()
        self.assertTrue(pipeline.luck_case(data, request))
        request["execution"]["scoreId"] = 999
        with self.assertRaises(ValueError):
            pipeline.luck_case(data, request)

    def test_release_formation_is_a_dependency_not_a_catalogue_variant(self):
        data, snapshot, _ = tiny_inputs()
        t = data["master"]["MasterGekisouSkillEffect"]
        t["rows"][0][t["columns"].index("_skillReleaseConditionGroup")] = 5
        self.assertEqual(pipeline.selected_keys(data, snapshot), [KEY])

    def test_explicit_subsets_keep_multiplicity_and_all_positions(self):
        spec = pipeline.read(Path(__file__).with_name("newcomer-combinations.json"))
        jobs = pipeline.extra_jobs(spec, "short-newcomer-score", [KEY])
        self.assertEqual(len(jobs), 26)
        self.assertEqual(sum(len(job["entries"]) == 3 for job in jobs), 10)
        self.assertIn([[KEY, 0], [KEY, 2], [KEY, 4]], [job["entries"] for job in jobs])
        with self.assertRaises(ValueError):
            pipeline.validate_entries([[KEY, 0], [KEY, 0]], [KEY])
        with self.assertRaises(ValueError):
            pipeline.validate_entries([[{**KEY, "level": 2}, 0]], [KEY])


class IdentityTests(unittest.TestCase):
    def test_dependency_invalidation_is_per_job_not_dataset_provenance(self):
        algorithm = {"digest": "code-a"}
        shared = {"chart": "chart-a", "play": [1, 2], "nativeSettings": "rules-a"}
        one, _ = pipeline.task_identity(algorithm, shared, {"skillRows": [7, 1, 100]}, [[KEY, 0]])
        same, _ = pipeline.task_identity(algorithm, copy.deepcopy(shared), {"skillRows": [7, 1, 100]}, [[KEY, 0]])
        changed_level, _ = pipeline.task_identity(algorithm, shared, {"skillRows": [7, 1, 101]}, [[KEY, 0]])
        other_before, _ = pipeline.task_identity(algorithm, shared, {"skillRows": [8, 2, 99]}, [[{**KEY, "id": 8}, 0]])
        other_after, _ = pipeline.task_identity(algorithm, shared, {"skillRows": [8, 2, 99]}, [[{**KEY, "id": 8}, 0]])
        self.assertEqual(one, same)
        self.assertNotEqual(one, changed_level)
        self.assertEqual(other_before, other_after)
        for context, code in [({**shared, "play": [1, 1]}, algorithm),
                              ({**shared, "nativeSettings": "rules-b"}, algorithm), (shared, {"digest": "code-b"})]:
            altered, _ = pipeline.task_identity(code, context, {"skillRows": [7, 1, 100]}, [[KEY, 0]])
            self.assertNotEqual(one, altered)

    def test_order_and_multiplicity_are_identity(self):
        identify = lambda entries: pipeline.task_identity({"digest": "a"}, {}, {}, entries)[0]
        self.assertNotEqual(identify([[KEY, 0]]), identify([[KEY, 0], [KEY, 1]]))
        self.assertNotEqual(identify([[KEY, 0], [KEY, 1]]), identify([[KEY, 1], [KEY, 0]]))

    def test_full_context_is_referenced_not_repeated_per_job(self):
        context = {"frames": list(range(1000))}
        _, descriptor = pipeline.task_identity({"digest": "a"}, context, {"oneRow": 7}, [[KEY, 0]])
        self.assertNotIn("context", descriptor)
        self.assertEqual(descriptor["contextSha256"], pipeline.sha(pipeline.canonical(context)))
        self.assertLess(len(pipeline.canonical(descriptor)), 500)


class StoreTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.generator = self.root / "native-generator"
        self.generator.write_bytes(b"fake native binary identity")
        self.inputs = {field: str(self.root / (field + ".json")) for field in ("data", "snapshot", "request")}
        for path in self.inputs.values():
            pipeline.write(path, {})
        self.jobs = []
        for spec in pipeline.base_jobs([KEY])[:3]:
            dependency = {"selected": spec["entries"]}
            task, descriptor = pipeline.task_identity({"digest": "a"}, {"shared": 1}, dependency, spec["entries"])
            self.jobs.append({"id": task, "descriptor": descriptor, "spec": spec, "referencedBy": ["real-case"]})
        self.plan = {"format": pipeline.PLAN_FORMAT, "generatorSha256": pipeline.sha(self.generator.read_bytes()),
                     "mcRuns": 0, "coverage": {"jobs": 3}, "contexts": [
            {"id": "context", "sharedFingerprint": "shared", "dependencyDescriptor": {"shared": 1},
             "inputs": self.inputs, "cases": [{"inputSha256": pipeline.provenance(self.inputs)}], "jobs": self.jobs}]}
        self.plan_path = self.root / "plan.json"
        pipeline.write(self.plan_path, self.plan)
        self.calls = []

    def tearDown(self):
        self.temporary.cleanup()

    def fake(self, generator, inputs, spec, work, **kwargs):
        self.calls.append(spec)
        rows, entries = [], []
        for job in spec["jobs"]:
            rows.append({**job, "dependencyDescriptor": {"selected": job["entries"]},
                         "status": "success", "entryIndex": len(entries)})
            entries.append({"key": job["entries"], "response": {"status": "success", "curve": {"steps": []}}})
        return {"format": "ournotes-deck.luck-response-generation/1", "sharedFingerprint": "shared",
                "dependencyDescriptor": {"shared": 1}, "context": {"fingerprint": "batch-specific"},
                "jobs": rows, "table": {"entries": entries}, "archiveReceipts": []}

    def run_plan(self, **kwargs):
        return pipeline.run_plan(self.plan_path, self.generator, self.root / "store", batch_size=2, call=self.fake, **kwargs)

    def test_second_run_has_zero_native_jobs_and_corrupt_object_rebuilds_only_one(self):
        first = self.run_plan()
        self.assertEqual((first["stats"]["generated"], len(self.calls)), (3, 2))
        self.calls.clear()
        second = self.run_plan()
        self.assertTrue(second["complete"])
        self.assertEqual((second["stats"]["reused"], len(self.calls)), (3, 0))
        index = pipeline.read(self.root / "store/index.json")
        record = index["tasks"][self.jobs[1]["id"]]
        pipeline.object_path(self.root / "store", record["objectSha256"]).write_bytes(b"corrupt")
        third = self.run_plan()
        self.assertEqual((third["stats"]["generated"], third["stats"]["reused"]), (1, 2))
        self.assertEqual(len(self.calls[0]["jobs"]), 1)

    def test_cancelled_batch_keeps_prior_success_and_resume_only_missing(self):
        original = self.fake
        def interrupted(*args, **kwargs):
            if len(self.calls) == 1:
                raise subprocess.TimeoutExpired("native", 1)
            return original(*args, **kwargs)
        first = pipeline.run_plan(self.plan_path, self.generator, self.root / "store", batch_size=2, call=interrupted)
        self.assertFalse(first["complete"])
        self.assertEqual(first["stats"]["generated"], 2)
        self.calls.clear()
        second = self.run_plan()
        self.assertTrue(second["complete"])
        self.assertEqual((second["stats"]["generated"], second["stats"]["reused"]), (1, 2))

    def test_input_mutation_requires_replanning(self):
        pipeline.write(self.inputs["request"], {"changed": True})
        with self.assertRaises(ValueError):
            self.run_plan()
        self.assertEqual(self.calls, [])

    def test_wrong_entry_identity_never_publishes_success(self):
        original = self.fake
        def wrong(*args, **kwargs):
            report = original(*args, **kwargs)
            report["jobs"][0]["entries"] = [[KEY, 4]]
            return report
        report = pipeline.run_plan(self.plan_path, self.generator, self.root / "store", batch_size=3, call=wrong)
        self.assertFalse(report["complete"])
        self.assertEqual(report["stats"]["generated"], 0)

    def test_atomic_store_leaves_no_partial_temporary_file(self):
        path = self.root / "nested/value.json"
        pipeline.write(path, {"answer": 1})
        pipeline.write(path, {"answer": 2})
        self.assertEqual(pipeline.read(path), {"answer": 2})
        self.assertEqual([p.name for p in path.parent.iterdir()], ["value.json"])

    def test_binary_mutation_requires_replanning(self):
        self.generator.write_bytes(b"different build")
        with self.assertRaises(ValueError):
            self.run_plan()

    def test_public_report_keeps_input_hashes_and_removes_private_paths(self):
        report = self.fake(None, None, {"jobs": [self.jobs[0]["spec"]]}, None)
        report["provenance"] = {"datasetSha256": "dataset", "inputs": {
            "data": "/private/work/data.json", "roster": "/private/work/snapshot.json",
            "request": "/private/work/request.json", "spec": "/private/temp/spec.json", "specSha256": "original-hash"}}
        digest = pipeline.put_report(self.root / "store", report)
        stored = pipeline.read(self.root / "store/reports" / (digest + ".json"))
        self.assertEqual(stored["provenance"]["inputs"]["request"], "request.json")
        self.assertEqual(stored["provenance"]["inputs"]["specSha256"], "original-hash")
        self.assertNotIn("dependencyDescriptor", stored)
        self.assertNotIn("/private/", json.dumps(stored))

    def test_codec_mode_failure_retains_complete_native_entries(self):
        spec = {"mode": "generate", "jobs": [self.jobs[0]["spec"]]}
        def process(command, **kwargs):
            output = Path(command[-1])
            report = self.fake(None, None, spec, None)
            report["archives"] = []
            for mode in ("lossless", "u16", "u24", "u32"):
                if mode == "u16":
                    report["archives"].append({"mode": mode, "status": "error", "error": "codec capacity"})
                    continue
                raw = (mode + " completed archive").encode()
                path = Path(str(output) + "." + mode + ".onlrsp")
                path.write_bytes(raw)
                report["archives"].append({"mode": mode, "status": "success", "path": str(path),
                                           "sha256": pipeline.sha(raw), "bytes": len(raw)})
            pipeline.write(output, report)
            return subprocess.CompletedProcess(command, 0)
        with patch.object(pipeline.subprocess, "run", side_effect=process):
            report = pipeline.native_call(self.generator, self.inputs, spec, self.root / "work", store=self.root / "store")
        self.assertEqual(report["jobs"][0]["status"], "success")
        self.assertEqual(len(report["archiveReceipts"]), 3)
        self.assertEqual(next(a for a in report["archives"] if a["mode"] == "u16")["status"], "error")

    def compact_planner(self, generator, inputs, spec, work):
        self.assertEqual(spec["mode"], "plan")
        report = self.fake(generator, inputs, spec, work)
        report["context"] = {"fingerprint": "complete-native-archive-identity"}
        for row in report["jobs"]:
            row["status"] = "planned"
        return report

    @staticmethod
    def fake_pack(generator, table, mode, work):
        raw = pipeline.canonical({"table": table, "mode": mode})
        return raw, {"mode": mode, "sha256": pipeline.sha(raw), "bytes": len(raw), "verifiedEntries": len(table["entries"])}

    def test_compact_merges_batches_without_generation_and_uses_native_context(self):
        self.run_plan()
        self.calls.clear()
        report = pipeline.compact_plan(self.plan_path, self.generator, self.root / "store", self.root / "compact",
                                       call=self.compact_planner, pack=self.fake_pack)
        self.assertTrue(report["complete"])
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.calls[0]["mode"], "plan")
        self.assertEqual(len(self.calls[0]["jobs"]), 3)
        self.assertEqual(len(report["contexts"][0]["archives"]), 4)
        for archive in report["contexts"][0]["archives"]:
            table = pipeline.read(self.root / "compact" / archive["path"])["table"]
            self.assertEqual(table["context"]["fingerprint"], "complete-native-archive-identity")
            self.assertEqual(len(table["entries"]), 3)

    def test_compact_declines_incomplete_coverage_without_native_calls(self):
        self.run_plan()
        index = pipeline.read(self.root / "store/index.json")
        del index["tasks"][self.jobs[1]["id"]]
        pipeline.write(self.root / "store/index.json", index)
        self.calls.clear()
        report = pipeline.compact_plan(self.plan_path, self.generator, self.root / "store", self.root / "compact",
                                       call=self.compact_planner, pack=self.fake_pack)
        self.assertFalse(report["complete"])
        self.assertEqual(len(report["missingTasks"]), 1)
        self.assertEqual(self.calls, [])

    def test_declared_manifest_can_add_another_difficulty_without_full48_claim(self):
        data, snapshot, request = tiny_inputs()
        pipeline.write(self.inputs["data"], data)
        dataset = pipeline.sha(Path(self.inputs["data"]).read_bytes())
        snapshot["datasetId"] = dataset
        pipeline.write(self.inputs["snapshot"], snapshot)
        pipeline.write(self.inputs["request"], request)
        manifest = {"format": "ournotes-deck.search-benchmark/1", "matrixId": "declared-real-chart",
                    "datasetId": dataset, "cases": [{"name": "other-difficulty", **self.inputs}]}
        manifest_path = self.root / "benchmark.json"
        pipeline.write(manifest_path, manifest)
        source = self.root / "source"
        for name in ("Cargo.toml", "Cargo.lock", "tools/search-harness/Cargo.toml", "tools/search-harness/Cargo.lock",
                     "crates/ournotes-sim/Cargo.toml", "crates/ournotes-sim/build.rs", "crates/ournotes-sim/src/lib.rs"):
            pipeline.atomic_bytes(source / name, b"fixture")
        def planned(generator, inputs, spec, work):
            return {"format": "ournotes-deck.luck-response-generation/1", "sharedFingerprint": "shared",
                    "dependencyDescriptor": {"chart": 10003, "algorithm": {"simSourceSha256": pipeline.sim_source_identity(source)}},
                    "context": {"fingerprint": "one-archive"},
                    "jobs": [{**job, "status": "planned", "dependencyDescriptor": {"entries": job["entries"]}}
                             for job in spec["jobs"]]}
        with self.assertRaises(ValueError):
            pipeline.build_plan(manifest_path, self.generator, source, self.root / "bad-plan.json", call=planned)
        result = pipeline.build_plan(manifest_path, self.generator, source, self.root / "declared-plan.json",
                                     corpus_mode="declared-manifest", call=planned)
        self.assertEqual(result["coverage"]["jobs"], 6)
        self.assertEqual(result["coverage"]["luckCases"], ["other-difficulty"])




class WholeCatalogueTests(unittest.TestCase):
    def test_declared_catalogue_uses_all_native_levels_and_refuses_omission(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data, snapshot, request = tiny_inputs()
            pipeline.write(root / 'data.json', data)
            digest = pipeline.sha((root / 'data.json').read_bytes())
            snapshot['datasetId'] = digest
            pipeline.write(root / 'snapshot.json', snapshot)
            pipeline.write(root / 'request.json', request)
            generator = root / 'generator'; generator.write_bytes(b'generator')
            key2 = {**KEY, 'level': 2}
            keys = [KEY, key2]
            manifest = {'format': 'ournotes-deck.search-benchmark/1', 'datasetId': digest,
                        'cases': [{'name': 'all', 'data': 'data.json', 'snapshot': 'snapshot.json', 'request': 'request.json'}],
                        'luckCatalogue': {'format': 'ournotes-deck.luck-catalogue/1',
                                          'selection': 'all-master-source-levels', 'keys': keys}}
            pipeline.write(root / 'benchmark.json', manifest)
            calls = []

            def native(_generator, _inputs, spec, _work):
                calls.append(spec)
                return {'capabilities': {'chain': keys}, 'sharedFingerprint': 'same',
                        'dependencyDescriptor': {'algorithm': {'simSourceSha256': 'native-source'}},
                        'jobs': [{**job, 'status': 'planned', 'dependencyDescriptor': {'key': job['entries']}}
                                 for job in spec['jobs']]}

            with patch.object(pipeline, 'source_identity', return_value={'digest': 'a', 'simSourceSha256': 'native-source'}):
                plan = pipeline.build_plan(root / 'benchmark.json', generator, root, root / 'plan.json',
                                           corpus_mode='declared-manifest', call=native)
                self.assertEqual(plan['coverage']['jobs'], 11)
                self.assertTrue(plan['coverage']['nativeCatalogueValidated'])
                self.assertTrue(any(job['entries'] == [[key2, 4]] for job in calls[0]['jobs']))
                manifest['luckCatalogue']['keys'] = [KEY]
                pipeline.write(root / 'benchmark.json', manifest)
                with self.assertRaisesRegex(ValueError, 'full catalogue differs'):
                    pipeline.build_plan(root / 'benchmark.json', generator, root, root / 'bad.json',
                                        corpus_mode='declared-manifest', call=native)


if __name__ == "__main__":
    unittest.main()
