"""Persistent program-dictionary contracts with a fake native transport only.

These tests make no simulation, probability, native-codec or performance claim.
"""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('luck_programs_tests_subject', Path(__file__).with_name('programs.py'))
programs = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(programs)
pipeline = programs.pipeline
SOURCE = '1' * 64


def job(name, skill, position=0):
    return {'name': name, 'entries': [[{'source': 'gekisou', 'id': skill, 'level': 1, 'matched': None}, position]]}


def fingerprint(value):
    # Fake transport deliberately maps position aliases to one program identity.
    return pipeline.sha(str(value['entries'][0][0]['id']).encode())


class ProgramTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.inputs = {}
        for name in ('data', 'snapshot', 'request'):
            path = self.root / (name + '.json'); pipeline.write(path, {'kind': name})
            self.inputs[name] = path
        self.spec = self.root / 'spec.json'
        self.jobs = [job('a', 7), job('alias-a', 7, 4), job('b', 8)]
        pipeline.write(self.spec, {'jobs': self.jobs, 'mcRuns': 100, 'scoreSamples': 2})
        self.generator = self.root / 'generator'; self.generator.write_bytes(b'fake native transport')
        self.store = self.root / 'store'; self.calls = []; self.ordinal = 0
        self.mutate_identify = self.mutate_generated = self.mutate_pack = None

    def tearDown(self):
        self.temp.cleanup()

    def report(self, jobs, identify):
        unique, rows, bodies = {}, [], []
        for ordinal, value in enumerate(jobs):
            fp = fingerprint(value)
            if fp not in unique:
                unique[fp] = len(bodies)
                body = {'fingerprint': fp, 'sourceVersion': SOURCE, 'representativeJobIndex': ordinal}
                if not identify:
                    body['status'] = 'success'
                    body['response'] = {'status': 'success', 'curve': {'steps': []}}
                bodies.append(body)
            rows.append({**value, 'status': 'identified' if identify else 'success',
                         'programIndex': unique[fp], 'programFingerprint': fp})
        return {'format': programs.REPORT, 'mode': 'identify' if identify else 'programs',
                'sourceVersion': SOURCE, 'identificationComplete': True, 'complete': not identify,
                'probabilityComplete': not identify,
                'jobs': rows, 'programs': bodies, 'stats': {'propagationCalls': 0 if identify else len(bodies)}}

    def native(self, command, log, timeout):
        command = [str(value) for value in command]
        action = command[1]
        if action not in ('program-pack', 'program-materialize', 'pack', 'predict'):
            spec = pipeline.read(command[-2]); identify = spec['mode'] == 'identify'
            self.calls.append((spec['mode'], [value['name'] for value in spec['jobs']]))
            self.assertEqual(spec['mcRuns'], 0)
            self.assertEqual(spec['scoreSamples'], 0)
            self.assertEqual(spec['validationDecks'], [])
            report = self.report(spec['jobs'], identify)
            mutate = self.mutate_identify if identify else self.mutate_generated
            if mutate:
                mutate(report)
            pipeline.write(command[-1], report)
        elif action == 'program-pack':
            self.calls.append((action, []))
            report = pipeline.read(command[2]); mode = command[3]; directory = Path(command[4])
            rows = []
            for body in report['programs']:
                raw = pipeline.canonical({'fake-response': body['fingerprint'], 'source': SOURCE, 'mode': mode})
                digest = pipeline.sha(raw); relative = 'blobs/' + digest + '.onlrsp'
                pipeline.atomic_bytes(directory / relative, raw)
                rows.append({'fingerprint': body['fingerprint'], 'sourceVersion': SOURCE, 'status': 'success',
                             'archive': {'path': relative, 'sha256': digest, 'bytes': len(raw),
                                         'mode': mode, 'verifiedEntries': 1}})
            index = {'format': programs.INDEX, 'sourceVersion': SOURCE, 'mode': mode,
                     'complete': True, 'programs': rows, 'inputReportSha256': pipeline.sha(Path(command[2]).read_bytes())}
            if self.mutate_pack:
                self.mutate_pack(index)
            pipeline.write(directory / 'program-index.json', index)
        elif action == 'program-materialize':
            self.calls.append((action, []))
            report = pipeline.read(command[2])
            pipeline.write(command[-1], {'context': {}, 'entries': [
                {'key': row['entries'], 'response': {'status': 'success', 'curve': {'steps': []}}}
                for row in report['jobs']]})
        elif action == 'pack':
            self.calls.append((action, [])); pipeline.atomic_bytes(Path(command[-1]), b'fake final archive')
        else:
            raise AssertionError('unexpected prediction work in dictionary contract tests')

    def query(self, **kwargs):
        self.ordinal += 1
        return programs.query(self.inputs['data'], self.inputs['snapshot'], self.inputs['request'], self.spec,
                              self.generator, self.store, self.root / ('query-' + str(self.ordinal)),
                              run_command=self.native, **kwargs)

    def index(self, mode='u24'):
        return self.store / SOURCE / mode / 'program-index.json'

    def test_cold_aliases_warm_and_new_program_only(self):
        cold = self.query()
        self.assertTrue(cold['complete'], cold)
        self.assertEqual(cold['stats']['requestedJobs'], 3)
        self.assertEqual(cold['stats']['uniquePrograms'], 2)
        self.assertEqual(cold['stats']['generatedPrograms'], 2)
        self.assertEqual([value for value in self.calls if value[0] == 'programs'], [('programs', ['a', 'b'])])
        original = self.spec.read_bytes()
        self.calls.clear(); warm = self.query()
        self.assertTrue(warm['complete']); self.assertEqual(warm['stats']['reusedPrograms'], 2)
        self.assertEqual(warm['stats']['propagationCalls'], 0)
        self.assertFalse(any(kind == 'programs' for kind, _ in self.calls))
        self.assertEqual(self.spec.read_bytes(), original)
        pipeline.write(self.spec, {'jobs': self.jobs + [job('c', 9), job('alias-c', 9, 3)]})
        self.calls.clear(); added = self.query()
        self.assertTrue(added['complete']); self.assertEqual(added['stats']['generatedPrograms'], 1)
        self.assertEqual([value for value in self.calls if value[0] == 'programs'], [('programs', ['c'])])
        self.assertFalse(added['nativeExpectationProven']); self.assertFalse(added['rankingProven'])
        self.assertFalse(pipeline.read(self.index())['complete'])

    def test_corrupt_blob_repairs_only_touched_program(self):
        self.assertTrue(self.query()['complete'])
        rows = pipeline.read(self.index())['programs']; broken = next(row for row in rows if row['fingerprint'] == fingerprint(self.jobs[0]))
        untouched = next(row for row in rows if row is not broken)
        directory = self.index().parent; untouched_before = (directory / untouched['archive']['path']).read_bytes()
        (directory / broken['archive']['path']).write_bytes(b'bad')
        self.calls.clear(); result = self.query()
        self.assertTrue(result['complete']); self.assertEqual(result['stats']['generatedPrograms'], 1)
        self.assertEqual([value for value in self.calls if value[0] == 'programs'], [('programs', ['a'])])
        self.assertEqual((directory / untouched['archive']['path']).read_bytes(), untouched_before)

    def test_no_generate_reports_missing_without_propagation_or_materialization(self):
        result = self.query(generate=False)
        self.assertFalse(result['complete']); self.assertEqual(result['status'], 'missing')
        self.assertEqual(set(result['missingPrograms']), {fingerprint(j) for j in self.jobs})
        self.assertEqual(self.calls, [('identify', ['a', 'alias-a', 'b'])])
        self.assertEqual(result['stats']['generatedPrograms'], 0)
        self.assertTrue(result['inputsUnchanged'])

    def test_refused_or_malformed_identification_never_materializes(self):
        mutations = [lambda r: r.update(identificationComplete=False),
                     lambda r: r['jobs'][0].update(status='unsupported'),
                     lambda r: r['jobs'][0].update(programFingerprint='f'*64),
                     lambda r: r['programs'][0].update(representativeJobIndex=True),
                     lambda r: r['programs'].append(copy.deepcopy(r['programs'][0])),
                     lambda r: r['jobs'].pop(), lambda r: r.update(sourceVersion='wrong')]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.calls.clear(); self.mutate_identify = mutation
                result = self.query()
                self.assertFalse(result['complete']); self.assertEqual(result['status'], 'error')
                self.assertEqual([kind for kind, _ in self.calls], ['identify'])
                self.assertFalse(self.index().exists())

    def test_malformed_propagation_cannot_pack_or_materialize(self):
        mutations = [lambda r: r['jobs'][0].update(status='unsupported'),
                     lambda r: r['jobs'][0].update(programIndex=1),
                     lambda r: r['jobs'][0].update(programFingerprint='f'*64),
                     lambda r: r['programs'][0].update(sourceVersion='2'*64),
                     lambda r: r['programs'][0].update(response={'status': 'unsupported', 'reason': 'refused'}),
                     lambda r: r['programs'].append(copy.deepcopy(r['programs'][0])),
                     lambda r: r.update(probabilityComplete=False),
                     lambda r: r['stats'].update(propagationCalls=0)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.calls.clear(); self.mutate_generated = mutation
                # Isolate each malformed cold dictionary attempt.
                self.store = self.root / ('bad-store-' + str(self.ordinal))
                result = self.query()
                self.assertFalse(result['complete']); self.assertEqual(result['status'], 'error')
                self.assertFalse(any(kind in ('program-pack', 'program-materialize') for kind, _ in self.calls))

    def test_pack_source_mode_path_and_checksum_are_checked(self):
        mutations = [lambda i: i.update(sourceVersion='2'*64), lambda i: i.update(mode='u16'),
                     lambda i: i['programs'][0]['archive'].update(path='../outside'),
                     lambda i: i['programs'][0]['archive'].update(sha256='f'*64),
                     lambda i: i['programs'][0]['archive'].update(bytes=True),
                     lambda i: i['programs'][0]['archive'].update(verifiedEntries=0)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.calls.clear(); self.mutate_pack = mutation
                self.store = self.root / ('pack-store-' + str(self.ordinal))
                result = self.query()
                self.assertFalse(result['complete']); self.assertEqual(result['status'], 'error')
                self.assertFalse(any(kind == 'program-materialize' for kind, _ in self.calls))

    def test_conflicting_import_is_refused_and_original_bytes_survive(self):
        self.assertTrue(self.query()['complete'])
        before = self.index().read_bytes(); original = pipeline.read(self.index())
        incoming = copy.deepcopy(original); row = incoming['programs'][0]
        directory = self.root / 'seed'; raw = b'different response of same native identity'
        digest = pipeline.sha(raw); relative = 'blobs/' + digest + '.onlrsp'
        row['archive'].update(sha256=digest, path=relative, bytes=len(raw))
        pipeline.atomic_bytes(directory / relative, raw)
        # Keep the other incoming row valid too, with its original content.
        for other in incoming['programs'][1:]:
            pipeline.atomic_bytes(directory / other['archive']['path'], (self.index().parent / other['archive']['path']).read_bytes())
        pipeline.write(directory / 'program-index.json', incoming)
        self.calls.clear(); result = self.query(seed_indexes=[directory / 'program-index.json'])
        self.assertFalse(result['complete']); self.assertIn('conflicting', result['error'])
        self.assertEqual(self.index().read_bytes(), before)
        self.assertFalse(any(kind == 'program-materialize' for kind, _ in self.calls))

    def test_valid_seed_avoids_propagation_and_wrong_dictionary_source_refuses(self):
        self.assertTrue(self.query()['complete']); seed = self.index()
        self.store = self.root / 'seeded-store'; self.calls.clear()
        result = self.query(seed_indexes=[seed], generate=False)
        self.assertTrue(result['complete']); self.assertEqual(result['stats']['reusedPrograms'], 2)
        self.assertFalse(any(kind == 'programs' for kind, _ in self.calls))
        changed = pipeline.read(self.index()); changed['sourceVersion'] = '2'*64
        pipeline.write(self.index(), changed); self.calls.clear()
        result = self.query()
        self.assertFalse(result['complete']); self.assertEqual([kind for kind, _ in self.calls], ['identify'])

    def test_lock_and_input_overlap_refuse_without_native_propagation(self):
        directory = self.store / SOURCE / 'u24'
        with programs.writer_lock(directory):
            with self.assertRaisesRegex(ValueError, 'another query'):
                with programs.writer_lock(directory):
                    self.fail('concurrent writer should never enter')
        with self.assertRaisesRegex(ValueError, 'must not contain'):
            programs.query(self.inputs['data'], self.inputs['snapshot'], self.inputs['request'], self.spec,
                           self.generator, self.store, self.root, run_command=self.native)
        self.assertEqual(self.calls, [])


if __name__ == '__main__':
    unittest.main()
