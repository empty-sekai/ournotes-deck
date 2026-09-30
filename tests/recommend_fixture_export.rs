//! Reproducible, explicitly synthetic inputs for CLI demonstrations and bounded-search benchmarks.
//! Run only on request: BDON_FIXTURE_OUT=<directory> cargo test --test recommend_fixture_export
//! -- --ignored --test-threads=1. BDON_FIXTURE_LARGE=1 also exports an 80-card/60-snap mock.
//! These fixtures are manual model inputs; they are neither OCR truth nor a real seed distribution.

mod common;

use common::{Rng, Synth, extend_table, replace_table, set_column, synth_snaps};
use ournotes_deck::cards::Roster;
use ournotes_deck::data::DeckData;
use ournotes_deck::live::model::{JudgementStream, JustRule};
use ournotes_deck::scenario::{ContextInput, Scenario, item_payoff};
use ournotes_deck::search::expectation::{self, FiniteSeedLaw, OracleOutcome, PhysicalDeck};
use ournotes_deck::search::recommendation::{REQUEST_FORMAT, RecommendationRequest, recommend};
use ournotes_deck::search::{
    Completion, Constraints, GekisouObjective, Objective, PlayInput, Pool, SearchRequest, SeedSet,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const FIXTURE_SEED: u64 = 20_261_001;
const SCORE_ID: i64 = 1004;
const EVENT_ID: i64 = 7;

fn synthetic_master(members: i64, snaps: i64, characters: i64) -> Synth {
    let mut s = synth_snaps(&mut Rng::new(FIXTURE_SEED), members, snaps, &[1, 3, 6, 10, 11]);
    replace_table(
        &mut s,
        "MasterCharacter",
        Value::Array((1..=characters).map(|id| json!({"_id":id,"_bandID":(id-1)%3+1})).collect()),
    );
    set_column(&mut s, "MasterMemberCard", &mut |r| {
        let id = r["_id"].as_i64().unwrap();
        r["_characterID"] = json!((id - 1) % characters + 1);
        r["_cardType"] = json!((id - 1) % 5 + 1);
        r["_liveSkillID"] = json!((id - 1) % 3 + 1);
        r["_leaderSkillID"] = json!(4);
    });
    set_column(&mut s, "MasterSupportCard", &mut |r| {
        let id = r["_id"].as_i64().unwrap();
        r["_characterIDs"] = json!([(id - 1) % characters + 1]);
        r["_supportSkillId01"] = json!(if id % 2 == 1 { 3 } else { 10 });
        r["_supportSkillId02"] = json!(if id % 2 == 1 { 1 } else { 6 });
    });
    replace_table(
        &mut s,
        "MasterLiveMusic",
        json!([{
            "_id":10,"_musicType":1,"_bestMusicTagIDs":[1],"_expertID":SCORE_ID,
            "_liveScoreRankGroup":1,"_gekisouMission1":1,"_gekisouMission2":2,"_gekisouMission3":3
        }]),
    );
    replace_table(
        &mut s,
        "MasterLiveMusicScore",
        json!([{
            "_id":SCORE_ID,"_musicScoreLevel":24,"_fullComboCount":12
        }]),
    );
    replace_table(
        &mut s,
        "MasterChallengeMusic",
        json!([
            {"_id":70,"_eventId":EVENT_ID,"_liveMusicId":10,"_musicType":4,"_bestMusicTagIDs":[2]},
            {"_id":71,"_eventId":EVENT_ID,"_liveMusicId":10,"_musicType":0,"_bestMusicTagIDs":[]}
        ]),
    );
    replace_table(
        &mut s,
        "MasterArenaMusic",
        json!([{
            "_id":80,"_liveMusicId":10,"_liveMusicType":5,
            "_gekisouMission1":3,"_gekisouMission2":2,"_gekisouMission3":1
        }]),
    );
    replace_table(
        &mut s,
        "MasterLiveJudgementTiming",
        json!([
            {"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_beforeMs":40,"_afterMs":40},
            {"_id":2,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_beforeMs":80,"_afterMs":80},
            {"_id":3,"_noteJudgementType":2,"_noteSimulateJudgement":5,"_beforeMs":80,"_afterMs":80}
        ]),
    );
    extend_table(
        &mut s,
        "MasterLiveSettings",
        vec![
            json!({"_id":30,"_key":"gekisou_luck_gauge_max","_value":"40"}),
            json!({"_id":31,"_key":"gekisou_luck_gauge_max_rush","_value":"20"}),
            json!({"_id":32,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}),
        ],
    );
    extend_table(
        &mut s,
        "MasterLiveComboScoreBonus",
        (1..=4).map(|i| json!({"_id":100+i,"_comboBonusType":1,"_requiredComboCount":i,"_bonusFactor":0.02})).collect(),
    );
    replace_table(
        &mut s,
        "MasterLiveGekisouRankingScoreBonus",
        Value::Array(
            (1..=3)
                .flat_map(|pattern| {
                    (1..=3).map(move |count| {
                        json!({"_id":pattern*10+count,"_missionPattern":pattern,
            "_count":count,"_rank":1,"_scoreBonusPercent":10})
                    })
                })
                .collect(),
        ),
    );
    replace_table(&mut s, "MasterLiveGekisouLuckBasePoint", Value::Array((3..=6).map(|judgement| {
        json!({"_id":judgement,"_noteCategory":0,"_noteSimulateJudgement":judgement,"_weight":1,"_basePoint":10})
    }).collect()));
    replace_table(
        &mut s,
        "MasterLiveGekisouLuckBonusLot",
        Value::Array(
            (0..5)
                .flat_map(|kind| {
                    (0..4).map(move |result| {
                        json!({"_id":kind*10+result+1,"_chanceLotType":kind,
            "_lotResult":result,"_weight":([5,4,2,1][result as usize])})
                    })
                })
                .collect(),
        ),
    );
    // Own skill-event trigger, with a real probability checker in the synthetic effect table.
    replace_table(
        &mut s,
        "MasterGekisouSkillEffect",
        json!([{
            "_id":1,"_gekisouSkillID":1,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":53,"_skillConditionGroup":66,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":2000,"_activationTimeSecond":0.4,"_effectValue":900,
            "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0
        }]),
    );
    replace_table(
        &mut s,
        "MasterEvent",
        json!([{
            "_id":EVENT_ID,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2
        }]),
    );
    replace_table(
        &mut s,
        "MasterEventEffect",
        Value::Array(
            (0..=2)
                .map(|kind| {
                    json!({"_id":kind+1,"_eventId":EVENT_ID,"_eventBonusType":kind,"_resourceTypeConstraint":2,
            "_rank1EffectValue":1000,"_rank2EffectValue":1000,"_rank3EffectValue":1000,
            "_rank4EffectValue":1000,"_rank5EffectValue":1000})
                })
                .collect(),
        ),
    );
    replace_table(&mut s, "MasterLiveScoreRank", Value::Array([0,300_000,450_000,600_000].iter().enumerate().map(|(i, score)| {
        json!({"_id":i+1,"_group":1,"_liveScoreRank":i+2,"_requiredScore":score,"_battleLiveRequiredScore":score})
    }).collect()));
    for (table, group, base) in [
        ("MasterLiveEventPoint", 1, 100),
        ("MasterChallengeLiveEventPoint", 2, 300),
        ("MasterLiveChallengePoint", 1, 5),
    ] {
        replace_table(
            &mut s,
            table,
            Value::Array(
                (2..=5)
                    .map(|rank| json!({"_id":rank,"_group":group,"_scoreRank":rank,"_value":base+(rank-2)*base/3}))
                    .collect(),
            ),
        );
    }
    replace_table(
        &mut s,
        "MasterLiveMusicBoostBonus",
        json!([{
            "_id":1,"_consumedLiveBoostCount":1,"_liveMusicRewardRate":2,"_playerExpRate":2,
            "_memberCardExpRate":2,"_friendshipExpRate":2,"_eventPointRate":2
        }]),
    );
    replace_table(
        &mut s,
        "MasterChallengeMusicBoostBonus",
        json!([{
            "_id":1,"_consumedChallengePointCount":201,"_liveMusicRewardRate":2,"_playerExpRate":2,
            "_memberCardExpRate":2,"_friendshipExpRate":2,"_eventPointRate":2
        }]),
    );
    for (table, count) in [("MasterLiveEventReward", 3), ("MasterChallengeLiveEventReward", 4)] {
        replace_table(
            &mut s,
            table,
            json!([{
                "_id":5,"_group":1,"_eventGroup":1,"_scoreRank":2,"_resourceType":11,
                "_resourceId":9,"_resourceCount":count,"_probability":999
            }]),
        );
    }
    replace_table(
        &mut s,
        "MasterEventAchievementReward",
        json!([{
            "_id":1,"_eventId":EVENT_ID,"_eventPoint":100,"_rewardIds":[5]
        }]),
    );
    s
}

fn columns_and_rows(s: &Synth) -> Value {
    let master: BTreeMap<_, _> = s
        .tables
        .iter()
        .map(|(name, rows)| {
            let objects = rows.as_array().expect("synthetic table is an array");
            let columns: Vec<_> = objects
                .iter()
                .flat_map(|r| r.as_object().unwrap().keys().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let data_rows: Vec<Vec<Value>> = objects
                .iter()
                .map(|r| columns.iter().map(|key| r.get(key).cloned().unwrap_or(Value::Null)).collect())
                .collect();
            (name.clone(), json!({"columns":columns,"rows":data_rows}))
        })
        .collect();
    json!(master)
}

fn data_document(s: &Synth, members: i64, snaps: i64, characters: i64) -> Value {
    json!({
        "format":"nnnotes.deck-data/1",
        "provenance":{"synthetic":true,"fixtureSeed":FIXTURE_SEED,"region":"synthetic",
            "masterVersion":"synthetic-recommend-fixture-20261001","clientVersion":"synthetic-inputs",
            "source":"tests/recommend_fixture_export.rs + tests/common::synth_snaps",
            "members":members,"snaps":snaps,"characters":characters,"isOCRTruth":false,
            "assetSha256Policy":"64 zero placeholder; no game asset or real chart is claimed"},
        "master":columns_and_rows(s),
        "charts":[{"scoreId":SCORE_ID,"asset":{"key":"SYNTHETIC-short-chart-1004",
            "sha256":"0000000000000000000000000000000000000000000000000000000000000000"},
            "notes":{"id":(1..=12).collect::<Vec<_>>(),"op":vec![1;12],
                "judgementType":vec![1;12],"timeMs":(1..=12).map(|i|i*100).collect::<Vec<_>>()},
            "skillEvents":{"timeMs":[0,250,500,750,1000]},
            "fevers":{"startMs":[150,450,850],"endMs":[400,800,1150]}}]
    })
}

fn roster_document(members: i64, snaps: i64, characters: i64) -> Value {
    let ranks: BTreeMap<_, _> = (1..=characters).map(|id| (id.to_string(), 10)).collect();
    json!({
        "provenance":{"synthetic":true,"isOCRTruth":false,"source":"manually fixed synthetic progress"},
        "player":{"characterRanks":ranks,"bandItems":{},"vipRank":3,"events":[EVENT_ID],
            "memory":null,"ownedMemberCardIds":(1..=members).collect::<Vec<_>>(),
            "ownedSupportCardIds":(1..=snaps).collect::<Vec<_>>()},
        "members":(1..=members).map(|id| json!({"id":id,"level":40,"exp":null,"awake":2,
            "rank":3,"liveSkillLevel":4,"gekisouSkillLevel":1})).collect::<Vec<_>>(),
        "snaps":(1..=snaps).map(|id| json!({"id":id,"level":30,"exp":null,"rank":3})).collect::<Vec<_>>()
    })
}

fn context_document(skip: bool, multiplayer: bool, expired: bool) -> Value {
    let mut value = json!({
        "powerSnapshot":{"eventIds":[EVENT_ID],"capturedJstTicks":50},
        "resultClock":if skip {json!({"execution":"skip","serverNowJstTicks":if expired {200} else {150}})}
            else {json!({"execution":"played","savedStartJstTicks":150,"serverNowJstTicks":201})},
        "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":EVENT_ID,"points":0,
            "challengePoints":500,"added":[]}],"eventWindows":[{"eventId":EVENT_ID,
            "startJstTicks":100,"endJstTicks":200}],"selectedRewards":[{"eventId":EVENT_ID,"rewardId":5}]}
    });
    if multiplayer {
        value["eventPayoff"]["multiplayerResultPanel"] = json!({"localPlayerIndex":1,
            "localDisconnected":false,"otherPlayers":[{"finalScore":100000,"disconnected":false},
                {"finalScore":50000,"disconnected":true}]});
    }
    value
}

