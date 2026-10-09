#!/usr/bin/env python3
"""Validate minimum-family recording reuse over every declared real regional chart.

Every applicable chart identifies the complete 1,250-job actual-master grid.
A separately recorded subset also runs the original native DP; its conditional
programs must cover the entire grid before they become the chart dictionary.
New support combinations and three non-minimum controls are compared with the
unoptimized recorder. None of these isolated skill kernels is an owned deck.
"""
from __future__ import annotations

import argparse
import copy
from fractions import Fraction
import gzip
import hashlib
import importlib.util
import json
import lzma
import math
from pathlib import Path
import subprocess
import sys
import time


def module(name):
    directory = str(Path(__file__).resolve().parent)
    inserted = directory not in sys.path
    if inserted:
        sys.path.insert(0, directory)
    try:
        spec = importlib.util.spec_from_file_location('luck_family_validation_' + name,
                                                    Path(directory) / (name + '.py'))
        result = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(result)
        return result
    finally:
        if inserted:
            sys.path.remove(directory)


basis, recipes = module('basis'), module('make_minimum_basis_specs')
analysis = module('analyze_programs')
pipeline, storage, catalogue = basis.pipeline, basis.storage, recipes.catalogue
FORMAT = 'ournotes-deck.luck-basis-family-catalogue/1'
CASE_FORMAT = 'ournotes-deck.luck-basis-family-chart/1'
SHARD_ORDERING = 'score-id-modulo/1'
FAMILY_COUNTS = ('nativeRecordings', 'familyAdmissionCalls', 'familyHits', 'familyMisses',
                 'familyFallbacks', 'retainedFamilies', 'familyCacheBytes',
                 'familyIdentityComputations', 'familyIdentityHits')


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def receipt(path, root):
    path, root = Path(path).resolve(), Path(root).resolve()
    return {'path': path.relative_to(root).as_posix(), 'sha256': file_hash(path),
            'bytes': path.stat().st_size}


def shard_cases(cases, index, count):
    if (type(index) is not int or type(count) is not int
            or not 1 <= count <= 1024 or not 0 <= index < count):
        raise ValueError('invalid score-ID shard index/count')
    return [case for case in sorted(cases, key=lambda row: row['scoreId'])
            if case['scoreId'] % count == index]


def declared_cases(manifest_path):
    """Require the exact complete master chart set, including non-LUCK records."""
    manifest_path = Path(manifest_path).resolve()
    manifest = pipeline.read(manifest_path)
    declaration = manifest.get('luckCatalogue', {})
    cases = manifest.get('cases')
    if (manifest.get('format') != 'ournotes-deck.search-benchmark/1'
            or declaration.get('format') != catalogue.INVENTORY_FORMAT
            or declaration.get('selection') != 'all-master-source-levels'
            or declaration.get('includeNonLuckCharts') is not True
            or not isinstance(cases, list) or not cases
            or len({case['name'] for case in cases}) != len(cases)):
        raise ValueError('provide a complete declared whole-catalogue manifest')
    data_paths = {(manifest_path.parent / case['data']).resolve() for case in cases}
    if len(data_paths) != 1:
        raise ValueError('one regional dataset is required per manifest')
    data_path = next(iter(data_paths))
    data_raw = data_path.read_bytes()
    data = json.loads(data_raw)
    inventory = catalogue.inventory(data, pipeline.sha(data_raw), data.get('provenance', {}).get('region'))
    if manifest.get('datasetId') != inventory['datasetId']:
        raise ValueError('original regional dataset differs from the manifest pin')
    keys = declaration.get('keys')
    if (not isinstance(keys, list) or len(keys) != len(inventory['luckCatalogueKeys'])
            or {pipeline.canonical(key) for key in keys}
               != {pipeline.canonical(key) for key in inventory['luckCatalogueKeys']}):
        raise ValueError('manifest does not retain every actual master source/level/formation key')
    expected = {row['scoreId']: row for row in inventory['charts']}
    retained = {}
    for case in cases:
        paths = {field: (manifest_path.parent / case[field]).resolve()
                 for field in ('data', 'snapshot', 'request')}
        request = pipeline.read(paths['request'])
        score = request['execution']['scoreId']
        if type(score) is not int or score not in expected or score in retained:
            raise ValueError('manifest has a duplicate or unknown requested chart')
        chart = expected[score]
        if (request['scenario'] != {'kind': 'mission', 'musicId': chart['musicId']}
                or request['execution'].get('kind') != 'live'
                or request['execution'].get('gekisou') is not True
                or pipeline.luck_case(data, request) != chart['hasLuckMission']
                or case.get('scoreId') != score or case.get('difficulty') != chart['difficulty']
                or case.get('hasLuckMission') is not chart['hasLuckMission']):
            raise ValueError('manifest alters an actual chart, difficulty or native mission context')
        if pipeline.read(paths['snapshot']).get('datasetId') != inventory['datasetId']:
            raise ValueError('regional anchor snapshot identifies different dataset bytes')
        retained[score] = {'name': case['name'], 'scoreId': score, 'chart': chart, 'paths': paths}
    if set(retained) != set(expected):
        raise ValueError('manifest omits one or more actual master charts')
    return manifest, inventory, data, list(retained.values())


