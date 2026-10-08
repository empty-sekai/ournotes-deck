#!/usr/bin/env python3
"""Measure minimum-skill interaction reuse on unchanged, declared real charts.

The skill specifications come from the supplied published master data. These
are isolated master-skill kernels, not owned-deck search benchmarks. A large
identification grid, independent native-DP comparisons, reusable query subsets
and a changed-nonminimum control have separate evidence and work counts.
"""
from __future__ import annotations

import argparse
from fractions import Fraction
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import subprocess
import sys
import time


def module(name):
    # The spec builder also uses its adjacent catalogue and operator modules.
    directory = str(Path(__file__).resolve().parent)
    inserted = directory not in sys.path
    if inserted:
        sys.path.insert(0, directory)
    try:
        spec = importlib.util.spec_from_file_location('luck_minimum_validation_' + name, Path(directory) / (name + '.py'))
        value = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(value)
        return value
    finally:
        if inserted:
            sys.path.remove(directory)


basis, recipes = module('basis'), module('make_minimum_basis_specs')
pipeline, storage = basis.pipeline, basis.storage
FORMAT = 'ournotes-deck.luck-minimum-basis-validation/1'


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def receipt(path, root):
    path = Path(path)
    return {'path': path.resolve().relative_to(Path(root).resolve()).as_posix(),
            'sha256': file_hash(path), 'bytes': path.stat().st_size}


def selected_cases(path, names):
    manifest = pipeline.read(path)
    if manifest.get('format') != 'ournotes-deck.search-benchmark/1':
        raise ValueError('provide an existing declared benchmark manifest')
    dataset = storage.digest(manifest.get('datasetId'))
    cases = manifest.get('cases')
    if not isinstance(cases, list) or len({case['name'] for case in cases}) != len(cases):
        raise ValueError('manifest has missing or repeated case identities')
    if not names or len(set(names)) != len(names):
        raise ValueError('select one or more distinct original case names')
    by_name = {case['name']: case for case in cases}
    if not set(names) <= set(by_name):
        raise ValueError('a selected case is absent from the original manifest')
    return dataset, [by_name[name] for name in names]


def identify(report, spec, hashes, spec_sha, expected_source=None):
    source, programs, mappings, references = basis.identification(report, spec['jobs'], 64)
    basis.provenance(report, hashes, spec_sha)
    stats = report['stats']
    if (expected_source is not None and source != expected_source
            or stats.get('requestedJobs') != len(spec['jobs'])
            or stats.get('compiledJobs') != len(spec['jobs'])
            or stats.get('uniquePrograms') != len(programs)
            or stats.get('reconstructedJobs') != 0 or len(programs) > 64):
        raise ValueError('identification source, complete job count or bounded program count differs')
    return source, {program['fingerprint'] for program in programs}, references


