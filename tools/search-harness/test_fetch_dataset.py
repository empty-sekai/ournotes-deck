"""Dataset identity, transport and offline acquisition contracts."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from fetch_dataset import acquire


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


class Response(io.BytesIO):
    def __init__(self, url, raw, encoding="identity"):
        super().__init__(raw)
        self.url = url
        self.headers = {"Content-Encoding": encoding}

    def geturl(self):
        return self.url


class DatasetAcquisition(unittest.TestCase):
    def fixture(self, suffix=".json"):
        # Preserve a BOM and whitespace in the dataset's identity.
        raw = b'\xef\xbb\xbf{ "format": "nnnotes.deck-data/1", "master": {}, "charts": [] }\n'
        base = "https://example.test/replay/"
        data = {"format": "nnnotes.deck-data/1", "url": base + "data" + suffix, "sha256": sha(raw), "bytes": len(raw)}
        manifest = json.dumps({"format": "nnnotes.replay-manifest/1", "deckData": {**data, "url": "data" + suffix}}).encode()
        source = {
            "format": "ournotes-deck.dataset-source/1", "name": "fixture", "region": "tw",
            "buildUrl": "https://example.test/build.json",
            "replayManifest": {"url": base + "manifest.json", "sha256": sha(manifest)}, "deckData": data,
        }
        return source, manifest, raw

    def test_gzip_http_and_gzip_file_preserve_exact_decoded_bytes(self):
        for suffix, encoding in [(".json", "gzip"), (".json.gz", "identity"), (".json.gz", "gzip")]:
            with self.subTest(suffix=suffix, encoding=encoding), tempfile.TemporaryDirectory() as directory:
                source, manifest, raw = self.fixture(suffix)
                urls = []

                def download(request, timeout):
                    urls.append(request.full_url)
                    if request.full_url == source["replayManifest"]["url"]:
                        return Response(request.full_url, gzip.compress(manifest), "gzip")
                    self.assertEqual(request.full_url, source["deckData"]["url"])
                    return Response(request.full_url, gzip.compress(raw), encoding)

                with patch("fetch_dataset.urlopen", side_effect=download):
                    path, receipt = acquire(source, Path(directory))
                self.assertEqual(path.read_bytes(), raw)
                self.assertEqual(receipt["datasetId"], sha(raw))
                self.assertEqual(receipt["acquiredVia"], "download")
                self.assertEqual(len(urls), 2)
                self.assertFalse(list(Path(directory).rglob("*.tmp")))

    def test_offline_cache_is_reverified_and_never_fetches(self):
        source, _, raw = self.fixture()
        with tempfile.TemporaryDirectory() as directory, patch("fetch_dataset.urlopen") as network:
            cache = Path(directory)
            with self.assertRaisesRegex(ValueError, "offline cache has no verified dataset"):
                acquire(source, cache, offline=True)
            path = cache / "data" / (sha(raw) + ".json")
            path.parent.mkdir()
            path.write_bytes(raw)
            found, receipt = acquire(source, cache, offline=True)
            self.assertEqual(found, path)
            self.assertEqual(receipt["acquiredVia"], "cache")
            path.write_bytes(raw.replace(b"charts", b"broken"))
            with self.assertRaisesRegex(ValueError, "offline cache is invalid"):
                acquire(source, cache, offline=True)
            network.assert_not_called()

    def test_explicit_data_rejects_different_identity_without_network(self):
        source, _, raw = self.fixture()
        with tempfile.TemporaryDirectory() as directory, patch("fetch_dataset.urlopen") as network:
            path = Path(directory) / "data.json"
            path.write_bytes(raw)
            _, receipt = acquire(source, Path(directory), data=path)
            self.assertEqual(receipt["acquiredVia"], "file")
            path.write_bytes(raw + b" ")
            with self.assertRaisesRegex(ValueError, "differs from pinned"):
                acquire(source, Path(directory), data=path)
            network.assert_not_called()

    def test_bad_download_does_not_replace_existing_cache(self):
        source, manifest, raw = self.fixture()
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            path = cache / "data" / (sha(raw) + ".json")
            path.parent.mkdir()
            path.write_bytes(b"invalid existing cache")

            def download(request, timeout):
                value = manifest if request.full_url == source["replayManifest"]["url"] else raw.replace(b"charts", b"broken")
                return Response(request.full_url, value)

            with patch("fetch_dataset.urlopen", side_effect=download), self.assertRaisesRegex(ValueError, "SHA-256"):
                acquire(source, cache)
            self.assertEqual(path.read_bytes(), b"invalid existing cache")
            self.assertFalse(list(cache.rglob("*.tmp")))


if __name__ == "__main__":
    unittest.main()
