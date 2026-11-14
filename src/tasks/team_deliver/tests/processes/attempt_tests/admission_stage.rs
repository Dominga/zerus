use super::*;
use zerus_replica::execution::Phase;

fn observe(f: &Fixture) {
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
}
fn lead_cancel(f: &mut Fixture) {
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
            reason: "Cancel this node".into(),
        },
    )
    .unwrap();
}
fn cancellation_state(f: &mut Fixture, state: TeamStatus) {
    match state {
        TeamStatus::Running => lead_cancel(f),
        TeamStatus::Paused => {
            lead_cancel(f);
            f.h.run(&person(), ops::Operation::Pause).unwrap();
        }
        TeamStatus::Stopped | TeamStatus::Archived => {
            f.h.run(&person(), ops::Operation::Stop).unwrap();
            if state == TeamStatus::Archived {
                f.h.run(&person(), ops::Operation::Archive).unwrap();
            }
        }
        _ => unreachable!(),
    }
}
fn person_reserves(f: &Fixture, member: &str) -> String {
    let mut doc = f.document();
    if doc.stage().unwrap() != Stage::Todo {
        doc.set_stage(Stage::Todo, &Actor::Person, now()).unwrap();
    }
    let to = recipient(f, member);
    let native = NativeRequest {
        machine: to.machine.clone(),
        name: to.session.clone(),
        run_id: format!("run-{member}"),
        conversation_id: format!("conversation-{member}"),
        digest: "bc".repeat(32),
    };
    let id = uuid::Uuid::new_v4().to_string();
    doc.reserve_attempt(&id, Origin::Send, &to, native, &Actor::Person, now())
        .unwrap();
    save(f, &mut doc);
    id
}
fn deliver_reserved(f: &Fixture, id: &str) {
    let mut doc = f.document();
    let a = doc.attempt(id).unwrap().unwrap();
    let d = a.delivery.unwrap();
    doc.settle_attempt(
        id,
        Settlement::Receipt(&Receipt {
            request_id: id.into(),
            name: d.name,
            run_id: d.run_id,
            conversation_id: d.conversation_id,
            first_message: false,
            digest: d.digest,
            submitted: true,
        }),
        &a.actor,
        now(),
    )
    .unwrap();
    receipt(f, &doc, id);
    save(f, &mut doc);
}
fn team_reserves(f: &Fixture) {
    let (child, path) = f.spawn(
        "crash-reserved",
        &f.args(false),
        &serde_json::to_string(&f.body).unwrap(),
        false,
    );
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(73));
    assert!(!path.exists());
    assert_eq!(
        f.document().attempt(&f.body.id).unwrap().unwrap().actor,
        Actor::Agent(f.lead.clone())
    );
}
fn live(state: TeamStatus) -> bool {
    matches!(state, TeamStatus::Running | TeamStatus::Paused)
}
fn states() -> [TeamStatus; 4] {
    [
        TeamStatus::Running,
        TeamStatus::Paused,
        TeamStatus::Stopped,
        TeamStatus::Archived,
    ]
}