def verification(report, spec, hashes, spec_sha, source):
    """Require the six actual original-DP comparisons, not just inferred term equivalence."""
    basis.provenance(report, hashes, spec_sha)
    jobs = spec['jobs']
    if (len(jobs) != 6 or spec.get('verifyBasis') is not True
            or report.get('format') != basis.REPORT or report.get('mode') != 'basisPrograms'
            or report.get('operatorContract') != basis.CONTRACT or report.get('sourceVersion') != source
            or any(report.get(field) is not True for field in
                   ('complete', 'identificationComplete', 'probabilityComplete', 'verificationRequested', 'verificationComplete'))
            or report.get('missingSelectedPrograms') != []):
        raise ValueError('six original-DP comparisons did not complete on the identified source')
    context = report.get('context', {})
    if context.get('algorithmVersion') != 'ournotes-luck-response/1/' + source:
        raise ValueError('verification response context uses a different source')
    storage.digest(context.get('fingerprint'))
    programs = report.get('programs', [])
    if not 1 <= len(programs) <= 64:
        raise ValueError('verification has no conditional programs or exceeds the fixed-tape bound')
    fingerprints = []
    for index, program in enumerate(programs):
        fingerprints.append(storage.digest(program.get('fingerprint')))
        if (program.get('programIndex') != index or program.get('sourceVersion') != source
                or program.get('operatorContract') != basis.CONTRACT or program.get('status') != 'success'
                or program.get('response', {}).get('status') != 'success'):
            raise ValueError('verification contains an unsuccessful conditional response')
    if len(set(fingerprints)) != len(fingerprints):
        raise ValueError('verification repeats a conditional program identity')
    rows = pipeline.job_results(report, jobs)
    referenced, comparisons = set(), []
    for ordinal, job in enumerate(jobs):
        row = rows[job['name']]
        check = row.get('verification', {})
        components = row.get('components', [])
        if (row.get('jobIndex') != ordinal or row.get('status') != 'success'
                or row.get('response', {}).get('status') != 'success'
                or not 1 <= len(components) <= 64 or row.get('termCount') != len(components)
                or check.get('status') != 'success' or check.get('disjointIntervals') != 0
                or type(check.get('comparedBuckets')) is not int or check['comparedBuckets'] <= 0
                or check.get('probesEqual') is not True or check.get('probeTransitionsEqual') is not True):
            raise ValueError('a requested kernel lacks a successful independent original-DP comparison')
        total_lower, total_upper = Fraction(0), Fraction(0)
        for component in components:
            index, weight = component.get('programIndex'), component.get('weight')
            if (type(index) is not int or not 0 <= index < len(programs)
                    or component.get('programFingerprint') != fingerprints[index]
                    or not isinstance(weight, list) or len(weight) != 2
                    or any(type(value) not in (int, float) or not 0 <= value <= 1 for value in weight)
                    or not 0 <= weight[0] <= weight[1] <= 1 or weight[1] == 0):
                raise ValueError('verification conditional mapping or positive weights differ')
            referenced.add(index)
            total_lower += Fraction(weight[0]); total_upper += Fraction(weight[1])
        if not total_lower <= 1 <= total_upper:
            raise ValueError('verification conditional weights do not enclose unit mass')
        difference = check.get('maximumEndpointDifference')
        if type(difference) not in (int, float) or not math.isfinite(difference) or difference < 0:
            raise ValueError('verification omitted its measured endpoint difference')
        comparisons.append({'name': job['name'], **check})
    stats = report.get('stats', {})
    if (referenced != set(range(len(programs))) or stats.get('requestedJobs') != 6
            or stats.get('compiledJobs') != 6 or stats.get('verificationCalls') != 6
            or stats.get('verifiedJobs') != 6 or stats.get('reconstructedJobs') != 6
            or stats.get('uniquePrograms') != len(programs) or stats.get('propagationCalls') != len(programs)):
        raise ValueError('verification work counters do not cover all six original comparisons and their terms')
    return {'verifiedJobs': 6, 'disjointIntervals': sum(check['disjointIntervals'] for check in comparisons),
            'comparedBuckets': sum(check['comparedBuckets'] for check in comparisons),
            'maximumEndpointDifference': max(check['maximumEndpointDifference'] for check in comparisons),
            'uniquePrograms': len(programs), 'comparisons': comparisons, 'nativeStats': stats}


def query_counts(result, jobs, hashes, spec_sha, source, complete=True):
    expected = {**hashes, 'spec': spec_sha}
    if (result.get('format') != basis.QUERY or result.get('sourceVersion') != source
            or result.get('inputSha256') != expected or result.get('inputsUnchanged') is not True
            or result.get('complete') is not complete or result.get('status') != ('success' if complete else 'missing')):
        raise ValueError('conditional query did not complete with its exact original inputs and expected missing status')
    stats = result.get('stats', {})
    for field in ('requestedJobs', 'conditionalTermReferences', 'uniqueBasisPrograms', 'reusedPrograms',
                  'generatedPrograms', 'propagationCalls', 'storedPrograms'):
        if type(stats.get(field)) is not int or stats[field] < 0:
            raise ValueError('conditional query omitted exact nonnegative work counts')
    if stats['requestedJobs'] != len(jobs) or not 1 <= stats['uniqueBasisPrograms'] <= 64:
        raise ValueError('conditional query did not identify every requested kernel within the fixed-tape bound')
    return stats


