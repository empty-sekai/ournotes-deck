use ournotes_sim::cards::SongView;
use ournotes_sim::master::Master;
use ournotes_sim::scenario::Scenario;
use serde_json::json;

fn master() -> Master {
    let tables = json!({
        "MasterLiveMusic": [{"_id": 10, "_musicType": 2, "_bestMusicTagIDs": [7, 9],
            "_gekisouMission1": 1, "_gekisouMission2": 2, "_gekisouMission3": 3}],
        "MasterChallengeMusic": [
            {"_id": 20, "_liveMusicId": 10, "_musicType": 4,
             "_gekisouMission1": 3, "_gekisouMission2": 3, "_gekisouMission3": 3},
            {"_id": 21, "_liveMusicId": 10}, {"_id": 22, "_liveMusicId": 999}],
        "MasterArenaMusic": [
            {"_id": 30, "_liveMusicId": 10, "_liveMusicType": 5,
             "_gekisouMission1": 3, "_gekisouMission2": 0, "_gekisouMission3": 1,
             "_typeBonusRate": 1200, "_bestMusicTagBonusRate": 3400},
            {"_id": 31, "_liveMusicId": 10}, {"_id": 32, "_liveMusicId": 999}]
    });
    let texts: Vec<_> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData": v}).to_string())).collect();
    Master::from_json_tables(|n| texts.iter().find(|(k, _)| k == n).map(|(_, t)| t.as_str())).unwrap()
}

#[test]
fn ordinary_modes_use_base_music_without_event_parameters() {
    let m = master();
    for mode in [Scenario::Free(10), Scenario::Mission(10), Scenario::Battle(10)] {
        let s = mode.resolve(&m).unwrap();
        assert_eq!(s.live_music_id, 10);
        assert_eq!(s.power_music.music_type, 2);
        assert_eq!(s.gekisou_missions, [1, 2, 3]);
        assert!(!s.calc_event_parameter);
    }
}

#[test]
fn challenge_changes_power_but_not_missions() {
    let m = master();
    for (id, ty) in [(20, 4), (21, 2)] {
        let s = Scenario::Challenge(id).resolve(&m).unwrap();
        assert_eq!(s.live_music_id, 10);
        assert_eq!(s.power_music.id, 10);
        assert_eq!(s.power_music.music_type, ty);
        assert_eq!(s.power_music.best_music_tag_ids.as_deref(), Some(&[7, 9][..]));
        assert_eq!((s.power_music.type_bonus_rate, s.power_music.tag_bonus_rate), (0, 0));
        assert_eq!(s.gekisou_missions, [1, 2, 3]);
        assert!(s.calc_event_parameter);
    }
}

#[test]
fn arena_resolves_each_mission_but_keeps_multiplayer_power() {
    let m = master();
    let s = Scenario::Arena(30).resolve(&m).unwrap();
    assert_eq!(s.gekisou_missions, [3, 2, 1]);
    assert_eq!(s.power_music.music_type, 2);
    assert_eq!((s.power_music.type_bonus_rate, s.power_music.tag_bonus_rate), (0, 0));
    assert!(!s.calc_event_parameter);
    let setup = s.gekisou_setup(&[(100, 200), (300, 400)]);
    assert_eq!(setup.fevers, [(100, 200), (300, 400)]);
    assert_eq!(setup.missions, [3, 2, 1]);
    assert_eq!(Scenario::Arena(31).resolve(&m).unwrap().gekisou_missions, [1, 2, 3]);
}

#[test]
fn arena_parameter_view_is_distinct_from_live_setup() {
    let m = master();
    let song = SongView::from_arena_row(&m, m.arena_music(30).unwrap()).unwrap();
    assert_eq!(song.id, 10);
    assert_eq!(song.music_type, 5);
    assert_eq!(song.best_music_tag_ids.as_deref(), Some(&[7, 9][..]));
    assert_eq!((song.type_bonus_rate, song.tag_bonus_rate), (1200, 3400));
    // Parameter type zero does not inherit the base type on an arena music object.
    let unset = SongView::from_arena_row(&m, m.arena_music(31).unwrap()).unwrap();
    assert_eq!(unset.music_type, 0);
}

#[test]
fn absent_music_and_broken_references_are_errors() {
    let m = master();
    for s in [
        Scenario::Free(999),
        Scenario::Mission(999),
        Scenario::Battle(999),
        Scenario::Challenge(999),
        Scenario::Challenge(22),
        Scenario::Arena(999),
        Scenario::Arena(32),
    ] {
        assert!(s.resolve(&m).is_err());
    }
    let empty = Master::from_json_tables(|_| None).unwrap();
    assert!(Scenario::Challenge(20).resolve(&empty).is_err());
    assert!(Scenario::Arena(30).resolve(&empty).is_err());
}

#[test]
fn special_music_indexes_reject_duplicate_ids() {
    let mut m = master();
    m.arena_musics.push(m.arena_musics[0].clone());
    assert!(m.reindex().is_err());
    let mut m = master();
    m.challenge_musics.push(m.challenge_musics[0].clone());
    assert!(m.reindex().is_err());
}
