"""Parser and comparison gates, never synthetic performance acceptance."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


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


def report_fixture(*, propagates=True, canonical=True):
    job = {'name': 'parse-only', 'entries': []}
    mode, status = ('programs', 'success') if propagates else ('identify', 'identified')
    contract = subject.CONTRACT if canonical else 'native-ordered-actions/1'
    report = {'format': subject.storage.REPORT, 'mode': mode, 'sourceVersion': SOURCE,
              'canonicalMissGauge': canonical, 'canonicalStartMinimum': False,
              'complete': True, 'probabilityComplete': propagates, 'identificationComplete': True,
              'context': {'algorithmVersion': 'ournotes-luck-response/1/' + SOURCE, 'fingerprint': 'c' * 64},
              'provenance': {'datasetSha256': HASHES['data'], 'inputs': {'rosterSha256': HASHES['snapshot'],
                             'requestSha256': HASHES['request'], 'specSha256': SPEC_SHA}},
              'programs': [{'programIndex': 0, 'fingerprint': FP, 'sourceVersion': SOURCE, 'status': status,
                            'response': {'status': 'success', 'curve': curve()} if propagates else None,
                            'compiledBytes': 100, 'identityBytes': 200,
                            'operatorContract': contract, 'representativeJobIndex': 0}],
              'jobs': [{**job, 'jobIndex': 0, 'programIndex': 0, 'programFingerprint': FP,
                        'status': status, 'operatorContract': contract, 'compiledBytes': 100, 'identityBytes': 200}],
              'stats': {'sourceJobs': 1, 'compiledJobs': 1, 'uniqueRetainedPrograms': 1,
                        'propagationCalls': 1 if propagates else 0, 'propagationMs': 0.1 if propagates else 0.0,
                        'exactProgramAliases': 0, 'retainedIdentityBytes': 1024,
                        'compiledProgramPeakBytes': 100, 'temporaryIdentityPeakBytes': 200,
                        'identityBudgetBytes': subject.IDENTITY_BYTES, 'programBudgetBytes': subject.PROGRAM_BYTES}}
    return report, {'mode': mode, 'jobs': [job], 'canonicalMissGauge': canonical, 'canonicalStartMinimum': False,
                    'identityBytes': subject.IDENTITY_BYTES, 'programBytes': subject.PROGRAM_BYTES,
                    'mcRuns': 0, 'scoreSamples': 0}


def phase_specs():
    return {name + '.json': report_fixture(canonical=canonical)[1]
            for name, canonical in (('original', False), ('canonical', True))}


def native_phase_stub(calls, change=None):
    """Protocol-only output; this stub never represents real-chart acceptance."""
    def call(command, _log, _timeout):
        spec_path, report_path = command[-2:]
        spec = subject.pipeline.read(spec_path)
        calls.append((spec['mode'], spec['canonicalMissGauge']))
        report, _ = report_fixture(propagates=spec['mode'] == 'programs', canonical=spec['canonicalMissGauge'])
        report['provenance']['inputs']['specSha256'] = subject.file_hash(spec_path)
        if change is not None:
            change(spec, report)
        subject.pipeline.write(report_path, report)
    return call


def capacity_refusal(report):
    report.update(complete=False, identificationComplete=False, programs=[])
    report['stats'].update(uniqueRetainedPrograms=0, retainedIdentityBytes=128)
    report['jobs'][0].update(status='capacity', programIndex=None,
                             error='capacity: complete program identity does not fit the interaction identity allowance')


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
                   lambda value: value['stats'].update(retainedIdentityBytes=subject.IDENTITY_BYTES + 1),
                   lambda value: value['stats'].update(compiledProgramPeakBytes=subject.PROGRAM_BYTES + 1),
                   lambda value: value['programs'][0].update(identityBytes=201),
                   lambda value: value['jobs'][0].update(programFingerprint='e' * 64),
                   lambda value: value['programs'][0].update(operatorContract='native-ordered-actions/1'),
                   lambda value: value['programs'][0].update(representativeJobIndex=1),
                   lambda value: value['provenance']['inputs'].update(specSha256='e' * 64)]
        for change in changes:
            changed = copy.deepcopy(report); change(changed)
            with self.assertRaises(ValueError):
                subject.program_report(changed, spec, HASHES, SPEC_SHA, SOURCE)

    def test_preflight_accepts_only_complete_identified_null_response_zero_dp(self):
        self.assertEqual(subject.IDENTITY_BYTES, 256 << 20)
        self.assertEqual(subject.PROGRAM_BYTES, 32 << 20)
        for canonical in (False, True):
            report, spec = report_fixture(propagates=False, canonical=canonical)
            self.assertEqual(subject.program_report(report, spec, HASHES, SPEC_SHA, SOURCE, propagates=False),
                             {'parse-only': {'fingerprint': FP, 'curve': None}})
            changes = [lambda value: value.update(complete=False),
                       lambda value: value.update(identificationComplete=False),
                       lambda value: value.update(probabilityComplete=True),
                       lambda value: value['programs'][0].update(status='success'),
                       lambda value: value['programs'][0].update(response={'status': 'success', 'curve': curve()}),
                       lambda value: value['programs'][0].pop('response'),
                       lambda value: value['programs'][0].update(propagationMs=1),
                       lambda value: value['jobs'][0].update(status='success'),
                       lambda value: value['jobs'][0].update(programFingerprint='e' * 64),
                       lambda value: value['jobs'][0].update(entries=[[subject.key('gekisou', 8, 3), 0]]),
                       lambda value: value['programs'][0].update(sourceVersion='e' * 64),
                       lambda value: value['programs'][0].update(operatorContract='another-contract'),
                       lambda value: value['provenance']['inputs'].update(specSha256='e' * 64),
                       lambda value: value['stats'].update(identityBudgetBytes=128 << 20),
                       lambda value: value['stats'].update(retainedIdentityBytes=subject.IDENTITY_BYTES + 1)]
            for change in changes:
                changed = copy.deepcopy(report); change(changed)
                with self.subTest(canonical=canonical, change=change), self.assertRaises(ValueError):
                    subject.program_report(changed, spec, HASHES, SPEC_SHA, SOURCE, propagates=False)
            for stats in ({'propagationCalls': 1}, {'propagationCalls': False}, {'propagationMs': 0.01}):
                changed = copy.deepcopy(report); changed['stats'].update(stats)
                with self.assertRaisesRegex(ValueError, 'zero DP'):
                    subject.program_report(changed, spec, HASHES, SPEC_SHA, SOURCE, propagates=False)

    def test_capacity_preflight_names_the_refusal_and_explicit_allowance(self):
        report, spec = report_fixture(propagates=False)
        capacity_refusal(report)
        with self.assertRaisesRegex(ValueError, r'capacity refusal before any DP: 1/1 jobs; identityBytes=268435456'):
            subject.program_report(report, spec, HASHES, SPEC_SHA, SOURCE, propagates=False)

    def test_both_preflights_finish_before_either_probability_run(self):
        calls, result = [], {'phases': {}}
        with tempfile.TemporaryDirectory() as temporary, patch.object(subject.storage, 'call', native_phase_stub(calls)):
            output = Path(temporary)
            reports, mappings = subject.run_program_phases(phase_specs(), HASHES, 'unused-generator', SOURCE,
                                                           HASHES, output, result)
            self.assertEqual(calls, [('identify', False), ('identify', True), ('programs', False), ('programs', True)])
            self.assertEqual(set(reports), {'original', 'canonical'})
            self.assertEqual(mappings['original']['parse-only']['curve'], curve())
            stored = subject.pipeline.read(output / 'validation.json')
            self.assertTrue(stored['preflight']['bothPassedBeforeAnyPropagation'])
            self.assertEqual(stored['preflight']['propagationCalls'], 0)
            for phase in ('original', 'canonical'):
                preflight = stored['phases'][phase + '-preflight']
                self.assertEqual(preflight['status'], 'success')
                self.assertEqual(preflight['nativeStats']['propagationCalls'], 0)
                for kind in ('spec', 'report'):
                    path = output / preflight[kind]['path']
                    self.assertEqual(subject.file_hash(path), preflight[kind]['sha256'])
                self.assertTrue(stored['phases'][phase]['preflightBinding']['allProgramFingerprintsMatch'])

    def test_either_preflight_capacity_refusal_prevents_every_probability_call(self):
        for refused_canonical in (False, True):
            calls, result = [], {'phases': {}}
            def change(spec, report):
                if spec['mode'] == 'identify' and spec['canonicalMissGauge'] is refused_canonical:
                    capacity_refusal(report)
            with tempfile.TemporaryDirectory() as temporary, patch.object(subject.storage, 'call', native_phase_stub(calls, change)):
                output = Path(temporary)
                with self.assertRaisesRegex(ValueError, 'capacity refusal before any DP'):
                    subject.run_program_phases(phase_specs(), HASHES, 'unused-generator', SOURCE, HASHES, output, result)
                self.assertTrue(calls and all(mode == 'identify' for mode, _ in calls))
                self.assertEqual(len(calls), 2 if refused_canonical else 1)
                stored = subject.pipeline.read(output / 'validation.json')
                self.assertFalse(stored['preflight']['complete'])
                self.assertNotIn('original', stored['phases'])
                self.assertNotIn('canonical', stored['phases'])
                for phase in ('original', 'canonical'):
                    self.assertTrue((output / 'specs' / (phase + '-preflight.json')).is_file())
                refused = 'canonical-preflight' if refused_canonical else 'original-preflight'
                self.assertEqual(stored['phases'][refused]['status'], 'error')
                self.assertIn('report', stored['phases'][refused])

    def test_formal_programs_cannot_replace_the_preflight_fingerprint(self):
        calls, result = [], {'phases': {}}
        def change(spec, report):
            if spec['mode'] == 'programs':
                report['programs'][0]['fingerprint'] = 'e' * 64
                report['jobs'][0]['programFingerprint'] = 'e' * 64
        with tempfile.TemporaryDirectory() as temporary, patch.object(subject.storage, 'call', native_phase_stub(calls, change)):
            output = Path(temporary)
            with self.assertRaisesRegex(ValueError, 'changed its preflight program fingerprint'):
                subject.run_program_phases(phase_specs(), HASHES, 'unused-generator', SOURCE, HASHES, output, result)
            self.assertEqual(calls, [('identify', False), ('identify', True), ('programs', False)])
            stored = subject.pipeline.read(output / 'validation.json')
            self.assertEqual(stored['phases']['original']['status'], 'error')

    def test_preflight_binding_checks_each_job_even_when_the_fp_union_matches(self):
        report, _ = report_fixture()
        mapping = {'first': {'fingerprint': FP}, 'second': {'fingerprint': 'e' * 64}}
        swapped = {'first': mapping['second'], 'second': mapping['first']}
        self.assertEqual({row['fingerprint'] for row in mapping.values()},
                         {row['fingerprint'] for row in swapped.values()})
        with self.assertRaisesRegex(ValueError, 'preflight program fingerprint: first'):
            subject.bind_preflight(report, mapping, report, swapped)
        changed = copy.deepcopy(report); changed['context']['fingerprint'] = 'e' * 64
        with self.assertRaisesRegex(ValueError, 'preflight native input'):
            subject.bind_preflight(report, mapping, changed, mapping)

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
