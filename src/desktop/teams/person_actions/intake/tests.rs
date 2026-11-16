//! Canonical Team/Task rows use the product producers. Validated models the
//! separately admitted native child; these are not physical gesture evidence.
use super::*;
use crate::teams::ops::tests::{self as producers, Harness, TEAM};
use std::sync::atomic::{AtomicBool, Ordering};
use zerus_replica::{Actor, Position, Stage, Target};

fn sample() -> (Harness, Config, Scope) {
    let h = Harness::new();
    let config = Config {
        home: h.temp.path().display().to_string(),
        dir: h.temp.path().join("config"),
        state: h.temp.path().to_owned(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    crate::platform::private_dir(&config.dir).unwrap();
    crate::swarm::fixture_project(
        &config,
        &h.store.team(TEAM).unwrap().unwrap().home,
        "example-project",
    );
    let tx = h
        .store
        .db
        .as_ref()
        .unwrap()
        .unchecked_transaction()
        .unwrap();
    ledger::migrate(&tx).unwrap();
    tx.commit().unwrap();
    let scope = Scope {
        configuration: "a".repeat(64),
        service_epoch: uuid::Uuid::new_v4().to_string(),
    };
    (h, config, scope)
}
fn new() -> wire::TaskDraft {
    wire::TaskDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: TEAM.into(),
        action: wire::Action::NewTask {
            project: "example-project".into(),
            title: "Native original control".into(),
            backlog: false,
        },
    }
}
fn prepare_fixture(
    config: &Config,
    scope: &Scope,
    draft: wire::TaskDraft,
) -> (Arc<Prepared>, wire::Intent) {
    let value = review(config, scope, draft).unwrap();
    let plan: wire::Plan = serde_json::from_value(value["plan"].clone()).unwrap();
    let review: wire::Review = serde_json::from_value(value["review"].clone()).unwrap();
    (
        Arc::new(Prepared::resolve(config, Arc::new(plan)).unwrap()),
        review.intent,
    )
}
fn run(config: &Config, scope: &Scope, prepared: Arc<Prepared>, intent: &wire::Intent) -> Value {
    execute(
        config,
        scope.clone(),
        Arc::new(AtomicBool::new(false)),
        Validated::fixture(&json!({"op":"team_person_action","intent":intent})),
        prepared,
    )
    .unwrap()
}
fn document(config: &Config) -> (TaskRef, String) {
    let project = projects(config, "example-project").unwrap();
    let mut document = project
        .new_task(
            "Existing native control",
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::End,
            "example",
            1,
        )
        .unwrap();
    project.save(&mut document).unwrap();
    (
        TaskRef {
            project: "example-project".into(),
            uuid: document.uuid().into(),
        },
        document.revision(),
    )
}
fn existing(reference: &TaskRef, revision: &str) -> wire::TaskDraft {
    wire::TaskDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: TEAM.into(),
        action: wire::Action::Intake {
            project: reference.project.clone(),
            task: reference.uuid.clone(),
            revision: revision.into(),
            role: None,
            pipeline: None,
        },
    }
}

