use super::*;
use crate::teams::ops::{
    self,
    tests::{fixture, person, Harness, TEAM},
};
use zerus_replica::{Actor as TaskActor, Position, ProjectStore, Stage, Target, TaskDoc};
use zerus_team_plan::model::{NodeState, TaskRef};

/// The direct tests below have no command project lock open; the meeting
/// still writes through one caller-owned store, never a second one.
fn guard(caller: &Caller<'_>, doc: &TaskDoc, action: &str) -> Result<()> {
    let project = doc.view().unwrap().project;
    let store = ProjectStore::open(&caller.state.join("replica"), &project).unwrap();
    let mut meeting = Meeting::Projects(&store);
    super::guard(caller, &mut meeting, doc, action)
}

fn caller<'a>(h: &'a Harness, actor: &'a TaskActor) -> Caller<'a> {
    Caller {
        state: h.temp.path(),
        machine: "00000000-0000-4000-8000-0000000000aa",
        actor: actor.clone(),
    }
}

fn document(h: &Harness) -> TaskRef {
    let store = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
    let mut task = store
        .new_task(
            "Implement the change",
            Stage::Todo,
            Target::Any,
            TaskActor::Person,
            &Position::End,
            "example",
            1,
        )
        .unwrap();
    let task_ref = TaskRef {
        project: "example-project".into(),
        uuid: task.uuid().into(),
    };
    store.save(&mut task).unwrap();
    task_ref
}

fn task(h: &Harness, reference: &TaskRef) -> TaskDoc {
    ProjectStore::open(&h.temp.path().join("replica"), &reference.project)
        .unwrap()
        .load(&reference.uuid)
        .unwrap()
}

fn agent(actor: ops::Actor) -> TaskActor {
    let ops::Actor::Session { session, .. } = actor else {
        panic!("bound actor")
    };
    TaskActor::Agent(zerus_replica::SessionKey {
        machine: session.machine,
        session_id: session.session_id,
        session: "codex/renamed-label".into(),
    })
}

#[test]
fn sends_twice_share_one_node_and_service_reads_the_document() {
    let h = Harness::new();
    let reference = document(&h);
    let first = send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let second = send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    assert_eq!(first.node, second.node);
    let store = super::super::store::Store::open(h.temp.path()).unwrap();
    let nodes = store.queue(TEAM, 200, None).unwrap().rows;
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].state, NodeState::Pending);
    let doc = task(&h, &reference);
    let observed = store.task_observation(TEAM, nodes[0].id).unwrap().unwrap();
    assert_eq!(observed.revision, doc.revision());
    assert_eq!(observed.rank, doc.rank());
    observe(h.temp.path(), &reference).unwrap();
    assert_eq!(doc.view().unwrap().target, Target::Any);
    assert_eq!(doc.team().unwrap().unwrap().team, TEAM);
    let actors = store.journal(TEAM, 200, None).unwrap().rows;
    assert!(actors
        .iter()
        .any(|row| row.kind == "observe_task" && row.actor["kind"] == "service"));
    assert!(actors
        .iter()
        .any(|row| row.kind == "add_task" && row.actor["kind"] == "person"));
}

#[test]
fn admission_and_suggestions_use_the_verified_callers_binding() {
    let mut h = Harness::new();
    let lead = agent(h.bind("lead"));
    let member = agent(h.bind("m1"));
    let foreign = TaskActor::Agent(zerus_replica::SessionKey {
        machine: "00000000-0000-4000-8000-0000000000aa".into(),
        session_id: "run_id:outside".into(),
        session: "codex/other".into(),
    });
    for (actor, admitted) in [(&lead, true), (&member, false), (&foreign, false)] {
        let reference = document(&h);
        let reply = send(
            &caller(&h, actor),
            &reference,
            TEAM,
            Some("developer"),
            Some("reviewed"),
        )
        .unwrap();
        let store = super::super::store::Store::open(h.temp.path()).unwrap();
        let node = store.node(TEAM, reply.node.unwrap()).unwrap().unwrap();
        assert_eq!(node.admitted, admitted);
        assert_eq!(
            store.suggestion(TEAM, node.id).unwrap().is_some(),
            !admitted
        );
    }
}

