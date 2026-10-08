#!/usr/bin/env python3
"""Audit and measure native program dictionaries without executing native code.

Report/job metadata and deployable index+blob costs are separate. The deployed
index contains program identities, never a global skill-combination alias list.
Compression uses a deterministic tar stream, bounded per-file reads, and temporary
files; raw response curves are not accumulated across reports.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import gzip
import hashlib
import importlib.util
import io
import json
import lzma
from pathlib import Path
import shutil
import tarfile
import tempfile
import time

_SPEC = importlib.util.spec_from_file_location('luck_program_analysis_store', Path(__file__).with_name('programs.py'))
store = importlib.util.module_from_spec(_SPEC); _SPEC.loader.exec_module(store)
pipeline = store.pipeline
_RUNNER_SPEC = importlib.util.spec_from_file_location('luck_program_analysis_runner', Path(__file__).with_name('stream_runner.py'))
runner = importlib.util.module_from_spec(_RUNNER_SPEC); _RUNNER_SPEC.loader.exec_module(runner)
try:
    import zstandard
except ImportError:
    zstandard = None

FORMAT = 'ournotes-deck.luck-program-analysis/1'
REPORT_LIMIT = 512 * 1024 * 1024
CHUNK = 1024 * 1024


def file_hash(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        while chunk := stream.read(CHUNK):
            h.update(chunk)
    return h.hexdigest()


def read_json(path, limit=REPORT_LIMIT):
    path = Path(path)
    if path.stat().st_size > limit:
        raise ValueError(f'{path.name}: exceeds explicit JSON size limit {limit}')
    return pipeline.read(path)


def label(path, roots):
    path = Path(path).resolve()
    for index, root in enumerate(roots):
        if path.is_relative_to(root):
            return f'root-{index}/' + path.relative_to(root).as_posix()
    return path.name


def input_scopes(roots, with_resume=False):
    """Bind scope only to exact dataset/request hashes from declared input files."""
    region_by_dataset, scope = {}, {}
    for root in roots:
        for path in sorted(root.rglob('catalogue.json')):
            value = read_json(path)
            if value.get('format') == 'ournotes-deck.luck-catalogue/1' and value.get('region'):
                dataset, region = value['datasetId'], value['region']
                if dataset in region_by_dataset and region_by_dataset[dataset] != region:
                    raise ValueError('dataset has conflicting declared regions')
                region_by_dataset[dataset] = region
    checked_data, resume_bindings = {}, {}
    for root in roots:
        for path in sorted(root.rglob('benchmark.json')):
            manifest = read_json(path)
            if manifest.get('format') != 'ournotes-deck.search-benchmark/1':
                continue
            dataset = store.digest(manifest['datasetId'])
            benchmark_sha = file_hash(path) if with_resume else None
            for case in manifest['cases']:
                data_path = (path.parent / case['data']).resolve()
                if data_path not in checked_data:
                    checked_data[data_path] = file_hash(data_path)
                if checked_data[data_path] != dataset:
                    raise ValueError('scope manifest dataset hash differs')
                request_path = path.parent / case['request']
                request = read_json(request_path)
                request_sha = file_hash(request_path)
                key = (dataset, request_sha)
                value = {'datasetSha256': dataset, 'region': region_by_dataset.get(dataset),
                         'scoreId': request['execution']['scoreId'],
                         'musicId': request['scenario'].get('musicId')}
                if key in scope and scope[key] != value:
                    raise ValueError('same request/input identity has conflicting chart scope')
                scope[key] = value
                declaration = manifest.get('luckCatalogue', {})
                if (with_resume and declaration.get('format') == runner.catalogue.INVENTORY_FORMAT
                        and declaration.get('selection') == 'all-master-source-levels'
                        and declaration.get('includeNonLuckCharts') is True and 'snapshot' in case):
                    keys = declaration.get('keys')
                    if not isinstance(keys, list):
                        raise ValueError('semantic scope input lacks its declared complete key catalogue')
                    binding = {'scope': value, 'keys': keys, 'benchmarkSha256': benchmark_sha,
                               'inputSha256': {'data': dataset, 'request': request_sha,
                                               'snapshot': file_hash(path.parent / case['snapshot'])}}
                    case_key = (benchmark_sha, case['name'])
                    if case_key in resume_bindings and resume_bindings[case_key] != binding:
                        raise ValueError('same benchmark case has conflicting semantic scope bindings')
                    resume_bindings[case_key] = binding
    return (scope, resume_bindings) if with_resume else scope


def semantic_scope_records(roots, bindings):
    """Locate candidate stream receipts; directory names confer no scope authority."""
    records, errors = defaultdict(list), []
    if not bindings:
        return records, errors
    for path in sorted({path for root in roots for path in root.rglob('manifest.json')}):
        try:
            value = read_json(path)
            if value.get('format') != runner.FORMAT:
                continue
            for row in value.get('charts', []):
                binding = bindings.get((value.get('benchmarkSha256'), row.get('name')))
                if row.get('reusedThroughPlan') is not True or binding is None:
                    continue
                directory = (path.parent / row['directory']).resolve()
                report = (directory / row['report']['path']).resolve()
                if not directory.is_relative_to(path.parent.resolve()) or not report.is_relative_to(directory):
                    raise ValueError('semantic scope receipt escapes its stream directory')
                records[report].append((path, value, directory, row, binding))
        except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
            errors.append({'path': label(path, roots), 'error': str(error)})
    return records, errors


def report_scope(path, report, source, scopes, records):
    provenance = report.get('provenance', {})
    original = {'data': provenance.get('datasetSha256'),
                'snapshot': provenance.get('inputs', {}).get('rosterSha256'),
                'request': provenance.get('inputs', {}).get('requestSha256')}
    key = (original['data'], original['request'])
    scope = scopes.get(key, {'datasetSha256': key[0], 'region': None, 'scoreId': None, 'musicId': None})
    detail = {'kind': 'exactInputProvenance' if key in scopes else 'unmatchedInputProvenance',
              'generationInputSha256': original}
    accepted = None
    for manifest_path, manifest, directory, row, binding in records.get(path.resolve(), []):
        hashes, keys = binding['inputSha256'], binding['keys']
        jobs = pipeline.base_jobs(keys)
        if (manifest.get('backend') != 'programs' or manifest.get('datasetId') != hashes['data']
                or manifest.get('algorithm', {}).get('simSourceSha256') != source
                or row.get('status') != 'success' or row.get('nativeCatalogueValidated') is not True
                or row.get('sourceVersion') != source or row.get('scoreId') != binding['scope']['scoreId']
                or row.get('inputSha256') != hashes or row.get('jobs') != len(jobs)):
            raise ValueError('semantic scope stream metadata differs from its current declared inputs')
        if not runner.reusable_programs(directory, row):
            raise ValueError('semantic scope needs complete verified program reports and all four archives')
        if runner.verified_file(directory, row['report']).resolve() != path.resolve():
            raise ValueError('semantic scope refers to a different original program report')
        fingerprints = runner.validate_program_report(report, jobs, keys, source)
        if fingerprints != row.get('programFingerprints'):
            raise ValueError('semantic scope program mapping differs from the retained native report')
        runner.validate_semantic_resume(directory, row, report, jobs, keys, source, hashes)
        if accepted is not None and accepted != binding['scope']:
            raise ValueError('one retained report has conflicting current semantic scopes')
        accepted = binding['scope']
        scope = accepted
        detail = {'kind': 'verifiedSemanticResume', 'generationInputSha256': original,
                  'currentInputSha256': hashes, 'benchmarkSha256': binding['benchmarkSha256'],
                  'streamManifestSha256': file_hash(manifest_path),
                  'resumePlanSha256': row['resumePlan']['report']['sha256'],
                  'resumeSpecSha256': row['resumePlan']['spec']['sha256']}
    return scope, detail


def report_summary(path, scopes, resume_records=None):
    value = read_json(path)
    if value.get('format') != store.REPORT:
        raise ValueError('not a native program report')
    source = store.digest(value['sourceVersion'])
    jobs, programs = value['jobs'], value['programs']
    if not isinstance(jobs, list) or not isinstance(programs, list) or not jobs:
        raise ValueError('program report has no requested jobs')
    names = [row['name'] for row in jobs]
    if len(set(names)) != len(names):
        raise ValueError('program report repeats requested job names')
    fingerprints = [store.digest(row['fingerprint']) for row in programs]
    if len(set(fingerprints)) != len(fingerprints):
        raise ValueError('program report repeats fingerprints')
    for program in programs:
        if program.get('sourceVersion') != source:
            raise ValueError('program report mixes native source versions')
    statuses = Counter(row['status'] for row in jobs)
    successful, references = set(), Counter()
    for job in jobs:
        if job['status'] not in ('success', 'identified'):
            continue
        number = job.get('programIndex')
        if (type(number) is not int or not 0 <= number < len(programs)
                or job.get('programFingerprint') != fingerprints[number]):
            raise ValueError('reported job/program identity mapping differs')
        references[fingerprints[number]] += 1
        if job['status'] == 'success':
            program = programs[number]
            if program.get('status') != 'success' or program.get('response', {}).get('status') != 'success':
                raise ValueError('successful job references an unavailable response')
            successful.add(fingerprints[number])
    complete = value.get('complete') is True and value.get('probabilityComplete') is True
    if complete and (statuses != {'success': len(jobs)} or len(successful) != len(programs)):
        raise ValueError('claimed complete report has missing probability responses')
    scope, scope_binding = report_scope(path, value, source, scopes, resume_records or {})
    summary = {'sha256': file_hash(path), 'bytes': path.stat().st_size, 'sourceVersion': source,
               'mode': value.get('mode'), 'probabilityComplete': complete,
               'requestedJobs': len(jobs), 'jobStatuses': dict(statuses), 'uniqueProgramRecords': len(programs),
               'successfulProgramIdentities': len(successful),
               'jobMetadataCanonicalBytes': len(pipeline.canonical(jobs)),
               'responsePayloadCanonicalBytes': sum(len(pipeline.canonical(row['response'])) for row in programs if 'response' in row),
               'scope': scope, 'scopeBinding': scope_binding, 'sharedFingerprint': value.get('sharedFingerprint'),
               'nativeStats': value.get('stats', {})}
    return summary, successful, set(fingerprints), references


def tar_member(archive, name, size, stream):
    info = tarfile.TarInfo(name)
    info.size, info.mode, info.mtime, info.uid, info.gid = size, 0o644, 0, 0, 0
    info.uname = info.gname = ''
    archive.addfile(info, stream)


def compression(index_raw, blobs, methods, retain=None):
    """Measure the actual deterministic index+blob tar, without concatenating RAM buffers."""
    with tempfile.TemporaryDirectory(prefix='luck-program-bundle-') as temporary:
        temporary = Path(temporary); raw_tar = temporary / 'dictionary.tar'
        with tarfile.open(raw_tar, 'w', format=tarfile.USTAR_FORMAT) as tar:
            tar_member(tar, 'program-index.json', len(index_raw), io.BytesIO(index_raw))
            for digest, path in sorted(blobs.items()):
                with path.open('rb') as stream:
                    tar_member(tar, 'blobs/' + digest + '.onlrsp', path.stat().st_size, stream)
        measured = {'tar': {'bytes': raw_tar.stat().st_size, 'sha256': file_hash(raw_tar)}}
        if retain:
            Path(retain).mkdir(parents=True, exist_ok=True)
        for method in methods:
            if method == 'zstd9' and zstandard is None:
                measured[method] = {'available': False, 'reason': 'optional zstandard package is not installed'}
                continue
            extension = {'gzip9': '.tar.gz', 'xz6': '.tar.xz', 'zstd9': '.tar.zst'}[method]
            target = temporary / ('dictionary' + extension); start = time.monotonic()
            with raw_tar.open('rb') as source, target.open('wb') as destination:
                if method == 'gzip9':
                    with gzip.GzipFile(filename='', fileobj=destination, mode='wb', compresslevel=9, mtime=0) as codec:
                        shutil.copyfileobj(source, codec, CHUNK)
                elif method == 'xz6':
                    with lzma.LZMAFile(destination, 'wb', preset=6) as codec:
                        shutil.copyfileobj(source, codec, CHUNK)
                else:
                    compressor = zstandard.ZstdCompressor(level=9, threads=0)
                    compressor.copy_stream(source, destination)
            result = {'available': True, 'bytes': target.stat().st_size, 'sha256': file_hash(target),
                      'elapsedMs': (time.monotonic() - start) * 1000}
            if retain:
                destination = Path(retain) / target.name
                shutil.copyfile(target, destination)
                result['artifact'] = destination.name
            measured[method] = result
        if retain:
            pipeline.atomic_bytes(Path(retain) / 'program-index.json', index_raw)
        return measured


def analyze(roots, reports=(), inputs=(), methods=('gzip9', 'xz6'), bundle_dir=None):
    roots = [Path(root).resolve() for root in roots]
    if not roots and not reports:
        raise ValueError('provide at least one results root or explicit native report')
    if not set(methods) <= {'gzip9', 'xz6', 'zstd9'} or len(set(methods)) != len(methods):
        raise ValueError('unknown or repeated compression method')
    input_roots = [Path(root).resolve() for root in inputs]
    if bundle_dir:
        destination = Path(bundle_dir).resolve()
        protected = roots + input_roots + [Path(path).resolve().parent for path in reports]
        if any(destination == path or destination.is_relative_to(path) or path.is_relative_to(destination)
               for path in protected):
            raise ValueError('bundle output must be separate from input/report roots')
    scopes, resume_bindings = input_scopes(input_roots, with_resume=True)
    resume_records, errors = semantic_scope_records(roots, resume_bindings)
    report_paths = {Path(path).resolve() for path in reports}
    index_paths = set()
    for root in roots:
        index_paths.update(root.rglob('program-index.json'))
        for name in ('programs.json', 'missing.json'):
            report_paths.update(root.rglob(name))
    report_records, report_by_hash = [], {}
    if not report_paths and not index_paths:
        errors.append({'path': '.', 'error': 'no native program reports or dictionary indexes were found'})
    program_occurrences = defaultdict(lambda: {'reports': set(), 'charts': set(), 'regions': set(), 'contexts': set()})
    for path in sorted(report_paths):
        try:
            summary, successful, declared, references = report_summary(path, scopes, resume_records)
            summary['path'] = label(path, roots)
            report_records.append(summary)
            report_by_hash[summary['sha256']] = (summary, successful, declared)
            for fingerprint in successful:
                occurrence = program_occurrences[(summary['sourceVersion'], fingerprint)]
                occurrence['reports'].add(summary['sha256'])
                scope = summary['scope']
                if scope.get('scoreId') is not None:
                    occurrence['charts'].add((scope.get('datasetSha256'), scope['scoreId']))
                if scope.get('region'):
                    occurrence['regions'].add(scope['region'])
                if summary['sharedFingerprint']:
                    occurrence['contexts'].add(summary['sharedFingerprint'])
        except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
            errors.append({'path': label(path, roots), 'error': str(error)})
    groups, index_records = {}, []
    blob_files = {}
    for path in sorted(index_paths):
        try:
            index = read_json(path)
            source = store.digest(index['sourceVersion']); mode = index['mode']
            if index.get('format') != store.INDEX or mode not in store.MODES:
                raise ValueError('unknown program dictionary format or mode')
            rows = store.load_index(path, source, mode)
            matching = report_by_hash.get(index.get('inputReportSha256'))
            complete = index.get('complete') is True
            if matching:
                summary, successful, declared = matching
                if summary['sourceVersion'] != source or set(rows) != declared:
                    raise ValueError('packed index coverage/source differs from its exact native report')
                if index.get('requestedJobs') != summary['requestedJobs']:
                    raise ValueError('packed index requested job count differs from its exact native report')
                if complete and not summary['probabilityComplete']:
                    raise ValueError('complete index references an unfinished probability report')
            elif index.get('inputReportSha256'):
                raise ValueError('referenced native report is missing; supply --report for nonstandard filenames')
            reference_bytes = 0
            group = groups.setdefault((source, mode), {})
            for fingerprint, row in rows.items():
                if row.get('status') != 'success':
                    if complete:
                        raise ValueError('complete index contains an unavailable program')
                    continue
                blob, raw = store.verified_blob(path.parent, row, source, mode)
                if not blob.resolve().is_relative_to(path.parent.resolve()):
                    raise ValueError('program blob symlink escapes its dictionary directory')
                digest = row['archive']['sha256']; size = len(raw); del raw
                archive = {'path': 'blobs/' + digest + '.onlrsp', 'sha256': digest, 'bytes': size,
                           'mode': mode, 'verifiedEntries': 1}
                entry = {'fingerprint': fingerprint, 'sourceVersion': source, 'status': 'success', 'archive': archive}
                if fingerprint in group and group[fingerprint]['entry'] != entry:
                    raise ValueError('same source/mode/program fingerprint has conflicting archive bytes')
                group.setdefault(fingerprint, {'entry': entry, 'blob': blob.resolve()})
                reference_bytes += size; blob_files[blob.resolve()] = size
            index_records.append({'path': label(path, roots), 'sha256': file_hash(path), 'bytes': path.stat().st_size,
                                  'sourceVersion': source, 'mode': mode, 'complete': complete,
                                  'coverageKind': 'packedNativeReport' if matching else 'openDictionary',
                                  'inputReportSha256': index.get('inputReportSha256'),
                                  'programStatuses': dict(Counter(row.get('status') for row in rows.values())),
                                  'archiveReferenceBytes': reference_bytes})
        except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
            errors.append({'path': label(path, roots), 'error': str(error)})
    dictionaries = []
    for (source, mode), entries in sorted(groups.items()):
        rows = [entries[fp]['entry'] for fp in sorted(entries)]
        blobs = {row['archive']['sha256']: entries[row['fingerprint']]['blob'] for row in rows}
        index = {'format': store.INDEX, 'sourceVersion': source, 'mode': mode, 'programs': rows,
                 'complete': False, 'completionMeaning': 'Open dictionary of measured native programs; no global combination alias index.',
                 'nativeExpectationProven': False, 'rankingProven': False}
        raw = pipeline.canonical(index) + b'\n'
        bytes_by_program = [row['archive']['bytes'] for row in rows]
        dictionary = {'sourceVersion': source, 'mode': mode, 'uniquePrograms': len(rows),
                      'indexBytes': len(raw), 'indexSha256': pipeline.sha(raw), 'uniqueBlobs': len(blobs),
                      'blobBytes': sum(path.stat().st_size for path in blobs.values()),
                      'perProgramArchiveBytes': [{'fingerprint': row['fingerprint'], 'bytes': row['archive']['bytes'],
                                                  'sha256': row['archive']['sha256']} for row in rows],
                      'minimumProgramBytes': min(bytes_by_program, default=0),
                      'maximumProgramBytes': max(bytes_by_program, default=0)}
        dictionary['indexAndBlobBytes'] = dictionary['indexBytes'] + dictionary['blobBytes']
        if errors:
            dictionary['compression'] = {'skipped': 'integrity errors prevent publication of a combined dictionary'}
        else:
            retain = Path(bundle_dir) / source / mode if bundle_dir else None
            dictionary['compression'] = compression(raw, blobs, methods, retain)
        dictionaries.append(dictionary)
    unique_reports = list({row['sha256']: row for row in report_records}.values())
    sharing = [{'sourceVersion': source, 'fingerprint': fingerprint,
                'completedReports': len(value['reports']), 'distinctChartDatasetPairs': len(value['charts']),
                'knownRegions': sorted(value['regions']), 'distinctNativeContexts': len(value['contexts'])}
               for (source, fingerprint), value in sorted(program_occurrences.items())]
    return {'format': FORMAT, 'valid': not errors, 'errors': errors,
            'scope': 'Observed native program reports and dictionary artifacts only; not a complete game or all-combination coverage claim.',
            'nativeExpectationProven': False, 'rankingProven': False, 'globalCombinationAliasIndex': False,
            'inputMetadata': {'reportFiles': len(report_records), 'uniqueReportContents': len(unique_reports),
                              'reportFileBytes': sum(row['bytes'] for row in report_records),
                              'requestedJobsAcrossUniqueReports': sum(row['requestedJobs'] for row in unique_reports),
                              'canonicalJobMetadataBytes': sum(row['jobMetadataCanonicalBytes'] for row in unique_reports),
                              'indexFileBytes': sum(row['bytes'] for row in index_records),
                              'allObservedIndexesComplete': bool(index_records) and all(row['complete'] for row in index_records),
                              'physicalBlobFiles': len(blob_files), 'physicalBlobFileBytes': sum(blob_files.values()),
                              'allObservedProbabilityRunsComplete': bool(unique_reports) and all(row['probabilityComplete'] for row in unique_reports)},
            'reports': report_records, 'indexes': index_records, 'deployableDictionaries': dictionaries,
            'sharing': {'uniqueCompleteProgramIdentities': len(sharing),
                        'sharedAcrossKnownCharts': sum(row['distinctChartDatasetPairs'] > 1 for row in sharing),
                        'sharedAcrossKnownRegions': sum(len(row['knownRegions']) > 1 for row in sharing),
                        'unknownScopeReports': sum(row['scope']['scoreId'] is None for row in unique_reports),
                        'programs': sharing},
            'compressionProtocol': {'bundle': 'USTAR; index first, blob SHA order; mode0644, uid/gid/mtime0, empty owner names',
                                    'gzip9': 'level9; filename empty; mtime0', 'xz6': 'XZ preset6',
                                    'zstd9': 'optional zstandard; level9, single thread'}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, action='append', default=[], help='recursive result root; standard programs.json/missing.json and all program-index.json')
    parser.add_argument('--report', type=Path, action='append', default=[], help='additional arbitrary-named native interaction report')
    parser.add_argument('--inputs', type=Path, action='append', default=[], help='pinned catalogue/benchmark inputs for hash-bound chart and region labels')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--bundle-dir', type=Path, help='optionally retain deterministic compressed deployable dictionaries')
    parser.add_argument('--compression', default='gzip9,xz6', help='comma-separated gzip9,xz6,zstd9; empty skips compression')
    args = parser.parse_args()
    result = analyze(args.root, args.report, args.inputs, tuple(filter(None, args.compression.split(','))), args.bundle_dir)
    pipeline.write(args.output, result)
    print(json.dumps({'valid': result['valid'], 'errors': len(result['errors']),
                      'reports': result['inputMetadata']['uniqueReportContents'],
                      'dictionaries': len(result['deployableDictionaries'])}))
    return 0 if result['valid'] else 1


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f'luck program analysis: {error}')
