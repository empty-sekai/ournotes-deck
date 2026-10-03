"""Record SHA-256 and sizes for explicitly supplied resources, without copying their content or paths.

Example: --resource client=/path/to/client --resource chart=/path/to/chart
Recording inputs is not a native-test pass. No supplied inputs yields status not_run.
"""
import argparse
import hashlib
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resource', action='append', default=[], metavar='ROLE=PATH')
    parser.add_argument('--client-version')
    parser.add_argument('--master-version')
    parser.add_argument('--region')
    parser.add_argument('--model-commit')
    parser.add_argument('--model-dirty', action='store_true')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    resources, missing, seen = [], [], set()
    for item in args.resource:
        if '=' not in item:
            parser.error('each resource must be ROLE=PATH')
        role, path = item.split('=', 1)
        if not role or role in seen:
            parser.error('resource roles must be nonempty and unique')
        seen.add(role)
        p = Path(path)
        if not p.is_file():
            missing.append(role)
            continue
        digest = hashlib.sha256()
        with p.open('rb') as f:
            for block in iter(lambda: f.read(4 * 1024 * 1024), b''):
                digest.update(block)
        resources.append({'role': role, 'bytes': p.stat().st_size, 'sha256': digest.hexdigest()})
    result = {'format': 'ournotes-deck.native-source-manifest/1',
              'status': 'not_run' if not resources or missing else 'inputs_recorded',
              'clientVersion': args.client_version, 'masterVersion': args.master_version,
              'region': args.region, 'modelCommit': args.model_commit, 'modelDirty': args.model_dirty,
              'resources': resources, 'missingRoles': missing,
              'qualification': 'Input identity only; no emulator execution or comparison performed.'}
    args.output.write_text(json.dumps(result, indent=2), encoding='utf8')
    print(json.dumps({'status': result['status'], 'recordedResources': len(resources), 'missingRoles': missing}))
    return 2 if result['status'] == 'not_run' else 0


if __name__ == '__main__':
    raise SystemExit(main())
