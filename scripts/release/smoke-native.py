"""Exercise the extracted shipping binaries on synthetic inputs."""
import hashlib
import json
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

archive = next(Path(sys.argv[1]).glob("ournotes-deck-*"))
corpus = Path(sys.argv[2]).resolve()
with tempfile.TemporaryDirectory() as temporary:
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as packed:
            packed.extractall(temporary)
    else:
        with tarfile.open(archive) as packed:
            packed.extractall(temporary, filter="data")
    root = next(Path(temporary).iterdir())
    metadata = json.loads((root / "build-info.json").read_text(encoding="utf-8"))
    for name, digest in metadata["files"].items():
        assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
    extension = ".exe" if "windows" in metadata["target"] else ""
    deck = str(root / ("ournotes-deck" + extension))
    assert not (root / ("ournotes-recommend" + extension)).exists()
    for command in [[deck, "--help"], [deck, "recommend", "--help"]]:
        help_result = subprocess.run(command, capture_output=True, encoding="utf-8")
        assert help_result.returncode == 0, help_result.stderr
        assert "--data" in help_result.stdout + help_result.stderr
    invalid = subprocess.run([deck, "recommend", "--unknown", "value"], capture_output=True, encoding="utf-8")
    assert invalid.returncode == 2
    assert json.loads(invalid.stderr)["error"]["code"] == "Input"
    for mode in ["power", "skip"]:
        command = [deck, mode, "--data", str(corpus / "DeckData.json"), "--roster", str(corpus / "roster.json"), "-k", "3"]
        if mode == "skip":
            command += ["--score", "1004"]
        result = json.loads(subprocess.check_output(command, encoding="utf-8"))
        assert result["completion"] == "Complete" and len(result["results"]) > 0, result
    for name in ["free-score", "free-pt", "mission-score", "mission-pt"]:
        request = corpus / (name + "-request.json")
        result = json.loads(subprocess.check_output([deck, "recommend", "--data", str(corpus / "DeckData.json"),
            "--roster", str(corpus / "roster.json"), "--request", str(request)], encoding="utf-8"))
        assert result["completion"] == "Complete" and len(result["results"]) == 3, name
    print(json.dumps({"target": metadata["target"], "commit": metadata["commit"], "cliCases": 6, "status": "passed"}))
