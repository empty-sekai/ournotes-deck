//! The deck data file (`nnnotes.deck-data/1`): the master tables and every chart of one master data version in one
//! JSON document, as written by `nnnotes deck-data`.
//!
//! The file holds facts only; the chart counts the client derives (judged notes, the per-note divisor, the last
//! timing note) are computed here from the notes. Unknown keys are ignored; a file of another major version is
//! rejected.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::error::Error;
use crate::live::score::LiveScoreSettings;
use crate::live::skip::{Chart, ChartNote, SkillEvent, judgement_note_total_count};
use crate::master::Master;

/// The format this reader reads.
pub const FORMAT: &str = "nnnotes.deck-data/1";
const FORMAT_FAMILY: &str = "nnnotes.deck-data/";

/// A chart as the client builds it at runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataChart {
    /// `MasterLiveMusicScore._id`.
    pub score_id: i64,
    pub asset_key: String,
    pub asset_sha256: String,
    /// Every runtime note, in the client's enumeration order (not time order).
    pub notes: Vec<ChartNote>,
    /// `NoteJudgementType` of each note of `notes`.
    pub judgement_types: Vec<i32>,
    /// Skill event times in chart order; the position is the event index.
    pub skill_event_ms: Vec<i32>,
    /// Fever ranges `(start, end)` in ms, sorted by start; the position is the range index.
    pub fevers: Vec<(i32, i32)>,
}

impl DataChart {
    /// The skill events with their indexes.
    pub fn skill_events(&self) -> Vec<SkillEvent> {
        self.skill_event_ms.iter().enumerate().map(|(i, &t)| SkillEvent { index: i as i32, time_ms: t }).collect()
    }

    /// The chart the score code reads, with the counts computed from the notes and the note score table.
    pub fn chart(&self, settings: &LiveScoreSettings) -> Result<Chart, Error> {
        Chart::from_notes(self.notes.clone(), self.skill_events(), settings)
            .map_err(|e| Error::Input(format!("chart {}: {e}", self.score_id)))
    }
}

/// A deck data file.
#[derive(Clone, Debug)]
pub struct DeckData {
    /// The provenance object, as written.
    pub provenance: serde_json::Value,
    pub master: Master,
    /// Sorted by score id.
    pub charts: Vec<DataChart>,
}

#[derive(Deserialize)]
struct Head {
    format: String,
}

#[derive(Deserialize)]
struct FileRaw {
    #[serde(default)]
    provenance: serde_json::Value,
    master: BTreeMap<String, TableRaw>,
    charts: Vec<ChartRaw>,
}

#[derive(Deserialize)]
struct TableRaw {
    columns: Vec<String>,
    rows: Vec<Vec<Box<RawValue>>>,
}

