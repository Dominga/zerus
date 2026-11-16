use super::*;
use serde_json::Value;
use zerus_team_plan::{Dep, DepKind};

fn source(name: &str) -> (EstimateContext, PhaseDraft) {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../../crates/team-plan/fixtures/cli-phase-publication.json"
    ))
    .unwrap();
    let raw = &cases[name];
    let draft = crate::teams::view::phases::draft(&raw["draft"], 1).unwrap();
    (
        EstimateContext {
            team: raw["draft"]["team"].as_str().unwrap().into(),
            home: raw["status"]["team"]["home"].as_str().unwrap().into(),
            revision: draft.revision,
        },
        draft,
    )
}

#[test]
fn writer_estimates_are_explicitly_preview_or_frozen_publication() {
    for name in [
        "draft",
        "rejected",
        "revised",
        "published",
        "awaiting",
        "accepted",
    ] {
        let (context, draft) = source(name);
        let view = project(&context, &draft, None);
        assert!(view.cohort_complete, "{name}: {view:?}");
        assert_eq!(view.rows.len(), draft.nodes.len());
        let scope = view.scope.unwrap();
        assert_eq!(scope.version.draft_generation, draft.generation.unwrap());
        assert_eq!(
            scope.basis,
            if draft.publication.is_some() {
                EstimateBasis::FrozenPublication
            } else {
                EstimateBasis::UnapprovedPreview
            }
        );
        for (row, original) in view.rows.iter().zip(&draft.nodes) {
            assert_eq!(row.estimate_minutes, Some(original.estimate_minutes));
            assert_eq!(row.task.is_some(), draft.publication.is_some());
        }
    }
}

#[test]
fn approved_values_never_follow_later_live_estimate_changes() {
    let (context, mut draft) = source("published");
    let original = project(&context, &draft, None);
    for node in &mut draft.live.as_mut().unwrap().nodes {
        node.estimate_minutes = 999;
    }
    assert_eq!(project(&context, &draft, None), original);
}

#[test]
fn an_unreturned_generation_or_publication_is_unavailable_not_retargeted() {
    let (context, draft) = source("published");
    let version = project(&context, &draft, None).scope.unwrap().version;
    assert_eq!(
        project(&context, &draft, Some(&version)),
        project(&context, &draft, None)
    );
    for fault in 0..3 {
        let mut requested = version.clone();
        match fault {
            0 => requested.draft_generation += 1,
            1 => requested.publication = Some("00000000-0000-4000-8000-000000000001".into()),
            _ => requested.phase += 1,
        }
        let view = project(&context, &draft, Some(&requested));
        assert_eq!(view.unknown, Some(EstimateUnknown::HistoryUnavailable));
        assert!(view.rows.is_empty() && view.scope.is_none());
    }
}

#[test]
fn zero_over_bound_and_external_prerequisites_stay_unknown() {
    for (value, reason) in [
        (0, EstimateUnknown::MissingEstimate),
        (MAX_ESTIMATE_MINUTES + 1, EstimateUnknown::EstimateOverBound),
    ] {
        let (context, mut draft) = source("published");
        draft.nodes[0].estimate_minutes = value;
        let view = project(&context, &draft, None);
        assert_eq!(view.rows[0].estimate_minutes, None);
        assert_eq!(view.rows[0].unknown, Some(reason));
    }
    let (context, mut draft) = source("published");
    draft.nodes[0].estimate_minutes = MAX_ESTIMATE_MINUTES;
    assert_eq!(
        project(&context, &draft, None).rows[0].estimate_minutes,
        Some(MAX_ESTIMATE_MINUTES)
    );
    draft.nodes[0].deps.push(Dep {
        node: 999,
        kind: DepKind::Hard,
    });
    let view = project(&context, &draft, None);
    assert_eq!(view.rows[0].estimate_minutes, None);
    assert_eq!(
        view.rows[0].unknown,
        Some(EstimateUnknown::ExternalPrerequisite)
    );
}

