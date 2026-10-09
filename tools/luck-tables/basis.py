#!/usr/bin/env python3
"""Resolve interacting minimum guarantees with persisted conditional programs.

The native compiler admits every original requested combination and conditions
the complete live on its range-start minima. Only missing conditional programs
need DP. Requested mixtures are materialized with outward arithmetic by native
code; imported curves remain prediction data and grant no ranking proof.
"""
from __future__ import annotations

import argparse
from fractions import Fraction
import importlib.util
import json
import math
from pathlib import Path
import subprocess
import sys
import time


_spec = importlib.util.spec_from_file_location('luck_basis_program_store', Path(__file__).with_name('programs.py'))
storage = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(storage)
pipeline = storage.pipeline
REPORT = 'ournotes-deck.luck-response-basis/1'
QUERY = 'ournotes-deck.luck-basis-query/1'
CONTRACT = 'conditional-start-minimum/1'
MISS_CONTRACT = 'conditional-start-minimum+canonical-miss-gauge-deltas/1'
CONTRACTS = (CONTRACT, MISS_CONTRACT)
MAX_TERMS = 64


def object_value(value, name):
    if not isinstance(value, dict):
        raise ValueError(name + ' must be an object')
    return value


def provenance(report, hashes, spec_sha):
    value = object_value(report.get('provenance'), 'native provenance')
    inputs = object_value(value.get('inputs'), 'native input provenance')
    if (value.get('datasetSha256') != hashes['data']
            or inputs.get('rosterSha256') != hashes['snapshot']
            or inputs.get('requestSha256') != hashes['request']
            or inputs.get('specSha256') != spec_sha):
        raise ValueError('native basis provenance differs from current input/spec bytes')


def structure(report, jobs, mode, max_terms, expected_contract=None):
    """Validate complete term mappings without interpreting a hash as a proof."""
    object_value(report, 'native basis report')
    if (report.get('format') != REPORT or report.get('mode') != mode
            or report.get('identificationComplete') is not True or report.get('complete') is not True
            or report.get('operatorContract') not in CONTRACTS
            or expected_contract is not None and report.get('operatorContract') != expected_contract):
        raise ValueError('native conditional basis is incomplete or uses a different operator contract')
    contract = report['operatorContract']
    source = storage.digest(report.get('sourceVersion'))
    context = object_value(report.get('context'), 'native response context')
    storage.digest(context.get('fingerprint'))
    if context.get('algorithmVersion') != 'ournotes-luck-response/1/' + source:
        raise ValueError('native response context uses a different source algorithm')
    programs = report.get('programs')
    if not isinstance(programs, list) or not 1 <= len(programs) <= len(jobs) * max_terms:
        raise ValueError('native basis has no programs or exceeds its requested term allowance')
    fingerprints = []
    for ordinal, value in enumerate(programs):
        program = object_value(value, 'conditional program')
        fingerprint = storage.digest(program.get('fingerprint'))
        if (program.get('sourceVersion') != source or program.get('operatorContract') != contract
                or not isinstance(program.get('identityVersion'), str) or not program['identityVersion']
                or type(program.get('programIndex')) is not int or program['programIndex'] != ordinal):
            raise ValueError('conditional program source, contract or ordinal differs')
        fingerprints.append(fingerprint)
    if len(set(fingerprints)) != len(programs):
        raise ValueError('native basis repeats a conditional program identity')
    rows = pipeline.job_results(report, jobs)
    mappings, referenced, term_references = {}, set(), 0
    for ordinal, job in enumerate(jobs):
        row = rows[job['name']]
        components = row.get('components')
        if (type(row.get('jobIndex')) is not int or row['jobIndex'] != ordinal
                or row.get('status') not in ('identified', 'success')
                or not isinstance(components, list) or not 1 <= len(components) <= max_terms
                or row.get('termCount') != len(components)
                or type(row.get('startCount')) is not int or not 0 <= row['startCount'] <= 3):
            raise ValueError('requested basis job has incomplete or excessive conditional coverage')
        choices_seen, normalized = set(), []
        lower_mass, upper_mass = Fraction(0), Fraction(0)
        for value in components:
            component = object_value(value, 'conditional component')
            index, weight, choices = component.get('programIndex'), component.get('weight'), component.get('choices')
            if (type(index) is not int or not 0 <= index < len(programs)
                    or component.get('programFingerprint') != fingerprints[index]):
                raise ValueError('conditional component references a different native program')
            if (not isinstance(weight, list) or len(weight) != 2
                    or any(type(number) not in (int, float) or not 0 <= number <= 1
                           or not math.isfinite(number) for number in weight)
                    or not 0 <= weight[0] <= weight[1] <= 1 or weight[1] == 0):
                raise ValueError('conditional component has an invalid probability interval')
            if (not isinstance(choices, list) or len(choices) != row['startCount']
                    or any(type(choice) is not int or not 0 <= choice <= 3 for choice in choices)
                    or tuple(choices) in choices_seen):
                raise ValueError('conditional component has duplicate or invalid start choices')
            choices_seen.add(tuple(choices)); referenced.add(index)
            lower_mass += Fraction(weight[0]); upper_mass += Fraction(weight[1])
            normalized.append({'programFingerprint': fingerprints[index], 'weight': weight, 'choices': choices})
        if not lower_mass <= 1 <= upper_mass:
            raise ValueError('conditional probability intervals do not enclose unit mass')
        mappings[job['name']] = normalized
        term_references += len(components)
    if referenced != set(range(len(programs))):
        raise ValueError('native basis contains unreferenced programs')
    for index, program in enumerate(programs):
        representative, term = program.get('representativeJobIndex'), program.get('representativeTermIndex')
        if type(representative) is not int or not 0 <= representative < len(jobs) or type(term) is not int:
            raise ValueError('conditional program has an invalid representative')
        components = rows[jobs[representative]['name']]['components']
        if not 0 <= term < len(components) or components[term]['programIndex'] != index:
            raise ValueError('conditional representative does not identify its program')
    stats = object_value(report.get('stats'), 'native basis statistics')
    if (type(stats.get('propagationCalls')) is not int or stats['propagationCalls'] < 0
            or type(stats.get('verificationCalls')) is not int or stats['verificationCalls'] != 0):
        raise ValueError('query basis statistics include unrequested verification or invalid propagation')
    return source, programs, mappings, term_references