def specifications(data, verification_jobs=6, full_reference_grid=False):
    if type(verification_jobs) is not int or not 2 <= verification_jobs <= 6:
        raise ValueError('independent original-DP verification needs 2..6 real configurations per chart')
    original, selected, supports = recipes.make_specs(data)
    grid = original['grid-identify.json']['jobs']
    # Alternate the two formations even for the two-comparison configuration.
    checked = original['verification.json']['jobs']
    verified = [copy.deepcopy(checked[index]) for index in (0, 3, 1, 4, 2, 5)][:verification_jobs]
    signature_to_name = {pipeline.canonical(job['entries']): job['name'] for job in grid}
    for job in verified:
        job['name'] = signature_to_name[pipeline.canonical(job['entries'])]
    controls = copy.deepcopy(original['new-combinations.json']['jobs'])
    base = recipes.job('mixed-matches', (1, 2, 3, 4))
    base['name'] = 'family-control-base'
    controls.append(base)
    available = {catalogue.canonical(key) for key in catalogue.all_chain_keys(data)[0]}
    operators = recipes.Catalogue(data['master'])
    control_keys = []
    # Every changed row is an actual skill. Miss-triggered gauge belongs to
    # support 96; main 14's 11002 is a Critical point bonus, not a gauge writer.
    for label, source, skill, effect in (('start-gauge', 'gekisou', 20, 11003),
                                         ('miss-gauge', 'gekisouSupport', 96, 11003),
                                         ('gauge-speed', 'gekisou', 7, 11001)):
        key = recipes.key(source, skill, 3, True if source == 'gekisouSupport' else None)
        if catalogue.canonical(key) not in available:
            raise ValueError('required actual non-minimum control key is absent')
        rows = operators.sources[source, skill, 3][1]
        if not rows or any(row['_skillEffectType'] != effect for row in rows):
            raise ValueError('actual non-minimum control changed its declared effect kind')
        job = copy.deepcopy(base)
        job['name'] = 'family-control-' + label
        for item, position in job['entries']:
            if (item['source'] == source and position == 4
                    and (source == 'gekisou' or item['id'] == 66)):
                item['id'] = skill
                item['level'] = 3
        controls.append(job)
        control_keys.append({'key': key, 'effectType': effect, 'rowIds': [row['_id'] for row in rows]})

    def spec(jobs, reuse, verify=False):
        return {'mode': 'basisPrograms' if verify else 'basisIdentify', 'jobs': copy.deepcopy(jobs),
                'basisMaxTerms': 64, 'basisFamilyReuse': reuse, 'verifyBasis': verify,
                'basisReconstruct': verify, 'mcRuns': 0, 'scoreSamples': 0}

    result = {'grid-fast.json': spec(grid, True), 'verification-original.json': spec(verified, False, True),
              'controls-fast.json': spec(controls, True), 'controls-original.json': spec(controls, False)}
    if full_reference_grid:
        result['grid-original.json'] = spec(grid, False)
    return result, {'selectedKeys': selected, 'supportOperatorsAtDeclaredPhaseLife1000': supports,
                    'nonminimumControls': control_keys,
                    'scope': 'Actual full-capacity master-skill kernels; not owned-deck search cases.'}


