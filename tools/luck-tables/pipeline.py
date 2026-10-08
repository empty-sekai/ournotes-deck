#!/usr/bin/env python3
"""Content-addressed orchestration for real-chart LUCK response experiments.

The native generator owns dependency resolution and probability semantics. This
module selects declared skill levels, schedules missing objects, and keeps receipts.
It never changes a search request or treats fitted probabilities as search proofs.
"""
from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


PLAN_FORMAT = "ournotes-deck.luck-response-plan/1"
INDEX_FORMAT = "ournotes-deck.luck-response-index/1"
OBJECT_FORMAT = "ournotes-deck.luck-response-object/1"
PIPELINE_SCHEMA = "luck-response-pipeline/1"
SOURCES = {
    "gekisou": ("MasterGekisouSkillEffect", "_gekisouSkillID"),
    "gekisouSupport": ("MasterGekisouSupportSkillEffect", "_gekisouSupportSkillID"),
}


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def read(path):
    return json.loads(Path(path).read_bytes())


def atomic_bytes(path, raw):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix="." + path.name + ".", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as out:
            out.write(raw)
            out.flush()
            os.fsync(out.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def write(path, value):
    atomic_bytes(path, canonical(value) + b"\n")


def table(data, name):
    value = data["master"][name]
    columns = value["columns"]
    if len(columns) != len(set(columns)):
        raise ValueError(f"{name}: duplicate columns")
    if any(len(row) != len(columns) for row in value["rows"]):
        raise ValueError(f"{name}: row width differs")
    return [dict(zip(columns, row)) for row in value["rows"]]


def indexed(rows, fields=("_id",)):
    result = {}
    for row in rows:
        key = tuple(row[field] for field in fields)
        if key in result:
            raise ValueError(f"duplicate master identity {key}")
        result[key] = row
    return result


def selected_keys(data, snapshot):
    """Enumerate declared chain source/levels and both catalogue formation variants.

    This is scheduling, not native equivalence admission. The generator must
    independently validate every key and supply its complete dependency descriptor.
    """
    if snapshot.get("format") != "ournotes.owned-snapshot/1":
        raise ValueError("expected owned snapshot")
    members = indexed(table(data, "MasterMemberCard"))
    snaps = indexed(table(data, "MasterSupportCard"))
    ranks = indexed(table(data, "MasterSupportCardRank"), ("_group", "_rank"))
    selected = set()
    eligible = snapshot["eligible"]
    for member in eligible["members"]:
        row = members[(member["id"],)]
        if row["_gekisouSkillID"]:
            selected.add(("gekisou", row["_gekisouSkillID"], member["gekisouSkillLevel"]))
    for snap in eligible["snaps"]:
        row = snaps[(snap["id"],)]
        rank = ranks[(row["_supportCardRankGroup"], snap["rank"])]
        for position in ("01", "02"):
            skill = row["_gekisouSupportSkillId" + position]
            level = rank["_gekisouSupportSkill" + position + "Level"]
            if skill and level:
                selected.add(("gekisouSupport", skill, level))
    conditions = indexed(table(data, "MasterSkillCondition"))
    groups = {}
    for row in table(data, "MasterSkillConditionSet"):
        groups.setdefault(row["_group"], []).append(row)
    effects = {source: table(data, definition[0]) for source, definition in SOURCES.items()}
    out = []
    for source, skill, level in sorted(selected):
        rows = [r for r in effects[source] if r[SOURCES[source][1]] == skill and r["_level"] == level]
        if not rows:
            raise ValueError(f"selected skill has no rows: {source}/{skill}/{level}")
        if not any(11000 <= r["_skillEffectType"] <= 11005 for r in rows):
            continue
        formation = False
        for row in rows:
            # Native formation_targets selects variants from trigger/condition.
            # Release/reset remain dependencies, but do not create extra keys.
            for field in ("_skillTriggerConditionGroup", "_skillConditionGroup"):
                group = row[field]
                if group == 0:
                    continue
                if group not in groups:
                    raise ValueError(f"unknown condition group {group}")
                for clause in groups[group]:
                    for condition in clause["_conditionIds"]:
                        formation |= conditions[(condition,)]["_conditionType"] == 5000
        for matched in ([False, True] if formation else [None]):
            out.append({"source": source, "id": skill, "level": level, "matched": matched})
    return out


def luck_case(data, request):
    execution = request["execution"]
    if execution.get("kind") != "live" or execution.get("gekisou") is not True:
        return False
    scenario = request["scenario"]
    if scenario.get("kind") != "mission":
        raise ValueError("initial corpus planner supports the declared Mission requests only")
    music = indexed(table(data, "MasterLiveMusic"))[(scenario["musicId"],)]
    score_id = execution["scoreId"]
    if score_id not in [music["_" + difficulty + "ID"] for difficulty in ("easy", "normal", "hard", "expert")]:
        raise ValueError("request chart is not part of its declared music")
    charts = [chart for chart in data["charts"] if chart["scoreId"] == score_id]
    if len(charts) != 1:
        raise ValueError("missing or duplicate chart")
    count = len(charts[0]["fevers"]["startMs"])
    if count > 3:
        raise ValueError("more than three native mission ranges")
    return any(music["_gekisouMission" + str(i + 1)] == 2 for i in range(count))


def job_name(entries):
    return "base" if not entries else "entry-" + sha(canonical(entries))[:24]


def base_jobs(keys):
    entries = [[]] + [[[key, position]] for key in keys for position in range(5)]
    return [{"name": job_name(entry), "entries": entry} for entry in entries]


def validate_entries(entries, available):
    allowed = {canonical(key) for key in available}
    held = [[] for _ in range(5)]
    for item in entries:
        if not isinstance(item, list) or len(item) != 2:
            raise ValueError("entry must be [LuckSkillKey, position]")
        key, position = item
        if type(position) is not int or not 0 <= position < 5 or canonical(key) not in allowed:
            raise ValueError("combination uses an unavailable key or position")
        held[position].append(key)
    for keys in held:
        if sum(k["source"] == "gekisou" for k in keys) > 1 or sum(k["source"] == "gekisouSupport" for k in keys) > 2:
            raise ValueError("combination exceeds native holder capacity")


def source_identity(source):
    """Conservative semantic-source invalidation, independent of data provenance."""
    source = Path(source).resolve()
    paths = set(source.glob("crates/**/*.rs")) | set(source.glob("crates/**/Cargo.toml"))
    paths.update(source / name for name in ("Cargo.toml", "Cargo.lock", "tools/search-harness/Cargo.toml",
                                          "tools/search-harness/Cargo.lock"))
    paths.update(source.glob("tools/search-harness/src/bin/luck_response*.rs"))
    paths.update(source.glob("tools/search-harness/src/bin/luck_response/**/*.rs"))
    if not paths or any(not path.is_file() for path in paths):
        raise ValueError("incomplete algorithm source tree")
    files = {path.relative_to(source).as_posix(): sha(path.read_bytes()) for path in sorted(paths)}
    return {"schema": PIPELINE_SCHEMA, "digest": sha(canonical(files)), "files": files,
            "simSourceSha256": sim_source_identity(source)}


def sim_source_identity(source):
    """Mirror ournotes-sim/build.rs's length-prefixed immutable source receipt."""
    root = Path(source) / "crates/ournotes-sim"
    paths = [root / "Cargo.toml", root / "build.rs"] + [path for path in (root / "src").rglob("*") if path.is_file()]
    digest = hashlib.sha256()
    for path in sorted(paths, key=lambda path: path.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix().encode()
        raw = path.read_bytes()
        digest.update(len(relative).to_bytes(8, "little"))
        digest.update(relative)
        digest.update(len(raw).to_bytes(8, "little"))
        digest.update(raw)
    return digest.hexdigest()


def object_path(store, digest):
    if not valid_digest(digest):
        raise ValueError("invalid object digest")
    return Path(store) / "objects" / digest[:2] / (digest + ".json")


def valid_digest(value):
    return isinstance(value, str) and len(value) == 64 and all(c in "0123456789abcdef" for c in value)


def verified_report(store, digest, cache=None):
    if not valid_digest(digest):
        raise ValueError("invalid report digest")
    if cache is not None and digest in cache:
        return cache[digest]
    raw = (Path(store) / "reports" / (digest + ".json")).read_bytes()
    if sha(raw) != digest:
        raise ValueError("report content digest differs")
    report = json.loads(raw)
    if cache is not None:
        cache[digest] = report
    return report


def cached_object(store, record, task_id, report_cache=None):
    try:
        raw = object_path(store, record["objectSha256"]).read_bytes()
        value = json.loads(raw)
        if sha(raw) != record["objectSha256"] or value["format"] != OBJECT_FORMAT or value["taskId"] != task_id:
            return None
        if value["result"].get("status") != "success":
            return None
        reference = value["entryRef"]
        if not valid_digest(reference["reportSha256"]) or not valid_digest(reference["entrySha256"]):
            return None
        entry = verified_report(store, reference["reportSha256"], report_cache)["table"]["entries"][reference["entryIndex"]]
        if (sha(canonical(entry)) != reference["entrySha256"] or entry["key"] != value["descriptor"]["entries"]
                or entry["response"].get("status") != "success"):
            return None
        for archive in value.get("archiveReceipts", []):
            if not valid_digest(archive["sha256"]):
                return None
            path = Path(store) / "blobs" / (archive["sha256"] + ".onlrsp")
            if path.stat().st_size != archive["bytes"] or sha(path.read_bytes()) != archive["sha256"]:
                return None
        return value
    except (OSError, ValueError, KeyError, TypeError, IndexError):
        return None


def put_object(store, value):
    raw = canonical(value) + b"\n"
    digest = sha(raw)
    path = object_path(store, digest)
    if path.exists() and path.read_bytes() == raw:
        return digest
    # A corrupted file at this digest is replaced atomically; valid immutable
    # successful objects are never overwritten with a different payload.
    atomic_bytes(path, raw)
    return digest


def cached_refusal(store, record, job):
    if record.get("status") != "unsupported":
        return False
    try:
        digest = record["reportSha256"]
        if not valid_digest(digest):
            return False
        raw = (Path(store) / "reports" / (digest + ".json")).read_bytes()
        if sha(raw) != digest:
            return False
        report = json.loads(raw)
        return any(row.get("status") == "unsupported" and row["entries"] == job["spec"]["entries"]
                   and row["dependencyDescriptor"] == job["descriptor"]["job"] for row in report["jobs"])
    except (OSError, ValueError, KeyError, TypeError):
        return False


def task_identity(algorithm, context_dependencies, job_dependencies, entries, mc_runs=0):
    descriptor = {"format": "ournotes-deck.luck-response-task/1", "algorithm": algorithm["digest"],
                  "pipelineSchema": PIPELINE_SCHEMA, "contextSha256": sha(canonical(context_dependencies)),
                  "job": job_dependencies, "entries": entries, "backend": "certified-dp", "mcRuns": mc_runs}
    return sha(canonical(descriptor)), descriptor


def native_call(generator, inputs, spec, work, timeout=None, store=None):
    work = Path(work)
    work.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="native-", dir=work) as temporary:
        temporary = Path(temporary)
        spec_path, output = temporary / "spec.json", temporary / "output.json"
        write(spec_path, spec)
        command = [str(Path(generator).resolve()), inputs["data"], inputs["snapshot"], inputs["request"],
                   str(spec_path), str(output)]
        subprocess.run(command, check=True, timeout=timeout, stdout=sys.stderr)
        result = read(output)
        if result.get("format") != "ournotes-deck.luck-response-generation/1":
            raise ValueError("native generator returned a different protocol")
        advertised = result.get("archives", [])
        modes = [archive["mode"] for archive in advertised]
        if len(set(modes)) != len(modes):
            raise ValueError("native archive manifest repeats a mode")
        if spec.get("mode") == "generate" and set(modes) != {"lossless", "u16", "u24", "u32"}:
            raise ValueError("native generation omitted an archive outcome")
        archives = []
        for archive in advertised:
            status = archive.get("status")
            if status == "error":
                # Completed probability entries remain reusable even when one
                # codec mode exceeded capacity. Compact can retry without DP.
                continue
            if status != "success" or store is None:
                raise ValueError("unexpected native archive status or plan archive")
            mode = archive["mode"]
            if mode not in {"lossless", "u16", "u24", "u32"}:
                raise ValueError("unknown native archive mode")
            path = temporary / ("output.json." + mode + ".onlrsp")
            raw = path.read_bytes()
            digest = sha(raw)
            if (digest, len(raw)) != (archive["sha256"], archive["bytes"]):
                raise ValueError("native archive content hash differs")
            destination = Path(store) / "blobs" / (digest + ".onlrsp")
            if not destination.exists() or sha(destination.read_bytes()) != digest:
                atomic_bytes(destination, raw)
            archives.append({"mode": mode, "sha256": digest, "bytes": len(raw),
                             "path": destination.relative_to(store).as_posix()})
        # Native paths point into the temporary execution directory. The portable
        # archive receipts refer only to verified content-addressed objects.
        result["archiveReceipts"] = archives
        for archive in result.get("archives", []):
            archive.pop("path", None)
        return result


def provenance(inputs):
    return {name: sha(Path(path).read_bytes()) for name, path in inputs.items()}


def job_results(report, expected):
    rows = report.get("jobs", [])
    result = {row["name"]: row for row in rows}
    if len(result) != len(rows) or set(result) != {row["name"] for row in expected}:
        raise ValueError("native result does not cover every requested job exactly once")
    for job in expected:
        if result[job["name"]]["entries"] != job["entries"]:
            raise ValueError("native result changed the labelled entry key")
    return result


def extra_jobs(spec, case_name, available):
    result = []
    if spec is None:
        return result
    if spec.get("format") != "ournotes-deck.luck-response-combinations/1":
        raise ValueError("unsupported combination specification")
    for group in spec["groups"]:
        if case_name not in group["cases"]:
            continue
        jobs = group.get("jobs", [])
        if "positionSubsets" in group:
            recipe = group["positionSubsets"]
            positions = recipe["positions"]
            if len(positions) != len(set(positions)) or any(type(p) is not int or not 0 <= p < 5 for p in positions):
                raise ValueError("invalid explicit position subsets")
            if (len(recipe["copies"]) != len(set(recipe["copies"]))
                    or any(type(count) is not int or not 0 <= count <= len(positions) for count in recipe["copies"])):
                raise ValueError("invalid explicit subset sizes")
            jobs = jobs + [
                {"name": "subset-" + "-".join(map(str, selected)),
                 "entries": [[recipe["skill"], position] for position in selected]}
                for count in recipe["copies"] for selected in itertools.combinations(positions, count)
            ]
        for job in jobs:
            validate_entries(job["entries"], available)
            result.append({"name": job_name(job["entries"]), "entries": job["entries"]})
    return result


def build_plan(benchmark, generator, source, output, combinations=None, cases="all", mc_runs=0,
               corpus_mode="real-benchmark", call=native_call):
    benchmark = Path(benchmark).resolve()
    manifest = read(benchmark)
    if manifest.get("format") != "ournotes-deck.search-benchmark/1":
        raise ValueError("unsupported benchmark manifest")
    names = [entry["name"] for entry in manifest["cases"]]
    if not names or len(names) != len(set(names)):
        raise ValueError("manifest cases must be nonempty and unique")
    if corpus_mode == "real-benchmark":
        if manifest.get("matrixId") != "full48" or len(names) != 48:
            raise ValueError("the complete original full48 manifest is required before selection")
    elif corpus_mode != "declared-manifest":
        raise ValueError("unknown corpus mode")
    selected = set(names if cases == "all" else cases.split(","))
    if not selected or not selected <= set(names):
        raise ValueError("unknown or empty case selection")
    extra = read(combinations) if combinations else None
    if extra:
        if any(not group["cases"] or not set(group["cases"]) <= set(names) for group in extra.get("groups", [])):
            raise ValueError("combination specification names an unknown or empty case group")
    algorithm = source_identity(source)
    generator_sha = sha(Path(generator).read_bytes())
    contexts, data_cache, included, controls = {}, {}, [], []
    for case in manifest["cases"]:
        if case["name"] not in selected:
            continue
        inputs = {field: str((benchmark.parent / case[field]).resolve()) for field in ("data", "snapshot", "request")}
        if inputs["data"] not in data_cache:
            data_cache[inputs["data"]] = read(inputs["data"])
        data = data_cache[inputs["data"]]
        snapshot, request = read(inputs["snapshot"]), read(inputs["request"])
        hashes = provenance(inputs)
        if snapshot.get("datasetId") != hashes["data"] or manifest.get("datasetId") != hashes["data"]:
            raise ValueError("original snapshot/manifest dataset identity differs")
        if corpus_mode == "real-benchmark":
            if (request.get("k") != 3 or request.get("constraints") != {}
                    or request.get("limits") != {"timeLimitMs": 60000, "maxCandidates": None, "cacheEntries": 1024}):
                raise ValueError("original full48 request limits or constraints changed")
        if not luck_case(data, request):
            controls.append(case["name"])
            continue
        keys = selected_keys(data, snapshot)
        jobs = base_jobs(keys) + extra_jobs(extra, case["name"], keys)
        jobs = list({canonical(job["entries"]): job for job in jobs}.values())
        spec = {"mode": "plan", "jobs": jobs, "mcRuns": mc_runs}
        report = call(generator, inputs, spec, Path(output).parent / "planning")
        rows = job_results(report, jobs)
        dependencies = report["dependencyDescriptor"]
        if dependencies["algorithm"]["simSourceSha256"] != algorithm["simSourceSha256"]:
            raise ValueError("native binary embeds a different simulation source identity")
        # A native resolved context may be shared across inventory or objective
        # changes only if its complete dependency descriptor is identical.
        context_id = sha(canonical({"algorithm": algorithm["digest"], "dependencies": dependencies}))
        context = contexts.setdefault(context_id, {
            "id": context_id, "sharedFingerprint": report["sharedFingerprint"], "dependencyDescriptor": dependencies,
            "inputs": inputs, "cases": [], "jobs": {},
        })
        if context["sharedFingerprint"] != report["sharedFingerprint"]:
            raise ValueError("same dependencies produced different native shared identities")
        context["cases"].append({"name": case["name"], "profile": case.get("profile"),
                                  "scoreId": request["execution"]["scoreId"], "inputSha256": hashes})
        for job in jobs:
            row = rows[job["name"]]
            if row.get("status") != "planned":
                raise ValueError(f"native plan did not resolve {job['name']}: {row.get('status')}")
            task_id, descriptor = task_identity(algorithm, dependencies, row["dependencyDescriptor"], job["entries"], mc_runs)
            task = context["jobs"].setdefault(task_id, {"id": task_id, "spec": job, "descriptor": descriptor,
                                                        "referencedBy": []})
            task["referencedBy"].append(case["name"])
        included.append(case["name"])
    if not included:
        raise ValueError("selection contains no LUCK Mission request")
    result = {"format": PLAN_FORMAT, "algorithm": algorithm, "generatorSha256": generator_sha,
              "mcRuns": mc_runs, "corpusMode": corpus_mode,
              "provenance": {"manifestSha256": sha(benchmark.read_bytes()), "manifestCases": names,
                             "combinationSpecSha256": sha(Path(combinations).read_bytes()) if combinations else None},
              "coverage": {"requestedCases": sorted(selected), "luckCases": included, "nonLuckControls": controls,
                           "meaning": "base and declared single-skill position kernels plus explicit combinations; not full-team combination coverage",
                           "searchDomainChanged": False, "performanceOrders": 120},
              "contexts": [{**context, "jobs": sorted(context["jobs"].values(), key=lambda job: job["id"])}
                           for context in contexts.values()]}
    result["coverage"].update(contexts=len(result["contexts"]), jobs=sum(len(c["jobs"]) for c in result["contexts"]))
    write(output, result)
    return result


def run_plan(plan_path, generator, store, batch_size=128, timeout=None, retry_failures=False, call=native_call):
    if batch_size < 1:
        raise ValueError("batch size must be positive")
    plan = read(plan_path)
    if plan.get("format") != PLAN_FORMAT:
        raise ValueError("unsupported plan")
    if sha(Path(generator).read_bytes()) != plan["generatorSha256"]:
        raise ValueError("generator bytes changed after planning")
    store = Path(store).resolve()
    index_path = store / "index.json"
    try:
        index = read(index_path)
        if (not isinstance(index, dict) or index.get("format") != INDEX_FORMAT
                or not isinstance(index.get("tasks"), dict)
                or any(not isinstance(record, dict) for record in index["tasks"].values())):
            raise ValueError("index format")
    except (OSError, ValueError):
        index = {"format": INDEX_FORMAT, "tasks": {}}
    stats = {"reused": 0, "generated": 0, "failed": 0, "retainedFailures": 0, "nativeBatches": 0}
    for context in plan["contexts"]:
        put_dependency(store, context["dependencyDescriptor"])
        report_cache = {}
        # Plans are ephemeral execution descriptions; changing inputs requires a
        # fresh native plan, even when a dependency-equivalent old object exists.
        if provenance(context["inputs"]) != context["cases"][0]["inputSha256"]:
            raise ValueError("input bytes changed after planning")
        missing = []
        for job in context["jobs"]:
            record = index["tasks"].get(job["id"], {})
            if cached_object(store, record, job["id"], report_cache) is not None:
                stats["reused"] += 1
            elif cached_refusal(store, record, job) and not retry_failures:
                # Structural refusal is visible, never counted as a usable table.
                stats["retainedFailures"] += 1
            else:
                missing.append(job)
        for start in range(0, len(missing), batch_size):
            batch = missing[start:start + batch_size]
            expected = [job["spec"] for job in batch]
            spec = {"mode": "generate", "jobs": expected, "mcRuns": plan["mcRuns"]}
            stats["nativeBatches"] += 1
            try:
                report = call(generator, context["inputs"], spec, store / "work", timeout=timeout, store=store)
                rows = job_results(report, expected)
                if (report["dependencyDescriptor"] != context["dependencyDescriptor"]
                        or report["sharedFingerprint"] != context["sharedFingerprint"]):
                    raise ValueError("native generation dependencies differ from its plan")
                report_hash = put_report(store, report)
                for job in batch:
                    row = rows[job["spec"]["name"]]
                    if row["dependencyDescriptor"] != job["descriptor"]["job"]:
                        raise ValueError("native job dependencies differ from plan")
                    status = row.get("status")
                    record = {"status": status, "reportSha256": report_hash, "referencedBy": job["referencedBy"]}
                    if status == "success":
                        entry = report["table"]["entries"][row["entryIndex"]]
                        if entry["key"] != job["spec"]["entries"]:
                            raise ValueError("native table entry differs from requested labels")
                        if entry["response"].get("status") != "success":
                            raise ValueError("native job and response statuses differ")
                        value = {"format": OBJECT_FORMAT, "taskId": job["id"], "descriptor": job["descriptor"],
                                 "context": report["context"], "result": row,
                                 "entryRef": {"reportSha256": report_hash, "entryIndex": row["entryIndex"],
                                              "entrySha256": sha(canonical(entry))},
                                 "archiveReceipts": report.get("archiveReceipts", [])}
                        record["objectSha256"] = put_object(store, value)
                        stats["generated"] += 1
                    elif status in ("unsupported", "capacity", "error"):
                        record["reason"] = row.get("reason", row.get("error", status))
                        stats["failed"] += 1
                    else:
                        raise ValueError(f"unknown generation status {status}")
                    index["tasks"][job["id"]] = record
                    write(index_path, index)
            except (OSError, subprocess.SubprocessError, ValueError, KeyError, IndexError, TypeError) as error:
                for job in batch:
                    # Do not erase an earlier completed entry if a later response
                    # item is malformed or the subprocess was interrupted.
                    if cached_object(store, index["tasks"].get(job["id"], {}), job["id"], report_cache) is not None:
                        continue
                    index["tasks"][job["id"]] = {"status": "error", "reason": error_text(error),
                                                    "referencedBy": job["referencedBy"]}
                    stats["failed"] += 1
                write(index_path, index)
    wanted = [job["id"] for context in plan["contexts"] for job in context["jobs"]]
    statuses = {task: index["tasks"].get(task, {"status": "missing"})["status"] for task in wanted}
    receipt = {"format": "ournotes-deck.luck-response-run/1", "planSha256": sha(Path(plan_path).read_bytes()),
               "coverage": plan["coverage"], "stats": stats, "statuses": statuses,
               "completionMeaning": "all selected DP response entries; complete archives require compact success",
               "complete": all(status == "success" for status in statuses.values())}
    write(store / "run-receipt.json", receipt)
    return receipt


def put_report(store, report):
    context_digest = put_dependency(store, report["dependencyDescriptor"])
    compact = {key: value for key, value in report.items() if key != "dependencyDescriptor"}
    compact["dependencyDescriptorRef"] = context_digest
    if "provenance" in compact:
        compact["provenance"] = dict(compact["provenance"])
        if "inputs" in compact["provenance"]:
            inputs = dict(compact["provenance"]["inputs"])
            for field in ("data", "roster", "snapshot", "request", "spec"):
                if isinstance(inputs.get(field), str):
                    inputs[field] = Path(inputs[field]).name
            compact["provenance"]["inputs"] = inputs
    raw = canonical(compact) + b"\n"
    digest = sha(raw)
    path = Path(store) / "reports" / (digest + ".json")
    if not path.exists() or sha(path.read_bytes()) != digest:
        atomic_bytes(path, raw)
    return digest


def error_text(error):
    if isinstance(error, subprocess.TimeoutExpired):
        return "native generator timed out before publishing its complete batch"
    if isinstance(error, subprocess.CalledProcessError):
        return f"native generator exited with status {error.returncode} before publishing a complete batch"
    if isinstance(error, OSError):
        return f"{type(error).__name__}: {error.strerror}"
    return str(error)


def put_dependency(store, descriptor):
    raw = canonical(descriptor)
    digest = sha(raw)
    path = Path(store) / "contexts" / (digest + ".json")
    if not path.exists() or sha(path.read_bytes()) != digest:
        atomic_bytes(path, raw)
    return digest


def pack_table(generator, table, mode, work):
    """Pack and independently verify every key through the native codec CLI."""
    work = Path(work)
    work.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="pack-", dir=work) as temporary:
        temporary = Path(temporary)
        source, archive = temporary / "table.json", temporary / (mode + ".onlrsp")
        write(source, table)
        result = subprocess.run([str(Path(generator).resolve()), "pack", str(source), mode, str(archive)],
                                check=True, capture_output=True, text=True)
        metadata = json.loads(result.stdout)
        raw = archive.read_bytes()
        if (metadata.get("mode") != mode or metadata.get("sha256") != sha(raw)
                or metadata.get("bytes") != len(raw) or metadata.get("verifiedEntries") != len(table["entries"])):
            raise ValueError("native compact archive failed its content/coverage receipt")
        metadata.pop("path", None)
        return raw, metadata


