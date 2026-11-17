//! Every planning example runs as the bound lead through the real CLI writer.
use super::*;

#[test]
fn planning_examples_submit_and_edit_the_complete_draft_without_publishing() {
    let h = Harness::new();
    let mut create = serde_json::to_value(crate::teams::ops::tests::create()).unwrap();
    create["op"] = "start_planning".into();
    create["goal"] = "Deliver the reviewed foundation.".into();
    h.apply(&person(), serde_json::from_value(create).unwrap());
    h.bind("lead", LEAD_SESSION);
    let caller = Caller::Session(key(LEAD_SESSION));
    let mut commands = Vec::new();
    for (index, example) in examples(PLANNING_PAGE).into_iter().enumerate() {
        let input = example.input.as_ref().map(|text| {
            let mut request: Value = serde_json::from_str(text).unwrap();
            for field in ["request_id", "team", "expected_revision"] {
                assert!(request.get(field).is_some(), "missing {field}: {text}");
            }
            request["request_id"] = json!(uid(0x6000 + index as u32));
            request["team"] = json!(TEAM);
            request["expected_revision"] = json!(h.revision());
            request.to_string()
        });
        let (reply, exit) = h.cli(&caller, &line(&example), input.as_deref().unwrap_or(""));
        assert_eq!(exit, 0, "planning page {:?}: {reply}", example.args);
        commands.push(example.args[0].clone());
    }
    assert_eq!(
        commands,
        [
            "current",
            "phases",
            "submit-draft-phase",
            "phase",
            "edit-draft-phase"
        ]
    );
    let store = h.store();
    let draft = store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(draft.generation, Some(2));
    assert_eq!(draft.phase.title, "Foundation revised");
    assert_eq!(draft.nodes.len(), 1);
    assert_eq!(draft.nodes[0].id, 1);
    assert_eq!(draft.nodes[0].criteria.len(), 2);
    assert!(draft.publication.is_none());
    assert!(store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    assert!(store.publication_work(TEAM).unwrap().is_empty());
}
