"""Cold/warm native-work and identity contracts."""
import copy
import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location('miss_basis_validation_tests',
                                             Path(__file__).with_name('validate_miss_basis.py'))
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)
SOURCE, BINARY = 'a' * 64, 'b' * 64
HASHES = {'data': '1' * 64, 'snapshot': '2' * 64, 'request': '3' * 64}
SPEC_SHA = '4' * 64


def family_fixture(jobs=2, families=1, reuse=True):
    stats = dict.fromkeys(subject.FAMILY_COUNTS, 0)
    stats.update(requestedJobs=jobs, compiledJobs=jobs, familyReuseEnabled=reuse,
                 familyReuseDecline=None if reuse else 'disabled',
                 nativeRecordings=families if reuse else jobs)
    if reuse:
        stats.update(familyAdmissionCalls=jobs, familyMisses=families, retainedFamilies=families,
                     familyHits=jobs - families, familyCacheBytes=100 * families,
                     familyIdentityComputations=2 * families, familyIdentityHits=2 * (jobs - families))
    return stats


def query_fixture(label):
    jobs = [{'name': 'first', 'entries': []}, {'name': 'second', 'entries': []}]
    cold = label == 'cold'
    result = {'format': subject.basis.QUERY, 'complete': True, 'status': 'success',
              'inputsUnchanged': True, 'operatorContract': subject.CONTRACT,
              'sourceVersion': SOURCE, 'inputSha256': {**HASHES, 'spec': SPEC_SHA},
              'generatorSha256': BINARY, 'mode': 'u24', 'missingPrograms': [],
              'nativeExpectationProven': False, 'rankingProven': False,
              'usesMonteCarlo': False, 'allTeamCombinationsCovered': False,
              'stats': {'requestedJobs': 2, 'conditionalTermReferences': 4, 'uniqueBasisPrograms': 2,
                        'reusedPrograms': 0 if cold else 2, 'generatedPrograms': 2 if cold else 0,
                        'propagationCalls': 2 if cold else 0, 'storedPrograms': 2},
              'identificationStats': {**family_fixture(), 'propagationCalls': 0, 'verificationCalls': 0}}
    if cold:
        result['generationStats'] = {**family_fixture(), 'propagationCalls': 2, 'verificationCalls': 0}
    return result, jobs


class MissBasisValidationTests(unittest.TestCase):
    def test_retained_families_count_exact_miss_controllers_separately_from_reweighted_minima(self):
        jobs = [{'name': str(index)} for index in range(6)]
        stats = family_fixture(6, 2)
        self.assertEqual(subject.family_counts(stats, jobs, True, 2)['familyHits'], 4)
        changes = [{'nativeRecordings': 1}, {'familyAdmissionCalls': 5}, {'familyMisses': 1},
                   {'familyHits': 3}, {'familyFallbacks': 1}, {'retainedFamilies': 1},
                   {'familyReuseEnabled': False}, {'familyReuseDecline': 'canonicalMissGauge'},
                   {'familyIdentityComputations': False}, {'compiledJobs': 5}]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                subject.family_counts({**stats, **change}, jobs, True, 2)

    def test_independent_recording_cannot_claim_retained_family_work(self):
        jobs = [{'name': 'first'}, {'name': 'second'}]
        stats = family_fixture(2, reuse=False)
        self.assertEqual(subject.family_counts(stats, jobs, False)['nativeRecordings'], 2)
        for change in ({'nativeRecordings': 1}, {'familyAdmissionCalls': 2}, {'familyHits': 1},
                       {'retainedFamilies': 1}, {'familyCacheBytes': 1}, {'familyIdentityHits': 1},
                       {'familyReuseDecline': None}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                subject.family_counts({**stats, **change}, jobs, False)

    def test_cold_and_warm_queries_require_exact_native_work_and_source_identity(self):
        for label in ('cold', 'warm'):
            result, jobs = query_fixture(label)
            subject.query_gate(result, label, HASHES, SPEC_SHA, BINARY, SOURCE, jobs, 1)
            changes = [lambda value: value.update(inputsUnchanged=False),
                       lambda value: value.update(generatorSha256='e' * 64),
                       lambda value: value['identificationStats'].update(propagationCalls=1),
                       lambda value: value['identificationStats'].update(verificationCalls=1),
                       lambda value: value['identificationStats'].update(nativeRecordings=2),
                       lambda value: value['stats'].update(generatedPrograms=1),
                       lambda value: value['stats'].update(propagationCalls=1)]
            if label == 'cold':
                changes.extend((lambda value: value['generationStats'].update(nativeRecordings=2),
                                lambda value: value['generationStats'].update(verificationCalls=1)))
            for change in changes:
                changed = copy.deepcopy(result)
                change(changed)
                with self.subTest(label=label, change=change), self.assertRaises(ValueError):
                    subject.query_gate(changed, label, HASHES, SPEC_SHA, BINARY, SOURCE, jobs, 1)


if __name__ == '__main__':
    unittest.main()
