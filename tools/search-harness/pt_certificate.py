"""Independent top-K resource DP for a globally maximal PT plateau.

Shares only exported model values with Rust; no production enumeration, bounds,
Hungarian matching or Top-K routines. The DP scans each Snap once, retaining K
bindings per assigned-slot mask. Future resources depend only on the mask, so
additive power and canonical ties are preserved by every extension.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path


def snap_key(binding):
    return tuple((x is not None, x if x is not None else 0) for x in binding)


def best_bindings(none, matrix, snap_ids, k):
    dp = {0: [(0, (None,) * 5)]}
    for j, sid in enumerate(snap_ids):
        nxt = {mask: list(rows) for mask, rows in dp.items()}
        for mask, rows in dp.items():
            for slot in range(5):
                if mask & (1 << slot):
                    continue
                target = nxt.setdefault(mask | (1 << slot), [])
                for power, binding in rows:
                    b = list(binding)
                    b[slot] = sid
                    target.append((power + matrix[slot][j] - none[slot], tuple(b)))
        dp = {}
        for mask, rows in nxt.items():
            rows.sort(key=lambda r: (-r[0], snap_key(r[1])))
            dp[mask] = rows[:k]
    rows = [(sum(none) + power, binding) for group in dp.values() for power, binding in group]
    rows.sort(key=lambda r: (-r[0], snap_key(r[1])))
    return rows[:k]


def certify(doc):
    chars = {}
    for _, char, bonus in doc['members']:
        assert bonus >= 0
        chars[char] = max(chars.get(char, 0), bonus)
    snaps = doc['snaps']
    assert all(b >= 0 for _, b in snaps)
    extra = sum(sorted((b for _, b in snaps), reverse=True)[:5])
    multiplier = doc['rewardUpper'] * doc['rate']
    caps = {m: (10000 + b + extra + sum(sorted((v for c, v in chars.items() if c != char), reverse=True)[:4])) * multiplier // 10000
            for m, char, b in doc['members']}
    n, d = int(doc['incumbentNumerator']), int(doc['incumbentDenominator'])
    assert d > 0
    kept = sorted(m for m, cap in caps.items() if cap * d >= n)
    ceiling = max(caps.values())
    assert ceiling * d == n, 'incumbent has not reached the global PT ceiling'
    assert kept == sorted(doc['selectedMembers']) and len(kept) == 5
    assert len({char for m, char, _ in doc['members'] if m in kept}) == 5
    assert set(doc['requiredMembers']).issubset(kept)
    expected = {p for p in itertools.permutations(kept) if p[2] in doc['allowedLeaders']}
    actual = [tuple(row['members']) for row in doc['layouts']]
    assert len(actual) == len(set(actual)) and set(actual) == expected
    rows = []
    for layout in doc['layouts']:
        assert len(layout['none']) == 5 and len(layout['slotPower']) == 5
        assert all(len(row) == len(snaps) for row in layout['slotPower'])
        assert all(v >= 0 for row in layout['slotPower'] for v in row)
        assert all(v >= 0 for v in layout['none'])
        assert sum(max([layout['none'][s]] + layout['slotPower'][s]) for s in range(5)) <= 2147483647
        assert sorted(layout['members']) == kept
        for power, binding in best_bindings(layout['none'], layout['slotPower'], [s for s, _ in snaps], doc['k']):
            rows.append({'power': power, 'members': layout['members'], 'snaps': binding})
    rows.sort(key=lambda r: (-r['power'], r['members'], snap_key(r['snaps'])))
    return {'format': 'ournotes-deck.pt-power-certificate/1', 'globalPtCap': str(ceiling),
            'excludedMembers': len(caps) - len(kept), 'keptMembers': kept,
            'layouts': len(doc['layouts']), 'snaps': len(snaps), 'results': rows[:doc['k']],
            'verificationPending': 'Each power-leading proposal must attain the PT ceiling in the fixed evaluator and equal the original complete Top-K.'}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('input')
    parser.add_argument('output')
    args = parser.parse_args()
    source = Path(args.input)
    out = certify(json.loads(source.read_text(encoding='utf-8')))
    out['inputSha256'] = hashlib.sha256(source.read_bytes()).hexdigest()
    out['verifierSha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    Path(args.output).write_text(json.dumps(out, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: out[k] for k in ['globalPtCap', 'excludedMembers', 'layouts', 'snaps']}))


if __name__ == '__main__':
    main()
