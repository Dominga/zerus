//! A person's run without an attempt of the team (a legacy-route send of a
//! queued task records none) is not the team's execution: the team's cancel,
//! which acts on one attempt, has none to act on (zerus-63rru.39).
use super::*;

const STATES: [TeamStatus; 4] = [
    TeamStatus::Running,
    TeamStatus::Paused,
    TeamStatus::Stopped,
    TeamStatus::Archived,
];

fn live(state: TeamStatus) -> bool {
    matches!(state, TeamStatus::Running | TeamStatus::Paused)
}
fn observe(f: &Fixture) {
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
}
/// The team state with the cancel it carries: the lead's in a live team, the
/// stop's own in a stopped or archived one.
fn into_state(f: &mut Fixture, state: TeamStatus) {
    if live(state) {
        let lead = ops::Actor::Session {
            person: "local".into(),
            team: TEAM.into(),
            member: "lead".into(),
            session: zerus_team_plan::model::SessionKey {
                machine: f.lead.machine.clone(),
                session_id: f.lead.session_id.clone(),
            },
        };
        f.h.run(
            &lead,
            ops::Operation::Cancel {
                node: f.body.node,
                reason: "Cancel the queued task".into(),
            },
        )
        .unwrap();
        if state == TeamStatus::Paused {
            f.h.run(&person(), ops::Operation::Pause).unwrap();
        }
    } else {
        f.h.run(&person(), ops::Operation::Stop).unwrap();
        if state == TeamStatus::Archived {
            f.h.run(&person(), ops::Operation::Archive).unwrap();
        }
    }
}
fn persons_session(f: &Fixture) -> SessionKey {
    SessionKey {
        machine: f.lead.machine.clone(),
        session: "codex/persons-own".into(),
        session_id: "launch_id:00000000-0000-4000-8000-0000000000c1".into(),
    }
}
/// The person's send of the queued task by the legacy route, with the
/// production writer: the document runs and records no attempt.
fn legacy_send(f: &Fixture) {
    let mut doc = f.document();
    assert!(!doc.execution_managed());
    doc.record_send(
        &persons_session(f),
        &uuid::Uuid::new_v4().to_string(),
        &Actor::Person,
        now(),
    )
    .unwrap();
    save(f, &mut doc);
    let view = f.document().view().unwrap();
    assert_eq!(view.stage, Stage::Active);
    assert_eq!(
        crate::teams::tasks::attempt(&view),
        (0, String::new(), false)
    );
}
fn pending_cancel(f: &Fixture) -> u64 {
    f.h.store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .rev()
        .find(|d| {
            d.node == f.body.node
                && d.kind == "cancel"
                && d.state == crate::teams::store::DecisionState::Pending
        })
        .unwrap()
        .seq
}

/// The reviewer's sequence (probe6d_note_an_older_cancel_and_a_persons_legacy_send)
/// in every team state, with and without the store's observation of the run:
/// refused, nothing written, no notice, and an ordinary refused answer.
#[test]
fn a_persons_run_without_an_attempt_is_refused_in_every_team_state() {
    for state in STATES {
        for seen in [false, true] {
            let mut f = Fixture::new();
            observe(&f);
            if live(state) {
                into_state(&mut f, state);
                legacy_send(&f);
                if seen {
                    observe(&f);
                }
            } else {
                legacy_send(&f);
                if seen {
                    observe(&f);
                }
                into_state(&mut f, state);
            }
            let before = bytes(&f);
            let sessions = f.sessions();
            let decision = pending_cancel(&f);
            let out = call(&f, "team-cancel");
            let case = format!("{state:?} seen={seen}: {out}");
            assert_eq!(out["reply"]["state"], "refused", "{case}");
            let reason = out["reply"]["error"].as_str().unwrap().to_owned();
            assert!(reason.starts_with("no_team_attempt:"), "{case}");
            assert!(reason.contains("a person started it"), "{case}");
            assert_eq!(bytes(&f), before, "{case}");
            assert_eq!(f.sessions(), sessions, "{case}");
            assert!(!f.config.state.join("cancel-notices").exists(), "{case}");
            let view = f.document().view().unwrap();
            assert_eq!(view.stage, Stage::Active, "{case}");
            assert!(view.cancelled_at.is_none(), "{case}");
            assert_eq!(view.session, Some(persons_session(&f)), "{case}");
            // Repeating the decision gives the same answer and writes nothing.
            assert_eq!(call(&f, "team-cancel")["reply"], out["reply"], "{case}");
            assert_eq!(bytes(&f), before, "{case}");
            // The store records it as any refused cancel; a live team's lead
            // is told.
            f.h.run(
                &ops::tests::service(),
                ops::Operation::RecordCancel {
                    node: f.body.node,
                    decision,
                    state: ops::CancelState::Refused,
                    reason: reason.clone(),
                    attempt: 0,
                    attempt_id: String::new(),
                },
            )
            .unwrap();
            let settled = f.h.store.decisions(TEAM, 200, None).unwrap().rows;
            assert!(settled.iter().any(
                |d| d.seq == decision && d.state == crate::teams::store::DecisionState::Settled
            ));
            let told =
                f.h.store
                    .inbox(TEAM, 200, None)
                    .unwrap()
                    .rows
                    .into_iter()
                    .any(|i| {
                        i.kind == format!("cancel_refused:{decision}") && i.text.contains(&reason)
                    });
            assert_eq!(told, live(state), "{case}");
        }
    }
}

