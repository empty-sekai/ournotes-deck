#!/usr/bin/env python3
"""Compare rounded Miss-gauge identities against full original real-chart DPs.

The complete 625-job grid uses five actual GK8 level-3 main skills, five
GS66 level-3 minimum skills and five GS96 Miss-gauge skills. GS96 levels at
holders 0..3 vary independently; holder 4 remains level 3. Every support has
its actual formation condition unmatched. These are parameter kernels, not
owned-deck benchmarks. Six real-source witnesses exercise multiplicity and
the distinction between separately rounded increments and raw percentages.
"""
from __future__ import annotations

import argparse
from collections import Counter
import copy
import gzip
import hashlib
import importlib.util
import itertools
import json
import lzma
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
        spec = importlib.util.spec_from_file_location('luck_miss_validation_' + name,
                                                    Path(directory) / (name + '.py'))
        result = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(result)
        return result
    finally:
        if inserted:
            sys.path.remove(directory)


operators, catalogue = module('analyze_start_operators'), module('catalogue')
storage, compare, analysis = module('programs'), module('validate_interactions'), module('analyze_programs')
selected = module('validate_minimum_basis')
pipeline = storage.pipeline
FORMAT = 'ournotes-deck.luck-miss-gauge-validation/1'
CASE_FORMAT = 'ournotes-deck.luck-miss-gauge-chart/1'
CONTRACT = 'canonical-miss-gauge-deltas/1'
# All 631 original identities must fit before any probability propagation.
# The longest pinned TW chart retains about 184 MB for this complete grid.
IDENTITY_BYTES = 256 << 20
PROGRAM_BYTES = 32 << 20
WITNESSES = (('base', ()), ('two-level1', (1, 1)), ('one-level2', (2,)),
             ('level1-plus-level2', (1, 2)), ('one-level3', (3,)), ('level2-plus-level1', (2, 1)))


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def receipt(path, root):
    path = Path(path).resolve()
    return {'path': path.relative_to(Path(root).resolve()).as_posix(),
            'sha256': file_hash(path), 'bytes': path.stat().st_size}


def key(source, skill, level):
    return {'source': source, 'id': skill, 'level': level,
            'matched': False if source == 'gekisouSupport' else None}


def gauge_domain(data):
    """Separate actual master maxima from the native constructor's extra domain."""
    settings = operators.unique(operators.table(data['master'], 'MasterLiveSettings'), '_key')
    selected_rows, actual = [], []
    for name in ('gekisou_luck_gauge_max', 'gekisou_luck_gauge_max_rush'):
        row = settings[name]
        value = row['_value']
        if not isinstance(value, str) or not value.isdecimal() or not 1 <= int(value) <= 2 ** 24:
            raise ValueError('actual native gauge maximum is not a supported positive integer setting')
        actual.append(int(value))
        selected_rows.append({'id': row['_id'], 'key': name, 'value': value})
    return {'actualMasterGaugeMaxima': actual, 'masterSettingRows': selected_rows,
            'nativeConstructorAdditionalMaxima': [50, 100],
            'parameterAnalysisMaxima': sorted(set(actual) | {50, 100}),
            'scope': 'Actual master initial/default and Rush maxima plus the native constructor defaults; the native compiler independently admits its complete transcript domain.'}