#[test]
fn native_intake_unknown_after_document_save_never_resumes_or_admits_a_second_uuid() {
    let (h, config, scope) = sample();
    let (reference, revision) = document(&config);
    let (prepared, intent) = prepare_fixture(&config, &scope, existing(&reference, &revision));
    AFTER_SAVE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(|custody| {
            assert!(
                ProjectStore::open_within(
                    custody.project.dir().parent().unwrap(),
                    "example-project",
                    Duration::ZERO
                )
                .map_err(store::storage)?
                .is_none(),
                "custody still holds the physical project lock"
            );
            Err(store::storage("model lost original after document save"))
        }))
    });
    let answer = run(&config, &scope, prepared.clone(), &intent);
    assert_eq!(answer["reply"]["code"], "team_person_action_unknown");
    let saved = ledger::original(h.store.db.as_ref().unwrap(), &intent).unwrap();
    assert!(saved.result.is_none());
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    let before = load(
        &projects(&config, &reference.project).unwrap(),
        &reference.uuid,
    )
    .unwrap()
    .revision();
    assert!(execute(
        &config,
        scope.clone(),
        Arc::new(AtomicBool::new(false)),
        Validated::fixture(&json!({"op":"team_person_action","intent":intent})),
        prepared
    )
    .is_err());
    let (next, next_intent) = prepare_fixture(&config, &scope, existing(&reference, &before));
    assert!(execute(
        &config,
        scope,
        Arc::new(AtomicBool::new(false)),
        Validated::fixture(&json!({"op":"team_person_action","intent":next_intent})),
        next
    )
    .is_err());
    assert_eq!(
        load(
            &projects(&config, &reference.project).unwrap(),
            &reference.uuid
        )
        .unwrap()
        .revision(),
        before
    );
}

#[test]
fn native_intake_reservation_precedes_every_new_task_write() {
    let (h, config, scope) = sample();
    let draft = new();
    let (prepared, intent) = prepare_fixture(&config, &scope, draft.clone());
    let state = config.state.clone();
    AFTER_RESERVE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |custody| {
            assert!(custody.document.is_none());
            crate::tasks::native_team::absent(&custody.project, &draft.request_id)?;
            let store = Store::person_evidence(&state)?;
            assert!(
                ledger::original(store.db.as_ref().unwrap(), &custody.intent)?
                    .result
                    .is_none()
            );
            Err(store::storage(
                "model crash before the first document write",
            ))
        }))
    });
    assert_eq!(
        run(&config, &scope, prepared, &intent)["reply"]["code"],
        "team_person_action_unknown"
    );
    assert!(ledger::original(h.store.db.as_ref().unwrap(), &intent)
        .unwrap()
        .result
        .is_none());
    let project = projects(&config, "example-project").unwrap();
    crate::tasks::native_team::absent(&project, &intent.plan.request_id).unwrap();
}

#[test]
fn native_intake_expiry_after_document_save_restores_binding_and_records_only_the_partial_outcome()
{
    let (h, config, scope) = sample();
    let (prepared, intent) = prepare_fixture(&config, &scope, new());
    let expired = Arc::new(AtomicBool::new(false));
    let flip = expired.clone();
    AFTER_SAVE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |_| {
            flip.store(true, Ordering::Release);
            Ok(())
        }))
    });
    let result = execute(
        &config,
        scope,
        Arc::new(AtomicBool::new(false)),
        Validated::fixture_clock(&json!({"op":"team_person_action","intent":intent}), expired),
        prepared,
    )
    .unwrap();
    assert_eq!(result["reply"]["task_exists"], true, "{result}");
    assert_eq!(result["reply"]["team"]["queued"], false);
    assert_eq!(result["reply"]["code"], "person_gesture_required");
    let project = projects(&config, "example-project").unwrap();
    assert!(load(&project, &intent.plan.request_id)
        .unwrap()
        .team()
        .unwrap()
        .is_none());
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    assert_eq!(
        ledger::original(h.store.db.as_ref().unwrap(), &intent)
            .unwrap()
            .result,
        Some(result["reply"].clone())
    );
}

