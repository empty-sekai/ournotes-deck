"""Run frozen cases, bind inputs/binary/source identities, and check that two reports return the same results."""
import argparse
import hashlib
import json
import platform
import os
import statistics
import subprocess
import sys
import time
from pathlib import Path


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def dump(path, value):
    Path(path).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def percentile(values, p):
    values = sorted(values)
    return values[min(len(values) - 1, max(0, __import__("math").ceil(p * len(values)) - 1))]


def identity(binary, source_root):
    files = {}
    # The scorer and search now live in workspace crates. Keep the legacy roots for older checkouts.
    for base in ("src", "tests", "crates", "tools/search-harness"):
        for path in sorted((source_root / base).rglob("*")):
            if path.is_file() and "target" not in path.parts and "__pycache__" not in path.parts:
                files[path.relative_to(source_root).as_posix()] = sha(path)
    for name in ("Cargo.toml", "Cargo.lock"):
        files[name] = sha(source_root / name)
    head = subprocess.run(["git", "-C", str(source_root), "rev-parse", "HEAD"], capture_output=True, text=True)
    return {"binarySha256": sha(binary), "sourceHead": head.stdout.strip() if head.returncode == 0 else os.environ.get("HARNESS_SOURCE_HEAD"),
            "sourceManifest": files, "host": platform.platform(), "python": platform.python_version()}


def summarize(report):
    experiments = []
    for item in report["experiments"]:
        samples = item["samples"]
        times = [s["wallMs"] for s in samples]
        experiments.append({"name": item["name"], "schedule":item.get("schedule","production"), "reducedDomain": item["reducedDomain"],
            "repeats": len(samples), "medianMs": statistics.median(times), "p95Ms": percentile(times, .95),
            "maxMs": max(times), "complete": sum(s["outcome"]["completion"] == "Complete" for s in samples),
            "matchesFullTopK": all(s["matchesFullDomainTopK"] for s in samples),
            "valuesVerified": all(s["returnedValuesVerified"] for s in samples),
            "gaps": [s["top1GapNumerator"] for s in samples],
            "telemetry": [s["outcome"]["telemetry"] for s in samples]})
    return {"case": report["case"], "oracle": {k:v for k,v in report["oracle"].items() if k != "topK"},
            "dominanceAudits": report["dominanceAudits"], "experiments": experiments}


def run(args):
    binary = Path(args.binary).resolve()
    suite = Path(args.suite).resolve()
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    doc = json.loads(suite.read_text(encoding="utf-8"))
    if doc["format"] != "ournotes-deck.search-harness-suite/1":
        raise ValueError("wrong suite format")
    provenance = identity(binary, Path(__file__).resolve().parents[2])
    reports = []
    started = time.monotonic()
    for filename in doc["cases"]:
        case_path = (suite.parent / filename).resolve()
        case = json.loads(case_path.read_text(encoding="utf-8"))
        destination = out / (case["id"] + ".json")
        inputs = {str(case_path): sha(case_path)}
        for field in ("data", "roster", "request"):
            path = (case_path.parent / case[field]).resolve()
            inputs[str(path)] = sha(path)
        print("Running", case["id"], flush=True)
        process = subprocess.run([str(binary), str(case_path), str(destination)], capture_output=True, text=True)
        if process.returncode:
            raise RuntimeError(f"{case['id']}: harness exited {process.returncode}: {process.stderr[-4000:]}")
        report = json.loads(destination.read_text(encoding="utf-8"))
        report["inputHashes"] = inputs
        report["inputIdentity"] = hashlib.sha256(json.dumps(
            {"case":sha(case_path), **{f:sha(case_path.parent/case[f]) for f in ("data","roster","request")}},
            sort_keys=True).encode()).hexdigest()
        report["runIdentity"] = provenance
        dump(destination, report)
        summary = summarize(report)
        reports.append(summary)
        print(json.dumps({"case":case["id"],"oracleCandidates":report["oracle"]["candidates"],
            "experiments":[{"name":r["name"],"medianMs":round(r["medianMs"],3),
                "matchesFullTopK":r["matchesFullTopK"]} for r in summary["experiments"]]}), flush=True)
    final = {"format":"ournotes-deck.search-harness-run/1", "scope":doc["scope"],
        "passed":True, "elapsedSeconds":time.monotonic()-started, "suiteSha256":sha(suite),
        "runIdentity":provenance, "reports":reports,
        "limits":"shared scoring model; bounded synthetic search checks; repeated timing is not production P95"}
    dump(out / "summary.json", final)


def compare(args):
    a = json.loads(Path(args.baseline).read_text(encoding="utf-8"))
    b = json.loads(Path(args.candidate).read_text(encoding="utf-8"))
    if a["inputIdentity"] != b["inputIdentity"]:
        raise ValueError("input identities differ; comparison rejected")
    if a["oracle"]["topK"] != b["oracle"]["topK"]:
        raise ValueError("fixed evaluator / canonical reference changed")
    pairs = []
    for x, y in zip(a["experiments"], b["experiments"], strict=True):
        if (x["name"],x["patch"],x.get("schedule","production")) != (y["name"],y["patch"],y.get("schedule","production")):
            raise ValueError("experiment contracts differ")
        results_equal = all(s["outcome"]["results"] == t["outcome"]["results"]
                            for s,t in zip(x["samples"],y["samples"],strict=True))
        pairs.append({"name":x["name"],"resultsEqual":results_equal})
    result = {"case":a["case"],"pairs":pairs,"passed":all(x["resultsEqual"] for x in pairs)}
    dump(args.out,result)
    if not result["passed"]:
        raise ValueError("A/B returned results differ")


def main():
    parser = argparse.ArgumentParser()
    modes = parser.add_subparsers(dest="mode",required=True)
    p = modes.add_parser("run")
    p.add_argument("--binary",required=True)
    p.add_argument("--suite",required=True)
    p.add_argument("--out",required=True)
    p = modes.add_parser("compare")
    p.add_argument("--baseline",required=True)
    p.add_argument("--candidate",required=True)
    p.add_argument("--out",required=True)
    args = parser.parse_args()
    try:
        run(args) if args.mode == "run" else compare(args)
    except (ValueError,RuntimeError,KeyError) as exc:
        print(str(exc),file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
