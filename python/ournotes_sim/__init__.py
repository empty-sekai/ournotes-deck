"""The deck model of ournotes-deck (the Rust crate ournotes-sim) for Python.

`chart_stats` and `chart_stats_cached` measure the chart statistics (`ournotes-deck.chart-stats/3`) of a deck data
document (`nnnotes.deck-data/1`); `info` names the model and the formats; `MASTER_TABLES` lists the master tables a
deck data document holds.
"""
from ._native import MASTER_TABLES, __version__, chart_stats, chart_stats_cached, info

__all__ = ["MASTER_TABLES", "__version__", "chart_stats", "chart_stats_cached", "info"]
