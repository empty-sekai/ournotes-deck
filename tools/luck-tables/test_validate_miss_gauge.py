"""Parser and comparison gates, never synthetic performance acceptance."""
import copy
import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location('miss_gauge_validation_tests',
                                             Path(__file__).with_name('validate_miss_gauge.py'))
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)
SOURCE, FP = 'a' * 64, 'b' * 64
HASHES = {'data': '1' * 64, 'snapshot': '2' * 64, 'request': '3' * 64}
SPEC_SHA = '4' * 64


def curve():
    return {'steps': [{'timeMs': 0, 'buckets': copy.deepcopy(subject.compare.EMPTY)},
                      {'timeMs': 10, 'buckets': [[0.5, 0.5], [0.5, 0.5], [0, 0], [0, 0]]}],
            'probes': [True], 'probeTransitions': [0, 1], 'rangeMoments': []}


def report_fixture():
    job = {'name': 'parse-only', 'entries': []}
    report = {'format': subject.storage.REPORT, 'mode': 'programs', 'sourceVersion': SOURCE,
              'canonicalMissGauge': True, 'canonicalStartMinimum': False,
              'complete': True, 'probabilityComplete': True, 'identificationComplete': True,
              'context': {'algorithmVersion': 'ournotes-luck-response/1/' + SOURCE, 'fingerprint': 'c' * 64},
              'provenance': {'datasetSha256': HASHES['data'], 'inputs': {'rosterSha256': HASHES['snapshot'],
                             'requestSha256': HASHES['request'], 'specSha256': SPEC_SHA}},
              'programs': [{'programIndex': 0, 'fingerprint': FP, 'sourceVersion': SOURCE, 'status': 'success',
                            'response': {'status': 'success', 'curve': curve()},
                            'operatorContract': subject.CONTRACT, 'representativeJobIndex': 0}],
              'jobs': [{**job, 'jobIndex': 0, 'programIndex': 0, 'programFingerprint': FP,
                        'status': 'success', 'operatorContract': subject.CONTRACT}],
              'stats': {'sourceJobs': 1, 'compiledJobs': 1, 'uniqueRetainedPrograms': 1,
                        'propagationCalls': 1, 'exactProgramAliases': 0,
                        'identityBudgetBytes': subject.IDENTITY_BYTES, 'programBudgetBytes': subject.PROGRAM_BYTES}}
    return report, {'jobs': [job], 'canonicalMissGauge': True}


