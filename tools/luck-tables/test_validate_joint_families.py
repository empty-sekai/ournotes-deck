"""Cartesian coverage and native-work accounting contracts."""
from collections import Counter
import copy
from fractions import Fraction
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('joint_family_validation_tests',
                                             Path(__file__).with_name('validate_joint_families.py'))
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)


def mapping(first, second, weight):
    return [{'choices': [0], 'programFingerprint': first, 'weight': [weight, weight]},
            {'choices': [1], 'programFingerprint': second, 'weight': [1 - weight, 1 - weight]}]


def coverage_fixture():
    jobs, mappings = [], {}
    for index, miss in enumerate(((1, 1, 1, 1), (1, 2, 1, 1), (2, 1, 1, 1))):
        for minima, weight in (((1, 1), 0.25), ((5, 5), 0.75)):
            job = subject.grid_job(miss, minima)
            jobs.append(job)
            # The two distinct ordered Miss controls share a complete shape.
            keys = ('a' * 64, 'b' * 64) if index else ('c' * 64, 'd' * 64)
            mappings[job['name']] = mapping(*keys, weight)
    return jobs, mappings


class JointFamilyValidationTests(unittest.TestCase):
    def test_full_cartesian_grid_has_complete_bounded_shards_and_original_holder_geometry(self):
        available = [subject.miss.key('gekisou', 8, 3)]
        available.extend(subject.miss.key('gekisouSupport', skill, level)
                         for skill in (66, 96) for level in range(1, 6))
        source = {'minimum': {'band': 1}}

        class Catalogue:
            def __init__(self, _master):
                pass

            def start(self, _source, _id, level, _matched, _life):
                return {'action': ('minimum', 1, Fraction(level, 10)), 'band': 1, 'rowIds': [level]}

        with patch.object(subject.miss, 'validate_sources', return_value=source), \
             patch.object(subject.miss.catalogue, 'all_chain_keys', return_value=(available, [])), \
             patch.object(subject.miss.operators, 'Catalogue', Catalogue):
            jobs, shards, receipt = subject.make_specs({'master': {}})
        self.assertEqual(len(jobs), 15625)
        self.assertEqual([len(shard) for shard in shards], [3125] * 5)
        self.assertEqual([job for shard in shards for job in shard], jobs)
        families = Counter(subject.nonminimum_key(job) for job in jobs)
        self.assertEqual(len(families), 625)
        self.assertEqual(set(families.values()), {25})
        minimum_pairs = Counter(tuple(key['level'] for key, holder in job['entries']
                                      if key['id'] == 66 and holder < 2) for job in jobs)
        self.assertEqual(len(minimum_pairs), 25)
        self.assertEqual(set(minimum_pairs.values()), {625})
        for shard in shards:
            self.assertEqual(len({subject.nonminimum_key(job) for job in shard}), 125)
        self.assertEqual(receipt['grid']['mainWriters'], 5)
        self.assertEqual(receipt['grid']['supportWriters'], 10)
        for job in (jobs[0], jobs[-1]):
            for holder in range(5):
                held = [key for key, position in job['entries'] if position == holder]
                self.assertEqual([key['id'] for key in held], [8, 66, 96])
                self.assertTrue(all(key['matched'] is False for key in held[1:]))

    def test_covering_subset_checks_two_minimum_laws_for_every_canonical_shape(self):
        jobs, mappings = coverage_fixture()
        selected, receipt = subject.coverage_subset(jobs, mappings)
        self.assertEqual(len(selected), 4)
        self.assertEqual(receipt['exactMissControllers'], 3)
        self.assertEqual(receipt['canonicalConditionalShapes'], 2)
        self.assertEqual(receipt['conditionalPrograms'], 4)
        self.assertTrue(receipt['allGridConditionalProgramsCovered'])
        self.assertTrue(receipt['twoDistinctMinimumDistributionsPerShape'])
        self.assertEqual({term['programFingerprint'] for job in selected for term in mappings[job['name']]},
                         {'a' * 64, 'b' * 64, 'c' * 64, 'd' * 64})

    def test_coverage_refuses_missing_jobs_changed_programs_and_unchanged_minimum_weights(self):
        jobs, mappings = coverage_fixture()
        with self.assertRaises(ValueError):
            subject.coverage_subset(jobs + [jobs[0]], mappings)
        mutations = [lambda value: value.pop(jobs[-1]['name']),
                     lambda value: value[jobs[1]['name']][0].update(programFingerprint='e' * 64),
                     lambda value: value[jobs[1]['name']][0].update(choices=[2]),
                     lambda value: value.update({jobs[1]['name']: copy.deepcopy(value[jobs[0]['name']])})]
        for mutation in mutations:
            changed = copy.deepcopy(mappings)
            mutation(changed)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                subject.coverage_subset(jobs, changed)

    def test_reuse_and_independent_specs_preserve_explicit_limits_and_zero_dp_modes(self):
        jobs, _mappings = coverage_fixture()
        fast, independent = (subject.specification(jobs, reuse) for reuse in (True, False))
        self.assertEqual({key: value for key, value in fast.items() if key != 'basisFamilyReuse'},
                         {key: value for key, value in independent.items() if key != 'basisFamilyReuse'})
        self.assertEqual(fast['mode'], 'basisIdentify')
        self.assertFalse(fast['verifyBasis'])
        self.assertFalse(fast['basisReconstruct'])
        self.assertTrue(fast['canonicalMissGauge'])
        self.assertEqual(fast['basisFamilyBytes'], 128 << 20)
        self.assertEqual(fast['identityBytes'], 256 << 20)
        self.assertEqual(fast['basisResponseBytes'], 256 << 20)


if __name__ == '__main__':
    unittest.main()
