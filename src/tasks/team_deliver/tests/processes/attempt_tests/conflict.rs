use super::*;

pub(super) fn refused_first(f: &mut Fixture) {
    assert_eq!(f.run("refused", false)["reply"]["state"], "refused");
    f.h.outcome(
        &f.body.id,
        ops::HandoutOutcome::Refused {
            reason: "The native fixture refused before input".into(),
        },
    );
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let execution = f.document().view().unwrap().execution.unwrap();
    assert_eq!(execution.highest, 1);
    assert!(!execution.conflict);
    assert!(execution.unknown.is_empty());
    assert_eq!(
        execution.current.unwrap().phase,
        zerus_replica::execution::Phase::NotStarted
    );
}
#[test]
fn a_refused_first_handout_is_released_with_an_empty_current_id_on_cancel_or_stop() {
    for stopped in [false, true] {
        let mut f = Fixture::new();
        refused_first(&mut f);
        if stopped {
            f.h.run(&person(), ops::Operation::Stop).unwrap();
        } else {
            let actor = ops::Actor::Session {
                person: "local".into(),
                team: TEAM.into(),
                member: "lead".into(),
                session: zerus_team_plan::model::SessionKey {
                    machine: f.lead.machine.clone(),
                    session_id: f.lead.session_id.clone(),
                },
            };
            f.h.run(
                &actor,
                ops::Operation::Cancel {
                    node: f.body.node,
                    reason: "Release the task that never started".into(),
                },
            )
            .unwrap();
        }
        let out = call(&f, "team-cancel");
        assert_eq!(out["reply"]["state"], "released", "{out}");
        assert_eq!(out["reply"]["attempt"], 1);
        assert_eq!(out["reply"]["attempt_id"], "");
        assert!(out["reply"].get("attempt_conflict").is_none());
        assert_eq!(f.document().stage().unwrap(), Stage::Todo);
        assert!(f.document().team().unwrap().is_none());
        assert!(!f.config.state.join("cancel-notices").exists());
        let before = bytes(&f);
        let repeat = call(&f, "team-cancel");
        assert_eq!(repeat["reply"], out["reply"]);
        assert_eq!(bytes(&f), before);
    }
}
#[test]
fn a_second_handout_after_a_refused_first_handout_delivers_as_an_ordinary_task() {
    let mut f = Fixture::new();
    refused_first(&mut f);
    let first = f.body.id.clone();
    f.body.id = f.h.intent(f.body.node, "m1");
    let out = f.run("submitted", false);
    assert_eq!(out["reply"]["state"], "submitted", "{out}");
    let doc = f.document();
    let execution = doc.view().unwrap().execution.unwrap();
    assert_eq!(execution.highest, 2);
    assert!(!execution.conflict);
    assert!(execution.unknown.is_empty());
    assert_eq!(execution.current.unwrap().id, f.body.id);
    assert_eq!(doc.stage().unwrap(), Stage::Active);
    assert!(doc.attempt(&first).unwrap().is_some());
    assert_eq!(
        doc.attempt(&f.body.id).unwrap().unwrap().actor,
        Actor::Agent(f.lead.clone())
    );
}

fn observe(f: &Fixture) {
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
}

#[test]
fn a_return_to_the_same_member_cannot_hand_over_the_persons_new_running_execution() {
    let mut f = Fixture::new();
    reopen(&mut f, "m1");
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let to = recipient(&f, "m2");
    let mut doc = f.document();
    doc.floor().unwrap();
    doc.set_stage(Stage::Todo, &Actor::Person, now()).unwrap();
    let id = reserve(&mut doc, &to, Origin::Send, true);
    receipt(&f, &doc, &id);
    doc.begin_attempt(&id, &Actor::Agent(to.clone()), now())
        .unwrap();
    save(&f, &mut doc);
    assert_eq!(f.body.member, "m1");
    assert_eq!(f.document().view().unwrap().session, Some(to));
    let before = bytes(&f);
    let count = f.count();
    stale(&f, &f.run("submitted", false), &before);
    assert_eq!(f.count(), count);
}

// Both replicas claim generation one with a never-admitted send. The two
// projections have the same number and no current execution, but only one
// projection conflicts. Nothing below forges an observation or an attempt.
fn flag_only_difference() -> (Fixture, Vec<u8>) {
    let mut f = Fixture::new();
    let mut baseline = f.document();
    baseline.floor().unwrap();
    let baseline = baseline.snapshot().unwrap();
    refused_first(&mut f);
    decide(&mut f);
    let clear = bytes(&f);
    let held =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!(
        (held.attempt, held.attempt_id, held.attempt_conflict),
        (1, String::new(), false)
    );
    let mut peer = TaskDoc::from_bytes(&baseline).unwrap();
    reserve(&mut peer, &recipient(&f, "m1"), Origin::Send, false);
    let mut doc = f.document();
    doc.import(&peer.snapshot().unwrap()).unwrap();
    let e = doc.view().unwrap().execution.unwrap();
    assert_eq!(e.highest, 1);
    assert!(e.conflict);
    assert!(e.unknown.is_empty());
    save(&f, &mut doc);
    (f, clear)
}

