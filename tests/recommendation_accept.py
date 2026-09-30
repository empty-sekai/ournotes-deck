"""Run a frozen binary across the explicit scene denominator and bounded mock sizes.

No OCR or native-code substitute. All fixture/master/law assumptions remain in reports.
CPU: one child search at a time, stdlib only; records actual process RSS and wall time.
"""
import argparse
import copy
import ctypes
import hashlib
import json
import math
import pathlib
import statistics
import subprocess
import time


def read(path):
    return json.loads(pathlib.Path(path).read_text(encoding="utf-8-sig"))


def write(path, value):
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False), encoding="utf-8")


def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


class MemoryCounters(ctypes.Structure):
    _fields_ = [("cb", ctypes.c_ulong), ("PageFaultCount", ctypes.c_ulong)] + [
        (name, ctypes.c_size_t) for name in (
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage",
            "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage", "PrivateUsage")]


def peak_memory(child):
    # Windows kernel retains PeakWorkingSetSize through process exit while handle is open.
    counter = MemoryCounters()
    counter.cb = ctypes.sizeof(counter)
    psapi = ctypes.WinDLL("psapi")
    get_info = psapi.GetProcessMemoryInfo
    get_info.argtypes = [ctypes.c_void_p, ctypes.POINTER(MemoryCounters), ctypes.c_ulong]
    get_info.restype = ctypes.c_int
    if not get_info(int(child._handle), ctypes.byref(counter), counter.cb):
        raise ctypes.WinError()
    return counter.PeakWorkingSetSize


def invoke(binary, data, roster, request, out):
    command = [str(binary), "--data", str(data), "--roster", str(roster), "--request", str(request)]
    started = time.perf_counter()
    child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, stderr = child.communicate(timeout=120)
    elapsed_ms = (time.perf_counter() - started) * 1000
    rss = peak_memory(child)
    pathlib.Path(str(out) + ".stdout.json").write_bytes(stdout)
    pathlib.Path(str(out) + ".stderr.txt").write_bytes(stderr)
    result = json.loads(stdout) if child.returncode == 0 else json.loads(stderr)
    return {"returncode": child.returncode, "wallMs": elapsed_ms, "peakWorkingSetBytes": rss,
            "command": command, "result": result}


def request(fixtures, mode, music_id, execution, metric, gekisou=False):
    multiplayer = mode in ("battle", "arena")
    context = read(fixtures / ("context-multiplayer.json" if multiplayer else
                              "context-skip.json" if execution == "skip" else "context-played.json"))
    q = {"format": "ournotes-deck.recommendation-request/1", "scenario": {"kind": mode, "musicId": music_id},
         "execution": {"kind": execution}, "context": context, "metric": metric,
         "constraints": {"leader": 1, "noSnaps": True}, "k": 5, "strategy": {"kind": "exhaustive"},
         "limits": {"timeLimitMs": None, "maxCandidates": None, "cacheEntries": 32}}
    if execution != "power":
        q["execution"]["scoreId"] = 1004
    if execution == "live":
        q["execution"].update(gekisou=gekisou, play={"kind": "theoreticalBest"})
        q["seedLaw"] = {"atoms": read(fixtures / "finite-law.json"),
                        "provenance": "Synthetic illustrative signed/duplicate finite law; native TickCount population unknown"}
        if multiplayer and gekisou:
            # Synthetic table has only rank1 entries: missing rank2 factor resolves to0.
            q["networkConfirmations"] = [{"frame": 185, "range": r, "rank": 2, "percent": 0} for r in range(3)]
    return q


