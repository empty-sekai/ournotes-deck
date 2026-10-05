//! One declared play, using the same live model as the native validation and chart tools.
//!
//! A supplied judgement is a completed simulator result before skill conversion. This API does not infer
//! physical touch eligibility. Its clock, result order, skill order, seed and ranking policy are explicit.
use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::data::DeckData;
use crate::live::full::{JudgedNote, LiveModel, LiveNote, LiveParams, Performer, gekisou_rank_factors};
use crate::live::full::{RawJudgedNote, RawJudgementRuntime};
use crate::live::raw::{NoteResult, TimingUnit};
use crate::live::raw_windows::TimingSet;
use crate::live::score::LiveScoreSettings;
use crate::live::skip::is_judgement_note;
use crate::scenario::Scenario;

pub const REQUEST_FORMAT: &str = "ournotes.replay/1";
pub const RESULT_FORMAT: &str = "ournotes.replay-result/1";

fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayPerformer {
    pub live_skill: Option<(i64, i64)>,
    #[serde(default)]
    pub support_skills: Vec<(i64, i64)>,
    #[serde(default)]
    pub band_id: i64,
    #[serde(default)]
    pub character_id: i64,
    #[serde(default)]
    pub card_type: i64,
    #[serde(default)]
    pub tag_ids: Vec<i64>,
    #[serde(default)]
    pub live_skill_categories: Vec<i64>,
    #[serde(default)]
    pub gekisou_skill_categories: Vec<i64>,
    #[serde(default)]
    pub gekisou_mission_type: i64,
    pub gekisou_skill: Option<(i64, i64)>,
    #[serde(default)]
    pub gekisou_support_skills: Vec<(i64, i64)>,
}