/// Once the person's run has ended, the document no longer runs and the
/// cancel acts as before: a result moved back to To do is released, a
/// cancelled run answers cancelled.
#[test]
fn after_the_persons_run_ends_the_cancel_acts_as_before() {
    for state in STATES {
        for end in ["to_do", "cancelled"] {
            let mut f = Fixture::new();
            observe(&f);
            if live(state) {
                into_state(&mut f, state);
                legacy_send(&f);
                observe(&f);
            } else {
                legacy_send(&f);
                observe(&f);
                into_state(&mut f, state);
            }
            let mut doc = f.document();
            if end == "to_do" {
                doc.record_report(
                    "The person's run reported",
                    &Actor::Agent(persons_session(&f)),
                    now(),
                )
                .unwrap();
                doc.set_stage(Stage::Todo, &Actor::Person, now()).unwrap();
            } else {
                doc.record_cancel("The person stopped the run", &Actor::Person, now())
                    .unwrap();
            }
            save(&f, &mut doc);
            let out = call(&f, "team-cancel");
            let case = format!("{state:?} {end}: {out}");
            assert_eq!(
                out["reply"]["state"],
                if end == "to_do" {
                    "released"
                } else {
                    "cancelled"
                },
                "{case}"
            );
            assert_eq!(out["reply"]["attempt"], 0, "{case}");
            assert_eq!(out["reply"]["attempt_id"], "", "{case}");
            assert!(!f.config.state.join("cancel-notices").exists(), "{case}");
        }
    }
}

/// The team's own attempt keeps the cancel of today in every team state.
#[test]
fn a_run_under_the_teams_own_attempt_is_cancelled_as_before() {
    for state in STATES {
        let mut f = Fixture::new();
        delivered(&mut f);
        into_state(&mut f, state);
        let out = call(&f, "team-cancel");
        let case = format!("{state:?}: {out}");
        assert_eq!(out["reply"]["state"], "cancelled", "{case}");
        assert_eq!(out["reply"]["attempt"], 1, "{case}");
        assert_eq!(out["reply"]["attempt_id"], f.body.id, "{case}");
        assert!(
            f.document().view().unwrap().cancelled_at.is_some(),
            "{case}"
        );
    }
}

/// A conflict has no current attempt either, but it keeps its own refusal:
/// the person must settle it.
#[test]
fn a_running_conflict_keeps_its_own_refusal() {
    for state in STATES {
        let mut f = Fixture::new();
        delivered(&mut f);
        let home = f.lead.machine.clone();
        f.bind("m2", &home);
        let to = recipient(&f, "m2");
        let baseline = f.document().snapshot().unwrap();
        let mut a = TaskDoc::from_bytes(&baseline).unwrap();
        let mut b = TaskDoc::from_bytes(&baseline).unwrap();
        reserve(&mut a, &to, Origin::Handoff, true);
        reserve(&mut b, &to, Origin::Handoff, true);
        a.import(&b.snapshot().unwrap()).unwrap();
        save(&f, &mut a);
        let view = f.document().view().unwrap();
        assert_eq!(view.stage, Stage::Active);
        assert_eq!(
            crate::teams::tasks::attempt(&view),
            (2, String::new(), true)
        );
        observe(&f);
        into_state(&mut f, state);
        let before = bytes(&f);
        let out = call(&f, "team-cancel");
        let case = format!("{state:?}: {out}");
        assert_eq!(out["reply"]["state"], "refused", "{case}");
        let reason = out["reply"]["error"].as_str().unwrap();
        assert!(!reason.starts_with("no_team_attempt:"), "{case}");
        assert!(reason.contains("unknown or conflicting"), "{case}");
        assert_eq!(bytes(&f), before, "{case}");
        assert!(!f.config.state.join("cancel-notices").exists(), "{case}");
    }
}
