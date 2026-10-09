#!/usr/bin/env python3
"""Independently audit complete real-chart family shards and their dictionaries.

The declared regional inputs, every native spec/report receipt and each packed
blob are checked before totals or deployable compression measurements are used.
The same native binary repacks each verified response projection through its
codec, with zero DP propagations. Imported data is never a ranking certificate.
"""
from __future__ import annotations

import argparse
from collections import Counter
import gzip
import hashlib
import json
import lzma
import math
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import validate_basis_families as family

pipeline, storage, analysis = family.pipeline, family.storage, family.analysis
FORMAT = 'ournotes-deck.luck-basis-family-audit/1'


def checked_file(root, receipt):
    root = Path(root).resolve()
    path = (root / receipt['path']).resolve()
    if (not path.is_relative_to(root) or not path.is_file()
            or path.stat().st_size != receipt['bytes']
            or family.file_hash(path) != receipt['sha256']):
        raise ValueError('an audited receipt has different bytes or escapes its evidence directory')
    return path


def phase(root, value, name, expected_spec):
    recorded = value['phases'][name]
    spec_path = checked_file(root, recorded['spec'])
    spec = pipeline.read(spec_path)
    if spec != expected_spec:
        raise ValueError('the native specification differs from the actual-master recipe: ' + name)
    report = pipeline.read(checked_file(root, recorded['report']))
    if recorded['nativeStats'] != report.get('stats'):
        raise ValueError('phase work counters differ from the original native report')
    if (type(recorded.get('wallMs')) not in (int, float)
            or not math.isfinite(recorded['wallMs']) or recorded['wallMs'] < 0):
        raise ValueError('phase timing is not a finite nonnegative measured duration')
    return spec, report, family.file_hash(spec_path)


def validate_index(path, source, mode, projection_sha, fingerprints):
    value = pipeline.read(path)
    rows = storage.load_index(path, source, mode)
    if (value.get('complete') is not True or value.get('inputReportSha256') != projection_sha
            or type(value.get('requestedJobs')) is not int or value['requestedJobs'] != len(fingerprints)
            or set(rows) != set(fingerprints)):
        raise ValueError('the packed index is not the complete encoding of its exact verified response projection')
    return rows


def verify_dictionary(directory, value, verified, programs, source, mode, generator):
    """Regenerate deterministic archive bytes, without recording or propagating DP."""
    dictionary = value['dictionary']
    index_path = checked_file(directory, dictionary['index'])
    projection_path = checked_file(directory, dictionary['projection'])
    verified_path = checked_file(directory, value['phases']['verification-original']['report'])
    expected = family.basis.storage_projection(verified, programs, family.file_hash(verified_path))
    expected_bytes = pipeline.canonical(expected) + b'\n'
    if projection_path.read_bytes() != expected_bytes:
        raise ValueError('the persisted projection differs from the independently verified native conditional responses')
    fingerprints = {program['fingerprint'] for program in programs}
    if (dictionary.get('mode') != mode or dictionary.get('programs') != len(fingerprints)
            or dictionary.get('globalSkillCombinationAliasesStored') is not False):
        raise ValueError('the deployable dictionary changes verified native coverage or stores combination aliases')
    projection_sha = pipeline.sha(expected_bytes)
    rows = validate_index(index_path, source, mode, projection_sha, fingerprints)
    with tempfile.TemporaryDirectory(prefix='luck-family-codec-audit-') as temporary:
        temporary = Path(temporary)
        expected_path = temporary / 'programs.json'
        expected_path.write_bytes(expected_bytes)
        generated = temporary / 'encoded'
        storage.call([generator, 'program-pack', expected_path, mode, generated], temporary / 'codec.log', None)
        recoded = validate_index(generated / 'program-index.json', source, mode, projection_sha, fingerprints)
        blobs = {}
        reference_bytes = 0
        for fingerprint, record in rows.items():
            path, original_bytes = storage.verified_blob(index_path.parent, record, source, mode)
            if not path.resolve().is_relative_to(index_path.parent.resolve()):
                raise ValueError('a stored conditional blob escapes its audited dictionary directory')
            _, recoded_bytes = storage.verified_blob(generated, recoded[fingerprint], source, mode)
            # Timing fields inside program-index.json are observations. The
            # program's contextual archive payload and identity are deterministic.
            if (any(record['archive'].get(field) != recoded[fingerprint]['archive'].get(field)
                    for field in ('sha256', 'bytes', 'mode')) or original_bytes != recoded_bytes
                    or not original_bytes.startswith(b'ONLRSP02')):
                raise ValueError('a stored conditional blob differs from encoding its independently verified native curve')
            blobs[record['archive']['sha256']] = len(original_bytes)
            reference_bytes += len(original_bytes)
    if (dictionary.get('uniqueBlobs') != len(blobs)
            or dictionary.get('indexAndUniqueBlobBytes') != index_path.stat().st_size + sum(blobs.values())
            or dictionary.get('sumReferencedBlobBytes') != reference_bytes):
        raise ValueError('a per-chart dictionary size summary differs from its verified index and blobs')
    return index_path.parent, projection_path, fingerprints


