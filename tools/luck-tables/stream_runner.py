#!/usr/bin/env python3
"""Run a complete declared catalogue one chart at a time, with atomic status.

The default backend stores native controller programs, not repeated job curves.
All natural non-LUCK charts are explicitly not applicable; missions never change.
The classic entry backend is retained for comparison and uses per-chart stores.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import time


def module(name):
    spec = importlib.util.spec_from_file_location("luck_stream_" + name, Path(__file__).with_name(name + ".py"))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


pipeline, catalogue = module("pipeline"), module("catalogue")
FORMAT = "ournotes-deck.luck-catalogue-run/1"
MODES = ("lossless", "u16", "u24", "u32")


def file_receipt(path, root):
    raw = Path(path).read_bytes()
    return {"path": Path(path).resolve().relative_to(Path(root).resolve()).as_posix(),
            "sha256": pipeline.sha(raw), "bytes": len(raw)}


def verified_file(root, entry):
    path = (Path(root) / entry["path"]).resolve()
    if not path.is_relative_to(Path(root).resolve()):
        raise ValueError("receipt path escapes its output directory")
    raw = path.read_bytes()
    if pipeline.sha(raw) != entry["sha256"] or len(raw) != entry["bytes"]:
        raise ValueError("receipt bytes differ")
    return path


def verify_program_index(path, expected_mode, report_sha, source_version, fingerprints):
    index = pipeline.read(path)
    if (index.get("format") != "ournotes-deck.luck-program-index/1" or index.get("mode") != expected_mode
            or index.get("inputReportSha256") != report_sha or index.get("sourceVersion") != source_version
            or index.get("complete") is not True):
        raise ValueError("native program pack did not certify this complete report/mode/source")
    programs = index.get("programs", [])
    if (len(programs) != len(fingerprints)
            or {row.get("fingerprint") for row in programs} != set(fingerprints)):
        raise ValueError("packed program coverage differs from the generated program list")
    for row in programs:
        archive = row.get("archive", {})
        if (row.get("status") != "success" or row.get("sourceVersion") != source_version
                or archive.get("verifiedEntries") != 1 or archive.get("mode") != expected_mode):
            raise ValueError("incomplete packed program")
        verified_file(Path(path).parent, archive)
    return index


def validate_program_report(report, jobs, keys, source_version):
    if (report.get("format") != "ournotes-deck.luck-response-programs/1"
            or report.get("sourceVersion") != source_version or report.get("complete") is not True):
        raise ValueError("native program generation is incomplete or from a different source")
    native_keys = report.get("capabilities", {}).get("chain")
    if (not isinstance(native_keys, list) or len(native_keys) != len(keys)
            or {pipeline.canonical(k) for k in native_keys} != {pipeline.canonical(k) for k in keys}):
        raise ValueError("requested catalogue differs from the full native chain catalogue")
    rows = pipeline.job_results(report, jobs)
    programs = report.get("programs", [])
    fingerprints = [row.get("fingerprint") for row in programs]
    if (not programs or len(set(fingerprints)) != len(programs)
            or any(not isinstance(f, str) or not f for f in fingerprints)):
        raise ValueError("missing or duplicate compiled program identity")
    referenced = set()
    for row in rows.values():
        index = row.get("programIndex")
        if row.get("status") != "success" or type(index) is not int or not 0 <= index < len(programs):
            raise ValueError("not every labelled requested job has a completed compiled program")
        referenced.add(index)
    if referenced != set(range(len(programs))):
        raise ValueError("program report contains unreferenced programs")
    for program in programs:
        if program.get("sourceVersion") != source_version or program.get("response", {}).get("status") != "success":
            raise ValueError("program response is unavailable or from a different source")
    return fingerprints


def native_programs(generator, inputs, jobs, keys, source_version, output, timeout=None):
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    spec_path, report_path = output / "spec.json", output / "programs.json"
    pipeline.write(spec_path, {"mode": "programs", "jobs": jobs, "mcRuns": 0, "cacheBytes": 32 * 1024 * 1024})
    command = [str(Path(generator).resolve()), inputs["data"], inputs["snapshot"], inputs["request"],
               str(spec_path), str(report_path)]
    with (output / "generate.log").open("w") as log:
        subprocess.run(command, check=True, timeout=timeout, stdout=log, stderr=subprocess.STDOUT)
    report = pipeline.read(report_path)
    fingerprints = validate_program_report(report, jobs, keys, source_version)
    report_sha = pipeline.sha(report_path.read_bytes())
    receipts = []
    for mode in MODES:
        directory = output / "packed" / mode
        command = [str(Path(generator).resolve()), "program-pack", str(report_path), mode, str(directory)]
        with (output / ("pack-" + mode + ".log")).open("w") as log:
            subprocess.run(command, check=True, timeout=timeout, stdout=log, stderr=subprocess.STDOUT)
        path = directory / "program-index.json"
        index = verify_program_index(path, mode, report_sha, source_version, fingerprints)
        receipts.append({"mode": mode, "index": file_receipt(path, output),
                         "programs": len(index["programs"]),
                         "archiveBytes": sum(row["archive"]["bytes"] for row in index["programs"])})
    return {"jobs": len(jobs), "uniquePrograms": len(fingerprints), "sourceVersion": source_version,
            "programFingerprints": fingerprints, "report": file_receipt(report_path, output),
            "archives": receipts, "nativeCatalogueValidated": True}


def reusable_programs(directory, record):
    try:
        report = verified_file(directory, record["report"])
        fingerprints = record["programFingerprints"]
        if {x["mode"] for x in record["archives"]} != set(MODES) or len(record["archives"]) != len(MODES):
            return False
        for archive in record["archives"]:
            path = verified_file(directory, archive["index"])
            verify_program_index(path, archive["mode"], pipeline.sha(report.read_bytes()),
                                 record["sourceVersion"], fingerprints)
        return True
    except (OSError, ValueError, KeyError, TypeError):
        return False


def classic_chart(benchmark, case_name, generator, source, directory, batch_size, timeout):
    plan_path = directory / "plan.json"
    plan = pipeline.build_plan(benchmark, generator, source, plan_path, cases=case_name, corpus_mode="declared-manifest")
    count = sum(len(c["jobs"]) for c in plan["contexts"])
    run = pipeline.run_plan(plan_path, generator, directory / "store", batch_size or count, timeout)
    if not run["complete"]:
        raise ValueError("classic chart has unfinished DP entries; retained in its store")
    packed = pipeline.compact_plan(plan_path, generator, directory / "store", directory / "compact")
    if not packed["complete"]:
        raise ValueError("classic chart archives are incomplete")
    return {"jobs": count, "stats": run["stats"], "compact": file_receipt(directory / "compact/manifest.json", directory),
            "nativeCatalogueValidated": plan["coverage"]["nativeCatalogueValidated"]}


def run_catalogue(benchmark, generator, source, output, backend="programs", batch_size=None, timeout=None,
                  call_programs=native_programs, call_classic=classic_chart, shard_index=0, shard_count=1):
    if (type(shard_index) is not int or type(shard_count) is not int
            or not 1 <= shard_count <= 1024 or not 0 <= shard_index < shard_count):
        raise ValueError("invalid shard index/count")
    benchmark, output = Path(benchmark).resolve(), Path(output).resolve()
    manifest = pipeline.read(benchmark)
    declaration = manifest.get("luckCatalogue", {})
    if (manifest.get("format") != "ournotes-deck.search-benchmark/1"
            or declaration.get("format") != catalogue.INVENTORY_FORMAT
            or declaration.get("selection") != "all-master-source-levels"
            or declaration.get("includeNonLuckCharts") is not True):
        raise ValueError("a complete declared whole-catalogue manifest is required")
    if backend not in ("programs", "entries") or (batch_size is not None and batch_size < 1):
        raise ValueError("unknown backend or nonpositive batch size")
    cases = manifest["cases"]
    if not cases or len({case["name"] for case in cases}) != len(cases):
        raise ValueError("empty or duplicate chart cases")
    datasets = {str((benchmark.parent / case["data"]).resolve()) for case in cases}
    if len(datasets) != 1:
        raise ValueError("stream one regional dataset per manifest")
    data_path = Path(next(iter(datasets)))
    data_raw = data_path.read_bytes(); data = json.loads(data_raw)
    inv = catalogue.inventory(data, pipeline.sha(data_raw))
    if manifest.get("datasetId") != inv["datasetId"]:
        raise ValueError("manifest dataset pin differs")
    expected = {row["scoreId"]: row for row in inv["charts"]}
    chart_cases = {}
    for case in cases:
        request = pipeline.read(benchmark.parent / case["request"])
        score_id = request["execution"]["scoreId"]
        if score_id in chart_cases or score_id not in expected:
            raise ValueError("duplicate or unknown chart in manifest")
        chart = expected[score_id]
        if (request["scenario"] != {"kind": "mission", "musicId": chart["musicId"]}
                or request["execution"].get("kind") != "live" or request["execution"].get("gekisou") is not True):
            raise ValueError("manifest changes native music missions")
        if pipeline.luck_case(data, request) != chart["hasLuckMission"]:
            raise ValueError("request and master LUCK classification differ")
        chart_cases[score_id] = case
    if set(chart_cases) != set(expected):
        raise ValueError("manifest omits master charts")
    keys = declaration["keys"]
    if (len(keys) != len(inv["luckCatalogueKeys"])
            or {pipeline.canonical(k) for k in keys} != {pipeline.canonical(k) for k in inv["luckCatalogueKeys"]}):
        raise ValueError("manifest does not include every raw master source/level/formation key")
    jobs = pipeline.base_jobs(keys)
    algorithm = pipeline.source_identity(source)
    binary_sha = pipeline.sha(Path(generator).read_bytes())
    try:
        previous = pipeline.read(output / "manifest.json")
        old_rows = {row["name"]: row for row in previous["charts"]} if previous.get("format") == FORMAT else {}
    except (OSError, ValueError, KeyError, TypeError):
        old_rows = {}
    selected_charts = [(sid, case) for ordinal, (sid, case) in enumerate(sorted(chart_cases.items()))
                       if ordinal % shard_count == shard_index]
    result = {"format": FORMAT, "benchmarkSha256": pipeline.sha(benchmark.read_bytes()),
              "shard": {"index": shard_index, "count": shard_count, "ordering": "ascending-score-id-modulo",
                        "catalogueCharts": len(chart_cases), "selectedCharts": len(selected_charts)},
              "datasetId": inv["datasetId"], "generatorSha256": binary_sha, "algorithm": algorithm,
              "backend": backend, "coverage": {**inv["counts"], "allChartsDeclared": True,
                  "allMasterWriterLevels": True, "allTeamCombinationsCovered": False,
                  "playProfiles": "exact per-chart request bytes; generated defaults are theoreticalBest",
                  "performanceOrders": 120, "nonLuckPolicy": "explicit notApplicable from actual master missions"},
              "complete": False, "charts": [{"name": c["name"], "scoreId": sid,
                  "difficulty": expected[sid]["difficulty"], "status": "pending"} for sid, c in selected_charts]}
    pipeline.write(output / "manifest.json", result)
    for row in result["charts"]:
        case = chart_cases[row["scoreId"]]
        chart = expected[row["scoreId"]]
        inputs = {key: str((benchmark.parent / case[key]).resolve()) for key in ("data", "snapshot", "request")}
        hashes = pipeline.provenance(inputs)
        snapshot = pipeline.read(inputs["snapshot"])
        if snapshot.get("datasetId") != inv["datasetId"]:
            raise ValueError("chart anchor identifies different dataset bytes")
        row["inputSha256"] = hashes
        row["missions"] = chart["missions"]
        if not chart["hasLuckMission"]:
            row.update(status="notApplicable", reason="actual played mission ranges contain no LUCK mission",
                       jobs=0, nativeCalls=0)
            pipeline.write(output / "manifest.json", result)
            continue
        identity = pipeline.sha(pipeline.canonical({"inputs": hashes, "algorithm": algorithm["digest"],
                                                   "generator": binary_sha, "keys": keys, "backend": backend}))
        directory = output / "charts" / pipeline.sha(case["name"].encode())[:24]
        old = old_rows.get(row["name"], {})
        if (backend == "programs" and old.get("status") == "success" and old.get("identity") == identity
                and old.get("inputSha256") == hashes and old.get("jobs") == len(jobs)
                and old.get("sourceVersion") == algorithm["simSourceSha256"]
                and old.get("missions") == chart["missions"] and old.get("difficulty") == chart["difficulty"]
                and reusable_programs(directory, old)):
            row.update(old); row["reusedChart"] = True
            pipeline.write(output / "manifest.json", result)
            continue
        row.update(identity=identity, directory=directory.relative_to(output).as_posix(), status="running", reusedChart=False)
        pipeline.write(output / "manifest.json", result)
        start = time.monotonic()
        try:
            directory.mkdir(parents=True, exist_ok=True)
            if backend == "programs":
                details = call_programs(generator, inputs, jobs, keys, algorithm["simSourceSha256"], directory, timeout)
            else:
                details = call_classic(benchmark, case["name"], generator, source, directory, batch_size, timeout)
            row.update(details, status="success")
        except (OSError, ValueError, KeyError, IndexError, TypeError, subprocess.SubprocessError) as error:
            row.update(status="error", reason=pipeline.error_text(error))
        row["elapsedMs"] = (time.monotonic() - start) * 1000
        pipeline.write(output / "manifest.json", result)
    result["complete"] = all(row["status"] in ("success", "notApplicable") for row in result["charts"])
    result["summary"] = {status: sum(row["status"] == status for row in result["charts"])
                         for status in ("success", "notApplicable", "error", "pending", "running")}
    result["wholeCatalogueComplete"] = result["complete"] and shard_count == 1
    result["completionMeaning"] = "Complete applies only to this explicit shard. Every selected chart is accounted for; each applicable selected chart has all base/single catalogue entries and four verified archives. Whole-catalogue completion requires validated aggregation of every shard. No claim of all team combinations or arbitrary play-profile coverage."
    pipeline.write(output / "manifest.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("benchmark", type=Path)
    parser.add_argument("--generator", type=Path, required=True)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--backend", choices=("programs", "entries"), default="programs")
    parser.add_argument("--batch-size", type=int, help="classic entry backend only; defaults to whole chart")
    parser.add_argument("--timeout-seconds", type=float)
    parser.add_argument("--shard-index", type=int, default=0)
    parser.add_argument("--shard-count", type=int, default=1)
    args = parser.parse_args()
    result = run_catalogue(args.benchmark, args.generator, args.source, args.output, args.backend,
                           args.batch_size, args.timeout_seconds, shard_index=args.shard_index, shard_count=args.shard_count)
    print(json.dumps({"complete": result["complete"], "summary": result["summary"]}))
    return 0 if result["complete"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"luck catalogue: {error}", file=sys.stderr)
        sys.exit(2)
