"""Audit boundaries only; fixture codecs and native reports are not performance evidence."""
import copy
import gzip
import importlib.util
import lzma
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('family_aggregate_tests',
                                             Path(__file__).with_name('aggregate_basis_families.py'))
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)
SOURCE, FP = 'a' * 64, 'b' * 64


class FamilyAggregateTests(unittest.TestCase):
    def test_file_receipts_reject_different_content_and_directory_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            evidence = root / 'evidence'; evidence.mkdir()
            path = evidence / 'report.json'; path.write_bytes(b'checked')
            receipt = subject.family.receipt(path, evidence)
            self.assertEqual(subject.checked_file(evidence, receipt), path)
            for changed in ({'bytes': 1}, {'sha256': 'f' * 64}):
                with self.assertRaises(ValueError):
                    subject.checked_file(evidence, {**receipt, **changed})
            outside = root / 'outside'; outside.write_bytes(b'checked')
            with self.assertRaises(ValueError):
                subject.checked_file(evidence, {**receipt, 'path': '../outside'})

    def test_projection_and_decoded_archive_must_bind_to_independent_native_responses(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            verified = {'unitTest': 'native-response-receipt-only'}
            programs = [{'fingerprint': FP}]
            expected = {'unitTest': 'expected-projection', 'programs': programs}
            verified_path = root / 'verification.json'; subject.pipeline.write(verified_path, verified)
            projection = root / 'programs.json'; subject.pipeline.write(projection, expected)
            dictionary = root / 'dictionary'; dictionary.mkdir()
            correct_bytes = b'ONLRSP02unit-test-contextual-payload'

            def make_index(directory, raw):
                blob_sha = subject.pipeline.sha(raw)
                blob = directory / 'blobs' / (blob_sha + '.onlrsp'); blob.parent.mkdir(parents=True, exist_ok=True)
                blob.write_bytes(raw)
                row = {'fingerprint': FP, 'sourceVersion': SOURCE, 'status': 'success',
                       'archive': {'path': 'blobs/' + blob_sha + '.onlrsp', 'sha256': blob_sha,
                                   'bytes': len(raw), 'mode': 'u24', 'verifiedEntries': 1}}
                index = {'format': subject.storage.INDEX, 'sourceVersion': SOURCE, 'mode': 'u24',
                         'complete': True, 'requestedJobs': 1, 'programs': [row],
                         'inputReportSha256': subject.family.file_hash(projection)}
                subject.pipeline.write(directory / 'program-index.json', index)
                return index

            def metadata():
                index = dictionary / 'program-index.json'
                rows = subject.storage.load_index(index, SOURCE, 'u24')
                size = sum(row['archive']['bytes'] for row in rows.values())
                return {'dictionary': {'mode': 'u24', 'programs': 1, 'uniqueBlobs': 1,
                    'globalSkillCombinationAliasesStored': False,
                    'index': subject.family.receipt(index, root), 'projection': subject.family.receipt(projection, root),
                    'indexAndUniqueBlobBytes': index.stat().st_size + size, 'sumReferencedBlobBytes': size},
                    'phases': {'verification-original': {'report': subject.family.receipt(verified_path, root)}}}

            def codec_fixture(command, log, timeout):
                self.assertEqual(command[1], 'program-pack')
                self.assertEqual(Path(command[2]).read_bytes(), subject.pipeline.canonical(expected) + b'\n')
                make_index(Path(command[4]), correct_bytes)

            make_index(dictionary, correct_bytes)
            with patch.object(subject.family.basis, 'storage_projection', return_value=expected), \
                 patch.object(subject.storage, 'call', side_effect=codec_fixture):
                directory, projected, fingerprints = subject.verify_dictionary(
                    root, metadata(), verified, programs, SOURCE, 'u24', root / 'fixture-codec')
                self.assertEqual((directory, projected, fingerprints), (dictionary, projection, {FP}))
                # Valid fresh checksums do not authorize a changed response projection.
                subject.pipeline.write(projection, {**expected, 'changedResponse': True})
                with self.assertRaisesRegex(ValueError, 'projection differs'):
                    subject.verify_dictionary(root, metadata(), verified, programs, SOURCE, 'u24', root / 'fixture-codec')
                subject.pipeline.write(projection, expected)
                # A self-consistent index/blob SHA still needs the independent codec comparison.
                make_index(dictionary, b'ONLRSP02different-probability-payload')
                with self.assertRaisesRegex(ValueError, 'stored conditional blob differs'):
                    subject.verify_dictionary(root, metadata(), verified, programs, SOURCE, 'u24', root / 'fixture-codec')
                for changed in ({'complete': False}, {'inputReportSha256': 'e' * 64}, {'requestedJobs': 2}):
                    index = make_index(dictionary, correct_bytes)
                    subject.pipeline.write(dictionary / 'program-index.json', {**index, **changed})
                    with self.assertRaisesRegex(ValueError, 'complete encoding'):
                        subject.verify_dictionary(root, metadata(), verified, programs, SOURCE, 'u24', root / 'fixture-codec')

    def test_merged_dictionary_has_exact_audited_union_and_verified_compression(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / SOURCE / 'u24'; directory.mkdir(parents=True)
            raw = b'round-trip unit fixture, not an ONLRSP performance response'
            compression = {'tar': {'bytes': len(raw), 'sha256': subject.pipeline.sha(raw)}}
            for method, data, name in (('gzip9', gzip.compress(raw, mtime=0), 'dictionary.tar.gz'),
                                       ('xz6', lzma.compress(raw), 'dictionary.tar.xz')):
                path = directory / name; path.write_bytes(data)
                compression[method] = {'artifact': name, 'bytes': len(data), 'sha256': subject.pipeline.sha(data)}
            dictionary = {'sourceVersion': SOURCE, 'mode': 'u24', 'uniquePrograms': 1,
                          'perProgramArchiveBytes': [{'fingerprint': FP}], 'compression': compression}
            report = {'valid': True, 'deployableDictionaries': [dictionary]}
            checked = subject.verify_merged_dictionary(copy.deepcopy(report), SOURCE, 'u24', {FP}, root)
            self.assertTrue(checked['compression']['gzip9']['roundTripVerified'])
            self.assertTrue(checked['compression']['xz6']['roundTripVerified'])
            changes = [lambda value: value['deployableDictionaries'].append(copy.deepcopy(dictionary)),
                       lambda value: value['deployableDictionaries'][0].update(sourceVersion='e' * 64),
                       lambda value: value['deployableDictionaries'][0].update(mode='u16'),
                       lambda value: value['deployableDictionaries'][0]['perProgramArchiveBytes'].append({'fingerprint': 'f' * 64})]
            for change in changes:
                modified = copy.deepcopy(report); change(modified)
                with self.assertRaises(ValueError):
                    subject.verify_merged_dictionary(modified, SOURCE, 'u24', {FP}, root)
            # The compressed object's own receipt is valid; its decoded payload is wrong.
            wrong = gzip.compress(b'wrong decoded bytes', mtime=0)
            path = directory / 'dictionary.tar.gz'; path.write_bytes(wrong)
            changed = copy.deepcopy(report)
            changed['deployableDictionaries'][0]['compression']['gzip9'].update(
                bytes=len(wrong), sha256=subject.pipeline.sha(wrong))
            with self.assertRaisesRegex(ValueError, 'round-trip'):
                subject.verify_merged_dictionary(changed, SOURCE, 'u24', {FP}, root)

    def test_full_coverage_and_deployment_inputs_are_separate_from_extra_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            generator = root / 'codec'; generator.write_bytes(b'unit-fixture-not-executed')
            algorithm = {'simSourceSha256': SOURCE, 'digest': 'c' * 64, 'files': {}}
            declarations, manifests, shard_roots = {}, [], []
            approved_index = root / 'shards/tw/charts/100/dictionary/u24'
            approved_projection = root / 'shards/tw/charts/100/programs.json'
            for region, score, lucky in (('tw', 100, True), ('jp', 200, False)):
                data_root = root / 'inputs' / region; data_root.mkdir(parents=True)
                paths = {}
                for field in ('data', 'snapshot', 'request'):
                    path = data_root / (field + '.json')
                    subject.pipeline.write(path, {'unitTest': field, 'region': region})
                    paths[field] = path
                dataset = subject.family.file_hash(paths['data'])
                manifest = data_root / 'benchmark.json'; subject.pipeline.write(manifest, {'unitTest': 'declared-input'})
                manifests.append(manifest)
                chart = {'scoreId': score, 'musicId': score // 100, 'difficulty': 'easy',
                         'hasLuckMission': lucky, 'missions': [2, 2, 2] if lucky else [1, 1, 1]}
                case = {'scoreId': score, 'name': region + '-case', 'chart': chart, 'paths': paths}
                declarations[manifest] = ({}, {'region': region, 'datasetId': dataset}, {}, [case])
                shard_root = root / 'shards' / region; shard_root.mkdir(parents=True)
                shard_roots.append(shard_root)
                row = {'scoreId': score, 'name': case['name'], 'difficulty': chart['difficulty'],
                       'missions': chart['missions'], 'inputSha256': subject.pipeline.provenance(paths),
                       'complete': True, 'status': 'success' if lucky else 'notApplicable', 'nativeCalls': 0}
                value = {'format': subject.family.FORMAT, 'complete': True, 'inputsUnchanged': True, 'status': 'success',
                    'region': region, 'datasetId': dataset, 'sourceVersion': SOURCE, 'algorithm': algorithm,
                    'generatorSha256': subject.family.file_hash(generator), 'manifestSha256': subject.family.file_hash(manifest),
                    'mode': 'u24', 'runnerSha256': subject.family.file_hash(subject.family.__file__),
                    'shard': {'index': 0, 'count': 1, 'ordering': subject.family.SHARD_ORDERING,
                              'catalogueCharts': 1, 'selectedCharts': 1},
                    'options': {'verificationJobsPerChart': 6, 'fullReferenceGrid': False}, 'charts': [row]}
                subject.pipeline.write(shard_root / 'validation.json', value)
            extra = root / 'shards/stray/program-index.json'; extra.parent.mkdir()
            subject.pipeline.write(extra, {'unitTest': 'valid-looking unrelated dictionary must not be discovered'})
            audited = {'name': 'tw-case', 'scoreId': 100, 'status': 'success', 'difficulty': 'easy',
                       'jobs': 1250, 'nativeRecordings': 1, 'familyHits': 1249, 'programs': 1,
                       'verificationCalls': 6, 'comparedBuckets': 4, 'maximumEndpointDifference': 0,
                       'gridWallMs': 1, 'codecRepackedPrograms': 1, 'codecPropagationCalls': 0}
            deployment = {'valid': True, 'deployableDictionaries': [{'unitTest': 'merged codec checked elsewhere'}]}
            with patch.object(subject.family, 'declared_cases', side_effect=lambda path: declarations[Path(path)]), \
                 patch.object(subject.pipeline, 'source_identity', return_value=algorithm), \
                 patch.object(subject.family, 'specifications', return_value=({}, {})), \
                 patch.object(subject, 'audit_chart', return_value=(audited, approved_index, approved_projection, {FP})), \
                 patch.object(subject.analysis, 'analyze', return_value=deployment) as analyze, \
                 patch.object(subject, 'verify_merged_dictionary') as verify:
                result = subject.audit(manifests, [root / 'shards'], generator, root,
                                       root / 'complete.json', root / 'bundles')
                self.assertTrue(result['complete'])
                self.assertEqual(result['coverage']['receivedRegionalCharts'], 2)
                self.assertEqual(analyze.call_args.args[0], [approved_index])
                self.assertEqual(analyze.call_args.args[1], [approved_projection])
                self.assertEqual(verify.call_args.args[3], {FP})
                analyze.reset_mock()
                missing = subject.audit(manifests, [shard_roots[0]], generator, root,
                                        root / 'missing.json', root / 'bundles')
                self.assertFalse(missing['complete'])
                self.assertTrue(any('missingRegionalCharts' in error for error in missing['errors']))
                analyze.assert_not_called()
                duplicate = root / 'shards/duplicate'; duplicate.mkdir()
                subject.pipeline.write(duplicate / 'validation.json', subject.pipeline.read(shard_roots[0] / 'validation.json'))
                repeated = subject.audit(manifests, [root / 'shards'], generator, root,
                                         root / 'duplicate.json', root / 'bundles')
                self.assertFalse(repeated['complete'])
                self.assertTrue(any('duplicate' in error.get('error', '') for error in repeated['errors']))


if __name__ == '__main__':
    unittest.main()
