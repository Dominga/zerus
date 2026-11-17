//! Synthetic provenance fault controls on a decoded product page. These do not
//! claim any original/native correlation or measured implementation coverage.
use super::*;
use crate::upstream_teams::{estimate_projection::EstimateContext, view::phases};
use serde_json::Value;
fn inputs() -> (EstimateContext, PhaseDraft, observed::ObservedPane) {
    let source: Value = serde_json::from_str(include_str!(
        "../../../../../crates/team-plan/fixtures/cli-observed-phase.json"
    ))
    .unwrap();
    let draft = phases::draft(&source["context"]["draft"], 1).unwrap();
    let context = EstimateContext {
        team: source["team"].as_str().unwrap().into(),
        home: source["observed"]["scope"]["home"].as_str().unwrap().into(),
        revision: draft.revision,
    };
    let page =
        observed::decode(&source, &context.team, 1, None, observed::DEFAULT_WINDOW_MS).unwrap();
    (context, draft, observed::project(page, 1, None))
}
fn synthetic_correlated() -> (EstimateContext, PhaseDraft, observed::ObservedPane) {
    let (context, draft, mut pane) = inputs();
    let p = draft.publication.as_ref().unwrap();
    let publication = Some(observed::PublicationKey {
        id: p.id.clone(),
        draft_generation: p.generation,
    });
    for unit in &mut pane.page.cohort.units {
        unit.key.publication = publication.clone();
    }
    for fact in &mut pane.page.rows {
        fact.key.publication = publication.clone();
        fact.reasons
            .retain(|r| *r != observed::Reason::PublicationUnavailable);
    }
    pane.page
        .reasons
        .retain(|r| *r != observed::Reason::PublicationUnavailable);
    pane = observed::project(pane.page, 1, None);
    (context, draft, pane)
}
#[test]
fn synthetic_matching_provenance_can_associate_an_estimate_but_never_an_actual() {
    let (context, draft, pane) = synthetic_correlated();
    let view = project(&context, &draft, &pane).unwrap();
    assert_eq!(view.units[0].estimate_minutes, Some(30));
    assert_eq!(
        view.units[0].unknown,
        ComparisonUnknown::ImplementationUnsupported
    );
    assert_eq!(view.units[0].coverage.implementation_observed_ms, None);
}
#[test]
fn synthetic_task_and_publication_changes_cannot_join_by_node_alone() {
    for fault in 0..3 {
        let (context, draft, mut pane) = synthetic_correlated();
        let change = |key: &mut observed::ObservedUnitKey| match fault {
            0 => key.task.uuid = "00000000-0000-4000-8000-00000000abcd".into(),
            1 => {
                key.publication.as_mut().unwrap().id = "00000000-0000-4000-8000-00000000abcd".into()
            }
            _ => key.publication.as_mut().unwrap().draft_generation += 1,
        };
        for unit in &mut pane.page.cohort.units {
            change(&mut unit.key);
        }
        for fact in &mut pane.page.rows {
            change(&mut fact.key);
        }
        pane = observed::project(pane.page, 1, None);
        let view = project(&context, &draft, &pane).unwrap();
        assert_eq!(view.units[0].estimate_minutes, None);
        assert_eq!(
            view.units[0].unknown,
            if fault == 0 {
                ComparisonUnknown::DifferentTask
            } else {
                ComparisonUnknown::DifferentPublication
            }
        );
    }
}
#[test]
fn synthetic_partial_coverage_and_extra_exact_units_do_not_mean_complete_work() {
    let (context, draft, mut pane) = synthetic_correlated();
    pane.page.more = true;
    pane.page.truncated = true;
    pane.page.next = Some("ot1.synthetic-control".into());
    let partial = project(&context, &draft, &pane).unwrap();
    assert!(partial.estimates.cohort_complete);
    assert_eq!(partial.units[0].unknown, ComparisonUnknown::PartialCoverage);
    assert_eq!(partial.units[0].estimate_minutes, None);
    let (context, draft, mut pane) = inputs();
    let mut second = pane.page.cohort.units[0].clone();
    second.id = pane.page.rows[1].seq;
    second.key.effect = Some("00000000-0000-4000-8000-00000000abcd".into());
    pane.page.rows[1].unit = second.id;
    pane.page.rows[1].key = second.key.clone();
    pane.page.cohort.units.push(second);
    pane = observed::project(pane.page, 1, None);
    let view = project(&context, &draft, &pane).unwrap();
    assert_eq!(view.estimates.rows.len(), 1);
    assert_eq!(view.units.len(), 2);
    assert_ne!(view.units[0].unit, view.units[1].unit);
    assert!(view
        .units
        .iter()
        .all(|u| u.coverage.implementation_observed_ms.is_none()));
}
#[test]
fn corrupt_positive_or_zero_coverage_is_rejected_instead_of_becoming_actual_time() {
    for actual in [0, 1] {
        let (context, draft, mut pane) = inputs();
        pane.coverage[0].implementation_observed_ms = Some(actual);
        assert!(project(&context, &draft, &pane).is_none());
    }
}
