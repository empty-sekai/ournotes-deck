"""Parsing and comparison contracts only; these fixtures make no simulation or performance claim."""
import copy
import importlib.util
import inspect
import itertools
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("interaction_validation_tests", Path(__file__).with_name("validate_interactions.py"))
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)
SOURCE = "a" * 64
DECKS = [{"name": "parsing-fixture", "members": [1, 2, 3, 4, 5], "snaps": [None] * 5}]
HASHES = {name: str(index) * 64 for index, name in enumerate(("data", "snapshot", "request", "decks"), 1)}


def curve(steps=None, probes=None):
    return {"steps": steps if steps is not None else [{"timeMs": 0, "buckets": copy.deepcopy(subject.EMPTY)}],
            "probes": probes if probes is not None else [True], "probeTransitions": [1], "rangeMoments": [],
            "peakStates": 1, "transitions": 1}


def provenance(spec_sha):
    return {"datasetSha256": HASHES["data"], "inputs": {"rosterSha256": HASHES["snapshot"],
            "requestSha256": HASHES["request"], "specSha256": spec_sha, "decksSha256": HASHES["decks"]}}


def reference():
    spec_sha = subject.pipeline.sha(subject.reference_bytes(DECKS))
    rows = [{"order": list(order), "ordinal": ordinal, "entries": [],
             "actual": {"status": "success", "curve": curve()},
             "scoring": {"status": "success", "scoreAtMean": 100, "samples": None}}
            for ordinal, order in enumerate(itertools.permutations(range(5)))]
    report = {"format": subject.NATIVE_FORMAT, "mode": "generate", "provenance": provenance(spec_sha),
              "context": {"algorithmVersion": subject.ALGORITHM + SOURCE, "fingerprint": "b" * 64},
              "sharedFingerprint": "c" * 64, "dependencyDescriptor": {"fixture": True}, "capabilities": {"chain": []},
              "validationDecks": [{**DECKS[0], "orders": rows}]}
    plan = {key: copy.deepcopy(report[key]) for key in ("context", "sharedFingerprint", "dependencyDescriptor", "capabilities")}
    return report, plan, spec_sha


