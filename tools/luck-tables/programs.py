#!/usr/bin/env python3
"""Resolve real LUCK combinations through a persistent native-program dictionary.

Every request is compiled by the native simulator. Only missing complete programs
are propagated. No global list of skill combinations, scalar fitting, or Monte
Carlo is required. Imported probabilities are prediction data, never proof tokens.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import time


_spec = importlib.util.spec_from_file_location("luck_program_pipeline", Path(__file__).with_name("pipeline.py"))
pipeline = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pipeline)
INDEX = "ournotes-deck.luck-program-index/1"
REPORT = "ournotes-deck.luck-response-programs/1"
QUERY = "ournotes-deck.luck-program-query/1"
MODES = ("lossless", "u16", "u24", "u32")


def digest(value):
    if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
        raise ValueError("invalid SHA256 identity")
    return value


def load_index(path, source, mode):
    path = Path(path)
    if not path.exists():
        return {}
    index = pipeline.read(path)
    if (index.get("format") != INDEX or index.get("sourceVersion") != source
            or index.get("mode") != mode):
        raise ValueError("program dictionary source, format or quantization differs")
    rows = {}
    for row in index["programs"]:
        fingerprint = digest(row["fingerprint"])
        if fingerprint in rows or row.get("sourceVersion") != source:
            raise ValueError("duplicate or wrong-source program record")
        rows[fingerprint] = row
    return rows


def verified_blob(directory, row, source, mode):
    """Verify persisted bytes before reuse; the native reader also checks the codec context."""
    digest(row["fingerprint"])
    if row.get("sourceVersion") != source or row.get("status") != "success":
        raise ValueError("program response is unavailable or from a different source")
    archive = row["archive"]
    sha = digest(archive["sha256"])
    relative = "blobs/" + sha + ".onlrsp"
    if (archive.get("path") != relative or archive.get("mode") != mode
            or archive.get("verifiedEntries") != 1):
        raise ValueError("program blob identity or quantization differs")
    path = Path(directory) / relative
    if (type(archive.get("bytes")) is not int or not 0 < archive["bytes"] <= 64 * 1024 * 1024
            or path.stat().st_size != archive["bytes"]):
        raise ValueError("program blob length differs or exceeds its allowance")
    raw = path.read_bytes()
    if pipeline.sha(raw) != sha:
        raise ValueError("program blob checksum differs")
    return path, raw


def available(directory, row, source, mode):
    try:
        verified_blob(directory, row, source, mode)
        return True
    except (OSError, ValueError, KeyError, TypeError):
        return False


def save_index(directory, rows, source, mode):
    value = {"format": INDEX, "sourceVersion": source, "mode": mode,
             "programs": [rows[key] for key in sorted(rows)],
             "complete": False, "completionMeaning": "An open dictionary, not an enumeration of all combinations.",
             "nativeExpectationProven": False, "rankingProven": False}
    pipeline.write(Path(directory) / "program-index.json", value)


@contextmanager
def writer_lock(directory):
    # CI workers have separate directories; local overlapping queries still must not lose updates.
    import fcntl
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / ".writer.lock").open("a") as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ValueError("another query is updating this program dictionary") from error
        try:
            yield
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


def import_index(index_path, directory, rows, source, mode):
    incoming = load_index(index_path, source, mode)
    added = 0
    for fingerprint, row in incoming.items():
        _, raw = verified_blob(Path(index_path).parent, row, source, mode)
        if fingerprint in rows and available(directory, rows[fingerprint], source, mode):
            if rows[fingerprint]["archive"]["sha256"] != row["archive"]["sha256"]:
                raise ValueError("same native program has conflicting persisted response bytes")
            continue
        pipeline.atomic_bytes(Path(directory) / row["archive"]["path"], raw)
        rows[fingerprint] = row
        added += 1
    return added


def call(command, log, timeout):
    with Path(log).open("w") as output:
        subprocess.run([str(x) for x in command], check=True, timeout=timeout,
                       stdout=output, stderr=subprocess.STDOUT)


def identification(report, jobs):
    if (report.get("format") != REPORT or report.get("mode") != "identify"
            or report.get("identificationComplete") is not True or not jobs):
        raise ValueError("native identification is incomplete; inspect identify.json for per-job refusals")
    source = digest(report["sourceVersion"])
    rows = pipeline.job_results(report, jobs)
    programs = report["programs"]
    seen, references = set(), set()
    for ordinal, program in enumerate(programs):
        fingerprint = digest(program["fingerprint"])
        representative = program.get("representativeJobIndex")
        if (fingerprint in seen or program.get("sourceVersion") != source
                or type(representative) is not int or not 0 <= representative < len(jobs)):
            raise ValueError("native program identity or representative differs")
        seen.add(fingerprint)
        if rows[jobs[representative]["name"]].get("programIndex") != ordinal:
            raise ValueError("native representative does not identify its program")
    for row in rows.values():
        index = row.get("programIndex")
        if (type(index) is not int or not 0 <= index < len(programs)
                or row.get("programFingerprint") != programs[index]["fingerprint"]
                or row.get("status") != "identified"):
            raise ValueError("requested job has no complete native identity")
        references.add(index)
    if references != set(range(len(programs))):
        raise ValueError("native identification contains unused programs")
    return source, programs


def generation(report, jobs, expected, source):
    if (report.get("format") != REPORT or report.get("mode") != "programs"
            or report.get("sourceVersion") != source or report.get("complete") is not True
            or report.get("probabilityComplete") is not True):
        raise ValueError("missing-program propagation is incomplete or differs from native identification")
    rows = pipeline.job_results(report, jobs)
    programs = report["programs"]
    fingerprints = [digest(p["fingerprint"]) for p in programs]
    if (len(fingerprints) != len(expected) or len(set(fingerprints)) != len(fingerprints)
            or set(fingerprints) != set(expected.values())):
        raise ValueError("generated program coverage differs from the missing identities")
    for program in programs:
        if (program.get("sourceVersion") != source or program.get("status") != "success"
                or program.get("response", {}).get("status") != "success"):
            raise ValueError("generated program response is not successful")
    for name, row in rows.items():
        index = row.get("programIndex")
        if (row.get("status") != "success" or type(index) is not int or not 0 <= index < len(programs)
                or fingerprints[index] != expected[name] or row.get("programFingerprint") != expected[name]):
            raise ValueError("generated job does not reference its successful identified program")
    if report.get("stats", {}).get("propagationCalls") != len(programs):
        raise ValueError("native propagation count differs from the distinct missing programs")


def query(data, snapshot, request, spec, generator, store, output, mode="u24", generate=True,
          seed_indexes=(), timeout=None, decks=None, run_command=call):
    if mode not in MODES:
        raise ValueError("unknown quantization")
    started = time.monotonic()
    output = Path(output).resolve()
    inputs = {name: str(Path(value).resolve()) for name, value in
              (("data", data), ("snapshot", snapshot), ("request", request), ("spec", spec))}
    generator = Path(generator).resolve()
    if decks is not None:
        inputs["decks"] = str(Path(decks).resolve())
    # Input files remain immutable, including a caller's spec and account snapshot.
    if any(Path(path).is_relative_to(output) for path in inputs.values()) or generator.is_relative_to(output):
        raise ValueError("query output must not contain any input or generator file")
    before = pipeline.provenance(inputs)
    output.mkdir(parents=True, exist_ok=True)
    original = pipeline.read(spec)
    jobs = original.get("jobs", [])
    if (not isinstance(jobs, list) or not jobs or len(jobs) > 65536
            or len({row["name"] for row in jobs}) != len(jobs)):
        raise ValueError("provide 1..65536 uniquely named requested combination jobs")
    identified_spec = {**original, "mode": "identify", "mcRuns": 0, "scoreSamples": 0,
                       "validationDecks": []}
    pipeline.write(output / "identify-spec.json", identified_spec)
    result = {"format": QUERY, "complete": False, "inputSha256": before,
              "generatorSha256": pipeline.sha(generator.read_bytes()), "mode": mode,
              "nativeExpectationProven": False, "rankingProven": False,
              "allTeamCombinationsCovered": False, "usesMonteCarlo": False,
              "scope": "Requested native-compiled joint probabilities; only missing program responses are generated.",
              "stats": {"requestedJobs": len(jobs), "compiledQueries": 0, "uniquePrograms": 0,
                        "reusedPrograms": 0, "generatedPrograms": 0, "propagationCalls": 0}}
    pipeline.write(output / "query.json", result)
    base = [generator, inputs["data"], inputs["snapshot"], inputs["request"]]
    try:
        run_command(base + [output / "identify-spec.json", output / "identify.json"], output / "identify.log", timeout)
        report = pipeline.read(output / "identify.json")
        source, programs = identification(report, jobs)
        result["sourceVersion"] = source
        result["identificationStats"] = report["stats"]
        result["stats"].update(compiledQueries=len(jobs), uniquePrograms=len(programs))
        directory = Path(store).resolve() / source / mode
        if directory == output or directory.is_relative_to(output) or output.is_relative_to(directory):
            raise ValueError("query working output and persistent dictionary must be separate")
        with writer_lock(directory):
            index_path = directory / "program-index.json"
            rows = load_index(index_path, source, mode)
            for seed in seed_indexes:
                import_index(seed, directory, rows, source, mode)
            missing = [p for p in programs if not available(directory, rows.get(p["fingerprint"], {}), source, mode)]
            result["stats"]["reusedPrograms"] = len(programs) - len(missing)
            result["missingPrograms"] = [p["fingerprint"] for p in missing]
            if missing and generate:
                subset = [jobs[p["representativeJobIndex"]] for p in missing]
                pipeline.write(output / "missing-spec.json", {**identified_spec, "mode": "programs", "jobs": subset})
                run_command(base + [output / "missing-spec.json", output / "missing.json"], output / "missing.log", timeout)
                generated = pipeline.read(output / "missing.json")
                expected = {jobs[p["representativeJobIndex"]]["name"]: p["fingerprint"] for p in missing}
                generation(generated, subset, expected, source)
                result["generationStats"] = generated["stats"]
                result["stats"]["propagationCalls"] = generated["stats"]["propagationCalls"]
                run_command([generator, "program-pack", output / "missing.json", mode, output / "packed"],
                            output / "pack.log", timeout)
                result["stats"]["generatedPrograms"] = import_index(
                    output / "packed/program-index.json", directory, rows, source, mode)
                missing = [p for p in programs if not available(directory, rows.get(p["fingerprint"], {}), source, mode)]
            save_index(directory, rows, source, mode)
            result["missingPrograms"] = [p["fingerprint"] for p in missing]
            result["stats"]["storedPrograms"] = len(rows)
            if not missing:
                run_command([generator, "program-materialize", output / "identify.json", index_path,
                             output / "responses.json"], output / "materialize.log", timeout)
                run_command([generator, "pack", output / "responses.json", mode, output / "responses.onlrsp"],
                            output / "archive.log", timeout)
                result["archive"] = {"path": "responses.onlrsp", "sha256": pipeline.sha((output / "responses.onlrsp").read_bytes()),
                                     "bytes": (output / "responses.onlrsp").stat().st_size}
                result["complete"] = True
        if result["complete"] and decks is not None:
            run_command([generator, "predict", output / "responses.onlrsp", inputs["data"], inputs["snapshot"],
                         inputs["request"], inputs["decks"], output / "prediction.json"], output / "predict.log", timeout)
            prediction = pipeline.read(output / "prediction.json")
            result["prediction"] = {"status": prediction["status"], "ordersScored": prediction["ordersScored"]}
            result["complete"] &= prediction["status"] == "success"
        result["status"] = "success" if result["complete"] else "missing"
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        result["complete"] = False
        result.update(status="error", error=pipeline.error_text(error))
    finally:
        try:
            result["inputsUnchanged"] = pipeline.provenance(inputs) == before
        except OSError:
            result["inputsUnchanged"] = False
        if not result["inputsUnchanged"]:
            result.update(complete=False, status="error", error="input bytes changed during the query")
        result["elapsedMs"] = (time.monotonic() - started) * 1000
        pipeline.write(output / "query.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("data", "snapshot", "request", "spec"):
        parser.add_argument(name, type=Path)
    for name in ("generator", "store", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--mode", choices=MODES, default="u24")
    parser.add_argument("--no-generate", action="store_true", help="report missing programs without running DP")
    parser.add_argument("--seed-index", type=Path, action="append", default=[])
    parser.add_argument("--timeout-seconds", type=float)
    parser.add_argument("--decks", type=Path, help="optional real decks to predict from the requested response keys")
    args = parser.parse_args()
    result = query(args.data, args.snapshot, args.request, args.spec, args.generator, args.store, args.output,
                   args.mode, not args.no_generate, args.seed_index, args.timeout_seconds, args.decks)
    print(json.dumps({key: result[key] for key in ("complete", "status", "stats", "elapsedMs")}))
    return 0 if result["complete"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"luck program query: {error}", file=sys.stderr)
        sys.exit(2)