#[test]
fn a_persons_send_observed_before_delivery_cannot_be_cancelled() {
    for state in states() {
        let mut f = Fixture::new();
        let home = f.lead.machine.clone();
        f.bind("m2", &home);
        let id = person_reserves(&f, "m2");
        observe(&f);
        let o =
            f.h.store
                .task_observation(TEAM, f.body.node)
                .unwrap()
                .unwrap();
        assert_eq!(o.stage, ops::TaskStage::Todo);
        assert_eq!(o.attempt_id, id);
        cancellation_state(&mut f, state);
        deliver_reserved(&f, &id);
        let before = bytes(&f);
        let sessions = f.sessions();
        let out = call(&f, "team-cancel");
        assert_eq!(
            out["reply"]["state"],
            if live(state) { "stale" } else { "refused" },
            "{state:?}: {out}"
        );
        if !live(state) {
            assert!(out["reply"]["error"]
                .as_str()
                .unwrap()
                .starts_with("attempt_changed:"));
        }
        assert_eq!(bytes(&f), before);
        assert_eq!(f.sessions(), sessions);
        assert!(!f.config.state.join("cancel-notices").exists());
        assert_eq!(f.document().stage().unwrap(), Stage::Active);
        assert!(f.document().view().unwrap().cancelled_at.is_none());
        if live(state) {
            observe(&f);
            assert_eq!(f.h.store.generation(TEAM, f.body.node).unwrap(), Some(2));
            assert!(f.h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
            assert_eq!(
                f.h.store
                    .journal(TEAM, 200, None)
                    .unwrap()
                    .rows
                    .last()
                    .unwrap()
                    .kind,
                "person_override"
            );
            let out = call(&f, "team-cancel");
            assert_eq!(out["reply"]["state"], "refused", "{out}");
            assert!(out["reply"]["error"]
                .as_str()
                .unwrap()
                .contains("No Pending cancel"));
            assert_eq!(bytes(&f), before);
        }
    }
}

#[test]
fn an_own_effect_with_an_equal_pair_still_needs_its_start_observed_in_a_live_team() {
    for state in states() {
        let mut f = Fixture::new();
        team_reserves(&f);
        observe(&f);
        assert_eq!(
            f.h.store
                .task_observation(TEAM, f.body.node)
                .unwrap()
                .unwrap()
                .stage,
            ops::TaskStage::Todo
        );
        deliver_reserved(&f, &f.body.id);
        cancellation_state(&mut f, state);
        if live(state) {
            let before = bytes(&f);
            assert_eq!(call(&f, "team-cancel")["reply"]["state"], "stale");
            assert_eq!(bytes(&f), before);
            observe(&f);
        }
        let out = call(&f, "team-cancel");
        assert_eq!(out["reply"]["state"], "cancelled", "{state:?}: {out}");
        assert_eq!(out["reply"]["attempt_id"], f.body.id);
    }
}

#[test]
fn a_never_delivered_person_reservation_keeps_the_existing_nonrunning_rules() {
    for state in states() {
        for admitted in [false, true] {
            let mut f = Fixture::new();
            let home = f.lead.machine.clone();
            f.bind("m2", &home);
            let id = person_reserves(&f, "m2");
            if !admitted {
                let mut doc = f.document();
                doc.settle_attempt(&id, Settlement::NotAdmitted, &Actor::Person, now())
                    .unwrap();
                save(&f, &mut doc);
            }
            observe(&f);
            cancellation_state(&mut f, state);
            let before = bytes(&f);
            let out = call(&f, "team-cancel");
            assert_eq!(
                out["reply"]["state"],
                if admitted { "refused" } else { "released" },
                "{state:?}: {out}"
            );
            if admitted {
                assert!(out["reply"]["error"].as_str().unwrap().contains("unknown"));
                assert_eq!(bytes(&f), before);
            } else {
                assert!(f.document().team().unwrap().is_none());
                assert_eq!(out["reply"]["attempt_id"], "");
            }
            assert_eq!(f.document().stage().unwrap(), Stage::Todo);
            assert!(!f.config.state.join("cancel-notices").exists());
        }
    }
}

#[test]
fn an_observed_running_attempt_remains_cancellable_in_every_team_state() {
    for state in states() {
        let mut f = Fixture::new();
        delivered(&mut f);
        cancellation_state(&mut f, state);
        assert_eq!(call(&f, "team-cancel")["reply"]["state"], "cancelled");
    }
}

#[test]
fn other_service_routes_cannot_act_on_a_persons_send_observed_before_start() {
    for route in ["handout", "reopen", "handover", "return-handover", "done"] {
        let mut f = Fixture::new();
        match route {
            "reopen" | "return-handover" => reopen(&mut f, "m1"),
            "handover" => reopen(&mut f, "m2"),
            "done" => completion_tests::accepted(&mut f),
            _ => (),
        }
        let home = f.lead.machine.clone();
        if f.h
            .store
            .member(TEAM, "m2")
            .unwrap()
            .unwrap()
            .session
            .is_none()
        {
            f.bind("m2", &home);
        }
        let member = if matches!(route, "reopen" | "handover") {
            "m1"
        } else {
            "m2"
        };
        let id = person_reserves(&f, member);
        observe(&f);
        deliver_reserved(&f, &id);
        assert_eq!(
            f.document()
                .view()
                .unwrap()
                .execution
                .unwrap()
                .current
                .unwrap()
                .phase,
            Phase::Submitted
        );
        let before = bytes(&f);
        let count = f.count();
        let sessions = f.sessions();
        let out = if route == "done" {
            call(&f, "team-done")
        } else {
            f.run("submitted", false)
        };
        assert_eq!(
            out["reply"]["state"],
            if route.contains("handover") {
                "stale"
            } else {
                "refused"
            },
            "{route}: {out}"
        );
        assert_eq!(bytes(&f), before, "{route}");
        assert_eq!(f.count(), count, "{route}");
        assert_eq!(f.sessions(), sessions, "{route}");
        assert_eq!(f.document().stage().unwrap(), Stage::Active);
        assert!(f.document().view().unwrap().cancelled_at.is_none());
    }
}

fn person_reopens(f: &Fixture) -> String {
    let to = recipient(f, "m1");
    let native = NativeRequest {
        machine: to.machine.clone(),
        name: to.session.clone(),
        run_id: "run-m1".into(),
        conversation_id: "conversation-m1".into(),
        digest: "ef".repeat(32),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let mut doc = f.document();
    doc.reserve_attempt(&id, Origin::Reopen, &to, native, &Actor::Person, now())
        .unwrap();
    save(f, &mut doc);
    id
}

#[test]
fn only_an_in_progress_observation_counts_as_the_start_of_a_persons_reopen() {
    for state in [TeamStatus::Running, TeamStatus::Paused] {
        let mut f = Fixture::new();
        completion_tests::reported(&mut f);
        let id = person_reopens(&f);
        observe(&f);
        let o =
            f.h.store
                .task_observation(TEAM, f.body.node)
                .unwrap()
                .unwrap();
        assert_eq!(o.stage, ops::TaskStage::InReview);
        assert_eq!((o.attempt, o.attempt_id.as_str()), (2, id.as_str()));
        cancellation_state(&mut f, state);
        deliver_reserved(&f, &id);
        assert_eq!(f.document().stage().unwrap(), Stage::Active);
        let before = bytes(&f);
        let sessions = f.sessions();
        let out = call(&f, "team-cancel");
        assert_eq!(out["reply"]["state"], "stale", "{state:?}: {out}");
        assert_eq!(bytes(&f), before);
        assert_eq!(f.sessions(), sessions);
        assert!(!f.config.state.join("cancel-notices").exists());
        observe(&f);
        assert_eq!(f.h.store.generation(TEAM, f.body.node).unwrap(), Some(2));
        assert!(f.h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
        assert_eq!(
            f.h.store
                .journal(TEAM, 200, None)
                .unwrap()
                .rows
                .last()
                .unwrap()
                .kind,
            "person_override"
        );
        let out = call(&f, "team-cancel");
        assert_eq!(out["reply"]["state"], "refused", "{out}");
        assert!(out["reply"]["error"]
            .as_str()
            .unwrap()
            .contains("No Pending cancel"));
        assert_eq!(bytes(&f), before);
        assert_eq!(f.document().stage().unwrap(), Stage::Active);
        assert!(f.document().view().unwrap().cancelled_at.is_none());
    }
}
