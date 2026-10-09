#!/usr/bin/env python3
"""Validate actual integer gauge-speed profiles on one unchanged real chart.

The complete grid contains zero, one or two of the twenty actual speed
source/level keys, including ordered pairs and multiplicity. The native
compiler records all 421 inputs and compares complete programs. Eighteen
selected original inputs then run independent, uncached probability DPs.
The separate five-slot vector count is parameter arithmetic, not a count of
legal physical decks or precomputed responses.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
import itertools
import json
from pathlib import Path
import struct
import subprocess
import sys
import time

import validate_miss_gauge as protocol

operators, catalogue = protocol.operators, protocol.catalogue
pipeline, storage, compare = protocol.pipeline, protocol.storage, protocol.compare
FORMAT = 'ournotes-deck.luck-speed-profile-validation/1'
DURATIONS = (2, 3, 4, 5, 6, 0)
EXPECTED = {(skill, level): (factor, level + 1 if level < 5 else 0)
            for skill, factor in ((7, 1), (8, 2), (9, 3)) for level in range(1, 6)}
EXPECTED.update({(22, level): (2, level + 2 if level < 5 else 0) for level in range(1, 6)})
ALIASES = (
    (((7, 1), (7, 1)), ((8, 1),)),
    (((7, 1), (8, 1)), ((9, 1),)),
    (((7, 5), (8, 5)), ((9, 5),)),
    (((8, 2),), ((22, 1),)),
    (((8, 3),), ((22, 2),)),
    (((8, 4),), ((22, 3),)),
    (((8, 5),), ((22, 5),)),
    (((7, 1), (7, 4)), ((7, 4), (7, 1))),
)
COUNTEREXAMPLE = (((7, 1), (7, 4)), ((7, 2), (7, 3)))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def name(keys):
    return 'speed-' + ('-'.join(f'{skill}l{level}' for skill, level in keys) or 'base')


def profile(keys, descriptors):
    vector = [0] * len(DURATIONS)
    for key in keys:
        factor, duration = descriptors[key]
        vector[DURATIONS.index(duration)] += factor
    return tuple(vector)


def direct_luck_trigger(master, group, kind):
    rows = master.group(group)
    require(len(rows) == 1, 'speed needs one direct native mission trigger')
    row = rows[0]
    require(row['_conditionType'] == kind and row['_isPositive'] is True
            and not row['_conditionValues'] and row['_conditionTargetIDs'],
            'speed trigger differs from the declared positive LUCK trigger')
    for identity in row['_conditionTargetIDs']:
        target = master.targets[identity]
        require(target['_skillTargetType'] == 5 and target['_gekisouMissionType'] == 2
                and not any(target.get(field) for field in
                            ('_characterID', '_bandID', '_cardType', '_tagID',
                             '_liveSkillCategories', '_gekisouSkillCategories')),
                'speed trigger has another target dependency')


def source_row(master, metadata, rows):
    speed = [row for row in rows if row['_skillEffectType'] == 11001]
    require(metadata['_gekisouMissionType'] == 2 and len(speed) == 1
            and master.phases[11001]['_phase'] == 2,
            'speed source does not have one phase-2 LUCK speed effect')
    row = speed[0]
    others = [effect for effect in rows if effect is not row]
    if metadata['_id'] == 22:
        # This complete source also reduces damage. The speed-only kernel has
        # no LIFE reader; a controller containing one must retain that row.
        ignored = {'_id', '_skillEffectType', '_effectValue'}
        require(len(others) == 1 and others[0]['_skillEffectType'] == 3004
                and others[0]['_effectValue'] == 2000 and master.phases[3004]['_phase'] == 2
                and {key: value for key, value in others[0].items() if key not in ignored}
                == {key: value for key, value in row.items() if key not in ignored},
                'the complete GK22 source changed its accompanying damage-reduction row')
    else:
        require(not others, 'speed source has an additional native effect')
    require(row['_skillEffectType'] == 11001 and not row['_skillTargetIDs']
            and not master.group(row['_skillConditionGroup'])
            and all(row[field] == 0 for field in
                    ('_maxEffectValue', '_effectLimitCount', '_skillCumulativeConditionID',
                     '_effectExecuteLimitCount', '_effectExecuteLimitResetConditionGroup')),
            'speed source has another action, condition, target or state dependency')
    value, duration = row['_effectValue'], row['_activationTimeSecond']
    require(type(value) is int and value in (10000, 20000, 30000)
            and type(duration) in (int, float) and duration in DURATIONS,
            'speed factor or duration leaves the declared integer domain')
    factor = value // 10000
    require(operators.binary32(operators.binary32(value) / 10000) == factor,
            'native binary32 speed factor is not the declared exact integer')
    if duration:
        require(row['_skillTriggerType'] == 1 and row['_skillReleaseConditionGroup'] == 0,
                'timed speed has a release or another trigger lifetime')
        direct_luck_trigger(master, row['_skillTriggerConditionGroup'], 7010)
    else:
        require(row['_skillTriggerType'] == 2, 'zero-duration speed must use the untimed sustained path')
        direct_luck_trigger(master, row['_skillTriggerConditionGroup'], 7020)
        if row['_skillReleaseConditionGroup']:
            master.direct(row['_skillReleaseConditionGroup'], 7013)
    return factor, int(duration)


def sources(data):
    require(data.get('format') == 'nnnotes.deck-data/1', 'provide the original published deck-data dataset')
    master = operators.Catalogue(data['master'])
    selected = {key: value for key, value in master.sources.items()
                if any(row['_skillEffectType'] == 11001 for row in value[1])}
    require(set(selected) == {('gekisou', *key) for key in EXPECTED},
            'actual speed catalogue differs from the declared complete twenty-key domain')
    descriptors, records = {}, []
    for (_, skill, level), (metadata, rows) in sorted(selected.items()):
        descriptor = source_row(master, metadata, rows)
        require(descriptor == EXPECTED[skill, level], 'a declared actual speed source changed factor or duration')
        descriptors[skill, level] = descriptor
        row = next(row for row in rows if row['_skillEffectType'] == 11001)
        records.append({'key': protocol.key('gekisou', skill, level), 'rows': rows,
                        'factor': descriptor[0], 'durationSeconds': descriptor[1],
                        'activationBits': struct.pack('<f', row['_activationTimeSecond']).hex(),
                        'trigger': master.group(row['_skillTriggerConditionGroup']),
                        'release': master.group(row['_skillReleaseConditionGroup'])})
    available, unsupported = catalogue.all_chain_keys(data)
    available = {pipeline.canonical(key) for key in available}
    require(all(pipeline.canonical(row['key']) in available for row in records)
            and not any(row['source'] == 'gekisou' and (row['id'], row['level']) in EXPECTED
                        for row in unsupported), 'a speed key is absent or has incompatible formation dependencies')
    cards = operators.table(data['master'], 'MasterMemberCard')
    held = {skill: sorted(row['_id'] for row in cards if row['_gekisouSkillID'] == skill)
            for skill in sorted({key[0] for key in EXPECTED})}
    return descriptors, {'sourceLevels': records, 'sourceLevelCount': len(records),
                         'distinctSingleProfiles': len(set(descriptors.values())),
                         'masterCardIdsBySource': held, 'speedCardCount': sum(map(len, held.values())),
                         'durationBinsSeconds': list(DURATIONS), 'zeroDurationMeaning': 'untimedRangePlaying',
                         'lifeDependentAdditionalEffectSources': [22],
                         'additionalEffectScope': 'GK22 also has native 3004 damage reduction. It remains a separate dependency whenever a controller reads LIFE; this experiment selects only speed sources.',
                         'nativeAdmissionProof': False}


def vector_space(descriptors, max_slots=5):
    require(type(max_slots) is int and 0 <= max_slots <= 5, 'vector enumeration supports at most five holders')
    vectors = {profile((key,), descriptors) for key in descriptors}
    exact, union, counts = {(0,) * len(DURATIONS)}, {(0,) * len(DURATIONS)}, []
    for count in range(1, max_slots + 1):
        exact = {tuple(a + b for a, b in zip(previous, addition))
                 for previous in exact for addition in vectors}
        union |= exact
        counts.append({'holders': count, 'exactly': len(exact), 'upTo': len(union)})
    return {'singleProfiles': len(vectors), 'counts': counts, 'includingEmpty': len(union),
            'orderedSourceLevelSelectionsWithEmptySlots': (len(descriptors) + 1) ** max_slots,
            'nativeProgramsCounted': False, 'physicalDeckLegalityChecked': False,
            'description': 'Exact integer parameter-vector enumeration; chart timing and all other controller inputs remain separate.'}


def make_specs(data):
    descriptors, receipt = sources(data)
    keys = sorted(descriptors)
    combinations = [()] + [(key,) for key in keys] + list(itertools.product(keys, repeat=2))
    jobs = [{'name': name(items), 'entries': [[protocol.key('gekisou', skill, level), position]
                                           for position, (skill, level) in enumerate(items)]}
            for items in combinations]
    require(len(jobs) == 421 and len({job['name'] for job in jobs}) == 421,
            'speed grid does not contain all zero/single/ordered-pair selections')
    wanted = {(), *COUNTEREXAMPLE, *(entry for pair in ALIASES for entry in pair)}
    references = [job for items, job in zip(combinations, jobs) if items in wanted]
    require(len(references) == 18, 'speed reference witnesses changed')
    profiles = {job['name']: profile(items, descriptors) for items, job in zip(combinations, jobs)}
    receipt['parameterVectorSpace'] = vector_space(descriptors)
    receipt['grid'] = {'jobs': len(jobs), 'maximumSpeedHolders': 2, 'positions': [0, 1],
                       'orderedPairs': len(keys) ** 2, 'parameterProfiles': len(set(profiles.values()))}
    identify = {'mode': 'identify', 'jobs': jobs, 'identityBytes': protocol.IDENTITY_BYTES,
                'programBytes': protocol.PROGRAM_BYTES, 'canonicalStartMinimum': False,
                'canonicalMissGauge': False, 'mcRuns': 0, 'scoreSamples': 0}
    reference = {'mode': 'generate', 'jobs': references, 'cacheBytes': 0, 'mcRuns': 0, 'scoreSamples': 0}
    return identify, reference, profiles, receipt


def identity_gate(report, spec, profiles, hashes, spec_sha, source):
    mapping = protocol.program_report(report, spec, hashes, spec_sha, source, propagates=False)
    require(set(profiles) == set(mapping), 'profile labels differ from the complete native grid')
    rows = pipeline.job_results(report, spec['jobs'])
    grouped = defaultdict(list)
    for label, vector in profiles.items():
        grouped[vector].append(label)
    require(all(len({rows[label]['programIndex'] for label in labels}) == 1 for labels in grouped.values()),
            'equal integer speed profiles produced different complete native programs')
    for left, right in ALIASES:
        require(rows[name(left)]['programIndex'] == rows[name(right)]['programIndex'],
                'an actual speed alias failed native complete-program equality')
    left, right = map(name, COUNTEREXAMPLE)
    require(rows[left]['programIndex'] != rows[right]['programIndex'],
            'selected chart does not distinguish the equal-integral duration counterexample')
    return mapping, {'jobs': len(rows), 'parameterProfiles': len(grouped),
                     'nativePrograms': len(report['programs']), 'nativeRecordings': report['stats']['compiledJobs'],
                     'propagationCalls': 0, 'allEqualProfilesShareCompleteNativeProgram': True,
                     'completeIdentityAliases': report['stats']['exactProgramAliases'],
                     'recordingReuseImplemented': False,
                     'counterexample': {'left': left, 'right': right, 'initialFactor': 2,
                                        'integratedFactorSeconds': 7, 'sameNativeProgram': False}}


def reference_gate(report, spec, identified, hashes, spec_sha, source):
    compare.validate_provenance(report, hashes, spec_sha)
    require(report.get('format') == 'ournotes-deck.luck-response-generation/1'
            and report.get('mode') == 'generate' and compare.source_version(report) == source
            and report.get('cacheBytes') == 0 and not report.get('validationDecks'),
            'speed reference is not the original uncached kernel DP')
    for field in ('sharedFingerprint', 'dependencyDescriptor', 'capabilities'):
        require(report.get(field) == identified.get(field), 'speed reference changed the resolved native context')
    jobs = spec['jobs']
    rows = pipeline.job_results(report, jobs)
    stats = report['cacheStats']
    require(type(stats.get('propagatedCurves')) is int and stats['propagatedCurves'] == len(jobs)
            and all(type(stats.get(field)) is int and stats[field] == 0
                    for field in ('hits', 'recordingHits', 'sharedRecordingHits', 'lifeRecordingHits')),
            'speed reference omitted an independent DP or reused a retained result')
    entries = report['table']['entries']
    require(report['table']['context'] == report['context'] and len(entries) == len(jobs),
            'speed reference table omits an original ordered source input')
    curves, used = {}, set()
    for job in jobs:
        row = rows[job['name']]
        index = row.get('entryIndex')
        require(row.get('status') == 'success' and not row.get('reusedExactEntry')
                and type(index) is int and 0 <= index < len(entries) and index not in used,
                'speed reference reused or omitted an original source input')
        entry = entries[index]
        require(entry['key'] == job['entries'] and entry['response'].get('status') == 'success',
                'speed reference response changed its original input')
        compare.validate_curve(entry['response']['curve'])
        curves[job['name']] = entry['response']['curve']
        used.add(index)
    comparisons = []
    for left, right in ALIASES:
        a, b = name(left), name(right)
        value = protocol.compare_probability_curves(curves[a], curves[b])
        require(all(value[field] is True for field in ('jointIntervalsOverlap', 'weightedIntervalsOverlap',
                    'sameProbeFlags', 'sameTransitionMasks', 'sameRangeMoments')),
                'independent original speed DPs disagree for an identified alias')
        comparisons.append({'left': a, 'right': b, **value})
    return {'independentOriginalDps': len(jobs), 'aliasComparisons': len(comparisons),
            'jointIntervals': sum(row['jointIntervals'] for row in comparisons),
            'disjointJointIntervals': sum(row['disjointJointIntervals'] for row in comparisons),
            'maximumEndpointDifference': max(row['maximumEndpointDifference'] for row in comparisons),
            'allJointEndpointsEqual': all(row['sameJointEndpoints'] for row in comparisons),
            'comparisons': comparisons}


def run(data, snapshot, request, generator, source, output, timeout=None):
    started = time.monotonic()
    paths = {key: Path(value).resolve() for key, value in
             (('data', data), ('snapshot', snapshot), ('request', request))}
    generator, source, output = (Path(value).resolve() for value in (generator, source, output))
    scripts = {path.name: path for path in Path(__file__).resolve().parent.glob('*.py')}
    protected = {*paths.values(), generator, *scripts.values()}
    require(not any(path.is_relative_to(output) for path in protected),
            'speed output must not contain any original input, generator or validation source')
    require(not output.exists() or not any(output.iterdir()), 'use a new empty output directory')
    before = {path: protocol.file_hash(path) for path in protected}
    hashes = {key: before[value] for key, value in paths.items()}
    algorithm = pipeline.source_identity(source)
    result = {'format': FORMAT, 'complete': False, 'status': 'running', 'inputSha256': hashes,
              'generatorSha256': before[generator], 'algorithm': algorithm,
              'sourceVersion': algorithm['simSourceSha256'],
              'validationSources': {name: before[path] for name, path in scripts.items()},
              'scope': 'Complete zero/single/ordered-pair grid of the declared twenty actual speed source levels on one unchanged real request, with selected independent original-DP witnesses.',
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
              'changesChartsOrMissions': False, 'recordingReuseImplemented': False, 'phases': {}}
    output.mkdir(parents=True, exist_ok=True)
    try:
        dataset, original = pipeline.read(paths['data']), pipeline.read(paths['request'])
        require(pipeline.luck_case(dataset, original), 'selected original chart has no natural LUCK mission')
        result.update(region=dataset.get('provenance', {}).get('region'), scoreId=original['execution']['scoreId'])
        identify, reference, profiles, recipe = make_specs(dataset)
        pipeline.write(output / 'specs/sources.json', recipe)
        pipeline.write(output / 'specs/profiles.json', profiles)
        result['sources'] = protocol.receipt(output / 'specs/sources.json', output)
        result['profiles'] = protocol.receipt(output / 'specs/profiles.json', output)
        result['parameterVectorSpace'] = recipe['parameterVectorSpace']

        def native(label, spec):
            spec_path, report_path = output / 'specs' / (label + '.json'), output / (label + '.json')
            pipeline.write(spec_path, spec)
            phase = {'status': 'running', 'spec': protocol.receipt(spec_path, output)}
            result['phases'][label] = phase
            pipeline.write(output / 'validation.json', result)
            began = time.monotonic()
            storage.call([generator, paths['data'], paths['snapshot'], paths['request'], spec_path, report_path],
                         output / (label + '.log'), timeout)
            phase.update(status='returned', report=protocol.receipt(report_path, output),
                         wallMs=(time.monotonic() - began) * 1000)
            return pipeline.read(report_path), protocol.file_hash(spec_path)

        identified, spec_sha = native('identify', identify)
        _, result['grid'] = identity_gate(identified, identify, profiles, hashes, spec_sha,
                                         algorithm['simSourceSha256'])
        result['phases']['identify'].update(status='success', nativeStats=identified['stats'])
        pipeline.write(output / 'validation.json', result)
        verified, spec_sha = native('reference', reference)
        result['comparison'] = reference_gate(verified, reference, identified, hashes, spec_sha,
                                             algorithm['simSourceSha256'])
        result['phases']['reference'].update(status='success', nativeStats=verified['cacheStats'])
        result.update(complete=True, status='success')
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(complete=False, status='error', error=pipeline.error_text(error))
    finally:
        try:
            result['inputsAndValidationSourcesUnchanged'] = all(protocol.file_hash(path) == digest
                                                                for path, digest in before.items())
            result['nativeSourceUnchanged'] = pipeline.source_identity(source) == algorithm
        except (OSError, ValueError):
            result['inputsAndValidationSourcesUnchanged'] = result['nativeSourceUnchanged'] = False
        if not result['inputsAndValidationSourcesUnchanged'] or not result['nativeSourceUnchanged']:
            result.update(complete=False, status='error', error='an original input, generator or source changed')
        result['elapsedMs'] = (time.monotonic() - started) * 1000
        pipeline.write(output / 'validation.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('data', type=Path)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('request', type=Path)
    parser.add_argument('--generator', type=Path, required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout-seconds', type=float)
    args = parser.parse_args()
    result = run(args.data, args.snapshot, args.request, args.generator, args.source, args.output, args.timeout_seconds)
    print(json.dumps({key: result[key] for key in ('complete', 'status', 'elapsedMs')}))
    return 0 if result['complete'] else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        print('LUCK speed validation: ' + str(error), file=sys.stderr)
        sys.exit(2)