/// Fixed five-character/two-snap inputs, parsed through the production data and roster readers.
pub fn small_fixture() -> (DeckData, Roster) {
    let s = synthetic_master(5, 2, 5);
    let document = data_document(&s, 5, 2, 5);
    let roster_json = roster_document(5, 2, 5);
    (DeckData::from_json(&document.to_string()).unwrap(), Roster::from_json(&roster_json.to_string()).unwrap())
}

/// Synthetic played/skip clocks and optional explicit multiplayer result-panel inputs.
pub fn fixture_context(skip: bool, multiplayer: bool) -> ContextInput {
    serde_json::from_value(context_document(skip, multiplayer, false)).unwrap()
}

/// A caller-chosen finite law, retaining signed roots, repeated roots and unequal masses.
pub fn fixture_law() -> FiniteSeedLaw {
    FiniteSeedLaw::new(vec![(1, 1), (-1, 3), (1, 2), (14, 1)]).unwrap()
}

fn write_json(path: &Path, value: &impl Serialize) {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

fn rerank_payoff(
    score_oracle: &OracleOutcome,
    pool: &Pool,
    context: &ournotes_deck::scenario::ResolvedContext,
    input: &ContextInput,
    items: bool,
) -> OracleOutcome {
    let mut out = score_oracle.clone();
    for row in &mut out.results {
        let mut atoms = row.evaluation.outcomes.clone();
        for atom in &mut atoms {
            let event = input.event_payoff.as_ref().unwrap();
            atom.terminal_payoff = if items {
                item_payoff(
                    &context
                        .preview_event_items(pool, &row.physical.as_deck(), event, EVENT_ID, atom.final_score)
                        .unwrap(),
                    EVENT_ID,
                    11,
                    9,
                )
                .unwrap()
            } else {
                i128::from(
                    context
                        .preview_event_points(pool, &row.physical.as_deck(), event, EVENT_ID, atom.final_score)
                        .unwrap()
                        .points_for(EVENT_ID),
                )
            };
        }
        row.evaluation = expectation::aggregate(atoms).unwrap();
    }
    out.results.sort_by(|a, b| {
        b.evaluation
            .expected_payoff
            .numerator
            .cmp(&a.evaluation.expected_payoff.numerator)
            .then_with(|| b.power.cmp(&a.power))
            .then_with(|| a.members.cmp(&b.members))
            .then_with(|| a.snaps.cmp(&b.snaps))
    });
    out
}

fn export_small(out: &Path) {
    fs::create_dir_all(out).unwrap();
    let s = synthetic_master(5, 2, 5);
    let document = data_document(&s, 5, 2, 5);
    let roster_json = roster_document(5, 2, 5);
    // Validate the same columns/rows document and Roster JSON the real CLI will read.
    let (data, roster) = small_fixture();
    assert_eq!(
        Pool::new(&data.master, &roster).unwrap().members.iter().map(|m| m.character_id).collect::<BTreeSet<_>>().len(),
        5
    );
    write_json(&out.join("DeckData.json"), &document);
    write_json(&out.join("roster.json"), &roster_json);
    let law = fixture_law();
    let atoms = law.atoms().to_vec();
    write_json(&out.join("finite-law.json"), &atoms);
    for (name, skip, multi, expired) in [
        ("context-played.json", false, false, false),
        ("context-multiplayer.json", false, true, false),
        ("context-skip.json", true, false, false),
        ("context-skip-expired.json", true, false, true),
    ] {
        write_json(&out.join(name), &context_document(skip, multi, expired));
    }
    let chart = data.chart(SCORE_ID).unwrap();
    let native_chart = data.data_chart(SCORE_ID).unwrap();
    let mut cases = Vec::new();
    for (name, scenario, id, multi) in [
        ("free", Scenario::Free(10), 10, false),
        ("mission", Scenario::Mission(10), 10, false),
        ("battle", Scenario::Battle(10), 10, true),
        ("arena", Scenario::Arena(80), 80, true),
        ("challenge", Scenario::Challenge(70), 70, false),
        ("challenge-type-zero", Scenario::Challenge(71), 71, false),
    ] {
        let context_file = if multi { "context-multiplayer.json" } else { "context-played.json" };
        let context_input = fixture_context(false, multi);
        let context = context_input.resolve(&data.master, scenario, Some(SCORE_ID), &native_chart.fevers).unwrap();
        let pool = context.pool(&data.master, &roster).unwrap();
        for gekisou in [false, true] {
            let mode = if gekisou { "gekisou" } else { "ordinary" };
            let stream = if gekisou {
                let rule = JustRule::new(&data.master, &context.gekisou).unwrap();
                JudgementStream::theoretical_best_gekisou(&chart, &native_chart.judgement_types, &rule).unwrap()
            } else {
                JudgementStream::theoretical_best(&chart)
            };
            let play_file = format!("play-{name}-{mode}.json");
            write_json(&out.join(&play_file), &stream);
            let objective = Objective::LiveScore {
                score_id: SCORE_ID,
                chart: chart.clone(),
                play: PlayInput::Stream { stream, judgement_types: native_chart.judgement_types.clone() },
                event: false,
                exclude_snap_skills: false,
                gekisou: gekisou
                    .then(|| GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: native_chart.fevers.clone() }),
            }
            .in_scenario(context.clone());
            let request = SearchRequest {
                objective: objective.clone(),
                k: 24,
                constraints: Constraints { leader: Some(1), no_snaps: true, ..Default::default() },
                time_limit: None,
            };
            let scores = expectation::oracle(&pool, &request, &law).unwrap();
            assert_eq!(scores.completion, Completion::Complete);
            assert_eq!(scores.evaluated, 24);
            assert_eq!(scores.results.len(), 24);
            assert!(scores.results.iter().all(|r| r.members[2] == 1 && r.snaps == [None; 5]));
            let physical: BTreeSet<_> = scores.results.iter().map(|r| r.physical).collect();
            assert_eq!(physical.len(), 24);
            let base = format!("oracle-{name}-{mode}");
            write_json(&out.join(format!("{base}-score.json")), &scores);
            let points = rerank_payoff(&scores, &pool, &context, &context_input, false);
            write_json(&out.join(format!("{base}-client-event-points.json")), &points);
            let items = rerank_payoff(&scores, &pool, &context, &context_input, true);
            write_json(&out.join(format!("{base}-conditional-client-event-items.json")), &items);
            // Exercise both paired snaps without expanding the video oracle's 24-deck space.
            let sample = PhysicalDeck { members: [1, 2, 0, 3, 4], snaps: [Some(0), None, None, None, Some(1)] };
            let paired = expectation::evaluate_finite_with_factory(
                &pool,
                &sample,
                &objective,
                &law,
                || Ok(()),
                |_, terminal, _| Ok(i128::from(terminal.final_score)),
            )
            .unwrap();
            write_json(
                &out.join(format!("sample-snaps-{name}-{mode}.json")),
                &json!({
                    "synthetic":true,"members":sample.members.map(|i|pool.members[i].id),
                    "snaps":sample.snaps.map(|s|s.map(|i|pool.snaps[i].id)),"evaluation":paired
                }),
            );
            let cli_scenario = if name == "challenge-type-zero" { "challenge" } else { name };
            let mut args = vec![
                "live".to_string(),
                "--data".into(),
                "DeckData.json".into(),
                "--roster".into(),
                "roster.json".into(),
                "--score".into(),
                SCORE_ID.to_string(),
                "--scenario".into(),
                cli_scenario.into(),
                "--scenario-music".into(),
                id.to_string(),
                "--context".into(),
                context_file.into(),
                "--expectation".into(),
                "finite".into(),
                "--seed-law".into(),
                "finite-law.json".into(),
                "--play".into(),
                play_file,
                "--leader".into(),
                "1".into(),
                "--no-snaps".into(),
                "-k".into(),
                "24".into(),
            ];
            if gekisou {
                args.push("--gekisou".into());
            }
            let native_validity =
                if !gekisou && matches!(scenario, Scenario::Mission(_) | Scenario::Battle(_) | Scenario::Arena(_)) {
                    "invalid-native-choice"
                } else if gekisou && matches!(scenario, Scenario::Battle(_) | Scenario::Arena(_)) {
                    "legacy-solo-rank-counterfactual"
                } else {
                    "native-solo-evidence-declared-scope"
                };
            cases.push(json!({"name":name,"mode":mode,"scenarioMusicId":id,"scoreId":SCORE_ID,
                "nativeValidity":native_validity,"videoEligible":name=="free" && gekisou,
                "program":"ournotes-deck","args":args,"scoreOracle":format!("{base}-score.json"),
                "clientEventPointsOracle":format!("{base}-client-event-points.json"),
                "conditionalItemsOracle":format!("{base}-conditional-client-event-items.json"),
                "eventPointsAdditionalArgs":["--objective","client-event-points","--event-id","7"],
                "conditionalItemsAdditionalArgs":["--objective","conditional-client-event-items","--event-id","7","--resource-type","11","--resource-id","9"]}));
        }
    }
    write_json(
        &out.join("manifest.json"),
        &json!({
            "format":"ournotes-deck.synthetic-recommend-fixtures/1","synthetic":true,"isOCRTruth":false,
            "fixtureSeed":FIXTURE_SEED,"scope":"synthetic offline CLI/model inputs; no real player or native population law",
            "pool":{"members":5,"snaps":2,"characters":5},"leader":1,"videoConstraints":{"leader":1,"noSnaps":true},
            "oraclePhysicalDeckCount":24,"rootLawAtoms":atoms,"totalWeight":law.total_weight(),
            "playPolicy":"theoretical best 60 fps; ordinary Perfect and chart-enabled Gekisou Just; conditional theory result",
            "payoffMethod":"event/item payoffs recomputed for every exactly simulated law atom, then reranked with oracle comparator",
            "conditionalRewardAssumption":{"eventId":7,"serverSelectedRewardId":5,"resourceType":11,"resourceId":9},
            "clockQualification":"synthetic normalized ticks; played start150 survives serverNow201, event window[100,200)",
            "multiplayerQualification":"Battle/Arena Gekisou uses the legacy Solo rank simulator and is counterfactual; explicit peer reward panel does not make the played simulation native Network",
            "nativeChoiceQualification":"ordinary Mission/Battle/Arena are invalid native choices even though the legacy CLI accepts them",
            "videoCase":"free-gekisou","videoOracle":"oracle-free-gekisou-score.json",
            "jsonNumberPolicy":"arbitrary-precision integers required; fractions remain numerator/denominator",
            "cliWorkingDirectory":"directory containing this manifest","cases":cases
        }),
    );
    fs::write(out.join("README.txt"), "SYNTHETIC fixtures; manually generated, never OCR ground truth.\n\
DeckData.json uses the real nnnotes.deck-data/1 columns/rows reader. roster.json uses the real Roster reader.\n\
manifest.json contains exact existing ournotes-deck CLI argument arrays and all 24 fixed-leader/no-snap oracle results.\n\
Run from this directory: cargo run --release -j 1 --manifest-path <solver Cargo.toml> --bin ournotes-deck -- <args from manifest>\n\
Expected final-score fractions are conditional on finite-law.json and the stated theoretical play.\n\
Legacy CLI acceptance is not evidence that a native mode is selectable: ordinary Mission/Battle/Arena are invalid native choices.\n\
Battle/Arena Gekisou oracle files use legacy Solo ranking and are counterfactual, not native Network oracles.\n\
Only Free Gekisou is selected for the video comparison. Check nativeValidity on every manifest case.\n\
Conditional item files assume explicit server selection of reward5; they do not predict a server drop lottery.\n\
The 64-zero chart asset hash is an explicit placeholder, not evidence of any game asset.\n").unwrap();
}

