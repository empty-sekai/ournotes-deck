"""Integrity and deterministic-container tests; fake bytes are not native codec tests."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('luck_analyze_programs_tests', Path(__file__).with_name('analyze_programs.py'))
analyze = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(analyze)
pipeline = analyze.pipeline
SOURCE, FP = '1'*64, '2'*64


class ProgramAnalysisTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.inputs = self.root / 'inputs'; self.results = self.root / 'results'

    def tearDown(self):
        self.temp.cleanup()

    def add_chart(self, region, score, mode='u24'):
        inp = self.inputs / region; result = self.results / region
        pipeline.write(inp / 'data.json', {'fake-region': region})
        dataset = analyze.file_hash(inp / 'data.json')
        request = {'execution': {'scoreId': score}, 'scenario': {'kind': 'mission', 'musicId': 100}}
        pipeline.write(inp / 'request.json', request)
        pipeline.write(inp / 'catalogue.json', {'format': 'ournotes-deck.luck-catalogue/1', 'datasetId': dataset, 'region': region})
        pipeline.write(inp / 'benchmark.json', {'format': 'ournotes-deck.search-benchmark/1', 'datasetId': dataset,
                                               'cases': [{'name': region, 'data': 'data.json', 'request': 'request.json'}]})
        report = {'format': analyze.store.REPORT, 'mode': 'programs', 'sourceVersion': SOURCE,
                  'complete': True, 'probabilityComplete': True, 'sharedFingerprint': region,
                  'provenance': {'datasetSha256': dataset, 'inputs': {'requestSha256': analyze.file_hash(inp / 'request.json')}},
                  'jobs': [{'name': name, 'entries': [], 'status': 'success', 'programIndex': 0,
                            'programFingerprint': FP} for name in ('one', 'alias')],
                  'programs': [{'fingerprint': FP, 'sourceVersion': SOURCE, 'status': 'success',
                                'response': {'status': 'success', 'curve': {'steps': []}}}]}
        pipeline.write(result / 'programs.json', report)
        raw = b'fake codec transport; same native program and quantization'
        digest = pipeline.sha(raw); relative = 'blobs/' + digest + '.onlrsp'
        pipeline.atomic_bytes(result / mode / relative, raw)
        index = {'format': analyze.store.INDEX, 'sourceVersion': SOURCE, 'mode': mode, 'complete': True,
                 'inputReportSha256': analyze.file_hash(result / 'programs.json'), 'requestedJobs': 2,
                 'programs': [{'fingerprint': FP, 'sourceVersion': SOURCE, 'status': 'success',
                               'archive': {'path': relative, 'sha256': digest, 'bytes': len(raw),
                                           'mode': mode, 'verifiedEntries': 1}}]}
        pipeline.write(result / mode / 'program-index.json', index)
        return result / mode / 'program-index.json'

    def semantic_chart(self):
        """A fake native transport receipt, using the stream runner's real validation schema."""
        self.add_chart('tw', 100003)
        inp, result = self.inputs / 'tw', self.results / 'tw'
        keys = [{'source': 'gekisou', 'id': 1, 'level': 1, 'matched': None}]
        jobs = pipeline.base_jobs(keys)
        report_path = result / 'programs.json'
        report = pipeline.read(report_path)
        report.update(capabilities={'chain': keys}, sharedFingerprint='3'*64,
                      dependencyDescriptor={'algorithm': {'simSourceSha256': SOURCE}, 'stable': True},
                      context={'fingerprint': '4'*64, 'algorithmVersion': 'ournotes-luck-response/1/' + SOURCE})
        report['jobs'] = [{**job, 'status': 'success', 'programIndex': 0, 'programFingerprint': FP} for job in jobs]
        pipeline.write(report_path, report)
        old_sha = analyze.file_hash(report_path)
        # Repin current input bytes while keeping the original native report immutable.
        data = pipeline.read(inp / 'data.json'); data['unused'] = 1
        pipeline.write(inp / 'data.json', data)
        dataset = analyze.file_hash(inp / 'data.json')
        pipeline.write(inp / 'snapshot.json', {'datasetId': dataset, 'fixture': True})
        inventory = pipeline.read(inp / 'catalogue.json'); inventory['datasetId'] = dataset
        pipeline.write(inp / 'catalogue.json', inventory)
        manifest = pipeline.read(inp / 'benchmark.json')
        manifest['datasetId'] = dataset
        manifest['cases'][0]['snapshot'] = 'snapshot.json'
        manifest['luckCatalogue'] = {'format': analyze.runner.catalogue.INVENTORY_FORMAT, 'keys': keys,
                                     'selection': 'all-master-source-levels', 'includeNonLuckCharts': True}
        pipeline.write(inp / 'benchmark.json', manifest)
        hashes = {key: analyze.file_hash(inp / (key + '.json')) for key in ('data', 'snapshot', 'request')}
        archives = []
        for mode in analyze.runner.MODES:
            directory = result / mode
            raw = ('fake conditional-free codec ' + mode).encode()
            digest = pipeline.sha(raw); relative = 'blobs/' + digest + '.onlrsp'
            pipeline.atomic_bytes(directory / relative, raw)
            index = {'format': analyze.store.INDEX, 'sourceVersion': SOURCE, 'mode': mode, 'complete': True,
                     'inputReportSha256': old_sha, 'requestedJobs': len(jobs),
                     'programs': [{'fingerprint': FP, 'sourceVersion': SOURCE, 'status': 'success',
                                   'archive': {'path': relative, 'sha256': digest, 'bytes': len(raw),
                                               'mode': mode, 'verifiedEntries': 1}}]}
            pipeline.write(directory / 'program-index.json', index)
            archives.append({'mode': mode, 'index': analyze.runner.file_receipt(directory / 'program-index.json', result)})
        spec = analyze.runner.plan_spec(jobs)
        spec_raw = pipeline.canonical(spec) + b'\n'
        plan = {'format': 'ournotes-deck.luck-response-generation/1', 'mode': 'plan',
                'validationDecks': [], 'archives': [], 'table': {'entries': [], 'context': report['context']},
                'context': report['context'], 'sharedFingerprint': report['sharedFingerprint'],
                'dependencyDescriptor': report['dependencyDescriptor'], 'capabilities': report['capabilities'],
                'jobs': [{**job, 'status': 'planned', 'dependencyDescriptor': {'fixture': True},
                          'dependencyFingerprint': pipeline.sha(pipeline.canonical(job['entries']))} for job in jobs],
                'provenance': {'datasetSha256': hashes['data'], 'inputs': {'rosterSha256': hashes['snapshot'],
                               'requestSha256': hashes['request'], 'specSha256': pipeline.sha(spec_raw)}}}
        resume = analyze.runner.preserve_plan(result, spec_raw, pipeline.canonical(plan) + b'\n')
        row = {'name': 'tw', 'scoreId': 100003, 'status': 'success', 'directory': '.', 'inputSha256': hashes,
               'reusedThroughPlan': True, 'resumePlan': resume, 'sourceVersion': SOURCE,
               'nativeCatalogueValidated': True, 'jobs': len(jobs), 'programFingerprints': [FP],
               'report': analyze.runner.file_receipt(report_path, result), 'archives': archives}
        stream = {'format': analyze.runner.FORMAT, 'backend': 'programs', 'datasetId': dataset,
                  'benchmarkSha256': analyze.file_hash(inp / 'benchmark.json'),
                  'algorithm': {'simSourceSha256': SOURCE}, 'charts': [row]}
        pipeline.write(result / 'manifest.json', stream)
        return report_path, stream, old_sha

    def test_cross_region_dedupe_and_deterministic_deployable_bundle(self):
        self.add_chart('tw', 100003); self.add_chart('jp', 100004)
        result = analyze.analyze([self.results], inputs=[self.inputs], bundle_dir=self.root / 'bundles')
        self.assertTrue(result['valid'], result['errors'])
        self.assertEqual(result['inputMetadata']['requestedJobsAcrossUniqueReports'], 4)
        self.assertEqual(result['sharing']['sharedAcrossKnownCharts'], 1)
        self.assertEqual(result['sharing']['sharedAcrossKnownRegions'], 1)
        dictionary = result['deployableDictionaries'][0]
        self.assertEqual(dictionary['uniquePrograms'], 1); self.assertEqual(dictionary['uniqueBlobs'], 1)
        self.assertEqual(result['inputMetadata']['physicalBlobFiles'], 2)
        deployed = pipeline.read(self.root / 'bundles' / SOURCE / 'u24/program-index.json')
        self.assertNotIn('jobs', deployed); self.assertFalse(deployed['complete'])
        self.assertNotIn('representativeJobIndex', deployed['programs'][0])
        again = analyze.analyze([self.results], inputs=[self.inputs])['deployableDictionaries'][0]
        for method in ('tar', 'gzip9', 'xz6'):
            self.assertEqual(dictionary['compression'][method]['sha256'], again['compression'][method]['sha256'])
            self.assertEqual(dictionary['compression'][method]['bytes'], again['compression'][method]['bytes'])

    def test_conflicting_same_program_refuses_combined_artifact(self):
        self.add_chart('tw', 100003); path = self.add_chart('jp', 100004)
        index = pipeline.read(path); raw = b'conflicting content with a valid own checksum'
        digest = pipeline.sha(raw); relative = 'blobs/' + digest + '.onlrsp'
        pipeline.atomic_bytes(path.parent / relative, raw)
        index['programs'][0]['archive'].update(path=relative, sha256=digest, bytes=len(raw))
        pipeline.write(path, index)
        destination = self.root / 'not-published'
        result = analyze.analyze([self.results], inputs=[self.inputs], bundle_dir=destination)
        self.assertFalse(result['valid']); self.assertFalse(destination.exists())
        self.assertTrue(any('conflicting' in error['error'] for error in result['errors']))

    def test_corrupt_blob_and_malformed_job_mapping_are_visible(self):
        path = self.add_chart('tw', 100003)
        index = pipeline.read(path); blob = path.parent / index['programs'][0]['archive']['path']
        blob.write_bytes(b'broken')
        result = analyze.analyze([self.results], methods=())
        self.assertFalse(result['valid']); self.assertTrue(result['errors'])
        report_path = path.parent.parent / 'programs.json'; report = pipeline.read(report_path)
        report['jobs'][0]['programIndex'] = 8; pipeline.write(report_path, report)
        result = analyze.analyze([self.results], methods=())
        self.assertFalse(result['valid'])
        self.assertTrue(any('mapping' in error['error'] for error in result['errors']))

    def test_missing_report_and_empty_root_do_not_claim_valid_measurement(self):
        path = self.add_chart('tw', 100003)
        report = path.parent.parent / 'programs.json'; moved = report.with_name('custom-interactions.json')
        report.rename(moved)
        missing = analyze.analyze([self.results], methods=())
        self.assertFalse(missing['valid'])
        restored = analyze.analyze([self.results], reports=[moved], methods=())
        self.assertTrue(restored['valid'], restored['errors'])
        self.assertEqual(restored['sharing']['unknownScopeReports'], 1)
        self.assertFalse(analyze.analyze([self.root / 'empty'], methods=())['valid'])
        with self.assertRaisesRegex(ValueError, 'separate'):
            analyze.analyze([self.results], reports=[moved], methods=(), bundle_dir=self.results / 'bundle')

    def test_repin_binds_scope_only_through_verified_plan_and_retains_original_provenance(self):
        report_path, stream, original_sha = self.semantic_chart()
        native = pipeline.read(report_path)
        result = analyze.analyze([self.results], inputs=[self.inputs], methods=())
        self.assertTrue(result['valid'], result['errors'])
        self.assertEqual(result['sharing']['unknownScopeReports'], 0)
        summary = result['reports'][0]
        self.assertEqual(summary['scope']['region'], 'tw')
        self.assertEqual(summary['scope']['scoreId'], 100003)
        self.assertEqual(summary['scope']['datasetSha256'], stream['datasetId'])
        self.assertEqual(summary['scopeBinding']['kind'], 'verifiedSemanticResume')
        self.assertEqual(summary['scopeBinding']['generationInputSha256']['data'], native['provenance']['datasetSha256'])
        self.assertNotEqual(summary['scopeBinding']['generationInputSha256']['data'], stream['datasetId'])
        self.assertEqual(analyze.file_hash(report_path), original_sha)
        # The region-shaped directory alone is insufficient once the proof receipt is absent.
        (report_path.parent / 'manifest.json').unlink()
        missing = analyze.analyze([self.results], inputs=[self.inputs], methods=())
        self.assertTrue(missing['valid'], missing['errors'])
        self.assertEqual(missing['sharing']['unknownScopeReports'], 1)

    def test_semantic_scope_refuses_bad_current_provenance_and_changed_native_dependencies(self):
        report_path, stream, _ = self.semantic_chart()
        result = report_path.parent
        receipt = stream['charts'][0]['resumePlan']['report']
        plan_path = result / receipt['path']
        original = pipeline.read(plan_path)
        for mutation in [lambda value: value['provenance']['inputs'].update(rosterSha256='f'*64),
                         lambda value: value['dependencyDescriptor'].update(stable=False),
                         lambda value: value['jobs'].pop()]:
            changed = copy.deepcopy(original); mutation(changed)
            pipeline.write(plan_path, changed)
            # Updating the transport receipt cannot make an invalid native dependency proof valid.
            stream['charts'][0]['resumePlan']['report'] = analyze.runner.file_receipt(plan_path, result)
            pipeline.write(result / 'manifest.json', stream)
            outcome = analyze.analyze([self.results], inputs=[self.inputs], methods=())
            self.assertFalse(outcome['valid'])
            self.assertTrue(any('provenance' in error['error'] or 'dependencies' in error['error']
                                or 'requested job' in error['error'] for error in outcome['errors']), outcome['errors'])


if __name__ == '__main__':
    unittest.main()
