//! An exact correction intent never adopts another publication or episode.
use super::types::{canonical_uuid, TeamAction};

pub(super) fn check(action: &TeamAction) -> Result<(), String> {
    let (phase, publication, rework, generation, draft, comment, rejection) = match action {
        TeamAction::SendBackPhase {
            phase,
            publication,
            comment,
        } => (*phase, publication, None, None, None, Some(comment), None),
        TeamAction::SubmitPhaseCorrections {
            phase,
            publication,
            rework,
            draft,
        } => (
            *phase,
            publication,
            Some(rework),
            None,
            Some(draft),
            None,
            None,
        ),
        TeamAction::EditPhaseCorrections {
            phase,
            publication,
            rework,
            generation,
            draft,
        } => (
            *phase,
            publication,
            Some(rework),
            Some(*generation),
            Some(draft),
            None,
            None,
        ),
        TeamAction::ApprovePhaseCorrections {
            phase,
            publication,
            rework,
            generation,
        } => (
            *phase,
            publication,
            Some(rework),
            Some(*generation),
            None,
            None,
            None,
        ),
        TeamAction::RejectPhaseCorrections {
            phase,
            publication,
            rework,
            generation,
            comment,
        } => (
            *phase,
            publication,
            Some(rework),
            Some(*generation),
            None,
            Some(comment),
            None,
        ),
        TeamAction::UndoPhaseCorrectionRejection {
            phase,
            publication,
            rework,
            generation,
            rejection,
        } => (
            *phase,
            publication,
            Some(rework),
            Some(*generation),
            None,
            None,
            Some(rejection),
        ),
        _ => unreachable!(),
    };
    if phase == 0
        || !canonical_uuid(publication)
        || rework.is_some_and(|v| !canonical_uuid(v))
        || generation.is_some_and(|g| g == 0 || g > i64::MAX as u64)
        || rejection.is_some_and(|v| !canonical_uuid(v))
    {
        return Err("Read the exact phase publication and correction episode again.".into());
    }
    if let Some(d) = draft {
        super::drafts::check(phase, d)?;
    }
    if let Some(c) = comment {
        if c.trim().is_empty()
            || c.len() > 16_384
            || zerus_team_plan::forbidden_char(&c.replace(['\n', '\t'], "")).is_some()
        {
            return Err(
                "A bounded nonblank phase comment without forbidden characters is required.".into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upstream_teams::action::{prepare, TeamCall};
    use serde_json::{json, Value};

    fn calls() -> Vec<Value> {
        let source: Value = serde_json::from_str(include_str!(
            "../../../../../crates/team-plan/fixtures/cli-phase-rework.json"
        ))
        .unwrap();
        let row = &source["proposal"]["draft"]["draft"];
        let episode = &row["rework"];
        let draft = &episode["draft"];
        let input = json!({"title":draft["phase"]["title"],"barrier":draft["phase"]["barrier"].as_bool().unwrap_or(false),"nodes":draft["nodes"]});
        let base = json!({"phase":1,"publication":episode["base_publication"],"rework":episode["id"],"generation":draft["generation"]});
        [
            "send_back_phase",
            "submit_phase_corrections",
            "edit_phase_corrections",
            "approve_phase_corrections",
            "reject_phase_corrections",
            "undo_phase_correction_rejection",
        ]
        .into_iter()
        .map(|kind| {
            let mut value = base.clone();
            value["kind"] = kind.into();
            if kind == "send_back_phase" {
                value.as_object_mut().unwrap().remove("rework");
            }
            if matches!(kind, "send_back_phase" | "submit_phase_corrections") {
                value.as_object_mut().unwrap().remove("generation");
            }
            if matches!(kind, "send_back_phase" | "reject_phase_corrections") {
                value["comment"] = "Keep accepted work.\nAdd the regression test.".into();
            }
            if matches!(kind, "submit_phase_corrections" | "edit_phase_corrections") {
                value["draft"] = input.clone();
            }
            if kind == "undo_phase_correction_rejection" {
                value["rejection"] = "00000000-0000-4000-8000-000000000076".into();
            }
            value
        })
        .collect()
    }

    #[test]
    fn correction_calls_keep_original_episode_and_request_in_the_existing_typed_lane() {
        for action in calls() {
            let call:TeamCall=serde_json::from_value(json!({"team":"00000000-0000-4000-8000-00000000beef","team_name":"Fixture","request_id":"00000000-0000-4000-8000-000000000077","expected_revision":16,"action":action})).unwrap();
            let (invocation, _) = prepare(&call).unwrap();
            assert_eq!(
                invocation.args,
                vec![
                    "team".to_owned(),
                    action["kind"].as_str().unwrap().replace('_', "-"),
                    "--json".to_owned()
                ]
            );
            let body: Value = serde_json::from_slice(invocation.stdin.as_ref().unwrap()).unwrap();
            assert_eq!(body["publication"], action["publication"]);
            for key in ["rework", "generation", "draft", "comment", "rejection"] {
                assert_eq!(body.get(key), action.get(key));
            }
            assert_eq!(body["expected_revision"], 16);
            assert_eq!(body["request_id"], call.request_id);
            assert!(body.get("actor").is_none());
            assert!(body.get("confirmed").is_none());
        }
    }

    #[test]
    fn correction_intents_refuse_invalid_ids_controls_and_comment_bounds_before_dispatch() {
        for mut action in calls() {
            action["publication"] = "not-a-publication".into();
            assert!(serde_json::from_value::<TeamAction>(action)
                .unwrap()
                .check()
                .is_err());
        }
        for comment in [
            "".to_owned(),
            "я".repeat(8193),
            "Do\u{2028}not hide a line".into(),
            "Do\u{202e}not reverse text".into(),
        ] {
            let mut action = calls().remove(0);
            action["comment"] = comment.into();
            assert!(serde_json::from_value::<TeamAction>(action)
                .unwrap()
                .check()
                .is_err());
        }
        let mut action = calls().remove(1);
        action["actor"] = json!({"kind":"person"});
        assert!(serde_json::from_value::<TeamAction>(action).is_err());
    }
}
