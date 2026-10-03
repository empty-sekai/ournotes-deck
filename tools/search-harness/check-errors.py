"""Exercise real harness refusal paths against an exported synthetic case."""
import json
import subprocess
import sys
from pathlib import Path

binary, corpus, output = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
case = json.loads((corpus / "free-score.json").read_text())
case["oracleMaxCandidates"] = 16
path = corpus / "oracle-cap-negative.json"
path.write_text(json.dumps(case))
process = subprocess.run([str(binary),str(path),str(output)], capture_output=True,text=True)
assert process.returncode == 2, process.stderr
assert "exceeds explicit cap" in process.stderr, process.stderr
assert not output.exists(), "capped oracle must not leave a complete report"
print(json.dumps({"case":"oracle-cap-refusal","passed":True,"error":json.loads(process.stderr)}))
