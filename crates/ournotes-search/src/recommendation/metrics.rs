//! The metrics of a request: their fields, simple metrics and combined metrics.

use super::*;

/// The fields of a metric kind; `None` for an unknown kind.
pub(super) fn metric_fields(kind: &str) -> Option<&'static [&'static str]> {
    let fields: &'static [&'static str] = match kind {
        "score" => &[],
        "scoreAtLeast" | "cappedScore" => &["threshold"],
        "scoreAndLife" => &["threshold", "minFinalLife"],
        "eventPoints" | "challengePoints" => &["eventId", "consumption"],
        "eventItems" => &["eventId", "resourceType", "resourceId", "consumption"],
        _ => return None,
    };
    Some(fields)
}

/// The search metric of a metric kind with its complete fields.
pub(super) fn simple_metric(
    kind: &str,
    threshold: Option<i32>,
    min_final_life: Option<i32>,
    event_id: Option<i64>,
    resource: (Option<i64>, Option<i64>),
) -> Metric {
    let threshold = threshold.unwrap_or_default();
    let event_id = event_id.unwrap_or_default();
    match kind {
        "score" => Metric::Score,
        "scoreAtLeast" => Metric::ScoreAtLeast { threshold },
        "cappedScore" => Metric::CappedScore { threshold },
        "scoreAndLife" => Metric::ScoreAndLifeAtLeast { threshold, min_final_life: min_final_life.unwrap_or_default() },
        "eventPoints" => Metric::ClientEventPoints { event_id },
        "challengePoints" => Metric::ClientChallengePoints { event_id },
        _ => Metric::RankedEventItems {
            event_id,
            resource_type: resource.0.unwrap_or_default(),
            resource_id: resource.1.unwrap_or_default(),
        },
    }
}

/// The search metric of a `combined` metric: levels of weighted terms, each term a metric of the goal.
pub(super) fn combined_metric(
    kind: GoalKind,
    m: &MetricWire,
    challenge_skip: bool,
    explicit_play: bool,
    issues: &mut Issues,
) -> Option<Metric> {
    let mut complete = true;
    for (name, present) in [
        ("secondaryPriority", m.secondary_priority.is_some()),
        ("threshold", m.threshold.is_some()),
        ("minFinalLife", m.min_final_life.is_some()),
        ("eventId", m.event_id.is_some()),
        ("resourceType", m.resource_type.is_some()),
        ("resourceId", m.resource_id.is_some()),
    ] {
        if present {
            complete = false;
            issues.add(format!("metric.{name}"), "input", format!("{name} does not apply to combined"));
        }
    }
    let Some(levels) = m.levels.as_deref() else {
        issues.add("metric.levels", "input", "combined needs levels");
        return None;
    };
    if !(1..=MAX_METRIC_LEVELS).contains(&levels.len()) {
        complete = false;
        issues.add("metric.levels", "input", format!("combined takes 1..={MAX_METRIC_LEVELS} levels"));
    }
    if levels.iter().map(Vec::len).sum::<usize>() > MAX_METRIC_TERMS {
        complete = false;
        issues.add("metric.levels", "input", format!("combined takes at most {MAX_METRIC_TERMS} terms"));
    }
    let mut event = false;
    let mut typed: Vec<MetricLevel> = Vec::with_capacity(levels.len());
    for (level, terms) in levels.iter().enumerate() {
        if terms.is_empty() {
            complete = false;
            issues.add(format!("metric.levels[{level}]"), "input", "a level needs a term");
        }
        typed.push(MetricLevel::new(Vec::with_capacity(terms.len())));
        for (i, term) in terms.iter().enumerate() {
            combined_term(kind, term, format!("metric.levels[{level}][{i}]"), challenge_skip, explicit_play, issues)
                .map_or_else(|| complete = false, |term| typed[level].terms.push(term));
            event |= metric_fields(&term.kind).is_some_and(|fields| fields.contains(&"eventId"));
        }
    }
    match (m.consumption.is_some(), event) {
        (false, true) => {
            complete = false;
            issues.add("metric.consumption", "input", "combined needs consumption for its event terms");
        }
        (true, false) => {
            complete = false;
            issues.add("metric.consumption", "input", "consumption applies only with event terms");
        }
        _ => {}
    }
    complete.then_some(Metric::Combined { levels: typed })
}

/// One term of a `combined` metric at `path`; None when the term is incomplete.
fn combined_term(
    kind: GoalKind,
    term: &MetricTermWire,
    path: String,
    challenge_skip: bool,
    explicit_play: bool,
    issues: &mut Issues,
) -> Option<MetricTerm> {
    let mut complete = true;
    let weight = term.weight.filter(|weight| (1..=i64::from(MAX_TERM_WEIGHT)).contains(weight));
    if weight.is_none() {
        complete = false;
        issues.add(
            format!("{path}.weight"),
            "input",
            format!("a term needs an integer weight in 1..={MAX_TERM_WEIGHT}"),
        );
    }
    let Some(fields) = metric_fields(&term.kind) else {
        issues.add(format!("{path}.kind"), "input", format!("{} is not a metric of a term", term.kind));
        return None;
    };
    if !kind.metrics().contains(&term.kind.as_str()) {
        issues.add(format!("{path}.kind"), "input", format!("{} does not apply to {}", term.kind, kind.name()));
        return None;
    }
    if support(kind, &term.kind) == Support::Unsupported {
        complete = false;
        issues.add(
            format!("{path}.kind"),
            "unsupported",
            format!("{} is not supported for {} yet", term.kind, kind.name()),
        );
    }
    if term.kind == "scoreAndLife" && !explicit_play {
        issues.add("goal.play", "input", "scoreAndLife requires an explicit complete play (pattern or stream); accuracy or an omitted play cannot describe life risk");
    }
    if challenge_skip && term.kind == "challengePoints" {
        issues.add(
            format!("{path}.kind"),
            "input",
            "challenge skip spends challenge points and cannot optimize challenge-point earnings",
        );
    }
    for (name, present) in [
        ("threshold", term.threshold.is_some()),
        ("minFinalLife", term.min_final_life.is_some()),
        ("eventId", term.event_id.is_some()),
        ("resourceType", term.resource_type.is_some()),
        ("resourceId", term.resource_id.is_some()),
    ] {
        match (present, fields.contains(&name)) {
            (true, false) => {
                complete = false;
                issues.add(format!("{path}.{name}"), "input", format!("{name} does not apply to {}", term.kind));
            }
            (false, true) => {
                complete = false;
                issues.add(format!("{path}.{name}"), "input", format!("{} needs {name}", term.kind));
            }
            _ => {}
        }
    }
    complete.then(|| MetricTerm {
        metric: simple_metric(
            &term.kind,
            term.threshold,
            term.min_final_life,
            term.event_id,
            (term.resource_type, term.resource_id),
        ),
        weight: weight.unwrap_or_default() as u32,
    })
}