def validate_sources(data):
    if data.get('format') != 'nnnotes.deck-data/1':
        raise ValueError('provide the original published nnnotes.deck-data/1 master dataset')
    chain, unsupported = catalogue.all_chain_keys(data)
    available = {catalogue.canonical(item) for item in chain}
    requested = [key('gekisou', 8, 3), key('gekisouSupport', 66, 3)]
    requested += [key('gekisouSupport', 96, level) for level in range(1, 6)]
    for item in requested:
        if (catalogue.canonical(item) not in available
                or any(all(row.get(field) == item[field] for field in ('source', 'id', 'level'))
                       for row in unsupported)):
            raise ValueError('a requested actual source is absent or has conflicting native formation targets')
    master = operators.Catalogue(data['master'])
    main = master.sources['gekisou', 8, 3][1]
    if not main or any(row['_skillEffectType'] != 11001 for row in main):
        raise ValueError('actual GK8 level 3 is no longer the fixed gauge-speed source')
    minimum = master.start('gekisouSupport', 66, 3, False, 1000)
    kind, value, probability = minimum['action']
    if kind != 'minimum' or value != 1 or not 0 < probability < 1 or minimum['band'] is None:
        raise ValueError('actual unmatched GS66 level 3 changed its minimum guarantee domain')
    domain = gauge_domain(data)
    actual = []
    for level in range(1, 6):
        metadata, rows = master.sources['gekisouSupport', 96, level]
        band = master.formation_band(rows)
        if (metadata['_gekisouMissionType'] != 2 or metadata['_gekisouSupportSkillExecTiming'] != 1
                or band != minimum['band'] or master.phases[11003]['_phase'] != 2):
            raise ValueError('actual GS96 and GS66 no longer have compatible LUCK timing and formation targets')
        enabled = []
        for row in rows:
            trigger = master.group(row['_skillTriggerConditionGroup'])
            if (len(trigger) != 1 or trigger[0]['_conditionType'] != 7000
                    or trigger[0]['_conditionValues'] != [0] or trigger[0]['_isPositive'] is not True
                    or trigger[0]['_conditionTargetIDs']):
                raise ValueError('actual GS96 trigger is not the direct native Miss-lottery result')
            master.direct(row['_skillReleaseConditionGroup'], 7013)
            master.direct(row['_effectExecuteLimitResetConditionGroup'], 7013)
            if (row['_skillEffectType'] != 11003 or row['_skillTriggerType'] != 1
                    or row['_activationTimeSecond'] != 0 or row['_effectExecuteLimitCount'] != 1
                    or row['_effectLimitCount'] != 0 or row['_skillTargetIDs']
                    or row['_maxEffectValue'] != 0 or row['_skillCumulativeConditionID'] != 0):
                raise ValueError('actual GS96 changed its once-per-range Miss-gauge action shape')
            conditions = master.group(row['_skillConditionGroup'])
            if len(conditions) != 1 or conditions[0]['_conditionType'] != 5000:
                raise ValueError('GS96 has a new dependency beyond its original formation predicate')
            chance = master.chance(row['_skillConditionGroup'], 1000, False)
            if chance not in (0, 1):
                raise ValueError('GS96 unmatched branch is no longer deterministic')
            if chance:
                enabled.append(row)
        if len(enabled) != 1 or enabled[0]['_effectValue'] != (500, 1000, 1500, 2000, 3500)[level - 1]:
            raise ValueError('actual unmatched GS96 values changed the declared complete level grid/witnesses')
        raw = enabled[0]['_effectValue']
        actual.append({'key': key('gekisouSupport', 96, level), 'band': band,
                       'allRowIds': [row['_id'] for row in rows], 'activeRowId': enabled[0]['_id'],
                       'rawEffectValue': raw,
                       'separatelyRoundedDeltas': [operators.native_gauge_delta(maximum, raw)
                                                  for maximum in domain['parameterAnalysisMaxima']]})
    return {'selectedKeys': requested, 'minimum': {'key': minimum['key'], 'band': minimum['band'],
        'minimum': value, 'probability': operators.rational(probability), 'rowIds': minimum['rowIds']},
        'missLevels': actual, 'gaugeDomain': domain,
        'declaredGaugeMaximaForParameterAnalysis': domain['parameterAnalysisMaxima'],
        'parameterAnalysisIsNativeAdmissionProof': False}


def miss_job(name, levels):
    entries = []
    for position in range(5):
        entries.extend([[key('gekisou', 8, 3), position], [key('gekisouSupport', 66, 3), position]])
        if position < len(levels):
            entries.append([key('gekisouSupport', 96, levels[position]), position])
    return {'name': name, 'entries': entries}