def audit_chart(root, row, case, specs, source, mode, generator):
    value = pipeline.read(checked_file(root, row['receipt']))
    directory = (Path(root) / row['directory']).resolve()
    if not directory.is_relative_to(Path(root).resolve()):
        raise ValueError('a chart directory escapes its shard')
    hashes = pipeline.provenance(case['paths'])
    if (value.get('format') != family.CASE_FORMAT or value.get('complete') is not True
            or value.get('status') != 'success' or value.get('inputsUnchanged') is not True
            or value.get('sourceVersion') != source or value.get('inputSha256') != hashes
            or value.get('scoreId') != case['scoreId'] or value.get('name') != case['name']
            or value.get('difficulty') != case['chart']['difficulty']
            or value.get('missions') != case['chart']['missions'] or row.get('status') != 'success'):
        raise ValueError('a chart receipt is incomplete or has a different native input scope')
    for field in ('grid', 'verification', 'controls', 'dictionary'):
        if row[field] != value[field]:
            raise ValueError('shard summary differs from its complete chart receipt: ' + field)
    spec, fast, sha = phase(directory, value, 'grid-fast', specs['grid-fast.json'])
    mappings, fingerprints, references, counts = family.identify(
        fast, spec, hashes, sha, source, single_family=True)
    starts = sum(mission == 2 for mission in case['chart']['missions'])
    if (not 1 <= starts <= 3 or len(fingerprints) > 3 ** starts
            or any(job['startCount'] != starts for job in fast['jobs'])
            or value['grid'] != {'jobs': len(mappings), 'conditionalTermReferences': references,
                'uniquePrograms': len(fingerprints), 'actualLuckStarts': starts,
                'maximumProgramsForThisChart': 3 ** starts, 'familyCounts': counts}):
        raise ValueError('conditional coverage differs from the actual native LUCK starts')
    spec, verified, sha = phase(directory, value, 'verification-original', specs['verification-original.json'])
    reference, programs, verification = family.verified_programs(verified, spec, hashes, sha, source)
    family.same_context(fast, verified)
    verification.update(family.compare_mappings(mappings, reference))
    if fingerprints != {program['fingerprint'] for program in programs}:
        raise ValueError('the independent native evaluations do not cover all grid conditional terms')
    verification['coversAllGridPrograms'] = True
    if value['verification'] != verification:
        raise ValueError('original-DP comparison evidence differs from the native reports')
    controls = []
    for name in ('controls-fast', 'controls-original'):
        spec, report, sha = phase(directory, value, name, specs[name + '.json'])
        mapping, _, _, _ = family.identify(report, spec, hashes, sha, source)
        family.same_context(fast, report)
        controls.append((mapping, report))
    family.same_context(controls[0][1], controls[1][1], same_jobs=True)
    compared = family.compare_mappings(controls[0][0], controls[1][0])
    separated = family.control_separation(controls[0][0])
    new = [name for name in controls[0][0] if name.startswith('minimum-new-')]
    if (len(new) != 6 or controls[0][1]['stats']['nativeRecordings'] < 4
            or any(term['programFingerprint'] not in fingerprints for name in new for term in controls[0][0][name])):
        raise ValueError('new minimum combinations or changed-controller recording coverage differs')
    expected_controls = {**compared, 'nonminimumSeparation': separated,
                         'newMinimumCombinations': 6, 'newMinimumPropagationCalls': 0}
    if value['controls'] != expected_controls:
        raise ValueError('controller comparison summary differs from its exact native mappings')
    if 'grid-original.json' in specs:
        spec, old, sha = phase(directory, value, 'grid-original', specs['grid-original.json'])
        reference, old_fingerprints, _, _ = family.identify(old, spec, hashes, sha, source)
        family.same_context(fast, old, same_jobs=True)
        if (old_fingerprints != fingerprints
                or value['fullReferenceGrid'] != family.compare_mappings(mappings, reference)):
            raise ValueError('the complete uncached reference grid differs')
    index_directory, projection, dictionary_fingerprints = verify_dictionary(
        directory, value, verified, programs, source, mode, generator)
    audited = {'name': case['name'], 'scoreId': case['scoreId'], 'status': 'success',
            'difficulty': case['chart']['difficulty'], 'luckStarts': starts,
            'jobs': len(mappings), 'nativeRecordings': counts['nativeRecordings'],
            'familyHits': counts['familyHits'], 'programs': len(fingerprints),
            'verificationCalls': verification['verifiedJobs'],
            'comparedBuckets': verification['comparedBuckets'],
            'maximumEndpointDifference': verification['maximumEndpointDifference'],
            'gridWallMs': value['phases']['grid-fast']['wallMs'],
            'gridNativeStats': fast['stats'], 'report': row['receipt'],
            'codecRepackedPrograms': len(dictionary_fingerprints), 'codecPropagationCalls': 0}
    return audited, index_directory, projection, dictionary_fingerprints


