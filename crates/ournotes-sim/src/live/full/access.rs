//! Settings and readouts of a Live model between frames.

use super::*;

impl LiveModel {
    /// Supply the previous frame's confirmed rank for condition 7012.
    /// The event is cleared after the next frame consumes it. This does not calculate or award ranking bonuses.
    pub fn set_previous_gekisou_rank_confirmation(&mut self, rank: Option<i32>) {
        self.prev_confirmed_rank = rank;
    }

    /// Queue a confirmed rank and its explicit bonus percentage for an externally ranked adapter.
    /// `new_gekisou_external` uses native network frame snapshots; `new_gekisou_ranked` uses solo timestamp
    /// queries for counterfactual fixed ranks. Applied after the range has completed.
    /// Identical retries are idempotent; conflicting confirmations are errors. These are adapter policies.
    /// Multiple confirmations applied together publish the last range's rank through the native scalar event.
    pub fn queue_gekisou_rank_confirmation(&mut self, range: usize, rank: i32, percent: i64) -> Result<(), Error> {
        let g = self.gk.as_mut().ok_or_else(|| Error::Input("rank confirmation without Gekisou".into()))?;
        if !g.external_ranking {
            return Err(Error::Input("external ranking mode is required".into()));
        }
        if rank <= 0 {
            return Err(Error::Input("confirmed rank must be positive".into()));
        }
        let pending = g.pending_ranks.get_mut(range).ok_or_else(|| Error::Input("rank range out of bounds".into()))?;
        if let Some(&(_, old_rank, _, old_percent)) = g.rank_bonus.iter().find(|r| r.0 == range) {
            return if (old_rank, old_percent) == (rank, percent) {
                Ok(())
            } else {
                Err(Error::Input("conflicting completed rank confirmation".into()))
            };
        }
        if pending.is_some_and(|old| old != (rank, percent)) {
            return Err(Error::Input("conflicting pending rank confirmation".into()));
        }
        *pending = Some((rank, percent));
        Ok(())
    }

    /// Install immutable, frame-indexed external arrivals before play starts.
    /// Frame-zero arrivals implement the declared rank-on-completion scenario:
    /// the controller still applies each bonus only after its actual completion.
    pub fn set_rank_confirmation_timeline(
        &mut self,
        confirmations: &[crate::replay::RankConfirmation],
    ) -> Result<(), Error> {
        let g = self.gk.as_ref().ok_or_else(|| Error::Input("rank timeline without Gekisou".into()))?;
        if !g.external_ranking || self.frames_played() != 0 {
            return Err(Error::Input("rank timeline requires an unplayed external-ranking model".into()));
        }
        let mut seen = std::collections::HashSet::new();
        if confirmations
            .iter()
            .any(|c| c.range >= g.pending_ranks.len() || !(1..=5).contains(&c.rank) || !seen.insert(c.range))
        {
            return Err(Error::Input("rank timeline requires unique known ranges and group ranks 1..5".into()));
        }
        self.rank_timeline = confirmations.to_vec();
        self.rank_timeline.sort_by_key(|c| (c.frame, c.range));
        self.next_rank_confirmation = 0;
        Ok(())
    }

    /// Diagnostics only: undos of score frames already declared settled.
    #[cfg(feature = "search-diagnostics")]
    pub fn settled_violations(&self) -> u64 {
        self.score.settled_violations
    }

    /// The current score.
    pub fn score(&self) -> i32 {
        self.score.score
    }

    /// Enables observation of the score-up factor used by each subsequent note
    /// execution, including executions after an undo. Repeated calls keep prior observations.
    pub fn track_score_up_factors(&mut self) {
        self.score.minimum_score_up.get_or_insert(f32::INFINITY);
    }

    /// The least observed score-up factor, or `None` when observation is disabled.
    /// An enabled calculator with no note executions returns positive infinity;
    /// a NaN observation remains NaN across later executions and model clones.
    pub fn minimum_score_up(&self) -> Option<f32> {
        self.score.minimum_score_up
    }

    /// The lottery-dependent note score-ups of this live's Gekisou (support) skills ([`luck_skills`]).
    pub(crate) fn luck_score_rows(&self, skills: &LuckSkills) -> Vec<LuckScoreRow> {
        luck_score_rows(&self.cond, &self.rows, skills)
    }

