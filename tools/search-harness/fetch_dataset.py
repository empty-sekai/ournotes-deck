"""Acquire a pinned public deck dataset, preserving its exact decoded JSON bytes."""
import argparse
import gzip
import hashlib
import io
import json
import os
import re
import tempfile
import zlib
from pathlib import Path
from urllib.parse import urljoin, urlsplit
from urllib.request import Request, urlopen


SOURCE_FORMAT = "ournotes-deck.dataset-source/1"
MANIFEST_FORMAT = "nnnotes.replay-manifest/1"
DATA_FORMAT = "nnnotes.deck-data/1"
MANIFEST_LIMIT = 2 * 1024 * 1024
DEFAULT_SOURCE = Path(__file__).parent / "fixtures" / "full48" / "source.json"


def _sha(raw):
    return hashlib.sha256(raw).hexdigest()


def _url(value, label):
    if not isinstance(value, str):
        raise ValueError(f"{label}: expected an absolute HTTP(S) URL")
    parsed = urlsplit(value)
    if parsed.scheme not in ("https", "http") or not parsed.netloc or parsed.username or parsed.password:
        raise ValueError(f"{label}: expected an absolute HTTP(S) URL without credentials")
    return value


def _entry(value, label, size=False):
    if not isinstance(value, dict):
        raise ValueError(f"{label}: expected a file descriptor")
    _url(value.get("url"), label)
    if not isinstance(value.get("sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", value["sha256"]):
        raise ValueError(f"{label}: expected a lowercase SHA-256")
    if size and (type(value.get("bytes")) is not int or value["bytes"] <= 0):
        raise ValueError(f"{label}: expected a positive decoded byte count")


def _validate_source(source):
    if not isinstance(source, dict) or source.get("format") != SOURCE_FORMAT:
        raise ValueError(f"source: expected {SOURCE_FORMAT}")
    for field in ("name", "region"):
        if not isinstance(source.get(field), str) or not source[field]:
            raise ValueError(f"source: missing {field}")
    _url(source.get("buildUrl"), "buildUrl")
    _entry(source.get("replayManifest"), "replayManifest")
    _entry(source.get("deckData"), "deckData", size=True)
    if source["deckData"].get("format") != DATA_FORMAT:
        raise ValueError(f"deckData: expected {DATA_FORMAT}")


def _json(raw, label):
    try:
        return json.loads(raw.decode("utf-8-sig"))
    except (UnicodeDecodeError, ValueError) as error:
        raise ValueError(f"{label}: invalid UTF-8 JSON") from error


def _verify(raw, entry, label):
    if "bytes" in entry and len(raw) != entry["bytes"]:
        raise ValueError(f"{label}: decoded size {len(raw)} differs from pinned size {entry['bytes']}")
    actual = _sha(raw)
    if actual != entry["sha256"]:
        raise ValueError(f"{label}: SHA-256 {actual} differs from pinned SHA-256 {entry['sha256']}")


def _dataset(raw, entry):
    _verify(raw, entry, "deck data")
    value = _json(raw, "deck data")
    if not isinstance(value, dict) or value.get("format") != DATA_FORMAT:
        raise ValueError(f"deck data: expected {DATA_FORMAT}")


def _download(url, limit):
    request = Request(url, headers={"Accept": "application/json", "Accept-Encoding": "gzip"})
    with urlopen(request, timeout=30) as response:
        _url(response.geturl(), "response URL")
        encoding = response.headers.get("Content-Encoding", "identity").strip().lower()
        if encoding not in ("", "identity", "gzip", "x-gzip"):
            raise ValueError(f"{url}: unsupported Content-Encoding {encoding}")
        # The wire representation may be slightly larger than its decoded JSON.
        wire_limit = limit + 1024 * 1024
        raw = response.read(wire_limit + 1)
    if len(raw) > wire_limit:
        raise ValueError(f"{url}: encoded response exceeds the download limit")
    # urllib leaves Content-Encoding intact. A .gz file and a gzip HTTP response
    # describe the same compressed bytes here; never decompress twice for both flags.
    compressed = encoding in ("gzip", "x-gzip") or raw.startswith(b"\x1f\x8b")
    if compressed:
        try:
            with gzip.GzipFile(fileobj=io.BytesIO(raw)) as stream:
                raw = stream.read(limit + 1)
        except (OSError, EOFError, zlib.error) as error:
            raise ValueError(f"{url}: invalid gzip response") from error
    if len(raw) > limit:
        raise ValueError(f"{url}: decoded response exceeds the download limit")
    return raw


def _atomic_write(path, raw):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=path.name + ".", suffix=".tmp", delete=False) as file:
            temporary = Path(file.name)
            file.write(raw)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _receipt(source, method):
    return {
        "source": source["name"], "region": source["region"], "acquiredVia": method,
        "datasetId": source["deckData"]["sha256"], "decodedBytes": source["deckData"]["bytes"],
        "replayManifest": dict(source["replayManifest"]), "deckData": dict(source["deckData"]),
    }