def make_specs(data):
    receipt_value = validate_sources(data)
    grid = [miss_job('miss-grid-' + ''.join(map(str, levels)), levels + (3,))
            for levels in itertools.product(range(1, 6), repeat=4)]
    witnesses = [miss_job('miss-witness-' + name, levels) for name, levels in WITNESSES]
    jobs = grid + witnesses
    if len(grid) != 625 or len({job['name'] for job in jobs}) != 631:
        raise ValueError('Miss experiment omitted a grid level or original witness')
    available = {catalogue.canonical(item) for item in catalogue.all_chain_keys(data)[0]}
    for job in jobs:
        held = [[] for _ in range(5)]
        for item, position in job['entries']:
            if catalogue.canonical(item) not in available or type(position) is not int or not 0 <= position < 5:
                raise ValueError('Miss experiment inserted a non-native skill key or position')
            held[position].append(item)
        if (any(sum(item['source'] == 'gekisou' for item in items) != 1
                or sum(item['source'] == 'gekisouSupport' for item in items) > 2 for items in held)
                or len(job['entries']) > 15):
            raise ValueError('Miss experiment violates a native holder capacity')
    maxima = receipt_value['declaredGaugeMaximaForParameterAnalysis']
    vectors = {row['key']['level']: row['separatelyRoundedDeltas'] for row in receipt_value['missLevels']}
    classes = Counter(tuple(sum(vectors[level][dimension] for level in levels + (3,)) for dimension in range(len(maxima)))
                      for levels in itertools.product(range(1, 6), repeat=4))
    two_level1 = [2 * delta for delta in vectors[1]]
    level1_plus_level2 = [left + right for left, right in zip(vectors[1], vectors[2])]
    if two_level1 == vectors[2] or level1_plus_level2 != vectors[3]:
        raise ValueError('the declared real-source rounding witnesses no longer hold over the actual native gauge domain')
    receipt_value.update(format='ournotes-deck.luck-miss-gauge-specs/1',
        scope='Actual master parameter kernels on unchanged real charts; not owned decks or a full search benchmark.',
        grid={'jobs': 625, 'mainWriters': 5, 'supportWriters': 10, 'matched': False,
              'variableGS96Holders': [0, 1, 2, 3], 'levels': [1, 2, 3, 4, 5], 'fixedGS96Holder4Level': 3},
        witnesses={'jobs': 6, 'holdersForAdditionalGS96': [0, 1], 'minimumWritersRemainFixed': 5},
        witnessArithmetic={'gaugeMaxima': maxima, 'twoLevel1': two_level1, 'oneLevel2': vectors[2],
                           'level1PlusLevel2': level1_plus_level2, 'oneLevel3': vectors[3]},
        parameterClassesAtDeclaredGaugeMaxima={'classes': len(classes), 'maximumMultiplicity': max(classes.values()),
            'nativeCompleteProgramEqualityNotAssumed': True})
    specs = {}
    for name, canonical in (('original.json', False), ('canonical.json', True)):
        specs[name] = {'mode': 'programs', 'jobs': copy.deepcopy(jobs),
                       'identityBytes': IDENTITY_BYTES, 'programBytes': PROGRAM_BYTES,
                       'canonicalMissGauge': canonical, 'canonicalStartMinimum': False,
                       'mcRuns': 0, 'scoreSamples': 0}
    return specs, receipt_value