impl From<&ReplayPerformer> for Performer {
    fn from(p: &ReplayPerformer) -> Self {
        Self {
            live_skill: p.live_skill,
            support_skills: p.support_skills.clone(),
            band_id: p.band_id,
            character_id: p.character_id,
            card_type: p.card_type,
            tag_ids: p.tag_ids.clone(),
            live_skill_categories: p.live_skill_categories.clone(),
            gekisou_skill_categories: p.gekisou_skill_categories.clone(),
            gekisou_mission_type: p.gekisou_mission_type,
            gekisou_skill: p.gekisou_skill,
            gekisou_support_skills: p.gekisou_support_skills.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RankConfirmation {
    pub frame: usize,
    pub range: usize,
    /// Native group ordinal, not competition/tie rank.
    pub rank: i32,
    pub percent: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReplayMode {
    Normal,
    SoloGekisou,
    /// Counterfactual fixed placement with native Solo timestamp recount; not network ranking.
    FixedSoloGekisou {
        ranks: [i32; 3],
    },
    /// Explicit external confirmation inputs with native network snapshots; no simulated opponents.
    ExternalGekisou {
        confirmations: Vec<RankConfirmation>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayJudgement {
    pub note_id: i32,
    /// 1 Miss, 2 Bad, 3 Good, 4 Great, 5 Perfect, 6 Just, 7 unscored Pass.
    pub judgement: i32,
    pub judgement_time_ms: i32,
    /// Required only when rawRuntime is supplied. FT result metadata is caller input, not inferred touch data.
    #[serde(default)]
    pub raw_result: Option<ReplayRawResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayRawResult {
    pub origin: i32,
    pub timing: i32,
    pub origin_diff_ms: i32,
    pub diff_ms: i32,
    #[serde(default)]
    pub direction_mismatch: bool,
    #[serde(default)]
    pub is_easy_flick: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayRawRuntime {
    /// Level-zero ordered timing units come from the loaded master, with no Assist tail hook.
    pub just_base_expansion_ms: i32,
    pub original_just_before_ms: i32,
    #[serde(default)]
    pub force_enable_just_judgement: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayFrame {
    pub time_ms: i32,
    pub delta_seconds: f32,
    #[serde(default)]
    pub judgements: Vec<ReplayJudgement>,
    /// Explicit lifecycle input to conditional skills; the template derives it from completed input results.
    #[serde(default)]
    pub live_finished: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayRequest {
    pub format: String,
    pub score_id: i64,
    pub power: i32,
    /// Actual audio length, not an inferred last-note duration.
    pub music_length_ms: i32,
    pub score_music_length_ms: Option<i32>,
    #[serde(default = "one")]
    pub assist_factor: f32,
    pub seed: i32,
    pub performers: Vec<ReplayPerformer>,
    /// A permutation of performer indexes. Does not shuffle member target identities.
    pub skill_order: Vec<usize>,
    pub mode: ReplayMode,
    #[serde(default)]
    pub raw_runtime: Option<ReplayRawRuntime>,
    pub frames: Vec<ReplayFrame>,
    #[serde(default = "yes")]
    pub complete: bool,
    #[serde(default)]
    pub trace: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgementCounts {
    pub just: u32,
    pub perfect: u32,
    pub great: u32,
    pub good: u32,
    pub bad: u32,
    pub miss: u32,
    pub pass: u32,
}
impl JudgementCounts {
    fn add(&mut self, grade: i32) {
        match grade {
            1 => self.miss += 1,
            2 => self.bad += 1,
            3 => self.good += 1,
            4 => self.great += 1,
            5 => self.perfect += 1,
            6 => self.just += 1,
            7 => self.pass += 1,
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayRange {
    pub mission: i64,
    pub state: u8,
    pub combo: i32,
    pub max_combo: i32,
    pub just_count: i32,
    pub start_score: i32,
    pub end_score: i32,
    pub luck_points: i32,
    pub luck_gauge: i32,
    pub rush_combo: i32,
    pub lot_results: [i32; 4],
    pub rank_bonus: Option<i32>,
}
fn ranges(model: &LiveModel) -> Vec<ReplayRange> {
    model
        .gekisou_ranges()
        .iter()
        .map(|r| ReplayRange {
            mission: r.mission,
            state: r.state,
            combo: r.combo,
            max_combo: r.max_combo,
            just_count: r.just_count,
            start_score: r.start_score,
            end_score: r.end_score,
            luck_points: r.luck_points,
            luck_gauge: r.luck_gauge,
            rush_combo: r.rush_combo,
            lot_results: r.lot_results,
            rank_bonus: r.rank_bonus,
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BonusEvent {
    pub frame: usize,
    pub range: usize,
    pub rank: i32,
    pub points: i32,
    pub percent: i64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameResult {
    pub frame: usize,
    pub time_ms: i32,
    pub frame_score: i32,
    pub score: i32,
    pub life: i32,
    pub combo: i32,
    pub converted_judgements: Vec<(i32, i32, i32)>,
    pub ranges: Vec<ReplayRange>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResult {
    pub format: &'static str,
    pub input_kind: &'static str,
    pub score_id: i64,
    pub complete: bool,
    pub mode: ReplayMode,
    pub frame_count: usize,
    pub end_time_ms: i32,
    /// Same-frame native renderer cache, before a possible Solo ranking rewind.
    pub frame_score: i32,
    /// Settled calculator channel, matching production LiveModel::score().
    pub score: i32,
    pub life: i32,
    pub combo: i32,
    /// Maximum sampled live-frame combo. Not the unvalidated FT internal per-note maximum.
    pub max_frame_combo: i32,
    pub judgements: JudgementCounts,
    pub converted_judgements: u64,
    pub random_draws: u64,
    pub ranges: Vec<ReplayRange>,
    pub bonus_events: Vec<BonusEvent>,
    pub frames: Vec<FrameResult>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDescription {
    pub note_id: i32,
    pub note_ms: i32,
    pub op_type: i32,
    pub judgement_type: i32,
    pub default_judgement: i32,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDescription {
    pub format: &'static str,
    pub score_id: i64,
    pub music_id: i64,
    pub music_length_ms: Option<i32>,
    /// Native LiveScoreController: chart.LastTimingNotePosition.TimeMs + 1000, independently of audio.
    pub score_music_length_ms: i32,
    pub asset_sha256: String,
    pub notes: Vec<NoteDescription>,
    pub skill_events: Vec<(i32, i32)>,
    pub fevers: Vec<(i32, i32)>,
    pub missions: [i64; 3],
    pub default_performers: Vec<ReplayPerformer>,
    pub default_skill_order: Vec<usize>,
    pub default_seed: i32,
    pub default_fps: u32,
}

/// A parsed, reusable data session. The WASM bridge owns one of these per loaded data version.
pub struct ReplaySession {
    data: DeckData,
}

impl ReplaySession {
    pub fn new(data: DeckData) -> Self {
        Self { data }
    }
    pub fn from_json(data_json: &str) -> Result<Self, Error> {
        Ok(Self::new(DeckData::from_json(data_json)?))
    }

    fn music_id(&self, score_id: i64) -> Result<i64, Error> {
        let mut ids = self
            .data
            .master
            .live_musics
            .iter()
            .filter(|m| [m.easy_id, m.normal_id, m.hard_id, m.expert_id].contains(&score_id));
        let id = ids.next().ok_or_else(|| Error::Input(format!("chart {score_id} has no base music row")))?.id;
        if ids.next().is_some() {
            return Err(Error::Input(format!("chart {score_id} has ambiguous base music rows")));
        }
        Ok(id)
    }

    pub fn describe_chart(&self, score_id: i64) -> Result<ChartDescription, Error> {
        let chart = self.data.data_chart(score_id).ok_or_else(|| Error::Input(format!("unknown chart {score_id}")))?;
        let music_id = self.music_id(score_id)?;
        let scene = Scenario::Free(music_id).resolve(&self.data.master)?;
        let score_chart = chart.chart(&LiveScoreSettings::from_master(&self.data.master)?)?;
        let length = self
            .data
            .provenance
            .get("replay")
            .and_then(|v| v.get("musicLengthsMs"))
            .and_then(|v| v.get(score_id.to_string()))
            .and_then(|v| v.as_i64())
            .and_then(|v| i32::try_from(v).ok())
            .filter(|&v| v > 0);
        Ok(ChartDescription {
            format: "ournotes.replay-chart/1",
            score_id,
            music_id,
            music_length_ms: length,
            score_music_length_ms: score_chart.last_timing_note_ms.wrapping_add(1000),
            asset_sha256: chart.asset_sha256.clone(),
            notes: chart
                .notes
                .iter()
                .zip(&chart.judgement_types)
                .map(|(n, &jt)| NoteDescription {
                    note_id: n.id,
                    note_ms: n.time_ms,
                    op_type: n.note_type,
                    judgement_type: jt,
                    default_judgement: if is_judgement_note(n.note_type) { 5 } else { 7 },
                })
                .collect(),
            skill_events: chart.skill_events().iter().map(|e| (e.index, e.time_ms)).collect(),
            fevers: chart.fevers.clone(),
            missions: scene.gekisou_missions,
            default_performers: vec![ReplayPerformer::default(); 5],
            default_skill_order: (0..5).collect(),
            default_seed: 0,
            default_fps: 60,
        })
    }

    /// Synthetic explicit Perfect/Pass result plan; never guesses Just or physical touch eligibility.
    /// Notes retain chart enumeration order within a frame. The requested clock remains visible in the request.
    pub fn template(&self, score_id: i64, power: i32, fps: u32) -> Result<ReplayRequest, Error> {
        if !matches!(fps, 30 | 60 | 120) {
            return Err(Error::Input("template fps must be 30, 60 or 120".into()));
        }
        if power < 0 {
            return Err(Error::Input("power must be non-negative".into()));
        }
        let d = self.describe_chart(score_id)?;
        let length = d.music_length_ms.ok_or_else(|| {
            Error::Input("actual musicLengthMs is absent from data provenance.replay.musicLengthsMs".into())
        })?;
        let max_note = d.notes.iter().map(|n| n.note_ms).max().unwrap_or(0);
        let max_event = d.skill_events.iter().map(|e| e.1).max().unwrap_or(0);
        let max_fever = d.fevers.iter().map(|e| e.1).max().unwrap_or(0);
        let tail = length
            .max(max_note)
            .max(max_event)
            .max(max_fever)
            .checked_add(2000)
            .ok_or_else(|| Error::Input("template tail clock overflow".into()))?;
        let end_frame = (i64::from(tail) * i64::from(fps) + 999) / 1000;
        let final_note_frame = (i64::from(max_note.max(0)) * i64::from(fps) + 999) / 1000;
        let mut frames: Vec<_> = (0..=end_frame)
            .map(|i| ReplayFrame {
                time_ms: (i * 1000 / i64::from(fps)) as i32,
                delta_seconds: 1.0 / fps as f32,
                judgements: Vec::new(),
                live_finished: i >= final_note_frame,
            })
            .collect();
        for n in d.notes {
            let frame = ((i64::from(n.note_ms.max(0)) * i64::from(fps) + 999) / 1000) as usize;
            frames[frame].judgements.push(ReplayJudgement {
                note_id: n.note_id,
                judgement: n.default_judgement,
                judgement_time_ms: n.note_ms,
                raw_result: None,
            });
        }
        Ok(ReplayRequest {
            format: REQUEST_FORMAT.into(),
            score_id,
            power,
            music_length_ms: length,
            score_music_length_ms: Some(d.score_music_length_ms),
            assist_factor: 1.0,
            seed: 0,
            performers: d.default_performers,
            skill_order: d.default_skill_order,
            mode: ReplayMode::Normal,
            raw_runtime: None,
            frames,
            complete: true,
            trace: false,
        })
    }

    pub fn describe_chart_json(&self, score_id: i64) -> Result<String, Error> {
        json(&self.describe_chart(score_id)?)
    }
    pub fn template_json(&self, score_id: i64, power: i32, fps: u32) -> Result<String, Error> {
        json(&self.template(score_id, power, fps)?)
    }
    pub fn run_json(&self, request_json: &str) -> Result<String, Error> {
        let request: ReplayRequest =
            serde_json::from_str(request_json).map_err(|e| Error::Input(format!("replay JSON: {e}")))?;
        json(&self.run(&request)?)
    }

    pub fn run(&self, r: &ReplayRequest) -> Result<ReplayResult, Error> {
        if r.format != REQUEST_FORMAT {
            return Err(Error::Input(format!("unsupported replay format {}", r.format)));
        }
        if r.power < 0 || r.music_length_ms <= 0 || !r.assist_factor.is_finite() || r.assist_factor < 0.0 {
            return Err(Error::Input("power, actual music length or assist factor is invalid".into()));
        }
        if r.score_music_length_ms.is_none_or(|v| v <= 0) {
            return Err(Error::Input(
                "explicit positive scoreMusicLengthMs is required; audio length is independent".into(),
            ));
        }
        if r.performers.is_empty()
            || r.performers.len() > 5
            || r.skill_order.len() != r.performers.len()
            || r.skill_order.iter().copied().collect::<HashSet<_>>().len() != r.performers.len()
            || r.skill_order.iter().any(|&i| i >= r.performers.len())
        {
            return Err(Error::Input("skillOrder must be a permutation of one to five performers".into()));
        }
        if r.frames.is_empty()
            || r.frames.windows(2).any(|w| w[1].time_ms <= w[0].time_ms)
            || r.frames.iter().any(|f| !f.delta_seconds.is_finite() || f.delta_seconds < 0.0)
        {
            return Err(Error::Input(
                "frames need a strictly increasing clock and finite non-negative binary32 deltaSeconds".into(),
            ));
        }
        let chart =
            self.data.data_chart(r.score_id).ok_or_else(|| Error::Input(format!("unknown chart {}", r.score_id)))?;
        for (member, p) in r.performers.iter().enumerate() {
            let m = &self.data.master;
            if p.live_skill.is_some_and(|(id, level)| {
                !m.live_skill_effects.iter().any(|e| e.live_skill_id == id && e.level == level)
            }) || p.support_skills.iter().any(|&(id, level)| {
                !m.support_skill_effects.iter().any(|e| e.support_skill_id == id && e.level == level)
            }) || p.gekisou_skill.is_some_and(|(id, level)| {
                !m.gekisou_skill_effects.iter().any(|e| e.skill_id == id && e.level == level)
            }) || p.gekisou_support_skills.iter().any(|&(id, level)| {
                !m.gekisou_support_skill_effects.iter().any(|e| e.skill_id == id && e.level == level)
            }) {
                return Err(Error::Input(format!("performer {member}: unknown skill id/level")));
            }
        }
        let settings = LiveScoreSettings::from_master(&self.data.master)?;
        let c = chart.chart(&settings)?;
        let notes: Vec<_> = chart
            .notes
            .iter()
            .zip(&chart.judgement_types)
            .map(|(n, &jt)| LiveNote {
                note_id: n.id,
                time_ms: n.time_ms,
                note_operate_type: n.note_type,
                judgement_type: jt,
            })
            .collect();
        let by_id: HashMap<_, _> = notes.iter().map(|n| (n.note_id, n)).collect();
        if by_id.len() != notes.len() {
            return Err(Error::Input("chart contains duplicate note ids".into()));
        }
        let mut seen = HashSet::new();
        for (i, f) in r.frames.iter().enumerate() {
            for j in &f.judgements {
                let note = by_id
                    .get(&j.note_id)
                    .ok_or_else(|| Error::Input(format!("frame {i}: unknown note {}", j.note_id)))?;
                if !seen.insert(j.note_id) {
                    return Err(Error::Input(format!("note {} has more than one supplied final result", j.note_id)));
                }
                if !(1..=7).contains(&j.judgement)
                    || (!is_judgement_note(note.note_operate_type) && j.judgement != 7)
                    || (is_judgement_note(note.note_operate_type) && j.judgement == 7)
                    || j.judgement_time_ms > f.time_ms
                {
                    return Err(Error::Input(format!("frame {i}: invalid result grade/time for note {}", j.note_id)));
                }
                if r.raw_runtime.is_some() != j.raw_result.is_some()
                    || j.raw_result
                        .as_ref()
                        .is_some_and(|raw| !(1..=7).contains(&raw.origin) || !(0..=3).contains(&raw.timing))
                {
                    return Err(Error::Input(format!(
                        "frame {i}: rawRuntime requires explicit valid rawResult on every judgement"
                    )));
                }
            }
        }
        if r.complete && (seen.len() != notes.len() || r.frames.last().unwrap().time_ms < r.music_length_ms) {
            return Err(Error::Input("complete replay requires every chart result and a clock through actual musicLengthMs; missing results are not filled".into()));
        }
        let music_id = self.music_id(r.score_id)?;
        let scene = Scenario::Free(music_id).resolve(&self.data.master)?;
        let setup = scene.gekisou_setup(&chart.fevers);
        let row =
            self.data.master.live_music_score(r.score_id).ok_or_else(|| Error::Input("missing score row".into()))?;
        let params = LiveParams {
            skill_target_music_type: scene.skill_target_music_type,
            total_power: r.power,
            music_level: i32::try_from(row.music_score_level)
                .map_err(|_| Error::Master("music level exceeds i32".into()))?,
            converted_note_count: c.converted_note_count,
            music_length_ms: r.music_length_ms,
            score_music_length_ms: r.score_music_length_ms,
            assist_factor: r.assist_factor,
        };
        let deck: Vec<_> = r.performers.iter().map(Performer::from).collect();
        let mut events = Vec::new();
        for e in chart.skill_events() {
            let position =
                usize::try_from(e.index).map_err(|_| Error::Input("negative chart skill event index".into()))?;
            let index = *r
                .skill_order
                .get(position)
                .ok_or_else(|| Error::Input("chart skill event exceeds skillOrder; no implicit cycling".into()))?;
            events.push((index as i32, e.time_ms));
        }
        let mut model = match &r.mode {
            ReplayMode::Normal => LiveModel::new(&self.data.master, &deck, &notes, &events, params)?,
            ReplayMode::SoloGekisou => {
                LiveModel::new_gekisou(&self.data.master, &deck, &notes, &events, params, &setup)?
            }
            ReplayMode::FixedSoloGekisou { ranks } => {
                if ranks.iter().any(|rank| !(1..=5).contains(rank)) {
                    return Err(Error::Input("fixed Solo ranks must be 1..=5".into()));
                }
                let mut m = LiveModel::new_gekisou_ranked(&self.data.master, &deck, &notes, &events, params, &setup)?;
                let factors = gekisou_rank_factors(&self.data.master, &scene.gekisou_missions)?;
                for (range, &rank) in ranks.iter().enumerate().take(setup.fevers.len()) {
                    m.queue_gekisou_rank_confirmation(range, rank, factors[range][rank as usize - 1])?;
                }
                m
            }
            ReplayMode::ExternalGekisou { .. } => {
                LiveModel::new_gekisou_external(&self.data.master, &deck, &notes, &events, params, &setup)?
            }
        };
        if let Some(raw) = &r.raw_runtime {
            if raw.just_base_expansion_ms < 0 || raw.original_just_before_ms < 0 {
                return Err(Error::Input("raw window bases must be non-negative".into()));
            }
            let mut timing_rows: Vec<_> =
                self.data.master.live_judgement_timings.iter().filter(|row| row.assist_level == 0).collect();
            timing_rows.sort_by_key(|row| (row.note_judgement_type, row.judgement_priority));
            let mut sets: BTreeMap<i32, Vec<TimingUnit>> = BTreeMap::new();
            for row in timing_rows {
                let integer = |v| i32::try_from(v).map_err(|_| Error::Master("raw timing exceeds i32".into()));
                let grade = integer(row.note_simulate_judgement)?;
                let mut unit = TimingUnit::new(grade, integer(row.before_ms)?, integer(row.after_ms)?);
                if grade == 6 {
                    unit.enabled = raw.force_enable_just_judgement;
                }
                sets.entry(integer(row.note_judgement_type)?).or_default().push(unit);
            }
            if notes.iter().any(|n| !sets.contains_key(&n.judgement_type)) {
                return Err(Error::Master("raw timing units absent for a chart judgement type".into()));
            }
            let mut runtime = RawJudgementRuntime::new(
                sets.into_iter().map(|(judgement_type, units)| TimingSet { judgement_type, units }).collect(),
                raw.just_base_expansion_ms,
                raw.original_just_before_ms,
            );
            runtime.force_enable_just_judgement = raw.force_enable_just_judgement;
            model.enable_raw_runtime(runtime)?;
        }
        let mut confirmations: BTreeMap<usize, Vec<&RankConfirmation>> = BTreeMap::new();
        if let ReplayMode::ExternalGekisou { confirmations: inputs } = &r.mode {
            let factors = gekisou_rank_factors(&self.data.master, &scene.gekisou_missions)?;
            let mut ranges = HashSet::new();
            for c in inputs {
                if c.frame >= r.frames.len()
                    || c.range >= setup.fevers.len().min(3)
                    || !(1..=5).contains(&c.rank)
                    || !ranges.insert(c.range)
                    || c.percent != factors[c.range][c.rank as usize - 1]
                {
                    return Err(Error::Input("external confirmation needs one known range, in-frame group rank and matching master percentage".into()));
                }
                confirmations.entry(c.frame).or_default().push(c);
            }
        }
        model.set_seed(r.seed);
        let mut counts = JudgementCounts::default();
        let mut trace = Vec::new();
        let mut bonuses = Vec::new();
        let mut bonus_count = 0;
        let mut max_frame_combo = 0;
        for (i, f) in r.frames.iter().enumerate() {
            if let Some(cs) = confirmations.get(&i) {
                for c in cs {
                    model.queue_gekisou_rank_confirmation(c.range, c.rank, c.percent)?;
                }
            }
            model.set_live_finished(f.live_finished);
            if r.raw_runtime.is_some() {
                let judged: Vec<_> = f
                    .judgements
                    .iter()
                    .map(|j| {
                        let raw = j.raw_result.as_ref().expect("validated raw input");
                        RawJudgedNote {
                            note_id: j.note_id,
                            result: NoteResult {
                                origin: raw.origin,
                                judgement: j.judgement,
                                judgement_type: by_id[&j.note_id].judgement_type,
                                timing: raw.timing,
                                time_ms: j.judgement_time_ms,
                                origin_diff_ms: raw.origin_diff_ms,
                                diff_ms: raw.diff_ms,
                            },
                            direction_mismatch: raw.direction_mismatch,
                            is_easy_flick: raw.is_easy_flick,
                        }
                    })
                    .collect();
                model.frame_raw_timed(f.time_ms, &judged, f.delta_seconds)?;
            } else {
                let judged: Vec<_> = f
                    .judgements
                    .iter()
                    .map(|j| JudgedNote {
                        note_id: j.note_id,
                        judgement: j.judgement,
                        judgement_time_ms: j.judgement_time_ms,
                    })
                    .collect();
                model.frame_timed(f.time_ms, &judged, f.delta_seconds)?;
            }
            for &(_, grade, _) in model.frame_judgements() {
                counts.add(grade);
            }
            max_frame_combo = max_frame_combo.max(model.current_combo());
            for &(range, rank, points, percent) in &model.gekisou_rank_bonuses()[bonus_count..] {
                bonuses.push(BonusEvent { frame: i, range, rank, points, percent });
            }
            bonus_count = model.gekisou_rank_bonuses().len();
            if r.trace {
                trace.push(FrameResult {
                    frame: i,
                    time_ms: f.time_ms,
                    frame_score: model.frame_score(),
                    score: model.score(),
                    life: model.current_life(),
                    combo: model.current_combo(),
                    converted_judgements: model.frame_judgements().to_vec(),
                    ranges: ranges(&model),
                });
            }
        }
        let final_ranges = ranges(&model);
        if r.complete
            && !matches!(r.mode, ReplayMode::Normal)
            && final_ranges.iter().any(|s| s.state != 8 || s.rank_bonus.is_none())
        {
            return Err(Error::Input("complete Gekisou replay ends before every range and rank confirmation settled; extend the explicit clock/input".into()));
        }
        Ok(ReplayResult {
            format: RESULT_FORMAT,
            input_kind: if r.raw_runtime.is_some() {
                "givenRawResultsBeforeSkillConversion"
            } else {
                "givenJudgementsBeforeSkillConversion"
            },
            score_id: r.score_id,
            complete: r.complete,
            mode: r.mode.clone(),
            frame_count: r.frames.len(),
            end_time_ms: r.frames.last().unwrap().time_ms,
            frame_score: model.frame_score(),
            score: model.score(),
            life: model.current_life(),
            combo: model.current_combo(),
            max_frame_combo,
            judgements: counts,
            converted_judgements: model.converted_judgements(),
            random_draws: model.draws(),
            ranges: final_ranges,
            bonus_events: bonuses,
            frames: trace,
        })
    }
}

fn json<T: Serialize>(value: &T) -> Result<String, Error> {
    serde_json::to_string(value).map_err(|e| Error::Input(format!("replay output JSON: {e}")))
}
