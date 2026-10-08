use super::*;

#[test]
fn bounded_source_indexes_account_actual_capacities_and_keep_the_exact_byte_boundary() {
    let (master, ..) = tests::fixture();
    let mut context = BuildContext::try_bounded(&master, usize::MAX).unwrap();
    let required = context.bounded_bytes().unwrap();
    let payload = (master.live_skill_effects.len()
        + master.support_skill_effects.len()
        + master.gekisou_skill_effects.len()
        + master.gekisou_support_skill_effects.len())
        * std::mem::size_of::<(SkillKey, usize)>()
        + master.skill_condition_sets.len() * std::mem::size_of::<(i64, usize)>();
    assert!(required >= std::mem::size_of::<BuildContext<'_>>() + payload);
    assert!(BuildContext::try_bounded(&master, required).is_some());
    assert!(BuildContext::try_bounded(&master, required - 1).is_none());
    assert!(BuildContext::try_bounded(&master, 0).is_none());
    assert!(BuildContext::new(&master).bounded_bytes().is_none(), "opaque hash storage is not called bounded");

    let RowIndex::Sorted(rows) = &mut context.live else { panic!("bounded representation") };
    let old_capacity = rows.capacity();
    rows.reserve_exact(37);
    let extra = (rows.capacity() - old_capacity) * std::mem::size_of::<(SkillKey, usize)>();
    assert!(extra > 0);
    assert_eq!(context.bounded_bytes().unwrap(), required + extra, "spare slots are retained bytes too");
}

#[test]
fn bounded_source_indexes_keep_original_row_positions_duplicates_and_missing_keys() {
    let (mut master, ..) = tests::fixture();
    master.live_skill_effects.push(master.live_skill_effects[0].clone());
    master.support_skill_effects.push(master.support_skill_effects[0].clone());
    master.gekisou_skill_effects.push(master.gekisou_skill_effects[0].clone());
    master.gekisou_support_skill_effects.push(master.gekisou_support_skill_effects[0].clone());
    master.skill_condition_sets.push(master.skill_condition_sets[1].clone());
    master.skill_condition_sets.reverse();
    // Index construction must neither validate duplicates nor consult a stale Master ID index.
    let bounded = BuildContext::try_bounded(&master, 1 << 20).unwrap();
    macro_rules! same_rows {
        ($field:ident, $method:ident, $id:ident) => {
            for key in master.$field.iter().map(|row| (row.$id, row.level)).chain([(i64::MIN, i64::MAX)]) {
                let expected: Vec<_> =
                    master.$field.iter().filter(|row| (row.$id, row.level) == key).map(std::ptr::from_ref).collect();
                let actual: Vec<_> = bounded.$method(key).map(std::ptr::from_ref).collect();
                assert_eq!(actual, expected);
            }
        };
    }
    same_rows!(live_skill_effects, live_rows, live_skill_id);
    same_rows!(support_skill_effects, support_rows, support_skill_id);
    same_rows!(gekisou_skill_effects, gekisou_rows, skill_id);
    same_rows!(gekisou_support_skill_effects, gekisou_support_rows, skill_id);
    for group in master.skill_condition_sets.iter().map(|row| row.group).chain([i64::MIN]) {
        let expected: Vec<_> =
            master.skill_condition_sets.iter().filter(|row| row.group == group).map(std::ptr::from_ref).collect();
        let actual: Vec<_> = bounded.condition_sets(group).map(std::ptr::from_ref).collect();
        assert_eq!(actual, expected);
    }
}