def make_matrix(binary, fixtures, out):
    metrics = [{"kind": "score"}, {"kind": "scoreAtLeast", "threshold": 100000},
               {"kind": "clientEventPoints", "eventId": 7},
               {"kind": "conditionalClientEventItems", "eventId": 7, "resourceType": 11, "resourceId": 9}]
    rows = []
    base_count = 0
    for mode, music_id in [("free", 10), ("mission", 10), ("battle", 10), ("arena", 80),
                           ("challenge", 70), ("challenge", 71)]:
        combos = [("power", False, {"kind": "power"})]
        if mode in ("free", "challenge"):
            combos += [("skip", False, metric) for metric in metrics]
        combos += [("live", gk, metric) for gk in [False, True] for metric in metrics
                   if gk or mode not in ("mission", "battle", "arena")]
        for execution, gk, metric in combos:
            name = f"{mode}-{music_id}-{execution}-{'gk' if gk else 'off'}-{metric['kind']}"
            q = request(fixtures, mode, music_id, execution, metric, gk)
            qp = out / "requests" / f"{name}.json"
            write(qp, q)
            run = invoke(binary, fixtures / "DeckData.json", fixtures / "roster.json", qp, out / name)
            result = run["result"]
            assert run["returncode"] == 0, (name, result)
            assert result["completion"] == "Complete" and result["optimality"] == "proven", (name, result)
            assert result["results"], name
            if execution == "live":
                assert result["stats"]["visitedCandidates"] == 24
            if mode in ("battle", "arena") and execution == "live":
                assert all(atom["networkApplications"] == [[0, 185], [1, 186], [2, 187]]
                           for deck in result["results"] for atom in deck["atoms"]), name
            rows.append({"id": name, "mode": mode, "musicId": music_id, "execution": execution,
                         "gekisou": gk if execution == "live" else None, "metric": metric,
                         "rosterConstraints": q["constraints"],
                         "probabilityAssumption": "explicit finite native roots" if execution == "live" else "deterministic",
                         "modelStatus": "conditional-exact" if execution == "live" or metric["kind"].startswith("conditional")
                                        else "exact-deterministic",
                         "completion": result["completion"], "optimality": result["optimality"],
                         "nativeScope": "1.0.1-25 verified mode/runtime mechanisms; synthetic master/chart/peer inputs",
                         "request": str(qp.relative_to(out)), "output": name + ".stdout.json",
                         "evaluated": result["stats"]["evaluated"], "wallMs": run["wallMs"]})
            if music_id != 71:
                base_count += 1
    assert (base_count, len(rows)) == (41, 54)
    guards = []
    for mode, ident in [("mission", 10), ("battle", 10), ("arena", 80)]:
        guards += [(f"invalid-{mode}-ordinary", request(fixtures, mode, ident, "live", {"kind": "score"}, False), "Game"),
                   (f"unsupported-{mode}-skip", request(fixtures, mode, ident, "skip", {"kind": "score"}), "Input")]
    unknown = request(fixtures, "free", 10, "live", {"kind": "score"}, False)
    unknown["constraints"]["bogusConstraint"] = True
    guards.append(("unknown-nested-constraint", unknown, "Input"))
    missing = request(fixtures, "battle", 10, "live", {"kind": "score"}, True)
    del missing["networkConfirmations"]
    guards.append(("missing-network-model", missing, "Unsupported"))
    guard_rows = []
    for name, q, code in guards:
        qp = out / "requests" / f"{name}.json"
        write(qp, q)
        run = invoke(binary, fixtures / "DeckData.json", fixtures / "roster.json", qp, out / name)
        assert run["returncode"] == 2 and run["result"]["error"]["code"] == code, (name, run)
        guard_rows.append({"id": name, "expectedCode": code, "passed": True, "error": run["result"]})
    write(out / "scene-matrix.json", {"format": "ournotes-deck.scene-matrix/1", "binarySha256": sha(binary),
          "denominator": {"legalModeExecutionMetricBranches": 41, "challengeLiteralZeroExtra": 13,
                          "syntheticExecutableBranches": len(rows)},
          "branchInterpretation": "5 power +2 skip contexts x4 metrics +7 played contexts x4 metrics; Challenge zero adds13",
          "nativeLegalModes": ["free", "mission", "battle", "arena", "challenge"],
          "action": "select five distinct-character cards, physical slot2 leader, inject unique paired snaps",
          "rows": rows, "invalidOrUnsupportedGuards": guard_rows,
          "scope": "Finite law/model coverage; not all native cards, player states, raw touches or regions"})
    print(f"matrix: {len(rows)} branches +{len(guard_rows)} guards passed", flush=True)


def percentile(samples, p):
    # Nearest-rank percentile; report sample count, not a distribution estimate.
    return sorted(samples)[max(0, math.ceil(len(samples) * p) - 1)]


