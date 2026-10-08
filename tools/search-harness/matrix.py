"""Prepare or run a declared chart and ownership matrix from one pinned dataset."""
import argparse
import copy
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

from fetch_dataset import acquire
from mock_rosters import Domains


HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
MATRIX_FORMAT = "ournotes-deck.harness-matrix/1"
PROFILE_FORMAT = "ournotes-deck.harness-profile/1"


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def rows(data, name):
    table = data["master"][name]
    return [dict(zip(table["columns"], row, strict=True)) for row in table["rows"]]


def named(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", value):
        raise ValueError(f"invalid matrix name: {value!r}")
    return value


def load_suite(suite):
    path = Path(suite)
    if not path.is_file():
        path = HERE / "fixtures" / named(suite) / "matrix.json"
    path = path.resolve()
    matrix = read_json(path)
    if matrix.get("format") != MATRIX_FORMAT:
        raise ValueError(f"matrix: expected {MATRIX_FORMAT}")
    named(matrix["name"])
    names = [named(case["name"]) for case in matrix["cases"]]
    if not names or len(names) != len(set(names)):
        raise ValueError("matrix case names must be unique and nonempty")
    for case in matrix["cases"]:
        for field, catalog in (("chart", "charts"), ("profile", "profiles"), ("play", "plays")):
            named(case[field])
            if case[field] not in matrix[catalog]:
                raise ValueError(f"{case['name']}: unknown {field} {case[field]}")
        if "context" in case and case["context"] not in matrix.get("contexts", {}):
            raise ValueError(f"{case['name']}: unknown context")
        chart, request = matrix["charts"][case["chart"]], case["request"]
        if request.get("format") != "ournotes-deck.search-request/1":
            raise ValueError(f"{case['name']}: unsupported request format")
        if request["execution"].get("kind") != "live" or request["execution"].get("scoreId") != chart["scoreId"]:
            raise ValueError(f"{case['name']}: execution does not match its chart")
        if request["scenario"] != {"kind": chart["scene"], "musicId": chart["musicId"]}:
            raise ValueError(f"{case['name']}: scenario does not match its chart")
    return path, matrix


def select_cases(matrix, selection):
    cases = matrix["cases"]
    if selection == "score":
        cases = [case for case in cases if case["group"] == "score"]
    elif selection != "all":
        selected = selection.split(",")
        known = {case["name"] for case in cases}
        if len(selected) != len(set(selected)) or any(name not in known for name in selected):
            raise ValueError("--cases contains duplicate or unknown case names")
        cases = [case for case in cases if case["name"] in selected]
    if not cases:
        raise ValueError("no matrix cases selected")
    # Keep the declared order within each group; the score requests run first.
    return sorted(cases, key=lambda case: case["group"] != "score")


def snapshot(profile, dataset_id, revision):
    if profile.get("format") != PROFILE_FORMAT:
        raise ValueError(f"profile: expected {PROFILE_FORMAT}")
    member_ids = [member["id"] for member in profile["members"]]
    snap_ids = [snap["id"] for snap in profile["snaps"]]
    if len(member_ids) != len(set(member_ids)) or len(snap_ids) != len(set(snap_ids)):
        raise ValueError("profile contains duplicate owned cards")
    return {
        "format": "ournotes.owned-snapshot/1", "datasetId": dataset_id, "revision": revision,
        "ownedFacts": {"memberIds": member_ids, "snapIds": snap_ids,
                       "memberCoverage": "complete", "snapCoverage": "complete"},
        "eligible": {"members": copy.deepcopy(profile["members"]), "snaps": copy.deepcopy(profile["snaps"])},
        "player": copy.deepcopy(profile["player"]), "assumptions": [],
    }


def legacy_roster(value):
    facts = value["player"]
    ranks = facts["characterRanks"]
    if ranks["coverage"] != "complete":
        raise ValueError("matrix profiles require complete character rank facts")
    def mapping(values):
        result = {str(item["id"]): item["value"] for item in values}
        if len(result) != len(values):
            raise ValueError("profile contains duplicate player fact IDs")
        return result
    memory = facts.get("memory")
    player = {
        "characterRanks": mapping(ranks["values"]), "bandItems": mapping(facts["bandItems"]),
        "vipRank": facts["vipRank"], "events": copy.deepcopy(facts["eventIds"]), "memory": None,
        "ownedMemberCardIds": value["ownedFacts"]["memberIds"], "ownedSupportCardIds": value["ownedFacts"]["snapIds"],
    }
    if facts.get("characterTotalRank") is not None:
        player["explicitCharacterTotalRank"] = facts["characterTotalRank"]
    if memory and any(memory.get(key) for key in ("musicRanks", "unlockedMembers", "unlockedSnaps")):
        player["memory"] = {"musicRanks": mapping(memory["musicRanks"]),
                            "unlockedMembers": memory["unlockedMembers"], "unlockedSupports": memory["unlockedSnaps"]}
    return {"player": player, **copy.deepcopy(value["eligible"])}


def canonical_ticks(text):
    """Canonical Gregorian wall time, without timezone conversion, in 100 ns ticks."""
    if text in (None, "", "null"):
        return 0
    match = re.fullmatch(
        r"([0-9]{4})([-/])([0-9]{2})\2([0-9]{2})(?:[T ]([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.([0-9]{1,7}))?)?", text,
    )
    if match is None:
        raise ValueError("event start is outside canonical no-offset date format")
    year, _, month, day, hour, minute, second, fraction = match.groups()
    value = datetime(int(year), int(month), int(day), int(hour or 0), int(minute or 0), int(second or 0))
    seconds = (value.toordinal() - 1) * 86400 + value.hour * 3600 + value.minute * 60 + value.second
    return seconds * 10_000_000 + int((fraction or "").ljust(7, "0"))


def materialize_context(template, data):
    context = copy.deepcopy(template)
    clock = context["resultClock"]
    if clock.get("kind") != "eventStartOffset" or type(clock.get("seconds")) is not int:
        raise ValueError("unsupported matrix result-clock recipe")
    events = [event for event in rows(data, "MasterEvent") if event["_id"] == clock["eventId"]]
    if len(events) != 1:
        raise ValueError(f"expected one master event {clock['eventId']}")
    ticks = canonical_ticks(events[0].get("_startAt")) + clock["seconds"] * 10_000_000
    if not 0 <= ticks <= 3155378975999999999:
        raise ValueError("result clock is outside the DateTime tick domain")
    context["resultClock"] = {"execution": clock["execution"], "savedStartJstTicks": ticks, "serverNowJstTicks": ticks}
    return context


def validate_chart(data, chart):
    music_id = chart["musicId"]
    if chart["scene"] in ("arena", "challenge"):
        table = "MasterArenaMusic" if chart["scene"] == "arena" else "MasterChallengeMusic"
        selected = [row for row in rows(data, table) if row["_id"] == music_id]
        if len(selected) != 1:
            raise ValueError(f"unknown {chart['scene']} music {music_id}")
        music_id = selected[0]["_liveMusicId"]
    music = [row for row in rows(data, "MasterLiveMusic") if row["_id"] == music_id]
    if len(music) != 1 or chart["scoreId"] not in [music[0].get(field) for field in ("_easyID", "_normalID", "_hardID", "_expertID")]:
        raise ValueError(f"chart {chart['scoreId']} does not belong to music {music_id}")
    if sum(item["scoreId"] == chart["scoreId"] for item in data["charts"]) != 1:
        raise ValueError(f"chart {chart['scoreId']} is missing or duplicated")


def binaries(args, need_run):
    target = Path(os.environ.get("CARGO_TARGET_DIR", str(HERE / "target")))
    if not target.is_absolute():
        target = REPO / target
    suffix = ".exe" if os.name == "nt" else ""
    prepare = Path(args.prepare_binary).resolve() if args.prepare_binary else target / "release" / ("benchmark_prepare" + suffix)
    candidate = Path(args.binary).resolve() if args.binary else target / "release" / ("profile_case" + suffix)
    required = [prepare, candidate] if need_run else [prepare]
    missing = [path for path in required if not path.is_file()]
    if missing and args.no_build:
        raise ValueError("missing binary with --no-build: " + ", ".join(map(str, missing)))
    if missing:
        subprocess.run(["cargo", "build", "--locked", "--release", "--manifest-path", str(HERE / "Cargo.toml"),
                        "--bin", "benchmark_prepare", "--bin", "profile_case"], cwd=REPO, check=True)
    if any(not path.is_file() for path in required):
        raise ValueError("requested binary not found after build: " + ", ".join(str(path) for path in required if not path.is_file()))
    return prepare, candidate


def prepare_matrix(args, suite_path, matrix, cases, out, prepare_binary):
    source = read_json(suite_path.parent / matrix["source"])
    data_path, receipt = acquire(source, args.cache_dir, args.data, args.offline)
    data = read_json(data_path)
    dataset_id = digest(data_path)
    domains = Domains(data["master"])
    profiles = {}
    for name in dict.fromkeys(case["profile"] for case in cases):
        profile_path = suite_path.parent / matrix["profiles"][name]
        value = snapshot(read_json(profile_path), dataset_id, matrix["name"] + "/" + name)
        roster = legacy_roster(value)
        errors = domains.validate(roster)
        if errors:
            raise ValueError(f"{name}: " + "; ".join(errors))
        profiles[name] = {"snapshot": value, "roster": roster, "sha256": digest(profile_path)}
    specs = {}
    for case in cases:
        chart = matrix["charts"][case["chart"]]
        validate_chart(data, chart)
        key = case["chart"] + "--" + case["play"]
        play = matrix["plays"][case["play"]]
        if play["kind"] not in ("theoreticalBest", "nativeTheoreticalWithMisses"):
            raise ValueError(f"unknown play recipe {play['kind']}")
        streamed = play["kind"] == "nativeTheoreticalWithMisses"
        specs[key] = {"key": key, **chart, "missEvery": play["missEvery"] if streamed else 0,
                      "seed": play.get("seed", 0), "includeStream": streamed}
    time_limits = {case["request"]["limits"]["timeLimitMs"] for case in cases}
    if len(time_limits) != 1:
        raise ValueError("the benchmark transport requires one declared request time limit")
    out.mkdir(parents=True, exist_ok=False)
    write_json(out / "preparation-specs.json", list(specs.values()))
    subprocess.run([str(prepare_binary), str(data_path), str(out / "preparation-specs.json"),
                    str(out / "preparation.json")], check=True)
    prepared = read_json(out / "preparation.json")
    if prepared.get("format") != "ournotes.benchmark-preparation/1" or prepared.get("datasetId") != dataset_id:
        raise ValueError("native preparation returned a different dataset identity or format")
    entries = {entry["key"]: entry for entry in prepared["entries"]}
    if len(entries) != len(prepared["entries"]) or set(entries) != set(specs):
        raise ValueError("native preparation returned different chart/play entries")
    for name, profile in profiles.items():
        write_json(out / "snapshots" / (name + ".json"), profile["snapshot"])
        write_json(out / "rosters" / (name + ".json"), profile["roster"])
    result_cases = []
    for case in cases:
        request = copy.deepcopy(case["request"])
        entry = entries[case["chart"] + "--" + case["play"]]
        play = matrix["plays"][case["play"]]
        if play["kind"] == "theoreticalBest":
            request["execution"]["play"] = copy.deepcopy(play)
        else:
            if entry.get("stream") is None:
                raise ValueError(f"{case['name']}: native preparation omitted the requested stream")
            request["execution"]["play"] = {"kind": "stream", "stream": entry["stream"]}
        if "context" in case:
            request["context"] = materialize_context(matrix["contexts"][case["context"]], data)
        request_file = "requests/" + case["name"] + ".json"
        write_json(out / request_file, request)
        result_cases.append({"name": case["name"], "family": case["group"], "chart": case["chart"],
                             "profile": case["profile"], "metric": request["metric"]["kind"],
                             "data": str(data_path), "snapshot": "snapshots/" + case["profile"] + ".json",
                             "roster": "rosters/" + case["profile"] + ".json", "request": request_file,
                             "scoreId": entry["scoreId"], "missCount": entry["missCount"]})
    manifest = {"format": "ournotes-deck.search-benchmark/1", "matrixId": matrix["name"],
                "requestTimeLimitMs": next(iter(time_limits)), "timeoutMs": matrix["watchdogMs"],
                "datasetId": dataset_id, "cases": result_cases}
    write_json(out / "benchmark.json", manifest)
    write_json(out / "preparation-receipt.json", {
        "format": "ournotes-deck.matrix-preparation/1", "suiteSha256": digest(suite_path),
        "profiles": {name: profile["sha256"] for name, profile in profiles.items()},
        "source": receipt, "prepareBinarySha256": digest(prepare_binary), "manifestSha256": digest(out / "benchmark.json"),
    })
    return out / "benchmark.json"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("prepare", "run"):
        command = commands.add_parser(name)
        command.add_argument("--suite", default="full48", help="fixture name or matrix JSON path")
        command.add_argument("--data", type=Path, help="local dataset matching the suite's pin")
        command.add_argument("--offline", action="store_true", help="acquire data only from verified local files")
        command.add_argument("--cache-dir", type=Path, default=REPO / "work" / "datasets")
        command.add_argument("--out", type=Path, help="new output directory; existing directories are rejected")
        command.add_argument("--cases", default="all", help="all, score, or comma-separated case names")
        command.add_argument("--prepare-binary", help="benchmark_prepare executable")
        command.add_argument("--binary", help="profile_case executable")
        command.add_argument("--no-build", action="store_true", help="fail when a required executable is absent")
        if name == "run":
            command.add_argument("--require-complete", action="store_true", help="require proven completion within every request's budget")
    args = parser.parse_args()
    try:
        suite_path, matrix = load_suite(args.suite)
        cases = select_cases(matrix, args.cases)
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
        out = (args.out or REPO / "work" / (matrix["name"] + "-" + stamp)).resolve()
        if out.exists():
            raise ValueError(f"output already exists: {out}; choose a new --out directory")
        prepare_binary, candidate = binaries(args, args.command == "run")
        manifest = prepare_matrix(args, suite_path, matrix, cases, out, prepare_binary)
        print(json.dumps({"manifest": str(manifest), "cases": len(cases)}, ensure_ascii=False), flush=True)
        if args.command == "run":
            results = out / "results"
            subprocess.run(["node", str(HERE / "benchmark.cjs"), str(manifest), str(candidate), str(results),
                            "--candidate-only", "--candidate-source", str(REPO)], cwd=REPO, check=True)
            report = read_json(results / "report.json")
            if args.require_complete and not report["summary"]["candidate"]["allProvenWithinBudget"]:
                raise ValueError("matrix contains requests without proven completion within their declared budget; see results/report.json")
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
