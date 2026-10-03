//! Prepared chart, play and performer construction for whole-live simulation.
use super::*;

/// A chart, a judgement stream and the live's numbers, prepared for simulating many decks.
#[derive(Clone, Debug)]
pub(crate) struct FullSetup {
    pub notes: Vec<LiveNote>,
    pub events: Vec<(i32, i32)>,
    pub play: LivePlay,
    pub params: LiveParams,
    /// With Gekisou on: the Gekisou setup, the frame delta times and the seed set.
    pub gk: Option<GkPlay>,
}

/// The Gekisou part of a live objective.
#[derive(Clone, Debug)]
pub(crate) struct GkPlay {
    pub setup: GekisouSetup,
    /// Delta time of each play frame, in seconds.
    pub dt: Vec<f32>,
    pub seeds: Vec<i32>,
}

impl FullSetup {
    pub fn new(
        master: &Master,
        music_level: i32,
        chart: &Chart,
        stream: &JudgementStream,
        judgement_types: &[i32],
    ) -> Result<FullSetup, Error> {
        if judgement_types.len() != chart.notes.len() {
            return Err(Error::Input(format!(
                "{} note judgement types for {} chart notes",
                judgement_types.len(),
                chart.notes.len()
            )));
        }
        let notes = chart
            .notes
            .iter()
            .zip(judgement_types)
            .map(|(n, &jt)| LiveNote {
                note_id: n.id,
                time_ms: n.time_ms,
                note_operate_type: n.note_type,
                judgement_type: jt,
            })
            .collect();
        let events = chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect();
        let assist_factor = if stream.assist {
            let v = master
                .live_settings
                .iter()
                .find(|r| r.key == "assist_score_percent")
                .ok_or_else(|| Error::Master("MasterLiveSettings assist_score_percent missing".into()))?;
            let p: f32 =
                v.value.trim().parse().map_err(|_| Error::Master("assist_score_percent is not a number".into()))?;
            p / 100f32
        } else {
            1.0
        };
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: 0,
            music_level,
            converted_note_count: chart.converted_note_count,
            music_length_ms: chart.last_timing_note_ms.wrapping_add(1000),
            score_music_length_ms: None,
            assist_factor,
        };
        Ok(FullSetup { notes, events, play: stream.to_live_play()?, params, gk: None })
    }

    /// Plays the live with Gekisou on, on these frame delta times and seeds.
    pub fn set_gekisou(&mut self, setup: GekisouSetup, dt: Vec<f32>, seeds: Vec<i32>) {
        self.gk = Some(GkPlay { setup, dt, seeds });
    }

    /// The simulated score of performers (in performance order) at a deck power, Gekisou off.
    pub fn score(&self, master: &Master, performers: &[Performer], power: i32) -> Result<i32, Error> {
        let params = LiveParams { total_power: power, ..self.params };
        let mut lm = LiveModel::new(master, performers, &self.notes, &self.events, params)?;
        lm.run(&self.play)
    }

    /// With Gekisou on: a live of performers at a deck power, before its first frame.
    pub fn gekisou_model(&self, master: &Master, performers: &[Performer], power: i32) -> Result<LiveModel, Error> {
        let g = self.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou live without a Gekisou setup".into()))?;
        let params = LiveParams { total_power: power, ..self.params };
        LiveModel::new_gekisou(master, performers, &self.notes, &self.events, params, &g.setup)
    }

    /// With Gekisou on: the score of performers on every seed, in order, each an independent run of the whole play
    /// with its base seed set to the seed ([`LiveModel::run_timed`]).
    pub fn seed_scores(&self, master: &Master, performers: &[Performer], power: i32) -> Result<Vec<i32>, Error> {
        let g = self.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou live without a Gekisou setup".into()))?;
        let mut play = self.play.clone();
        let mut out = Vec::with_capacity(g.seeds.len());
        for &seed in &g.seeds {
            let mut lm = self.gekisou_model(master, performers, power)?;
            play.base_seed = seed;
            out.push(lm.run_timed(&play, &g.dt)?);
        }
        Ok(out)
    }

    /// Plays frames `from..` of the play with Gekisou on; returns the final score.
    pub(super) fn play_from(&self, lm: &mut LiveModel, from: usize) -> Result<i32, Error> {
        let g = self.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou live without a Gekisou setup".into()))?;
        for (f, &dt) in self.play.frames[from..].iter().zip(&g.dt[from..]) {
            lm.frame_timed(f.time_ms, &f.judged, dt)?;
        }
        Ok(lm.score())
    }
}

/// The performer of a member card paired with a snap (the Gekisou fields are read only by a live with Gekisou on).
pub(crate) fn performer(m: &MemberView, s: Option<&SnapView>) -> Result<Performer, Error> {
    Ok(Performer {
        live_skill: Some((m.live_skill_id, m.live_skill_level)),
        support_skills: match s {
            None => Vec::new(),
            Some(s) => s.support_skills()?,
        },
        band_id: m.band_id,
        character_id: m.character_id,
        card_type: m.card_type,
        tag_ids: m.best_music_tag_ids.clone(),
        live_skill_categories: m.live_skill_categories.clone().unwrap_or_default(),
        gekisou_skill_categories: m.gekisou_skill_categories.clone().unwrap_or_default(),
        gekisou_mission_type: m.gekisou_mission_type.unwrap_or(0),
        gekisou_skill: (m.gekisou_skill_id != 0).then_some((m.gekisou_skill_id, m.gekisou_skill_level)),
        gekisou_support_skills: match s {
            None => Vec::new(),
            Some(s) => s.gekisou_support_skills()?,
        },
    })
}

/// The performers of a deck, in performance order.
pub(crate) fn deck_performers(pool: &Pool, deck: &Deck) -> Result<Vec<Performer>, Error> {
    deck.performance_order
        .iter()
        .map(|&slot| performer(&pool.members[deck.members[slot]], deck.snaps[slot].map(|s| &pool.snaps[s])))
        .collect()
}
