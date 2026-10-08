"""Receipt and gate parsing only; no synthetic simulator or performance benchmark is run."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('minimum_validation_tests', Path(__file__).with_name('validate_minimum_basis.py'))
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)
SOURCE, FP = 'a' * 64, 'b' * 64
HASHES = {'data': '1' * 64, 'snapshot': '2' * 64, 'request': '3' * 64}
SPEC_SHA = '4' * 64


def verification_fixture():
    jobs = [{'name': 'parse-' + str(index), 'entries': []} for index in range(6)]
    program = {'programIndex': 0, 'fingerprint': FP, 'sourceVersion': SOURCE,
               'operatorContract': subject.basis.CONTRACT, 'status': 'success', 'response': {'status': 'success'}}
    rows = [{**job, 'jobIndex': index, 'status': 'success', 'termCount': 1, 'response': {'status': 'success'},
             'components': [{'programIndex': 0, 'programFingerprint': FP, 'weight': [1, 1]}],
             'verification': {'status': 'success', 'disjointIntervals': 0, 'comparedBuckets': 4,
                              'probesEqual': True, 'probeTransitionsEqual': True, 'maximumEndpointDifference': 1e-12}}
            for index, job in enumerate(jobs)]
    report = {'format': subject.basis.REPORT, 'mode': 'basisPrograms', 'operatorContract': subject.basis.CONTRACT,
              'sourceVersion': SOURCE, 'complete': True, 'identificationComplete': True, 'probabilityComplete': True,
              'verificationRequested': True, 'verificationComplete': True, 'missingSelectedPrograms': [],
              'context': {'algorithmVersion': 'ournotes-luck-response/1/' + SOURCE, 'fingerprint': 'c' * 64},
              'programs': [program], 'jobs': rows,
              'provenance': {'datasetSha256': HASHES['data'], 'inputs': {'rosterSha256': HASHES['snapshot'],
                             'requestSha256': HASHES['request'], 'specSha256': SPEC_SHA}},
              'stats': {'requestedJobs': 6, 'compiledJobs': 6, 'verificationCalls': 6, 'verifiedJobs': 6,
                        'reconstructedJobs': 6, 'uniquePrograms': 1, 'propagationCalls': 1}}
    return report, {'jobs': jobs, 'verifyBasis': True}


class MinimumValidationTests(unittest.TestCase):
    def test_all_six_independent_comparisons_and_actual_work_counts_are_required(self):
        report, spec = verification_fixture()
        checked = subject.verification(report, spec, HASHES, SPEC_SHA, SOURCE)
        self.assertEqual(checked['verifiedJobs'], 6)
        self.assertEqual(checked['comparedBuckets'], 24)
        self.assertEqual(checked['disjointIntervals'], 0)
        self.assertEqual(checked['maximumEndpointDifference'], 1e-12)
        mutations = [lambda value: value.update(verificationComplete=False),
                     lambda value: value['stats'].update(verifiedJobs=5),
                     lambda value: value['stats'].update(verificationCalls=0),
                     lambda value: value['stats'].update(propagationCalls=0),
                     lambda value: value['jobs'][0]['verification'].update(disjointIntervals=1),
                     lambda value: value['jobs'][0]['verification'].update(comparedBuckets=0),
                     lambda value: value['provenance']['inputs'].update(specSha256='f' * 64)]
        for mutation in mutations:
            changed = copy.deepcopy(report); mutation(changed)
            with self.assertRaises(ValueError):
                subject.verification(changed, spec, HASHES, SPEC_SHA, SOURCE)

    def test_minimum_only_queries_cannot_silently_generate_or_grow_the_dictionary(self):
        stats = {'generatedPrograms': 0, 'propagationCalls': 0, 'reusedPrograms': 3,
                 'uniqueBasisPrograms': 3, 'storedPrograms': 4}
        subject.require_reuse(stats, 4)
        for change in ({'generatedPrograms': 1}, {'propagationCalls': 1}, {'storedPrograms': 5}, {'reusedPrograms': 2}):
            with self.assertRaises(ValueError):
                subject.require_reuse({**stats, **change}, 4)

    def test_query_missing_status_and_original_provenance_are_explicit(self):
        jobs = [{'name': 'parse', 'entries': []}]
        result = {'format': subject.basis.QUERY, 'sourceVersion': SOURCE, 'inputSha256': {**HASHES, 'spec': SPEC_SHA},
                  'inputsUnchanged': True, 'complete': False, 'status': 'missing',
                  'stats': {'requestedJobs': 1, 'conditionalTermReferences': 3, 'uniqueBasisPrograms': 3,
                            'reusedPrograms': 0, 'generatedPrograms': 0, 'propagationCalls': 0, 'storedPrograms': 4}}
        self.assertEqual(subject.query_counts(result, jobs, HASHES, SPEC_SHA, SOURCE, complete=False)['storedPrograms'], 4)
        with self.assertRaises(ValueError):
            subject.query_counts(result, jobs, HASHES, SPEC_SHA, SOURCE)
        for change in ({'inputsUnchanged': False}, {'sourceVersion': 'f' * 64}, {'inputSha256': HASHES}):
            with self.assertRaises(ValueError):
                subject.query_counts({**result, **change}, jobs, HASHES, SPEC_SHA, SOURCE, complete=False)

    def test_only_exact_existing_manifest_cases_can_be_selected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'benchmark.json'
            subject.pipeline.write(path, {'format': 'ournotes-deck.search-benchmark/1', 'datasetId': HASHES['data'],
                                          'cases': [{'name': 'original-one'}, {'name': 'original-two'}]})
            before = path.read_bytes()
            dataset, cases = subject.selected_cases(path, ['original-two'])
            self.assertEqual(dataset, HASHES['data'])
            self.assertEqual(cases, [{'name': 'original-two'}])
            self.assertEqual(path.read_bytes(), before)
            for names in ([], ['invented'], ['original-one', 'original-one']):
                with self.assertRaises(ValueError):
                    subject.selected_cases(path, names)


if __name__ == '__main__':
    unittest.main()
