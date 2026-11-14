use super::super::super::super::native::cancel_answer;
use super::*;
use crate::desktop::teams::child::Ended;
use zerus_replica::execution::{NativeRequest, Origin, Receipt, ReportKind};

pub(super) fn run() {
    for stage in [Stage::Backlog, Stage::Todo, Stage::Review] {
        released(stage);
    }
    already_cancelled();
    refused_done();
    stale();
    terminal_refuses_an_unobserved_person_attempt();
}
fn decide(f: &mut Fixture) {
    f.h.run(
        &person(),
        Operation::Cancel {
            node: f.node,
            reason: "Cancel the observed work".into(),
        },
    )
    .unwrap();
}
fn check(f: &Fixture, args: &[String], input: &Value, state: &str) -> Value {
    let value = f.call(args, input, false);
    assert_eq!(value["state"], state, "{value}");
    let answer = cancel_answer(
        Ended::Exited {
            code: 0,
            output: value.to_string().into_bytes(),
        },
        input,
    )
    .unwrap();
    if matches!(state, "released" | "cancelled") {
        let attempt = answer.attempt.expect("successful cancel has no attempt");
        assert_eq!(value["attempt"], attempt.number);
        assert_eq!(value["attempt_id"], attempt.id);
    }
    value
}
fn released(stage: Stage) {
    let mut f = Fixture::new();
    if stage == Stage::Review {
        f.submitted();
        f.reported();
    } else if stage != Stage::Todo {
        let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
        let mut doc = tasks.load(&f.task.uuid).unwrap();
        doc.set_stage(stage, &TaskActor::Person, 2).unwrap();
        tasks.save(&mut doc).unwrap();
        drop(tasks);
        crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    }
    decide(&mut f);
    let (args, input) = f.cancel_vector();
    let before = f.inputs();
    for _ in 0..2 {
        check(&f, &args, &input, "released");
    }
    assert_eq!(f.inputs(), before);
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    let doc = tasks.load(&f.task.uuid).unwrap();
    assert_eq!(doc.view().unwrap().stage, stage);
    assert!(doc.team().unwrap().is_none());
}
fn already_cancelled() {
    let mut f = Fixture::new();
    f.submitted();
    decide(&mut f);
    let (args, input) = f.cancel_vector();
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    let mut doc = tasks.load(&f.task.uuid).unwrap();
    doc.record_cancel("Person cancelled it", &TaskActor::Person, 3)
        .unwrap();
    tasks.save(&mut doc).unwrap();
    drop(tasks);
    for _ in 0..2 {
        check(&f, &args, &input, "cancelled");
    }
    assert_eq!(f.inputs(), 1);
}
fn person_attempt(f: &Fixture) {
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    let mut doc = tasks.load(&f.task.uuid).unwrap();
    let member =
        f.h.store
            .member(TEAM, "m1")
            .unwrap()
            .unwrap()
            .session
            .unwrap();
    let to = SessionKey {
        machine: member.machine,
        session_id: member.session_id,
        session: "codex/command-m1".into(),
    };
    let native = NativeRequest {
        machine: to.machine.clone(),
        name: to.session.clone(),
        run_id: "run-m1".into(),
        conversation_id: "conversation-m1".into(),
        digest: "a".repeat(64),
    };
    let id = uuid::Uuid::new_v4().to_string();
    doc.reserve_attempt(
        &id,
        Origin::Send,
        &to,
        native.clone(),
        &TaskActor::Person,
        4,
    )
    .unwrap();
    doc.settle_attempt(
        &id,
        zerus_replica::Settlement::Receipt(&Receipt {
            request_id: id.clone(),
            name: native.name,
            run_id: native.run_id,
            conversation_id: native.conversation_id,
            digest: native.digest,
            first_message: false,
            submitted: true,
        }),
        &TaskActor::Person,
        5,
    )
    .unwrap();
    doc.report_attempt(
        &id,
        "Person's result",
        ReportKind::Ready,
        None,
        &TaskActor::Agent(to),
        6,
    )
    .unwrap();
    tasks.save(&mut doc).unwrap();
}
fn stale() {
    let mut f = Fixture::new();
    decide(&mut f);
    let (args, input) = f.cancel_vector();
    person_attempt(&f);
    check(&f, &args, &input, "stale");
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    check(&f, &args, &input, "released");
    assert_eq!(f.inputs(), 0);
}

fn refused_done() {
    let mut f = Fixture::new();
    f.submitted();
    f.reported();
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    let mut doc = tasks.load(&f.task.uuid).unwrap();
    doc.set_stage(Stage::Done, &TaskActor::Person, 7).unwrap();
    tasks.save(&mut doc).unwrap();
    let revision = doc.revision();
    drop(tasks);
    decide(&mut f);
    let (args, input) = f.cancel_vector();
    for _ in 0..2 {
        let value = check(&f, &args, &input, "refused");
        assert!(value.get("attempt").is_none());
        assert!(value.get("attempt_id").is_none());
    }
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    assert_eq!(tasks.load(&f.task.uuid).unwrap().revision(), revision);
    assert_eq!(f.inputs(), 1);
}

fn terminal_refuses_an_unobserved_person_attempt() {
    for archived in [false, true] {
        let mut f = Fixture::new();
        f.h.run(&person(), Operation::Stop).unwrap();
        if archived {
            f.h.run(&person(), Operation::Archive).unwrap();
        }
        person_attempt(&f);
        let (args, input) = f.cancel_vector();
        let before = {
            let tasks =
                ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
            tasks.load(&f.task.uuid).unwrap().revision()
        };
        for _ in 0..2 {
            let reply = check(&f, &args, &input, "refused");
            let reason = reply["error"].as_str().unwrap();
            assert!(reason.starts_with("attempt_changed:"), "{reply}");
            let decoded = cancel_answer(
                Ended::Exited {
                    code: 0,
                    output: reply.to_string().into_bytes(),
                },
                &input,
            )
            .unwrap();
            assert_eq!(
                decoded.reply,
                super::super::super::super::ports::TaskReply::Refused {
                    reason: reason.into()
                }
            );
        }
        assert_eq!(f.inputs(), 0);
        let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
        assert_eq!(tasks.load(&f.task.uuid).unwrap().revision(), before);
    }
}