def acquire(source: dict, cache_dir: Path, data: Path | None = None, offline: bool = False) -> tuple[Path, dict]:
    """Return verified decoded data and its receipt; never change the pinned source.

    An explicit local file must match the pin. Offline operation accepts only that
    file or the matching verified cache entry. buildUrl is discovery metadata and
    is not consulted while acquiring a frozen dataset.
    """
    _validate_source(source)
    entry = source["deckData"]
    if data is not None:
        path = Path(data).resolve()
        _dataset(path.read_bytes(), entry)
        return path, _receipt(source, "file")
    cache = Path(cache_dir).resolve()
    path = cache / "data" / (entry["sha256"] + ".json")
    if path.is_file():
        try:
            _dataset(path.read_bytes(), entry)
        except ValueError as error:
            if offline:
                raise ValueError(f"offline cache is invalid: {error}") from error
        else:
            return path, _receipt(source, "cache")
    if offline:
        raise ValueError(f"offline cache has no verified dataset {entry['sha256']}; provide --data or download it first")
    manifest_entry = source["replayManifest"]
    manifest_raw = _download(manifest_entry["url"], MANIFEST_LIMIT)
    _verify(manifest_raw, manifest_entry, "replay manifest")
    manifest = _json(manifest_raw, "replay manifest")
    if not isinstance(manifest, dict) or manifest.get("format") != MANIFEST_FORMAT:
        raise ValueError(f"replay manifest: expected {MANIFEST_FORMAT}")
    published = manifest.get("deckData")
    if not isinstance(published, dict) or not isinstance(published.get("url"), str):
        raise ValueError("replay manifest: missing deckData descriptor")
    actual = {key: published.get(key) for key in ("format", "sha256", "bytes")}
    actual["url"] = urljoin(manifest_entry["url"], published["url"])
    if actual != {key: entry[key] for key in ("format", "sha256", "bytes", "url")}:
        raise ValueError("replay manifest: deckData differs from the frozen source descriptor")
    raw = _download(entry["url"], entry["bytes"])
    _dataset(raw, entry)
    _atomic_write(cache / "manifests" / (manifest_entry["sha256"] + ".json"), manifest_raw)
    _atomic_write(path, raw)
    return path, _receipt(source, "download")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE, help="pinned source descriptor JSON")
    parser.add_argument("--cache-dir", type=Path, default=Path(__file__).resolve().parents[2] / "work" / "datasets")
    parser.add_argument("--data", type=Path, help="offline decoded deck-data JSON, which must match the source pin")
    parser.add_argument("--offline", action="store_true", help="use only a verified local file or cache entry")
    args = parser.parse_args()
    try:
        source = _json(args.source.read_bytes(), "source")
        path, receipt = acquire(source, args.cache_dir, args.data, args.offline)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps({"data": str(path), **receipt}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
