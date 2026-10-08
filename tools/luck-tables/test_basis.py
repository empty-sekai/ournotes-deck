"""Conditional dictionary transport contracts; no simulated game or codec claims."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('luck_basis_tests_subject', Path(__file__).with_name('basis.py'))
basis = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(basis)
pipeline, storage = basis.pipeline, basis.storage
SOURCE = '1' * 64


def job(name, skill, position=0):
    return {'name': name, 'entries': [[{'source': 'gekisou', 'id': skill, 'level': 1, 'matched': None}, position]]}


class BasisTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.inputs = {}
        for name in ('data', 'snapshot', 'request'):
            path = self.root / (name + '.json'); pipeline.write(path, {'input': name}); self.inputs[name] = path
        self.jobs = [job('first-chance', 7), job('second-chance', 8), job('position-alias', 7, 4)]
        self.original = {'jobs': self.jobs, 'mcRuns': 100, 'scoreSamples': 10, 'verifyBasis': True, 'basisReconstruct': True,
                         'basisPrograms': ['f' * 64], 'validationDecks': [{'name': 'not requested by query'}],
                         'contextDeck': {'name': 'preserved-anchor', 'members': [1, 2, 3, 4, 5], 'snaps': [None] * 5}}
        self.spec = self.root / 'spec.json'; pipeline.write(self.spec, self.original)
        self.generator = self.root / 'generator'; self.generator.write_bytes(b'fake native transport')
        self.store = self.root / 'store'; self.calls = []; self.ordinal = 0
        self.mutate_identify = self.mutate_generated = self.mutate_pack = None
        self.fail_materialize = False

    def tearDown(self):
        self.temp.cleanup()

    def report(self, spec, spec_path):
        identify = spec['mode'] == 'basisIdentify'
        jobs = spec['jobs']; selected = set(spec.get('basisPrograms', []))
        unique, programs, rows = {}, [], []
        for ordinal, value in enumerate(jobs):
            skills = [entry[0]['id'] for entry in value['entries']]
            # This fake transport varies mixture weights while preserving the
            # same conditional identities. It asserts no game probability law.
            lower_probability = 1.0
            for skill in skills:
                lower_probability *= {7: 0.75, 8: 0.25, 9: 0.5}[skill]
            components = []
            for term, minimum in enumerate((0, 2)):
                key = ('changed-controller' if 9 in skills else 'same-controller') + '/' + str(minimum)
                fingerprint = pipeline.sha(key.encode())
                if fingerprint not in unique:
                    index = len(programs); unique[fingerprint] = index
                    propagated = not identify and fingerprint in selected
                    programs.append({'programIndex': index, 'fingerprint': fingerprint, 'sourceVersion': SOURCE,
                        'identityVersion': 'test-conditional-domain/' + SOURCE, 'operatorContract': basis.CONTRACT,
                        'representativeJobIndex': ordinal, 'representativeTermIndex': term,
                        'status': 'success' if propagated else 'identified',
                        'response': {'status': 'success', 'curve': {'steps': []}} if propagated else None})
                weight = lower_probability if term == 0 else 1 - lower_probability
                components.append({'programIndex': unique[fingerprint], 'programFingerprint': fingerprint,
                                   'weight': [weight, weight], 'choices': [minimum]})
            success = all(component['programFingerprint'] in selected for component in components) and not identify
            rows.append({**value, 'jobIndex': ordinal, 'status': 'success' if success else 'identified',
                         'startCount': 1, 'termCount': 2, 'components': components})
        hashes = pipeline.provenance(self.inputs)
        context = {'algorithmVersion': 'ournotes-luck-response/1/' + SOURCE,
                   'fingerprint': pipeline.sha(pipeline.canonical([job['entries'] for job in jobs]))}
        return {'format': basis.REPORT, 'mode': spec['mode'], 'sourceVersion': SOURCE,
                'identificationComplete': True, 'complete': True, 'probabilityComplete': not identify,
                'operatorContract': basis.CONTRACT, 'context': context, 'sharedFingerprint': 'a' * 64,
                'dependencyDescriptor': {'algorithm': {'simSourceSha256': SOURCE}}, 'capabilities': {'chain': []},
                'provenance': {'contextDeck': spec.get('contextDeck'), 'datasetSha256': hashes['data'], 'inputs': {
                    'rosterSha256': hashes['snapshot'], 'requestSha256': hashes['request'],
                    'specSha256': pipeline.sha(Path(spec_path).read_bytes())}},
                'missingSelectedPrograms': [], 'jobs': rows, 'programs': programs,
                'stats': {'propagationCalls': 0 if identify else len(selected), 'verificationCalls': 0}}

    def native(self, command, log, timeout):
        command = [str(value) for value in command]; action = command[1]
        if action not in ('program-pack', 'basis-materialize', 'pack', 'predict'):
            spec = pipeline.read(command[-2]); identify = spec['mode'] == 'basisIdentify'
            self.calls.append((spec['mode'], [job['name'] for job in spec['jobs']], spec.get('basisPrograms')))
            self.assertEqual(spec['mcRuns'], 0); self.assertEqual(spec['scoreSamples'], 0)
            self.assertFalse(spec['verifyBasis']); self.assertEqual(spec['validationDecks'], [])
            self.assertFalse(spec['basisReconstruct'])
            self.assertEqual(spec['basisMaxTerms'], 64)
            self.assertEqual(spec['contextDeck'], self.original['contextDeck'])
            if identify:
                self.assertNotIn('basisPrograms', spec)
            value = self.report(spec, command[-2])
            mutation = self.mutate_identify if identify else self.mutate_generated
            if mutation:
                mutation(value)
            pipeline.write(command[-1], value)
        elif action == 'program-pack':
            self.calls.append((action, [], None)); report = pipeline.read(command[2])
            self.assertEqual(report['format'], storage.REPORT)
            self.assertEqual(report['kind'], 'conditionalProgramStorageProjection')
            self.assertTrue(all(row['name'].startswith('basis-control-') and row['entries'] == [] for row in report['jobs']))
            mode, directory = command[3], Path(command[4]); rows = []
            for body in report['programs']:
                raw = pipeline.canonical({'conditionalResponse': body['fingerprint'], 'source': SOURCE, 'mode': mode})
                digest = pipeline.sha(raw); relative = 'blobs/' + digest + '.onlrsp'
                pipeline.atomic_bytes(directory / relative, raw)
                rows.append({'fingerprint': body['fingerprint'], 'sourceVersion': SOURCE, 'status': 'success',
                    'archive': {'path': relative, 'sha256': digest, 'bytes': len(raw), 'mode': mode, 'verifiedEntries': 1}})
            value = {'format': storage.INDEX, 'sourceVersion': SOURCE, 'mode': mode, 'complete': True,
                     'inputReportSha256': pipeline.sha(Path(command[2]).read_bytes()),
                     'requestedJobs': len(report['jobs']), 'programs': rows}
            if self.mutate_pack:
                self.mutate_pack(value)
            pipeline.write(directory / 'program-index.json', value)
        elif action == 'basis-materialize':
            self.calls.append((action, [], None))
            if self.fail_materialize:
                raise ValueError('native mixture refused incompatible term curves')
            report = pipeline.read(command[2]); index = pipeline.read(command[3])
            self.assertEqual(report['mode'], 'basisIdentify')
            self.assertLessEqual({p['fingerprint'] for p in report['programs']}, {p['fingerprint'] for p in index['programs']})
            pipeline.write(command[-1], {'table': {'context': report['context'], 'entries': []}})
        elif action == 'pack':
            self.calls.append((action, [], None)); pipeline.atomic_bytes(Path(command[-1]), b'fake mixed archive')
        elif action == 'predict':
            self.calls.append((action, [], None)); pipeline.write(command[-1], {'status': 'success', 'ordersScored': 120})

    def query(self, **kwargs):
        self.ordinal += 1
        return basis.query(self.inputs['data'], self.inputs['snapshot'], self.inputs['request'], self.spec,
                           self.generator, self.store, self.root / ('query-' + str(self.ordinal)),
                           run_command=self.native, **kwargs)

    def index(self):
        return self.store / SOURCE / 'u24/program-index.json'

    def test_cold_and_new_weights_or_combination_reuse_same_conditional_programs(self):
        original = self.spec.read_bytes(); cold = self.query()
        self.assertTrue(cold['complete'], cold)
        self.assertEqual(cold['stats']['requestedJobs'], 3)
        self.assertEqual(cold['stats']['conditionalTermReferences'], 6)
        self.assertEqual(cold['stats']['uniqueBasisPrograms'], 2)
        self.assertEqual(cold['stats']['generatedPrograms'], 2)
        generated = [call for call in self.calls if call[0] == 'basisPrograms']
        self.assertEqual(len(generated), 1); self.assertEqual(generated[0][1], [job['name'] for job in self.jobs])
        self.assertEqual(len(generated[0][2]), 2)
        before_index = self.index().read_bytes(); self.calls.clear()
        self.assertTrue(self.query()['complete'])
        self.assertFalse(any(call[0] == 'basisPrograms' for call in self.calls))
        self.assertEqual(self.spec.read_bytes(), original)
        combination = {'name': 'new-combination', 'entries': self.jobs[0]['entries'] + job('other', 8, 3)['entries']}
        pipeline.write(self.spec, {**self.original, 'jobs': self.jobs + [combination]})
        self.calls.clear(); combined = self.query()
        self.assertTrue(combined['complete']); self.assertEqual(combined['stats']['conditionalTermReferences'], 8)
        self.assertEqual(combined['stats']['generatedPrograms'], 0); self.assertEqual(combined['stats']['reusedPrograms'], 2)
        self.assertFalse(any(call[0] == 'basisPrograms' for call in self.calls))
        self.assertEqual(self.index().read_bytes(), before_index)
        self.assertFalse(combined['rankingProven']); self.assertFalse(combined['nativeExpectationProven'])

    def test_new_controller_propagates_only_its_missing_terms_but_preserves_full_jobs(self):
        self.assertTrue(self.query()['complete'])
        expanded = self.jobs + [job('different-controller', 9)]
        pipeline.write(self.spec, {**self.original, 'jobs': expanded})
        self.calls.clear(); result = self.query()
        self.assertTrue(result['complete'], result)
        self.assertEqual(result['stats']['reusedPrograms'], 2); self.assertEqual(result['stats']['generatedPrograms'], 2)
        self.assertEqual(result['stats']['uniqueBasisPrograms'], 4)
        selected = next(call for call in self.calls if call[0] == 'basisPrograms')
        self.assertEqual(selected[1], [job['name'] for job in expanded]); self.assertEqual(len(selected[2]), 2)

    def test_corrupt_term_repairs_one_program_and_preserves_other_blob(self):
        self.assertTrue(self.query()['complete'])
        rows = pipeline.read(self.index())['programs']; directory = self.index().parent
        intact = directory / rows[1]['archive']['path']; before = intact.read_bytes()
        (directory / rows[0]['archive']['path']).write_bytes(b'corrupt')
        self.calls.clear(); result = self.query()
        self.assertTrue(result['complete'], result); self.assertEqual(result['stats']['generatedPrograms'], 1)
        self.assertEqual(result['stats']['propagationCalls'], 1)
        selected = next(call for call in self.calls if call[0] == 'basisPrograms')
        self.assertEqual(selected[2], [rows[0]['fingerprint']]); self.assertEqual(intact.read_bytes(), before)

    def test_no_generate_and_valid_seed(self):
        missing = self.query(generate=False)
        self.assertFalse(missing['complete']); self.assertEqual(missing['status'], 'missing')
        self.assertEqual(len(missing['missingPrograms']), 2)
        self.assertEqual([call[0] for call in self.calls], ['basisIdentify'])
        self.assertTrue(self.query()['complete']); seed = self.index()
        self.store = self.root / 'seeded'; self.calls.clear()
        result = self.query(generate=False, seed_indexes=[seed])
        self.assertTrue(result['complete']); self.assertEqual(result['stats']['reusedPrograms'], 2)
        self.assertFalse(any(call[0] == 'basisPrograms' for call in self.calls))

    def test_invalid_identification_weights_components_or_coverage_never_propagate(self):
        mutations = [lambda r: r.update(identificationComplete=False), lambda r: r.update(operatorContract='unknown'),
            lambda r: r['jobs'].pop(), lambda r: r['jobs'][0].update(status='unsupported'),
            lambda r: r['jobs'][0]['components'][0].update(programFingerprint='f' * 64),
            lambda r: r['jobs'][0]['components'][0].update(programIndex=True),
            lambda r: r['jobs'][0]['components'][0].update(weight=[-0.1, 0.75]),
            lambda r: r['jobs'][0]['components'][0].update(weight=[0.9, 0.9]),
            lambda r: r['jobs'][0]['components'][0].update(weight=[True, 1]),
            lambda r: r['jobs'][0]['components'][1].update(choices=[0]),
            lambda r: r['jobs'][0]['components'][1].update(choices=[255]),
            lambda r: r['programs'][0].update(representativeTermIndex=True),
            lambda r: r['programs'].append(copy.deepcopy(r['programs'][0])),
            lambda r: r['provenance'].update(datasetSha256='f' * 64),
            lambda r: r['stats'].update(propagationCalls=1)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.calls.clear(); self.mutate_identify = mutation
                result = self.query()
                self.assertFalse(result['complete']); self.assertEqual(result['status'], 'error')
                self.assertEqual([call[0] for call in self.calls], ['basisIdentify'])
                self.assertFalse(self.index().exists())

    def test_propagation_must_preserve_exact_component_weights_choices_and_selected_coverage(self):
        def change_weights(report):
            for component in report['jobs'][0]['components']:
                component['weight'] = [0.5, 0.5]
        mutations = [change_weights, lambda r: r['jobs'][0]['components'][1].update(choices=[1]),
            lambda r: r['jobs'][0]['components'].pop(), lambda r: r['jobs'].pop(),
            lambda r: r['programs'][0].update(status='identified', response=None),
            lambda r: r['programs'][0].update(sourceVersion='2' * 64),
            lambda r: r['programs'][0].update(response={'status': 'unsupported'}),
            lambda r: r.update(probabilityComplete=False), lambda r: r['context'].update(fingerprint='f' * 64),
            lambda r: r['stats'].update(propagationCalls=1), lambda r: r['stats'].update(verificationCalls=1),
            lambda r: r['provenance']['inputs'].update(specSha256='f' * 64)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.calls.clear(); self.mutate_generated = mutation; self.store = self.root / ('invalid-' + str(self.ordinal))
                result = self.query()
                self.assertFalse(result['complete']); self.assertEqual(result['status'], 'error')
                self.assertFalse(any(call[0] in ('program-pack', 'basis-materialize') for call in self.calls))

    def test_pack_coverage_hashes_and_mode_are_checked_before_import(self):
        mutations = [lambda i: i.update(complete=False), lambda i: i.update(inputReportSha256='f' * 64),
            lambda i: i.update(requestedJobs=1), lambda i: i.update(mode='u16'), lambda i: i['programs'].pop(),
            lambda i: i['programs'][0].update(fingerprint='f' * 64),
            lambda i: i['programs'][0]['archive'].update(sha256='f' * 64),
            lambda i: i['programs'][0]['archive'].update(path='../escape'),
            lambda i: i['programs'][0]['archive'].update(bytes=True)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.calls.clear(); self.mutate_pack = mutation; self.store = self.root / ('bad-pack-' + str(self.ordinal))
                result = self.query()
                self.assertFalse(result['complete']); self.assertEqual(result['status'], 'error')
                self.assertFalse(any(call[0] == 'basis-materialize' for call in self.calls))

    def test_materialize_failure_keeps_completed_terms_for_a_retry(self):
        self.fail_materialize = True; failed = self.query()
        self.assertFalse(failed['complete']); self.assertEqual(failed['status'], 'error')
        self.assertTrue(self.index().exists())
        self.fail_materialize = False; self.calls.clear(); result = self.query()
        self.assertTrue(result['complete']); self.assertEqual(result['stats']['generatedPrograms'], 0)
        self.assertFalse(any(call[0] == 'basisPrograms' for call in self.calls))

    def test_optional_native_prediction_and_shared_dictionary_writer_lock(self):
        decks = self.root / 'decks.json'; pipeline.write(decks, [{'name': 'declared actual decks'}])
        result = self.query(decks=decks)
        self.assertTrue(result['complete']); self.assertEqual(result['prediction']['ordersScored'], 120)
        self.assertTrue(result['inputsUnchanged'])
        with storage.writer_lock(self.index().parent):
            self.calls.clear(); rejected = self.query()
        self.assertFalse(rejected['complete']); self.assertIn('another query', rejected['error'])
        self.assertEqual([call[0] for call in self.calls], ['basisIdentify'])


if __name__ == '__main__':
    unittest.main()