#[test]
fn incomplete_or_unsettled_published_scopes_never_use_complete_draft_values() {
    for mode in 0..5 {
        let (context, mut draft) = source("published");
        match mode {
            0 => draft.live = None,
            1 => draft.live.as_mut().unwrap().complete = false,
            2 => draft.live.as_mut().unwrap().revision += 1,
            3 => draft.live.as_mut().unwrap().nodes.clear(),
            _ => draft.publication.as_mut().unwrap().state = "reserved".into(),
        }
        assert!(draft.complete);
        let view = project(&context, &draft, None);
        assert!(!view.cohort_complete && view.unknown.is_some());
        assert!(view
            .rows
            .iter()
            .all(|r| r.estimate_minutes.is_none() && r.task.is_none()));
    }
}

#[test]
fn old_phase_nodes_and_stages_do_not_expand_the_frozen_implementation_cohort() {
    let (context, mut draft) = source("published");
    let original = project(&context, &draft, None);
    let live = draft.live.as_mut().unwrap();
    let mut old = live.nodes[0].clone();
    old.id = 900;
    old.estimate_minutes = 500;
    old.task.as_mut().unwrap().uuid = "00000000-0000-4000-8000-000000000900".into();
    let mut stage = old.clone();
    stage.id = 901;
    stage.kind = NodeKind::Stage;
    stage.stage_of = Some(1);
    stage.estimate_minutes = 0;
    live.nodes.extend([old, stage]);
    assert_eq!(project(&context, &draft, None), original);
    // A check is not an implementation unit, even with a nonzero estimate.
    draft.nodes[0].kind = NodeKind::Check;
    draft.live.as_mut().unwrap().nodes[0].kind = NodeKind::Check;
    let view = project(&context, &draft, None);
    assert_eq!(
        view.rows[0].unknown,
        Some(EstimateUnknown::NotImplementation)
    );
    assert_eq!(view.rows[0].estimate_minutes, None);
}

#[test]
fn identity_and_membership_ambiguity_do_not_become_numeric_estimates() {
    let (context, draft) = source("published");
    let mut alias = context.clone();
    alias.home = "arch".into();
    assert_eq!(
        project(&alias, &draft, None).unknown,
        Some(EstimateUnknown::SourceIdentityUnavailable)
    );
    let mut stale = context.clone();
    stale.revision += 1;
    assert_eq!(
        project(&stale, &draft, None).unknown,
        Some(EstimateUnknown::StaleRevision)
    );
    for mode in 0..4 {
        let mut broken = draft.clone();
        let live = broken.live.as_mut().unwrap();
        match mode {
            0 => live.nodes[0].task = None,
            1 => live.nodes[1].task = live.nodes[0].task.clone(),
            2 => live.nodes[1].id = live.nodes[0].id,
            _ => live.nodes[0].kind = NodeKind::Stage,
        }
        let view = project(&context, &broken, None);
        assert_eq!(view.unknown, Some(EstimateUnknown::ConflictingMembership));
        assert!(view.rows.iter().all(|r| r.estimate_minutes.is_none()));
    }
}

#[test]
fn unknown_fields_incomplete_input_and_caps_have_no_numeric_fallback() {
    let (context, original) = source("draft");
    for mode in 0..5 {
        let mut draft = original.clone();
        match mode {
            0 => draft.unknown_fields = true,
            1 => draft.complete = false,
            2 => draft.generation = None,
            3 => draft.graph.complete = false,
            _ => {
                draft.nodes = (1..=MAX_COHORT as u32 + 1)
                    .map(|id| {
                        let mut node = original.nodes[0].clone();
                        node.id = id;
                        node
                    })
                    .collect();
            }
        }
        let view = project(&context, &draft, None);
        assert!(view.unknown.is_some());
        assert!(view.rows.iter().all(|r| r.estimate_minutes.is_none()));
    }
}
