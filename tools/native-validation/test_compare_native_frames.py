import copy
import unittest
from compare_native_frames import compare


class StrictComparisonTests(unittest.TestCase):
    def setUp(self):
        self.capture = {'chartId': 1, 'frames': [{'frame': 0, 'timeMs': 0, 'score': 12,
            'float32Bits': {'noteScoreUp': 1065353216}, 'skills': [{'index': 0, 'state': 2}]}]}
        self.contract = {'integerFields': ['frame', 'timeMs', 'score', 'skills.*.index', 'skills.*.state'],
                         'float32BitFields': ['float32Bits.noteScoreUp']}

    def test_exact_contract_passes(self):
        r = compare(self.capture, copy.deepcopy(self.capture), self.contract)
        self.assertEqual((r['status'], r['fieldComparisons']), ('passed', 6))

    def test_one_ulp_and_frame_shift_are_retained(self):
        rust = copy.deepcopy(self.capture)
        rust['frames'][0]['frame'] = 1
        rust['frames'][0]['float32Bits']['noteScoreUp'] += 1
        r = compare(self.capture, rust, self.contract)
        self.assertEqual((r['status'], r['differenceCount'], r['maxDifferenceUlp']), ('different', 2, 1))

    def test_missing_field_cannot_pass(self):
        rust = copy.deepcopy(self.capture)
        del rust['frames'][0]['score']
        self.assertEqual(compare(self.capture, rust, self.contract)['status'], 'incomplete')

    def test_extra_pool_instance_cannot_disappear_through_zip(self):
        rust = copy.deepcopy(self.capture)
        rust['frames'][0]['skills'].append({'index': 1, 'state': 0})
        r = compare(self.capture, rust, self.contract)
        self.assertEqual((r['status'], r['differenceCount']), ('different', 2))

    def test_float_bits_require_int_not_bool_or_float(self):
        for value in (True, 1065353216.0, -1):
            rust = copy.deepcopy(self.capture)
            rust['frames'][0]['float32Bits']['noteScoreUp'] = value
            with self.assertRaises(ValueError):
                compare(self.capture, rust, self.contract)

    def test_matching_nonfinite_bits_cannot_pass(self):
        for value in (0x7f800000, 0xff800000, 0x7fc00000):
            native = copy.deepcopy(self.capture)
            native['frames'][0]['float32Bits']['noteScoreUp'] = value
            with self.assertRaises(ValueError):
                compare(native, copy.deepcopy(native), self.contract)


    def test_both_empty_pools_cannot_satisfy_a_required_pool(self):
        capture = copy.deepcopy(self.capture)
        capture['frames'][0]['skills'] = []
        contract = {**self.contract, 'arrayLengths': {'skills': 1}}
        self.assertEqual(compare(capture, copy.deepcopy(capture), contract)['status'], 'incomplete')


if __name__ == '__main__':
    unittest.main()
