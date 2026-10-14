use super::*;
fn r(combo: i32, score: i32, perfect: i32) -> Option<NetworkGekisouResult> {
    Some(NetworkGekisouResult { combo, score, perfect_count: perfect, ..Default::default() })
}
#[test]
fn group_rank_and_confirmed_rank_are_different() {
    let n = calculate_network_gekisou_ranking(&[1, 2, 3], 3, &[r(10, 5, 3), r(10, 5, 3), r(8, 99, 99)]).unwrap();
    assert_eq!(n.groups, [vec![0, 1], vec![2]]);
    assert_eq!(n.player_rank(2), 2);
    assert_eq!(n.confirmed_rank(2), 3);
    assert_eq!(n.confirmed_rank(20), 5);
}
#[test]
fn null_comparison_follows_primary_and_noinput_needs_zero_counts() {
    let mut added = r(1, 0, 0).unwrap();
    added.has_no_input = true;
    assert!(!gekisou_no_input(Some(&added)));
    let n = calculate_network_gekisou_ranking(&[1], 4, &[r(-1, 90, 90), None, Some(added), r(0, 0, 0)]).unwrap();
    assert_eq!(n.groups, [vec![2], vec![3], vec![1], vec![0]]);
    assert_eq!(n.confirmed_ranks, [3, 5, 1, 2]);
}
#[test]
fn score_precedes_perfect_and_broken_is_not_controller_limit() {
    let n = calculate_network_gekisou_ranking(&[1], 3, &[r(1, 3, 9), r(1, 4, 0), r(1, 3, 10)]).unwrap();
    assert_eq!(n.groups, [vec![1], vec![2], vec![0]]);
    let mut fourth = r(1, 0, 0).unwrap();
    fourth.range_index = 3;
    let n = calculate_network_gekisou_ranking(&[1; 4], 1, &[Some(fourth)]).unwrap();
    assert_eq!(n.broken, Some(BrokenGekisouRanking::RangeOutOfBounds(3)));
    assert_eq!(n.confirmed_rank(4), 1);
    assert_eq!(n.confirmed_rank(5), 5);
    assert_eq!(
        calculate_network_gekisou_ranking(&[4], 1, &[r(1, 0, 0)]).unwrap().broken,
        Some(BrokenGekisouRanking::InvalidMission(4))
    );
}
