#!/usr/bin/env python3
"""Validate joint Miss/minimum recording reuse on an unchanged real LUCK chart.

The declared Cartesian grid contains 625 GS96 level tuples and 25 GS66 level
pairs. Five shards each admit 125 exact Miss controllers, within the native
256-family bound. Every grid mapping is independently recorded with reuse
disabled; these two identification passes perform no probability propagation.

A deterministic subset retains two different minimum distributions for every
distinct conditional mapping shape. Its cold U24 dictionary must cover every
grid program, and its warm query forbids generation. Every selected response
is compared with an independently propagated original whole-live program.
The experiment covers actual-master parameter kernels, not owned decks,
arbitrary skill combinations, full-score expectations or search rankings.
"""
from __future__ import annotations

import argparse
import copy
import importlib.util
import itertools
import json
import math
from pathlib import Path
import subprocess
import sys
import time

_spec = importlib.util.spec_from_file_location('luck_joint_family_lookup',
                                              Path(__file__).with_name('validate_miss_basis.py'))
lookup = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(lookup)
families = lookup.module('validate_basis_families')
miss, basis = lookup.miss, lookup.basis
pipeline, storage = basis.pipeline, basis.storage
require = lookup.require
FORMAT = 'ournotes-deck.luck-joint-family-validation/1'
FAMILY_BYTES = 128 << 20
RESPONSE_BYTES = 256 << 20
MISS_TUPLES = tuple(itertools.product(range(1, 6), repeat=4))
MINIMUM_PAIRS = tuple(itertools.product(range(1, 6), repeat=2))
SHARD_FAMILIES = 125


def grid_job(miss_levels, minimum_levels):
    """One main writer and two actual support writers per original holder."""
    require(len(miss_levels) == 4 and len(minimum_levels) == 2
            and all(type(level) is int and 1 <= level <= 5 for level in (*miss_levels, *minimum_levels)),
            'grid levels must retain four Miss and two minimum level coordinates')
    name = 'joint-miss-' + ''.join(map(str, miss_levels)) + '-minimum-' + ''.join(map(str, minimum_levels))
    job = miss.miss_job(name, tuple(miss_levels) + (3,))
    for key, position in job['entries']:
        if key['source'] == 'gekisouSupport' and key['id'] == 66 and position < 2:
            key['level'] = minimum_levels[position]
    return job


def nonminimum_key(job):
    """The grid varies only GS66 while retaining the ordered Miss controller."""
    return pipeline.canonical([entry for entry in job['entries']
                               if not (entry[0]['source'] == 'gekisouSupport' and entry[0]['id'] == 66)])


def specification(jobs, reuse, mode='basisIdentify'):
    return {'mode': mode, 'jobs': copy.deepcopy(jobs), 'canonicalStartMinimum': False,
            'canonicalMissGauge': True, 'basisFamilyReuse': reuse,
            'identityBytes': miss.IDENTITY_BYTES, 'programBytes': miss.PROGRAM_BYTES,
            'basisFamilyBytes': FAMILY_BYTES, 'basisResponseBytes': RESPONSE_BYTES,
            'basisMaxTerms': lookup.MAX_TERMS, 'basisReconstruct': False,
            'verifyBasis': False, 'mcRuns': 0, 'scoreSamples': 0}