class MissGaugeValidationTests(unittest.TestCase):
    def test_actual_master_gauge_maxima_are_distinct_from_constructor_defaults(self):
        data = {'master': {'MasterLiveSettings': {
            'columns': ['_id', '_key', '_value'],
            'rows': [[15, 'gekisou_luck_gauge_max', '140'],
                     [16, 'gekisou_luck_gauge_max_rush', '70']]}}}
        domain = subject.gauge_domain(data)
        self.assertEqual(domain['actualMasterGaugeMaxima'], [140, 70])
        self.assertEqual(domain['nativeConstructorAdditionalMaxima'], [50, 100])
        self.assertEqual(domain['parameterAnalysisMaxima'], [50, 70, 100, 140])
        for changed_value in ('0', '-1', '70.5', 'nan'):
            changed = copy.deepcopy(data)
            changed['master']['MasterLiveSettings']['rows'][1][2] = changed_value
            with self.assertRaises(ValueError):
                subject.gauge_domain(changed)

    def test_source_flags_capacity_and_all_native_propagations_are_required(self):
        report, spec = report_fixture()
        result = subject.program_report(report, spec, HASHES, SPEC_SHA, SOURCE)
        self.assertEqual(result['parse-only']['fingerprint'], FP)
        changes = [lambda value: value.update(canonicalMissGauge=False),
                   lambda value: value.update(canonicalStartMinimum=True),
                   lambda value: value.update(identificationComplete=False),
                   lambda value: value['stats'].update(propagationCalls=0),
                   lambda value: value['stats'].update(compiledJobs=0),
                   lambda value: value['stats'].update(identityBudgetBytes=32 << 20),
                   lambda value: value['jobs'][0].update(programFingerprint='e' * 64),
                   lambda value: value['programs'][0].update(operatorContract='native-ordered-actions/1'),
                   lambda value: value['programs'][0].update(representativeJobIndex=1),
                   lambda value: value['provenance']['inputs'].update(specSha256='e' * 64)]
        for change in changes:
            changed = copy.deepcopy(report); change(changed)
            with self.assertRaises(ValueError):
                subject.program_report(changed, spec, HASHES, SPEC_SHA, SOURCE)

    def test_union_times_use_forward_fill_and_every_joint_bucket(self):
        original, canonical = curve(), curve()
        canonical['steps'].insert(1, {'timeMs': 5, 'buckets': copy.deepcopy(subject.compare.EMPTY)})
        result = subject.compare_probability_curves(original, canonical)
        self.assertEqual(result['timePoints'], 3)
        self.assertEqual(result['jointIntervals'], 12)
        self.assertEqual(result['disjointJointIntervals'], 0)
        self.assertEqual(result['maximumEndpointDifference'], 0)
        self.assertTrue(result['sameJointEndpoints'])
        canonical['steps'][1]['buckets'] = [[0.5, 0.5], [0, 0], [0, 0], [0.5, 0.5]]
        result = subject.compare_probability_curves(original, canonical)
        self.assertEqual(result['disjointJointIntervals'], 2)
        self.assertEqual(result['maximumEndpointDifference'], 0.5)
        with self.assertRaises(ValueError):
            subject.compare_jobs({'job': {'curve': original}}, {'job': {'curve': canonical}})

    def test_observer_metadata_cannot_be_lost_when_probabilities_match(self):
        original, canonical = curve(), curve()
        canonical['probeTransitions'][1] = 0
        with self.assertRaises(ValueError):
            subject.compare_jobs({'job': {'curve': original}}, {'job': {'curve': canonical}})
        canonical = curve(); canonical['probes'][0] = False
        with self.assertRaises(ValueError):
            subject.compare_jobs({'job': {'curve': original}}, {'job': {'curve': canonical}})

    def test_rounded_witness_requires_both_separation_and_multiplicity_equivalence(self):
        fingerprints = {'base': '0', 'two-level1': '1', 'one-level2': '2',
                        'level1-plus-level2': '3', 'one-level3': '3', 'level2-plus-level1': '3'}
        mapping = {'miss-witness-' + name: {'fingerprint': fingerprint * 64, 'curve': curve()}
                   for name, fingerprint in fingerprints.items()}
        self.assertTrue(subject.witness_gate(mapping)['nativeDeltaEquivalenceShared'])
        wrong = copy.deepcopy(mapping)
        wrong['miss-witness-two-level1']['fingerprint'] = wrong['miss-witness-one-level2']['fingerprint']
        with self.assertRaises(ValueError):
            subject.witness_gate(wrong)
        wrong = copy.deepcopy(mapping)
        wrong['miss-witness-one-level3']['fingerprint'] = 'f' * 64
        with self.assertRaises(ValueError):
            subject.witness_gate(wrong)

    def test_parameter_jobs_preserve_five_main_and_ten_support_capacity(self):
        job = subject.miss_job('structural-only', (1, 2, 3, 4, 5))
        self.assertEqual(len(job['entries']), 15)
        for position in range(5):
            entries = [item for item, held in job['entries'] if held == position]
            self.assertEqual([item['id'] for item in entries], [8, 66, 96])
            self.assertEqual([item['matched'] for item in entries], [None, False, False])
        witness = subject.miss_job('structural-witness', (1, 2))
        self.assertEqual(len(witness['entries']), 12)
        self.assertEqual(sum(item['id'] == 66 for item, _ in witness['entries']), 5)


if __name__ == '__main__':
    unittest.main()
