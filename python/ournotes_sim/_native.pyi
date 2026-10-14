import os
from typing import TypedDict

class Info(TypedDict):
    name: str
    version: str
    source: str
    commit: str | None
    sourceSha256: str
    dataFormat: str
    format: str

class CacheCounts(TypedDict):
    requests: int
    hits: int
    computed: int
    writes: int
    invalid: int
    bytes: int

__version__: str
MASTER_TABLES: tuple[str, ...]

def chart_stats(data: str, seeds: int | None = None, workers: int | None = None, aptitude: bool = True) -> str: ...
def chart_stats_cached(
    data: str,
    seeds: int | None = None,
    workers: int | None = None,
    aptitude: bool = True,
    *,
    cache_dir: str | os.PathLike[str],
) -> tuple[str, CacheCounts]: ...
def info() -> Info: ...