def program_report(report, spec, hashes, spec_sha, source, *, propagates=True):
    """Validate the same complete native protocol for admission and propagation."""
    compare.validate_provenance(report, hashes, spec_sha)
    jobs, canonical = spec['jobs'], spec['canonicalMissGauge']
    mode, status = ('programs', 'success') if propagates else ('identify', 'identified')
    contract = CONTRACT if canonical else 'native-ordered-actions/1'
    if (type(propagates) is not bool or type(canonical) is not bool or not jobs
            or spec.get('mode') != mode or spec.get('identityBytes') != IDENTITY_BYTES
            or spec.get('programBytes') != PROGRAM_BYTES or spec.get('canonicalStartMinimum') is not False
            or report.get('format') != storage.REPORT or report.get('mode') != mode
            or report.get('sourceVersion') != source or report.get('canonicalMissGauge') is not canonical
            or report.get('canonicalStartMinimum') is not False
            or compare.source_version(report) != source):
        raise ValueError('original/canonical programs do not match the requested native mode, source and allowances')
    storage.digest(report['context']['fingerprint'])
    stats = report.get('stats', {})
    if not propagates:
        if (type(stats.get('propagationCalls')) is not int or stats['propagationCalls'] != 0
                or stats.get('propagationMs') != 0):
            raise ValueError('Miss identity preflight must perform zero DP propagations')
        refused = [row for row in report.get('jobs', []) if row.get('status') != 'identified']
        capacity = [row for row in refused if row.get('status') == 'capacity']
        if capacity:
            first = capacity[0]
            raise ValueError(f'Miss identity preflight capacity refusal before any DP: '
                             f'{len(capacity)}/{len(jobs)} jobs; identityBytes={IDENTITY_BYTES}, '
                             f'programBytes={PROGRAM_BYTES}; first {first.get("name")}: {first.get("error")}')
    if (any(report.get(field) is not True for field in ('complete', 'identificationComplete'))
            or report.get('probabilityComplete') is not propagates):
        raise ValueError('complete original/canonical programs do not match the requested native completion phase')
    programs = report.get('programs', [])
    if not isinstance(programs, list) or not 1 <= len(programs) <= len(jobs):
        raise ValueError('native Miss experiment has no bounded program set')
    fingerprints = []
    rows = pipeline.job_results(report, jobs)
    for ordinal, program in enumerate(programs):
        fingerprints.append(storage.digest(program.get('fingerprint')))
        representative = program.get('representativeJobIndex')
        if (type(program.get('programIndex')) is not int or program['programIndex'] != ordinal
                or program.get('sourceVersion') != source or program.get('status') != status
                or program.get('operatorContract') != contract
                or type(representative) is not int or not 0 <= representative < len(jobs)
                or rows[jobs[representative]['name']].get('programIndex') != ordinal):
            raise ValueError('a native Miss representative is missing its requested status, source or contract')
        if propagates:
            if not isinstance(program.get('response'), dict) or program['response'].get('status') != 'success':
                raise ValueError('a native Miss representative lacks its original probability response')
            compare.validate_curve(program['response']['curve'])
        elif ('response' not in program or program['response'] is not None
              or program.get('propagationMs', 0) != 0):
            raise ValueError('Miss identity preflight must retain identified programs with null responses and zero DP')
    if len(set(fingerprints)) != len(fingerprints):
        raise ValueError('native Miss report repeats a complete program identity')
    used, mapping = set(), {}
    for ordinal, job in enumerate(jobs):
        row = rows[job['name']]; index = row.get('programIndex')
        if (type(index) is not int or not 0 <= index < len(programs)
                or type(row.get('jobIndex')) is not int or row['jobIndex'] != ordinal
                or row.get('programFingerprint') != fingerprints[index] or row.get('status') != status
                or row.get('operatorContract') != contract):
            raise ValueError('a requested actual skill configuration lacks its original/canonical native program')
        used.add(index)
        mapping[job['name']] = {'fingerprint': fingerprints[index],
                              'curve': programs[index]['response']['curve'] if propagates else None}
    counters = ('sourceJobs', 'compiledJobs', 'uniqueRetainedPrograms', 'propagationCalls', 'exactProgramAliases',
                'identityBudgetBytes', 'programBudgetBytes', 'retainedIdentityBytes',
                'compiledProgramPeakBytes', 'temporaryIdentityPeakBytes')
    if (any(type(stats.get(field)) is not int or stats[field] < 0 for field in counters)
            or used != set(range(len(programs))) or stats.get('sourceJobs') != len(jobs)
            or stats.get('compiledJobs') != len(jobs) or stats.get('uniqueRetainedPrograms') != len(programs)
            or stats.get('propagationCalls') != (len(programs) if propagates else 0)
            or stats.get('exactProgramAliases') != len(jobs) - len(programs)
            or stats.get('identityBudgetBytes') != IDENTITY_BYTES
            or stats.get('programBudgetBytes') != PROGRAM_BYTES
            or not 0 < stats['retainedIdentityBytes'] <= IDENTITY_BYTES
            or not 0 < stats['compiledProgramPeakBytes'] <= PROGRAM_BYTES
            or not 0 < stats['temporaryIdentityPeakBytes'] <= IDENTITY_BYTES):
        raise ValueError('native Miss work counters omit jobs, propagations or the explicit identity allowance')
    for row in [*programs, *rows.values()]:
        if (type(row.get('compiledBytes')) is not int or not 0 < row['compiledBytes'] <= stats['compiledProgramPeakBytes']
                or type(row.get('identityBytes')) is not int
                or not 0 < row['identityBytes'] <= stats['temporaryIdentityPeakBytes']):
            raise ValueError('native Miss program identity/compiled bytes exceed the reported budget peaks')
    return mapping


