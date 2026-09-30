"""Focused whole-CLI explicit-deltaTimes acceptance; no native recapture/long oracle."""
import argparse
import copy
import hashlib
import json
import subprocess
import time
from pathlib import Path


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--native-config", type=Path)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    fixture = args.fixtures / "choice7"
    q = read(fixture / "request-gekisou-member-and-snap-choice.json")
    q["execution"]["play"] = {"kind": "stream", "stream": read(fixture / "play-gekisou-member-and-snap-choice.json")}
    q["constraints"] = {"leader": 1, "includeMembers": [1, 2, 3, 4, 5], "excludeMembers": [6, 7], "noSnaps": True}
    q["limits"] = {"timeLimitMs": 10000, "maxCandidates": 1000, "cacheEntries": 32}
    checks = []

    def run(name, request, data=None, roster=None, error=None, raw_token=None):
        path = args.out / (name + "-request.json")
        text = json.dumps(request, ensure_ascii=False, allow_nan=False)
        if raw_token:
            text = text.replace('"RAW_NON_FINITE"', raw_token)
        path.write_text(text, encoding="utf-8")
        argv = [str(args.binary), "--data", str(data or fixture / "DeckData.json"),
                "--roster", str(roster or fixture / "roster.json"), "--request", str(path)]
        start = time.perf_counter()
        process = subprocess.run(argv, capture_output=True, timeout=90)
        (args.out / (name + "-stdout.json")).write_bytes(process.stdout)
        (args.out / (name + "-stderr.json")).write_bytes(process.stderr)
        row = {"id": name, "wallMs": (time.perf_counter() - start) * 1000,
               "requestSha256": sha(path), "returncode": process.returncode}
        if error:
            assert process.returncode == 2, row
            result = json.loads(process.stderr)
            assert result["error"]["code"] == error, result
            assert not process.stdout.strip(), row
            row["error"] = result["error"]
        else:
            assert process.returncode == 0, (row, process.stderr.decode())
            result = json.loads(process.stdout)
            assert result["completion"] == "Complete" and result["optimality"] == "proven", result
            row.update({"completion": result["completion"], "evaluated": result["stats"]["evaluated"]})
        row["pass"] = True
        checks.append(row)
        return result

    baseline = run("omitted-documented-default-control", copy.deepcopy(q))
    frames = len(q["execution"]["play"]["stream"]["frames"])
    for name, dt in [("explicit-full-decimal", 1 / 60), ("explicit-f32-decimal", 0.016666668),
                     ("explicit-short-decimal", 0.016666667)]:
        request = copy.deepcopy(q)
        request["execution"]["play"]["stream"]["deltaTimes"] = [dt] * frames
        result = run(name, request)
        assert result["results"] == baseline["results"], name
    variable = copy.deepcopy(q)
    variable["execution"]["play"]["stream"]["deltaTimes"] = [[1/30, 1/120, 0.016666668][i % 3] for i in range(frames)]
    variable_result = run("explicit-variable-deltas", variable)
    for name, value in [
        ("rational-object", {"numerator": 1, "denominator": 60}),
        ("private-number-object", {"$serde_json::private::Number": "0.01"}),
        ("numeric-string", "0.016666668"), ("null-element", None), ("boolean-element", True),
        ("negative-step", -0.01), ("f32-overflow-step", 1e40),
    ]:
        request = copy.deepcopy(variable)
        request["execution"]["play"]["stream"]["deltaTimes"][0] = value
        run(name, request, error="Input")
    for token in ["NaN", "Infinity", "-Infinity"]:
        request = copy.deepcopy(variable)
        request["execution"]["play"]["stream"]["deltaTimes"][0] = "RAW_NON_FINITE"
        run("non-json-" + token, request, error="Input", raw_token=token)
    wrong_length = copy.deepcopy(variable)
    wrong_length["execution"]["play"]["stream"]["deltaTimes"].pop()
    run("wrong-delta-length", wrong_length, error="Input")
    unknown = copy.deepcopy(variable)
    unknown["constraints"]["bogusConstraint"] = True
    run("unknown-nested-constraint", unknown, error="Input")
    extra = copy.deepcopy(variable)
    extra["execution"]["musicId"] = None
    run("unused-variant-null-field", extra, error="Input")

    native = None
    if args.native_config:
        source = args.native_config / "production-request-explicit-deltaTimes-f32.json"
        request = read(source)
        original_k = request["k"]
        request["k"] = min(original_k, 100)  # Public K capacity; does not change the explicit stream.
        result = run("native-explicit-deltaTimes-fixed-contract", request,
                     args.native_config / "deck-data.json", args.native_config / "production-roster.json")
        winner = result["results"][0]
        assert result["stats"]["evaluated"] == 144
        assert winner["power"] == 75038 and winner["atoms"][0]["score"] == 704741, winner
        assert winner["members"] == [16, 17, 43, 19, 20]
        assert winner["snaps"] == [None, 42, None, None, None]
        assert winner["atoms"][0]["performanceOrder"] == [0, 3, 1, 2, 4]
        native = {"sourceRequestSha256": sha(source), "sourceK": original_k, "runK": request["k"],
                  "frames": len(request["execution"]["play"]["stream"]["frames"]),
                  "explicitDeltas": len(request["execution"]["play"]["stream"]["deltaTimes"]),
                  "nativeScore": 704741, "winner": winner,
                  "scope": "Actual CLI accepts explicit numeric f32 steps; native-certified one winner only, not all144 native captures"}
    report = {"format": "ournotes-deck.explicit-json-acceptance/1", "binarySha256": sha(args.binary),
              "pass": True, "checks": checks, "syntheticFrames": frames,
              "variableDiffersFromDefault": variable_result["results"] != baseline["results"],
              "variableOracle": "recommendation_scenarios::explicit_variable_delta_times_json_matches_typed_small_pool_oracle",
              "nativeExplicit": native}
    (args.out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"pass": True, "cases": len(checks), "binarySha256": report["binarySha256"], "nativeExplicit": bool(native)}))


if __name__ == "__main__":
    main()