#[test]
fn guarded_team_tasks_refuse_every_agent_and_allow_the_person() {
    let mut h = Harness::new();
    let lead = agent(h.bind("lead"));
    let member = agent(h.bind("m1"));
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let doc = task(&h, &reference);
    for action in ["send", "handoff", "done"] {
        assert!(guard(&caller(&h, &member), &doc, action).is_err());
        assert!(guard(&caller(&h, &lead), &doc, action).is_err());
        assert!(guard(&caller(&h, &TaskActor::Person), &doc, action).is_ok());
        let mut remote = caller(&h, &lead);
        remote.machine = "00000000-0000-4000-8000-0000000000cc";
        assert!(guard(&remote, &doc, action).is_err());
        let mut person = caller(&h, &TaskActor::Person);
        person.machine = remote.machine;
        assert!(guard(&person, &doc, action).is_ok());
    }
    // Display-name reuse grants nothing.
    let mut impostor = lead.clone();
    if let TaskActor::Agent(key) = &mut impostor {
        key.session_id = "run_id:replacement".into();
    }
    assert!(guard(&caller(&h, &impostor), &doc, "send").is_err());
}

#[test]
fn a_task_without_a_key_is_not_guarded_even_with_a_stored_node() {
    let mut h = Harness::new();
    let member = agent(h.bind("m1"));
    let reference = document(&h);
    h.run(
        &person(),
        ops::Operation::AddTask {
            task: reference.clone(),
            role: None,
            pipeline: None,
        },
    )
    .unwrap();
    let doc = task(&h, &reference);
    assert_eq!(doc.team().unwrap(), None);
    for action in ["send", "handoff", "done"] {
        assert!(guard(&caller(&h, &member), &doc, action).is_ok());
    }
    h.run(&person(), ops::Operation::Stop).unwrap();
    assert!(guard(&caller(&h, &member), &doc, "send").is_ok());
}

#[test]
fn preview_off_does_not_write_intake_and_ordinary_tasks_stay_unchanged() {
    let h = Harness::new();
    let reference = document(&h);
    let before = task(&h, &reference).revision();
    std::fs::remove_file(h.temp.path().join("teams/preview")).unwrap();
    let error = send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, "teams_inactive");
    assert_eq!(task(&h, &reference).revision(), before);
    assert!(guard(
        &caller(&h, &TaskActor::Person),
        &task(&h, &reference),
        "send"
    )
    .is_ok());
}

#[test]
fn service_observation_records_a_persons_override_from_disk() {
    let mut h = Harness::new();
    h.settings(|s| s.integration = zerus_team_plan::model::Integration::None);
    let reference = document(&h);
    let reply = send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    {
        let store = ProjectStore::open(&h.temp.path().join("replica"), &reference.project).unwrap();
        let mut doc = store.load(&reference.uuid).unwrap();
        let session = zerus_replica::SessionKey {
            machine: fixture().team.home,
            session: "codex/example".into(),
            session_id: "run_id:example".into(),
        };
        doc.record_send(&session, "request", &TaskActor::Person, 2)
            .unwrap();
        doc.report(
            "Ready",
            zerus_replica::execution::ReportKind::Ready,
            None,
            &TaskActor::Person,
            3,
        )
        .unwrap();
        doc.set_stage(Stage::Done, &TaskActor::Person, 4).unwrap();
        store.save(&mut doc).unwrap();
    }
    observe(h.temp.path(), &reference).unwrap();
    let store = super::super::store::Store::open(h.temp.path()).unwrap();
    assert_eq!(
        store
            .node(TEAM, reply.node.unwrap())
            .unwrap()
            .unwrap()
            .state,
        NodeState::Accepted
    );
}

#[test]
fn unmarked_tasks_never_open_the_team_store_and_bad_keys_are_guarded() {
    let mut h = Harness::new();
    let member = agent(h.bind("m1"));
    let reference = document(&h);
    let mut doc = task(&h, &reference);
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(elsewhere.path().join("teams")).unwrap();
    std::fs::write(elsewhere.path().join("teams/preview"), "on").unwrap();
    std::fs::write(
        elsewhere.path().join("teams/teams-v1.sqlite3"),
        "invalid database",
    )
    .unwrap();
    let mut calling = caller(&h, &member);
    calling.state = elsewhere.path();
    assert!(guard(&calling, &doc, "send").is_ok());
    doc.set_raw_team_for_test("not JSON");
    assert!(guard(&calling, &doc, "send").is_err());
    calling.actor = TaskActor::Person;
    assert!(guard(&calling, &doc, "send").is_ok());
}