def bind_preflight(identified_report, identified, propagated_report, propagated):
    """Bind every expensive response to its already admitted complete identity."""
    for field in ('context', 'sharedFingerprint', 'dependencyDescriptor', 'capabilities'):
        if identified_report.get(field) != propagated_report.get(field):
            raise ValueError('Miss propagation changed its preflight native input or selected-source dependency')
    if not identified or set(identified) != set(propagated):
        raise ValueError('Miss propagation changed its complete preflight job set')
    for name, value in identified.items():
        if value['fingerprint'] != propagated[name]['fingerprint']:
            raise ValueError('Miss propagation changed its preflight program fingerprint: ' + name)
    return {'complete': True, 'jobs': len(identified), 'allProgramFingerprintsMatch': True}


def compare_probability_curves(original, canonical):
    result = compare.compare_curves(original, canonical)
    times = sorted({step['timeMs'] for curve in (original, canonical) for step in curve['steps']})
    cursor, current = [0, 0], [compare.EMPTY, compare.EMPTY]
    maximum, disjoint = 0.0, 0
    for point in times:
        for index, curve in enumerate((original, canonical)):
            while cursor[index] < len(curve['steps']) and curve['steps'][cursor[index]]['timeMs'] <= point:
                current[index] = curve['steps'][cursor[index]]['buckets']; cursor[index] += 1
        for before, after in zip(*current):
            maximum = max(maximum, *(abs(a - b) for a, b in zip(before, after)))
            disjoint += before[0] > after[1] or after[0] > before[1]
    result.update(maximumEndpointDifference=maximum, disjointJointIntervals=disjoint)
    return result


def compare_jobs(original, canonical):
    if set(original) != set(canonical) or not original:
        raise ValueError('original/canonical comparison omits a requested actual configuration')
    comparisons = []
    for name in original:
        result = compare_probability_curves(original[name]['curve'], canonical[name]['curve'])
        if any(result[field] is not True for field in
               ('jointIntervalsOverlap', 'weightedIntervalsOverlap', 'sameTransitionMasks',
                'sameProbeFlags', 'sameRangeMoments')):
            raise ValueError('canonical Miss identity disagrees with an original native DP: ' + name)
        comparisons.append({'name': name, **result})
    return {'comparedJobs': len(comparisons), 'jointIntervals': sum(row['jointIntervals'] for row in comparisons),
            'disjointJointIntervals': sum(row['disjointJointIntervals'] for row in comparisons),
            'maximumEndpointDifference': max(row['maximumEndpointDifference'] for row in comparisons),
            'allJointEndpointsEqual': all(row['sameJointEndpoints'] for row in comparisons),
            'allProbeFlagsAndTransitionMasksEqual': True, 'comparisons': comparisons}