def family_counts(report, jobs, reuse, single_family=False):
    stats = report.get('stats', {})
    if (stats.get('requestedJobs') != len(jobs) or stats.get('compiledJobs') != len(jobs)
            or any(type(stats.get(field)) is not int or stats[field] < 0 for field in FAMILY_COUNTS)):
        raise ValueError('native family statistics do not cover all admitted requested jobs')
    if reuse:
        if stats['nativeRecordings'] + stats['familyHits'] != len(jobs):
            raise ValueError('native recordings and family hits do not partition the admitted jobs')
        if single_family and (stats['nativeRecordings'] != 1 or stats['familyHits'] != len(jobs) - 1
                              or stats['familyMisses'] != 1 or stats['familyFallbacks'] != 0
                              or stats['retainedFamilies'] != 1):
            raise ValueError('the actual minimum-only grid did not reuse one admitted non-minimum family')
    elif (stats['nativeRecordings'] != len(jobs) or stats['familyHits'] != 0
          or stats['retainedFamilies'] != 0):
        raise ValueError('the unoptimized reference unexpectedly reused a native family')
    return {field: stats[field] for field in FAMILY_COUNTS}


def identify(report, spec, hashes, spec_sha, source, single_family=False):
    actual, programs, mappings, references = basis.identification(report, spec['jobs'])
    basis.provenance(report, hashes, spec_sha)
    if actual != source or report['stats'].get('uniquePrograms') != len(programs):
        raise ValueError('identification changed the expected native source or program count')
    counts = family_counts(report, spec['jobs'], spec['basisFamilyReuse'], single_family)
    return mappings, {program['fingerprint'] for program in programs}, references, counts


def compare_mappings(left, right, names=None):
    names = list(right) if names is None else list(names)
    if not names or len(set(names)) != len(names) or not set(names) <= set(left) & set(right):
        raise ValueError('requested native mapping comparison is missing a labelled configuration')
    references = 0
    for name in names:
        # Fingerprint, deterministic choices and both outward weight endpoints
        # must agree. A close probability or a matching program count is weaker.
        if left[name] != right[name]:
            raise ValueError('fast and independently recorded native mappings differ: ' + name)
        references += len(left[name])
    return {'comparedJobs': len(names), 'comparedTermReferences': references, 'exactMappingsEqual': True}


