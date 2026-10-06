"""Compare explicitly enumerated frame captures under one calculation-source identity."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re

from compare_native_frames import compare, resolve

FORMAT = "ournotes-deck.frame-validation-matrix/1"


def decode(raw):
    def object_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON field: {key}")
            result[key] = value
        return result

    def constant(value):
        raise ValueError(f"finite JSON numbers required: {value}")

    return json.loads(raw, object_pairs_hook=object_pairs, parse_constant=constant)


def positive(value, name):
    if type(value) is not int or value <= 0:
        raise ValueError(f"{name} must be a positive integer")
    return value


def validate_capture(capture, case, role):
    if not isinstance(capture, dict):
        raise ValueError(f"{role}: capture must be an object")
    if capture.get("inputIdentity") != case["inputIdentity"]:
        raise ValueError(f"{role}: input identity mismatch")
    if capture.get("context") != case["context"]:
        raise ValueError(f"{role}: context mismatch")
    frames = capture["frames"]
    if len(frames) != case["frameCount"]:
        raise ValueError(f"{role}: frame count mismatch")
    previous = None
    for index, frame in enumerate(frames):
        if not isinstance(frame, dict) or type(frame.get("frame")) is not int or frame["frame"] != index:
            raise ValueError(f"{role}: frame sequence must cover consecutive indexes from zero")
        time = frame.get("timeMs")
        if type(time) is not int or (previous is not None and time < previous):
            raise ValueError(f"{role}: frame times must be ordered integers")
        previous = time
    if frames[-1]["timeMs"] != case["finalTimeMs"]:
        raise ValueError(f"{role}: terminal time mismatch")


def run(manifest, directory):
    if not isinstance(manifest, dict) or manifest.get("format") != FORMAT:
        raise ValueError("unsupported frame matrix format")
    model = manifest["modelSourceSha256"]
    if not isinstance(model, str) or re.fullmatch(r"[0-9a-f]{64}", model) is None:
        raise ValueError("modelSourceSha256 must be a lowercase SHA-256 digest")
    cases = manifest["cases"]
    if not isinstance(cases, list) or not cases:
        raise ValueError("at least one explicit case is required")
    ids = []
    for case in cases:
        if not isinstance(case, dict):
            raise ValueError("each case must be an object")
        name = case["id"]
        if not isinstance(name, str) or not name or name in ids:
            raise ValueError("case identifiers must be nonempty and unique")
        ids.append(name)
        positive(case["frameCount"], "frameCount")
        if type(case["finalTimeMs"]) is not int:
            raise ValueError("finalTimeMs must be an integer")
        if not isinstance(case["inputIdentity"], str) or not case["inputIdentity"]:
            raise ValueError("inputIdentity must be nonempty")
        context = case["context"]
        if not isinstance(context, dict) or any(
            not isinstance(context.get(key), str) or not context[key]
            for key in ("region", "clientVersion", "masterVersion", "scenario", "play")
        ):
            raise ValueError("each case must declare its version, scenario and play context")
    results = []
    for case in cases:
        result = {"id": case["id"], "context": case["context"], "inputIdentity": case["inputIdentity"]}
        try:
            raw = {role: (directory / case[role]).read_bytes() for role in ("reference", "model", "contract")}
            reference, actual, contract = (decode(raw[role]) for role in ("reference", "model", "contract"))
            result["resources"] = {
                role: {"sha256": hashlib.sha256(value).hexdigest(), "bytes": len(value)} for role, value in raw.items()
            }
            if not isinstance(actual, dict) or actual.get("modelSourceSha256") != model:
                raise ValueError("calculation source identity mismatch")
            for role, capture in (("reference", reference), ("model", actual)):
                validate_capture(capture, case, role)
            fields = contract["integerFields"] + contract.get("float32BitFields", [])
            if not set(fields) - {"frame", "timeMs"}:
                raise ValueError("the contract must include a state field")
            state_fields = set(fields) - {"frame", "timeMs"}
            for role, capture in (("reference", reference), ("model", actual)):
                observed = any(resolve(frame, field.split("."))
                               for frame in capture["frames"] for field in state_fields)
                if not observed:
                    raise ValueError(f"{role}: state fields must select an observation")
            result.update(compare(reference, actual, contract))
        except FileNotFoundError:
            result.update(status="not_run", reason="a required case input is absent")
        except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
            reason = "case input cannot be read" if isinstance(error, OSError) else str(error)
            result.update(status="invalid", reason=reason)
        results.append(result)
    counts = {status: sum(row["status"] == status for row in results)
              for status in ("passed", "different", "incomplete", "invalid", "not_run")}
    status = "passed" if counts["passed"] == len(cases) else "incomplete" if any(
        counts[key] for key in ("incomplete", "invalid", "not_run")
    ) else "different"
    return {"format": FORMAT, "status": status, "modelSourceSha256": model,
            "caseCount": len(cases), "counts": counts,
            "fieldComparisons": sum(row.get("fieldComparisons", 0) for row in results), "cases": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        manifest_raw = args.manifest.read_bytes()
        result = run(decode(manifest_raw), args.manifest.parent)
        result["manifestSha256"] = hashlib.sha256(manifest_raw).hexdigest()
    except (OSError, ValueError, KeyError, TypeError) as error:
        result = {"format": FORMAT, "status": "invalid",
                  "reason": "manifest cannot be read" if isinstance(error, OSError) else str(error)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in result.items() if key != "cases"}))
    return 0 if result["status"] == "passed" else 1 if result["status"] == "different" else 2


if __name__ == "__main__":
    raise SystemExit(main())