def require_reuse(stats, count):
    if (stats['generatedPrograms'] != 0 or stats['propagationCalls'] != 0
            or stats['reusedPrograms'] != stats['uniqueBasisPrograms'] or stats['storedPrograms'] != count):
        raise ValueError('unseen minimum levels or combinations required new conditional DP programs')


def run_case(case, paths, generator, directory, mode, hashes, data, timeout):
    directory.mkdir(parents=True)
    result = {'name': case['name'], 'complete': False, 'status': 'running', 'inputSha256': hashes,
              'phases': {}, 'nativeExpectationProven': False, 'rankingProven': False}
    started = time.monotonic()
    try:
        request = pipeline.read(paths['request'])
        if not pipeline.luck_case(data, request):
            raise ValueError('selected original request has no natural LUCK mission; no mission is replaced')
        result.update(scoreId=request['execution']['scoreId'], musicId=request['scenario']['musicId'])
        files, selected, supports = recipes.make_specs(data)
        if {name: len(value['jobs']) for name, value in files.items()} != {
                'grid-identify.json': 1250, 'warmup.json': 2, 'unseen-levels.json': 12,
                'new-combinations.json': 6, 'verification.json': 6, 'nonminimum-control.json': 1}:
            raise ValueError('real master recipe no longer has the declared complete experiment scope')
        spec_files = {}
        for name, spec in files.items():
            path = directory / 'specs' / name
            pipeline.write(path, spec)
            spec_files[name] = path
        pipeline.write(directory / 'specs/receipt.json', {
            'format': 'ournotes-deck.minimum-basis-specs/1', 'dataSha256': hashes['data'],
            'scope': 'Isolated actual master skills at original holder positions; not owned-deck search cases.',
            'selectedKeys': selected, 'supportOperatorsAtDeclaredPhaseLife1000': supports,
            'files': {name: {**receipt(path, directory / 'specs'), 'jobs': len(files[name]['jobs'])}
                      for name, path in spec_files.items()},
            'sourceSha256': {name: file_hash(Path(__file__).parent / name) for name in
                            ('make_minimum_basis_specs.py', 'catalogue.py', 'analyze_start_operators.py')}})
        base = [generator, paths['data'], paths['snapshot'], paths['request']]

        def native(name, spec_name):
            spec_path = spec_files[spec_name]
            output = directory / (name + '.json')
            began = time.monotonic()
            storage.call(base + [spec_path, output], directory / (name + '.log'), timeout)
            phase = {'report': receipt(output, directory), 'spec': receipt(spec_path, directory),
                     'wallMs': (time.monotonic() - began) * 1000}
            result['phases'][name] = phase
            return pipeline.read(output), phase

        grid, phase = native('grid-identify', 'grid-identify.json')
        source, grid_programs, references = identify(grid, files['grid-identify.json'], hashes,
                                                    file_hash(spec_files['grid-identify.json']))
        result['sourceVersion'] = source
        phase.update(requestedJobs=1250, identifiedJobs=1250, uniquePrograms=len(grid_programs),
                     conditionalTermReferences=references, propagationCalls=0, nativeStats=grid['stats'])
        checked, phase = native('verification', 'verification.json')
        phase.update(verification(checked, files['verification.json'], hashes,
                                  file_hash(spec_files['verification.json']), source))
        if (checked.get('sharedFingerprint') != grid.get('sharedFingerprint')
                or checked.get('dependencyDescriptor') != grid.get('dependencyDescriptor')):
            raise ValueError('verification changed the original resolved real-chart context')

        def query(name, spec_name, generate):
            output = directory / name
            before = time.monotonic()
            value = basis.query(paths['data'], paths['snapshot'], paths['request'], spec_files[spec_name],
                                generator, directory / 'store', output, mode=mode, generate=generate, timeout=timeout)
            phase = {'report': receipt(output / 'query.json', directory), 'spec': receipt(spec_files[spec_name], directory),
                     'wallMs': (time.monotonic() - before) * 1000, 'queryElapsedMs': value.get('elapsedMs'),
                     'status': value.get('status'), 'stats': value.get('stats')}
            result['phases'][name] = phase
            return value, phase

        warmup, phase = query('warmup', 'warmup.json', True)
        count = query_counts(warmup, files['warmup.json']['jobs'], hashes, file_hash(spec_files['warmup.json']), source)
        stored = count['storedPrograms']
        if (count['reusedPrograms'] != 0 or not count['generatedPrograms'] == count['propagationCalls']
                == count['uniqueBasisPrograms'] == stored):
            raise ValueError('empty-dictionary warmup did not generate exactly its distinct conditional programs')
        index_path = directory / 'store' / source / mode / 'program-index.json'
        warm_programs = set(storage.load_index(index_path, source, mode))
        if warm_programs != grid_programs:
            raise ValueError('two warmup kernels did not cover the complete 1,250-kernel identified program set')
        phase.update(coversAllGridPrograms=True, storedPrograms=stored)
        archives = {}
        for name, spec_name in (('unseen-levels', 'unseen-levels.json'), ('new-combinations', 'new-combinations.json'),
                                ('warm-repeat', 'unseen-levels.json')):
            value, phase = query(name, spec_name, False)
            stats = query_counts(value, files[spec_name]['jobs'], hashes, file_hash(spec_files[spec_name]), source)
            require_reuse(stats, stored)
            if set(storage.load_index(index_path, source, mode)) != warm_programs:
                raise ValueError('minimum-only queries changed the warmed program dictionary')
            phase['archive'] = value['archive']
            archives[name] = value['archive']['sha256']
        if archives['unseen-levels'] != archives['warm-repeat']:
            raise ValueError('repeating identical requested minimum mixtures changed their archive bytes')
        result['sameRepeatedQueryArchiveBytes'] = True

        value, phase = query('control-missing', 'nonminimum-control.json', False)
        stats = query_counts(value, files['nonminimum-control.json']['jobs'], hashes,
                             file_hash(spec_files['nonminimum-control.json']), source, complete=False)
        original_spec = pipeline.read(directory / 'control-missing/identify-spec.json')
        control = pipeline.read(directory / 'control-missing/identify.json')
        _, control_programs, _ = identify(control, original_spec, hashes,
                                         file_hash(directory / 'control-missing/identify-spec.json'), source)
        missing = control_programs - warm_programs
        if (control_programs == warm_programs or not missing or set(value.get('missingPrograms', [])) != missing
                or stats['propagationCalls'] != 0 or stats['generatedPrograms'] != 0 or stats['storedPrograms'] != stored
                or stats['reusedPrograms'] != len(control_programs & warm_programs)):
            raise ValueError('changed nonminimum tape did not expose exactly the new native program identities')
        phase.update(newMissingPrograms=len(missing), changedNativeProgramSet=True)
        value, phase = query('control-generated', 'nonminimum-control.json', True)
        stats = query_counts(value, files['nonminimum-control.json']['jobs'], hashes,
                             file_hash(spec_files['nonminimum-control.json']), source)
        if (stats['generatedPrograms'] != len(missing) or stats['propagationCalls'] != len(missing)
                or stats['reusedPrograms'] != len(control_programs & warm_programs)
                or stats['storedPrograms'] != stored + len(missing)
                or set(storage.load_index(index_path, source, mode)) != warm_programs | control_programs):
            raise ValueError('nonminimum control did not propagate precisely its new conditional programs')
        phase['newMissingPrograms'] = len(missing)
        result.update(complete=True, status='success', summary={
            'gridJobs': 1250, 'uniqueGridPrograms': len(grid_programs), 'verifiedJobs': 6,
            'verificationDisjointIntervals': 0, 'warmupJobs': 2, 'warmupPrograms': stored,
            'unseenLevelJobs': 12, 'newCombinationJobs': 6, 'repeatedJobs': 12,
            'minimumOnlyQueryPropagationCalls': 0, 'nonminimumControlJobs': 1,
            'nonminimumNewPrograms': len(missing), 'nonminimumPropagationCalls': stats['propagationCalls']})
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(complete=False, status='error', error=pipeline.error_text(error))
    finally:
        try:
            result['inputsUnchanged'] = all(file_hash(path) == hashes[name] for name, path in paths.items())
        except OSError:
            result['inputsUnchanged'] = False
        if not result['inputsUnchanged']:
            result.update(complete=False, status='error', error='an original case input changed during the experiment')
        result['elapsedMs'] = (time.monotonic() - started) * 1000
        pipeline.write(directory / 'validation.json', result)
    return result


