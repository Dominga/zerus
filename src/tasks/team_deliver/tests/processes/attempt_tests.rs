//! Attempt boundaries use real private replica documents and production writers.
use super::*;
use zerus_replica::execution::{NativeRequest, Receipt, ReportKind};
use zerus_replica::Settlement;

fn bytes(f: &Fixture) -> Vec<u8> {
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut files = BTreeMap::new();
    for suffix in ["loro", "updates", "managed"] {
        let path = store
            .dir()
            .join("tasks")
            .join(format!("{}.{suffix}", f.task.uuid));
        let data = match std::fs::read(path) {
            Ok(data) => Some(data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => panic!("{e}"),
        };
        files.insert(suffix.to_owned(), data);
    }
    serde_json::to_vec(&files).unwrap()
}
fn restore_files(f: &Fixture, bytes: &[u8]) {
    let files: BTreeMap<String, Option<Vec<u8>>> = serde_json::from_slice(bytes).unwrap();
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    for (suffix, data) in files {
        let path = store
            .dir()
            .join("tasks")
            .join(format!("{}.{suffix}", f.task.uuid));
        if let Some(data) = data {
            std::fs::write(path, data).unwrap();
        } else if path.exists() {
            std::fs::remove_file(path).unwrap();
        }
    }
}
fn save(f: &Fixture, doc: &mut TaskDoc) {
    crate::tasks::store(&f.config, &f.task.project)
        .unwrap()
        .save(doc)
        .unwrap();
}
fn recipient(f: &Fixture, member: &str) -> SessionKey {
    let key =
        f.h.store
            .member(TEAM, member)
            .unwrap()
            .unwrap()
            .session
            .unwrap();
    SessionKey {
        machine: key.machine,
        session_id: key.session_id,
        session: format!("codex/deliver-{member}"),
    }
}
fn reserve(doc: &mut TaskDoc, to: &SessionKey, origin: Origin, submitted: bool) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let native = NativeRequest {
        machine: to.machine.clone(),
        name: to.session.clone(),
        run_id: format!("run-{}", to.session.rsplit('-').next().unwrap()),
        conversation_id: format!("conversation-{}", to.session.rsplit('-').next().unwrap()),
        digest: "a".repeat(64),
    };
    doc.reserve_attempt(&id, origin, to, native.clone(), &Actor::Person, now())
        .unwrap();
    if submitted {
        doc.settle_attempt(
            &id,
            Settlement::Receipt(&Receipt {
                request_id: id.clone(),
                name: native.name,
                run_id: native.run_id,
                conversation_id: native.conversation_id,
                first_message: false,
                digest: native.digest,
                submitted: true,
            }),
            &Actor::Person,
            now(),
        )
        .unwrap();
    } else {
        doc.settle_attempt(&id, Settlement::NotAdmitted, &Actor::Person, now())
            .unwrap();
    }
    id
}
fn receipt(f: &Fixture, doc: &TaskDoc, id: &str) {
    let delivery = doc.attempt(id).unwrap().unwrap().delivery.unwrap();
    std::fs::create_dir_all(f.config.state.join("input_receipts")).unwrap();
    std::fs::write(f.config.state.join("input_receipts").join(format!("{id}.json")),json!({"request_id":id,"name":delivery.name,"run_id":delivery.run_id,"conversation_id":delivery.conversation_id,"first_message":false,"digest":delivery.digest,"status":"submitted"}).to_string()).unwrap();
}
fn person_send(f: &Fixture, reported: bool) -> String {
    let mut doc = f.document();
    doc.floor().unwrap();
    if doc.stage().unwrap() == Stage::Active {
        doc.record_cancel(
            "The person cancelled the previous execution",
            &Actor::Person,
            now(),
        )
        .unwrap();
    }
    if doc.stage().unwrap() != Stage::Todo {
        doc.set_stage(Stage::Todo, &Actor::Person, now()).unwrap();
    }
    let to = recipient(f, "m1");
    let id = reserve(&mut doc, &to, Origin::Send, true);
    receipt(f, &doc, &id);
    if reported {
        doc.report_attempt(
            &id,
            "The person's new execution reported",
            ReportKind::Ready,
            None,
            &Actor::Agent(to),
            now(),
        )
        .unwrap();
    }
    save(f, &mut doc);
    id
}
fn call(f: &Fixture, action: &str) -> Value {
    let mut args = vec![action.into()];
    args.extend(f.args(false));
    let input = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn(
        if action == "team-cancel" {
            "cancel-command"
        } else {
            "command"
        },
        &args,
        &input.to_string(),
        false,
    );
    f.finish(child, path)
}
fn decide(f: &mut Fixture) {
    f.h.run(
        &person(),
        ops::Operation::Cancel {
            node: f.body.node,
            reason: "Cancel only the observed attempt".into(),
        },
    )
    .unwrap();
}
fn delivered(f: &mut Fixture) {
    assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
    f.h.run(
        &ops::tests::service(),
        ops::Operation::HandoutOutcome {
            effect_request_id: f.body.id.clone(),
            outcome: ops::HandoutOutcome::Submitted {
                receipt: "fixture".into(),
            },
        },
    )
    .unwrap();
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
}
fn reopen(f: &mut Fixture, member: &str) {
    completion_tests::reported(f);
    if member == "m1" {
        f.h.run(
            &person(),
            ops::Operation::Return {
                node: f.body.node,
                reason: "Return the observed result".into(),
            },
        )
        .unwrap();
    } else {
        let home = f.lead.machine.clone();
        f.bind(member, &home);
        f.h.run(
            &person(),
            ops::Operation::Reassign {
                node: f.body.node,
                member: member.into(),
                reason: "Reassign the observed execution".into(),
            },
        )
        .unwrap();
    }
    f.body.id = f.h.reopen(f.body.node, member);
    f.body.member = member.into();
}
fn stale(f: &Fixture, out: &Value, before: &[u8]) {
    assert_eq!(out["reply"]["state"], "stale", "{out}");
    assert_eq!(bytes(f), before, "a stale service command wrote the task");
    assert!(!f.config.state.join("cancel-notices").exists());
}
#[test]
fn cancel_stale_after_person_cancel_restore_and_send_then_repeat_after_observation() {
    let mut f = Fixture::new();
    delivered(&mut f);
    decide(&mut f);
    let old = f.body.id.clone();
    let new = person_send(&f, false);
    let before = bytes(&f);
    let out = call(&f, "team-cancel");
    stale(&f, &out, &before);
    let error = out["reply"]["error"].as_str().unwrap();
    assert!(error.contains('2') && error.contains('1'), "{out}");
    assert!(error.contains(&old) && error.contains(&new), "{out}");
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    // A fresh Stop decision is valid with both the contract-only store and
    // the later person-override rules. The command never settles that store.
    f.h.run(&person(), ops::Operation::Stop).unwrap();
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "cancelled", "{out}");
    assert_eq!(out["reply"]["attempt"], 2);
    assert_eq!(out["reply"]["attempt_id"], new);
    let after = bytes(&f);
    let repeat = call(&f, "team-cancel");
    assert_eq!(repeat["reply"]["attempt"], 2);
    assert_eq!(repeat["reply"]["attempt_id"], new);
    assert_eq!(bytes(&f), after);
}
#[test]
fn cancelled_and_released_replies_pin_the_pair_under_the_write_lock() {
    for active in [false, true] {
        let mut f = Fixture::new();
        if active {
            delivered(&mut f);
        }
        decide(&mut f);
        let out = call(&f, "team-cancel");
        assert_eq!(
            out["reply"]["state"],
            if active { "cancelled" } else { "released" },
            "{out}"
        );
        assert_eq!(out["reply"]["attempt"], u64::from(active));
        assert_eq!(
            out["reply"]["attempt_id"],
            if active { f.body.id.as_str() } else { "" }
        );
        let after = bytes(&f);
        assert_eq!(call(&f, "team-cancel")["reply"], out["reply"]);
        assert_eq!(bytes(&f), after);
    }
}
#[test]
fn cancel_refuses_a_document_older_than_the_observed_attempt() {
    let mut f = Fixture::new();
    delivered(&mut f);
    decide(&mut f);
    let old = bytes(&f);
    person_send(&f, false);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    // Observing the person's execution supersedes the earlier decision.
    // A fresh decision still reaches the live team's lower-number guard.
    decide(&mut f);
    restore_files(&f, &old);
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(bytes(&f), old);
    assert!(
        out["reply"]["error"].as_str().unwrap().contains("older"),
        "{out}"
    );
    assert!(!f.config.state.join("cancel-notices").exists());
}
#[test]
fn cancel_is_stale_when_a_concurrent_handoff_changes_only_the_current_id() {
    let mut f = Fixture::new();
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    delivered(&mut f);
    decide(&mut f);
    let mut rejected = f.document();
    let baseline = rejected.snapshot().unwrap();
    let to = recipient(&f, "m2");
    reserve(&mut rejected, &to, Origin::Handoff, false);
    save(&f, &mut rejected);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let mut submitted = TaskDoc::from_bytes(&baseline).unwrap();
    let current = reserve(&mut submitted, &to, Origin::Handoff, true);
    rejected.import(&submitted.snapshot().unwrap()).unwrap();
    save(&f, &mut rejected);
    let execution = rejected.view().unwrap().execution.unwrap();
    assert_eq!(execution.highest, 2);
    assert!(!execution.conflict);
    assert!(execution.unknown.is_empty());
    assert_eq!(execution.current.unwrap().id, current);
    let observed =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!(observed.attempt, 2);
    assert_eq!(observed.attempt_id, f.body.id);
    let before = bytes(&f);
    let out = call(&f, "team-cancel");
    stale(&f, &out, &before);
    let error = out["reply"]["error"].as_str().unwrap();
    assert!(
        error.contains(&current) && error.contains(&f.body.id),
        "{out}"
    );
}
#[test]
fn newer_active_person_execution_keeps_the_existing_handout_and_reopen_refusals() {
    for returning in [false, true] {
        let mut f = Fixture::new();
        if returning {
            reopen(&mut f, "m1");
        }
        person_send(&f, false);
        let before = bytes(&f);
        let count = f.count();
        let out = f.run("submitted", false);
        assert_eq!(out["reply"]["state"], "refused", "{out}");
        assert_eq!(bytes(&f), before);
        assert_eq!(f.count(), count);
        let message = out["reply"]["error"].as_str().unwrap();
        assert!(
            if returning {
                message.contains("Reopen no longer")
            } else {
                message.contains("already In progress")
            },
            "{out}"
        );
    }
}
#[test]
fn handout_is_stale_for_a_new_unobserved_rejected_attempt_then_delivers_after_observing() {
    let f = Fixture::new();
    let mut doc = f.document();
    doc.floor().unwrap();
    reserve(&mut doc, &recipient(&f, "m1"), Origin::Send, false);
    save(&f, &mut doc);
    let before = bytes(&f);
    let out = f.run("submitted", false);
    stale(&f, &out, &before);
    assert_eq!(f.count(), 0);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let observed =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!((observed.attempt, observed.attempt_id), (1, String::new()));
    assert!(!observed.attempt_conflict);
    assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
    assert_eq!(f.count(), 1);
}
#[test]
fn reopen_is_stale_for_a_persons_new_reported_attempt() {
    let mut f = Fixture::new();
    reopen(&mut f, "m1");
    person_send(&f, true);
    let before = bytes(&f);
    let count = f.count();
    let out = f.run("submitted", false);
    stale(&f, &out, &before);
    assert_eq!(f.count(), count);
}
#[test]
fn handover_is_stale_for_a_persons_new_active_attempt() {
    let mut f = Fixture::new();
    reopen(&mut f, "m2");
    person_send(&f, false);
    let before = bytes(&f);
    let count = f.count();
    let out = f.run("submitted", false);
    stale(&f, &out, &before);
    assert_eq!(f.count(), count);
}
#[test]
fn done_keeps_the_existing_current_id_refusal_for_a_persons_new_active_attempt() {
    let mut f = Fixture::new();
    completion_tests::accepted(&mut f);
    person_send(&f, false);
    let before = bytes(&f);
    let out = call(&f, "team-done");
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert!(
        out["reply"]["error"]
            .as_str()
            .unwrap()
            .contains("another attempt"),
        "{out}"
    );
    assert_eq!(bytes(&f), before);
}
#[test]
fn done_is_stale_when_a_new_rejected_reopen_raises_highest_but_not_current() {
    let mut f = Fixture::new();
    completion_tests::accepted(&mut f);
    let mut doc = f.document();
    let to = recipient(&f, "m1");
    reserve(&mut doc, &to, Origin::Reopen, false);
    save(&f, &mut doc);
    let execution = doc.view().unwrap().execution.unwrap();
    assert_eq!(execution.highest, 2);
    assert_eq!(execution.current.unwrap().id, f.body.id);
    let before = bytes(&f);
    let out = call(&f, "team-done");
    stale(&f, &out, &before);
}