#[test]
fn cancel_is_stale_when_only_the_documents_conflict_flag_changed() {
    let (f, _) = flag_only_difference();
    let before = bytes(&f);
    stale(&f, &call(&f, "team-cancel"), &before);
}

#[test]
fn cancel_is_stale_when_only_the_stores_conflict_flag_changed() {
    let (f, clear) = flag_only_difference();
    observe(&f);
    let o =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!(o.attempt, 1);
    assert_eq!(o.attempt_id, "");
    assert!(o.attempt_conflict);
    restore_files(&f, &clear);
    stale(&f, &call(&f, "team-cancel"), &clear);
}

#[test]
fn cancel_keeps_its_conflict_refusal_when_the_store_has_the_same_flag() {
    let (f, _) = flag_only_difference();
    observe(&f);
    let before = bytes(&f);
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert!(
        out["reply"]["error"]
            .as_str()
            .unwrap()
            .contains("conflicting"),
        "{out}"
    );
    assert_eq!(bytes(&f), before);
    assert!(!f.config.state.join("cancel-notices").exists());
}

fn conflicting_deliveries(f: &Fixture, origin: Origin, report: bool) {
    let mut baseline = f.document();
    baseline.floor().unwrap();
    let baseline = baseline.snapshot().unwrap();
    let to = recipient(f, "m1");
    let mut a = TaskDoc::from_bytes(&baseline).unwrap();
    let mut b = TaskDoc::from_bytes(&baseline).unwrap();
    for doc in [&mut a, &mut b] {
        let id = reserve(doc, &to, origin, true);
        receipt(f, doc, &id);
        if report {
            doc.report_attempt(
                &id,
                "A real concurrent holder report",
                ReportKind::Ready,
                None,
                &Actor::Agent(to.clone()),
                now(),
            )
            .unwrap();
        }
    }
    a.import(&b.snapshot().unwrap()).unwrap();
    assert!(a.view().unwrap().execution.unwrap().conflict);
    save(f, &mut a);
    observe(f);
    assert!(
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap()
            .attempt_conflict
    );
}

#[test]
fn each_delivery_route_refuses_an_observed_conflict() {
    for route in ["handout", "reopen", "handover"] {
        let mut f = Fixture::new();
        if route != "handout" {
            reopen(&mut f, if route == "reopen" { "m1" } else { "m2" });
        }
        conflicting_deliveries(
            &f,
            if route == "handout" {
                Origin::Send
            } else {
                Origin::Reopen
            },
            route == "reopen",
        );
        let before = bytes(&f);
        let count = f.count();
        let out = f.run("submitted", false);
        assert_eq!(out["reply"]["state"], "refused", "{route}: {out}");
        assert_eq!(bytes(&f), before, "{route}");
        assert_eq!(f.count(), count, "{route}");
    }
}

#[test]
fn done_keeps_its_existing_conflict_refusal() {
    let mut f = Fixture::new();
    completion_tests::accepted(&mut f);
    conflicting_deliveries(&f, Origin::Reopen, true);
    let before = bytes(&f);
    let out = call(&f, "team-done");
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(out["reply"]["error"], "Task execution changed.");
    assert_eq!(bytes(&f), before);
}

#[test]
fn a_handout_refuses_conflicting_never_admitted_claims_while_the_task_is_todo() {
    let f = Fixture::new();
    let mut doc = f.document();
    doc.floor().unwrap();
    let mut peer = TaskDoc::from_bytes(&doc.snapshot().unwrap()).unwrap();
    let to = recipient(&f, "m1");
    reserve(&mut doc, &to, Origin::Send, false);
    reserve(&mut peer, &to, Origin::Send, false);
    doc.import(&peer.snapshot().unwrap()).unwrap();
    assert_eq!(doc.stage().unwrap(), Stage::Todo);
    assert!(doc.view().unwrap().execution.unwrap().conflict);
    save(&f, &mut doc);
    observe(&f);
    let before = bytes(&f);
    let out = f.run("submitted", false);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(bytes(&f), before);
    assert_eq!(f.count(), 0);
}