#[test]
fn native_intake_cancelled_node_is_settled_before_one_replacement_with_exact_original_revision() {
    let (mut h, config, scope) = sample();
    let (reference, _) = document(&config);
    let node = h
        .run(
            &producers::person(),
            ops::Operation::AddTask {
                task: reference.clone(),
                role: None,
                pipeline: None,
            },
        )
        .unwrap()
        .node
        .unwrap();
    h.bind("lead");
    h.bind("m1");
    let team = h.store.team(TEAM).unwrap().unwrap();
    {
        let project = projects(&config, &reference.project).unwrap();
        let mut doc = load(&project, &reference.uuid).unwrap();
        doc.set_team_named(
            Some(&zerus_replica::TaskTeam {
                team: TEAM.into(),
                home: team.home.clone(),
            }),
            &team.name,
            &Actor::Person,
            2,
        )
        .unwrap();
        project.save(&mut doc).unwrap();
        native::record_saved(&mut h.store, TEAM, node, &doc).unwrap();
    }
    let effect = h.intent(node, "m1");
    let revision = {
        let project = projects(&config, &reference.project).unwrap();
        let mut doc = load(&project, &reference.uuid).unwrap();
        let executor = zerus_replica::SessionKey {
            machine: team.home.clone(),
            session: "codex/native-control".into(),
            session_id: "run_id:native-control".into(),
        };
        doc.reserve_attempt(
            &effect,
            zerus_replica::execution::Origin::Send,
            &executor,
            zerus_replica::execution::NativeRequest {
                machine: team.home,
                name: executor.session.clone(),
                run_id: "native-control".into(),
                conversation_id: uuid::Uuid::new_v4().to_string(),
                digest: "d".repeat(64),
            },
            &Actor::Person,
            3,
        )
        .unwrap();
        // The product's exact disposition producer ends this Unknown attempt;
        // it performs no native input, and the card remains in To do.
        doc.cancel_attempt(
            &effect,
            "Abandon the private model attempt",
            &Actor::Person,
            4,
        )
        .unwrap();
        project.save(&mut doc).unwrap();
        doc.revision()
    };
    let (prepared, intent) = prepare_fixture(&config, &scope, existing(&reference, &revision));
    let value = run(&config, &scope, prepared, &intent);
    assert_eq!(value["reply"]["team"]["queued"], true, "{value}");
    assert_ne!(value["reply"]["team"]["node"], json!(node));
    let rows = h.store.queue(TEAM, 200, None).unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_ne!(rows[0].id, node);
}

#[test]
fn native_intake_read_draft_freezes_the_task_clock_and_changed_documents_only_record_refusal() {
    let (h, config, scope) = sample();
    let (reference, revision) = document(&config);
    let (prepared, intent) = prepare_fixture(&config, &scope, existing(&reference, ""));
    assert!(
        matches!(&prepared.plan.action,wire::Action::Intake {revision:expected,..} if expected==&revision)
    );
    {
        let project = projects(&config, &reference.project).unwrap();
        let mut doc = load(&project, &reference.uuid).unwrap();
        doc.note(
            "The document changed after native review",
            &Actor::Person,
            5,
        )
        .unwrap();
        project.save(&mut doc).unwrap();
    }
    let project = projects(&config, &reference.project).unwrap();
    let before = load(&project, &reference.uuid).unwrap().revision();
    drop(project);
    let reply = execute(
        &config,
        scope,
        Arc::new(AtomicBool::new(false)),
        Validated::fixture(&json!({"op":"team_person_action","intent":intent})),
        prepared,
    )
    .unwrap();
    assert_eq!(reply["reply"]["code"], code::STALE_REVISION);
    assert_eq!(reply["reply"]["team"]["queued"], false);
    assert_eq!(
        ledger::original(h.store.db.as_ref().unwrap(), &intent)
            .unwrap()
            .result,
        Some(reply["reply"].clone())
    );
    let project = projects(&config, &reference.project).unwrap();
    assert_eq!(
        load(&project, &reference.uuid).unwrap().revision(),
        before,
        "the closed refusal makes no document edit"
    );
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn native_intake_expiry_before_first_save_never_creates_a_task_during_refusal_cleanup() {
    let (h, config, scope) = sample();
    let (prepared, intent) = prepare_fixture(&config, &scope, new());
    let expired = Arc::new(AtomicBool::new(false));
    let flip = expired.clone();
    BEFORE_SAVE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |custody| {
            assert!(!custody.saved);
            flip.store(true, Ordering::Release);
            Ok(())
        }))
    });
    let result = execute(
        &config,
        scope,
        Arc::new(AtomicBool::new(false)),
        Validated::fixture_clock(&json!({"op":"team_person_action","intent":intent}), expired),
        prepared,
    )
    .unwrap();
    assert_eq!(result["reply"]["task_exists"], false, "{result}");
    assert_eq!(result["reply"]["team"]["queued"], false);
    let project = projects(&config, "example-project").unwrap();
    crate::tasks::native_team::absent(&project, &intent.plan.request_id).unwrap();
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn native_intake_final_consumption_refusal_never_reports_the_uncommitted_success() {
    let (h, config, scope) = sample();
    let (prepared, intent) = prepare_fixture(&config, &scope, new());
    let expired = Arc::new(AtomicBool::new(false));
    let flip = expired.clone();
    BEFORE_CONSUME.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |custody| {
            assert!(
                ProjectStore::open_within(
                    custody.project.dir().parent().unwrap(),
                    "example-project",
                    Duration::ZERO
                )
                .map_err(store::storage)?
                .is_none(),
                "project custody must survive all writer rows until actual commit"
            );
            flip.store(true, Ordering::Release);
            Ok(())
        }))
    });
    let result = execute(
        &config,
        scope,
        Arc::new(AtomicBool::new(false)),
        Validated::fixture_clock(&json!({"op":"team_person_action","intent":intent}), expired),
        prepared,
    )
    .unwrap();
    assert_eq!(result["reply"]["team"]["queued"], false, "{result}");
    assert_eq!(result["reply"]["code"], "person_gesture_required");
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    let project = projects(&config, "example-project").unwrap();
    assert!(load(&project, &intent.plan.request_id)
        .unwrap()
        .team()
        .unwrap()
        .is_none());
    assert_eq!(
        ledger::original(h.store.db.as_ref().unwrap(), &intent)
            .unwrap()
            .result,
        Some(result["reply"].clone())
    );
}

