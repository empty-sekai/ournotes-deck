"""Stream/resume contracts with tiny descriptors; no native or network jobs."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    value = importlib.util.module_from_spec(spec); spec.loader.exec_module(value)
    return value


runner, fixtures = load('stream_runner'), load('test_catalogue')
pipeline = runner.pipeline


class StreamingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.data = fixtures.fixture()
        effects = self.data['master']['MasterGekisouSkillEffect']
        effects['columns'].append('_effectValue')
        for row in effects['rows']:
            row.append(10)
        # One of the four real chart schemas has no played range; preserve it explicitly.
        self.data['charts'][0]['fevers'] = {'startMs': [], 'endMs': []}
        pipeline.write(self.root / 'data.json', self.data)
        self.dataset = pipeline.sha((self.root / 'data.json').read_bytes())
        pipeline.write(self.root / 'anchor.json', {'format': 'ournotes.owned-snapshot/1', 'datasetId': self.dataset,
                                                 'eligible': {'members': [{'id': 1}], 'snaps': []}})
        runner.catalogue.build_manifest(self.root / 'data.json', self.root / 'anchor.json', self.root / 'inputs', 'jp')
        self.generator = self.root / 'generator'; self.generator.write_bytes(b'native binary')
        self.output = self.root / 'output'; self.calls = []; self.plan_calls = []
        self.source = patch.object(pipeline, 'source_identity', return_value={'digest': 'code', 'simSourceSha256': 'native'})
        self.source.start()

    def tearDown(self):
        self.source.stop(); self.temp.cleanup()

    def native_evidence(self, inputs, jobs, keys, source):
        """Contract fixture: selected chart/effect dependencies, not a simulator."""
        data, request = pipeline.read(inputs['data']), pipeline.read(inputs['request'])
        chart = next(chart for chart in data['charts'] if chart['scoreId'] == request['execution']['scoreId'])
        shared = {'algorithm': {'name': 'ournotes-luck-response/1', 'simSourceSha256': source},
                  'resolved': {'chart': chart, 'request': request}}
        rows = []
        for job in jobs:
            effects = []
            for key, position in job['entries']:
                table = 'MasterGekisouSkillEffect' if key['source'] == 'gekisou' else 'MasterGekisouSupportSkillEffect'
                effects.append({'position': position, 'key': key, 'effects': [row for row in runner.catalogue.rows(data, table)
                    if row['_gekisouSkillID'] == key['id'] and row['_level'] == key['level']]})
            deps = {'sources': effects}
            rows.append({**job, 'status': 'planned', 'dependencyDescriptor': deps,
                         'dependencyFingerprint': pipeline.sha(pipeline.canonical(deps))})
        shared_fp = pipeline.sha(pipeline.canonical(shared))
        context = {'algorithmVersion': 'ournotes-luck-response/1/' + source,
                   'fingerprint': pipeline.sha(pipeline.canonical({'shared': shared_fp, 'entries': {
                       json.dumps(row['entries'], separators=(',', ':')): row['dependencyFingerprint'] for row in rows}}))}
        hashes = pipeline.provenance(inputs)
        provenance = {'datasetSha256': hashes['data'], 'inputs': {
            'rosterSha256': hashes['snapshot'], 'requestSha256': hashes['request']}}
        return {'context': context, 'sharedFingerprint': shared_fp, 'dependencyDescriptor': shared,
                'capabilities': {'chain': keys}, 'provenance': provenance, 'jobs': rows}

    def fake_plan(self, generator, inputs, jobs, directory, timeout):
        self.plan_calls.append(inputs['request'])
        keys, unsupported = runner.catalogue.all_chain_keys(pipeline.read(inputs['data']))
        self.assertFalse(unsupported)
        report = self.native_evidence(inputs, jobs, keys, 'native')
        spec_raw = pipeline.canonical(runner.plan_spec(jobs)) + b'\n'
        report.update(format='ournotes-deck.luck-response-generation/1', mode='plan',
                      table={'context': report['context'], 'entries': []}, validationDecks=[], archives=[])
        report['provenance']['inputs']['specSha256'] = pipeline.sha(spec_raw)
        return runner.preserve_plan(directory, spec_raw, pipeline.canonical(report) + b'\n')

    def fake(self, generator, inputs, jobs, keys, source, directory, timeout):
        self.calls.append(inputs['request'])
        evidence = self.native_evidence(inputs, jobs, keys, source)
        fp = evidence['context']['fingerprint']
        report = {**evidence, 'format': 'ournotes-deck.luck-response-programs/1', 'mode': 'programs',
                  'complete': True, 'probabilityComplete': True,
                  'sourceVersion': source, 'capabilities': {'chain': keys},
                  'jobs': [{**job, 'status': 'success', 'programIndex': 0, 'programFingerprint': fp} for job in jobs],
                  'programs': [{'fingerprint': fp, 'sourceVersion': source, 'status': 'success',
                                'response': {'status': 'success', 'curve': {}}}]}
        runner.validate_program_report(report, jobs, keys, source)
        pipeline.write(directory / 'programs.json', report)
        report_sha = pipeline.sha((directory / 'programs.json').read_bytes())
        receipts = []
        for mode in runner.MODES:
            packed = directory / 'packed' / mode
            raw = mode.encode(); digest = pipeline.sha(raw)
            path = packed / 'blobs' / (digest + '.onlrsp')
            pipeline.atomic_bytes(path, raw)
            index = {'format': 'ournotes-deck.luck-program-index/1', 'mode': mode, 'complete': True,
                     'inputReportSha256': report_sha, 'sourceVersion': source,
                     'programs': [{'fingerprint': fp, 'sourceVersion': source, 'status': 'success',
                                   'archive': {**runner.file_receipt(path, packed), 'mode': mode, 'verifiedEntries': 1}}]}
            pipeline.write(packed / 'program-index.json', index)
            receipts.append({'mode': mode, 'index': runner.file_receipt(packed / 'program-index.json', directory)})
        return {'jobs': len(jobs), 'uniquePrograms': 1, 'sourceVersion': source, 'programFingerprints': [fp],
                'report': runner.file_receipt(directory / 'programs.json', directory), 'archives': receipts,
                'nativeCatalogueValidated': True}

    def run_catalogue(self, **kwargs):
        return runner.run_catalogue(self.root / 'inputs/benchmark.json', self.generator, self.root,
                                    self.output, call_programs=kwargs.get('call', self.fake),
                                    call_plan=kwargs.get('plan', self.fake_plan))

    def repin(self, target=None):
        pipeline.write(self.root / 'data.json', self.data)
        anchor = pipeline.read(self.root / 'anchor.json')
        anchor['datasetId'] = pipeline.sha((self.root / 'data.json').read_bytes())
        pipeline.write(self.root / 'anchor.json', anchor)
        target = target or self.root / 'inputs'
        shutil.rmtree(target)
        runner.catalogue.build_manifest(self.root / 'data.json', self.root / 'anchor.json', target, 'jp')

    def add_unrelated_song(self):
        table = self.data['master']['MasterLiveMusic']
        row = list(table['rows'][0]); row[table['columns'].index('_id')] = 50
        row[table['columns'].index('_gekisouMission1')] = 1
        for key, score in zip(('_easyID', '_normalID', '_hardID', '_expertID'), range(1, 5)):
            row[table['columns'].index(key)] = score
        table['rows'].append(row)
        for score in range(1, 5):
            self.data['master']['MasterLiveMusicScore']['rows'].append([score, score])
            self.data['charts'].append({'scoreId': score, 'notes': {'id': [1], 'timeMs': [500]},
                                        'fevers': {'startMs': [0], 'endMs': [600]}})

    def test_unused_repin_reuses_native_responses_with_current_plan_and_original_provenance(self):
        before = self.run_catalogue()
        old_reports = {row['name']: row['report'] for row in before['charts'] if row['status'] == 'success'}
        self.data['unusedPublisherAnnotation'] = {'revision': 'new publication'}
        self.repin(); self.calls.clear()
        result = self.run_catalogue()
        self.assertTrue(result['complete']); self.assertEqual(self.calls, [])
        self.assertEqual(len(self.plan_calls), 3)
        self.assertEqual(result['work']['generatedPrograms'], 0)
        keys = runner.catalogue.all_chain_keys(self.data)[0]
        for row in result['charts']:
            if row['status'] != 'success':
                continue
            self.assertTrue(row['reusedThroughPlan'])
            self.assertEqual(row['report'], old_reports[row['name']])
            report = pipeline.read(self.output / row['directory'] / row['report']['path'])
            self.assertNotEqual(report['provenance']['datasetSha256'], row['inputSha256']['data'])
            plan = runner.validate_semantic_resume(self.output / row['directory'], row, report,
                pipeline.base_jobs(keys), keys, 'native', row['inputSha256'])
            self.assertEqual(plan['provenance']['datasetSha256'], row['inputSha256']['data'])
        self.plan_calls.clear(); warm = self.run_catalogue()
        self.assertTrue(warm['complete']); self.assertEqual(self.calls, []); self.assertEqual(self.plan_calls, [])

    def test_selected_skill_change_invalidates_full_context_even_when_shared_descriptor_matches(self):
        before = self.run_catalogue()
        self.data['master']['MasterGekisouSkillEffect']['rows'][0][-1] += 1
        self.repin(); self.calls.clear()
        result = self.run_catalogue()
        self.assertTrue(result['complete']); self.assertEqual(len(self.calls), 3)
        self.assertEqual(len(self.plan_calls), 3)
        for old, current in zip(before['charts'], result['charts']):
            if old['status'] == 'success':
                self.assertIn('context or dependencies differ', current['semanticResumeMiss'])
                self.assertFalse(current['reusedChart']); self.assertFalse(current['reusedThroughPlan'])
                self.assertNotEqual(old['programFingerprints'], current['programFingerprints'])

    def test_new_catalogue_key_regenerates_instead_of_omitting_jobs(self):
        self.run_catalogue()
        table = self.data['master']['MasterGekisouSkillEffect']
        row = list(table['rows'][0]); row[table['columns'].index('_id')] = 3
        row[table['columns'].index('_level')] = 3; table['rows'].append(row)
        self.repin(); self.calls.clear()
        result = self.run_catalogue()
        self.assertTrue(result['complete']); self.assertEqual(len(self.calls), 3)
        self.assertEqual(self.plan_calls, [])
        self.assertTrue(all(row['jobs'] == 16 for row in result['charts'] if row['status'] == 'success'))

    def test_score_id_partition_keeps_existing_charts_on_their_shards_when_song_is_inserted(self):
        def shards():
            return [runner.run_catalogue(self.root / 'inputs/benchmark.json', self.generator, self.root,
                self.output / str(index), call_programs=self.fake, call_plan=self.fake_plan,
                shard_index=index, shard_count=3) for index in range(3)]
        before = shards(); self.add_unrelated_song(); self.repin(); self.calls.clear()
        after = shards()
        self.assertEqual(self.calls, []); self.assertEqual(len(self.plan_calls), 3)
        for old, new in zip(before, after):
            self.assertTrue(new['complete']); self.assertEqual(new['shard']['ordering'], runner.SHARD_ORDERING)
            self.assertLessEqual({row['scoreId'] for row in old['charts']}, {row['scoreId'] for row in new['charts']})
            self.assertTrue(all(row.get('reusedThroughPlan') for row in new['charts'] if row['status'] == 'success'))

    def test_stream_all_charts_resume_zero_calls_and_rebuild_corruption(self):
        result = self.run_catalogue()
        self.assertTrue(result['complete']); self.assertEqual(result['summary']['success'], 3)
        self.assertEqual(result['summary']['notApplicable'], 1); self.assertEqual(len(self.calls), 3)
        self.assertTrue(all(row.get('jobs') == 11 for row in result['charts'] if row['status'] == 'success'))
        self.calls.clear(); again = self.run_catalogue()
        self.assertTrue(again['complete']); self.assertEqual(self.calls, [])
        row = next(r for r in result['charts'] if r['status'] == 'success')
        index_path = self.output / row['directory'] / row['archives'][0]['index']['path']
        index = pipeline.read(index_path)
        blob = index_path.parent / index['programs'][0]['archive']['path']
        blob.write_bytes(b'broken')
        repaired = self.run_catalogue()
        self.assertTrue(repaired['complete']); self.assertEqual(len(self.calls), 1)

    def test_failures_remain_visible_and_other_charts_continue(self):
        def failing(*args):
            if not self.calls:
                self.calls.append('failure')
                raise ValueError('unsupported real input')
            return self.fake(*args)
        result = self.run_catalogue(call=failing)
        self.assertFalse(result['complete']); self.assertEqual(result['summary']['error'], 1)
        self.assertEqual(result['summary']['success'], 2); self.assertEqual(len(result['charts']), 4)

    def test_omitted_chart_and_incomplete_native_labels_fail(self):
        path = self.root / 'inputs/benchmark.json'; manifest = pipeline.read(path)
        manifest['cases'].pop(); pipeline.write(path, manifest)
        with self.assertRaisesRegex(ValueError, 'omits master charts'):
            self.run_catalogue()
        report = {'format': 'ournotes-deck.luck-response-programs/1', 'complete': True,
                  'sourceVersion': 'native', 'capabilities': {'chain': []}, 'jobs': [], 'programs': []}
        with self.assertRaises(ValueError):
            runner.validate_program_report(report, [{'name': 'base', 'entries': []}], [], 'native')

    def test_pack_path_traversal_and_unknown_source_are_rejected(self):
        with self.assertRaises(ValueError):
            runner.verified_file(self.output, {'path': '../../outside', 'sha256': '0'*64, 'bytes': 1})
        report = {'format': 'ournotes-deck.luck-response-programs/1', 'complete': True,
                  'sourceVersion': 'different'}
        with self.assertRaises(ValueError):
            runner.validate_program_report(report, [], [], 'native')


if __name__ == '__main__':
    unittest.main()
