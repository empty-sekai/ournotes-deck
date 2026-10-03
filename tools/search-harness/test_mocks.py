"""Deterministic mock roster domains."""
import json
import pathlib
import tempfile
import unittest

import mock_rosters


def columns(rows):
    keys = list(rows[0]) if rows else []
    return {"columns": keys, "rows": [[r[k] for k in keys] for r in rows]}


def master(nonstandard=False, members=7):
    awakes, ranks, item_levels = ([1, 3, 7], [1, 4], [1, 4, 9]) if nonstandard else (list(range(1, 6)), list(range(1, 6)), list(range(1, 31)))
    live, gk = ([1, 3, 9], [2, 7]) if nonstandard else (list(range(1, 6)), list(range(1, 6)))
    tables = {
        "MasterCharacter": [{"_id": i, "_bandID": 1 if i <= 3 else 2} for i in range(1, 7)],
        "MasterMemberCard": [{"_id": i, "_characterID": min(i, 6), "_rarity": 2, "_cardType": 1 + i % 3,
                              "_memberCardLevelGroup": 1, "_memberCardAwakeGroup": 1,
                              "_memberCardRankGroup": 1, "_liveSkillID": 1, "_gekisouSkillID": 1 + i % 2}
                             for i in range(1, members + 1)],
        "MasterSupportCard": [{"_id": i, "_rarity": 2, "_cardType": 1 + i % 3, "_supportCardLevelGroup": 1,
                               "_supportCardRankGroup": 1, "_supportSkillId01": i, "_supportSkillId02": 0,
                               "_gekisouSupportSkillId01": 0, "_gekisouSupportSkillId02": 0} for i in range(1, 5)],
        "MasterSupportSkillEffect": [{"_supportSkillID": i, "_skillEffectType": 12006 if i == 2 else 2000}
                                     for i in range(1, 5)],
        "MasterGekisouSkill": [{"_id": 1, "_gekisouMissionType": 1}, {"_id": 2, "_gekisouMissionType": 2}],
        "MasterMemberCardAwake": [{"_group": 1, "_awakeCount": a} for a in awakes],
        "MasterMemberCardLevelLimit": [{"_rarity": 2, "_awakeCount": a, "_limitLevel": 10 * a} for a in awakes],
        "MasterMemberCardLevel": [{"_group": 1, "_level": v} for v in range(1, 71)],
        "MasterMemberCardRank": [{"_group": 1, "_rank": r} for r in ranks],
        "MasterSupportCardRank": [{"_group": 1, "_rank": r, "_limitLevel": 10 * r} for r in ranks],
        "MasterSupportCardLevel": [{"_group": 1, "_level": v} for v in range(1, 71)],
        "MasterLiveSkillEffect": [{"_liveSkillID": 1, "_level": v} for v in live],
        "MasterGekisouSkillEffect": [{"_gekisouSkillID": k, "_level": v} for k in [1, 2] for v in gk],
        "MasterBandItem": [{"_id": 11, "_bandId": 1}, {"_id": 22, "_bandId": 2}],
        "MasterBandItemLevel": [{"_bandItemId": i, "_level": v} for i in [11, 22] for v in item_levels],
        "MasterBandItemSkillEffect": [{"_bandItemId": i, "_level": v} for i in [11, 22] for v in item_levels + [999]],
        "MasterCharacterRank": [{"_rank": v} for v in ([1, 3, 7] if nonstandard else range(1, 51))],
        "MasterVip": [{"_vipRank": v} for v in ([1, 2, 8] if nonstandard else range(1, 22))],
        "MasterEvent": [{"_id": 1}],
    }
    return {name: columns(rows) for name, rows in tables.items()}


def document(data):
    hashes = {name: {"sha256": mock_rosters.digest(table)} for name, table in data.items()}
    return {"format": "nnnotes.deck-data/1", "master": data,
            "provenance": {"region": "test", "master": {"version": "1", "tables": hashes}}}