def witness_gate(mapping):
    names = {'miss-witness-' + name for name, _ in WITNESSES}
    if not names <= set(mapping):
        raise ValueError('canonical Miss witness coverage is incomplete')
    fp = {name.removeprefix('miss-witness-'): mapping[name]['fingerprint'] for name in names}
    if fp['two-level1'] == fp['one-level2']:
        raise ValueError('raw percentages were added before native rounding in the canonical identity')
    if not fp['level1-plus-level2'] == fp['one-level3'] == fp['level2-plus-level1']:
        raise ValueError('equal separately rounded native Miss increments did not share a complete identity')
    return {'rawSumCounterexampleSeparated': True, 'nativeDeltaEquivalenceShared': True,
            'fingerprints': fp, 'rawSumCounterexampleCurveDifference': compare_probability_curves(
                mapping['miss-witness-two-level1']['curve'], mapping['miss-witness-one-level2']['curve']),
            'counterexampleScope': 'Native identity distinguishes the admitted gauge domains; an observable difference on each chart is measured, not assumed.'}


def pack(report_path, report, generator, output, source, mode, timeout):
    storage.call([generator, 'program-pack', report_path, mode, output], output.parent / (output.name + '-pack.log'), timeout)
    index_path = output / 'program-index.json'
    rows = storage.load_index(index_path, source, mode)
    if set(rows) != {program['fingerprint'] for program in report['programs']}:
        raise ValueError('native archive changed the complete Miss program set')
    blobs = {}
    for row in rows.values():
        path, raw = storage.verified_blob(output, row, source, mode)
        if not raw.startswith(b'ONLRSP02'):
            raise ValueError('Miss program blob has an unknown native encoding')
        blobs[row['archive']['sha256']] = path
    compressed = analysis.compression(index_path.read_bytes(), blobs, ('gzip9', 'xz6'), output)
    for method, opener in (('gzip9', gzip.open), ('xz6', lzma.open)):
        digest, length = hashlib.sha256(), 0
        with opener(output / compressed[method]['artifact'], 'rb') as stream:
            while chunk := stream.read(1024 * 1024):
                digest.update(chunk); length += len(chunk)
        if length != compressed['tar']['bytes'] or digest.hexdigest() != compressed['tar']['sha256']:
            raise ValueError('Miss program dictionary did not round-trip to the exact original TAR')
        compressed[method]['roundTripVerified'] = True
    return {'mode': mode, 'programs': len(rows), 'uniqueBlobs': len(blobs), 'index': receipt(index_path, output.parent),
            'indexAndUniqueBlobBytes': index_path.stat().st_size + sum(path.stat().st_size for path in blobs.values()),
            'compression': compressed, 'globalSkillCombinationAliasesStored': False}


def run_program_phases(specs, paths, generator, source, hashes, output, result, timeout=None):
    """Complete both zero-DP admissions before starting either expensive run."""
    phases = ('original', 'canonical')
    if (set(specs) != {phase + '.json' for phase in phases}
            or specs['original.json']['jobs'] != specs['canonical.json']['jobs']):
        raise ValueError('original/canonical Miss phases must request the same complete job set')
    preflight_specs = {}
    for phase in phases:
        spec = specs[phase + '.json']
        pipeline.write(output / 'specs' / (phase + '.json'), spec)
        name = phase + '-preflight'
        preflight_specs[phase] = {**copy.deepcopy(spec), 'mode': 'identify'}
        spec_path = output / 'specs' / (name + '.json')
        pipeline.write(spec_path, preflight_specs[phase])
        result['phases'][name] = {'spec': receipt(spec_path, output), 'status': 'pending',
                                 'mode': 'identify', 'propagates': False}
    result['preflight'] = {'complete': False, 'requiredPhases': [phase + '-preflight' for phase in phases]}
    pipeline.write(output / 'validation.json', result)

    def run_phase(name, spec, propagates, previous=None):
        spec_path, report_path = output / 'specs' / (name + '.json'), output / (name + '.json')
        phase = {'spec': receipt(spec_path, output), 'status': 'running',
                 'mode': spec['mode'], 'propagates': propagates}
        result['phases'][name] = phase
        pipeline.write(output / 'validation.json', result)
        began = time.monotonic()
        try:
            storage.call([generator, paths['data'], paths['snapshot'], paths['request'], spec_path, report_path],
                         report_path.with_suffix('.log'), timeout)
            report = pipeline.read(report_path)
            phase.update(report=receipt(report_path, output), nativeStats=report.get('stats'))
            mapping = program_report(report, spec, hashes, file_hash(spec_path), source, propagates=propagates)
            if previous is not None:
                phase['preflightBinding'] = bind_preflight(*previous, report, mapping)
            phase['status'] = 'success'
            return report, mapping
        except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
            phase.update(status='error', error=pipeline.error_text(error))
            raise
        finally:
            phase['wallMs'] = (time.monotonic() - began) * 1000
            pipeline.write(output / 'validation.json', result)

    preflights = {phase: run_phase(phase + '-preflight', preflight_specs[phase], False) for phase in phases}
    for field in ('context', 'sharedFingerprint', 'dependencyDescriptor', 'capabilities'):
        if preflights['original'][0].get(field) != preflights['canonical'][0].get(field):
            raise ValueError('original/canonical preflights changed a resolved native input or selected-source dependency')
    result['preflight'].update(complete=True, propagationCalls=0,
                              identifiedJobs=sum(len(value[1]) for value in preflights.values()),
                              bothPassedBeforeAnyPropagation=True)
    pipeline.write(output / 'validation.json', result)
    reports, mappings = {}, {}
    for phase in phases:
        reports[phase], mappings[phase] = run_phase(phase, specs[phase + '.json'], True, preflights[phase])
    return reports, mappings