#[derive(Deserialize)]
struct AssetRaw {
    key: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotesRaw {
    id: Vec<i32>,
    op: Vec<i32>,
    judgement_type: Vec<i32>,
    time_ms: Vec<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimesRaw {
    time_ms: Vec<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeversRaw {
    start_ms: Vec<i32>,
    end_ms: Vec<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartRaw {
    score_id: i64,
    asset: AssetRaw,
    notes: NotesRaw,
    skill_events: TimesRaw,
    fevers: FeversRaw,
}

/// A table's rows as the `{"_allData": [row objects]}` text the master loader reads. A null value is left out, so it
/// reads like an absent column.
fn table_text(name: &str, t: &TableRaw) -> Result<String, Error> {
    let mut seen = HashSet::new();
    for c in &t.columns {
        if !seen.insert(c.as_str()) {
            return Err(Error::Input(format!("table {name}: column {c} listed twice")));
        }
    }
    let keys: Vec<String> = t.columns.iter().map(|c| serde_json::to_string(c).expect("string")).collect();
    let mut out = String::from("{\"_allData\":[");
    for (i, row) in t.rows.iter().enumerate() {
        if row.len() != t.columns.len() {
            return Err(Error::Input(format!(
                "table {name}: row {i} has {} values for {} columns",
                row.len(),
                t.columns.len()
            )));
        }
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        let mut first = true;
        for (k, v) in keys.iter().zip(row) {
            if v.get().trim() == "null" {
                continue;
            }
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str(k);
            out.push(':');
            out.push_str(v.get());
        }
        out.push('}');
    }
    out.push_str("]}");
    Ok(out)
}

fn chart_of(c: ChartRaw, master: &Master) -> Result<DataChart, Error> {
    let id = c.score_id;
    let n = c.notes.id.len();
    if c.notes.op.len() != n || c.notes.judgement_type.len() != n || c.notes.time_ms.len() != n {
        return Err(Error::Input(format!("chart {id}: note columns differ in length")));
    }
    let mut seen = HashSet::with_capacity(n);
    for &nid in &c.notes.id {
        if !seen.insert(nid) {
            return Err(Error::Input(format!("chart {id}: note id {nid} listed twice")));
        }
    }
    if c.fevers.start_ms.len() != c.fevers.end_ms.len() {
        return Err(Error::Input(format!("chart {id}: fever columns differ in length")));
    }
    if c.fevers.start_ms.windows(2).any(|w| w[0] > w[1]) {
        return Err(Error::Input(format!("chart {id}: fever ranges are not sorted by start")));
    }
    let notes: Vec<ChartNote> = (0..n)
        .map(|i| ChartNote { id: c.notes.id[i], time_ms: c.notes.time_ms[i], note_type: c.notes.op[i] })
        .collect();
    let row =
        master.live_music_score(id).ok_or_else(|| Error::Input(format!("chart {id}: no MasterLiveMusicScore row")))?;
    let judged = judgement_note_total_count(&notes) as i64;
    if judged != row.full_combo_count {
        return Err(Error::Input(format!(
            "chart {id}: {judged} judged notes, the score row's full combo count is {}",
            row.full_combo_count
        )));
    }
    Ok(DataChart {
        score_id: id,
        asset_key: c.asset.key,
        asset_sha256: c.asset.sha256,
        notes,
        judgement_types: c.notes.judgement_type,
        skill_event_ms: c.skill_events.time_ms,
        fevers: c.fevers.start_ms.into_iter().zip(c.fevers.end_ms).collect(),
    })
}

impl DeckData {
    /// Reads a deck data document. Checks: the format, that every row has one value per column, equal column
    /// lengths inside a chart, unique note ids, fevers sorted by start, charts sorted by score id, and that the
    /// judged notes of each chart match its score row's full combo count.
    pub fn from_json(text: &str) -> Result<DeckData, Error> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let head: Head = serde_json::from_str(text).map_err(|e| Error::Input(format!("deck data: {e}")))?;
        if head.format != FORMAT {
            return Err(Error::Input(if head.format.starts_with(FORMAT_FAMILY) {
                format!("deck data format {} is not supported (this reader reads {FORMAT})", head.format)
            } else {
                format!("not a deck data file (format {:?})", head.format)
            }));
        }
        let raw: FileRaw = serde_json::from_str(text).map_err(|e| Error::Input(format!("deck data: {e}")))?;
        let mut texts: BTreeMap<&str, String> = BTreeMap::new();
        for (name, t) in &raw.master {
            texts.insert(name.as_str(), table_text(name, t)?);
        }
        let master = Master::from_json_tables(|n| texts.get(n).map(|s| s.as_str()))?;
        drop(texts);
        let mut charts = Vec::with_capacity(raw.charts.len());
        for c in raw.charts {
            charts.push(chart_of(c, &master)?);
        }
        if charts.windows(2).any(|w| w[0].score_id >= w[1].score_id) {
            return Err(Error::Input("deck data: charts are not sorted by score id".into()));
        }
        Ok(DeckData { provenance: raw.provenance, master, charts })
    }

    /// Reads a deck data file (plain JSON; a gzip file has to be decompressed first).
    pub fn from_path(path: impl AsRef<Path>) -> Result<DeckData, Error> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| Error::Input(format!("{}: {e}", path.display())))?;
        DeckData::from_json(&text)
    }

    /// The chart of a score id.
    pub fn data_chart(&self, score_id: i64) -> Option<&DataChart> {
        self.charts.binary_search_by_key(&score_id, |c| c.score_id).ok().map(|i| &self.charts[i])
    }

    /// The chart of a score id as the score code reads it.
    pub fn chart(&self, score_id: i64) -> Result<Chart, Error> {
        let c = self.data_chart(score_id).ok_or_else(|| Error::Input(format!("no chart for score id {score_id}")))?;
        c.chart(&LiveScoreSettings::from_master(&self.master)?)
    }
}