def compact_plan(plan_path, generator, store, output, call=native_call, pack=pack_table):
    """Merge completed batches without DP; native planning owns archive identity."""
    plan = read(plan_path)
    if plan.get("format") != PLAN_FORMAT or sha(Path(generator).read_bytes()) != plan["generatorSha256"]:
        raise ValueError("compact needs the same valid plan and generator")
    store, output = Path(store).resolve(), Path(output).resolve()
    index = read(store / "index.json")
    if index.get("format") != INDEX_FORMAT:
        raise ValueError("unsupported store index")
    contexts, missing = [], []
    # Full preflight: an old completed archive is never presented as covering a
    # new plan whose extra combination is still absent or refused.
    for context in plan["contexts"]:
        if provenance(context["inputs"]) != context["cases"][0]["inputSha256"]:
            raise ValueError("original inputs changed before compaction")
        reports = {}
        for job in context["jobs"]:
            record = index["tasks"].get(job["id"], {})
            value = cached_object(store, record, job["id"], reports)
            if value is None:
                missing.append({"taskId": job["id"], "status": record.get("status", "missing"),
                                "reason": "completed object, report, or archive is missing or invalid"})
                continue
        contexts.append(context)
    manifest = {"format": "ournotes-deck.luck-response-compact/1", "planSha256": sha(Path(plan_path).read_bytes()),
                "algorithmDigest": plan.get("algorithm", {}).get("digest"), "generatorSha256": plan["generatorSha256"],
                "coverage": plan["coverage"], "complete": False, "missingTasks": missing, "contexts": []}
    if missing:
        write(output / "manifest.json", manifest)
        return manifest
    for context in contexts:
        row = {"id": context["id"], "sharedFingerprint": context["sharedFingerprint"],
               "contextSha256": sha(canonical(context["dependencyDescriptor"])), "cases": context["cases"],
               "tasks": [{"id": job["id"], "entries": job["spec"]["entries"]} for job in context["jobs"]],
               "status": "error", "archives": []}
        try:
            entries, reports = [], {}
            for job in context["jobs"]:
                value = cached_object(store, index["tasks"][job["id"]], job["id"], reports)
                if value is None:
                    raise ValueError("cached entry changed after compact preflight")
                ref = value["entryRef"]
                entries.append(reports[ref["reportSha256"]]["table"]["entries"][ref["entryIndex"]])
            jobs = [job["spec"] for job in context["jobs"]]
            report = call(generator, context["inputs"], {"mode": "plan", "jobs": jobs, "mcRuns": 0}, output / "work")
            resolved = job_results(report, jobs)
            if (report["sharedFingerprint"] != context["sharedFingerprint"]
                    or report["dependencyDescriptor"] != context["dependencyDescriptor"]):
                raise ValueError("native compact context dependencies differ from generation")
            for job in context["jobs"]:
                native = resolved[job["spec"]["name"]]
                if native.get("status") != "planned" or native["dependencyDescriptor"] != job["descriptor"]["job"]:
                    raise ValueError("native compact entry dependencies differ from generation")
            table = {"context": report["context"], "entries": entries}
            row["context"] = report["context"]
            for mode in ("lossless", "u16", "u24", "u32"):
                raw, metadata = pack(generator, table, mode, output / "work")
                digest = sha(raw)
                # Keep the validation boundary even when a different pack
                # transport is injected by an embedding caller.
                if (metadata.get("sha256") != digest or metadata.get("bytes") != len(raw)
                        or metadata.get("verifiedEntries") != len(entries) or metadata.get("mode") != mode):
                    raise ValueError("compact pack receipt differs from actual bytes or complete entries")
                path = output / "archives" / (digest + ".onlrsp")
                if not path.exists() or sha(path.read_bytes()) != digest:
                    atomic_bytes(path, raw)
                row["archives"].append({**metadata, "path": path.relative_to(output).as_posix()})
            row["status"] = "success"
        except (OSError, subprocess.SubprocessError, ValueError, KeyError, IndexError, TypeError) as error:
            row["reason"] = error_text(error)
        manifest["contexts"].append(row)
        write(output / "manifest.json", manifest)
    manifest["complete"] = all(row["status"] == "success" for row in manifest["contexts"])
    write(output / "manifest.json", manifest)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    plan = commands.add_parser("plan")
    plan.add_argument("benchmark", type=Path)
    plan.add_argument("--generator", type=Path, required=True)
    plan.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[2])
    plan.add_argument("--output", type=Path, required=True)
    plan.add_argument("--combinations", type=Path)
    plan.add_argument("--cases", default="all")
    plan.add_argument("--corpus-mode", choices=("real-benchmark", "declared-manifest"), default="real-benchmark")
    plan.add_argument("--mc-runs", type=int, default=0)
    run = commands.add_parser("run")
    run.add_argument("plan", type=Path)
    run.add_argument("--generator", type=Path, required=True)
    run.add_argument("--store", type=Path, required=True)
    run.add_argument("--batch-size", type=int, default=128)
    run.add_argument("--timeout-seconds", type=float)
    run.add_argument("--retry-failures", action="store_true")
    compact = commands.add_parser("compact")
    compact.add_argument("plan", type=Path)
    compact.add_argument("--generator", type=Path, required=True)
    compact.add_argument("--store", type=Path, required=True)
    compact.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "plan":
        if args.mc_runs < 0:
            parser.error("mc-runs must be nonnegative")
        result = build_plan(args.benchmark, args.generator, args.source, args.output, args.combinations,
                            args.cases, args.mc_runs, args.corpus_mode)
        print(json.dumps(result["coverage"], sort_keys=True))
    elif args.command == "run":
        result = run_plan(args.plan, args.generator, args.store, args.batch_size, args.timeout_seconds, args.retry_failures)
        print(json.dumps({"complete": result["complete"], **result["stats"]}, sort_keys=True))
        if not result["complete"]:
            return 1
    else:
        result = compact_plan(args.plan, args.generator, args.store, args.output)
        print(json.dumps({"complete": result["complete"], "contexts": len(result["contexts"]),
                          "missingTasks": len(result["missingTasks"])}, sort_keys=True))
        if not result["complete"]:
            return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"luck response pipeline: {error}", file=sys.stderr)
        sys.exit(2)