    /// Whether a trigger, condition, reset or release of the deck's effects reads the lottery or a probability.
    pub(crate) fn reads_lottery(&self) -> bool {
        let has = |x: &Option<Checker>| x.as_ref().is_some_and(|x| x.any(&is_lottery_predicate));
        self.cond.iter().any(|s| {
            s.updater.effects().iter().any(|ef| has(&ef.trigger) || has(&ef.condition) || has(&ef.reset))
                || s.updater.updaters.iter().any(|u| has(&u.release))
        }) || self.live.iter().any(|s| s.effects.iter().any(|e| has(&e.condition) || has(&e.release)))
    }

    /// Makes this Gekisou live a LUCK weighted live: it draws no lottery; every luck range runs the rush score bonus
    /// from its start to its finish; the lottery-dependent note score-ups ([`luck_skills`]) file nothing, and a note
    /// scores its expectation over the lottery ([`LiveScoreCalculator::score_up_and_luck`]) from the deck's score-up
    /// per shape of `skills` and `steps`, the probabilities `(from ms, [rush, s_0, rs_0, s_1, rs_1, ...])` with
    /// increasing times. Probability conditions may gate only luck chain effects, which do nothing without a lottery,
    /// and lottery-dependent score-ups, so the live draws no random value. Call it before the first frame.
    pub fn set_luck_weights(&mut self, skills: &LuckSkills, steps: Vec<(i32, Vec<f32>)>) -> Result<(), Error> {
        let width = 1 + 2 * skills.shapes.len();
        for (_, p) in &steps {
            if p.len() != width {
                return Err(Error::Input(format!("LUCK weights of width {} for {width}", p.len())));
            }
            if let Some(c) = p.iter().find(|c| !(0.0..=1.0).contains(*c)) {
                return Err(Error::Input(format!("LUCK weight {c} outside [0, 1]")));
            }
        }
        if steps.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err(Error::Input("LUCK weight steps need increasing times".into()));
        }
        if self.score.score != 0 || self.random.draws() != 0 || self.frame_time != 0 {
            return Err(Error::Input("LUCK weights apply before the first frame".into()));
        }
        let mut suppressed = vec![false; self.rows.len()];
        let mut values = vec![0f32; skills.shapes.len()];
        for r in self.luck_score_rows(skills) {
            suppressed[r.row] = true;
            if r.may_hold {
                values[r.shape] += r.value;
            }
        }
        let has = |x: &Option<Checker>| x.as_ref().is_some_and(|x| x.any(&is_lottery_predicate));
        for s in &self.cond {
            for ef in s.updater.effects() {
                let row = &self.rows[ef.row];
                let reads = has(&ef.trigger) || has(&ef.condition) || has(&ef.reset);
                if reads && !suppressed[ef.row] && !is_luck_chain(row.effect_type) {
                    return Err(Error::Unsupported(format!(
                        "LUCK weights: effect row {} of type {} reads the lottery or a probability",
                        row.id, row.effect_type
                    )));
                }
            }
            for u in &s.updater.updaters {
                let row = s.updater.effect(u.effect).row;
                if !suppressed[row] && has(&u.release) {
                    return Err(Error::Unsupported(format!(
                        "LUCK weights: effect row {} releases on the lottery or a probability",
                        self.rows[row].id
                    )));
                }
            }
        }
        for s in &self.live {
            for e in &s.effects {
                if has(&e.condition) || has(&e.release) {
                    return Err(Error::Unsupported(
                        "LUCK weights: a live skill reads the lottery or a probability".into(),
                    ));
                }
            }
        }
        let gk = self.gk.as_mut().ok_or_else(|| Error::Input("LUCK weights without Gekisou".into()))?;
        gk.ctrl.luck_weighted = true;
        self.score.calc.luck_weight = Some(std::sync::Arc::new(LuckWeights { values, steps }));
        self.luck_suppressed = suppressed;
        Ok(())
    }

    /// Seeds the live's random streams, as [`LiveModel::run`] does with the play's base seed. Before any value has
    /// been drawn ([`LiveModel::draws`] is 0) this equals building the live with that seed.
    pub fn set_seed(&mut self, seed: i32) {
        self.random.set_seed(seed);
    }

    /// Supplies the live-finished input before a frame. This is a lifecycle signal, not a guessed music-time cutoff.
    /// It suppresses new condition-skill triggers; already running one-shot effects continue updating.
    pub fn set_live_finished(&mut self, finished: bool) {
        self.is_live_finished = finished;
    }

    /// The number of random values drawn since the streams were last seeded.
    pub fn draws(&self) -> u64 {
        self.random.draws()
    }

    /// Replaces the live's random streams, as [`LiveModel::run_with_random`] does before the first frame.
    pub fn set_random(&mut self, random: LiveRandom) {
        self.random = random;
    }

    /// The number of frames played so far.
    pub fn frames_played(&self) -> usize {
        self.trace.len()
    }

    /// The performance positions that have acted so far (bit k: position k): those whose chart skill events have
    /// fired, and those whose condition skills have started an effect or drawn a random value. Members of the other
    /// positions can still move (see [`OrderedLive`]).
    pub fn acted_positions(&self) -> (u32, u32) {
        (self.touched_events, self.touched_skills)
    }

    /// `(frame time, score after the frame)` of every frame played.
    pub fn trace(&self) -> &[(i32, i32)] {
        &self.trace
    }

    /// The chart-time spans of the live's rush score bonus commands: `(added at, disabled at)`, `i32::MAX` while
    /// running.
    pub fn rush_command_spans(&self) -> Vec<(i32, i32)> {
        self.rush_spans()
    }

    pub(crate) fn rush_spans(&self) -> Vec<(i32, i32)> {
        self.gk.as_ref().map(|g| g.ctrl.rush_log().to_vec()).unwrap_or_default()
    }

    /// Diagnostics only: the Gekisou combo bonus commands `(time, change)` in filing order.
    #[cfg(feature = "search-diagnostics")]
    pub fn gk_combo_bonus_commands(&self) -> Vec<(i32, f32)> {
        self.gk.as_ref().map(|g| g.ctrl.combo_bonus_commands()).unwrap_or_default()
    }

    /// Diagnostics only: each note's last executed score `(time, note id, score, [Gekisou combo, combo, score-up])`
    /// and the filed fixed scores `(frame, score)`, for comparison with bound terms. Not part of the native result.
    #[cfg(feature = "search-diagnostics")]
    pub fn filed_scores(&self) -> (Vec<scorecalc::FiledNote>, Vec<(i32, i32)>) {
        self.score.filed_scores()
    }

    /// The score factor state.
    pub fn factor_state(&self) -> &ScoreFactorState {
        &self.score.calc.state
    }

    /// The life stored at the end of the last frame.
    pub fn current_life(&self) -> i32 {
        self.life.current_life
    }

    /// `(frame, range)` receipts for external ranking packets, after the range's snapshots are complete.
    pub fn rank_confirmation_applications(&self) -> &[(usize, usize)] {
        self.gk.as_ref().map_or(&[], |g| g.rank_applications.as_slice())
    }

    /// Live combo controller's current value, after this frame's converted results.
    pub fn current_combo(&self) -> i32 {
        self.current_combo
    }

    /// This frame's converted `(note id, judgement, chart time_ms)` results, including unscored Pass events.
    pub fn frame_judgements(&self) -> &[(i32, i32, i32)] {
        &self.judged
    }

    /// Native frame-result score, captured after skill updates and before Gekisou ranking's timestamp queries.
    /// [`LiveModel::score`] remains the calculator's settled value after ranking.
    pub fn frame_score(&self) -> i32 {
        self.frame_score
    }

    /// Number of judgements converted by skills so far.
    pub fn converted_judgements(&self) -> u64 {
        self.conversion.converted
    }

    /// Applied confirmations as (range, rank, awarded bonus, explicit percentage).
    pub fn gekisou_rank_bonuses(&self) -> &[(usize, i32, i32, i64)] {
        self.gk.as_ref().map_or(&[], |g| g.rank_bonus.as_slice())
    }

    /// Rank bonus score counted again by frames executed after score rewind.
    pub fn rank_bonus_score(&self) -> i32 {
        self.score.rank_bonus
    }

    /// The Gekisou ranges (empty without Gekisou).
    pub fn gekisou_ranges(&self) -> Vec<GekisouRange> {
        let Some(gk) = &self.gk else { return Vec::new() };
        let c = &gk.ctrl;
        c.ranges
            .iter()
            .zip(&c.states)
            .enumerate()
            .map(|(i, (r, s))| GekisouRange {
                mission: r.mission,
                state: s.state,
                combo: s.combo,
                max_combo: s.max_combo,
                just_count: s.just,
                start_score: s.start_score,
                end_score: s.end_score,
                luck_points: s.luck.total_bonus_point,
                luck_gauge: s.luck.gauge,
                rush_combo: s.luck.rush_combo,
                lot_results: s.luck.results,
                rank_bonus: gk.rank_bonus.iter().find(|b| b.0 == i).map(|b| b.2),
            })
            .collect()
    }
}
