"""Receipt/admission gates only; these fixtures are not simulated performance cases."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('basis_family_validation_tests',
                                             Path(__file__).with_name('validate_basis_families.py'))
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)
SOURCE, FP = 'a' * 64, 'b' * 64
HASHES = {'data': '1' * 64, 'snapshot': '2' * 64, 'request': '3' * 64}
SPEC_SHA = '4' * 64


def counts(jobs, reuse):
    result = dict.fromkeys(subject.FAMILY_COUNTS, 0)
    result.update(requestedJobs=jobs, compiledJobs=jobs, nativeRecordings=1 if reuse else jobs)
    if reuse:
        result.update(familyAdmissionCalls=jobs, familyHits=jobs - 1, familyMisses=1,
                      retainedFamilies=1, familyCacheBytes=123, familyIdentityComputations=1,
                      familyIdentityHits=jobs - 1)
    return result


def verification_fixture():
    jobs = [{'name': 'receipt-' + str(index), 'entries': []} for index in range(2)]
    program = {'programIndex': 0, 'fingerprint': FP, 'sourceVersion': SOURCE,
               'operatorContract': subject.basis.CONTRACT, 'status': 'success',
               'response': {'status': 'success'}, 'identityVersion': 'receipt-test/1',
               'representativeJobIndex': 0, 'representativeTermIndex': 0}
    rows = [{**job, 'jobIndex': index, 'status': 'success', 'termCount': 1, 'startCount': 1,
             'response': {'status': 'success'},
             'components': [{'programIndex': 0, 'programFingerprint': FP, 'weight': [1, 1], 'choices': [0]}],
             'verification': {'status': 'success', 'disjointIntervals': 0, 'comparedBuckets': 4,
                              'probesEqual': True, 'probeTransitionsEqual': True,
                              'maximumEndpointDifference': 1e-12}}
            for index, job in enumerate(jobs)]
    report = {'format': subject.basis.REPORT, 'mode': 'basisPrograms', 'operatorContract': subject.basis.CONTRACT,
              'sourceVersion': SOURCE, 'complete': True, 'identificationComplete': True, 'probabilityComplete': True,
              'verificationRequested': True, 'verificationComplete': True, 'missingSelectedPrograms': [],
              'context': {'algorithmVersion': 'ournotes-luck-response/1/' + SOURCE, 'fingerprint': 'c' * 64},
              'programs': [program], 'jobs': rows,
              'provenance': {'datasetSha256': HASHES['data'], 'inputs': {'rosterSha256': HASHES['snapshot'],
                             'requestSha256': HASHES['request'], 'specSha256': SPEC_SHA}},
              'stats': {**counts(2, False), 'verificationCalls': 2, 'verifiedJobs': 2,
                        'reconstructedJobs': 2, 'uniquePrograms': 1, 'propagationCalls': 1}}
    return report, {'jobs': jobs, 'verifyBasis': True, 'basisFamilyReuse': False}


class BasisFamilyValidationTests(unittest.TestCase):
    def test_every_independent_original_dp_and_conditional_term_must_complete(self):
        report, spec = verification_fixture()
        mapping, programs, result = subject.verified_programs(report, spec, HASHES, SPEC_SHA, SOURCE)
        self.assertEqual(result['verifiedJobs'], 2)
        self.assertEqual(result['comparedBuckets'], 8)
        self.assertEqual(result['originalDpVerificationCalls'], 2)
        self.assertEqual(len(mapping), 2)
        self.assertEqual(len(programs), 1)
        changes = [lambda value: value.update(verificationComplete=False),
                   lambda value: value['stats'].update(verificationCalls=0),
                   lambda value: value['stats'].update(verifiedJobs=1),
                   lambda value: value['stats'].update(propagationCalls=0),
                   lambda value: value['stats'].update(nativeRecordings=1),
                   lambda value: value['jobs'][0]['verification'].update(disjointIntervals=1),
                   lambda value: value['jobs'][0]['verification'].update(comparedBuckets=0),
                   lambda value: value['jobs'][0]['components'][0].update(weight=[float('nan'), 1]),
                   lambda value: value['jobs'][0]['components'][0].update(weight=[0, 0]),
                   lambda value: value['jobs'][0]['components'][0].update(choices=[2, 0]),
                   lambda value: value['programs'][0].update(representativeTermIndex=1),
                   lambda value: value['provenance']['inputs'].update(specSha256='f' * 64)]
        for change in changes:
            modified = copy.deepcopy(report); change(modified)
            with self.assertRaises(ValueError):
                subject.verified_programs(modified, spec, HASHES, SPEC_SHA, SOURCE)

    def test_family_hits_and_actual_recordings_are_separate_from_logical_jobs(self):
        jobs = [{'name': 'a'}, {'name': 'b'}]
        fast = {'stats': counts(2, True)}
        subject.family_counts(fast, jobs, True, single_family=True)
        subject.family_counts({'stats': counts(2, False)}, jobs, False)
        for changed in ({'nativeRecordings': 0}, {'nativeRecordings': 2}, {'familyHits': 0},
                        {'familyFallbacks': 1}, {'retainedFamilies': 2}, {'familyCacheBytes': -1}):
            with self.assertRaises(ValueError):
                subject.family_counts({'stats': {**fast['stats'], **changed}}, jobs, True, single_family=True)
        with self.assertRaises(ValueError):
            subject.family_counts(fast, jobs, False)

    def test_mapping_comparison_requires_exact_weight_and_controller_identity(self):
        left = {'label': [{'programFingerprint': FP, 'weight': [0.5, 0.5], 'choices': [1]}]}
        self.assertEqual(subject.compare_mappings(left, copy.deepcopy(left))['comparedJobs'], 1)
        for change in ({'programFingerprint': 'e' * 64}, {'weight': [0.5, 0.5000000000000001]}, {'choices': [2]}):
            changed = copy.deepcopy(left); changed['label'][0].update(change)
            with self.assertRaises(ValueError):
                subject.compare_mappings(left, changed)
        with self.assertRaises(ValueError):
            subject.compare_mappings(left, {'missing': left['label']})

    def test_nonminimum_controls_cannot_reuse_a_base_family_program(self):
        mappings = {'family-control-base': [{'programFingerprint': FP}]}
        for ordinal, kind in enumerate(('start-gauge', 'miss-gauge', 'gauge-speed')):
            mappings['family-control-' + kind] = [{'programFingerprint': str(ordinal) * 64}]
        self.assertEqual(len(subject.control_separation(mappings)), 3)
        for kind in ('start-gauge', 'miss-gauge', 'gauge-speed'):
            changed = copy.deepcopy(mappings)
            changed['family-control-' + kind].append({'programFingerprint': FP})
            with self.assertRaises(ValueError):
                subject.control_separation(changed)

    def test_score_id_sharding_accounts_for_every_record_without_ordinal_drift(self):
        cases = [{'scoreId': score} for score in (101, 102, 103, 104, 108)]
        partitions = [subject.shard_cases(cases, index, 4) for index in range(4)]
        self.assertEqual(sorted(row['scoreId'] for shard in partitions for row in shard),
                         sorted(row['scoreId'] for row in cases))
        extended = [{'scoreId': 100}] + cases
        for index, before in enumerate(partitions):
            after = subject.shard_cases(extended, index, 4)
            self.assertEqual(before, [row for row in after if row['scoreId'] != 100])
        for index, count in ((0, 0), (1, 1), (-1, 2), (True, 2), (0, 1025)):
            with self.assertRaises(ValueError):
                subject.shard_cases(cases, index, count)

    def test_manifest_gate_rejects_missing_real_chart_or_changed_mission_binding(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            subject.pipeline.write(root / 'data.json', {'unitTest': 'manifest parsing only'})
            dataset = subject.file_hash(root / 'data.json')
            subject.pipeline.write(root / 'snapshot.json', {'datasetId': dataset})
            charts = [{'scoreId': 100, 'musicId': 1, 'difficulty': 'easy', 'hasLuckMission': False},
                      {'scoreId': 101, 'musicId': 1, 'difficulty': 'normal', 'hasLuckMission': True}]
            keys = [{'source': 'gekisou', 'id': 8, 'level': 3, 'matched': None}]
            inventory = {'datasetId': dataset, 'charts': charts, 'luckCatalogueKeys': keys}
            cases = []
            for chart in charts:
                name = str(chart['scoreId'])
                request = {'scenario': {'kind': 'mission', 'musicId': 1},
                           'execution': {'kind': 'live', 'scoreId': chart['scoreId'], 'gekisou': True}}
                subject.pipeline.write(root / (name + '.json'), request)
                cases.append({**chart, 'name': name, 'request': name + '.json',
                              'data': 'data.json', 'snapshot': 'snapshot.json'})
            manifest = {'format': 'ournotes-deck.search-benchmark/1', 'datasetId': dataset, 'cases': cases,
                        'luckCatalogue': {'format': subject.catalogue.INVENTORY_FORMAT,
                                          'selection': 'all-master-source-levels',
                                          'includeNonLuckCharts': True, 'keys': keys}}
            path = root / 'benchmark.json'
            subject.pipeline.write(path, manifest)
            with patch.object(subject.catalogue, 'inventory', return_value=inventory), \
                 patch.object(subject.pipeline, 'luck_case', side_effect=lambda data, request:
                              request['execution']['scoreId'] == 101):
                self.assertEqual(len(subject.declared_cases(path)[3]), 2)
                missing = {**manifest, 'cases': cases[:1]}
                subject.pipeline.write(path, missing)
                with self.assertRaises(ValueError):
                    subject.declared_cases(path)
                subject.pipeline.write(path, manifest)
                request = subject.pipeline.read(root / '101.json')
                request['scenario']['musicId'] = 99
                subject.pipeline.write(root / '101.json', request)
                with self.assertRaises(ValueError):
                    subject.declared_cases(path)


if __name__ == '__main__':
    unittest.main()
