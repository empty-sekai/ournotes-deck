"""Declared matrix ordering, ownership and canonical result clocks."""
import copy
from datetime import datetime
import unittest

from matrix import canonical_ticks, legacy_roster, load_suite, materialize_context, select_cases, snapshot


class MatrixContracts(unittest.TestCase):
    def test_suite_selection_preserves_all_declared_requests(self):
        _, matrix = load_suite("full48")
        original = copy.deepcopy(matrix)
        cases = select_cases(matrix, "all")
        self.assertEqual(len(cases), 48)
        self.assertEqual([case["group"] for case in cases[:18]], ["score"] * 18)
        self.assertEqual(len(select_cases(matrix, "score")), 18)
        chosen = matrix["cases"][1]["name"]
        self.assertEqual([case["name"] for case in select_cases(matrix, chosen)], [chosen])
        with self.assertRaises(ValueError):
            select_cases(matrix, "missing-case")
        self.assertEqual(matrix, original)

    def test_profile_owns_and_keeps_every_declared_eligible_card(self):
        profile = {"format": "ournotes-deck.harness-profile/1",
                   "members": [{"id": 9, "level": 1}, {"id": 3, "level": 2}], "snaps": [{"id": 7}],
                   "player": {"characterRanks": {"coverage": "complete", "values": [{"id": 1, "value": 2}]},
                              "characterTotalRank": None, "vipRank": 1, "bandItems": [], "eventIds": [1],
                              "memory": {"musicRanks": [], "unlockedMembers": [], "unlockedSnaps": []}}}
        original = copy.deepcopy(profile)
        value = snapshot(profile, "a" * 64, "suite/profile")
        self.assertEqual(value["ownedFacts"]["memberIds"], [9, 3])
        self.assertEqual(value["ownedFacts"]["snapIds"], [7])
        self.assertEqual(value["eligible"]["members"], profile["members"])
        self.assertEqual(value["player"], profile["player"])
        self.assertEqual(legacy_roster(value)["player"]["ownedMemberCardIds"], [9, 3])
        value["player"]["vipRank"] = 2
        self.assertEqual(profile, original)

    def test_event_context_uses_integer_no_offset_ticks(self):
        template = {"powerSnapshot": {"eventIds": [1], "capturedJstTicks": None},
                    "resultClock": {"kind": "eventStartOffset", "eventId": 1, "seconds": 496800, "execution": "played"},
                    "eventPayoff": {"consumedCount": 1, "eventWindowAdapter": "canonical-master-no-offset"}}
        original = copy.deepcopy(template)
        data = {"master": {"MasterEvent": {"columns": ["_id", "_startAt"], "rows": [[1, "2026-09-01 12:00:00"]]}}}
        result = materialize_context(template, data)
        start = (datetime(2026, 9, 1).toordinal() - 1) * 86400 + 12 * 3600
        expected = (start + 496800) * 10_000_000
        self.assertGreater(expected, 2 ** 53)
        self.assertEqual(result["resultClock"], {"execution": "played", "savedStartJstTicks": expected,
                                                "serverNowJstTicks": expected})
        self.assertEqual(result["eventPayoff"], template["eventPayoff"])
        self.assertEqual(template, original)
        self.assertEqual(canonical_ticks("0001/01/01T00:00:00.0000001"), 1)
        for invalid in ("2026-09-01T00:00:00Z", "2026-09/01", "2026-02-30", " 2026-09-01"):
            with self.assertRaises(ValueError):
                canonical_ticks(invalid)


if __name__ == "__main__":
    unittest.main()