def identification(report, jobs, max_terms=MAX_TERMS, expected_contract=None):
    result = structure(report, jobs, 'basisIdentify', max_terms, expected_contract)
    if (report.get('probabilityComplete') is not False or report['stats']['propagationCalls'] != 0
            or any(row['status'] != 'identified' for row in report['jobs'])
            or any(program.get('status') != 'identified' or program.get('response') is not None
                   for program in result[1])):
        raise ValueError('native basis identification unexpectedly propagated probabilities')
    return result


def generation(report, identified, jobs, missing, max_terms=MAX_TERMS):
    source, programs, mappings, _count = structure(
        report, jobs, 'basisPrograms', max_terms, identified.get('operatorContract'))
    old_source, original, old_mappings, _count = identification(identified, jobs, max_terms)
    expected = set(missing)
    if (source != old_source or report.get('probabilityComplete') is not True
            or report.get('missingSelectedPrograms') != [] or mappings != old_mappings
            or report.get('context') != identified.get('context')
            or report.get('sharedFingerprint') != identified.get('sharedFingerprint')
            or report.get('dependencyDescriptor') != identified.get('dependencyDescriptor')
            or report.get('capabilities') != identified.get('capabilities')):
        raise ValueError('conditional propagation changed current native dependencies, weights or coverage')
    original_by_fp = {program['fingerprint']: program for program in original}
    if ({program['fingerprint'] for program in programs} != set(original_by_fp)
            or not expected or not expected <= set(original_by_fp)):
        raise ValueError('conditional propagation has different or unknown program identities')
    for program in programs:
        fingerprint = program['fingerprint']; previous = original_by_fp[fingerprint]
        if any(program.get(field) != previous.get(field) for field in
               ('representativeJobIndex', 'representativeTermIndex', 'identityVersion')):
            raise ValueError('conditional program representative or identity version changed')
        if fingerprint in expected:
            if (program.get('status') != 'success'
                    or object_value(program.get('response'), 'conditional response').get('status') != 'success'):
                raise ValueError('missing conditional program was not propagated successfully')
        elif program.get('status') != 'identified' or program.get('response') is not None:
            raise ValueError('native basis propagated an unrequested conditional program')
    if report['stats']['propagationCalls'] != len(expected):
        raise ValueError('native propagation count differs from distinct missing conditional programs')
    return [program for program in programs if program['fingerprint'] in expected]


