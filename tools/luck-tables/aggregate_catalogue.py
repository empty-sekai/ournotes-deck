#!/usr/bin/env python3
"""Pin the shared CI bundle and audit every whole-catalogue shard and blob."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location("luck_aggregate_runner", Path(__file__).with_name("stream_runner.py"))
runner = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(runner)
pipeline, catalogue = runner.pipeline, runner.catalogue
BUNDLE_FORMAT = "ournotes-deck.luck-catalogue-bundle/1"
AGGREGATE_FORMAT = "ournotes-deck.luck-catalogue-aggregate/1"


def inputs_identity(inputs):
    inputs = Path(inputs).resolve()
    files = {path.relative_to(inputs).as_posix(): pipeline.sha(path.read_bytes())
             for path in sorted(inputs.rglob('*')) if path.is_file()}
    if not files:
        raise ValueError('empty catalogue input bundle')
    return files


def make_bundle(inputs, generator, source, output):
    bundle = {'format': BUNDLE_FORMAT, 'files': inputs_identity(inputs),
              'generatorSha256': pipeline.sha(Path(generator).read_bytes()),
              'algorithm': pipeline.source_identity(source)}
    pipeline.write(output, bundle)
    return bundle


def verify_bundle(bundle_path, inputs, source, generator=None):
    bundle = pipeline.read(bundle_path)
    if bundle.get('format') != BUNDLE_FORMAT or bundle.get('files') != inputs_identity(inputs):
        raise ValueError('shared input bundle was changed or incompletely restored')
    if bundle.get('algorithm') != pipeline.source_identity(source):
        raise ValueError('checkout algorithm source differs from the single build bundle')
    if generator is not None and bundle.get('generatorSha256') != pipeline.sha(Path(generator).read_bytes()):
        raise ValueError('restored generator differs from the single build bundle')
    return bundle


def aggregate(inputs, results, bundle_path, source, output, shard_count=8, regions=('tw', 'jp')):
    if type(shard_count) is not int or not 1 <= shard_count <= 1024:
        raise ValueError('invalid expected shard count')
    inputs, results, output = Path(inputs).resolve(), Path(results).resolve(), Path(output).resolve()
    bundle = verify_bundle(bundle_path, inputs, source)
    result = {'format': AGGREGATE_FORMAT, 'bundleSha256': pipeline.sha(Path(bundle_path).read_bytes()),
              'algorithm': bundle['algorithm'], 'generatorSha256': bundle['generatorSha256'],
              'complete': False, 'regions': [], 'errors': [], 'charts': [],
              'completionMeaning': 'Every master chart/difficulty in both pinned published regional catalogues is accounted for; every natural LUCK chart has the complete native base/single source-level catalogue and four verified program archives. All team combinations and arbitrary play profiles are not pre-enumerated.'}
    errors = result['errors']
    found = []
    for path in sorted(results.rglob('manifest.json')):
        try:
            value = pipeline.read(path)
            if value.get('format') == runner.FORMAT:
                found.append((path, value))
        except (OSError, ValueError, TypeError) as error:
            errors.append(f'{path.relative_to(results)}: unreadable manifest: {error}')
    consumed = set()
    for region in regions:
        benchmark = inputs / region / 'inputs/benchmark.json'
        manifest = pipeline.read(benchmark)
        cases = {case['name']: case for case in manifest['cases']}
        data_path = benchmark.parent / next(iter(cases.values()))['data']
        if pipeline.sha(data_path.read_bytes()) != manifest['datasetId']:
            raise ValueError(f'{region}: shared manifest dataset pin differs from actual bytes')
        data = pipeline.read(data_path)
        inv = catalogue.inventory(data, manifest['datasetId'], region)
        master = {row['scoreId']: row for row in inv['charts']}
        by_score = {pipeline.read(benchmark.parent / case['request'])['execution']['scoreId']: case
                    for case in cases.values()}
        if len(cases) != len(by_score) or set(by_score) != set(master):
            raise ValueError(f'{region}: pinned manifest is not the complete master score set')
        keys = inv['luckCatalogueKeys']; jobs = pipeline.base_jobs(keys)
        benchmark_sha = pipeline.sha(benchmark.read_bytes())
        matching = [(path, value) for path, value in found if value.get('benchmarkSha256') == benchmark_sha]
        shard_indices, seen = set(), set()
        region_rows = []
        for path, shard in matching:
            consumed.add(path)
            prefix = f'{region}/{path.relative_to(results)}'
            metadata = shard.get('shard', {})
            index = metadata.get('index')
            if (type(index) is not int or not 0 <= index < shard_count or index in shard_indices
                    or metadata.get('count') != shard_count or metadata.get('ordering') != 'ascending-score-id-modulo'
                    or metadata.get('catalogueCharts') != len(master)):
                errors.append(prefix + ': duplicate, unknown or inconsistent shard identity')
                continue
            shard_indices.add(index)
            expected = {sid for ordinal, sid in enumerate(sorted(master)) if ordinal % shard_count == index}
            rows = shard.get('charts', [])
            if (len(rows) != len(expected) or {r.get('scoreId') for r in rows} != expected
                    or metadata.get('selectedCharts') != len(expected)):
                errors.append(prefix + ': selected chart coverage differs from the exact modulo partition')
            if (shard.get('algorithm') != bundle['algorithm'] or shard.get('generatorSha256') != bundle['generatorSha256']
                    or shard.get('datasetId') != manifest['datasetId'] or shard.get('backend') != 'programs'):
                errors.append(prefix + ': source, binary, dataset or backend differs')
            if shard.get('complete') is not True:
                errors.append(prefix + ': shard is incomplete')
            for row in rows:
                sid = row.get('scoreId')
                if sid not in expected or sid in seen:
                    errors.append(prefix + f': unexpected/duplicate chart {sid}')
                    continue
                seen.add(sid)
                chart, case = master[sid], by_score[sid]
                summary = {**row, 'region': region, 'shardIndex': index,
                           'shardManifest': path.relative_to(results).as_posix()}
                try:
                    if row.get('name') != case['name'] or row.get('difficulty') != chart['difficulty'] or row.get('missions') != chart['missions']:
                        raise ValueError('chart identity/difficulty/missions differ')
                    hashes = {field: pipeline.sha((benchmark.parent / case[field]).read_bytes())
                              for field in ('data', 'snapshot', 'request')}
                    if row.get('inputSha256') != hashes:
                        raise ValueError('actual data/snapshot/request bytes differ from the shared pin')
                    if chart['hasLuckMission']:
                        if (row.get('status') != 'success' or row.get('jobs') != len(jobs)
                                or row.get('nativeCatalogueValidated') is not True
                                or row.get('sourceVersion') != bundle['algorithm']['simSourceSha256']):
                            raise ValueError('natural LUCK chart is not fully completed under the same native source')
                        directory = (path.parent / row['directory']).resolve()
                        if not directory.is_relative_to(path.parent.resolve()) or not runner.reusable_programs(directory, row):
                            raise ValueError('missing, corrupt, incomplete or escaping program archives')
                        report = pipeline.read(runner.verified_file(directory, row['report']))
                        fingerprints = runner.validate_program_report(report, jobs, keys, bundle['algorithm']['simSourceSha256'])
                        if fingerprints != row['programFingerprints']:
                            raise ValueError('job/program mapping differs from chart receipt')
                    elif (row.get('status') != 'notApplicable' or row.get('jobs') != 0 or row.get('nativeCalls') != 0):
                        raise ValueError('non-LUCK chart must remain explicitly notApplicable')
                    summary['auditPassed'] = True
                except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
                    summary['auditPassed'] = False
                    errors.append(prefix + f': chart {sid}: {error}')
                region_rows.append(summary)
        if shard_indices != set(range(shard_count)):
            errors.append(f'{region}: missing shard indices {sorted(set(range(shard_count))-shard_indices)}')
        if seen != set(master):
            errors.append(f'{region}: missing master charts {sorted(set(master)-seen)}')
        result['regions'].append({'region': region, 'datasetId': manifest['datasetId'],
                                  'benchmarkSha256': benchmark_sha, 'expected': inv['counts'],
                                  'receivedShards': sorted(shard_indices), 'receivedCharts': len(seen)})
        result['charts'].extend(sorted(region_rows, key=lambda row: row['scoreId']))
    for path, _value in found:
        if path not in consumed:
            errors.append(f'unexpected manifest from another input pin: {path.relative_to(results)}')
    result['complete'] = not errors
    result['summary'] = {'declaredCharts': sum(r['expected']['charts'] for r in result['regions']),
                         'receivedCharts': len(result['charts']),
                         'naturalLuckCharts': sum(r['expected']['luckCharts'] for r in result['regions']),
                         'success': sum(r['status'] == 'success' and r['auditPassed'] for r in result['charts']),
                         'notApplicable': sum(r['status'] == 'notApplicable' and r['auditPassed'] for r in result['charts']),
                         'errors': len(errors)}
    pipeline.write(output / 'aggregate.json', result)
    lines = ['# Whole-catalogue LUCK response generation', '',
             f"Complete: **{str(result['complete']).lower()}**. All input pins, source identities, native job labels and program blob hashes were checked.", '',
             '| Region | Declared charts | Natural LUCK | Received charts | Shards |', '|---|---:|---:|---:|---:|']
    for row in result['regions']:
        lines.append(f"| {row['region']} | {row['expected']['charts']} | {row['expected']['luckCharts']} | {row['receivedCharts']} | {len(row['receivedShards'])}/{shard_count} |")
    lines.extend(['', result['completionMeaning'], ''])
    if errors:
        lines.extend(['Errors:', ''] + ['- ' + error for error in errors])
    pipeline.atomic_bytes(output / 'summary.md', ('\n'.join(lines) + '\n').encode())
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    for command in ('bundle', 'verify-bundle'):
        p = commands.add_parser(command); p.add_argument('--inputs', type=Path, required=True)
        p.add_argument('--generator', type=Path, required=True); p.add_argument('--source', type=Path, required=True)
        p.add_argument('--bundle', type=Path, required=True)
    p = commands.add_parser('aggregate')
    p.add_argument('--inputs', type=Path, required=True); p.add_argument('--results', type=Path, required=True)
    p.add_argument('--bundle', type=Path, required=True); p.add_argument('--source', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True); p.add_argument('--shard-count', type=int, default=8)
    args = parser.parse_args()
    if args.command == 'bundle':
        make_bundle(args.inputs, args.generator, args.source, args.bundle)
    elif args.command == 'verify-bundle':
        verify_bundle(args.bundle, args.inputs, args.source, args.generator)
    else:
        result = aggregate(args.inputs, args.results, args.bundle, args.source, args.output, args.shard_count)
        print(json.dumps({'complete': result['complete'], **result['summary']}))
        return 0 if result['complete'] else 1
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f'luck catalogue audit: {error}', file=sys.stderr)
        sys.exit(2)
