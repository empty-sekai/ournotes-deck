use super::*;
use ournotes_sim::live::full::{LuckExactBudget, LuckExactSession, LuckScoreSession, LuckTerminalPayoffSession};

fn raw_summary(
    master: &Master,
    skills: &LuckSkills,
    input: &FiniteSeedContext,
    complete_terminal: bool,
) -> CertifiedEvaluation {
    let schedule: Vec<_> = (0..uniform::ORDERS).rev().collect();
    let mut observed = Vec::new();
    let mut cache = LuckDpCache::new(1 << 20);
    let result = evaluate_luck_context_bounded_policy(
        master,
        skills,
        input,
        &PayoffMap::Score,
        Some(&mut cache),
        complete_terminal,
        (&schedule, false, |event| {
            if let LuckContextEvent::Scored { index, order } = event {
                assert!(order.evaluated);
                assert!(order.refined_payoff.is_none());
                observed.push(index);
            }
            true
        }),
        || false,
    )
    .unwrap();
    assert_eq!(observed, schedule);
    let LuckContextOutcome::Full(result) = result else { panic!("complete raw summary") };
    assert_eq!(result.orders.iter().map(|order| order.order).collect::<Vec<_>>(), uniform::all_orders());
    result
}

#[test]
fn nonlinear_initial_terminal_summaries_enclose_all_native_laws_and_keep_mapped_refinement() {
    let (master, skills, input) = input_with_order_skills(true);
    let mut exact = LuckExactSession::new(
        &master,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        input.rank_confirmations.as_deref(),
        0,
    )
    .unwrap();
    let mut budget = LuckExactBudget::default();
    let laws: Vec<_> = uniform::all_orders()
        .into_iter()
        .map(|order| {
            let performers = order.map(|slot| input.performers[slot].clone());
            exact.law(&performers, &mut budget, || false).unwrap().law.expect("complete native branch law")
        })
        .collect();
    let witness = laws
        .iter()
        .position(|law| law.atoms().iter().any(|atom| atom.score != law.atoms()[0].score))
        .expect("the threshold must split an actual nonconstant native law");
    let low = laws[witness].atoms().iter().map(|atom| atom.score).min().unwrap();
    let high = laws[witness].atoms().iter().map(|atom| atom.score).max().unwrap();
    let threshold = low + (high - low + 1) / 2;
    let life = laws[witness].atoms()[0].final_life;
    let mut session = LuckScoreSession::new(
        &master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        input.rank_confirmations.as_deref(),
    );
    let LuckRushPreparation::Ready(terminal) = session.rush_cap_preparation(&input.performers, None, || false) else {
        panic!("exercise an admitted terminal summary, not only its replay fallback")
    };
    assert!(terminal.terminal_summary(i64::from(input.params.total_power)).is_some());
    let raw = [raw_summary(&master, &skills, &input, false), raw_summary(&master, &skills, &input, true)];
    for map in [
        PayoffMap::ScoreAtLeast { threshold },
        PayoffMap::CappedScore { threshold },
        PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life: life },
    ] {
        let mut answers = Vec::new();
        for raw in &raw {
            let initial = aggregate_orders(raw.orders.clone(), &map).unwrap();
            assert!(initial.exact_payoff.is_none());
            assert!(initial.refinements.iter().any(|row| row.order_index == witness));
            let mut rows = initial.orders;
            // The mapped terminal provider remains a separate proof. It may refine payoff without
            // replacing raw mean/support/LIFE; the independent full native law then checks both proofs.
            let order = rows[witness].order;
            let performers = order.map(|slot| input.performers[slot].clone());
            let before =
                (rows[witness].mean, rows[witness].support, rows[witness].final_life, rows[witness].exact_mean);
            let mut mapped = LuckTerminalPayoffSession::new(
                &master,
                &skills,
                &input.notes,
                &input.events,
                input.params,
                input.gekisou.as_ref().unwrap(),
                &input.play,
                &input.delta_times,
            );
            let proof = mapped
                .payoff(&performers, map.terminal_payoff().unwrap(), &mut LuckExactBudget::default(), || false)
                .unwrap()
                .payoff
                .expect("complete mapped terminal payoff");
            assert!(proof.exact_constant().is_none());
            refine_order_with_terminal_payoff(&mut rows[witness], &map, &proof).unwrap();
            assert_eq!(
                before,
                (rows[witness].mean, rows[witness].support, rows[witness].final_life, rows[witness].exact_mean)
            );
            assert!(
                aggregate_orders(rows.clone(), &map).unwrap().refinements.iter().any(|row| row.order_index == witness)
            );
            for (row, law) in rows.iter_mut().zip(&laws) {
                assert!(refine_order_with_exact_law(row, &map, law).unwrap());
            }
            let final_value = aggregate_orders(rows, &map).unwrap();
            assert!(final_value.refinements.is_empty());
            answers.push((final_value.exact_score.unwrap(), final_value.exact_payoff.unwrap()));
        }
        assert_eq!(answers[0], answers[1], "terminal and factor summaries preserve the same nominal payoff");
    }
}

#[test]
fn nonlinear_initial_terminal_refusal_keeps_the_full_factor_summary_fallback() {
    let (master, skills, mut input) = input_with_order_skills(true);
    input.rank_confirmations = Some(Vec::new());
    let mut session = LuckScoreSession::new(
        &master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        input.rank_confirmations.as_deref(),
    );
    assert!(matches!(
        session.rush_cap_preparation(&input.performers, None, || false),
        LuckRushPreparation::Unavailable { reason: LuckRushDecline::ExternalRanking, .. }
    ));
    let old = raw_summary(&master, &skills, &input, false);
    let fallback = raw_summary(&master, &skills, &input, true);
    for (a, b) in old.orders.iter().zip(&fallback.orders) {
        assert_eq!(
            (a.order, a.mean, a.support, a.final_life, a.exact_mean),
            (b.order, b.mean, b.support, b.final_life, b.exact_mean)
        );
    }
    for map in [
        PayoffMap::ScoreAtLeast { threshold: 1000 },
        PayoffMap::CappedScore { threshold: 1000 },
        PayoffMap::ScoreAndLifeAtLeast { threshold: 1000, min_final_life: 1 },
    ] {
        let a = aggregate_orders(old.orders.clone(), &map).unwrap();
        let b = aggregate_orders(fallback.orders.clone(), &map).unwrap();
        assert_eq!(
            (a.score, a.payoff, a.exact_score, a.exact_payoff),
            (b.score, b.payoff, b.exact_score, b.exact_payoff)
        );
    }
    assert!(session.summary_or_terminal(&input.performers, None, || true).unwrap().is_none());
}