def verify_merged_dictionary(report, source, mode, fingerprints, bundles):
    dictionaries = report.get('deployableDictionaries', [])
    if report.get('valid') is not True or len(dictionaries) != 1:
        raise ValueError('the merged deployment must contain exactly one fully audited source/codec dictionary')
    dictionary = dictionaries[0]
    records = dictionary.get('perProgramArchiveBytes', [])
    if (dictionary.get('sourceVersion') != source or dictionary.get('mode') != mode
            or dictionary.get('uniquePrograms') != len(fingerprints) or len(records) != len(fingerprints)
            or {record.get('fingerprint') for record in records} != set(fingerprints)):
        raise ValueError('the merged deployment includes missing, extra or wrong-source native programs')
    directory = Path(bundles).resolve() / source / mode
    compression = dictionary['compression']
    tar = compression['tar']
    for method, opener in (('gzip9', gzip.open), ('xz6', lzma.open)):
        compressed = compression[method]
        packed = checked_file(directory, {'path': compressed['artifact'], 'bytes': compressed['bytes'],
                                           'sha256': compressed['sha256']})
        digest, size = hashlib.sha256(), 0
        with opener(packed, 'rb') as stream:
            while chunk := stream.read(1024 * 1024):
                digest.update(chunk); size += len(chunk)
                if size > tar['bytes']:
                    raise ValueError('merged dictionary decompression exceeds its exact source TAR length')
        if size != tar['bytes'] or digest.hexdigest() != tar['sha256']:
            raise ValueError('merged dictionary compression did not round-trip to its exact source TAR')
        compressed['roundTripVerified'] = True
    return dictionary


