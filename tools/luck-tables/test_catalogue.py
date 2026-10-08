"""Small acquisition/coverage checks; no network, native simulation, or builds."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("luck_catalogue", Path(__file__).with_name("catalogue.py"))
catalogue = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(catalogue)


def table(values):
    columns = list(values[0]) if values else []
    return {"columns": columns, "rows": [[value[k] for k in columns] for value in values]}


def fixture():
    effects = [{"_id": level, "_gekisouSkillID": 7, "_level": level, "_skillEffectType": 11001,
                "_skillTriggerConditionGroup": 0, "_skillConditionGroup": 0,
                "_skillReleaseConditionGroup": 5} for level in (1, 2)]
    masters = {
        "MasterLiveMusic": [{"_id": 100, "_easyID": 10, "_normalID": 11, "_hardID": 12,
                             "_expertID": 13, "_gekisouMission1": 2}],
        "MasterLiveMusicScore": [{"_id": i, "_musicScoreLevel": i-9} for i in range(10, 14)],
        "MasterGekisouSkillEffect": effects, "MasterGekisouSupportSkillEffect": [],
        "MasterSkillConditionSet": [{"_id": 1, "_group": 5, "_conditionIds": [2]}],
        "MasterSkillCondition": [{"_id": 2, "_conditionType": 5000, "_conditionTargetIDs": [8]}],
        "MasterMemberCard": [{"_id": 1}], "MasterSupportCard": [],
    }
    return {"format": "nnnotes.deck-data/1", "master": {k: table(v) for k, v in masters.items()},
            "charts": [{"scoreId": i, "notes": {"id": [1], "timeMs": [100]},
                        "fevers": {"startMs": [0], "endMs": [200]}} for i in range(10, 14)]}


class InventoryTests(unittest.TestCase):
    def test_all_difficulties_and_unowned_levels_are_preserved(self):
        value = catalogue.inventory(fixture())
        self.assertEqual(value["counts"]["difficulties"], dict.fromkeys(catalogue.DIFFICULTIES, 1))
        self.assertEqual(value["counts"]["writerSourceLevels"], 2)
        self.assertEqual(value["counts"]["writerFormationKeys"], 2)
        self.assertEqual(value["counts"]["allChartBaseAndSingleJobs"], 44)
        self.assertFalse(value["nativeCatalogueValidated"])

    def test_formation_only_trigger_condition_and_full_level_rows(self):
        data = fixture()
        effect = data["master"]["MasterGekisouSkillEffect"]
        effect["rows"][1][effect["columns"].index("_skillConditionGroup")] = 5
        keys, unsupported = catalogue.all_chain_keys(data)
        self.assertEqual([(k["level"], k["matched"]) for k in keys], [(1, None), (2, False), (2, True)])
        self.assertEqual(unsupported, [])
        # Formation on an ordinary companion row also affects its whole skill.
        row = list(effect["rows"][1]); row[effect["columns"].index("_skillEffectType")] = 2000
        row[effect["columns"].index("_level")] = 1
        effect["rows"].append(row)
        self.assertEqual(len(catalogue.all_chain_keys(data)[0]), 4)

    def test_missing_duplicate_or_orphan_chart_is_rejected(self):
        for mutation in (lambda d: d["charts"].pop(),
                         lambda d: d["charts"].append(copy.deepcopy(d["charts"][0])),
                         lambda d: d["master"]["MasterLiveMusic"]["rows"][0].__setitem__(1, 0)):
            data = fixture(); mutation(data)
            with self.assertRaises(ValueError):
                catalogue.inventory(data)

    def test_notes_must_be_complete_parallel_arrays(self):
        data = fixture(); data["charts"][0]["notes"]["timeMs"] = []
        with self.assertRaises(ValueError):
            catalogue.inventory(data)

    def test_non_luck_charts_remain_in_inventory(self):
        data = fixture(); data["master"]["MasterLiveMusic"]["rows"][0][-1] = 1
        value = catalogue.inventory(data)
        self.assertEqual(value["counts"]["charts"], 4)
        self.assertEqual(value["counts"]["luckCharts"], 0)


class AcquisitionTests(unittest.TestCase):
    def documents(self):
        raw = catalogue.canonical(fixture())
        manifest = {"format": "nnnotes.replay-manifest/1", "deckData": {
            "format": "nnnotes.deck-data/1", "url": "deck-data.json", "sha256": catalogue.sha(raw), "bytes": len(raw)},
            "charts": [{"scoreId": i} for i in range(10, 14)]}
        manifest_raw = catalogue.canonical(manifest)
        build = {"format": "moenotes.music-data-build/1", "provenance": {"region": "jp"},
                 "charts": 4, "songs": 1, "replay": {"manifestUrl": "replay/hash/manifest.json",
                                                       "sha256": catalogue.sha(manifest_raw), "charts": 4}}
        return raw, manifest_raw, catalogue.canonical(build)

    def test_discovery_pins_and_verifies_before_publishing(self):
        raw, manifest, build = self.documents()
        responses = [build, manifest, raw]
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "new"
            receipt = catalogue.acquire("jp", out, lambda _url, _limit: responses.pop(0))
            self.assertEqual(receipt["source"]["deckData"]["sha256"], catalogue.sha(raw))
            self.assertEqual((out / "deck-data.json").read_bytes(), raw)
            self.assertTrue(receipt["embeddedNotesComplete"])

    def test_corrupt_download_does_not_publish_partial_dataset(self):
        raw, manifest, build = self.documents()
        responses = [build, manifest, raw + b" "]
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "new"
            with self.assertRaises(ValueError):
                catalogue.acquire("jp", out, lambda _url, _limit: responses.pop(0))
            self.assertFalse(out.exists())

    def test_declared_manifest_keeps_input_anchor_and_every_chart(self):
        data = fixture()
        data["master"]["MasterLiveMusic"]["rows"][0][-1] = 1
        raw = catalogue.canonical(data)
        snapshot = {"format": "ournotes.owned-snapshot/1", "datasetId": catalogue.sha(raw),
                    "eligible": {"members": [{"id": 1}], "snaps": []}}
        snapshot_raw = json.dumps(snapshot, indent=2).encode()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); data_path = root / "data.json"; snap_path = root / "snap.json"
            data_path.write_bytes(raw); snap_path.write_bytes(snapshot_raw)
            result = catalogue.build_manifest(data_path, snap_path, root / "manifest", "jp")
            self.assertEqual(len(result["cases"]), 4)
            self.assertTrue(result["luckCatalogue"]["includeNonLuckCharts"])
            self.assertEqual(len(result["luckCatalogue"]["keys"]), 2)
            self.assertEqual((root / "manifest/snapshot.json").read_bytes(), snapshot_raw)
            self.assertEqual(data_path.read_bytes(), raw)
            self.assertFalse(result["coverage"]["allTeamCombinationsCovered"])
            snapshot["datasetId"] = "0" * 64; snap_path.write_text(json.dumps(snapshot))
            with self.assertRaises(ValueError):
                catalogue.build_manifest(data_path, snap_path, root / "bad")


if __name__ == "__main__":
    unittest.main()