def make_specs(data):
    source = miss.validate_sources(data)
    available, unsupported = miss.catalogue.all_chain_keys(data)
    available_keys = {pipeline.canonical(key) for key in available}
    operators = miss.operators.Catalogue(data['master'])
    minimum_levels = []
    for level in range(1, 6):
        key = miss.key('gekisouSupport', 66, level)
        require(pipeline.canonical(key) in available_keys
                and not any(all(item.get(field) == key[field] for field in ('source', 'id', 'level'))
                            for item in unsupported),
                'a declared GS66 minimum level is absent or has conflicting formation targets')
        action = operators.start('gekisouSupport', 66, level, False, 1000)
        kind, minimum, probability = action['action']
        require(kind == 'minimum' and minimum == 1 and 0 < probability < 1
                and action['band'] == source['minimum']['band'],
                'actual GS66 levels changed the declared unmatched minimum shape')
        minimum_levels.append({'key': key, 'minimum': minimum,
                               'probability': miss.operators.rational(probability), 'rowIds': action['rowIds']})
    jobs = [grid_job(levels, minima) for levels in MISS_TUPLES for minima in MINIMUM_PAIRS]
    require(len(jobs) == len({job['name'] for job in jobs}) == 15625
            and len({nonminimum_key(job) for job in jobs}) == 625,
            'the Cartesian grid omits or duplicates an exact controller or level pair')
    for job in jobs:
        require(all(pipeline.canonical(key) in available_keys and type(position) is int and 0 <= position < 5
                    for key, position in job['entries']),
                'a joint kernel contains an unavailable actual key or holder position')
        for holder in range(5):
            held = [key for key, position in job['entries'] if position == holder]
            require(len(held) == 3 and sum(key['source'] == 'gekisou' for key in held) == 1
                    and sum(key['source'] == 'gekisouSupport' for key in held) == 2,
                    'a joint kernel changes the native five-main/ten-support capacity')
    shard_size = SHARD_FAMILIES * len(MINIMUM_PAIRS)
    shards = [jobs[start:start + shard_size] for start in range(0, len(jobs), shard_size)]
    require(len(shards) == 5 and all(len(shard) == 3125
            and len({nonminimum_key(job) for job in shard}) == SHARD_FAMILIES for shard in shards),
            'joint family shards do not retain five complete 125-controller partitions')
    source.update(format='ournotes-deck.luck-joint-family-specs/1',
        grid={'jobs': len(jobs), 'missTuples': len(MISS_TUPLES), 'minimumPairs': len(MINIMUM_PAIRS),
              'levels': [1, 2, 3, 4, 5], 'variableGS96Holders': [0, 1, 2, 3],
              'fixedGS96Holder4Level': 3, 'variableGS66Holders': [0, 1],
              'fixedGS66Holders234Level': 3, 'mainGK8Level': 3, 'matched': False,
              'mainWriters': 5, 'supportWriters': 10},
        minimumLevels=minimum_levels,
        shards={'count': len(shards), 'exactMissFamiliesPerShard': SHARD_FAMILIES,
                'jobsPerShard': shard_size, 'ordering': 'lexicographic Miss tuple, then minimum pair',
                'basisFamilyBytes': FAMILY_BYTES, 'nativeFamilyCountBound': 256},
        changesChartsOrMissions=False, allTeamCombinationsCovered=False,
        scope='Actual-master full-capacity parameter kernels on the unchanged supplied LUCK chart.')
    return jobs, shards, source


def identify(report, spec, hashes, spec_sha, source):
    actual, programs, mappings, references = basis.identification(
        report, spec['jobs'], lookup.MAX_TERMS, lookup.CONTRACT)
    basis.provenance(report, hashes, spec_sha)
    require(actual == source and report['stats'].get('uniquePrograms') == len(programs),
            'joint identification changed native source or conditional coverage')
    expected = len({nonminimum_key(job) for job in spec['jobs']})
    counts = lookup.family_counts(report['stats'], spec['jobs'], spec['basisFamilyReuse'], expected)
    require(report['stats'].get('familyBudgetBytes') == FAMILY_BYTES
            and report['stats']['familyCacheBytes'] <= FAMILY_BYTES,
            'joint identification exceeded its explicit retained-family byte allowance')
    return mappings, {program['fingerprint'] for program in programs}, references, counts


def mapping_shape(terms):
    return tuple((tuple(term['choices']), term['programFingerprint']) for term in terms)


def coverage_subset(jobs, mappings):
    """Select two minimum laws for each complete ordered conditional shape."""
    require(len(jobs) == len(mappings) == len({job['name'] for job in jobs})
            and set(mappings) == {job['name'] for job in jobs},
            'conditional coverage cannot be selected from an incomplete labelled grid')
    by_family = {}
    for job in jobs:
        by_family.setdefault(nonminimum_key(job), []).append(job)
    by_shape = {}
    for family_jobs in by_family.values():
        shapes = {mapping_shape(mappings[job['name']]) for job in family_jobs}
        require(len(shapes) == 1, 'minimum-only level changes altered a complete conditional program shape')
        shape = next(iter(shapes))
        by_shape.setdefault(shape, family_jobs)
    selected, classes = [], []
    for shape, family_jobs in by_shape.items():
        first = family_jobs[0]
        second = next((job for job in reversed(family_jobs)
                       if mappings[job['name']] != mappings[first['name']]), None)
        require(second is not None, 'a conditional class lacks two distinct observed minimum distributions')
        selected.extend((first, second))
        classes.append({'shapeSha256': pipeline.sha(pipeline.canonical(shape)),
                        'jobs': [first['name'], second['name']], 'conditionalPrograms': len(shape),
                        'differentIntervalWeights': True})
    complete = {term['programFingerprint'] for terms in mappings.values() for term in terms}
    covered = {term['programFingerprint'] for job in selected for term in mappings[job['name']]}
    require(complete == covered and len({job['name'] for job in selected}) == len(selected),
            'selected original-DP comparisons omit or duplicate conditional program coverage')
    return selected, {'gridJobs': len(jobs), 'exactMissControllers': len(by_family),
                      'canonicalConditionalShapes': len(by_shape), 'selectedJobs': len(selected),
                      'allGridConditionalProgramsCovered': True, 'conditionalPrograms': len(complete),
                      'conditionalCoverageMeaning': 'Every grid program has positive weight in selected mixtures compared with original whole-live DP.',
                      'twoDistinctMinimumDistributionsPerShape': True, 'classes': classes}