def verified_programs(report, spec, hashes, spec_sha, source):
    """Require independent original DP and its complete positive term mapping."""
    jobs = spec['jobs']
    basis.provenance(report, hashes, spec_sha)
    if (spec.get('verifyBasis') is not True or spec.get('basisFamilyReuse') is not False
            or report.get('format') != basis.REPORT or report.get('mode') != 'basisPrograms'
            or report.get('operatorContract') != basis.CONTRACT or report.get('sourceVersion') != source
            or any(report.get(field) is not True for field in ('complete', 'identificationComplete',
                   'probabilityComplete', 'verificationRequested', 'verificationComplete'))
            or report.get('missingSelectedPrograms') != []):
        raise ValueError('independent original-DP verification did not complete')
    context = report.get('context', {})
    if context.get('algorithmVersion') != 'ournotes-luck-response/1/' + source:
        raise ValueError('verification response context uses a different native algorithm')
    storage.digest(context.get('fingerprint'))
    programs = report.get('programs', [])
    if not 1 <= len(programs) <= 64:
        raise ValueError('verification has no bounded conditional program set')
    fingerprints = []
    for index, program in enumerate(programs):
        fingerprints.append(storage.digest(program.get('fingerprint')))
        if (program.get('programIndex') != index or program.get('sourceVersion') != source
                or program.get('operatorContract') != basis.CONTRACT or program.get('status') != 'success'
                or program.get('response', {}).get('status') != 'success'
                or not isinstance(program.get('identityVersion'), str) or not program['identityVersion']):
            raise ValueError('verification contains an unsuccessful or wrong-source conditional response')
    if len(set(fingerprints)) != len(fingerprints):
        raise ValueError('verification repeats a complete conditional program identity')
    rows = pipeline.job_results(report, jobs)
    mappings, referenced, comparisons = {}, set(), []
    for ordinal, job in enumerate(jobs):
        row = rows[job['name']]
        check, components = row.get('verification', {}), row.get('components', [])
        if (row.get('jobIndex') != ordinal or row.get('status') != 'success'
                or row.get('response', {}).get('status') != 'success'
                or not 1 <= len(components) <= 64 or row.get('termCount') != len(components)
                or type(row.get('startCount')) is not int or not 0 <= row['startCount'] <= 3
                or check.get('status') != 'success' or check.get('disjointIntervals') != 0
                or type(check.get('comparedBuckets')) is not int or check['comparedBuckets'] <= 0
                or check.get('probesEqual') is not True or check.get('probeTransitionsEqual') is not True):
            raise ValueError('a requested configuration lacks its complete independent original-DP comparison')
        lower, upper, choices_seen, mapped = Fraction(0), Fraction(0), set(), []
        for component in components:
            index, weight, choices = component.get('programIndex'), component.get('weight'), component.get('choices')
            if (type(index) is not int or not 0 <= index < len(programs)
                    or component.get('programFingerprint') != fingerprints[index]
                    or not isinstance(weight, list) or len(weight) != 2
                    or any(type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 1
                           for value in weight)
                    or not 0 <= weight[0] <= weight[1] <= 1 or weight[1] == 0
                    or not isinstance(choices, list) or len(choices) != row['startCount']
                    or any(type(value) is not int or not 0 <= value <= 3 for value in choices)
                    or tuple(choices) in choices_seen):
                raise ValueError('independent verification changed a native term, choice or positive interval weight')
            referenced.add(index); choices_seen.add(tuple(choices))
            lower += Fraction(weight[0]); upper += Fraction(weight[1])
            mapped.append({'programFingerprint': fingerprints[index], 'weight': weight, 'choices': choices})
        if not lower <= 1 <= upper:
            raise ValueError('verification conditional weights do not enclose unit mass')
        difference = check.get('maximumEndpointDifference')
        if type(difference) not in (int, float) or not math.isfinite(difference) or difference < 0:
            raise ValueError('verification omitted a finite measured endpoint difference')
        mappings[job['name']] = mapped
        comparisons.append({'name': job['name'], **check})
    for index, program in enumerate(programs):
        representative, term = program.get('representativeJobIndex'), program.get('representativeTermIndex')
        if (type(representative) is not int or not 0 <= representative < len(jobs)
                or type(term) is not int or not 0 <= term < len(rows[jobs[representative]['name']]['components'])
                or rows[jobs[representative]['name']]['components'][term]['programIndex'] != index):
            raise ValueError('verification has an invalid conditional representative')
    stats = report.get('stats', {})
    if (referenced != set(range(len(programs))) or stats.get('verificationCalls') != len(jobs)
            or stats.get('verifiedJobs') != len(jobs) or stats.get('reconstructedJobs') != len(jobs)
            or stats.get('uniquePrograms') != len(programs) or stats.get('propagationCalls') != len(programs)):
        raise ValueError('verification work counters omit an original DP or requested conditional term')
    counts = family_counts(report, jobs, False)
    return mappings, programs, {'verifiedJobs': len(jobs), 'conditionalPropagationCalls': len(programs),
        'originalDpVerificationCalls': len(jobs), 'disjointIntervals': 0,
        'comparedBuckets': sum(check['comparedBuckets'] for check in comparisons),
        'maximumEndpointDifference': max(check['maximumEndpointDifference'] for check in comparisons),
        'comparisons': comparisons, 'familyCounts': counts}


def same_context(left, right, same_jobs=False):
    if any(left.get(field) != right.get(field) for field in
           ('sourceVersion', 'sharedFingerprint', 'dependencyDescriptor', 'capabilities')):
        raise ValueError('independent original recording changed the resolved input/dependency context')
    # Archive context also hashes requested entry labels. Distinct verification
    # subsets must retain the shared native context, while their archive labels
    # correctly differ. Equal requested jobs must preserve both contexts.
    if same_jobs and left.get('context') != right.get('context'):
        raise ValueError('equal requested jobs received different native archive contexts')


def control_separation(mappings):
    base = {row['programFingerprint'] for row in mappings['family-control-base']}
    controls = {}
    for label in ('start-gauge', 'miss-gauge', 'gauge-speed'):
        name = 'family-control-' + label
        current = {row['programFingerprint'] for row in mappings[name]}
        if not current or current & base:
            raise ValueError('a changed non-minimum controller reused a base-family program: ' + label)
        controls[label] = {'programs': len(current), 'sharedBasePrograms': 0}
    return controls