#[test]
fn cancel_reply_keeps_the_written_pair_when_a_person_restarts_before_the_reply() {
    let mut f = Fixture::new();
    delivered(&mut f);
    decide(&mut f);
    let old = f.body.id.clone();
    let mut args = vec!["team-cancel".into()];
    args.extend(f.args(false));
    let input = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn(
        "lock-cancel-reply-command",
        &args,
        &input.to_string(),
        false,
    );
    let until = Instant::now() + Duration::from_secs(5);
    while !f.config.state.join("cancel_reply-ready").exists() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(f.document().view().unwrap().cancelled_at.is_some());
    let new = person_send(&f, false);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let before = bytes(&f);
    std::fs::write(f.config.state.join("cancel_reply-release"), "go").unwrap();
    let out = f.finish(child, path);
    assert_eq!(out["reply"]["state"], "cancelled", "{out}");
    assert_eq!(out["reply"]["attempt"], 1);
    assert_eq!(out["reply"]["attempt_id"], old);
    assert_eq!(bytes(&f), before);
    let observed =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!((observed.attempt, observed.attempt_id), (2, new));
}
#[test]
fn delivery_rechecks_the_pair_inside_admission_after_freezing_the_request() {
    let f = Fixture::new();
    let mut doc = f.document();
    doc.floor().unwrap();
    save(&f, &mut doc);
    let (child, path) = f.spawn(
        "submitted",
        &f.args(false),
        &serde_json::to_string(&f.body).unwrap(),
        true,
    );
    let until = Instant::now() + Duration::from_secs(5);
    while !std::fs::read_dir(&f.config.state)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("ready-"))
    {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut doc = f.document();
    reserve(&mut doc, &recipient(&f, "m1"), Origin::Send, false);
    save(&f, &mut doc);
    let before = bytes(&f);
    std::fs::write(f.config.state.join("release"), "go").unwrap();
    let out = f.finish(child, path);
    stale(&f, &out, &before);
    assert_eq!(f.count(), 0);
    assert!(f.document().attempt(&f.body.id).unwrap().is_none());
}

