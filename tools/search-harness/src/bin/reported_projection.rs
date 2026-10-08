//! Strict, complete-field original-roster to owned-snapshot audit. No search or live replay runs.
use ournotes_search::{
    handler,
    owned_snapshot::{GoalDependencies, OwnedSnapshot},
    types::RecommendationRequest,
};
use ournotes_sim::{
    calc::PowerCalculator,
    cards::{MemberView, OwnedMember, OwnedSnap, Player, Roster, SnapView},
    data::DeckData,
    memory::MemoryState,
};
use serde_json::{Value, json};
use std::{env, fs};

// Exhaustive patterns make added model fields a compile error until the audit includes them.
fn owned_member(value: &OwnedMember) -> Value {
    let OwnedMember { id, level, exp, awake, rank, live_skill_level, gekisou_skill_level } = value;
    json!({"id":id, "level":level, "exp":exp, "awake":awake, "rank":rank, "live_skill_level":live_skill_level, "gekisou_skill_level":gekisou_skill_level})
}

fn owned_snap(value: &OwnedSnap) -> Value {
    let OwnedSnap { id, level, exp, rank } = value;
    json!({"id":id, "level":level, "exp":exp, "rank":rank})
}

fn member_view(value: &MemberView) -> Value {
    let MemberView {
        id,
        character_id,
        band_id,
        card_type,
        rarity,
        level,
        awake,
        rank,
        rank_group,
        best_music_tag_ids,
        leader_skill_id,
        leader_skill_level,
        live_skill_id,
        live_skill_level,
        live_skill_categories,
        gekisou_skill_id,
        gekisou_skill_level,
        gekisou_skill_categories,
        gekisou_mission_type,
        power,
        character_rank,
        character_total_rank,
    } = value;
    json!({"id":id, "character_id":character_id, "band_id":band_id, "card_type":card_type, "rarity":rarity, "level":level, "awake":awake, "rank":rank, "rank_group":rank_group, "best_music_tag_ids":best_music_tag_ids, "leader_skill_id":leader_skill_id, "leader_skill_level":leader_skill_level, "live_skill_id":live_skill_id, "live_skill_level":live_skill_level, "live_skill_categories":live_skill_categories, "gekisou_skill_id":gekisou_skill_id, "gekisou_skill_level":gekisou_skill_level, "gekisou_skill_categories":gekisou_skill_categories, "gekisou_mission_type":gekisou_mission_type, "power":power.to_array(), "character_rank":character_rank, "character_total_rank":character_total_rank})
}

fn snap_view(value: &SnapView) -> Value {
    let SnapView {
        id,
        card_type,
        rarity,
        level,
        rank,
        rank_group,
        character_ids,
        character_band_ids,
        power_bonus_percent,
        support_skill_ids,
        support_skill_levels,
        gekisou_support_skill_ids,
        gekisou_support_skill_levels,
    } = value;
    json!({"id":id, "card_type":card_type, "rarity":rarity, "level":level, "rank":rank, "rank_group":rank_group, "character_ids":character_ids, "character_band_ids":character_band_ids, "power_bonus_percent":power_bonus_percent.to_array(), "support_skill_ids":support_skill_ids, "support_skill_levels":support_skill_levels, "gekisou_support_skill_ids":gekisou_support_skill_ids, "gekisou_support_skill_levels":gekisou_support_skill_levels})
}

fn player(value: &Player) -> Value {
    let Player {
        character_ranks,
        explicit_character_total_rank,
        band_items,
        vip_rank,
        events,
        memory,
        owned_member_card_ids,
        owned_support_card_ids,
    } = value;
    json!({"character_ranks":character_ranks, "explicit_character_total_rank":explicit_character_total_rank.unwrap_or_else(|| character_ranks.values().fold(0i64, |a, &b| a.wrapping_add(b))), "band_items":band_items, "vip_rank":vip_rank, "events":events, "memory":memory.as_ref().map(memory_state), "owned_member_card_ids":owned_member_card_ids, "owned_support_card_ids":owned_support_card_ids})
}

fn memory_state(value: &MemoryState) -> Value {
    let MemoryState { music_ranks, music_groups, unlocked_members, unlocked_supports } = value;
    json!({"musicRanks": music_ranks, "musicGroups": music_groups,
        "unlockedMembers": unlocked_members, "unlockedSupports": unlocked_supports})
}
fn roster(value: &Roster) -> Value {
    let Roster { player: facts, members, snaps } = value;
    json!({"player": player(facts), "members": members.iter().map(owned_member).collect::<Vec<_>>(),
        "snaps": snaps.iter().map(owned_snap).collect::<Vec<_>>()})
}