def timing(values):
    ordered = sorted(values)
    if not ordered:
        return {'count': 0}
    return {'count': len(values), 'sumMs': sum(values), 'medianMs': statistics.median(values),
            'minimumMs': ordered[0], 'maximumMs': ordered[-1],
            'p95Ms': ordered[min(len(ordered) - 1, (95 * len(ordered) + 99) // 100 - 1)]}


def audit(inputs, roots, generator, source, output, bundles, mode='u24'):
    inputs, roots = [Path(path).resolve() for path in inputs], [Path(path).resolve() for path in roots]
    generator = Path(generator).resolve()
    expected, by_dataset = {}, {}
    algorithm = pipeline.source_identity(source)
    source_sha = algorithm['simSourceSha256']
    generator_sha = family.file_hash(generator)
    for manifest_path in inputs:
        manifest, inventory, data, cases = family.declared_cases(manifest_path)
        region, dataset = inventory['region'], inventory['datasetId']
        if region not in ('tw', 'jp') or region in expected or dataset in by_dataset:
            raise ValueError('one independently pinned TW and JP catalogue is required')
        expected[region] = {'cases': {case['scoreId']: case for case in cases}, 'data': data,
                            'datasetId': dataset, 'manifestSha256': family.file_hash(manifest_path)}
        by_dataset[dataset] = region
    if set(expected) != {'tw', 'jp'}:
        raise ValueError('both complete regional catalogues are required')
    result = {'format': FORMAT, 'complete': False, 'algorithm': algorithm,
              'sourceVersion': source_sha, 'generatorSha256': generator_sha,
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
              'scope': 'All declared TW/JP chart and difficulty records; 1,250 actual minimum configurations per natural LUCK chart in one fixed non-minimum family. Native codec repacking uses zero DP. Not all owned teams or all play profiles.',
              'shards': [], 'charts': [], 'errors': []}
    seen, shard_keys = set(), set()
    audited_index_roots, audited_projections, audited_fingerprints = set(), set(), set()
    paths = sorted({path for root in roots for path in root.rglob('validation.json')})
    for path in paths:
        value = pipeline.read(path)
        if value.get('format') != family.FORMAT:
            continue
        try:
            region = by_dataset[value['datasetId']]
            declaration = expected[region]
            if (value.get('complete') is not True or value.get('inputsUnchanged') is not True
                    or value.get('status') != 'success' or value.get('region') != region
                    or value.get('sourceVersion') != source_sha or value.get('algorithm') != algorithm
                    or value.get('generatorSha256') != generator_sha
                    or value.get('manifestSha256') != declaration['manifestSha256'] or value.get('mode') != mode
                    or value.get('runnerSha256') != family.file_hash(family.__file__)):
                raise ValueError('a shard has incomplete, different-source or unpinned input evidence')
            shard = value['shard']
            shard_key = (region, shard['count'], shard['index'])
            if shard.get('ordering') != family.SHARD_ORDERING or shard_key in shard_keys:
                raise ValueError('duplicate or differently ordered catalogue shard')
            selected = family.shard_cases(list(declaration['cases'].values()), shard['index'], shard['count'])
            rows = value['charts']
            if ([row['scoreId'] for row in rows] != [case['scoreId'] for case in selected]
                    or shard['catalogueCharts'] != len(declaration['cases']) or shard['selectedCharts'] != len(selected)):
                raise ValueError('a shard omits, duplicates or changes its assigned master chart set')
            specs, _ = family.specifications(declaration['data'], value['options']['verificationJobsPerChart'],
                                              value['options']['fullReferenceGrid'])
            shard_keys.add(shard_key)
            for row, case in zip(rows, selected):
                key = (region, case['scoreId'])
                chart = case['chart']
                if (key in seen or row.get('inputSha256') != pipeline.provenance(case['paths'])
                        or row.get('name') != case['name'] or row.get('difficulty') != chart['difficulty']
                        or row.get('missions') != chart['missions'] or row.get('complete') is not True):
                    raise ValueError('a chart is duplicated, altered or incomplete')
                if chart['hasLuckMission']:
                    audited, index_root, projection, fingerprints = audit_chart(
                        path.parent, row, case, specs, source_sha, mode, generator)
                    audited_index_roots.add(index_root)
                    audited_projections.add(projection)
                    audited_fingerprints.update(fingerprints)
                else:
                    if row.get('status') != 'notApplicable' or row.get('nativeCalls') != 0:
                        raise ValueError('a non-LUCK record is not explicitly accounted for without native work')
                    audited = {'name': case['name'], 'scoreId': case['scoreId'],
                               'difficulty': chart['difficulty'], 'status': 'notApplicable'}
                seen.add(key)
                result['charts'].append({'region': region, 'datasetId': declaration['datasetId'], **audited})
            result['shards'].append({'region': region, 'shard': shard, 'sha256': family.file_hash(path),
                                      'bytes': path.stat().st_size})
        except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
            result['errors'].append({'path': str(path), 'error': str(error)})
    expected_keys = {(region, score) for region, declaration in expected.items() for score in declaration['cases']}
    missing = sorted(expected_keys - seen)
    if missing:
        result['errors'].append({'missingRegionalCharts': missing})
    successful = [row for row in result['charts'] if row['status'] == 'success']
    result['coverage'] = {'declaredRegionalCharts': len(expected_keys), 'receivedRegionalCharts': len(seen),
                          'statuses': dict(Counter(row['status'] for row in result['charts'])),
                          'allTeamCombinationsCovered': False, 'allPlayProfilesCovered': False}
    result['work'] = {field: sum(row.get(key, 0) for row in successful) for field, key in
                       (('fastGridJobs', 'jobs'), ('gridNativeRecordings', 'nativeRecordings'),
                        ('gridFamilyHits', 'familyHits'), ('regionalConditionalPrograms', 'programs'),
                        ('originalDpVerificationCalls', 'verificationCalls'), ('comparedBuckets', 'comparedBuckets'),
                        ('codecRepackedPrograms', 'codecRepackedPrograms'), ('codecPropagationCalls', 'codecPropagationCalls'))}
    result['work']['maximumEndpointDifference'] = max((row['maximumEndpointDifference'] for row in successful), default=0)
    result['fastGridTiming'] = timing([row['gridWallMs'] for row in successful])
    if not result['errors']:
        try:
            # Only these exact indexes and projections survived native response
            # and codec verification. Other files under artifact roots confer no
            # membership in the complete-catalogue deployment dictionary.
            dictionary = analysis.analyze(sorted(audited_index_roots), sorted(audited_projections),
                                          [path.parent.parent for path in inputs], ('gzip9', 'xz6'), bundles)
            verify_merged_dictionary(dictionary, source_sha, mode, audited_fingerprints, bundles)
            analysis_path = Path(output).with_name('program-analysis.json')
            pipeline.write(analysis_path, dictionary)
            result['dictionaryAnalysis'] = {'sha256': family.file_hash(analysis_path),
                                            'bytes': analysis_path.stat().st_size,
                                            'valid': dictionary['valid'],
                                            'dictionaries': dictionary['deployableDictionaries']}
        except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
            result['errors'].append({'dictionaryAudit': str(error)})
    if family.file_hash(generator) != generator_sha:
        result['errors'].append({'generator': 'the native codec binary changed during the audit'})
    result['complete'] = seen == expected_keys and not result['errors']
    pipeline.write(output, result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, action='append', required=True)
    parser.add_argument('--root', type=Path, action='append', required=True)
    parser.add_argument('--generator', type=Path, required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--bundle-dir', type=Path, required=True)
    args = parser.parse_args()
    result = audit(args.manifest, args.root, args.generator, args.source, args.output, args.bundle_dir)
    print(json.dumps({key: result[key] for key in ('complete', 'coverage', 'work', 'fastGridTiming')}))
    return 0 if result['complete'] else 1


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        raise SystemExit('LUCK family audit: ' + str(error))