def performance(binary, fixtures, out, repeats):
    large = fixtures / "large"
    short_data = large / "DeckData.json"
    long_data = copy.deepcopy(read(short_data))
    chart = long_data["charts"][0]
    chart["notes"] = {"id": list(range(1, 1201)), "op": [1] * 1200, "judgementType": [1] * 1200,
                      "timeMs": [i * 150 for i in range(1, 1201)]}
    chart["skillEvents"]["timeMs"] = [1000, 40000, 75000, 110000, 145000]
    chart["fevers"] = {"startMs": [30000, 90000, 140000], "endMs": [55000, 115000, 165000]}
    chart["asset"]["key"] = "SYNTHETIC-180sec-1200notes-60fps-shape"
    long_data["provenance"]["performanceChart"] = "synthetic 180sec,1200notes,5skill-events,3fevers"
    score_table = long_data["master"]["MasterLiveMusicScore"]
    count_column = score_table["columns"].index("_fullComboCount")
    id_column = score_table["columns"].index("_id")
    for row in score_table["rows"]:
        if row[id_column] == 1004:
            row[count_column] = 1200
    long_path = out / "long-shape-DeckData.json"
    write(long_path, long_data)
    cases = [
        ("short-power", "power", False, {"kind": "power"}, "exhaustive", short_data),
        ("short-skip", "skip", False, {"kind": "score"}, "exhaustive", short_data),
        ("short-live-score", "live", False, {"kind": "score"}, "candidate", short_data),
        ("long-live-score", "live", False, {"kind": "score"}, "candidate", long_path),
        ("long-gk-score", "live", True, {"kind": "score"}, "candidate", long_path),
        ("long-gk-probability", "live", True, {"kind": "scoreAtLeast", "threshold": 1000000}, "candidate", long_path),
        ("long-gk-event", "live", True, {"kind": "clientEventPoints", "eventId": 7}, "candidate", long_path),
        ("long-live-exhaustive-cap", "live", False, {"kind": "score"}, "exhaustive", long_path),
    ]
    report_path = out / "performance-report.json"
    reports = read(report_path)["reports"] if report_path.exists() else []
    for name, execution, gk, metric, strategy, dp in cases:
        if any(existing["name"] == name for existing in reports):
            continue
        q = request(fixtures, "free", 10, execution, metric, gk)
        q["constraints"] = {}
        q["limits"] = {"timeLimitMs": 500, "maxCandidates": 100000, "cacheEntries": 128}
        if name.endswith("exhaustive-cap"):
            q["limits"]["maxCandidates"] = 3
        q["strategy"] = {"kind": "candidate", "powerSeeds": 4, "proposals": 1000000, "proposalSeed": 5001} if strategy == "candidate" else {"kind": "exhaustive"}
        qp = out / f"{name}-request.json"
        write(qp, q)
        samples = []
        for rep in range(repeats):
            run = invoke(binary, dp, large / "roster.json", qp, out / f"{name}-{rep:02}")
            assert run["returncode"] == 0, (name, run["result"])
            result = run["result"]
            if execution == "live":
                assert result["completion"] == "TimedOut" and result["optimality"] == ("heuristic" if strategy == "candidate" else "unproven")
            samples.append({"wallMs": run["wallMs"], "modelElapsedMs": result["elapsedMs"],
                            "peakWorkingSetBytes": run["peakWorkingSetBytes"], "completion": result["completion"],
                            "optimality": result["optimality"], "exitReason": result["exitReason"],
                            "counters": result["stats"], "returnedK": len(result["results"])})
        walls = [s["wallMs"] for s in samples]
        reports.append({"name": name, "repeats": repeats, "wallMsP50": statistics.median(walls),
                        "wallMsP95NearestRank": percentile(walls, .95),
                        "peakWorkingSetBytesMax": max(s["peakWorkingSetBytes"] for s in samples),
                        "exitReasons": sorted({s["exitReason"] for s in samples}), "samples": samples})
        write(out / "performance-report.json", {"format": "ournotes-deck.bounded-performance/1",
              "binarySha256": sha(binary), "pool": {"members": 80, "snaps": 60, "characters": 40},
              "workers": 1, "budgets": "500ms cooperative model deadline, includes no process startup; cap applies physical",
              "rssMethod": "Windows GetProcessMemoryInfo.PeakWorkingSetSize, queried on retained exited process handle",
              "charts": "synthetic short12notes and realistic-shape180sec/1200notes/60fps; no real chart/master timing claim",
              "seedLaw": read(fixtures / "finite-law.json"), "reports": reports})
        print(f"{name}: p50={statistics.median(walls):.1f}ms p95={percentile(walls,.95):.1f}ms rss={reports[-1]['peakWorkingSetBytesMax']/1048576:.1f}MiB", flush=True)


def portability_inputs(fixtures, out):
    cases = [("free", 10, False), ("free", 10, True), ("mission", 10, True),
             ("challenge", 70, False), ("challenge", 70, True), ("challenge", 71, True),
             ("battle", 10, True), ("arena", 80, True)]
    for mode, music_id, gk in cases:
        q = request(fixtures, mode, music_id, "live", {"kind": "score"}, gk)
        v = {"data": (fixtures / "DeckData.json").read_text(), "roster": (fixtures / "roster.json").read_text(),
             "scene": mode, "musicId": music_id, "scoreId": 1004, "gekisou": gk,
             "context": q["context"], "members": [2, 3, 1, 4, 5], "snaps": [1, None, None, None, 2],
             "roots": [0, 1, -1, 14, -2147483648, 2147483647, 1]}
        if "networkConfirmations" in q:
            v["networkConfirmations"] = q["networkConfirmations"]
        write(out / "inputs" / f"{mode}-{music_id}-{'gk' if gk else 'off'}.json", v)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["matrix", "performance", "portability-inputs"])
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument("--fixtures", type=pathlib.Path, required=True)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    parser.add_argument("--repeats", type=int, default=10)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    {"matrix": make_matrix, "performance": performance,
     "portability-inputs": lambda b, f, o: portability_inputs(f, o)}[args.action](
        args.binary, args.fixtures, args.out, *([args.repeats] if args.action == "performance" else []))