def storage_projection(report, selected, report_sha):
    """CONTROL rows adapt conditional responses to the existing binary codec."""
    bodies, jobs = [], []
    for index, original in enumerate(selected):
        fingerprint = original['fingerprint']
        bodies.append({**original, 'programIndex': index, 'representativeJobIndex': index,
                       'basisOrigin': {'reportSha256': report_sha,
                           'representativeJobIndex': original['representativeJobIndex'],
                           'representativeTermIndex': original['representativeTermIndex']}})
        jobs.append({'name': 'basis-control-' + fingerprint, 'entries': [], 'jobIndex': index,
                     'programIndex': index, 'programFingerprint': fingerprint, 'status': 'success'})
    result = {'format': storage.REPORT, 'mode': 'programs', 'sourceVersion': report['sourceVersion'],
              'identificationComplete': True, 'probabilityComplete': True, 'complete': True,
              'kind': 'conditionalProgramStorageProjection', 'operatorContract': report['operatorContract'],
              'scope': 'CONTROL rows are conditional native programs for storage, not simulated teams or independent skill curves.',
              'basisReportSha256': report_sha, 'jobs': jobs, 'programs': bodies,
              'context': report['context'], 'sharedFingerprint': report['sharedFingerprint'],
              'dependencyDescriptor': report['dependencyDescriptor'], 'capabilities': report['capabilities'],
              'provenance': report['provenance'], 'stats': {'propagationCalls': len(selected)},
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False}
    storage.generation(result, jobs, {job['name']: job['programFingerprint'] for job in jobs}, report['sourceVersion'])
    return result


