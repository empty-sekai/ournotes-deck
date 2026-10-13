"""The installed package on synthetic deck data documents."""
import json
import tomllib
from importlib import metadata
from pathlib import Path

import pytest

import ournotes_sim

PACKAGE = Path(__file__).resolve().parents[1]


def empty_data(charts=()) -> str:
    info = ournotes_sim.info()
    master = {name: {"columns": [], "rows": []} for name in ournotes_sim.MASTER_TABLES}
    return json.dumps({"format": info["dataFormat"], "master": master, "charts": list(charts)})


def test_info():
    info = ournotes_sim.info()
    assert info["name"] == "ournotes-deck"
    assert info["version"] == ournotes_sim.__version__ == metadata.version("ournotes-sim")
    assert info["source"] == "https://github.com/empty-sekai/ournotes-deck"
    assert info["dataFormat"] == "nnnotes.deck-data/1"
    assert info["format"] == "ournotes-deck.chart-stats/3"
    assert len(info["sourceSha256"]) == 64 and set(info["sourceSha256"]) <= set("0123456789abcdef")
    assert info["commit"] is None or (len(info["commit"]) == 40 and set(info["commit"]) <= set("0123456789abcdef"))


def test_master_tables():
    assert ournotes_sim.MASTER_TABLES
    assert len(set(ournotes_sim.MASTER_TABLES)) == len(ournotes_sim.MASTER_TABLES)
    assert all(name.startswith("Master") for name in ournotes_sim.MASTER_TABLES)


def test_chart_stats_without_charts():
    stats = json.loads(ournotes_sim.chart_stats(empty_data(), workers=1, aptitude=False))
    assert stats["format"] == ournotes_sim.info()["format"]
    assert stats["charts"] == []


def test_chart_stats_cached_without_charts(tmp_path):
    text, counts = ournotes_sim.chart_stats_cached(empty_data(), workers=1, aptitude=False, cache_dir=tmp_path)
    assert text == ournotes_sim.chart_stats(empty_data(), workers=1, aptitude=False)
    assert counts == {"requests": 0, "hits": 0, "computed": 0, "writes": 0, "invalid": 0, "bytes": 0}
    assert (tmp_path / "chart-program-v1" / ournotes_sim.info()["sourceSha256"]).is_dir()


def test_invalid_data():
    with pytest.raises(ValueError, match="format"):
        ournotes_sim.chart_stats(json.dumps({"format": "other/1", "master": {}, "charts": []}))
    data = json.loads(empty_data())
    del data["master"][ournotes_sim.MASTER_TABLES[0]]
    with pytest.raises(ValueError, match=ournotes_sim.MASTER_TABLES[0]):
        ournotes_sim.chart_stats(json.dumps(data))


@pytest.mark.skipif(not (PACKAGE / "Cargo.toml").is_file(), reason="needs the source tree")
def test_source_tree():
    package = tomllib.loads((PACKAGE / "Cargo.toml").read_text(encoding="utf-8"))["package"]
    workspace = tomllib.loads((PACKAGE.parent / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]
    assert package["version"] == workspace["version"] == ournotes_sim.__version__
    assert package["license"] == workspace["license"]
    for name in ("LICENSE-MIT", "LICENSE-APACHE"):
        assert (PACKAGE / name).read_bytes() == (PACKAGE.parent / name).read_bytes(), name
