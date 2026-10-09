#!/usr/bin/env python3
"""Measure reversible block/prefix sharing in one actual conditional dictionary.

This offline experiment preserves every input ONLRSP02 archive byte. Its block
pool is an analysis artifact, not a browser or native response-table codec.
Compare raw payload plus index, outer compression, and exact reconstruction
separately; a small-family result does not establish full-catalogue savings.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import importlib.util
import io
import json
import lzma
from pathlib import Path
import struct
import sys
import tarfile
import time


SPEC = importlib.util.spec_from_file_location('basis_block_analysis', Path(__file__).with_name('analyze_programs.py'))
analysis = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(analysis)
pipeline, storage = analysis.pipeline, analysis.store
MAGIC = b'ONLPOOL1'
MAX_BYTES = 256 << 20


class Reader:
    def __init__(self, raw):
        if len(raw) > MAX_BYTES:
            raise ValueError('analysis input exceeds its explicit 256 MiB allowance')
        self.raw, self.at = raw, 0

    def take(self, length):
        if type(length) is not int or length < 0 or self.at + length > len(self.raw):
            raise ValueError('truncated or invalid native/prototype byte section')
        begin = self.at; self.at += length
        return self.raw[begin:self.at]

    def var(self):
        value = 0
        for shift in range(0, 70, 7):
            byte = self.take(1)[0]
            value |= (byte & 127) << shift
            if byte < 128:
                return value
        raise ValueError('oversized integer in an analysis input')

    def u32(self):
        return struct.unpack('<I', self.take(4))[0]

    def string(self):
        return self.take(self.var()).decode('utf-8')


def native_sections(raw, fingerprint, source):
    """Locate existing byte boundaries; never requantize or rewrite probabilities."""
    read = Reader(raw)
    if read.take(8) != b'ONLRSP02' or read.take(1)[0] != 24:
        raise ValueError('this experiment requires the native U24 ONLRSP02 format')
    if (read.string() != fingerprint
            or read.string() != 'ournotes-luck-program-response/1/' + source
            or read.var() != 1 or read.var() != 0):
        raise ValueError('native archive context or its single empty-key response differs')
    offset, size = struct.unpack('<QQ', read.take(16)); digest = read.take(32)
    if offset != read.at or size != len(raw) - offset or hashlib.sha256(raw[offset:]).digest() != digest:
        raise ValueError('native archive payload offset, length or SHA differs')
    if read.take(1) != b'\x01':
        raise ValueError('a conditional archive is not a successful native response')
    read.var(); read.var()  # original peak states and transitions
    steps = read.var()
    if not 0 < steps <= 2_000_000:
        raise ValueError('native response has an invalid step count')
    times = read.at
    for _ in range(steps):
        read.var()
    probabilities = read.at
    seen = 0
    while seen < steps:
        count = read.var()
        if count == 0 or seen + count > steps:
            raise ValueError('invalid native probability run length')
        for _ in range(8):
            read.var()
        seen += count
    masks = read.at
    frames = read.var(); seen = 0
    while seen < frames:
        count = read.var(); mask = read.take(1)[0]
        if count == 0 or seen + count > frames or not 1 <= mask <= 15:
            raise ValueError('invalid native transition-mask run')
        seen += count
    probes = read.at
    read.take((read.var() + 7) // 8)
    moments = read.at
    read.take(read.var() * 80)
    if read.at != len(raw):
        raise ValueError('unexpected native archive tail')
    boundaries = (0, times, probabilities, masks, probes, moments, len(raw))
    return [raw[begin:end] for begin, end in zip(boundaries, boundaries[1:])]


def encode_pool(blobs, pieces, ordering):
    pool, addresses, records = [], {}, []
    for sha in ordering:
        sequence = []
        for part in pieces[sha]:
            if not part:
                continue
            if part not in addresses:
                addresses[part] = len(pool); pool.append(part)
            sequence.append(addresses[part])
        if b''.join(pool[index] for index in sequence) != blobs[sha]:
            raise ValueError('proposed block split changed an original native archive')
        records.append((sha, len(blobs[sha]), sequence))
    out = bytearray(MAGIC + struct.pack('<II', len(pool), len(records)))
    for part in pool:
        out.extend(struct.pack('<I', len(part))); out.extend(part)
    for sha, length, sequence in records:
        out.extend(bytes.fromhex(sha)); out.extend(struct.pack('<II', length, len(sequence)))
        out.extend(struct.pack('<' + 'I' * len(sequence), *sequence))
    return bytes(out), {'blocks': len(pool), 'blockPayloadBytes': sum(map(len, pool)),
                        'blockReferences': sum(len(sequence) for _, _, sequence in records)}


def decode_pool(raw):
    read = Reader(raw)
    if read.take(8) != MAGIC:
        raise ValueError('unknown offline block-pool format')
    count, archives = read.u32(), read.u32()
    if not 1 <= count <= 1_000_000 or not 1 <= archives <= 65_536:
        raise ValueError('offline block-pool count exceeds the analysis allowance')
    pool = [read.take(read.u32()) for _ in range(count)]
    result = {}
    for _ in range(archives):
        sha = read.take(32).hex(); length, count = read.u32(), read.u32()
        if length > MAX_BYTES or count > 1_000_000 or sha in result:
            raise ValueError('duplicate or excessive archive in an offline pool')
        parts = []
        for _ in range(count):
            index = read.u32()
            if index >= len(pool):
                raise ValueError('offline block reference is outside its pool')
            parts.append(pool[index])
        archive = b''.join(parts)
        if len(archive) != length or pipeline.sha(archive) != sha:
            raise ValueError('offline pool did not reconstruct the exact original native archive')
        result[sha] = archive
    if read.at != len(raw):
        raise ValueError('unexpected offline block-pool tail')
    return result


def prefix_pieces(probabilities, choices):
    result = {sha: [] for sha in probabilities}
    lengths = {len(choice) for choice in choices.values()}
    if len(lengths) != 1 or not 1 <= next(iter(lengths)) <= 3:
        raise ValueError('one conditional family with one to three start choices is required')
    depth_limit = next(iter(lengths))
    nodes = []

    def visit(keys, depth, offset):
        representative = probabilities[keys[0]]
        end = min(len(probabilities[key]) for key in keys)
        at = offset
        while at < end and all(probabilities[key][at] == representative[at] for key in keys[1:]):
            at += 1
        part = representative[offset:at]
        for key in keys:
            result[key].append(part)
        nodes.append({'choicePrefix': list(choices[keys[0]][:depth]), 'programs': len(keys),
                      'sharedBytes': len(part)})
        if depth == depth_limit:
            if len(keys) != 1 or at != len(representative):
                raise ValueError('a complete choice tuple does not identify one byte-exact conditional program')
            return
        groups = {}
        for key in keys:
            groups.setdefault(choices[key][depth], []).append(key)
        for group in sorted(groups):
            visit(groups[group], depth + 1, at)

    visit(sorted(probabilities, key=lambda key: choices[key]), 0, 0)
    if any(b''.join(result[key]) != probabilities[key] for key in probabilities):
        raise ValueError('conditional prefix tree failed its exact native probability-byte reconstruction')
    return result, nodes


def bundle(directory, index, payloads):
    directory.mkdir(parents=True)
    tar_path = directory / 'dictionary.tar'
    with tarfile.open(tar_path, 'w', format=tarfile.USTAR_FORMAT) as tar:
        analysis.tar_member(tar, 'program-index.json', len(index), io.BytesIO(index))
        for name, raw in payloads:
            analysis.tar_member(tar, name, len(raw), io.BytesIO(raw))
    raw = tar_path.read_bytes()
    result = {'indexBytes': len(index), 'payloadBytes': sum(len(raw) for _, raw in payloads),
              'indexAndPayloadBytes': len(index) + sum(len(raw) for _, raw in payloads),
              'tar': {'bytes': len(raw), 'sha256': pipeline.sha(raw)}}
    for method in ('gzip9', 'xz6'):
        start = time.monotonic()
        if method == 'gzip9':
            stream = io.BytesIO()
            with gzip.GzipFile(filename='', mode='wb', compresslevel=9, fileobj=stream, mtime=0) as compressor:
                compressor.write(raw)
            encoded = stream.getvalue(); decoded = gzip.decompress(encoded)
            target = directory / 'dictionary.tar.gz'
        else:
            encoded = lzma.compress(raw, preset=6); decoded = lzma.decompress(encoded)
            target = directory / 'dictionary.tar.xz'
        if decoded != raw:
            raise ValueError('offline dictionary bundle compression failed its exact TAR round trip')
        target.write_bytes(encoded)
        result[method] = {'bytes': len(encoded), 'sha256': pipeline.sha(encoded),
                          'roundTripVerified': True, 'elapsedMs': (time.monotonic() - start) * 1000}
    return result


def run(report_path, index_path, output, block_sizes=(64, 128, 256, 512)):
    report_path, index_path, output = Path(report_path).resolve(), Path(index_path).resolve(), Path(output).resolve()
    if output.exists() and any(output.iterdir()):
        raise ValueError('use a new empty output for the reversible compression experiment')
    if any(path.is_relative_to(output) for path in (report_path, index_path)):
        raise ValueError('analysis output must not contain a source response or dictionary')
    if any(type(size) is not int or not 16 <= size <= 65536 for size in block_sizes):
        raise ValueError('fixed chunk sizes must be explicit integers from 16 through 65536')
    report, index_raw = pipeline.read(report_path), index_path.read_bytes()
    source = storage.digest(report.get('sourceVersion'))
    if (report.get('format') != 'ournotes-deck.luck-response-basis/1'
            or report.get('complete') is not True or report.get('probabilityComplete') is not True):
        raise ValueError('provide a complete original native conditional response report')
    rows = storage.load_index(index_path, source, 'u24')
    if set(rows) != {program['fingerprint'] for program in report['programs']}:
        raise ValueError('native conditional response and dictionary program coverage differ')
    choices_by_fp, seen_choices = {}, {}
    for job in report['jobs']:
        for component in job['components']:
            fingerprint, choice = component['programFingerprint'], tuple(component['choices'])
            if (fingerprint in choices_by_fp and choices_by_fp[fingerprint] != choice
                    or choice in seen_choices and seen_choices[choice] != fingerprint):
                raise ValueError('one complete conditional family is required for prefix sharing')
            choices_by_fp[fingerprint] = choice; seen_choices[choice] = fingerprint
    if set(choices_by_fp) != set(rows):
        raise ValueError('conditional choices do not cover the exact native dictionary')
    blobs, sections, choices, blob_paths = {}, {}, {}, {}
    for fingerprint, row in rows.items():
        path, raw = storage.verified_blob(index_path.parent, row, source, 'u24')
        if not path.resolve().is_relative_to(index_path.parent.resolve()):
            raise ValueError('a native archive escapes its declared dictionary')
        sha = row['archive']['sha256']
        blobs[sha] = raw; choices[sha] = choices_by_fp[fingerprint]
        blob_paths[sha] = path
        sections[sha] = native_sections(raw, fingerprint, source)
    if sum(map(len, blobs.values())) > MAX_BYTES:
        raise ValueError('native dictionary exceeds the explicit 256 MiB experiment allowance')
    output.mkdir(parents=True, exist_ok=True)
    result = {'format': 'ournotes-deck.luck-basis-block-analysis/1', 'sourceVersion': source,
              'reportSha256': analysis.file_hash(report_path), 'indexSha256': pipeline.sha(index_raw),
              'runnerSha256': analysis.file_hash(__file__), 'nativeProvenance': report.get('provenance'),
              'programs': len(rows), 'uniqueNativeArchives': len(blobs), 'sourceCodec': 'u24',
              'scope': 'One actual whole-live conditional family; all source archive bytes preserved. Offline prototype pool, no browser/native codec integration and no full-catalogue size inference.',
              'nativeExpectationProven': False, 'rankingProven': False, 'propagationCalls': 0,
              'schemes': {}, 'compressionProtocol': 'USTAR; original native index first; mode0644, uid/gid/mtime0; gzip9 mtime0 and filename empty; XZ preset6'}
    result['schemes']['native'] = bundle(output / 'native', index_raw,
        [('blobs/' + sha + '.onlrsp', blobs[sha]) for sha in sorted(blobs)])
    ordered = sorted(blobs, key=lambda sha: choices[sha])

    def measure(name, parts):
        raw, metadata = encode_pool(blobs, parts, ordered)
        if decode_pool(raw) != blobs:
            raise ValueError('the experimental pool does not reconstruct every original native archive')
        measured = bundle(output / name, index_raw, [('blocks.onlpool', raw)])
        (output / name / 'blocks.onlpool').write_bytes(raw)
        measured.update(pool={**metadata, 'bytes': len(raw), 'sha256': pipeline.sha(raw),
                              'nativeArchivesExactlyReconstructed': len(blobs)})
        result['schemes'][name] = measured

    for size in dict.fromkeys(block_sizes):
        measure('fixed-' + str(size), {sha: [raw[at:at + size] for at in range(0, len(raw), size)]
                                      for sha, raw in blobs.items()})
    measure('native-sections', sections)
    prefixes, nodes = prefix_pieces({sha: values[2] for sha, values in sections.items()}, choices)
    measure('conditional-prefix', {sha: values[:2] + prefixes[sha] + values[3:] for sha, values in sections.items()})
    result['conditionalPrefixNodes'] = nodes
    result['inputsUnchanged'] = (analysis.file_hash(report_path) == result['reportSha256']
                                 and index_path.read_bytes() == index_raw
                                 and all(analysis.file_hash(path) == sha for sha, path in blob_paths.items()))
    if not result['inputsUnchanged']:
        raise ValueError('a source report, native index or original archive changed during analysis')
    result['complete'] = True
    pipeline.write(output / 'analysis.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--index', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--block-size', type=int, action='append')
    args = parser.parse_args()
    result = run(args.report, args.index, args.output, args.block_size or (64, 128, 256, 512))
    print(json.dumps({'complete': result['complete'], 'programs': result['programs'],
                      'schemes': {name: {'indexAndPayloadBytes': value['indexAndPayloadBytes'],
                                        'gzip9': value['gzip9']['bytes'], 'xz6': value['xz6']['bytes']}
                                  for name, value in result['schemes'].items()}}))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, IndexError, struct.error) as error:
        raise SystemExit('LUCK basis block analysis: ' + str(error))