#[test]
fn home_keys_of_stopped_and_archived_teams_do_not_guard_even_a_retained_binding() {
    for state in ["stopped", "archived"] {
        let mut h = Harness::new();
        let member = agent(h.bind("m1"));
        let reference = document(&h);
        send(
            &caller(&h, &TaskActor::Person),
            &reference,
            TEAM,
            None,
            None,
        )
        .unwrap();
        let doc = task(&h, &reference);
        h.store
            .db
            .as_ref()
            .unwrap()
            .execute("UPDATE teams SET status=?1", [format!("\"{state}\"")])
            .unwrap();
        assert!(guard(&caller(&h, &member), &doc, "send").is_ok(), "{state}");
    }
}

#[test]
fn only_the_delivered_attempt_holder_can_acknowledge_or_register_a_worker() {
    use zerus_replica::execution::{NativeRequest, Origin, Receipt};
    let mut h = Harness::new();
    let lead = agent(h.bind("lead"));
    let member = agent(h.bind("m1"));
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let mut doc = task(&h, &reference);
    let TaskActor::Agent(key) = &member else {
        unreachable!()
    };
    let id = uuid::Uuid::new_v4().to_string();
    let digest = "a".repeat(64);
    doc.reserve_attempt(
        &id,
        Origin::Send,
        key,
        NativeRequest {
            machine: key.machine.clone(),
            name: key.session.clone(),
            run_id: "fixture-run".into(),
            conversation_id: "fixture-conversation".into(),
            digest: digest.clone(),
        },
        &TaskActor::Person,
        2,
    )
    .unwrap();
    doc.settle_attempt(
        &id,
        zerus_replica::Settlement::Receipt(&Receipt {
            request_id: id.clone(),
            name: key.session.clone(),
            run_id: "fixture-run".into(),
            conversation_id: "fixture-conversation".into(),
            digest,
            submitted: true,
            first_message: false,
        }),
        &TaskActor::Person,
        3,
    )
    .unwrap();
    for action in ["start", "worker"] {
        assert!(guard(&caller(&h, &member), &doc, action).is_ok());
        assert!(guard(&caller(&h, &lead), &doc, action).is_err());
    }
    for action in ["send", "handoff", "stage", "target", "reopen"] {
        assert!(guard(&caller(&h, &member), &doc, action).is_err());
    }
    for action in ["comment", "report", "report-attempt"] {
        assert!(guard(&caller(&h, &lead), &doc, action).is_ok());
    }
}

#[test]
fn terminal_team_cleanup_clears_only_the_expected_marker() {
    let mut h = Harness::new();
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    h.run(&person(), ops::Operation::Stop).unwrap();
    let before = h.store.team(TEAM).unwrap().unwrap().revision;
    assert!(!clear_team_key(
        h.temp.path(),
        &reference,
        "00000000-0000-4000-8000-000000000099",
        &TaskActor::Person
    )
    .unwrap());
    assert!(task(&h, &reference).team().unwrap().is_some());
    assert!(clear_team_key(h.temp.path(), &reference, TEAM, &TaskActor::Person).unwrap());
    let doc = task(&h, &reference);
    assert_eq!(doc.team().unwrap(), None);
    assert!(doc
        .view()
        .unwrap()
        .log
        .last()
        .unwrap()
        .text
        .starts_with("Removed from team "));
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, before);
}

#[test]
fn local_preview_off_is_dormant_but_foreign_and_invalid_markers_still_guard_without_io() {
    let mut h = Harness::new();
    let member = agent(h.bind("m1"));
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let mut doc = task(&h, &reference);
    let private = tempfile::tempdir().unwrap();
    // No preview, and a corrupt store: any attempt to open it is observable.
    std::fs::create_dir(private.path().join("teams")).unwrap();
    std::fs::write(private.path().join("teams/teams-v1.sqlite3"), "corrupt").unwrap();
    let mut calling = caller(&h, &member);
    calling.state = private.path();
    assert!(guard(&calling, &doc, "stage").is_ok());
    // A different home with no corresponding team in this computer's store.
    doc.set_team(
        Some(&zerus_replica::TaskTeam {
            team: uuid::Uuid::new_v4().to_string(),
            home: uuid::Uuid::new_v4().to_string(),
        }),
        &TaskActor::Person,
        5,
    )
    .unwrap();
    for preview in [false, true] {
        if preview {
            std::fs::write(private.path().join("teams/preview"), "on").unwrap();
        }
        assert_eq!(
            guard(&calling, &doc, "stage").unwrap_err().code,
            code::NOT_ALLOWED
        );
    }
    doc.set_raw_team_for_test("invalid JSON");
    assert_eq!(
        guard(&calling, &doc, "stage").unwrap_err().code,
        code::NOT_ALLOWED
    );
    assert_eq!(
        std::fs::read_to_string(private.path().join("teams/teams-v1.sqlite3")).unwrap(),
        "corrupt"
    );
}