/// Seven different characters and three paired support cards for real member-selection tests.
pub fn choice7_fixture() -> (DeckData, Roster) {
    let s = synthetic_master(7, 3, 7);
    (
        DeckData::from_json(&data_document(&s, 7, 3, 7).to_string()).unwrap(),
        Roster::from_json(&roster_document(7, 3, 7).to_string()).unwrap(),
    )
}

fn power_breakdown(
    pool: &Pool,
    music: &ournotes_deck::cards::SongView,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
) -> Value {
    let deck = pool.deck(members, snaps, [0, 1, 2, 3, 4]).unwrap();
    let power = pool.deck_power(&deck, Some(music), false).unwrap();
    let names = [
        "basePower",
        "characterRank",
        "characterTotalRank",
        "support",
        "bandItem",
        "typeLink",
        "musicType",
        "musicTag",
        "leaderSkill",
        "memory",
        "vip",
        "pctSupport",
        "pctTypeLink",
        "pctMusicType",
        "pctMusicTag",
        "total",
    ];
    let bp = |value: ournotes_deck::power::CardPower| {
        json!({"performance":value.performance.to_string(),
        "technique":value.technique.to_string(),"visual":value.visual.to_string()})
    };
    let slots: Vec<_> = power
        .slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let values = slot.fields();
            let sum = values[..11].iter().fold(ournotes_deck::power::CardPower::EMPTY, |sum, value| sum.add(*value));
            assert_eq!(sum, slot.total, "additive power terms must reproduce the exact slot");
            let fields: BTreeMap<_, _> = names.iter().zip(values).map(|(name, value)| (*name, bp(value))).collect();
            json!({"slot":i,"isLeader":i==2,"memberId":members[i],"snapId":snaps[i],"termsBP":fields})
        })
        .collect();
    json!({"synthetic":true,"power":power.power(),"unit":"10000 BP per stat point; percentage terms: 10000 BP = 100 percent",
        "statOrder":["performance","technique","visual"],"additiveTerms":&names[..11],
        "allSlotTermsSumExactly":true,"totalBP":bp(power.total),"slots":slots})
}

