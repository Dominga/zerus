use super::*;

fn lead_cancel(f: &mut Fixture) {
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
            reason: "Cancel the observed work".into(),
        },
    )
    .unwrap();
}

fn extended_row_is_filled_once(action: &str) {
    let mut f = Fixture::new();
    if action == "team-done" {
        completion_tests::accepted(&mut f);
    } else {
        delivered(&mut f);
        lead_cancel(&mut f);
    }
    let triple = crate::teams::tasks::attempt(&f.document().view().unwrap());
    f.h.store.db.as_ref().unwrap().execute(
        "UPDATE task_observations SET data=json_remove(data,'$.attempt','$.attempt_id','$.attempt_conflict'),attempt=0,attempt_id='',attempt_conflict=0,last_attempt_id='' WHERE team=?1 AND node=?2",
        params![TEAM, f.body.node],
    ).unwrap();
    let before = bytes(&f);
    assert_eq!(call(&f, action)["reply"]["state"], "stale");
    assert_eq!(bytes(&f), before);
    let revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let o =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!((o.attempt, o.attempt_id, o.attempt_conflict), triple);
    assert_eq!(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        revision + 1
    );
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    assert_eq!(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        revision + 1,
        "the populated unchanged observation must be skipped again"
    );
    let out = call(&f, action);
    assert_eq!(
        out["reply"]["state"],
        if action == "team-done" {
            "done"
        } else {
            "cancelled"
        },
        "{out}"
    );
}

#[test]
fn unchanged_active_task_is_observed_once_after_the_attempt_extension() {
    extended_row_is_filled_once("team-cancel");
}

#[test]
fn unchanged_accepted_review_is_observed_once_after_the_attempt_extension() {
    extended_row_is_filled_once("team-done");
}

#[test]
fn a_never_admitted_first_handout_is_observed_once_after_the_attempt_extension() {
    let mut f = Fixture::new();
    assert_eq!(f.run("refused", false)["reply"]["state"], "refused");
    f.h.run(
        &ops::tests::service(),
        ops::Operation::HandoutOutcome {
            effect_request_id: f.body.id.clone(),
            outcome: ops::HandoutOutcome::Refused {
                reason: "fixture closed before input".into(),
            },
        },
    )
    .unwrap();
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let triple = crate::teams::tasks::attempt(&f.document().view().unwrap());
    assert_eq!(triple, (1, String::new(), false));
    lead_cancel(&mut f);
    f.h.store.db.as_ref().unwrap().execute(
        "UPDATE task_observations SET data=json_remove(data,'$.attempt','$.attempt_id','$.attempt_conflict'),attempt=0,attempt_id='',attempt_conflict=0,last_attempt_id='' WHERE team=?1 AND node=?2",
        params![TEAM, f.body.node],
    ).unwrap();
    let before = bytes(&f);
    assert_eq!(call(&f, "team-cancel")["reply"]["state"], "stale");
    assert_eq!(bytes(&f), before);
    let revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let o =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!((o.attempt, o.attempt_id, o.attempt_conflict), triple);
    assert_eq!(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        revision + 1
    );
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    assert_eq!(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        revision + 1,
        "the populated unchanged observation must be skipped again"
    );
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "released", "{out}");
    assert_eq!(out["reply"]["attempt"], 1);
    assert_eq!(out["reply"]["attempt_id"], "");
    assert!(f.document().team().unwrap().is_none());
}
