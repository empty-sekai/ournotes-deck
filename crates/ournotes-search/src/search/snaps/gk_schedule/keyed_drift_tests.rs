use super::*;

#[test]
fn prefix_counts_intersect_the_complete_pool_certificate() {
    let d = KeyedDrift {
        roundings: factor_roundings(200.0, 20.0),
        judgement_max: 1.0,
        chain_extra: 0.0,
        e_max: 100_000.0,
        execution_limit: 200.0,
        command_limit: 20.0,
        factor_limit: 0.7,
        cmd_top: [4.0; 5],
        fac_top: [0.14; 5],
    };
    let pool = factor_drift(200.0, 20.0, 0.7).unwrap();
    assert!(factor_drift(d.e_max * 200.0, 200.0, 1000.0).is_none());
    assert_eq!(d.drift(200.0, 1000.0), Some(pool));
    assert!(d.drift(1.0, 0.1).unwrap() < pool);
    assert!(d.drift(0.0, 0.0).unwrap() < pool);
}