def dictionary(report, programs, source, mode, generator, directory, timeout):
    report_path = directory / 'verification-original.json'
    projected = basis.storage_projection(report, programs, file_hash(report_path))
    projection = directory / 'programs.json'
    pipeline.write(projection, projected)
    destination = directory / 'dictionary' / mode
    storage.call([generator, 'program-pack', projection, mode, destination], directory / 'pack.log', timeout)
    index_path = destination / 'program-index.json'
    rows = storage.load_index(index_path, source, mode)
    if set(rows) != {program['fingerprint'] for program in programs}:
        raise ValueError('packed dictionary changed the complete verified conditional program set')
    blobs, decoded = {}, 0
    for row in rows.values():
        path, raw = storage.verified_blob(destination, row, source, mode)
        if not raw.startswith(b'ONLRSP02'):
            raise ValueError('native packed conditional response uses a different binary format')
        blobs[row['archive']['sha256']] = path
        decoded += row['archive']['bytes']
    compressed = analysis.compression(index_path.read_bytes(), blobs, ('gzip9', 'xz6'), destination)
    for method, opener in (('gzip9', gzip.open), ('xz6', lzma.open)):
        packed = destination / compressed[method]['artifact']
        digest, size = hashlib.sha256(), 0
        with opener(packed, 'rb') as stream:
            while chunk := stream.read(1024 * 1024):
                digest.update(chunk); size += len(chunk)
        if size != compressed['tar']['bytes'] or digest.hexdigest() != compressed['tar']['sha256']:
            raise ValueError('conditional dictionary compression failed its exact TAR round trip')
        compressed[method]['roundTripVerified'] = True
    return {'mode': mode, 'programs': len(rows), 'uniqueBlobs': len(blobs),
            'index': receipt(index_path, directory), 'projection': receipt(projection, directory),
            'indexAndUniqueBlobBytes': index_path.stat().st_size + sum(path.stat().st_size for path in blobs.values()),
            'sumReferencedBlobBytes': decoded, 'compression': compressed,
            'globalSkillCombinationAliasesStored': False}