def run(data, snapshot, request, generator, source, output, timeout=None):
    started = time.monotonic()
    paths = {name: Path(path).resolve() for name, path in
             (('data', data), ('snapshot', snapshot), ('request', request))}
    generator, source, output = (Path(path).resolve() for path in (generator, source, output))
    scripts = {path.name: path.resolve() for path in Path(__file__).parent.glob('*.py')}
    protected = {**paths, 'generator': generator, **{'script:' + name: path for name, path in scripts.items()}}
    require(not any(path.is_relative_to(output) for path in protected.values())
            and not source.is_relative_to(output), 'output must not contain an input, generator or source tree')
    require(not output.exists() or output.is_dir() and not any(output.iterdir()),
            'use a new empty output directory for joint validation')
    require(timeout is None or math.isfinite(timeout) and timeout > 0, 'timeout must be positive and finite')
    before = {name: lookup.receipt(path) for name, path in protected.items()}
    hashes = {name: before[name]['sha256'] for name in paths}
    algorithm = pipeline.source_identity(source)
    native_source = algorithm['simSourceSha256']
    actual = pipeline.read(paths['data'])
    require(pipeline.luck_case(actual, pipeline.read(paths['request'])),
            'the unchanged supplied real chart has no declared LUCK mission')
    jobs, shards, source_receipt = make_specs(actual)
    output.mkdir(parents=True, exist_ok=True)
    pipeline.write(output / 'real-source-receipt.json', source_receipt)
    result = {'format': FORMAT, 'complete': False, 'status': 'running', 'operatorContract': lookup.CONTRACT,
              'algorithm': algorithm, 'sourceVersion': native_source,
              'generatorSha256': before['generator']['sha256'], 'inputSha256': hashes,
              'inputs': {name: before[name] for name in (*paths, 'generator')},
              'sourceSha256': {name: before['script:' + name]['sha256'] for name in sorted(scripts)},
              'scope': source_receipt['scope'], 'grid': source_receipt['grid'],
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
              'allTeamCombinationsCovered': False, 'changesChartsOrMissions': False,
              'timingScope': 'Observed sequential wall time, including report I/O; no isolated-CPU claim.',
              'shards': []}
    pipeline.write(output / 'validation.json', result)
    combined, reference_context, all_programs = {}, None, set()
    try:
        for ordinal, shard_jobs in enumerate(shards):
            directory = output / 'shards' / str(ordinal)
            phase = {'index': ordinal, 'jobs': len(shard_jobs), 'phases': {}}
            result['shards'].append(phase)
            native, mappings = {}, {}
            for label, reuse in (('reuse', True), ('independent', False)):
                spec = specification(shard_jobs, reuse)
                spec_path, report_path = directory / (label + '-spec.json'), directory / (label + '.json')
                pipeline.write(spec_path, spec)
                began = time.monotonic()
                storage.call([generator, paths['data'], paths['snapshot'], paths['request'], spec_path, report_path],
                             directory / (label + '.log'), timeout)
                native[label] = pipeline.read(report_path)
                mapping, programs, references, counts = identify(native[label], spec, hashes,
                    miss.file_hash(spec_path), native_source)
                mappings[label] = mapping
                phase['phases'][label] = {'wallMs': (time.monotonic() - began) * 1000,
                    'report': lookup.receipt(report_path, output), 'spec': lookup.receipt(spec_path, output),
                    'conditionalPrograms': len(programs), 'conditionalTermReferences': references,
                    'propagationCalls': 0, 'familyCounts': counts, 'nativeStats': native[label]['stats']}
                pipeline.write(output / 'validation.json', result)
            families.same_context(native['reuse'], native['independent'], same_jobs=True)
            if reference_context is None:
                reference_context = native['reuse']
            else:
                families.same_context(reference_context, native['reuse'])
            phase['comparison'] = families.compare_mappings(mappings['reuse'], mappings['independent'])
            require(not combined.keys() & mappings['reuse'].keys(), 'joint shards repeat a labelled source job')
            combined.update(mappings['reuse'])
            all_programs.update(term['programFingerprint'] for terms in mappings['reuse'].values() for term in terms)
            pipeline.write(output / 'validation.json', result)
        require(set(combined) == {job['name'] for job in jobs}, 'joint shards omitted a Cartesian grid job')
        totals = {}
        for label in ('reuse', 'independent'):
            parts = [shard['phases'][label] for shard in result['shards']]
            totals[label] = {'requestedJobs': sum(shard['jobs'] for shard in result['shards']),
                'conditionalTermReferences': sum(part['conditionalTermReferences'] for part in parts),
                'conditionalProgramsAcrossShards': len(all_programs), 'propagationCalls': 0,
                'wallMs': sum(part['wallMs'] for part in parts),
                'familyCounts': {field: sum(part['familyCounts'][field] for part in parts)
                                 for field in lookup.FAMILY_COUNTS
                                 if field not in ('familyCacheBytes', 'retainedFamilies')},
                'retainedFamilyAdmissionsAcrossShards': sum(part['familyCounts']['retainedFamilies']
                                                            for part in parts),
                'maximumRetainedFamiliesPerShard': max(part['familyCounts']['retainedFamilies'] for part in parts),
                'maximumFamilyCacheBytesPerShard': max(part['familyCounts']['familyCacheBytes'] for part in parts)}
        result['identification'] = totals
        result['comparison'] = {'comparedJobs': len(combined),
            'comparedTermReferences': sum(len(terms) for terms in combined.values()), 'exactMappingsEqual': True}
        selected, coverage = coverage_subset(jobs, combined)
        result['coverage'] = coverage
        query_spec = specification(selected, True, 'basisPrograms')
        original_spec = {**query_spec, 'mode': 'programs', 'canonicalMissGauge': False,
                         'basisFamilyReuse': False}
        subset_receipt = {'gridSourceReceiptSha256': miss.file_hash(output / 'real-source-receipt.json'),
                          'coverage': coverage}
        pipeline.write(output / 'validation.json', result)
        checked = lookup.run(paths['data'], paths['snapshot'], paths['request'], generator, source,
            output / 'lookup', timeout, kernel_specs=(selected, original_spec, query_spec, subset_receipt),
            expected_families=len({nonminimum_key(job) for job in selected}),
            scope='Two distinct minimum distributions per conditional mapping shape of the declared joint grid; all grid conditional programs participate in the mixtures compared with original whole-live DP.')
        result['lookup'] = checked
        require(checked.get('complete') is True and checked.get('status') == 'success',
                'joint original-DP/cold/warm verification failed: ' + str(checked.get('error', 'incomplete')))
        lookup_map = pipeline.read(output / 'lookup/cold/identify.json')
        _source, _programs, selected_mapping, _refs = basis.identification(
            lookup_map, selected, lookup.MAX_TERMS, lookup.CONTRACT)
        families.same_context(reference_context, lookup_map)
        result['coverage']['subsetMatchesFullGrid'] = families.compare_mappings(combined, selected_mapping)
        require({term['programFingerprint'] for terms in selected_mapping.values() for term in terms} == all_programs,
                'cold/warm subset changed complete grid conditional coverage')
        result['work'] = {'gridReuseRecordings': totals['reuse']['familyCounts']['nativeRecordings'],
            'gridIndependentRecordings': totals['independent']['familyCounts']['nativeRecordings'],
            'gridFamilyHits': totals['reuse']['familyCounts']['familyHits'], 'gridPropagationCalls': 0,
            'originalWholeLivePropagationCalls': checked['reference']['stats']['propagationCalls'],
            'coldConditionalPropagationCalls': checked['queries']['cold']['stats']['propagationCalls'],
            'warmConditionalPropagationCalls': checked['queries']['warm']['stats']['propagationCalls'],
            'coldIdentificationRecordings': checked['queries']['cold']['identificationStats']['nativeRecordings'],
            'coldGenerationRecordings': checked['queries']['cold']['generationStats']['nativeRecordings'],
            'warmIdentificationRecordings': checked['queries']['warm']['identificationStats']['nativeRecordings']}
        result.update(status='success', complete=True)
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(status='error', complete=False, error=pipeline.error_text(error))
    finally:
        try:
            result['inputsUnchanged'] = all(lookup.receipt(path) == before[name] for name, path in protected.items())
            result['sourceUnchanged'] = pipeline.source_identity(source) == algorithm
        except (OSError, ValueError) as error:
            result.update(inputsUnchanged=False, sourceUnchanged=False, error=pipeline.error_text(error))
        if not result['inputsUnchanged'] or not result['sourceUnchanged']:
            result.update(status='error', complete=False,
                          error='an input, generator, orchestration file or native source changed during validation')
        result['elapsedMs'] = (time.monotonic() - started) * 1000
        pipeline.write(output / 'validation.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('data', 'snapshot', 'request'):
        parser.add_argument(name, type=Path)
    for name in ('generator', 'source', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--timeout-seconds', type=float)
    args = parser.parse_args()
    result = run(args.data, args.snapshot, args.request, args.generator, args.source, args.output, args.timeout_seconds)
    print(json.dumps({key: result[key] for key in ('complete', 'status', 'elapsedMs')}))
    return 0 if result['complete'] else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        print('luck joint family validation: ' + str(error), file=sys.stderr)
        sys.exit(2)