fn same_number_change(f: &Fixture, to: &SessionKey, origin: Origin, report: bool) {
    let mut old = f.document();
    let old_id = old.view().unwrap().execution.unwrap().current.unwrap().id;
    let baseline = old.snapshot().unwrap();
    reserve(&mut old, to, origin, false);
    save(f, &mut old);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let mut new = TaskDoc::from_bytes(&baseline).unwrap();
    let id = reserve(&mut new, to, origin, true);
    receipt(f, &new, &id);
    if report {
        new.report_attempt(
            &id,
            "A newer result on the concurrent replica",
            ReportKind::Ready,
            None,
            &Actor::Agent(to.clone()),
            now(),
        )
        .unwrap();
    }
    old.import(&new.snapshot().unwrap()).unwrap();
    save(f, &mut old);
    let e = old.view().unwrap().execution.unwrap();
    assert_eq!(e.highest, 2);
    assert!(!e.conflict);
    assert!(e.unknown.is_empty());
    assert_eq!(e.current.unwrap().id, id);
    let o =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!((o.attempt, o.attempt_id), (2, old_id));
}
#[test]
fn reopen_is_stale_when_only_the_current_id_changes_at_the_same_highest() {
    let mut f = Fixture::new();
    reopen(&mut f, "m1");
    same_number_change(&f, &recipient(&f, "m1"), Origin::Reopen, true);
    let before = bytes(&f);
    let count = f.count();
    let out = f.run("submitted", false);
    stale(&f, &out, &before);
    assert_eq!(f.count(), count);
}
#[test]
fn handover_is_stale_when_only_the_current_id_changes_at_the_same_highest() {
    let mut f = Fixture::new();
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    f.bind("m3", &home);
    delivered(&mut f);
    f.h.run(
        &person(),
        ops::Operation::Reassign {
            node: f.body.node,
            member: "m2".into(),
            reason: "Reassign only the observed execution".into(),
        },
    )
    .unwrap();
    f.body.id = f.h.reopen(f.body.node, "m2");
    f.body.member = "m2".into();
    same_number_change(&f, &recipient(&f, "m3"), Origin::Handoff, false);
    let before = bytes(&f);
    let count = f.count();
    let out = f.run("submitted", false);
    stale(&f, &out, &before);
    assert_eq!(f.count(), count);
}

#[test]
fn cancel_is_stale_when_a_rejected_handoff_raises_highest_but_not_current() {
    let mut f = Fixture::new();
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    delivered(&mut f);
    decide(&mut f);
    let mut doc = f.document();
    reserve(&mut doc, &recipient(&f, "m2"), Origin::Handoff, false);
    save(&f, &mut doc);
    let e = doc.view().unwrap().execution.unwrap();
    assert_eq!(e.highest, 2);
    assert_eq!(e.current.unwrap().id, f.body.id);
    let before = bytes(&f);
    let out = call(&f, "team-cancel");
    stale(&f, &out, &before);
}

mod admission_stage;
mod conflict;
mod no_attempt;
mod terminal;
mod upgrade;
mod witness;