def run_case(case, specs, spec_receipt, generator, source, output, mode, timeout):
    paths, chart = case['paths'], case['chart']
    hashes = pipeline.provenance(paths)
    output.mkdir(parents=True, exist_ok=True)
    result = {'format': CASE_FORMAT, 'name': case['name'], 'scoreId': case['scoreId'],
              'difficulty': chart['difficulty'], 'missions': chart['missions'],
              'sourceVersion': source, 'inputSha256': hashes, 'specification': spec_receipt,
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
              'complete': False, 'status': 'running', 'phases': {}}
    started = time.monotonic()
    try:
        spec_paths = {}
        for name, spec in specs.items():
            path = output / 'specs' / name
            pipeline.write(path, spec)
            spec_paths[name] = path
        base = [generator, paths['data'], paths['snapshot'], paths['request']]

        def native(name):
            path = output / name
            began = time.monotonic()
            storage.call(base + [spec_paths[name], path], path.with_suffix('.log'), timeout)
            value = pipeline.read(path)
            result['phases'][name.removesuffix('.json')] = {
                'report': receipt(path, output), 'spec': receipt(spec_paths[name], output),
                'wallMs': (time.monotonic() - began) * 1000, 'nativeStats': value.get('stats')}
            return value

        fast = native('grid-fast.json')
        fast_map, grid_programs, references, counts = identify(fast, specs['grid-fast.json'], hashes,
            file_hash(spec_paths['grid-fast.json']), source, single_family=True)
        expected_starts = sum(mission == 2 for mission in chart['missions'])
        if (not 1 <= expected_starts <= 3 or len(grid_programs) > 3 ** expected_starts
                or any(row['startCount'] != expected_starts for row in fast['jobs'])):
            raise ValueError('native conditional family exceeds the actual LUCK-start bound')
        result['grid'] = {'jobs': len(fast_map), 'conditionalTermReferences': references,
                          'uniquePrograms': len(grid_programs), 'actualLuckStarts': expected_starts,
                          'maximumProgramsForThisChart': 3 ** expected_starts, 'familyCounts': counts}
        verified = native('verification-original.json')
        verify_map, programs, verification = verified_programs(verified, specs['verification-original.json'],
            hashes, file_hash(spec_paths['verification-original.json']), source)
        same_context(fast, verified)
        verification.update(compare_mappings(fast_map, verify_map))
        if {program['fingerprint'] for program in programs} != grid_programs:
            raise ValueError('independent original-DP configurations do not cover every grid conditional program')
        verification['coversAllGridPrograms'] = True
        result['verification'] = verification

        control_fast, control_original = native('controls-fast.json'), native('controls-original.json')
        fast_controls, _, _, _ = identify(control_fast, specs['controls-fast.json'], hashes,
            file_hash(spec_paths['controls-fast.json']), source)
        old_controls, _, _, _ = identify(control_original, specs['controls-original.json'], hashes,
            file_hash(spec_paths['controls-original.json']), source)
        same_context(fast, control_fast); same_context(control_fast, control_original, same_jobs=True)
        result['controls'] = {**compare_mappings(fast_controls, old_controls),
                              'nonminimumSeparation': control_separation(fast_controls)}
        if control_fast['stats']['nativeRecordings'] < 4:
            raise ValueError('non-minimum controls did not receive their separate native recordings')
        # New minimum multiplicities/sources still use only the independently
        # propagated grid terms. No probability propagation is run here.
        new = [name for name in fast_controls if name.startswith('minimum-new-')]
        if len(new) != 6 or any(row['programFingerprint'] not in grid_programs
                               for name in new for row in fast_controls[name]):
            raise ValueError('new actual minimum combinations did not reuse the verified grid term dictionary')
        result['controls'].update(newMinimumCombinations=6, newMinimumPropagationCalls=0)
        if 'grid-original.json' in specs:
            old = native('grid-original.json')
            old_map, old_programs, _, _ = identify(old, specs['grid-original.json'], hashes,
                file_hash(spec_paths['grid-original.json']), source)
            same_context(fast, old, same_jobs=True)
            if old_programs != grid_programs:
                raise ValueError('full independently recorded grid has different conditional programs')
            result['fullReferenceGrid'] = compare_mappings(fast_map, old_map)
        result['dictionary'] = dictionary(verified, programs, source, mode, generator, output, timeout)
        result.update(complete=True, status='success')
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(complete=False, status='error', error=pipeline.error_text(error))
    finally:
        try:
            result['inputsUnchanged'] = pipeline.provenance(paths) == hashes
        except OSError:
            result['inputsUnchanged'] = False
        if not result['inputsUnchanged']:
            result.update(complete=False, status='error', error='an original real-chart input changed')
        result['elapsedMs'] = (time.monotonic() - started) * 1000
        pipeline.write(output / 'validation.json', result)
    return result


