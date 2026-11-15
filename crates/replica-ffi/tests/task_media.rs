//! Media crosses the phone boundary as bounded metadata and selected bytes.
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use zerus_replica::{Actor, NewTask, ProjectStore, Stage, Target, TaskAttachment, TaskDoc};
use zerus_replica_ffi::{
    ReplicaError, TaskMediaInput, TaskPosition, TaskStage, TaskStore, TaskTarget,
};

fn phone(root: &tempfile::TempDir) -> Arc<TaskStore> {
    TaskStore::new(
        root.path().display().to_string(),
        "project-1".into(),
        "orchard".into(),
    )
}

fn file() -> TaskAttachment {
    TaskAttachment {
        name: "fixture-note.txt".into(),
        mime: "text/plain".into(),
        data_base64: "aGVsbG8=".into(),
        reference: "note-1".into(),
    }
}

fn input(data: Vec<u8>) -> TaskMediaInput {
    TaskMediaInput {
        name: "picked-note.txt".into(),
        mime: "text/plain".into(),
        data,
        reference: "picked-1".into(),
    }
}

fn seed(root: &tempfile::TempDir, comment_media: bool) -> String {
    seed_with(root, comment_media, false)
}

// Literal old data, without a native birth seal. The projection lane keeps
// these pre-cutover media/content rules testable without granting authority.
fn legacy_seed(root: &tempfile::TempDir, comment_media: bool) -> String {
    seed_with(root, comment_media, true)
}

fn legacy_store(root: &tempfile::TempDir) -> ProjectStore {
    ProjectStore::open_phone(root.path(), "project-1").unwrap()
}

fn seed_with(root: &tempfile::TempDir, comment_media: bool, legacy: bool) -> String {
    let store = if legacy {
        legacy_store(root)
    } else {
        ProjectStore::open(root.path(), "project-1").unwrap()
    };
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: &store.unused_short_id().unwrap(),
        text: "Inspect the fixture",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    task.set_attachments(&[file()], &Actor::Person, 2).unwrap();
    if comment_media {
        task.comment_with_attachments("An immutable comment", &[file()], &Actor::Person, 3)
            .unwrap();
    }
    store.save(&mut task).unwrap();
    task.uuid().to_owned()
}

fn saved(root: &tempfile::TempDir, uuid: &str) -> serde_json::Value {
    serde_json::to_value(
        ProjectStore::open(root.path(), "project-1")
            .unwrap()
            .find(uuid)
            .unwrap()
            .view()
            .unwrap(),
    )
    .unwrap()
}

fn bytes(size: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..size)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) as u8
        })
        .collect()
}

fn refused<T: std::fmt::Debug>(value: Result<T, ReplicaError>) {
    match value {
        Err(ReplicaError::Refused(message)) => assert!(!message.is_empty()),
        other => panic!("expected a rule refusal, got {other:?}"),
    }
}

#[test]
fn sealed_computer_tasks_allow_media_reads_but_refuse_phone_edits() {
    for comment_media in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let uuid = seed(&root, comment_media);
        let api = phone(&root);
        let task = api.get(uuid.clone()).unwrap();
        assert_eq!(task.schema, zerus_replica::TASK_SCHEMA);
        assert!(!task.writable);
        assert_eq!(
            api.read_media(uuid.clone(), None, 0, task.attachments[0].token.clone())
                .unwrap()
                .data,
            b"hello"
        );
        if comment_media {
            let entry = task
                .log
                .iter()
                .find(|entry| entry.kind == "comment")
                .unwrap();
            let selected = api
                .media(uuid.clone(), Some(entry.entry_id.clone()))
                .unwrap();
            assert_eq!(
                api.read_media(
                    uuid.clone(),
                    Some(entry.entry_id.clone()),
                    0,
                    selected[0].token.clone()
                )
                .unwrap()
                .data,
                b"hello"
            );
        }
        let before = saved(&root, &uuid);
        let edits = [
            api.edit_text(uuid.clone(), "No phone edit".into()),
            api.comment(uuid.clone(), "No phone comment".into()),
            api.edit_content(
                uuid.clone(),
                "No replacement".into(),
                vec![],
                vec![input(b"new".to_vec())],
                task.content_revision,
            ),
        ];
        for edit in edits {
            match edit {
                Err(ReplicaError::Refused(reason)) => {
                    assert_eq!(reason, "Update Zerus to change this task.")
                }
                other => panic!("a sealed task must remain read-only: {other:?}"),
            }
        }
        assert_eq!(saved(&root, &uuid), before);
    }
}