fn production_request(execution: Value, metric: Value, no_snaps: bool, include: &[i64]) -> Value {
    let played = execution["kind"] == "live";
    json!({"format":REQUEST_FORMAT,"execution":execution,"scenario":{"kind":"free","musicId":10},
        "context":context_document(execution["kind"]=="skip",false,false),"metric":metric,
        "seedLaw":if played {json!({"atoms":fixture_law().atoms(),
            "provenance":"synthetic finite roots chosen by fixture; unequal masses and duplicate root retained; not a TickCount population estimate"})} else {Value::Null},
        "constraints":{"leader":1,"noSnaps":no_snaps,"includeMembers":include},"k":5,
        "strategy":{"kind":"exhaustive"},"limits":{"timeLimitMs":null,"maxCandidates":null,"cacheEntries":512}})
}

fn export_choice7(out: &Path) {
    let out = out.join("choice7");
    fs::create_dir_all(&out).unwrap();
    let s = synthetic_master(7, 3, 7);
    let (data, roster) = choice7_fixture();
    write_json(&out.join("DeckData.json"), &data_document(&s, 7, 3, 7));
    write_json(&out.join("roster.json"), &roster_document(7, 3, 7));
    let law = fixture_law();
    write_json(&out.join("finite-law.json"), &law.atoms());
    write_json(&out.join("context-played.json"), &context_document(false, false, false));
    write_json(&out.join("context-skip.json"), &context_document(true, false, false));
    let chart = data.chart(SCORE_ID).unwrap();
    let dc = data.data_chart(SCORE_ID).unwrap();
    let context =
        fixture_context(false, false).resolve(&data.master, Scenario::Free(10), Some(SCORE_ID), &dc.fevers).unwrap();
    let pool = context.pool(&data.master, &roster).unwrap();
    assert_eq!(pool.members.iter().map(|m| m.character_id).collect::<BTreeSet<_>>().len(), 7);
    let mut cases = Vec::new();
    let mut summaries = Vec::new();
    for (name, gekisou, no_snaps, include, expected_count) in [
        ("ordinary", false, true, vec![], 360),
        ("gekisou", true, true, vec![], 360),
        ("gekisou-member-and-snap-choice", true, false, vec![1, 2, 3, 4], 9792),
    ] {
        eprintln!("choice7 {name}: exhaustive oracle starts ({expected_count} physical decks, 4 law atoms)");
        let start = std::time::Instant::now();
        let stream = if gekisou {
            JudgementStream::theoretical_best_gekisou(
                &chart,
                &dc.judgement_types,
                &JustRule::new(&data.master, &context.gekisou).unwrap(),
            )
            .unwrap()
        } else {
            JudgementStream::theoretical_best(&chart)
        };
        write_json(&out.join(format!("play-{name}.json")), &stream);
        let objective = Objective::LiveScore {
            score_id: SCORE_ID,
            chart: chart.clone(),
            play: PlayInput::Stream { stream, judgement_types: dc.judgement_types.clone() },
            event: false,
            exclude_snap_skills: false,
            gekisou: gekisou.then(|| GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: dc.fevers.clone() }),
        }
        .in_scenario(context.clone());
        let request = SearchRequest {
            objective,
            k: 5,
            constraints: Constraints {
                leader: Some(1),
                no_snaps,
                include_members: include.clone(),
                ..Default::default()
            },
            time_limit: None,
        };
        let oracle = expectation::oracle(&pool, &request, &law).unwrap();
        assert_eq!(oracle.completion, Completion::Complete);
        assert_eq!(oracle.evaluated, expected_count);
        let oracle_ms = start.elapsed().as_secs_f64() * 1000.0;
        write_json(&out.join(format!("oracle-{name}-score.json")), &oracle);
        let request_json = production_request(
            json!({"kind":"live","scoreId":SCORE_ID,"gekisou":gekisou,
            "play":{"kind":"theoreticalBest"}}),
            json!({"kind":"score"}),
            no_snaps,
            &include,
        );
        write_json(&out.join(format!("request-{name}.json")), &request_json);
        let typed: RecommendationRequest = serde_json::from_value(request_json).unwrap();
        let result = recommend(&data, &roster, &typed).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.results.len(), oracle.results.len());
        for (got, want) in result.results.iter().zip(&oracle.results) {
            assert_eq!((got.members, got.snaps, got.power), (want.members, want.snaps, want.power));
            let exact = got.expected_score.as_ref().unwrap();
            assert_eq!(exact.numerator, want.evaluation.expected_score.numerator.to_string());
            assert_eq!(exact.denominator, law.total_weight().to_string());
            assert_eq!(got.atoms.len(), law.atoms().len());
            for (got, want) in got.atoms.iter().zip(&want.evaluation.outcomes) {
                assert_eq!(
                    (got.root_seed, got.performance_order, got.score),
                    (want.root_seed, want.performance_order, want.final_score)
                );
                assert_eq!(got.weight, want.weight.to_string());
                assert_eq!(got.payoff, want.terminal_payoff.to_string());
            }
        }
        write_json(&out.join(format!("result-{name}.json")), &result);
        let best = &oracle.results[0];
        let breakdown = power_breakdown(&pool, &context.resolved.power_music, best.members, best.snaps);
        assert_eq!(breakdown["power"], best.power);
        write_json(&out.join(format!("power-breakdown-{name}.json")), &breakdown);
        let selected: BTreeSet<_> = best.members.into_iter().collect();
        summaries.push(json!({"case":name,"synthetic":true,"nativeValidity":"native-solo-evidence-declared-scope",
            "constraints":{"leader":1,"noSnaps":no_snaps,"includeMembers":include},
            "bestMembers":best.members,"bestSnaps":best.snaps,"selectedMemberSet":selected,
            "notSelectedMembers":(1..=7).filter(|id|!selected.contains(id)).collect::<Vec<_>>(),
            "physicalCandidates":oracle.evaluated,"completion":oracle.completion,
            "expectedScore":best.evaluation.expected_score,"atoms":best.evaluation.outcomes,
            "oracleElapsedMs":oracle_ms,"productionElapsedMs":result.elapsed_ms,
            "productionOracleTop5ExactlyEqual":true,"powerBreakdown":format!("power-breakdown-{name}.json")}));
        cases.push(json!({"name":name,"nativeValidity":"native-solo-evidence-declared-scope",
            "videoEligible":name=="gekisou-member-and-snap-choice","request":format!("request-{name}.json"),
            "result":format!("result-{name}.json"),"oracle":format!("oracle-{name}-score.json"),
            "physicalCandidates":oracle.evaluated,"lawAtoms":4,"denominator":law.total_weight().to_string(),
            "constraints":{"leader":1,"noSnaps":no_snaps,"includeMembers":include}}));
        eprintln!(
            "choice7 {name}: oracle {oracle_ms:.1} ms; production {:.1} ms; complete Top5 identical",
            result.elapsed_ms
        );
    }
    for (name, skip) in [("power", false), ("skip", true)] {
        let input = fixture_context(skip, false);
        let ctx = input
            .resolve(&data.master, Scenario::Free(10), skip.then_some(SCORE_ID), if skip { &dc.fevers } else { &[] })
            .unwrap();
        let pool = ctx.pool(&data.master, &roster).unwrap();
        let objective = if skip {
            Objective::SkipScore { score_id: SCORE_ID, chart: chart.clone() }
        } else {
            Objective::Power { music_id: Some(10), event: false }
        };
        let request = SearchRequest {
            objective: objective.in_scenario(ctx.clone()),
            k: 5,
            constraints: Constraints { leader: Some(1), ..Default::default() },
            time_limit: None,
        };
        let exact = ournotes_deck::search::search(&pool, &request).unwrap();
        let (oracle, evaluated) = ournotes_deck::search::oracle::brute_force(&pool, &request).unwrap();
        assert_eq!(exact.results, oracle);
        write_json(
            &out.join(format!("oracle-{name}.json")),
            &json!({"synthetic":true,"nativeValidity":"native-solo-evidence-declared-scope",
            "completion":exact.completion,"canonicalMemberSets":15,"oracleEvaluated":evaluated,"results":oracle}),
        );
        let request_json = production_request(
            if skip {
                json!({"kind":"skip","scoreId":SCORE_ID})
            } else {
                json!({"kind":"power","musicId":10,"eventParameter":false})
            },
            if skip { json!({"kind":"score"}) } else { json!({"kind":"power"}) },
            false,
            &[],
        );
        write_json(&out.join(format!("request-{name}.json")), &request_json);
        let typed: RecommendationRequest = serde_json::from_value(request_json).unwrap();
        let result = recommend(&data, &roster, &typed).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        for (got, want) in result.results.iter().zip(&exact.results) {
            assert_eq!((got.members, got.snaps, got.power), (want.members, want.snaps, want.power));
        }
        write_json(&out.join(format!("result-{name}.json")), &result);
        let best = &result.results[0];
        write_json(
            &out.join(format!("power-breakdown-{name}.json")),
            &power_breakdown(&pool, &ctx.resolved.power_music, best.members, best.snaps),
        );
        cases.push(json!({"name":name,"nativeValidity":"native-solo-evidence-declared-scope","videoEligible":false,
            "request":format!("request-{name}.json"),"result":format!("result-{name}.json"),"oracle":format!("oracle-{name}.json"),
            "oracleEvaluated":evaluated,"constraints":{"leader":1,"noSnaps":false},
            "resultIdentity":"one canonical representative per member set"}));
    }
    write_json(
        &out.join("video-summary.json"),
        &json!({"synthetic":true,"isOCRTruth":false,
        "videoCase":"gekisou-member-and-snap-choice","seedLawAtoms":law.atoms(),"denominator":law.total_weight().to_string(),
        "selectionScope":"leader1 and members1,2,3,4 required; choose member5,6 or7 and optimize all three snap injections",
        "playPolicy":"theoretical best; conditional finite-law result, not observed player accuracy or real seed population",
        "cases":summaries}),
    );
    write_json(
        &out.join("manifest.json"),
        &json!({"format":"ournotes-deck.synthetic-choice-fixture/1",
        "synthetic":true,"isOCRTruth":false,"fixtureSeed":FIXTURE_SEED,"members":7,"characters":7,"snaps":3,
        "scoreId":SCORE_ID,"scenario":{"kind":"free","musicId":10},"nativeValidity":"native-solo-evidence-declared-scope",
        "totalWeight":law.total_weight().to_string(),"rootLawAtoms":law.atoms(),
        "fullUnconstrainedSnapPhysicalSpace":48960,"videoConstrainedPhysicalSpace":9792,
        "videoCase":"gekisou-member-and-snap-choice","oracleTopK":5,"productionOracleMatch":true,
        "cases":cases,"cli":{"program":"ournotes-recommend","args":["--data","DeckData.json","--roster","roster.json",
            "--request","request-gekisou-member-and-snap-choice.json","-o","cli-video-result.json"]}}),
    );
    fs::write(out.join("README.txt"),"SYNTHETIC seven-character/three-snap member and snap selection fixture. Never OCR ground truth.\n\
Video: Free Gekisou, leader1 and members1/2/3/4 required; choose fifth member5/6/7 and all distinct snap placements.\n\
Exactly 9792 physical decks (3 fifth-member choices x24 nonleader permutations x136 snap injections), Top5 retained.\n\
Finite law [[1,1],[-1,3],[1,2],[14,1]] has denominator7, includes signed roots and a duplicate.\n\
No candidate or time caps; production Complete Top5 is compared atom-by-atom to the exhaustive oracle.\n\
Ordinary/other Gekisou no-snap files additionally choose four of six nonleaders across 360 physical decks.\n\
Power/skip permit all three snaps and compare exact canonical member-set search to 2040 enumerated assignments.\n\
Power breakdown contains each exact fixed-point stat term (10000 BP per point), paired members/snaps and totals.\n\
Theoretical play and finite roots are declared assumptions; these results do not claim real player accuracy or seed population law.\n").unwrap();
}

