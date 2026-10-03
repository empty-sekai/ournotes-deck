//! Furniture power against reference values recorded from the client, with the master rows they were recorded on.
//! The recorded constructor profile (rank 0) is not a valid OwnedSnapshot account.
#![cfg(feature = "native-fixtures")]
mod reference;

use std::collections::BTreeMap;

use ournotes_sim::{calc::BonusData, cards::Roster, master::Master, pool::Pool, power::CardPower};
use serde_json::Value;

fn capture() -> (Master, Value) {
    let native = reference::json("jp-native-band-item-power.json");
    let items = reference::json("jp-band-item-catalog.json");
    let ranks = reference::json("jp-player-rank-catalog.json");
    let mut tables = BTreeMap::new();
    for source in [&native, &items, &ranks] {
        for (name, record) in source["tables"].as_object().unwrap() {
            assert!(tables.insert(name.clone(), record["table"].to_string()).is_none());
        }
    }
    let master = Master::from_json_tables(|name| tables.get(name).map(String::as_str)).unwrap();
    (master, native)
}

fn observed_power(value: &Value) -> CardPower {
    let bp = &value["bp"];
    CardPower::bp(
        bp["Performance"].as_i64().unwrap(),
        bp["Technique"].as_i64().unwrap(),
        bp["Visual"].as_i64().unwrap(),
    )
}

fn assert_native_power(actual: CardPower, expected: &Value, path: &str) {
    assert_eq!(actual, observed_power(expected), "{path}: native BP channels");
    let ints = &expected["integerPower"];
    assert_eq!(
        [actual.performance_points(), actual.technique_points(), actual.visual_points(), actual.total()],
        [
            ints["Performance"].as_i64().unwrap() as i32,
            ints["Technique"].as_i64().unwrap() as i32,
            ints["Visual"].as_i64().unwrap() as i32,
            ints["Total"].as_i64().unwrap() as i32
        ],
        "{path}: native getters"
    );
    assert_eq!(expected["nativeBoxAndValueSelfGetterAgree"], true);
}

#[test]
fn actual_furniture_inputs_match_both_distinct_native_profiles_and_combined_floor() {
    let (master, captured) = capture();
    assert_eq!(captured["newNativeCalls"], 0);
    let mut observed = BTreeMap::new();
    let mut retained_calls = 0;
    for case in captured["cases"].as_array().unwrap() {
        let name = case["case"].as_str().unwrap();
        let input = &case["input"];
        let expected = &case["expected"];
        let roster = Roster::from_json(&input["roster"].to_string()).unwrap();
        assert_eq!(roster.player.vip_rank, 0); // The recorded profile has no player rank.
        assert_eq!(input["leaderPhysicalSlot"], 2);
        let pool = Pool::new(&master, &roster).unwrap();
        let members: [i64; 5] = serde_json::from_value(input["members"].clone()).unwrap();
        let snaps: [i64; 5] = serde_json::from_value(input["snaps"].clone()).unwrap();
        let deck = pool.deck(members, snaps.map(Some), [0, 1, 2, 3, 4]).unwrap();
        let song = pool.song(input["musicId"].as_i64().unwrap()).unwrap();
        let live_factory_profile = pool.deck_power(&deck, Some(&song), false).unwrap();
        assert_eq!(
            live_factory_profile.total.total() as i64,
            expected["liveFactoryTotal"].as_i64().unwrap(),
            "{name}: full inherited Live factory profile"
        );

        let m = deck.members.map(|index| pool.members[index].slot());
        let s = deck.snaps.map(|index| pool.snaps[index.unwrap()].slot());
        // The isolated calculator receives furniture bonuses only, derived from the declared items and the master
        // rows; recorded values are compared, never fed back into the calculation.
        let bonuses = deck.members.map(|index| BonusData {
            band_item_bonus: pool.power.band_items.bonus(&pool.members[index]),
            ..Default::default()
        });
        let mu = song.slot();
        let isolated = pool
            .power
            .calc
            .deck_power(
                [&m[0], &m[1], &m[2], &m[3], &m[4]],
                [Some(&s[0]), Some(&s[1]), Some(&s[2]), Some(&s[3]), Some(&s[4])],
                &bonuses,
                Some(&mu),
            )
            .unwrap();
        let mut total_base = CardPower::EMPTY;
        let mut total_furniture = CardPower::EMPTY;
        for (i, (slot, bonus)) in isolated.slots.iter().zip(&bonuses).enumerate() {
            assert_native_power(pool.members[deck.members[i]].power, &expected["nativeMemberPower"][i], name);
            assert_native_power(bonus.band_item_bonus, &expected["nativeRate"][i], name);
            let native = &expected["isolatedSlots"][i];
            assert_native_power(slot.base_power, &native["get_BasePower"], name);
            assert_native_power(slot.band_item, &native["get_BandItemBonusPower"], name);
            assert_native_power(slot.total, &native["get_TotalPower"], name);
            total_base = total_base.add(slot.base_power);
            total_furniture = total_furniture.add(slot.band_item);
        }
        assert_native_power(isolated.total, &expected["isolatedDeck"]["get_Total"], name);
        assert_native_power(total_base, &expected["isolatedDeck"]["get_TotalBasePower"], name);
        assert_native_power(total_furniture, &expected["isolatedDeck"]["get_TotalBandItemBonusPower"], name);
        assert_eq!(case["pathEntryCalls"]["liveMemberFactory"], 1);
        assert_eq!(case["pathEntryCalls"]["isolatedSlotCalculator"], 5);
        assert_eq!(case["pathEntryCalls"]["isolatedDeckCalculator"], 1);
        retained_calls += case["retainedNativeCalls"].as_u64().unwrap();
        observed.insert(
            name.to_string(),
            (live_factory_profile.total.total(), isolated.total.total(), total_furniture.total()),
        );
    }
    assert_eq!(retained_calls, 3079);
    assert_eq!(observed["empty"], (59223, 57207, 0));
    assert_eq!(observed["single-101-lv1"], (59268, 57252, 45));
    assert_eq!(observed["single-101-lv30"], (60733, 58717, 1510));
    assert_eq!(observed["five-band1-lv30"], (66808, 64792, 7585));
    assert_eq!(observed["cross-band2-lv30"], observed["empty"]);
    // Flooring each item before adding would lose 35 points.
    assert_eq!(observed["five-band1-lv30"].2 - 5 * observed["single-101-lv30"].2, 35);
    for values in observed.values() {
        // The two profiles differ by a constant.
        assert_eq!(values.0 - values.1, 2016);
    }
}