fn calculator(value: &PowerCalculator) -> Value {
    let PowerCalculator {
        music_type_base,
        music_tag_base,
        type_link_base,
        character_rank_bonus,
        character_total_rank_bonus,
        member_rank,
        support_rank,
    } = value;
    // Tuple-key maps need an ordered row representation, rather than JSON object keys.
    let mut member_rank: Vec<_> = member_rank.iter().collect();
    let mut support_rank: Vec<_> = support_rank.iter().collect();
    member_rank.sort_unstable();
    support_rank.sort_unstable();
    json!({"musicTypeBase":music_type_base,"musicTagBase":music_tag_base,"typeLinkBase":type_link_base,
        "characterRankBonus":character_rank_bonus,"characterTotalRankBonus":character_total_rank_bonus,
        "memberRank":member_rank,"supportRank":support_rank})
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("reported_projection DATA ORIGINAL_ROSTER SNAPSHOT ORIGINAL_REQUEST OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let original = Roster::from_json(&fs::read_to_string(&args[1])?)?;
    let snapshot = OwnedSnapshot::from_json(&fs::read_to_string(&args[2])?)?;
    let request_text = fs::read_to_string(&args[3])?;
    let request: RecommendationRequest = serde_json::from_str(&request_text)?;
    let wire: Value = serde_json::from_str(&request_text)?;
    assert_eq!(wire["constraints"], json!({}));
    assert_eq!(wire["k"], 5);
    assert_eq!(wire["limits"], json!({"cacheEntries":2048,"maxCandidates":null,"timeLimitMs":60000}));
    let resolution = snapshot.resolve_data(
        &data,
        data.sha256.as_deref().unwrap_or_default(),
        GoalDependencies::of(&request.execution),
    );
    if !resolution.errors.is_empty() || !resolution.missing.is_empty() {
        return Err(serde_json::to_string(&json!({"errors":resolution.errors,"missing":resolution.missing}))?.into());
    }
    let resolved = resolution.resolved.ok_or("strict owned projection did not resolve")?;
    let projected = resolved.diagnostic_projection();
    // This compares each parsed field; only None-total versus its identical derived Some-total is normalized.
    assert_eq!(roster(&original), roster(projected));
    let original_problem = handler::build_card_pool(&data, &original, &request)?;
    let projected_problem = handler::build_card_pool(&data, projected, &request)?;
    let a = original_problem.pool();
    let b = projected_problem.pool();
    assert!(std::ptr::eq(a.master, b.master));
    assert_eq!(player(&a.player), player(&b.player));
    assert_eq!(
        a.members.iter().map(member_view).collect::<Vec<_>>(),
        b.members.iter().map(member_view).collect::<Vec<_>>()
    );
    assert_eq!(a.snaps.iter().map(snap_view).collect::<Vec<_>>(), b.snaps.iter().map(snap_view).collect::<Vec<_>>());
    assert_eq!(a.members.len(), 15);
    assert_eq!(a.snaps.len(), 35);
    assert_eq!(a.power.vip_bonus, b.power.vip_bonus);
    assert!(std::ptr::eq(a.power.master, b.power.master));
    assert_eq!(calculator(&a.power.calc), calculator(&b.power.calc));
    // Calculator tables are derived from this same immutable Master. Item maps are derived from the
    // equal Player; also compare their effective bonus for every member in the complete candidate pool.
    for (left, right) in a.members.iter().zip(&b.members) {
        assert_eq!(a.power.band_items.bonus(left), b.power.band_items.bonus(right));
    }
    for (left, right) in a.snaps.iter().zip(&b.snaps) {
        assert_eq!(left.support_skills()?, right.support_skills()?);
        assert_eq!(left.gekisou_support_skills()?, right.gekisou_support_skills()?);
    }
    let (left, right) = (original_problem.domain(), projected_problem.domain());
    assert_eq!(left.members(), right.members());
    assert_eq!(left.snaps(), right.snaps());
    assert_eq!(left.required(), right.required());
    assert_eq!(left.leader(), right.leader());
    assert_eq!(left.is_feasible(), right.is_feasible());
    assert!(left.is_feasible() && left.required().is_empty() && left.leader().is_none());
    assert_eq!(left.members().len(), 15);
    assert_eq!(left.snaps().len(), 35);
    assert_eq!(original_problem.context().resolved_context(), projected_problem.context().resolved_context());
    assert_eq!(original_problem.context().route(), projected_problem.context().route());
    let member_ids: Vec<_> = a.members.iter().map(|m| m.id).collect();
    let snap_ids: Vec<_> = a.snaps.iter().map(|s| s.id).collect();
    let leader_ids: Vec<_> = left.members().iter().map(|&i| a.members[i].id).collect();
    let answer = json!({"format":"ournotes-deck.reported-projection/1","datasetId":data.sha256,
        "strictResolution":true,"parsedRosterEqual":true,"poolFieldsEqual":true,"candidateDomainEqual":true,
        "memberIds":member_ids,"snapIds":snap_ids,"leaderIds":leader_ids,
        "characterTotalRank":a.player.character_total_rank(),"solverExecuted":false,
        "totalRankNormalization":"legacy absent total and strict derived total use the identical complete-map sum"});
    fs::write(&args[4], serde_json::to_vec_pretty(&answer)?)?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