def query(data, snapshot, request, spec, generator, store, output, mode='u24', generate=True,
          seed_indexes=(), timeout=None, decks=None, run_command=storage.call):
    if mode not in storage.MODES:
        raise ValueError('unknown quantization')
    started = time.monotonic(); output = Path(output).resolve(); generator = Path(generator).resolve()
    inputs = {name: str(Path(value).resolve()) for name, value in
              (('data', data), ('snapshot', snapshot), ('request', request), ('spec', spec))}
    if decks is not None:
        inputs['decks'] = str(Path(decks).resolve())
    if any(Path(path).is_relative_to(output) for path in inputs.values()) or generator.is_relative_to(output):
        raise ValueError('query output must not contain any input or generator file')
    before = pipeline.provenance(inputs)
    original = object_value(pipeline.read(spec), 'requested specification'); jobs = original.get('jobs')
    if (not isinstance(jobs, list) or not 1 <= len(jobs) <= 65536
            or any(not isinstance(row, dict) or not isinstance(row.get('name'), str) or not row['name']
                   or not isinstance(row.get('entries'), list) for row in jobs)
            or len({row['name'] for row in jobs}) != len(jobs)):
        raise ValueError('provide 1..65536 uniquely named requested combination jobs')
    max_terms = original.get('basisMaxTerms')
    if max_terms is None:
        max_terms = MAX_TERMS
    if type(max_terms) is not int or not 1 <= max_terms <= MAX_TERMS:
        raise ValueError('basisMaxTerms must be an integer from 1 to 64')
    canonical_miss = original.get('canonicalMissGauge', False)
    family_reuse = original.get('basisFamilyReuse', True)
    if type(canonical_miss) is not bool or type(family_reuse) is not bool:
        raise ValueError('canonicalMissGauge and basisFamilyReuse must be booleans')
    contract = MISS_CONTRACT if canonical_miss else CONTRACT
    identified_spec = {**original, 'mode': 'basisIdentify', 'mcRuns': 0, 'scoreSamples': 0,
                       'validationDecks': [], 'verifyBasis': False, 'basisReconstruct': False,
                       'basisMaxTerms': max_terms}
    identified_spec.pop('basisPrograms', None)
    output.mkdir(parents=True, exist_ok=True)
    pipeline.write(output / 'identify-spec.json', identified_spec)
    result = {'format': QUERY, 'complete': False, 'inputSha256': before,
              'generatorSha256': pipeline.sha(generator.read_bytes()), 'mode': mode,
              'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
              'allTeamCombinationsCovered': False, 'operatorContract': contract,
              'scope': 'Requested whole-live conditional minimum basis; only missing native term responses are propagated.',
              'stats': {'requestedJobs': len(jobs), 'conditionalTermReferences': 0, 'uniqueBasisPrograms': 0,
                        'reusedPrograms': 0, 'generatedPrograms': 0, 'propagationCalls': 0}}
    pipeline.write(output / 'query.json', result)
    base = [generator, inputs['data'], inputs['snapshot'], inputs['request']]
    try:
        run_command(base + [output / 'identify-spec.json', output / 'identify.json'], output / 'identify.log', timeout)
        identified = pipeline.read(output / 'identify.json')
        source, programs, _mappings, references = identification(identified, jobs, max_terms, contract)
        provenance(identified, before, pipeline.sha((output / 'identify-spec.json').read_bytes()))
        result.update(sourceVersion=source, identificationStats=identified['stats'])
        result['stats'].update(conditionalTermReferences=references, uniqueBasisPrograms=len(programs))
        directory = Path(store).resolve() / source / mode
        if directory == output or directory.is_relative_to(output) or output.is_relative_to(directory):
            raise ValueError('query working output and persistent dictionary must be separate')
        with storage.writer_lock(directory):
            index_path = directory / 'program-index.json'; rows = storage.load_index(index_path, source, mode)
            for seed in seed_indexes:
                storage.import_index(seed, directory, rows, source, mode)
            missing = [p['fingerprint'] for p in programs if not storage.available(directory, rows.get(p['fingerprint'], {}), source, mode)]
            result['stats']['reusedPrograms'] = len(programs) - len(missing)
            if missing and generate:
                pipeline.write(output / 'missing-spec.json', {**identified_spec, 'mode': 'basisPrograms', 'basisPrograms': missing})
                run_command(base + [output / 'missing-spec.json', output / 'basis-missing.json'], output / 'missing.log', timeout)
                generated = pipeline.read(output / 'basis-missing.json')
                selected = generation(generated, identified, jobs, missing, max_terms)
                provenance(generated, before, pipeline.sha((output / 'missing-spec.json').read_bytes()))
                result['generationStats'] = generated['stats']
                result['stats']['propagationCalls'] = generated['stats']['propagationCalls']
                projected = storage_projection(generated, selected, pipeline.sha((output / 'basis-missing.json').read_bytes()))
                pipeline.write(output / 'programs.json', projected)
                run_command([generator, 'program-pack', output / 'programs.json', mode, output / 'packed'], output / 'pack.log', timeout)
                packed_path = output / 'packed/program-index.json'
                packed = pipeline.read(packed_path)
                incoming = storage.load_index(packed_path, source, mode)
                if (packed.get('complete') is not True or packed.get('inputReportSha256') != pipeline.sha((output / 'programs.json').read_bytes())
                        or packed.get('requestedJobs') != len(missing) or set(incoming) != set(missing)):
                    raise ValueError('packed conditional program coverage differs from the verified missing projection')
                result['stats']['generatedPrograms'] = storage.import_index(packed_path, directory, rows, source, mode)
                missing = [p['fingerprint'] for p in programs if not storage.available(directory, rows.get(p['fingerprint'], {}), source, mode)]
            storage.save_index(directory, rows, source, mode)
            result['missingPrograms'] = missing; result['stats']['storedPrograms'] = len(rows)
            if not missing:
                run_command([generator, 'basis-materialize', output / 'identify.json', index_path, output / 'responses.json'],
                            output / 'materialize.log', timeout)
                run_command([generator, 'pack', output / 'responses.json', mode, output / 'responses.onlrsp'], output / 'archive.log', timeout)
                archive = output / 'responses.onlrsp'
                result['archive'] = {'path': archive.name, 'sha256': pipeline.sha(archive.read_bytes()), 'bytes': archive.stat().st_size}
                result['complete'] = True
        if result['complete'] and decks is not None:
            run_command([generator, 'predict', output / 'responses.onlrsp', inputs['data'], inputs['snapshot'],
                         inputs['request'], inputs['decks'], output / 'prediction.json'], output / 'predict.log', timeout)
            prediction = pipeline.read(output / 'prediction.json')
            result['prediction'] = {'status': prediction['status'], 'ordersScored': prediction['ordersScored']}
            result['complete'] &= prediction['status'] == 'success'
        result['status'] = 'success' if result['complete'] else 'missing'
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(complete=False, status='error', error=pipeline.error_text(error))
    finally:
        try:
            result['inputsUnchanged'] = pipeline.provenance(inputs) == before
        except OSError:
            result['inputsUnchanged'] = False
        if not result['inputsUnchanged']:
            result.update(complete=False, status='error', error='input bytes changed during the query')
        result['elapsedMs'] = (time.monotonic() - started) * 1000
        pipeline.write(output / 'query.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('data', 'snapshot', 'request', 'spec'):
        parser.add_argument(name, type=Path)
    for name in ('generator', 'store', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--mode', choices=storage.MODES, default='u24')
    parser.add_argument('--no-generate', action='store_true', help='identify missing conditional programs without DP')
    parser.add_argument('--seed-index', type=Path, action='append', default=[])
    parser.add_argument('--timeout-seconds', type=float)
    parser.add_argument('--decks', type=Path, help='optional actual decks to score from requested mixed responses')
    args = parser.parse_args()
    result = query(args.data, args.snapshot, args.request, args.spec, args.generator, args.store, args.output,
                   args.mode, not args.no_generate, args.seed_index, args.timeout_seconds, args.decks)
    print(json.dumps({key: result[key] for key in ('complete', 'status', 'stats', 'elapsedMs')}))
    return 0 if result['complete'] else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f'luck basis query: {error}', file=sys.stderr)
        sys.exit(2)
