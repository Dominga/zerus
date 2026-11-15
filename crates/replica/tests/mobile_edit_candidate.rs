use serde_json::json;
use zerus_replica::{
    mobile_edit::{Baseline, Manifest},
    Actor, NewTask, ProjectStore, Stage, Target, TaskDoc,
};

fn task(store: &ProjectStore) -> TaskDoc {
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: "T-draft1",
        text: "Original requirement",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    store.save(&mut task).unwrap();
    task
}

fn manifest(baseline: &Baseline) -> Manifest {
    Manifest::decode(&serde_json::to_vec(&json!({"version":1,"project":baseline.project,"task":baseline.task,
        "edit_id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","receipt_nonce":"cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        "edits":[{"kind":"content","expected":baseline.content,"title":"A named task","text":"Reviewed requirement","media":[]},
                 {"kind":"rank","expected":"V","rank":"W"},
                 {"kind":"comment","text":"A person comment","attachments":[]}]})).unwrap()).unwrap()
}

#[test]
fn a_typed_candidate_preserves_the_seal_and_has_only_the_reviewed_effects() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let original = task(&store);
    let before = original.snapshot().unwrap();
    let baseline = Baseline::capture(&original).unwrap();
    let candidate = manifest(&baseline)
        .candidate(&original, &baseline, 2)
        .unwrap();
    assert_eq!(
        original.snapshot().unwrap(),
        before,
        "draft construction changed its source"
    );
    let view = candidate.view().unwrap();
    assert_eq!(view.title, "A named task");
    assert_eq!(view.text, "Reviewed requirement");
    assert_eq!(view.rank, "W");
    assert_eq!(view.stage, Stage::Todo);
    assert_eq!(view.session, None);
    assert!(view.worker.is_none());
    assert_eq!(
        candidate.cutover_seal().unwrap().unwrap().identity(),
        baseline.seal
    );
    assert_eq!(
        view.log
            .iter()
            .filter(|entry| entry.kind == "comment")
            .count(),
        1
    );
    assert_eq!(view.execution, original.view().unwrap().execution);
    assert_eq!(
        store.load(original.uuid()).unwrap().view().unwrap().text,
        "Original requirement"
    );
}

#[test]
fn a_changed_field_refuses_the_whole_edit_but_unrelated_history_survives() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let mut original = task(&store);
    let baseline = Baseline::capture(&original).unwrap();
    original
        .comment("Unrelated native comment", &Actor::Person, 2)
        .unwrap();
    let candidate = manifest(&baseline)
        .candidate(&original, &baseline, 3)
        .unwrap();
    assert!(candidate
        .view()
        .unwrap()
        .log
        .iter()
        .any(|entry| entry.text == "Unrelated native comment"));
    original.set_rank("X").unwrap();
    let before = original.snapshot().unwrap();
    assert!(manifest(&baseline)
        .candidate(&original, &baseline, 4)
        .is_err());
    assert_eq!(original.snapshot().unwrap(), before);
}

#[test]
fn a_phone_extra_is_refused_without_changing_its_original_bytes() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let mut original = task(&store);
    let baseline = Baseline::capture(&original).unwrap();
    baseline.covers(&original).unwrap();
    original
        .comment("A stale local extra", &Actor::Person, 2)
        .unwrap();
    let before = original.snapshot().unwrap();
    let error = baseline.covers(&original).unwrap_err();
    assert!(error.to_string().contains("draft was kept"));
    assert_eq!(original.snapshot().unwrap(), before);
}

#[test]
fn a_forged_field_baseline_cannot_approve_a_changed_current_field() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let mut original = task(&store);
    let baseline = Baseline::capture(&original).unwrap();
    original
        .edit_text("A concurrent requirement", &Actor::Person, 2)
        .unwrap();
    let mut draft = manifest(&baseline);
    let zerus_replica::mobile_edit::Edit::Content { expected, .. } = &mut draft.edits[0] else {
        panic!()
    };
    *expected = zerus_replica::mobile_edit::content_revision(&original).unwrap();
    assert!(draft.candidate(&original, &baseline, 3).is_err());
}