#[test]
fn native_intake_observation_receipt_failure_keeps_the_whole_queue_commit_unknown() {
    let (h, config, scope) = sample();
    let (prepared, intent) = prepare_fixture(&config, &scope, new());
    let state = config.state.clone();
    AFTER_SAVE.with(|hook|*hook.borrow_mut()=Some(Box::new(move |_|{
        // Fault injection only: canonical rows still come from the writers.
        let store=Store::open(&state)?;
        store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER native_observation_receipt_failure BEFORE INSERT ON requests WHEN json_extract(NEW.content,'$.op')='observe_task' BEGIN SELECT RAISE(ABORT,'private observer receipt failure'); END;").map_err(store::storage)?;
        Ok(())
    })));
    let answer = run(&config, &scope, prepared, &intent);
    assert_eq!(
        answer["reply"]["code"], "team_person_action_unknown",
        "{answer}"
    );
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    assert!(ledger::original(h.store.db.as_ref().unwrap(), &intent)
        .unwrap()
        .result
        .is_none());
}

#[test]
fn native_intake_stale_team_records_a_closed_no_task_refusal_without_granting_custody() {
    let (mut h, config, scope) = sample();
    let (prepared, intent) = prepare_fixture(&config, &scope, new());
    h.run(
        &producers::person(),
        ops::Operation::RenameTeam {
            name: "Changed after review".into(),
        },
    )
    .unwrap();
    let result = run(&config, &scope, prepared, &intent);
    assert_eq!(result["reply"]["code"], code::STALE_REVISION, "{result}");
    assert_eq!(result["reply"]["task_exists"], false);
    let project = projects(&config, "example-project").unwrap();
    crate::tasks::native_team::absent(&project, &intent.plan.request_id).unwrap();
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    assert_eq!(
        ledger::original(h.store.db.as_ref().unwrap(), &intent)
            .unwrap()
            .result,
        Some(result["reply"].clone())
    );
}
