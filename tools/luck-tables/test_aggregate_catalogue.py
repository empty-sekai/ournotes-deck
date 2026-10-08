"""Exact shard union and artifact validation; fixtures never invoke native work."""
import importlib.util
from pathlib import Path
import shutil
import unittest
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    value = importlib.util.module_from_spec(spec); spec.loader.exec_module(value)
    return value


fixture = load('test_stream_runner')
aggregate = load('aggregate_catalogue')
runner, pipeline = fixture.runner, fixture.pipeline


class AggregateTests(fixture.StreamingTests):
    def setUp(self):
        super().setUp()
        self.catalogues = self.root / 'catalogues'
        (self.catalogues / 'jp').mkdir(parents=True)
        shutil.copytree(self.root / 'inputs', self.catalogues / 'jp/inputs')
        self.benchmark = self.catalogues / 'jp/inputs/benchmark.json'
        self.results = self.root / 'shards'
        self.bundle = self.root / 'bundle.json'
        self.aggregate_source = patch.object(aggregate.pipeline, 'source_identity',
                                             return_value={'digest': 'code', 'simSourceSha256': 'native'})
        self.aggregate_source.start()
        aggregate.make_bundle(self.catalogues, self.generator, self.root, self.bundle)

    def tearDown(self):
        self.aggregate_source.stop(); super().tearDown()

    def shards(self):
        for index in (0, 1):
            result = runner.run_catalogue(self.benchmark, self.generator, self.root,
                                          self.results / f'jp-{index}', call_programs=self.fake,
                                          call_plan=self.fake_plan,
                                          shard_index=index, shard_count=2)
            self.assertTrue(result['complete'])
            self.assertFalse(result['wholeCatalogueComplete'])
            self.assertEqual(len(result['charts']), 2)

    def audit(self):
        return aggregate.aggregate(self.catalogues, self.results, self.bundle, self.root,
                                   self.root / 'audit', shard_count=2, regions=('jp',))

    def test_full_union_and_shared_bundle_validation(self):
        self.shards(); result = self.audit()
        self.assertTrue(result['complete'], result['errors'])
        self.assertEqual(result['summary']['receivedCharts'], 4)
        self.assertEqual(result['summary']['success'], 3)
        self.assertEqual(result['summary']['notApplicable'], 1)
        self.generator.write_bytes(b'changed binary')
        with self.assertRaisesRegex(ValueError, 'generator differs'):
            aggregate.verify_bundle(self.bundle, self.catalogues, self.root, self.generator)

    def repin_shards(self):
        self.shards()
        self.data['unusedPublisherAnnotation'] = {'newBuild': True}
        self.repin(self.catalogues / 'jp/inputs')
        aggregate.make_bundle(self.catalogues, self.generator, self.root, self.bundle)
        self.calls.clear(); self.shards()
        self.assertEqual(self.calls, []); self.assertEqual(len(self.plan_calls), 3)

    def test_fresh_plan_audits_current_repin_without_relabelling_original_generation(self):
        self.repin_shards()
        result = self.audit()
        self.assertTrue(result['complete'], result['errors'])
        self.assertEqual(result['work']['reusedCharts'], 3)
        self.assertEqual(result['work']['nativePlanCalls'], 3)
        self.assertEqual(result['work']['generatedPrograms'], 0)
        for row in result['charts']:
            if row['status'] == 'success':
                self.assertTrue(row['reusedThroughPlan'])

    def test_semantic_resume_requires_current_provenance_full_plan_and_matching_context(self):
        self.repin_shards()
        manifest_path = self.results / 'jp-0/manifest.json'
        manifest_raw = manifest_path.read_bytes()
        manifest = pipeline.read(manifest_path)
        row = next(row for row in manifest['charts'] if row['status'] == 'success')
        directory = manifest_path.parent / row['directory']
        plan_path = directory / row['resumePlan']['report']['path']
        plan_raw = plan_path.read_bytes()
        mutations = {
            'stale provenance': lambda plan: plan['provenance'].__setitem__('datasetSha256', '0' * 64),
            'changed context': lambda plan: plan['context'].__setitem__('fingerprint', '0' * 64),
            'missing job': lambda plan: plan['jobs'].pop(),
            'unknown source': lambda plan: plan['dependencyDescriptor']['algorithm'].__setitem__('simSourceSha256', 'other'),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name):
                plan_path.write_bytes(plan_raw); manifest_path.write_bytes(manifest_raw)
                plan = pipeline.read(plan_path); mutate(plan); pipeline.write(plan_path, plan)
                current = pipeline.read(manifest_path)
                current_row = next(row for row in current['charts'] if row['status'] == 'success')
                # An internally updated receipt still cannot turn a stale plan
                # into valid evidence for a newly pinned native input.
                current_row['resumePlan']['report'] = runner.file_receipt(plan_path, directory)
                pipeline.write(manifest_path, current)
                result = self.audit()
                self.assertFalse(result['complete']); self.assertGreater(result['summary']['errors'], 0)
        plan_path.write_bytes(plan_raw); manifest_path.write_bytes(manifest_raw)
        current = pipeline.read(manifest_path)
        next(row for row in current['charts'] if row['status'] == 'success')['reusedThroughPlan'] = False
        pipeline.write(manifest_path, current)
        result = self.audit()
        self.assertFalse(result['complete'])
        self.assertTrue(any('provenance' in error for error in result['errors']))

    def test_historical_ordinal_partition_cannot_be_mislabelled_as_current_complete_catalogue(self):
        self.shards()
        path = self.results / 'jp-0/manifest.json'; value = pipeline.read(path)
        value['shard']['ordering'] = 'ascending-score-id-modulo'
        pipeline.write(path, value)
        self.assertFalse(self.audit()['complete'])

    def test_missing_and_duplicate_shards_never_complete(self):
        self.shards()
        path = self.results / 'jp-1/manifest.json'; raw = path.read_bytes(); path.unlink()
        missing = self.audit(); self.assertFalse(missing['complete'])
        self.assertTrue(any('missing shard' in error for error in missing['errors']))
        path.write_bytes(raw)
        shutil.copytree(self.results / 'jp-1', self.results / 'duplicate')
        duplicate = self.audit(); self.assertFalse(duplicate['complete'])
        self.assertTrue(any('duplicate' in error for error in duplicate['errors']))

    def test_changed_input_receipt_or_blob_never_complete(self):
        self.shards()
        path = self.results / 'jp-0/manifest.json'; value = pipeline.read(path)
        value['charts'][0]['inputSha256']['request'] = '0'*64
        pipeline.write(path, value)
        self.assertFalse(self.audit()['complete'])
        # Regeneration corrects a corrupted shard receipt before the second challenge.
        self.shards()
        blob = next((self.results / 'jp-1').rglob('*.onlrsp'))
        blob.write_bytes(b'corrupt')
        result = self.audit(); self.assertFalse(result['complete'])
        self.assertTrue(any('corrupt' in error for error in result['errors']))

    def test_unknown_source_and_out_of_partition_rows_fail(self):
        self.shards()
        path = self.results / 'jp-0/manifest.json'; value = pipeline.read(path)
        value['algorithm']['digest'] = 'wrong'
        value['charts'].append(pipeline.read(self.results / 'jp-1/manifest.json')['charts'][0])
        pipeline.write(path, value)
        result = self.audit(); self.assertFalse(result['complete'])
        self.assertTrue(any('partition' in error for error in result['errors']))
        self.assertTrue(any('source' in error for error in result['errors']))


if __name__ == '__main__':
    unittest.main()
