import copy
import itertools
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import validate_speed_profiles as subject


def specs():
    with patch.object(subject, 'sources', return_value=(subject.EXPECTED.copy(), {})):
        return subject.make_specs({})


def native_row(skill=7, duration=2):
    row = {'_id': 1, '_gekisouSkillID': skill, '_level': 1, '_skillEffectType': 11001,
           '_skillTriggerType': 1 if duration else 2, '_skillTriggerConditionGroup': 1,
           '_skillConditionGroup': 0, '_skillReleaseConditionGroup': 0,
           '_skillTargetIDs': [], '_activationTimeSecond': duration, '_effectValue': 10000,
           '_maxEffectValue': 0, '_effectLimitCount': 0, '_skillCumulativeConditionID': 0,
           '_effectExecuteLimitCount': 0, '_effectExecuteLimitResetConditionGroup': 0}
    trigger = {'_conditionType': 7010 if duration else 7020, '_conditionValues': [],
               '_isPositive': True, '_conditionTargetIDs': [56]}
    groups = {0: [], 1: [trigger]}
    master = SimpleNamespace(phases={11001: {'_phase': 2}, 3004: {'_phase': 2}},
                             group=lambda identity: groups[identity],
                             targets={56: {'_skillTargetType': 5, '_gekisouMissionType': 2}})
    return master, {'_id': skill, '_gekisouMissionType': 2}, [row]


def reference_fixture():
    _, spec, _, _ = specs()
    source, digest = 'a' * 64, 'b' * 64
    hashes = {key: digest for key in ('data', 'snapshot', 'request')}
    curve = {'steps': [{'timeMs': 0, 'buckets': [[1.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]}],
             'probes': [True], 'probeTransitions': [1], 'rangeMoments': []}
    context = {'algorithmVersion': 'ournotes-luck-response/1/' + source, 'fingerprint': digest}
    identified = {'sharedFingerprint': digest, 'dependencyDescriptor': {'declared': True}, 'capabilities': {}}
    report = {**copy.deepcopy(identified), 'format': 'ournotes-deck.luck-response-generation/1', 'mode': 'generate',
              'context': context, 'cacheBytes': 0, 'validationDecks': [],
              'provenance': {'datasetSha256': digest, 'inputs': {'rosterSha256': digest,
                    'requestSha256': digest, 'specSha256': digest}},
              'cacheStats': {'propagatedCurves': len(spec['jobs']), 'hits': 0, 'recordingHits': 0,
                             'sharedRecordingHits': 0, 'lifeRecordingHits': 0},
              'table': {'context': context, 'entries': []}, 'jobs': []}
    for index, job in enumerate(spec['jobs']):
        report['jobs'].append({**copy.deepcopy(job), 'status': 'success', 'entryIndex': index})
        report['table']['entries'].append({'key': copy.deepcopy(job['entries']),
                                           'response': {'status': 'success', 'curve': copy.deepcopy(curve)}})
    return report, spec, identified, hashes, digest, source


class SpeedProfilesTest(unittest.TestCase):
    def test_complete_ordered_pair_domain_and_uncached_witnesses(self):
        identify, reference, profiles, recipe = specs()
        self.assertEqual(len(identify['jobs']), 421)
        self.assertEqual(len(reference['jobs']), 18)
        self.assertEqual(reference['cacheBytes'], 0)
        self.assertEqual(len(set(profiles.values())), 138)
        pair_inputs = {tuple((key['id'], key['level']) for key, _ in job['entries'])
                       for job in identify['jobs'] if len(job['entries']) == 2}
        self.assertEqual(pair_inputs, set(itertools.product(subject.EXPECTED, repeat=2)))
        self.assertEqual(recipe['parameterVectorSpace']['includingEmpty'], 8754)
        self.assertFalse(recipe['parameterVectorSpace']['nativeProgramsCounted'])

    def test_separate_duration_bins_and_full_bounded_vector_count(self):
        for left, right in subject.ALIASES:
            self.assertEqual(subject.profile(left, subject.EXPECTED), subject.profile(right, subject.EXPECTED))
        left, right = subject.COUNTEREXAMPLE
        self.assertNotEqual(subject.profile(left, subject.EXPECTED), subject.profile(right, subject.EXPECTED))
        for keys in (left, right):
            self.assertEqual(sum(subject.EXPECTED[key][0] for key in keys), 2)
            self.assertEqual(sum(a * b for a, b in (subject.EXPECTED[key] for key in keys)), 7)
        vectors = set(subject.profile((key,), subject.EXPECTED) for key in subject.EXPECTED)
        independent = {(0,) * 6}
        for count in range(1, 6):
            for entries in itertools.combinations_with_replacement(vectors, count):
                independent.add(tuple(map(sum, zip(*entries))))
        self.assertEqual(len(independent), 8754)
        for count in (-1, 6, True):
            with self.assertRaises(ValueError):
                subject.vector_space(subject.EXPECTED, count)

    def test_source_domain_refuses_fractional_speed_and_new_state_dependencies(self):
        master, metadata, rows = native_row()
        self.assertEqual(subject.source_row(master, metadata, rows), (1, 2))
        for field, value in (('_effectValue', 15000), ('_activationTimeSecond', 2.5),
                             ('_skillTriggerType', 2), ('_skillTargetIDs', [56]),
                             ('_skillReleaseConditionGroup', 1), ('_effectExecuteLimitCount', 1),
                             ('_skillCumulativeConditionID', 1)):
            changed = copy.deepcopy(rows)
            changed[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                subject.source_row(master, metadata, changed)
        master.phases[11001]['_phase'] = 1
        with self.assertRaises(ValueError):
            subject.source_row(master, metadata, rows)

    def test_full_source_keeps_damage_reduction_dependency_visible(self):
        master, metadata, rows = native_row(skill=22, duration=3)
        rows[0]['_effectValue'] = 20000
        with self.assertRaises(ValueError):
            subject.source_row(master, metadata, rows)
        damage = {**rows[0], '_id': 2, '_skillEffectType': 3004, '_effectValue': 2000}
        rows.append(damage)
        self.assertEqual(subject.source_row(master, metadata, rows), (2, 3))
        damage['_activationTimeSecond'] = 4
        with self.assertRaises(ValueError):
            subject.source_row(master, metadata, rows)

    def test_reference_gate_requires_original_inputs_and_one_dp_per_witness(self):
        args = reference_fixture()
        result = subject.reference_gate(*args)
        self.assertEqual(result['independentOriginalDps'], 18)
        self.assertEqual(result['aliasComparisons'], 8)
        changes = [lambda report: report.update(cacheBytes=1),
                   lambda report: report['cacheStats'].update(propagatedCurves=17),
                   lambda report: report['cacheStats'].update(hits=1),
                   lambda report: report['cacheStats'].update(lifeRecordingHits=1),
                   lambda report: report['provenance']['inputs'].update(specSha256='c' * 64),
                   lambda report: report['dependencyDescriptor'].update(extra=True),
                   lambda report: report['jobs'][1].update(entryIndex=0),
                   lambda report: report['jobs'][0].update(reusedExactEntry=True),
                   lambda report: report['table']['entries'].pop()]
        for change in changes:
            changed = copy.deepcopy(args)
            change(changed[0])
            with self.subTest(change=change), self.assertRaises(ValueError):
                subject.reference_gate(*changed)


if __name__ == '__main__':
    unittest.main()