def run(manifest_path, names, generator, output, mode='u24', timeout=None):
    if mode not in storage.MODES:
        raise ValueError('unknown response quantization')
    manifest_path, generator, output = Path(manifest_path).resolve(), Path(generator).resolve(), Path(output).resolve()
    dataset, cases = selected_cases(manifest_path, names)
    inputs = [{field: (manifest_path.parent / case[field]).resolve() for field in ('data', 'snapshot', 'request')}
              for case in cases]
    protected = {manifest_path, generator, *(path for case in inputs for path in case.values())}
    if any(path.is_relative_to(output) for path in protected):
        raise ValueError('experiment output must not contain the original manifest, generator or input files')
    if output.exists() and any(output.iterdir()):
        raise ValueError('use a new empty output so every case has a genuinely cold dictionary')
    before = {path: file_hash(path) for path in protected}
    output.mkdir(parents=True, exist_ok=True)
    result = {'format': FORMAT, 'complete': False, 'status': 'running', 'mode': mode,
              'manifestSha256': before[manifest_path], 'generatorSha256': before[generator],
              'runnerSha256': file_hash(__file__), 'selectedCases': names, 'cases': [],
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
              'changesChartsOrMissions': False, 'allSkillCombinationsPrecomputed': False,
              'scope': 'Declared real-chart contexts and actual published master-skill kernels; full-capacity interaction experiments, not owned-deck search or all-combination coverage.'}
    started = time.monotonic()
    try:
        loaded = {}
        for index, (case, paths) in enumerate(zip(cases, inputs)):
            hashes = {field: before[path] for field, path in paths.items()}
            if hashes['data'] != dataset:
                raise ValueError('selected original dataset bytes differ from the manifest pin')
            if paths['data'] not in loaded:
                loaded[paths['data']] = pipeline.read(paths['data'])
            directory = output / 'cases' / f'{index:03d}'
            outcome = run_case(case, paths, generator, directory, mode, hashes, loaded[paths['data']], timeout)
            result['cases'].append({'name': case['name'], 'directory': directory.relative_to(output).as_posix(),
                                    'complete': outcome['complete'], 'status': outcome['status'],
                                    'sourceVersion': outcome.get('sourceVersion'), 'inputSha256': hashes,
                                    'summary': outcome.get('summary'), 'error': outcome.get('error'),
                                    'elapsedMs': outcome['elapsedMs'], 'receipt': receipt(directory / 'validation.json', output)})
            pipeline.write(output / 'validation.json', result)
        sources = {case['sourceVersion'] for case in result['cases'] if case['complete']}
        result['complete'] = len(result['cases']) == len(cases) and all(case['complete'] for case in result['cases']) and len(sources) == 1
        result['status'] = 'success' if result['complete'] else 'error'
        result['sourceVersion'] = next(iter(sources)) if len(sources) == 1 else None
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(complete=False, status='error', error=pipeline.error_text(error))
    finally:
        try:
            result['inputsUnchanged'] = all(file_hash(path) == digest for path, digest in before.items())
        except OSError:
            result['inputsUnchanged'] = False
        if not result['inputsUnchanged']:
            result.update(complete=False, status='error', error='an original manifest, generator or input file changed')
        result['elapsedMs'] = (time.monotonic() - started) * 1000
        pipeline.write(output / 'validation.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('--case', dest='cases', action='append', required=True, help='exact existing manifest case name; repeat to select several')
    parser.add_argument('--generator', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--mode', choices=storage.MODES, default='u24')
    parser.add_argument('--timeout-seconds', type=float)
    args = parser.parse_args()
    result = run(args.manifest, args.cases, args.generator, args.output, args.mode, args.timeout_seconds)
    print(json.dumps({key: result[key] for key in ('complete', 'status', 'elapsedMs')}))
    return 0 if result['complete'] else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        print(f'luck minimum-basis validation: {error}', file=sys.stderr)
        sys.exit(2)