class MockRosters(unittest.TestCase):
    def generate(self, root, data, directory, extended=True):
        path = root / "data.json"
        path.write_text(json.dumps(document(data)), encoding="utf-8")
        return mock_rosters.generate(path, root / directory, extended=extended)

    def test_extended_is_deterministic_48_and_item_pairs_change_only_items(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            first = self.generate(root, master(), "a")
            second = self.generate(root, master(), "b")
            self.assertEqual(first, second)
            kinds = len(mock_rosters.KINDS) * len(mock_rosters.KIND_SEEDS)
            self.assertEqual(len(first["rosters"]), 6 + kinds + 42)
            pairs = {}
            for row in first["rosters"]:
                path = root / "a" / row["file"]
                roster = json.loads(path.read_text(encoding="utf-8"))
                self.assertFalse(mock_rosters.Domains(master()).validate(roster))
                if row["family"] == "no-snap":
                    self.assertEqual(roster["snaps"], [])
                if row["pairedGroup"]:
                    pairs.setdefault(row["pairedGroup"], []).append((row, roster))
            self.assertEqual(len(pairs), 6)
            for group in pairs.values():
                self.assertEqual({r[0]["variant"] for r in group}, {"baseline", "none", "low", "high"})
                self.assertEqual(len({r[0]["fixedFactsSha256"] for r in group}), 1)
                fixed = mock_rosters.without_items(group[0][1])
                self.assertTrue(all(mock_rosters.without_items(r[1]) == fixed for r in group))

    def test_legacy_profiles_are_byte_exact_when_represented(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            data = master()
            generated = self.generate(root, data, "out")
            source = {"schema": mock_rosters.SCHEMA, "tables": generated["tables"]}
            for profile, row in zip(mock_rosters.PROFILES, generated["rosters"]):
                self.assertTrue(row["legacyPreserved"], row)
                roster = mock_rosters.build(mock_rosters.Domains(data), profile)
                expected = json.dumps({**roster, "mockSource": source}, ensure_ascii=False, indent=1) + "\n"
                self.assertEqual((root / "out" / row["file"]).read_bytes(), expected.encode())

    def test_a_new_card_changes_no_other_cards_facts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            old = self.generate(root, master(), "old")
            new = self.generate(root, master(members=8), "new")
            added = 0
            for a, b in zip(old["rosters"], new["rosters"]):
                self.assertEqual(a["name"], b["name"])
                ra = json.loads((root / "old" / a["file"]).read_text(encoding="utf-8"))
                rb = json.loads((root / "new" / b["file"]).read_text(encoding="utf-8"))
                # Owned in both: identical facts. Only a card added to reach five characters may drop out.
                common = {m["id"] for m in ra["members"]} & {m["id"] for m in rb["members"]}
                same = lambda r: [m for m in r["members"] if m["id"] in common]
                self.assertEqual(same(ra), same(rb), a["name"])
                self.assertLessEqual(len(ra["members"]) - len(common), 1, a["name"])
                added += any(m["id"] == 8 for m in rb["members"])
                self.assertEqual(ra["snaps"], rb["snaps"], a["name"])
            self.assertGreater(added, 0)

    def test_kinds_concentrate_their_cards(self):
        domains = mock_rosters.Domains(master())
        self.assertEqual(domains.converting_snaps, {2})
        characters = lambda r: {domains.members[m["id"]]["_characterID"] for m in r["members"]}
        for seed in mock_rosters.KIND_SEEDS:
            rosters = {kind: mock_rosters.build_kind(domains, kind, seed) for kind in mock_rosters.KINDS}
            for kind, roster in rosters.items():
                self.assertEqual(domains.validate(roster), [], kind)
            self.assertIn(2, [s["id"] for s in rosters["conversion-rich"]["snaps"]])
            self.assertEqual(len(characters(rosters["five-characters"])), 5)
            self.assertEqual(rosters["near-ties"]["player"]["bandItems"], {})
            self.assertEqual(len(set(rosters["near-ties"]["player"]["characterRanks"].values())), 1)

    def test_rosters_carry_and_check_their_table_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            data = master()
            manifest = self.generate(root, data, "out", extended=False)
            self.assertIsNone(manifest["tables"]["MasterVip"] if "MasterVip" not in data else None)
            roster = json.loads((root / "out" / manifest["rosters"][0]["file"]).read_text(encoding="utf-8"))
            self.assertEqual(roster["mockSource"]["tables"], manifest["tables"])
            mock_rosters.check_tables(manifest, document(data))
            changed = master()
            changed["MasterSupportCard"]["rows"][0][-1] = 5
            with self.assertRaisesRegex(ValueError, "regenerate"):
                mock_rosters.check_tables(manifest, document(changed))

    def test_sparse_nonstandard_domains_exclude_effect_only_item_rows(self):
        data = master(True)
        domains = mock_rosters.Domains(data)
        for family in mock_rosters.FAMILIES:
            for seed in [101, 202, 303]:
                roster = mock_rosters.build_extended(domains, family, seed)
                self.assertEqual(domains.validate(roster), [])
                self.assertTrue(all(v in [1, 4, 9] for v in roster["player"]["bandItems"].values()))
                self.assertTrue(all(m["liveSkillLevel"] in [1, 3, 9] for m in roster["members"]))
                self.assertTrue(all(m["gekisouSkillLevel"] in [2, 7] for m in roster["members"]))

    def test_missing_vip_is_explicit_and_never_inferred_from_bonus_rows(self):
        data = master()
        del data["MasterVip"]
        data["MasterVipRankBonus"] = columns([{"_vipRank": 999}])
        domains = mock_rosters.Domains(data)
        roster = mock_rosters.build_extended(domains, "balanced", 101)
        self.assertEqual(roster["player"]["vipRank"], 1)
        self.assertTrue(domains.warnings)


if __name__ == "__main__":
    unittest.main()