#[test]
fn the_holder_of_a_legacy_dispatch_can_acknowledge_and_report_but_cannot_drive_the_team() {
    let mut h = Harness::new();
    let member = agent(h.bind("m1"));
    let lead = agent(h.bind("lead"));
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let mut doc = task(&h, &reference);
    let TaskActor::Agent(key) = &member else {
        unreachable!()
    };
    doc.record_send(key, "legacy-request", &lead, 2).unwrap();
    for action in ["start", "worker"] {
        assert!(
            guard(&caller(&h, &member), &doc, action).is_ok(),
            "{action}"
        );
        assert!(guard(&caller(&h, &lead), &doc, action).is_err());
    }
    for action in ["cancel", "cancel-execution", "move", "edit", "rename"] {
        assert!(
            guard(&caller(&h, &member), &doc, action).is_err(),
            "{action}"
        );
        assert!(guard(&caller(&h, &lead), &doc, action).is_err(), "{action}");
    }
    doc.record_start(&member, 3).unwrap();
    assert!(guard(&caller(&h, &member), &doc, "report").is_ok());
    doc.report(
        "Ready",
        zerus_replica::execution::ReportKind::Ready,
        None,
        &member,
        4,
    )
    .unwrap();
    assert_eq!(doc.stage().unwrap(), Stage::Review);
}

#[test]
fn service_releases_a_live_node_when_the_replicated_key_is_absent_or_other() {
    for mode in ["absent", "other_team", "other_home", "invalid", "ours"] {
        let h = Harness::new();
        let reference = document(&h);
        let node = send(
            &caller(&h, &TaskActor::Person),
            &reference,
            TEAM,
            None,
            None,
        )
        .unwrap()
        .node
        .unwrap();
        {
            let store =
                ProjectStore::open(&h.temp.path().join("replica"), &reference.project).unwrap();
            let mut doc = store.load(&reference.uuid).unwrap();
            let key = zerus_replica::TaskTeam {
                team: if mode == "other_team" {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    TEAM.into()
                },
                home: if mode == "other_home" {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    fixture().team.home
                },
            };
            if mode == "invalid" {
                doc.set_raw_team_for_test("not JSON");
            } else {
                doc.set_team((mode != "absent").then_some(&key), &TaskActor::Person, 5)
                    .unwrap();
            }
            store.save(&mut doc).unwrap();
        }
        let before = task(&h, &reference).revision();
        observe(h.temp.path(), &reference).unwrap();
        let expected = match mode {
            "absent" => ops::KeyState::Absent,
            "ours" => ops::KeyState::Ours,
            _ => ops::KeyState::Other,
        };
        let observation = h.store.task_observation(TEAM, node).unwrap().unwrap();
        assert_eq!(observation.key, expected, "{mode}");
        assert_eq!(observation.revision, before);
        assert_eq!(task(&h, &reference).revision(), before);
        assert_eq!(
            h.store.queue(TEAM, 200, None).unwrap().rows.is_empty(),
            mode != "ours",
            "{mode}"
        );
    }
}

#[test]
fn foreign_home_is_guarded_when_this_computers_store_has_no_team() {
    let h = Harness::empty();
    let reference = document(&h);
    let mut doc = task(&h, &reference);
    doc.set_team(
        Some(&zerus_replica::TaskTeam {
            team: uuid::Uuid::new_v4().to_string(),
            home: uuid::Uuid::new_v4().to_string(),
        }),
        &TaskActor::Person,
        2,
    )
    .unwrap();
    let foreign = TaskActor::Agent(zerus_replica::SessionKey {
        machine: fixture().team.home,
        session_id: "run_id:caller".into(),
        session: "codex/caller".into(),
    });
    for action in [
        "send", "handoff", "stage", "start", "worker", "cancel", "edit", "rename", "move",
    ] {
        assert_eq!(
            guard(&caller(&h, &foreign), &doc, action).unwrap_err().code,
            code::NOT_ALLOWED
        );
    }
    assert!(h
        .store
        .team(doc.team().unwrap().unwrap().team.as_str())
        .unwrap()
        .is_none());
    assert!(!h.temp.path().join("teams/teams-v1.sqlite3").exists());
}

