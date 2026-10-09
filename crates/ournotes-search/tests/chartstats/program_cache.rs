//! Native cache regressions: current documents agree byte for byte with independent measurements while
//! unchanged compiled programs remain reusable when the public kind and shape catalogs grow.

use super::*;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chartstats::{ChartStatsCache, ChartStatsCacheStats, Options};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        Self(std::env::temp_dir().join(format!(
            "ournotes-chart-cache-integration-{}-{time}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cached(data: &DeckData, path: &Path) -> (Value, ChartStatsCacheStats) {
    // A new instance for every build proves that reuse comes from committed records on disk.
    let cache = ChartStatsCache::new(path).unwrap();
    let doc = chartstats::document_with_cache(data, &Options::default(), &cache).unwrap();
    (doc, cache.snapshot())
}

fn assert_document_bytes(actual: &Value, expected: &Value) {
    assert_eq!(serde_json::to_vec(actual).unwrap(), serde_json::to_vec(expected).unwrap());
}

fn incremental(before: &DeckData, after: &DeckData) -> (Value, Value, ChartStatsCacheStats) {
    let directory = Directory::new();
    let (old, _) = cached(before, &directory.0);
    let (current, reused) = cached(after, &directory.0);
    let independent = chartstats::document_with(after, &Options::default()).unwrap();
    assert_document_bytes(&current, &independent);
    let cold_directory = Directory::new();
    let (cold_doc, cold) = cached(after, &cold_directory.0);
    assert_document_bytes(&current, &cold_doc);
    assert!(reused.hits > 0, "unchanged programs must be reused: {reused:?}");
    assert!(reused.computed > 0, "changed programs must be measured: {reused:?}");
    assert!(reused.computed < cold.computed, "incremental {reused:?}, cold {cold:?}");
    assert_eq!(reused.invalid, 0);
    (old, current, reused)
}

#[test]
fn metadata_and_growth_changes_reuse_all_programs_with_current_headers() {
    let before = aptitude_data(&APT_FEVERS);
    let directory = Directory::new();
    let (old, first) = cached(&before, &directory.0);
    assert!(first.computed > 0);
    let (warm, second) = cached(&before, &directory.0);
    assert_document_bytes(&warm, &old);
    assert_eq!(second.computed, 0);
    assert_eq!(second.writes, 0);

    let mut after = before.clone();
    after.provenance["master"]["version"] = json!("v2");
    after.provenance["exporter"]["version"] = json!("1");
    after.sha256 = Some("1".repeat(64));
    for row in &mut after.master.member_card_levels {
        row.performance_rate += 17;
        row.exp += 1;
    }
    for row in &mut after.master.support_card_levels {
        row.visual_rate += 19;
    }
    let (current, reused) = cached(&after, &directory.0);
    assert_eq!(reused.computed, 0, "{reused:?}");
    assert_eq!(reused.writes, 0);
    assert_eq!(reused.invalid, 0);
    assert!(reused.hits > 0);
    assert_eq!(current["source"]["master"]["version"], "v2");
    assert_eq!(current["source"]["exporter"]["version"], "1");
    assert_eq!(current["charts"], old["charts"]);
    assert_document_bytes(&current, &chartstats::document_with(&after, &Options::default()).unwrap());
}

#[test]
fn adding_an_ordinary_kind_preserves_previous_programs() {
    let before = aptitude_data(&APT_FEVERS);
    let mut after = before.clone();
    let mut row = after.master.live_skill_effects[0].clone();
    row.id = after.master.live_skill_effects.iter().map(|r| r.id).max().unwrap() + 100;
    row.live_skill_id = 90_001;
    row.activation_time_second += 0.125;
    after.master.live_skill_effects.push(row);
    after.master.reindex().unwrap();
    assert_eq!(chartstats::kinds(&after.master).len(), chartstats::kinds(&before.master).len() + 1);

    let (old, current, _) = incremental(&before, &after);
    let old_weights = old["charts"][0]["expectation"]["weights"].as_array().unwrap();
    let current_weights = current["charts"][0]["expectation"]["weights"].as_array().unwrap();
    assert_eq!(&current_weights[..old_weights.len()], old_weights);
    assert_eq!(current["gekisouAptitude"], old["gekisouAptitude"]);
}

#[test]
fn changing_values_reuses_unit_weights_and_regenerates_checks() {
    let before = aptitude_data(&APT_FEVERS);
    let mut after = before.clone();
    for row in &mut after.master.live_skill_effects {
        row.effect_value += 137;
    }
    let (old, current, _) = incremental(&before, &after);
    assert_eq!(chartstats::kinds(&before.master).len(), chartstats::kinds(&after.master).len());
    assert_ne!(current["kinds"], old["kinds"]);
    assert_eq!(current["charts"][0]["expectation"]["weights"], old["charts"][0]["expectation"]["weights"]);
    assert_ne!(current["charts"][0]["expectation"]["check"], old["charts"][0]["expectation"]["check"]);
}

#[test]
fn adding_a_member_shape_reuses_support_measurements_after_their_indices_move() {
    let before = aptitude_data(&APT_FEVERS);
    let old_shapes = chartstats::shapes(&before.master);
    let old_support = shape_for(&old_shapes, "support", 1);
    let mut after = before.clone();
    let mut skill = after.master.gekisou_skills.iter().find(|r| r.id == 1).unwrap().clone();
    skill.id = 90_001;
    after.master.gekisou_skills.push(skill);
    let mut effect =
        after.master.gekisou_skill_effects.iter().find(|r| r.skill_id == 1 && r.level == 3).unwrap().clone();
    effect.id = after.master.gekisou_skill_effects.iter().map(|r| r.id).max().unwrap() + 100;
    effect.skill_id = 90_001;
    effect.effect_value += 2;
    after.master.gekisou_skill_effects.push(effect);
    // Another card still exposes the original skill of this duplicate card.
    after.master.member_cards.last_mut().unwrap().gekisou_skill_id = 90_001;
    after.master.reindex().unwrap();
    let current_shapes = chartstats::shapes(&after.master);
    let current_support = shape_for(&current_shapes, "support", 1);
    assert_eq!(current_shapes.len(), old_shapes.len() + 1);
    assert_eq!(current_support, old_support + 1);

    let (old, current, _) = incremental(&before, &after);
    let variants = |doc: &Value, id: usize| {
        doc["charts"][0]["gekisouAptitude"]["variants"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["shape"] == id)
            .cloned()
            .collect::<Vec<_>>()
    };
    let old_variants = variants(&old, old_support);
    let current_variants = variants(&current, current_support);
    assert_eq!(old_variants.len(), 2);
    assert_eq!(current_variants.len(), 2);
    assert!(old_variants.iter().zip(&current_variants).any(|(a, b)| a["check"] != b["check"]));
    for (mut a, mut b) in old_variants.into_iter().zip(current_variants) {
        for key in ["shape", "check"] {
            a.as_object_mut().unwrap().remove(key);
            b.as_object_mut().unwrap().remove(key);
        }
        assert_document_bytes(&a, &b);
    }
}
