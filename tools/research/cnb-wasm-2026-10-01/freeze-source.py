from pathlib import Path
import json,subprocess,hashlib,sys
root=Path('/workspace/cache/member-owned-current4e-20261001');src=root/'source';mode=sys.argv[1]
files=subprocess.check_output(['git','ls-files','-c','-o','--exclude-standard'],cwd=src).decode().splitlines();sha=lambda b:hashlib.sha256(b).hexdigest()
rows=[{'path':n,'bytes':(src/n).stat().st_size,'sha256':sha((src/n).read_bytes())}for n in files if(src/n).is_file()]
if mode=='input':assert rows==json.loads((root/'controlled-input-manifest.json').read_text())['source']
if mode=='after':assert rows==json.loads((root/'source-before.json').read_text())['source']
(root/('source-'+mode+'.json')).write_text(json.dumps({'baseCommit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=src).decode().strip(),'source':rows},indent=2)+'\n');print(mode,len(rows),'SOURCE_EQUAL' if mode=='after' else 'FROZEN')