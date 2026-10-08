"""Stream/resume contracts with tiny descriptors; no native or network jobs."""
import copy
import importlib.util
from pathlib import Path
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
        # One of the four real chart schemas has no played range; preserve it explicitly.
        self.data['charts'][0]['fevers'] = {'startMs': [], 'endMs': []}
        pipeline.write(self.root / 'data.json', self.data)
        self.dataset = pipeline.sha((self.root / 'data.json').read_bytes())
        pipeline.write(self.root / 'anchor.json', {'format': 'ournotes.owned-snapshot/1', 'datasetId': self.dataset,
                                                 'eligible': {'members': [{'id': 1}], 'snaps': []}})
        runner.catalogue.build_manifest(self.root / 'data.json', self.root / 'anchor.json', self.root / 'inputs', 'jp')
        self.generator = self.root / 'generator'; self.generator.write_bytes(b'native binary')
        self.output = self.root / 'output'; self.calls = []
        self.source = patch.object(pipeline, 'source_identity', return_value={'digest': 'code', 'simSourceSha256': 'native'})
        self.source.start()

    def tearDown(self):
        self.source.stop(); self.temp.cleanup()

    def fake(self, generator, inputs, jobs, keys, source, directory, timeout):
        self.calls.append(inputs['request'])
        fp = pipeline.sha(Path(inputs['request']).read_bytes())
        report = {'format': 'ournotes-deck.luck-response-programs/1', 'complete': True,
                  'sourceVersion': source, 'capabilities': {'chain': keys},
                  'jobs': [{**job, 'status': 'success', 'programIndex': 0} for job in jobs],
                  'programs': [{'fingerprint': fp, 'sourceVersion': source,
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
                                    self.output, call_programs=kwargs.get('call', self.fake))

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