def run(manifest, generator, source, output, mode='u24', verification_jobs=6,
        full_reference_grid=False, shard_index=0, shard_count=1, timeout=None):
    if mode not in storage.MODES:
        raise ValueError('unknown conditional response quantization')
    manifest, generator, output = Path(manifest).resolve(), Path(generator).resolve(), Path(output).resolve()
    declaration, inventory, data, cases = declared_cases(manifest)
    selected = shard_cases(cases, shard_index, shard_count)
    protected = {manifest, generator, *(path for case in cases for path in case['paths'].values())}
    if any(path.is_relative_to(output) for path in protected):
        raise ValueError('validation output must not contain the manifest, generator or any original input')
    if output.exists() and any(output.iterdir()):
        raise ValueError('use a new empty output for independently cold native family sessions')
    before = {path: file_hash(path) for path in protected}
    algorithm = pipeline.source_identity(source)
    specs, specification = specifications(data, verification_jobs, full_reference_grid)
    output.mkdir(parents=True, exist_ok=True)
    result = {'format': FORMAT, 'complete': False, 'wholeCatalogueComplete': False, 'status': 'running',
              'region': inventory['region'], 'datasetId': inventory['datasetId'],
              'manifestSha256': before[manifest], 'generatorSha256': before[generator],
              'algorithm': algorithm, 'sourceVersion': algorithm['simSourceSha256'],
              'runnerSha256': file_hash(__file__), 'mode': mode,
              'shard': {'index': shard_index, 'count': shard_count, 'ordering': SHARD_ORDERING,
                        'catalogueCharts': len(cases), 'selectedCharts': len(selected)},
              'options': {'verificationJobsPerChart': verification_jobs, 'fullReferenceGrid': full_reference_grid},
              'coverage': {**inventory['counts'], 'allChartsDeclared': True,
                           'allTeamCombinationsCovered': False, 'allPlayProfilesCovered': False,
                           'nonLuckPolicy': 'explicit notApplicable from unchanged actual master missions'},
              'scope': 'Complete declared regional chart/difficulty records; one fixed non-minimum kernel family per applicable chart, actual minimum levels/combinations and independent native controls. Not owned-deck search.',
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
              'changesChartsOrMissions': False, 'charts': []}
    started = time.monotonic()
    pipeline.write(output / 'validation.json', result)
    for case in selected:
        chart = case['chart']
        row = {'name': case['name'], 'scoreId': case['scoreId'], 'difficulty': chart['difficulty'],
               'missions': chart['missions'], 'inputSha256': pipeline.provenance(case['paths'])}
        result['charts'].append(row)
        if not chart['hasLuckMission']:
            row.update(status='notApplicable', complete=True, nativeCalls=0,
                       reason='actual native music missions contain no LUCK range')
        else:
            directory = output / 'charts' / str(case['scoreId'])
            value = run_case(case, specs, specification, generator, algorithm['simSourceSha256'],
                             directory, mode, timeout)
            row.update(status=value['status'], complete=value['complete'],
                       directory=directory.relative_to(output).as_posix(), receipt=receipt(directory / 'validation.json', output),
                       elapsedMs=value['elapsedMs'], error=value.get('error'))
            if value['complete']:
                row.update(grid=value['grid'], verification=value['verification'], controls=value['controls'],
                           dictionary=value['dictionary'], phases={name: {key: phase[key] for key in ('wallMs', 'nativeStats')}
                                                                  for name, phase in value['phases'].items()})
        pipeline.write(output / 'validation.json', result)
    result['summary'] = {status: sum(row['status'] == status for row in result['charts'])
                         for status in ('success', 'notApplicable', 'error')}
    result['work'] = {'fastGridJobs': sum(row.get('grid', {}).get('jobs', 0) for row in result['charts']),
                      'gridNativeRecordings': sum(row.get('grid', {}).get('familyCounts', {}).get('nativeRecordings', 0)
                                                 for row in result['charts']),
                      'originalDpVerificationCalls': sum(row.get('verification', {}).get('verifiedJobs', 0)
                                                         for row in result['charts']),
                      'conditionalPropagationCalls': sum(row.get('verification', {}).get('conditionalPropagationCalls', 0)
                                                        for row in result['charts']),
                      'regionalConditionalPrograms': sum(row.get('grid', {}).get('uniquePrograms', 0)
                                                         for row in result['charts'])}
    result['inputsUnchanged'] = all(file_hash(path) == digest for path, digest in before.items())
    result['complete'] = (len(result['charts']) == len(selected) and result['summary']['error'] == 0
                          and result['inputsUnchanged'])
    result['wholeCatalogueComplete'] = result['complete'] and shard_count == 1
    result['status'] = 'success' if result['complete'] else 'error'
    result['elapsedMs'] = (time.monotonic() - started) * 1000
    pipeline.write(output / 'validation.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('--generator', type=Path, required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--mode', choices=storage.MODES, default='u24')
    parser.add_argument('--verification-jobs', type=int, choices=range(2, 7), default=6)
    parser.add_argument('--full-reference-grid', action='store_true',
                        help='also record all 1,250 original grid jobs independently; otherwise compare the verified subset')
    parser.add_argument('--shard-index', type=int, default=0)
    parser.add_argument('--shard-count', type=int, default=1)
    parser.add_argument('--timeout-seconds', type=float)
    args = parser.parse_args()
    result = run(args.manifest, args.generator, args.source, args.output, args.mode,
                 args.verification_jobs, args.full_reference_grid, args.shard_index,
                 args.shard_count, args.timeout_seconds)
    print(json.dumps({key: result[key] for key in ('complete', 'wholeCatalogueComplete', 'summary', 'work', 'elapsedMs')}))
    return 0 if result['complete'] else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        print('luck basis family validation: ' + str(error), file=sys.stderr)
        sys.exit(2)