#[test]
fn retained_media_indices_resolve_only_against_the_exact_content_revision() {
    use zerus_replica::{
        mobile_edit::{Edit, Media},
        TaskAttachment,
    };
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let mut original = task(&store);
    let retained = TaskAttachment {
        name: "original.txt".into(),
        mime: "text/plain".into(),
        data_base64: "b3JpZ2luYWw=".into(),
        reference: "file-1".into(),
    };
    original
        .set_attachments(std::slice::from_ref(&retained), &Actor::Person, 2)
        .unwrap();
    let baseline = Baseline::capture(&original).unwrap();
    let mut draft = manifest(&baseline);
    let Edit::Content { media, .. } = &mut draft.edits[0] else {
        panic!()
    };
    *media = vec![
        Media::Keep { index: 0 },
        Media::New {
            attachment: TaskAttachment {
                name: "new.txt".into(),
                mime: "text/plain".into(),
                data_base64: "bmV3".into(),
                reference: "file-2".into(),
            },
        },
    ];
    let candidate = draft.candidate(&original, &baseline, 3).unwrap();
    assert_eq!(candidate.attachments().unwrap()[0], retained);
    assert_eq!(candidate.attachments().unwrap()[1].bytes().unwrap(), b"new");
    original.set_attachments(&[], &Actor::Person, 4).unwrap();
    assert!(draft.candidate(&original, &baseline, 5).is_err());
}

#[test]
fn a_late_task_rule_refusal_leaves_the_original_and_all_its_history_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let original = task(&store);
    let baseline = Baseline::capture(&original).unwrap();
    let mut value = serde_json::to_value(manifest(&baseline)).unwrap();
    value["edits"][0]["media"] = json!([{"kind":"keep","index":7}]);
    let draft = Manifest::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    let before = original.snapshot().unwrap();
    assert!(draft.candidate(&original, &baseline, 3).is_err());
    assert_eq!(original.snapshot().unwrap(), before);
    assert_eq!(
        store.load(original.uuid()).unwrap().view().unwrap().text,
        "Original requirement"
    );
}

#[test]
fn another_task_or_seal_cannot_retarget_the_retained_baseline() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let original = task(&store);
    let baseline = Baseline::capture(&original).unwrap();
    let draft = manifest(&baseline);
    let mut wrong = baseline.clone();
    wrong.seal = "0".repeat(64);
    assert!(draft.candidate(&original, &wrong, 3).is_err());
    let mut wrong = baseline.clone();
    wrong.task = "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into();
    assert!(draft.candidate(&original, &wrong, 3).is_err());
    let mut wrong = baseline.clone();
    wrong.project = "project-2".into();
    assert!(draft.candidate(&original, &wrong, 3).is_err());
    let mut wrong = baseline.clone();
    wrong.version.insert("123".into(), i32::MAX);
    assert!(draft.candidate(&original, &wrong, 3).is_err());
}

#[test]
fn a_target_edit_keeps_immutable_session_identity_and_can_replace_a_legacy_hint() {
    use zerus_replica::SessionKey;
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let mut original = task(&store);
    original
        .set_target(
            &Target::Session(SessionKey {
                machine: "computer-1".into(),
                session: "old-label".into(),
                session_id: String::new(),
            }),
            &Actor::Person,
            2,
        )
        .unwrap();
    let baseline = Baseline::capture(&original).unwrap();
    let next = json!({"kind":"session","session":{"machine":"computer-1","session":"new-label","session_id":"launch_id:exact-child"}});
    let draft = Manifest::decode(&serde_json::to_vec(&json!({"version":1,"project":baseline.project,"task":baseline.task,
        "edit_id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","receipt_nonce":"cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        "edits":[{"kind":"target","expected":baseline.target,"target":next}]})).unwrap()).unwrap();
    let candidate = draft.candidate(&original, &baseline, 3).unwrap();
    let Target::Session(key) = candidate.view().unwrap().target else {
        panic!()
    };
    assert_eq!(key.session_id, "launch_id:exact-child");
    assert_eq!(key.session, "new-label");
    assert_eq!(key.machine, "computer-1");
}

#[test]
fn a_canonical_phone_projection_cannot_become_a_native_candidate() {
    use zerus_replica::phone::{answer, Exchange};
    let host_root = tempfile::tempdir().unwrap();
    let phone_root = tempfile::tempdir().unwrap();
    let host = ProjectStore::open(host_root.path(), "project-1").unwrap();
    let original = task(&host);
    let baseline = Baseline::capture(&original).unwrap();
    let phone = ProjectStore::open_phone(phone_root.path(), "project-1").unwrap();
    let mut exchange = Exchange::new();
    for _ in 0..8 {
        let Some(request) = exchange.next_request(&phone).unwrap() else {
            break;
        };
        exchange
            .accept(&phone, answer(&host, request).unwrap())
            .unwrap();
    }
    let projected = phone.load(original.uuid()).unwrap();
    assert!(!projected.view().unwrap().writable);
    baseline.covers(&projected).unwrap();
    assert!(manifest(&baseline)
        .candidate(&projected, &baseline, 3)
        .is_err());
    assert!(phone
        .admitted_cutover_identity(original.uuid())
        .unwrap()
        .is_none());
}