#[test]
fn a_legacy_direct_claim_does_not_gain_a_delivered_holders_exception() {
    let mut h = Harness::new();
    let member = agent(h.bind("m1"));
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let mut doc = task(&h, &reference);
    doc.record_start(&member, 2).unwrap();
    for action in ["start", "worker"] {
        assert!(guard(&caller(&h, &member), &doc, action).is_err());
    }
}

mod attempts;
pub(super) mod intake_errors;
fn person_session() -> zerus_replica::SessionKey {
    zerus_replica::SessionKey {
        machine: fixture().team.home,
        session: "codex/example".into(),
        session_id: "run_id:example".into(),
    }
}
fn edit(h: &Harness, reference: &TaskRef, change: impl FnOnce(&mut TaskDoc)) {
    let store = ProjectStore::open(&h.temp.path().join("replica"), &reference.project).unwrap();
    let mut doc = store.load(&reference.uuid).unwrap();
    change(&mut doc);
    store.save(&mut doc).unwrap();
}

/// The shape end to end, through the service's own observer: the command
/// answers cancelled; that answer is refused for the bound live node and
/// writes nothing; the service observes the task, and the observation of the
/// cancelled task ends the node and settles the decision. A resend of the
/// still-bound cancelled task is refused by its stage and writes nothing
/// (zerus-63rru.36 round 3); the To do resend queues.
#[test]
fn a_cancelled_task_is_ended_by_its_observation() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let reference = document(&h);
    let node = send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap()
    .node
    .unwrap();
    edit(&h, &reference, |doc| {
        doc.record_send(&person_session(), "request-1", &TaskActor::Person, 2)
            .unwrap()
    });
    observe(h.temp.path(), &reference).unwrap();
    h.run(
        &lead,
        ops::Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    let decision = h
        .store
        .pending_cancels(TEAM, 1, 0)
        .unwrap()
        .remove(0)
        .decision;
    // The command's work on the task, then its answer.
    edit(&h, &reference, |doc| {
        doc.record_cancel("Superseded", &TaskActor::Person, 3)
            .unwrap()
    });
    let answer = h.run(
        &ops::tests::service(),
        ops::Operation::RecordCancel {
            node,
            decision,
            state: ops::CancelState::Cancelled,
            reason: String::new(),
            attempt: 0,
            attempt_id: String::new(),
        },
    );
    assert_eq!(answer.unwrap_err().code, code::INVALID_STATE);
    assert_eq!(h.store.pending_cancels(TEAM, 200, 0).unwrap().len(), 1);
    assert_eq!(h.node(node).state, NodeState::InProgress);
    assert_eq!(
        binding(&h.store, &reference).unwrap(),
        Some((TEAM.to_owned(), node))
    );
    let again = send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    );
    assert_eq!(again.unwrap_err().code, code::INVALID_STATE);
    assert_eq!(
        task(&h, &reference).team().unwrap().unwrap().team,
        TEAM,
        "the refused send leaves the key"
    );
    assert_eq!(h.node(node).state, NodeState::InProgress, "nothing was written");
    assert_eq!(
        binding(&h.store, &reference).unwrap(),
        Some((TEAM.to_owned(), node))
    );
    observe(h.temp.path(), &reference).unwrap();
    assert_eq!(h.node(node).state, NodeState::Cancelled);
    assert!(h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
    assert_eq!(binding(&h.store, &reference).unwrap(), None);
}

/// A task-store admission another caller holds past this call's deadline is
/// a busy non-start, not a storage failure: nothing was read or written and
/// the same call may repeat (zerus-63rru.35).
#[test]
fn a_task_store_admission_that_cannot_be_taken_answers_store_busy() {
    let h = Harness::new();
    let reference = document(&h);
    let store = ProjectStore::open(&h.temp.path().join("replica"), &reference.project).unwrap();
    let held = store
        .admit_until(std::time::Instant::now() + std::time::Duration::from_secs(30))
        .expect("the store is free");
    let problem = super::load(&store, &reference.uuid).err().expect("busy");
    assert_eq!(problem.code, code::STORE_BUSY);
    drop(held);
    super::load(&store, &reference.uuid).unwrap();
}
