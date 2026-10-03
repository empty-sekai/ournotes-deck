import itertools
import unittest
from pt_certificate import best_bindings, snap_key


class AssignmentCertificateTests(unittest.TestCase):
    def test_dp_matches_all_physical_bindings_including_none_and_ties(self):
        for ids, matrix in [([4, 8, 9], [[2, 3, 2], [0, 5, -1], [3, 3, 3], [1, 0, 0], [4, 2, 2]]),
                            ([-3, 8], [[0, 0]] * 5), ([], [[]] * 5)]:
            none = [0] * 5
            oracle = []
            for binding in itertools.product([None] + ids, repeat=5):
                used = [s for s in binding if s is not None]
                if len(set(used)) != len(used):
                    continue
                power = sum(matrix[i][ids.index(s)] if s is not None else none[i]
                            for i, s in enumerate(binding))
                oracle.append((power, binding))
            oracle.sort(key=lambda r: (-r[0], snap_key(r[1])))
            for k in [1, 3, 15]:
                self.assertEqual(best_bindings(none, matrix, ids, k), oracle[:k])


if __name__ == '__main__':
    unittest.main()