def run_case(case, paths, generator, source, data, output, mode, timeout=None):
    hashes = pipeline.provenance(paths)
    output.mkdir(parents=True, exist_ok=True)
    result = {'format': CASE_FORMAT, 'name': case['name'], 'scoreId': case.get('scoreId'),
              'region': data.get('provenance', {}).get('region'), 'sourceVersion': source,
              'inputSha256': hashes, 'complete': False, 'status': 'running', 'phases': {},
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False}
    started = time.monotonic()
    try:
        specs, recipe = make_specs(data)
        pipeline.write(output / 'specs/receipt.json', recipe)
        results, mappings = run_program_phases(specs, paths, generator, source, hashes, output, result, timeout)
        for field in ('context', 'sharedFingerprint', 'dependencyDescriptor', 'capabilities'):
            if results['original'].get(field) != results['canonical'].get(field):
                raise ValueError('original/canonical runs changed a resolved native input or selected-source dependency')
        original, canonical = mappings['original'], mappings['canonical']
        grid = [name for name in original if name.startswith('miss-grid-')]
        if len(grid) != 625 or len({original[name]['fingerprint'] for name in grid}) != 625:
            raise ValueError('complete 625-grid reference did not independently propagate every original native program')
        result['comparison'] = compare_jobs(original, canonical)
        result['witnesses'] = witness_gate(canonical)
        result['grid'] = {'jobs': 625, 'originalIndependentPrograms': 625,
                          'canonicalPrograms': len({canonical[name]['fingerprint'] for name in grid}),
                          'parameterClassesAtDeclaredGaugeMaxima': recipe['parameterClassesAtDeclaredGaugeMaxima']['classes'],
                          'parameterAnalysisGaugeMaxima': recipe['declaredGaugeMaximaForParameterAnalysis']}
        result['dictionaries'] = {}
        for phase, report in results.items():
            result['dictionaries'][phase] = pack(output / (phase + '.json'), report, generator,
                output / ('dictionary-' + phase), source, mode, timeout)
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


def default_cases(manifest):
    names = {case['name'] for case in pipeline.read(manifest)['cases']}
    for candidates in (['short-newcomer-score', 'long-newcomer-score'],
                       ['tw-10006103-expert', 'tw-10010703-expert'], ['jp-10000300-easy']):
        if set(candidates) <= names:
            return candidates
    raise ValueError('select exact original --case names for this declared manifest')