class ValidationTests(unittest.TestCase):
    def test_backend_dispatch_requires_its_own_report_and_native_work_counts(self):
        self.assertEqual(inspect.signature(subject.validate).parameters["backend"].default, "programs")
        for backend, expected, mode, field in (
                ("programs", subject.programs, "programs", "uniquePrograms"),
                ("basis", subject.basis, "basisPrograms", "uniqueBasisPrograms")):
            with self.subTest(backend=backend):
                implementation, query_mode, unique_field = subject.lookup_backend(backend)
                self.assertIs(implementation.query, expected.query)
                self.assertEqual((query_mode, unique_field), (mode, field))
                query = {"format": expected.QUERY, "status": "success", "complete": True,
                         "sourceVersion": SOURCE, "inputsUnchanged": True,
                         "inputSha256": {**HASHES, "spec": "f" * 64},
                         "operatorContract": subject.basis.CONTRACT,
                         "stats": {"requestedJobs": 120, field: 27, "reusedPrograms": 0,
                                   "generatedPrograms": 27, "propagationCalls": 27}}
                self.assertEqual(subject.query_statistics(query, backend, "cold", SOURCE, HASHES, "f" * 64, 120)[field], 27)
                wrong_schema = copy.deepcopy(query)
                wrong_schema["stats"]["uniquePrograms" if backend == "basis" else "uniqueBasisPrograms"] = wrong_schema["stats"].pop(field)
                with self.assertRaises(ValueError):
                    subject.query_statistics(wrong_schema, backend, "cold", SOURCE, HASHES, "f" * 64, 120)
                query["stats"].update(reusedPrograms=27, generatedPrograms=0, propagationCalls=0)
                subject.query_statistics(query, backend, "warm", SOURCE, HASHES, "f" * 64, 120)
                mutations = [lambda value: value.update(format="other-backend"),
                             lambda value: value.update(sourceVersion="e" * 64),
                             lambda value: value["inputSha256"].update(spec="e" * 64),
                             lambda value: value["inputSha256"].update(decks="e" * 64),
                             lambda value: value["stats"].update(requestedJobs=119),
                             lambda value: value["stats"].update(propagationCalls=1),
                             lambda value: value["stats"].update(reusedPrograms=26)]
                if backend == "basis":
                    mutations.append(lambda value: value.update(operatorContract="other-contract"))
                for mutate in mutations:
                    changed = copy.deepcopy(query)
                    mutate(changed)
                    with self.assertRaises(ValueError):
                        subject.query_statistics(changed, backend, "warm", SOURCE, HASHES, "f" * 64, 120)
        with self.assertRaises(ValueError):
            subject.lookup_backend("auto")

    def test_reused_reference_requires_original_hashes_spec_and_current_native_context(self):
        report, plan, spec_sha = reference()
        source, rows = subject.validate_reference(report, DECKS, HASHES, spec_sha, plan)
        self.assertEqual(source, SOURCE)
        self.assertEqual(len(rows[0]), 120)
        mutations = [lambda value: value["provenance"].update(datasetSha256="f" * 64),
                     lambda value: value["provenance"]["inputs"].update(rosterSha256="f" * 64),
                     lambda value: value["provenance"]["inputs"].update(requestSha256="f" * 64),
                     lambda value: value["provenance"]["inputs"].update(specSha256="f" * 64),
                     lambda value: value["context"].update(algorithmVersion=subject.ALGORITHM + "f" * 64),
                     lambda value: value.update(dependencyDescriptor={"changed": True}),
                     lambda value: value.update(capabilities={"chain": ["changed"]})]
        for mutate in mutations:
            changed = copy.deepcopy(report)
            mutate(changed)
            with self.assertRaises(ValueError):
                subject.validate_reference(changed, DECKS, HASHES, spec_sha, plan)

    def test_reference_requires_all_original_orders_and_successful_unsampled_scores(self):
        report, plan, spec_sha = reference()
        mutations = [lambda rows: rows.pop(), lambda rows: rows.__setitem__(1, copy.deepcopy(rows[0])),
                     lambda rows: rows[0]["actual"].update(status="capacity"),
                     lambda rows: rows[0]["scoring"].update(status="error"),
                     lambda rows: rows[0]["scoring"].update(scoreAtMean=None),
                     lambda rows: rows[0]["scoring"].update(samples={"count": 1})]
        for mutate in mutations:
            changed = copy.deepcopy(report)
            mutate(changed["validationDecks"][0]["orders"])
            with self.assertRaises(ValueError):
                subject.validate_reference(changed, DECKS, HASHES, spec_sha, plan)
        changed = copy.deepcopy(report)
        changed["validationDecks"][0]["members"][0] = 99
        with self.assertRaises(ValueError):
            subject.validate_reference(changed, DECKS, HASHES, spec_sha, plan)

    def test_union_times_detects_a_shift_hidden_by_zipped_step_values(self):
        neither = copy.deepcopy(subject.EMPTY)
        both = [[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [1.0, 1.0]]
        actual = curve([{"timeMs": 0, "buckets": neither}, {"timeMs": 10, "buckets": both}])
        lookup = curve([{"timeMs": 0, "buckets": neither}, {"timeMs": 20, "buckets": both}])
        result = subject.compare_curves(actual, lookup)
        self.assertEqual(result["timePoints"], 3)
        self.assertFalse(result["jointIntervalsOverlap"])
        self.assertFalse(result["weightedIntervalsOverlap"])
        self.assertEqual(result["maximumJointSeparatedGap"], 1.0)
        self.assertEqual(result["maximumWeightedSeparatedGap"], 1.0)

    def test_outward_lookup_and_virtual_probe_superset_are_reported_separately(self):
        actual = curve([{"timeMs": 0, "buckets": [[0.7, 0.7], [0, 0], [0, 0], [0.3, 0.3]]}], [True, False])
        lookup = curve([{"timeMs": 0, "buckets": [[0.69999, 0.70001], [0, 0], [0, 0], [0.29999, 0.30001]]}], [True, True])
        result = subject.compare_curves(actual, lookup)
        self.assertTrue(result["jointIntervalsOverlap"])
        self.assertTrue(result["weightedIntervalsOverlap"])
        self.assertTrue(result["lookupEnclosesReferenceEndpoints"])
        self.assertTrue(result["lookupCoversRequiredProbes"])
        self.assertFalse(result["sameJointEndpoints"])
        self.assertFalse(result["sameProbeFlags"])
        lookup["probes"][0] = False
        self.assertFalse(subject.compare_curves(actual, lookup)["lookupCoversRequiredProbes"])

    def test_invalid_curve_and_duplicate_materialized_keys_are_refused(self):
        for mutate in [lambda value: value["steps"].append(copy.deepcopy(value["steps"][0])),
                       lambda value: value["steps"][0]["buckets"][0].__setitem__(0, float("nan")),
                       lambda value: value["steps"][0]["buckets"][0].__setitem__(1, 1.1)]:
            value = curve()
            mutate(value)
            with self.assertRaises(ValueError):
                subject.validate_curve(value)
        entry = {"key": [], "response": {"status": "success", "curve": curve()}}
        value = {"format": "ournotes-deck.luck-response-materialization/1", "table": {"entries": [entry]}}
        self.assertEqual(len(subject.materialized_curves(value)), 1)
        value["table"]["entries"].append(copy.deepcopy(entry))
        with self.assertRaises(ValueError):
            subject.materialized_curves(value)

    def test_basis_materialization_uses_its_actual_native_envelope_and_source_contract(self):
        # Faithful basis-materialize envelope from the real owned-deck run;
        # one reduced curve checks parsing only, never a simulation outcome.
        key = [[{"source": "gekisou", "id": 14, "level": 5, "matched": None}, 0]]
        value = {"format": "ournotes-deck.luck-response-materialized/1",
                 "sourceVersion": SOURCE, "operatorContract": "conditional-start-minimum/1",
                 "nativeExpectationProven": False, "rankingProven": False, "usesMonteCarlo": False,
                 "propagationCalls": 0, "retainedResponseBytes": 128,
                 "table": {"context": {"algorithmVersion": subject.ALGORITHM + SOURCE,
                                       "fingerprint": "b" * 64},
                           "entries": [{"key": key, "response": {"status": "success", "curve": curve()}}]}}
        parsed = subject.materialized_curves(value, "basis", SOURCE)
        self.assertEqual(parsed, {subject.pipeline.canonical(key): curve()})
        self.assertTrue(subject.compare_curves(curve(), parsed[subject.pipeline.canonical(key)])["weightedIntervalsOverlap"])
        with self.assertRaises(ValueError):
            subject.materialized_curves(value, "programs", SOURCE)
        programs_value = {"format": "ournotes-deck.luck-response-materialization/1",
                          "table": copy.deepcopy(value["table"])}
        self.assertEqual(subject.materialized_curves(programs_value, "programs", SOURCE), parsed)
        with self.assertRaises(ValueError):
            subject.materialized_curves(programs_value, "basis", SOURCE)
        mutations = [lambda report: report.update(format="ournotes-deck.other/1"),
                     lambda report: report.update(operatorContract="other-contract"),
                     lambda report: report.update(sourceVersion="f" * 64),
                     lambda report: report["table"]["context"].update(algorithmVersion=subject.ALGORITHM + "f" * 64),
                     lambda report: report.update(propagationCalls=1),
                     lambda report: report.update(nativeExpectationProven=True),
                     lambda report: report["table"]["entries"][0]["response"].update(status="missing"),
                     lambda report: report["table"]["entries"].append(copy.deepcopy(report["table"]["entries"][0]))]
        for mutate in mutations:
            changed = copy.deepcopy(value)
            mutate(changed)
            with self.assertRaises(ValueError):
                subject.materialized_curves(changed, "basis", SOURCE)
        with self.assertRaises(ValueError):
            subject.materialized_curves(value, "basis", "f" * 64)

    def test_prediction_must_bind_original_input_bytes_and_all_physical_orders(self):
        ref, _, _ = reference()
        rows = [{"order": row["order"], "ordinal": row["ordinal"], "entries": [],
                 "status": "success", "scoreAtLookup": 101} for row in ref["validationDecks"][0]["orders"]]
        prediction = {"format": "ournotes-deck.luck-response-prediction/1", "status": "success", "ordersScored": 120,
                      "context": ref["context"], "provenance": provenance("unused"),
                      "decks": [{**DECKS[0], "orders": rows}]}
        self.assertEqual(len(subject.prediction_rows(prediction, DECKS, SOURCE, HASHES)[0]), 120)
        for mutate in [lambda value: value.update(status="partial"),
                       lambda value: value["provenance"]["inputs"].update(decksSha256="f" * 64),
                       lambda value: value["decks"][0]["orders"][0].update(status="missing")]:
            changed = copy.deepcopy(prediction)
            mutate(changed)
            with self.assertRaises(ValueError):
                subject.prediction_rows(changed, DECKS, SOURCE, HASHES)


if __name__ == "__main__":
    unittest.main()
