//! `ournotes_sim._native`: the chart statistics of the deck model ournotes-sim (`ournotes-deck.chart-stats/3`) on a
//! deck data document held in memory, the charts measured in parallel.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use ournotes_sim::chartstats::{self, ChartStats, ChartStatsCache, Options, REPLAY_SEEDS};
use ournotes_sim::data::DeckData;
use ournotes_sim::error::Error;
use ournotes_sim::master::TABLES;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

fn value_error(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// The statistics of every chart, in chart order, on `workers` threads.
fn measure(
    data: &DeckData,
    options: &Options,
    workers: usize,
    cache: Option<&ChartStatsCache>,
) -> Result<Vec<ChartStats>, Error> {
    let kinds = chartstats::kinds(&data.master);
    let n = data.charts.len();
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<Result<ChartStats, Error>>>> = Mutex::new((0..n).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers.clamp(1, n.max(1)) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let c = &data.charts[i];
                    // as chartstats::document names a chart's domain errors
                    let r = match cache {
                        Some(cache) => chartstats::chart_stats_with_cache(&data.master, c, &kinds, options, cache),
                        None => chartstats::chart_stats_with(&data.master, c, &kinds, options),
                    }
                    .map_err(|e| match e {
                        Error::Domain(m) => Error::Domain(format!("chart {}: {m}", c.score_id)),
                        e => e,
                    });
                    let failed = r.is_err();
                    out.lock().expect("results")[i] = Some(r);
                    if failed {
                        next.store(n, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    // charts are taken in order and a taken chart always completes, so every chart before the first failure has
    // its result: the error returned is the first in chart order, as chartstats::document's
    let mut stats = Vec::with_capacity(n);
    for r in out.into_inner().expect("results") {
        stats.push(r.expect("a chart before the first failure has run")?);
    }
    Ok(stats)
}

/// The chart statistics document of `data` as JSON text.
fn document(
    data: &str,
    seeds: Option<usize>,
    workers: Option<usize>,
    aptitude: bool,
    cache: Option<&ChartStatsCache>,
) -> PyResult<String> {
    let options = Options { replay_seeds: seeds.unwrap_or(REPLAY_SEEDS), aptitude };
    let workers = workers.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let mut data = DeckData::from_json(data).map_err(value_error)?;
    let charts = std::mem::take(&mut data.charts);
    let mut doc = chartstats::document_with(&data, &options).map_err(value_error)?;
    data.charts = charts;
    let stats = measure(&data, &options, workers, cache).map_err(value_error)?;
    doc["charts"] = serde_json::to_value(stats).map_err(value_error)?;
    serde_json::to_string(&doc).map_err(value_error)
}

/// chart_stats(data, seeds=None, workers=None, aptitude=True) -> str
///
/// The `ournotes-deck.chart-stats/3` document (JSON text) of a deck data document (`nnnotes.deck-data/1` JSON text).
/// `seeds`: the number of replay seeds (default 8); expectations use independent nominal probabilities.
/// `workers`: the number of threads the charts are measured on (default: the available parallelism).
/// `aptitude`: include the single Gekisou skill shapes (default true).
/// Raises ValueError for invalid data or a failed expectation check.
#[pyfunction]
#[pyo3(signature = (data, seeds=None, workers=None, aptitude=true))]
fn chart_stats(
    py: Python<'_>,
    data: &str,
    seeds: Option<usize>,
    workers: Option<usize>,
    aptitude: bool,
) -> PyResult<String> {
    py.detach(|| document(data, seeds, workers, aptitude, None))
}

/// chart_stats_cached(data, seeds=None, workers=None, aptitude=True, *, cache_dir) -> (str, dict)
///
/// chart_stats with the simulation programs kept in `cache_dir` (under a directory named by the model's source
/// SHA-256), and the cache's counters (requests, hits, computed, writes, invalid, bytes). Programs completed before a
/// failure stay in the cache; every call validates the whole input and builds the document's header again.
#[pyfunction]
#[pyo3(signature = (data, seeds=None, workers=None, aptitude=true, *, cache_dir))]
fn chart_stats_cached<'py>(
    py: Python<'py>,
    data: &str,
    seeds: Option<usize>,
    workers: Option<usize>,
    aptitude: bool,
    cache_dir: PathBuf,
) -> PyResult<(String, Bound<'py, PyDict>)> {
    let (text, stats) = py.detach(|| {
        let cache = ChartStatsCache::new(&cache_dir).map_err(value_error)?;
        let text = document(data, seeds, workers, aptitude, Some(&cache))?;
        Ok::<_, PyErr>((text, cache.snapshot()))
    })?;
    let counts = PyDict::new(py);
    counts.set_item("requests", stats.requests)?;
    counts.set_item("hits", stats.hits)?;
    counts.set_item("computed", stats.computed)?;
    counts.set_item("writes", stats.writes)?;
    counts.set_item("invalid", stats.invalid)?;
    counts.set_item("bytes", stats.bytes)?;
    Ok((text, counts))
}

/// info() -> dict: the deck model of this module (name: the ournotes-deck repository; version and source: the
/// package as built; commit: the source commit of a release build, None otherwise; sourceSha256: the SHA-256 of the
/// ournotes-sim sources, equal for builds that run the same model) and the formats it reads and writes.
#[pyfunction]
fn info(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("name", "ournotes-deck")?;
    d.set_item("version", env!("CARGO_PKG_VERSION"))?;
    d.set_item("source", env!("CARGO_PKG_REPOSITORY"))?;
    d.set_item("commit", option_env!("SOURCE_COMMIT"))?;
    d.set_item("sourceSha256", ournotes_sim::SOURCE_SHA256)?;
    d.set_item("dataFormat", ournotes_sim::data::FORMAT)?;
    d.set_item("format", chartstats::FORMAT)?;
    Ok(d)
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("MASTER_TABLES", PyTuple::new(m.py(), TABLES)?)?;
    m.add_function(wrap_pyfunction!(chart_stats, m)?)?;
    m.add_function(wrap_pyfunction!(chart_stats_cached, m)?)?;
    m.add_function(wrap_pyfunction!(info, m)?)?;
    Ok(())
}