def run(manifest, names, generator, source, output, mode='u24', timeout=None):
    manifest, generator, output = Path(manifest).resolve(), Path(generator).resolve(), Path(output).resolve()
    if mode not in storage.MODES:
        raise ValueError('unknown native dictionary encoding')
    names = names or default_cases(manifest)
    dataset, cases = selected.selected_cases(manifest, names)
    inputs = [{field: (manifest.parent / case[field]).resolve() for field in ('data', 'snapshot', 'request')}
              for case in cases]
    protected = {manifest, generator, *(path for paths in inputs for path in paths.values())}
    if any(path.is_relative_to(output) for path in protected):
        raise ValueError('Miss validation output must not contain any original input or generator')
    if output.exists() and any(output.iterdir()):
        raise ValueError('use a new empty output for independently cold original and canonical runs')
    before = {path: file_hash(path) for path in protected}
    algorithm = pipeline.source_identity(source)
    output.mkdir(parents=True, exist_ok=True)
    result = {'format': FORMAT, 'complete': False, 'status': 'running', 'mode': mode,
              'manifestSha256': before[manifest], 'generatorSha256': before[generator], 'datasetId': dataset,
              'algorithm': algorithm, 'sourceVersion': algorithm['simSourceSha256'], 'selectedCases': names,
              'sourceSha256': {name: file_hash(Path(__file__).parent / name) for name in
                              ('validate_miss_gauge.py', 'analyze_start_operators.py', 'catalogue.py',
                               'validate_interactions.py', 'programs.py', 'analyze_programs.py')},
              'scope': '625 full-capacity actual-master Miss/minimum/speed kernels plus six actual-source witnesses per selected unchanged real chart; not owned-deck search or all-skill-combination coverage.',
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
              'changesChartsOrMissions': False, 'cases': []}
    started, loaded = time.monotonic(), {}
    try:
        for index, (case, paths) in enumerate(zip(cases, inputs)):
            if before[paths['data']] != dataset:
                raise ValueError('real chart dataset differs from the declared original manifest pin')
            if paths['data'] not in loaded:
                loaded[paths['data']] = pipeline.read(paths['data'])
            if not pipeline.luck_case(loaded[paths['data']], pipeline.read(paths['request'])):
                raise ValueError('selected original real chart has no LUCK mission')
            directory = output / 'cases' / f'{index:03d}'
            outcome = run_case(case, paths, generator, algorithm['simSourceSha256'], loaded[paths['data']], directory, mode, timeout)
            result['cases'].append({'name': case['name'], 'directory': directory.relative_to(output).as_posix(),
                'complete': outcome['complete'], 'status': outcome['status'], 'grid': outcome.get('grid'),
                'comparison': {key: value for key, value in outcome.get('comparison', {}).items() if key != 'comparisons'},
                'dictionaries': outcome.get('dictionaries'), 'error': outcome.get('error'),
                'elapsedMs': outcome['elapsedMs'], 'receipt': receipt(directory / 'validation.json', output)})
            pipeline.write(output / 'validation.json', result)
        result['complete'] = len(result['cases']) == len(cases) and all(case['complete'] for case in result['cases'])
        result['status'] = 'success' if result['complete'] else 'error'
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(complete=False, status='error', error=pipeline.error_text(error))
    finally:
        result['inputsUnchanged'] = all(file_hash(path) == digest for path, digest in before.items())
        if not result['inputsUnchanged']:
            result.update(complete=False, status='error', error='an original input, manifest or generator changed')
        result['elapsedMs'] = (time.monotonic() - started) * 1000
        pipeline.write(output / 'validation.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('--case', dest='cases', action='append', help='exact original manifest case name; repeat as needed')
    parser.add_argument('--generator', type=Path, required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--mode', choices=storage.MODES, default='u24')
    parser.add_argument('--timeout-seconds', type=float)
    args = parser.parse_args()
    result = run(args.manifest, args.cases, args.generator, args.source, args.output, args.mode, args.timeout_seconds)
    print(json.dumps({key: result[key] for key in ('complete', 'status', 'elapsedMs')}))
    return 0 if result['complete'] else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        print('luck Miss-gauge validation: ' + str(error), file=sys.stderr)
        sys.exit(2)
