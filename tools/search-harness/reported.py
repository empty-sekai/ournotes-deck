"""Export the immutable reported reproduction only after a native strict projection audit."""
import argparse
import copy
import os
from pathlib import Path
import shutil
import subprocess

from fetch_dataset import acquire
from matrix import digest, read_json, snapshot, write_json

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
FIXTURE = HERE / "fixtures/reported"
CASE = "issue9-battle-original"
HASHES = {"request": "38c2f8fb0d46ec60b5d2da9786a1efccd8b7d7107bd68a2b8785c24fd93dd3c3",
          "roster": "8aa45d2746a424bf12ccd2f060453007a5c593b3c7a0d6235e924fd59eef0309"}


def original_inputs(fixture=FIXTURE):
    provenance = read_json(fixture / "provenance.json")
    if provenance.get("case") != CASE or provenance.get("sha256") != HASHES:
        raise ValueError("reported provenance differs from the fixed original identities")
    for name, expected in HASHES.items():
        if digest(fixture / (name + ".json")) != expected:
            raise ValueError(f"original reported {name} bytes changed")
    return read_json(fixture / "roster.json"), provenance


def derive_snapshot(roster, data, dataset_id):
    """Schema conversion for this fixed original; no missing cultivation is guessed."""
    player = roster["player"]
    characters = data["master"]["MasterCharacter"]
    character_ids = {row[characters["columns"].index("_id")] for row in characters["rows"]}
    if set(map(int, player["characterRanks"])) != character_ids:
        raise ValueError("original character ranks do not cover the complete selected character table")
    if ([card["id"] for card in roster["members"]] != player["ownedMemberCardIds"]
            or [card["id"] for card in roster["snaps"]] != player["ownedSupportCardIds"]):
        raise ValueError("original owned resource lists differ from eligible lists")
    def values(mapping):
        return [{"id": int(key), "value": value} for key, value in mapping.items()]
    memory = player["memory"]
    profile = {"format": "ournotes-deck.harness-profile/1",
               "members": copy.deepcopy(roster["members"]), "snaps": copy.deepcopy(roster["snaps"]),
               "player": {"characterRanks": {"coverage": "complete", "values": values(player["characterRanks"])},
                          "characterTotalRank": None, "vipRank": player["vipRank"],
                          "bandItems": values(player["bandItems"]), "eventIds": copy.deepcopy(player["events"]),
                          "memory": {"musicRanks": values(memory["musicRanks"]),
                                     "unlockedMembers": copy.deepcopy(memory["unlockedMembers"]),
                                     "unlockedSnaps": copy.deepcopy(memory["unlockedSupports"])}}}
    value = snapshot(profile, dataset_id, "reported/" + CASE + "/" + HASHES["roster"])
    # Explicit legacy IDs define resources, not a verified complete account inventory.
    value["ownedFacts"]["memberCoverage"] = "partial"
    value["ownedFacts"]["snapCoverage"] = "partial"
    return value


def prepare(args):
    roster, provenance = original_inputs()
    pin = read_json(FIXTURE / provenance["selectedDatasetSource"])
    data_path, acquired = acquire(pin, args.cache_dir, args.data, args.offline)
    value = derive_snapshot(roster, read_json(data_path), digest(data_path))
    binary = Path(args.audit_binary).resolve() if args.audit_binary else (
        HERE / "target/release" / ("reported_projection.exe" if os.name == "nt" else "reported_projection"))
    if not binary.is_file() and not args.no_build:
        subprocess.run(["cargo", "build", "--locked", "--release", "--manifest-path", str(HERE / "Cargo.toml"),
                        "--bin", "reported_projection"], cwd=REPO, check=True)
    if not binary.is_file():
        raise ValueError(f"missing native projection audit: {binary}")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    for name in ("request.json", "roster.json", "provenance.json"):
        shutil.copyfile(FIXTURE / name, out / name)
    write_json(out / "snapshot.json", value)
    # No benchmark manifest is published unless the production resolver and full-field audit pass.
    subprocess.run([str(binary), str(data_path), str(out / "roster.json"), str(out / "snapshot.json"),
                    str(out / "request.json"), str(out / "projection-audit.json")], check=True)
    audit = read_json(out / "projection-audit.json")
    if (audit.get("format") != "ournotes-deck.reported-projection/1" or audit.get("datasetId") != digest(data_path)
            or any(audit.get(field) is not True for field in
                   ("strictResolution", "parsedRosterEqual", "poolFieldsEqual", "candidateDomainEqual"))
            or audit.get("memberIds") != roster["player"]["ownedMemberCardIds"]
            or audit.get("snapIds") != roster["player"]["ownedSupportCardIds"]
            or audit.get("leaderIds") != roster["player"]["ownedMemberCardIds"]):
        raise ValueError("native projection audit did not certify the original complete input domain")
    inputs = {name: digest(out / (name + ".json")) for name in ("request", "roster", "snapshot")}
    if any(inputs[name] != expected for name, expected in HASHES.items()):
        raise ValueError("original bytes changed during projection audit")
    manifest = {"format": "ournotes-deck.search-benchmark/1", "matrixId": "reported",
                "requestTimeLimitMs": 60000, "timeoutMs": 90000, "datasetId": digest(data_path),
                "cases": [{"name": CASE, "family": "reported", "data": str(data_path),
                           "snapshot": "snapshot.json", "roster": "roster.json", "request": "request.json"}]}
    write_json(out / "preparation-receipt.json", {
        "format": "ournotes-deck.reported-preparation/1", "source": acquired,
        "sourceProvenanceSha256": digest(out / "provenance.json"), "inputSha256": inputs,
        "auditBinarySha256": digest(binary), "auditSha256": digest(out / "projection-audit.json"),
        "nativeProjectionPassed": True,
    })
    write_json(out / "benchmark.json", manifest)
    return out / "benchmark.json"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["prepare"])
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cache-dir", type=Path, default=REPO / "work/datasets")
    parser.add_argument("--data", type=Path)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--audit-binary")
    print(prepare(parser.parse_args()))


if __name__ == "__main__":
    main()