#[test]
fn task_rows_carry_metadata_and_previews_keep_the_original_scope() {
    let root = tempfile::tempdir().unwrap();
    let original = seed(&root, false);
    let other = seed(&root, false);
    let api = phone(&root);
    let task = api.get(original.clone()).unwrap();
    assert_eq!(task.attachments.len(), 1);
    let meta = &task.attachments[0];
    assert_eq!(
        (&*meta.name, &*meta.mime, &*meta.reference, meta.size),
        ("fixture-note.txt", "text/plain", "note-1", 5)
    );
    assert_eq!(
        meta.sha256,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    assert_eq!(meta.token.len(), 64);
    let preview = api
        .read_media(original.clone(), None, 0, meta.token.clone())
        .unwrap();
    assert_eq!(preview.data, b"hello");
    assert_eq!(preview.info, *meta);
    // Identical file bytes in a different task grant no selection authority.
    assert_ne!(
        api.get(other.clone()).unwrap().attachments[0].token,
        meta.token
    );
    refused(api.read_media(other, None, 0, meta.token.clone()));
    refused(api.read_media(original.clone(), None, 1, meta.token.clone()));
    refused(api.read_media(
        original.clone(),
        Some("unknown-entry".into()),
        0,
        meta.token.clone(),
    ));
    refused(api.read_media(original.clone(), None, 0, "f".repeat(64)));
    // Short display IDs never reselect a new task for a captured preview.
    refused(api.read_media(task.id, None, 0, meta.token.clone()));
}

#[test]
fn changing_only_file_metadata_invalidates_the_preview() {
    let root = tempfile::tempdir().unwrap();
    let uuid = seed(&root, false);
    let api = phone(&root);
    for field in 0..3 {
        let old = api.get(uuid.clone()).unwrap().attachments.remove(0);
        {
            let store = ProjectStore::open(root.path(), "project-1").unwrap();
            let mut task = store.find(&uuid).unwrap();
            let mut changed = task.attachments().unwrap().remove(0);
            match field {
                0 => changed.name = "renamed-note.txt".into(),
                1 => changed.mime = "application/octet-stream".into(),
                _ => changed.reference = "note-2".into(),
            }
            task.set_attachments(&[changed], &Actor::Person, 4 + field)
                .unwrap();
            store.save(&mut task).unwrap();
        }
        refused(api.read_media(uuid.clone(), None, 0, old.token));
        let current = api.get(uuid.clone()).unwrap().attachments.remove(0);
        assert_eq!(current.sha256, old.sha256);
    }
}

#[test]
fn previews_bind_the_selected_position_and_exact_bytes() {
    let root = tempfile::tempdir().unwrap();
    let uuid = seed(&root, false);
    {
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        let mut task = store.find(&uuid).unwrap();
        let mut duplicate = file();
        duplicate.reference.clear();
        task.set_attachments(&[duplicate.clone(), duplicate], &Actor::Person, 3)
            .unwrap();
        store.save(&mut task).unwrap();
    }
    let api = phone(&root);
    let first = api.get(uuid.clone()).unwrap();
    assert_ne!(first.attachments[0].token, first.attachments[1].token);
    refused(api.read_media(uuid.clone(), None, 1, first.attachments[0].token.clone()));
    let before = saved(&root, &uuid);
    refused(api.edit_content(
        uuid.clone(),
        "Do not duplicate retained selections".into(),
        vec![first.attachments[0].token.clone(); 2],
        vec![],
        first.content_revision,
    ));
    assert_eq!(saved(&root, &uuid), before);
    {
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        let mut task = store.find(&uuid).unwrap();
        let mut files = task.attachments().unwrap();
        files[0].data_base64 = "amVsbG8=".into(); // Same size, different bytes: jello.
        task.set_attachments(&files, &Actor::Person, 4).unwrap();
        store.save(&mut task).unwrap();
    }
    refused(api.read_media(uuid.clone(), None, 0, first.attachments[0].token.clone()));
    assert_eq!(
        api.read_media(uuid, None, 1, first.attachments[1].token.clone())
            .unwrap()
            .data,
        b"hello"
    );
}

#[test]
fn unrelated_history_does_not_overwrite_or_invalidate_a_content_draft() {
    let root = tempfile::tempdir().unwrap();
    let uuid = legacy_seed(&root, false);
    let api = phone(&root);
    let original = api.get(uuid.clone()).unwrap();
    let arrived = api
        .comment(uuid.clone(), "Arrived while editing".into())
        .unwrap();
    assert_eq!(arrived.content_revision, original.content_revision);
    let edited = api
        .edit_content(
            uuid,
            "Reviewed content".into(),
            vec![original.attachments[0].token.clone()],
            vec![],
            original.content_revision,
        )
        .unwrap();
    assert!(edited
        .log
        .iter()
        .any(|entry| entry.text == "Arrived while editing"));
    assert_eq!(edited.attachments, original.attachments);
}

#[test]
fn comment_media_is_lazy_read_only_and_bound_to_a_stable_history_entry() {
    let root = tempfile::tempdir().unwrap();
    let uuid = legacy_seed(&root, true);
    let api = phone(&root);
    let task = api.get(uuid.clone()).unwrap();
    assert!(!task.writable);
    assert_eq!(task.schema, 3);
    let comment = task.log.iter().find(|e| e.kind == "comment").unwrap();
    assert!(!comment.entry_id.is_empty());
    assert_eq!(comment.attachment_count, 1);
    let selected = api
        .media(uuid.clone(), Some(comment.entry_id.clone()))
        .unwrap();
    assert_eq!(selected.len(), 1);
    let meta = &selected[0];
    assert_ne!(meta.token, task.attachments[0].token);
    assert_eq!(
        api.read_media(
            uuid.clone(),
            Some(comment.entry_id.clone()),
            0,
            meta.token.clone()
        )
        .unwrap()
        .data,
        b"hello"
    );
    {
        let store = legacy_store(&root);
        let mut task = store.find(&uuid).unwrap();
        // An arriving row must not reinterpret a selection as a history index.
        task.comment("A concurrently arriving comment", &Actor::Person, 0)
            .unwrap();
        store.save(&mut task).unwrap();
    }
    assert_eq!(
        api.read_media(
            uuid.clone(),
            Some(comment.entry_id.clone()),
            0,
            meta.token.clone()
        )
        .unwrap()
        .data,
        b"hello"
    );
    let before = saved(&root, &uuid);
    refused(api.edit_content(
        uuid.clone(),
        "No schema expansion".into(),
        vec![],
        vec![],
        task.content_revision,
    ));
    assert_eq!(saved(&root, &uuid), before);
}

#[test]
fn create_and_edit_preserve_all_bytes_and_reject_stale_drafts_atomically() {
    let root = tempfile::tempdir().unwrap();
    let api = phone(&root);
    let task = api
        .add_with_media(
            "Review the picked file".into(),
            TaskStage::Todo,
            TaskTarget::AnySession,
            TaskPosition::End,
            vec![input(vec![0, 255, 10])],
        )
        .unwrap();
    assert_eq!(task.schema, 2);
    assert!(task.writable);
    assert_eq!(
        api.read_media(
            task.uuid.clone(),
            None,
            0,
            task.attachments[0].token.clone()
        )
        .unwrap()
        .data,
        [0, 255, 10]
    );
    // A plain legacy text edit preserves media; the original draft becomes stale.
    let current = api
        .edit_text(task.uuid.clone(), "Changed on the host".into())
        .unwrap();
    let before = saved(&root, &task.uuid);
    refused(api.edit_content(
        task.uuid.clone(),
        "Stale draft".into(),
        vec![task.attachments[0].token.clone()],
        vec![],
        task.content_revision,
    ));
    assert_eq!(saved(&root, &task.uuid), before);
    let mut second = input(b"second".to_vec());
    second.reference = "picked-2".into();
    let edited = api
        .edit_content(
            task.uuid.clone(),
            "Reviewed edit".into(),
            vec![current.attachments[0].token.clone()],
            vec![second],
            current.content_revision,
        )
        .unwrap();
    assert_eq!(edited.attachments.len(), 2);
    let native = ProjectStore::open(root.path(), "project-1")
        .unwrap()
        .find(&task.uuid)
        .unwrap();
    assert_eq!(native.view().unwrap().text, "Reviewed edit");
    assert_eq!(
        native.attachments().unwrap()[0].bytes().unwrap(),
        [0, 255, 10]
    );
    assert_eq!(native.attachments().unwrap()[1].bytes().unwrap(), b"second");
    let template = ProjectStore::open(root.path(), "project-1")
        .unwrap()
        .project()
        .unwrap()
        .settings()
        .task_template;
    let (_, dispatched) = zerus_replica::task_dispatch_content(
        &template,
        &native.view().unwrap(),
        "orchard",
        zerus_replica::Tools::Cli,
    )
    .unwrap();
    assert_eq!(dispatched[0].bytes().unwrap(), [0, 255, 10]);
    assert_eq!(dispatched[1].bytes().unwrap(), b"second");
    let before = saved(&root, &task.uuid);
    let mut bad = input(b"bad".to_vec());
    bad.name = "../escape.txt".into();
    refused(api.edit_content(
        task.uuid.clone(),
        "Must not be saved".into(),
        vec![],
        vec![bad],
        edited.content_revision.clone(),
    ));
    assert_eq!(saved(&root, &task.uuid), before);
    refused(api.edit_content(
        task.uuid.clone(),
        "Duplicate kept files".into(),
        vec![edited.attachments[0].token.clone(); 2],
        vec![],
        edited.content_revision.clone(),
    ));
    refused(api.edit_content(
        task.uuid.clone(),
        "Unknown kept file".into(),
        vec!["f".repeat(64)],
        vec![],
        edited.content_revision.clone(),
    ));
    assert_eq!(saved(&root, &task.uuid), before);
    let removed = api
        .edit_content(
            task.uuid.clone(),
            edited.text,
            vec![],
            vec![],
            edited.content_revision,
        )
        .unwrap();
    assert!(removed.attachments.is_empty());
    assert_eq!(
        removed.schema, 2,
        "removal never lowers the reader boundary"
    );
}

#[test]
fn legacy_cards_stay_schema_one_and_media_bounds_refuse_before_creation() {
    let root = tempfile::tempdir().unwrap();
    let api = phone(&root);
    let task = api
        .add_with_media(
            "A legacy card".into(),
            TaskStage::Todo,
            TaskTarget::AnySession,
            TaskPosition::End,
            vec![],
        )
        .unwrap();
    assert_eq!(task.schema, 1);
    assert!(task.attachments.is_empty());
    assert_eq!(task.content_revision.len(), 64);
    let many = (0..9)
        .map(|index| {
            let mut file = input(vec![1]);
            file.reference = format!("picked-{index}");
            file
        })
        .collect();
    let oversized_total = (0..3)
        .map(|index| {
            let mut file = input(vec![1; 7 * 1024 * 1024]);
            file.reference = format!("picked-{index}");
            file
        })
        .collect();
    for files in [
        vec![input(vec![])],
        many,
        vec![input(vec![1; 10 * 1024 * 1024 + 1])],
        oversized_total,
    ] {
        refused(api.add_with_media(
            "Must not create".into(),
            TaskStage::Todo,
            TaskTarget::AnySession,
            TaskPosition::End,
            files,
        ));
        assert_eq!(api.list().unwrap().tasks.len(), 1);
    }
    refused(api.edit_content(
        task.uuid,
        "No implicit revision fallback".into(),
        vec![],
        vec![],
        "".into(),
    ));
}

#[test]
fn a_late_history_budget_refusal_saves_neither_text_nor_files() {
    let root = tempfile::tempdir().unwrap();
    let uuid = legacy_seed(&root, false);
    let mut rejected = None;
    {
        let store = legacy_store(&root);
        let mut task = store.find(&uuid).unwrap();
        for generation in 1..=6 {
            let data = bytes(16 * 1024 * 1024, generation);
            let picked: Vec<_> = data
                .chunks(8 * 1024 * 1024)
                .enumerate()
                .map(|(index, data)| TaskMediaInput {
                    name: format!("history-{index}.bin"),
                    mime: "application/octet-stream".into(),
                    data: data.to_vec(),
                    reference: format!("history-{index}"),
                })
                .collect();
            let files: Vec<_> = picked
                .iter()
                .map(|file| TaskAttachment {
                    name: file.name.clone(),
                    mime: file.mime.clone(),
                    reference: file.reference.clone(),
                    data_base64: STANDARD.encode(&file.data),
                })
                .collect();
            match task.set_attachments(&files, &Actor::Person, generation as i64 + 10) {
                Ok(()) => store.save(&mut task).unwrap(),
                Err(error) => {
                    assert!(
                        error.to_string().contains("attachment history is full"),
                        "{error}"
                    );
                    rejected = Some(picked);
                    break;
                }
            }
        }
    }
    let picked = rejected.expect("the bounded fixture must reach the retained-history quota");
    let api = phone(&root);
    let task = api.get(uuid.clone()).unwrap();
    let before = Sha256::digest(serde_json::to_vec(&saved(&root, &uuid)).unwrap());
    let error = api
        .edit_content(
            uuid.clone(),
            "Never save this early".into(),
            vec![],
            picked,
            task.content_revision,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("attachment history is full"),
        "{error}"
    );
    let after = Sha256::digest(serde_json::to_vec(&saved(&root, &uuid)).unwrap());
    assert_eq!(
        after, before,
        "a late media validation failure must preserve the whole original task"
    );
}

#[test]
fn host_media_reaches_the_phone_in_bounded_pages_with_exact_preview_bytes() {
    let (computer_root, phone_root) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (computer, phone) = (phone(&computer_root), phone(&phone_root));
    let data = bytes(800 * 1024, 41);
    let task = computer
        .add_with_media(
            "Inspect the paged fixture".into(),
            TaskStage::Todo,
            TaskTarget::AnySession,
            TaskPosition::End,
            vec![input(data.clone())],
        )
        .unwrap();
    let exchange = phone.clone().exchange();
    let mut rounds = 0;
    let mut finished = false;
    for _ in 0..64 {
        let Some(request) = exchange.next_request().unwrap() else {
            finished = true;
            break;
        };
        assert!(request.len() <= 1024 * 1024);
        let answer = computer.answer_exchange(request).unwrap();
        assert!(answer.len() <= 1024 * 1024);
        let progress = exchange.accept(answer).unwrap();
        assert!(
            progress.refused_here.is_empty()
                && progress.refused_there.is_empty()
                && progress.unsent.is_empty()
        );
        rounds += 1;
        if rounds == 1 {
            computer
                .add(
                    "A row arriving during the transfer".into(),
                    TaskStage::Todo,
                    TaskTarget::AnySession,
                    TaskPosition::End,
                )
                .unwrap();
        }
        if progress.finished {
            finished = true;
            break;
        }
    }
    assert!(finished && rounds > 1);
    let loaded = phone.get(task.uuid.clone()).unwrap();
    assert_eq!(loaded.attachments, task.attachments);
    assert_eq!(
        phone
            .read_media(task.uuid, None, 0, loaded.attachments[0].token.clone())
            .unwrap()
            .data,
        data
    );
}