#[test]
#[ignore = "exports complete small member-and-snap choice demonstration inputs and oracle"]
fn export_choice7_recommendation_fixture() {
    let directory = std::env::var_os("BDON_FIXTURE_OUT").expect("set BDON_FIXTURE_OUT to an output directory");
    let out = Path::new(&directory);
    export_small(out);
    export_choice7(out);
    eprintln!("choice7 complete fixtures exported to {}", out.join("choice7").display());
}

#[test]
#[ignore = "writes explicitly requested synthetic CLI/performance fixtures"]
fn export_recommendation_fixtures() {
    let directory = std::env::var_os("BDON_FIXTURE_OUT").expect("set BDON_FIXTURE_OUT to an output directory");
    let out = Path::new(&directory);
    export_small(out);
    if std::env::var("BDON_FIXTURE_LARGE").as_deref() == Ok("1") {
        let large = out.join("large");
        fs::create_dir_all(&large).unwrap();
        let s = synthetic_master(80, 60, 40);
        let data_json = data_document(&s, 80, 60, 40);
        let roster_json = roster_document(80, 60, 40);
        let data = DeckData::from_json(&data_json.to_string()).unwrap();
        let roster = Roster::from_json(&roster_json.to_string()).unwrap();
        let pool = Pool::new(&data.master, &roster).unwrap();
        assert_eq!((pool.members.len(), pool.snaps.len()), (80, 60));
        assert_eq!(pool.members.iter().map(|m| m.character_id).collect::<BTreeSet<_>>().len(), 40);
        write_json(&large.join("DeckData.json"), &data_json);
        write_json(&large.join("roster.json"), &roster_json);
        write_json(
            &large.join("manifest.json"),
            &json!({"format":"ournotes-deck.synthetic-perf-mock/1",
            "synthetic":true,"isOCRTruth":false,"members":80,"snaps":60,"characters":40,
            "fixtureSeed":FIXTURE_SEED,"oracleRun":false,"scoreId":SCORE_ID,
            "finiteLaw":"../finite-law.json","playedContext":"../context-played.json",
            "scope":"large synthetic pool, same short chart; bounded-search throughput only, no real chart timing claim"}),
        );
    }
    eprintln!("synthetic recommendation fixtures exported to {}", out.display());
}
