"""Exact frame comparison with an explicit field contract and no private resource paths.

Inputs are externally supplied JSON objects with chartId and ordered frames. The contract
lists integer fields and float32 bit-pattern fields separately. Wildcards address arrays.
An absent input is not_run; a missing requested field is incomplete, never passed.
"""
import argparse
import hashlib
import json
import math
import struct
from pathlib import Path


def resolve(value, parts, prefix=''):
    if not parts:
        return [(prefix, value)]
    key, *rest = parts
    if key == '*':
        if not isinstance(value, list):
            raise ValueError(f'{prefix}: expected ordered array')
        return [item for index, child in enumerate(value)
                for item in resolve(child, rest, f'{prefix}.{index}')]
    if not isinstance(value, dict) or key not in value:
        raise ValueError(f'{prefix}.{key}: missing requested field')
    return resolve(value[key], rest, f'{prefix}.{key}')


def float_detail(a, b):
    def ordered(x):
        return 0xffffffff - x if x & 0x80000000 else x + 0x80000000
    def finite(x):
        v = struct.unpack('<f', struct.pack('<I', x))[0]
        return v if math.isfinite(v) else None
    return {'nativeBits': f'{a:08x}', 'rustBits': f'{b:08x}',
            'nativeValue': finite(a), 'rustValue': finite(b),
            'ulpDistance': abs(ordered(a) - ordered(b))}


def compare(native, rust, contract):
    integer = contract['integerFields']
    floating = contract.get('float32BitFields', [])
    if not integer or not {'frame', 'timeMs'} <= set(integer):
        raise ValueError('contract must include frame and timeMs')
    if len(set(integer + floating)) != len(integer + floating):
        raise ValueError('duplicate contract fields')
    if native['chartId'] != rust['chartId']:
        raise ValueError('chart identity mismatch')
    nf, rf = native['frames'], rust['frames']
    if not nf or not rf:
        raise ValueError('empty trajectory')
    differences, incomplete = [], []
    count = 0
    if len(nf) != len(rf):
        differences.append({'kind': 'frameCount', 'native': len(nf), 'rust': len(rf)})
    for index, (a, b) in enumerate(zip(nf, rf)):
        for field, expected in contract.get('arrayLengths', {}).items():
            if type(expected) is not int or expected < 0:
                raise ValueError('expected array lengths must be nonnegative integers')
            for role, row in (('native', a), ('rust', b)):
                try:
                    values = resolve(row, field.split('.'))
                    if len(values) != 1 or not isinstance(values[0][1], list) or len(values[0][1]) != expected:
                        incomplete.append({'frameIndex': index, 'field': field, 'role': role,
                                           'reason': f'expected ordered array of length {expected}'})
                except ValueError as error:
                    incomplete.append({'frameIndex': index, 'field': field, 'role': role, 'reason': str(error)})
        for field in integer + floating:
            try:
                av, bv = resolve(a, field.split('.')), resolve(b, field.split('.'))
            except ValueError as error:
                incomplete.append({'frameIndex': index, 'field': field, 'reason': str(error)})
                continue
            if len(av) != len(bv):
                differences.append({'frameIndex': index, 'field': field, 'kind': 'arrayLength',
                                    'native': len(av), 'rust': len(bv)})
                continue
            for (path, x), (rpath, y) in zip(av, bv):
                if path != rpath or type(x) is not int or type(y) is not int:
                    raise ValueError(f'frame {index}: {path}: integer values and identical array paths required')
                if field in floating and not (0 <= x <= 0xffffffff and 0 <= y <= 0xffffffff):
                    raise ValueError(f'frame {index}: {path}: float32 uint32 bits required')
                if field in floating and any((bits & 0x7f800000) == 0x7f800000 for bits in (x, y)):
                    raise ValueError(f'frame {index}: {path}: nonfinite float32 value')
                count += 1
                if x != y:
                    detail = float_detail(x, y) if field in floating else {'native': x, 'rust': y}
                    differences.append({'frameIndex': index, 'field': path, **detail})
    status = 'incomplete' if incomplete else 'different' if differences else 'passed'
    return {'status': status, 'chartId': native['chartId'], 'nativeFrameCount': len(nf),
            'rustFrameCount': len(rf), 'fieldComparisons': count, 'contract': contract,
            'differenceCount': len(differences), 'firstDifference': differences[0] if differences else None,
            'differences': differences, 'missingRequestedFields': incomplete,
            'maxDifferenceUlp': max((d.get('ulpDistance', 0) for d in differences), default=0),
            'qualification': 'Exact equality for the declared fields and supplied ordered inputs only; '
                             'no frame shifts, numeric tolerance, or full-game equivalence claim.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--contract', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        raw = [p.read_bytes() for p in (args.native, args.rust, args.contract)]
        result = compare(*(json.loads(x) for x in raw))
        result['sourceSha256'] = dict(zip(('nativeCapture', 'rustCapture', 'contract'),
                                         (hashlib.sha256(x).hexdigest() for x in raw)))
    except FileNotFoundError as error:
        result = {'status': 'not_run', 'reason': str(error)}
    except (ValueError, KeyError, TypeError) as error:
        result = {'status': 'invalid', 'reason': str(error)}
    args.output.write_text(json.dumps(result, indent=2, allow_nan=False), encoding='utf8')
    print(json.dumps({k: v for k, v in result.items() if k not in ('differences', 'missingRequestedFields')}))
    return 0 if result['status'] == 'passed' else 1 if result['status'] == 'different' else 2


if __name__ == '__main__':
    raise SystemExit(main())
