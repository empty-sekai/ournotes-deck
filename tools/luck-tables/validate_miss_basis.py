#!/usr/bin/env python3
"""Validate six real Miss/minimum kernels through cold and warm U24 dictionaries.

The unchanged DATA, SNAPSHOT and REQUEST supply a real LUCK chart context. The
six recipes come from validate_miss_gauge.py and retain five actual GK8 level-3
speed skills and five GS66 level-3 minimum skills. Actual GS96 levels exercise
Miss-gauge multiplicity, native rounding and equivalent increments. These are
master-skill parameter kernels, not owned decks or all legal team combinations.

The reference propagates six original whole-live programs with both canonical
flags disabled. The independent query uses conditional-start-minimum plus the
Miss-gauge quotient, persists U24 terms, and materializes every requested curve.
The warm query forbids missing-program generation. Imported nominal curves do
not establish full-score expectations or search rankings. Cold conditioning
can require more DP work than the six original programs; timings are recorded
without asserting a performance improvement or an isolated benchmark machine.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import math
from pathlib import Path
import subprocess
import sys
import time


def module(name):
    directory = Path(__file__).resolve().parent
    spec = importlib.util.spec_from_file_location('luck_miss_basis_validation_' + name,
                                                directory / (name + '.py'))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


miss, basis = module('validate_miss_gauge'), module('basis')
pipeline, storage, compare = basis.pipeline, basis.storage, miss.compare
FORMAT = 'ournotes-deck.luck-miss-basis-validation/1'
CONTRACT = 'conditional-start-minimum+canonical-miss-gauge-deltas/1'
MAX_TERMS = 64


def require(condition, message):
    if not condition:
        raise ValueError(message)


def receipt(path, root=None):
    path = Path(path).resolve()
    name = path.relative_to(Path(root).resolve()).as_posix() if root is not None else str(path)
    return {'path': name, 'bytes': path.stat().st_size, 'sha256': miss.file_hash(path)}


def make_specs(data):
    """Use the existing six actual-source witnesses, without its 625-job grid."""
    source_receipt = miss.validate_sources(data)
    jobs = [miss.miss_job('miss-witness-' + name, levels) for name, levels in miss.WITNESSES]
    require(len(jobs) == 6 and len({job['name'] for job in jobs}) == 6, 'the six real-source witnesses changed')
    available, _unsupported = miss.catalogue.all_chain_keys(data)
    for job in jobs:
        pipeline.validate_entries(job['entries'], available)
        require(all(sum(key['source'] == 'gekisou' and position == holder
                        for key, position in job['entries']) == 1 for holder in range(5)),
                'a witness does not retain one actual main skill per holder')
    common = {'jobs': jobs, 'identityBytes': miss.IDENTITY_BYTES, 'programBytes': miss.PROGRAM_BYTES,
              'canonicalStartMinimum': False, 'mcRuns': 0, 'scoreSamples': 0}
    original = {**common, 'mode': 'programs', 'canonicalMissGauge': False}
    query = {**common, 'mode': 'basisPrograms', 'canonicalMissGauge': True,
             'basisFamilyReuse': True, 'basisMaxTerms': MAX_TERMS,
             'basisReconstruct': False, 'verifyBasis': False}
    return jobs, original, query, source_receipt


def query_gate(result, label, hashes, spec_sha, binary_sha, source, jobs):
    require(result.get('format') == basis.QUERY and result.get('complete') is True
            and result.get('status') == 'success' and result.get('inputsUnchanged') is True
            and result.get('operatorContract') == CONTRACT and result.get('sourceVersion') == source
            and result.get('inputSha256') == {**hashes, 'spec': spec_sha}
            and result.get('generatorSha256') == binary_sha and result.get('mode') == 'u24'
            and result.get('missingPrograms') == [],
            label + ' is incomplete or has different source, inputs, specification or contract')
    require(all(result.get(field) is False for field in
                ('nativeExpectationProven', 'rankingProven', 'usesMonteCarlo', 'allTeamCombinationsCovered')),
            label + ' claims a wider scope than this nominal probability validation')
    stats = result['stats']
    for field in ('requestedJobs', 'conditionalTermReferences', 'uniqueBasisPrograms', 'reusedPrograms',
                  'generatedPrograms', 'propagationCalls', 'storedPrograms'):
        require(type(stats.get(field)) is int and stats[field] >= 0, label + ' lacks exact native work counts')
    require(stats['requestedJobs'] == len(jobs)
            and 0 < stats['uniqueBasisPrograms'] <= stats['conditionalTermReferences'],
            label + ' has incomplete conditional coverage')
    if label == 'cold':
        require(stats['reusedPrograms'] == 0 and stats['generatedPrograms'] == stats['propagationCalls']
                == stats['uniqueBasisPrograms'] == stats['storedPrograms'],
                'cold did not generate exactly its missing native conditional programs')
    elif label == 'warm':
        require(stats['generatedPrograms'] == stats['propagationCalls'] == 0
                and stats['reusedPrograms'] == stats['uniqueBasisPrograms'] == stats['storedPrograms'],
                'warm performed DP or failed to reuse the complete requested dictionary')
    else:
        raise ValueError('unknown cold/warm phase')
    identified = result['identificationStats']
    require(identified['propagationCalls'] == identified['verificationCalls'] == 0
            and identified['requestedJobs'] == identified['compiledJobs'] == len(jobs)
            and identified['familyReuseEnabled'] is False
            and identified['familyReuseDecline'] == 'canonicalMissGauge',
            label + ' identification has unexpected propagation or unproved Miss-family reuse')


def curves(table, context, jobs):
    require(table.get('context') == context, 'materialized/decoded context differs from the native reference')
    by_key = {}
    for entry in table['entries']:
        key = pipeline.canonical(entry['key'])
        require(key not in by_key and entry['response'].get('status') == 'success',
                'materialized/decoded table has duplicate or unsuccessful entries')
        curve = entry['response']['curve']
        compare.validate_curve(curve)
        by_key[key] = curve
    require(set(by_key) == {pipeline.canonical(job['entries']) for job in jobs},
            'materialized/decoded table did not retain all six exact ordered source keys')
    return {job['name']: {'curve': by_key[pipeline.canonical(job['entries'])]} for job in jobs}


def materialized(report, source, context, jobs):
    require(report.get('format') == 'ournotes-deck.luck-response-materialized/1'
            and report.get('sourceVersion') == source and report.get('operatorContract') == CONTRACT
            and type(report.get('propagationCalls')) is int and report['propagationCalls'] == 0
            and all(report.get(field) is False for field in
                    ('nativeExpectationProven', 'rankingProven', 'usesMonteCarlo')),
            'materialization changed source, contract or scope, or unexpectedly propagated')
    return curves(report['table'], context, jobs)


def compare_gate(reference, values):
    # Native original and conditional DPs perform different outward arithmetic.
    # Require interval intersection and identical observation metadata; measure
    # whether lookup intervals also contain both original reference endpoints.
    value = miss.compare_jobs(reference, values)
    value['allLookupEncloseReferenceEndpoints'] = all(row['lookupEnclosesReferenceEndpoints']
                                                     for row in value['comparisons'])
    value['maximumJointSeparatedGap'] = max(row['maximumJointSeparatedGap'] for row in value['comparisons'])
    value['maximumWeightedSeparatedGap'] = max(row['maximumWeightedSeparatedGap'] for row in value['comparisons'])
    value['allRangeMomentsEqual'] = all(row['sameRangeMoments'] for row in value['comparisons'])
    return value


def run(data, snapshot, request, generator, source, output, timeout=None):
    started = time.monotonic()
    paths = {name: Path(path).resolve() for name, path in
             (('data', data), ('snapshot', snapshot), ('request', request))}
    generator, source, output = (Path(path).resolve() for path in (generator, source, output))
    scripts = {path.name: path.resolve() for path in Path(__file__).resolve().parent.glob('*.py')}
    protected = {**paths, 'generator': generator, **{'script:' + name: path for name, path in scripts.items()}}
    require(not any(path.is_relative_to(output) for path in protected.values())
            and not source.is_relative_to(output), 'output must not contain an input, generator or source tree')
    require(not output.exists() or output.is_dir() and not any(output.iterdir()),
            'use a new empty output directory for a genuinely cold dictionary')
    require(timeout is None or math.isfinite(timeout) and timeout > 0, 'timeout must be positive and finite')
    before = {name: receipt(path) for name, path in protected.items()}
    hashes = {name: before[name]['sha256'] for name in paths}
    algorithm = pipeline.source_identity(source)
    expected_source, binary_sha = algorithm['simSourceSha256'], before['generator']['sha256']
    actual_data = pipeline.read(paths['data'])
    require(pipeline.luck_case(actual_data, pipeline.read(paths['request'])),
            'the unchanged supplied real chart has no declared LUCK mission')
    jobs, reference_spec, query_spec, source_receipt = make_specs(actual_data)
    output.mkdir(parents=True, exist_ok=True)
    pipeline.write(output / 'reference-spec.json', reference_spec)
    pipeline.write(output / 'query-spec.json', query_spec)
    pipeline.write(output / 'real-source-receipt.json', source_receipt)
    result = {
        'format': FORMAT, 'status': 'running', 'complete': False, 'mode': 'u24',
        'jobs': jobs, 'operatorContract': CONTRACT, 'algorithm': algorithm,
        'sourceVersion': expected_source, 'generatorSha256': binary_sha, 'inputSha256': hashes,
        'inputs': {name: before[name] for name in (*paths, 'generator')},
        'sourceSha256': {name: before['script:' + name]['sha256'] for name in sorted(scripts)},
        'specs': {name: receipt(output / name, output) for name in ('reference-spec.json', 'query-spec.json')},
        'realSourceReceipt': receipt(output / 'real-source-receipt.json', output),
        'scope': 'Six actual-master Miss/minimum/speed parameter kernels on the unchanged supplied real LUCK chart; not owned decks, all legal combinations, or a full search benchmark.',
        'nativeExpectationProven': False, 'rankingProven': False, 'usesMonteCarlo': False,
        'allTeamCombinationsCovered': False, 'changesChartsOrMissions': False,
        'timingScope': 'Observed wall time; no claim of isolated CPU execution or cold speedup.',
        'calls': [], 'queries': {}, 'comparisons': {},
    }
    pipeline.write(output / 'validation.json', result)

    def call(command, log, seconds):
        entry = {'argv': [str(part) for part in command], 'log': Path(log).relative_to(output).as_posix(),
                 'status': 'running'}
        result['calls'].append(entry)
        began = time.monotonic()
        try:
            storage.call(command, log, seconds)
            entry['status'] = 'success'
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            entry.update(status='error', error=pipeline.error_text(error))
            raise
        finally:
            entry['elapsedMs'] = (time.monotonic() - began) * 1000
            pipeline.write(output / 'validation.json', result)

    try:
        # No imported response or conditional compilation in this reference.
        call([generator, paths['data'], paths['snapshot'], paths['request'], output / 'reference-spec.json',
              output / 'reference.json'], output / 'reference.log', timeout)
        raw = pipeline.read(output / 'reference.json')
        require(compare.source_version(raw) == expected_source,
                'generator native source differs from the supplied source tree')
        reference = miss.program_report(raw, reference_spec, hashes, miss.file_hash(output / 'reference-spec.json'),
                                        expected_source)
        require(raw['stats']['propagationCalls'] == len(jobs) == len(raw['programs']),
                'independent reference did not propagate six distinct original programs')
        result['reference'] = {'stats': raw['stats'], 'report': receipt(output / 'reference.json', output),
                               'operatorContract': 'native-ordered-actions/1', 'independentWholeLiveDP': True}
        materializations, decoded, mappings_by_phase = {}, {}, {}
        for label, generate in (('cold', True), ('warm', False)):
            query = basis.query(paths['data'], paths['snapshot'], paths['request'], output / 'query-spec.json',
                                generator, output / 'store', output / label, mode='u24', generate=generate,
                                timeout=timeout, run_command=call)
            result['queries'][label] = query
            query_gate(query, label, hashes, miss.file_hash(output / 'query-spec.json'), binary_sha,
                       expected_source, jobs)
            identified = pipeline.read(output / label / 'identify.json')
            found_source, _programs, mappings, _references = basis.identification(identified, jobs, MAX_TERMS, CONTRACT)
            require(found_source == expected_source and all(identified.get(field) == raw.get(field)
                    for field in ('context', 'sharedFingerprint', 'dependencyDescriptor', 'capabilities')),
                    label + ' native context/dependency admission differs from the original reference')
            mappings_by_phase[label] = mappings
            materializations[label] = materialized(pipeline.read(output / label / 'responses.json'),
                                                   expected_source, raw['context'], jobs)
            result['comparisons'][label + 'MaterializedVsOriginalDP'] = compare_gate(reference, materializations[label])
            call([generator, 'unpack', output / label / 'responses.onlrsp', output / label / 'decoded.json'],
                 output / label / 'unpack.log', timeout)
            decoded[label] = curves(pipeline.read(output / label / 'decoded.json'), raw['context'], jobs)
            result['comparisons'][label + 'FinalU24VsOriginalDP'] = compare_gate(reference, decoded[label])
            codec = compare_gate(materializations[label], decoded[label])
            result['comparisons'][label + 'FinalU24VsMaterialized'] = codec
            require(codec['allLookupEncloseReferenceEndpoints'],
                    label + ' final U24 archive did not outward-enclose the materialized response')
            witness_input = {name: {**value, 'fingerprint': pipeline.sha(pipeline.canonical(mappings[name]))}
                             for name, value in materializations[label].items()}
            witness = miss.witness_gate(witness_input)
            witness['identityScope'] = 'SHA256 of complete native conditional mappings: program fingerprint, interval weight and start choices.'
            result.setdefault('witnessIdentity', {})[label] = witness
            pipeline.write(output / 'validation.json', result)
        require(mappings_by_phase['cold'] == mappings_by_phase['warm'],
                'cold/warm conditional identities, choices or interval weights changed')
        require(materializations['cold'] == materializations['warm'] and decoded['cold'] == decoded['warm'],
                'cold/warm probability responses changed')
        require(miss.file_hash(output / 'cold/responses.json') == miss.file_hash(output / 'warm/responses.json')
                and miss.file_hash(output / 'cold/responses.onlrsp') == miss.file_hash(output / 'warm/responses.onlrsp'),
                'cold/warm materialized or archive bytes changed')
        result['coldWarmIdentical'] = {'termMappingsAndWeights': True, 'materializedCurves': True,
                                       'decodedFinalU24Curves': True, 'materializedBytes': True, 'archiveBytes': True}
        directory = output / 'store' / expected_source / 'u24'
        index = storage.load_index(directory / 'program-index.json', expected_source, 'u24')
        require(set(index) == {term['programFingerprint'] for terms in mappings_by_phase['cold'].values()
                               for term in terms}, 'persisted dictionary changed the complete conditional program set')
        blobs = {}
        for row in index.values():
            path, raw_blob = storage.verified_blob(directory, row, expected_source, 'u24')
            require(raw_blob.startswith(b'ONLRSP02'), 'persisted blob uses a different binary codec')
            blobs[row['archive']['sha256']] = receipt(path, output)
        result['dictionary'] = {'programs': len(index), 'uniqueBlobs': len(blobs),
                                'index': receipt(directory / 'program-index.json', output),
                                'blobBytes': sum(item['bytes'] for item in blobs.values()), 'blobs': list(blobs.values())}
        result['artifacts'] = {str(path.relative_to(output)): receipt(path, output) for label in ('cold', 'warm')
                               for path in (output / label / 'responses.json', output / label / 'responses.onlrsp',
                                            output / label / 'decoded.json', output / label / 'identify.json',
                                            output / label / 'identify-spec.json', output / label / 'query.json')}
        result.update(status='success', complete=True)
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        result.update(status='error', complete=False, error=pipeline.error_text(error))
    finally:
        try:
            result['inputsUnchanged'] = all(receipt(path) == before[name] for name, path in protected.items())
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
        print('luck Miss/minimum basis validation: ' + str(error), file=sys.stderr)
        sys.exit(2)
