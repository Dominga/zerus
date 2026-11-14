use super::*;

fn stop(f: &mut Fixture, archived: bool) {
    f.h.run(&person(), ops::Operation::Stop).unwrap();
    if archived {
        f.h.run(&person(), ops::Operation::Archive).unwrap();
    }
}

#[test]
fn terminal_cancel_accepts_a_delivered_unobserved_team_effect_and_repeats() {
    for archived in [false, true] {
        for route in ["handout", "reopen", "handover"] {
            let mut f = Fixture::new();
            if route != "handout" {
                reopen(&mut f, if route == "reopen" { "m1" } else { "m2" });
            }
            assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
            let current = crate::teams::tasks::attempt(&f.document().view().unwrap());
            stop(&mut f, archived);
            let out = call(&f, "team-cancel");
            assert_eq!(out["reply"]["state"], "cancelled", "{route}: {out}");
            assert_eq!(out["reply"]["attempt"], current.0);
            assert_eq!(out["reply"]["attempt_id"], current.1);
            let before = bytes(&f);
            assert_eq!(call(&f, "team-cancel")["reply"], out["reply"]);
            assert_eq!(bytes(&f), before);
        }
    }
}

#[test]
fn terminal_cancel_accepts_the_observed_attempt() {
    for archived in [false, true] {
        let mut f = Fixture::new();
        delivered(&mut f);
        stop(&mut f, archived);
        let out = call(&f, "team-cancel");
        assert_eq!(out["reply"]["state"], "cancelled", "{out}");
        assert_eq!(out["reply"]["attempt"], 1);
        assert_eq!(out["reply"]["attempt_id"], f.body.id);
    }
}

#[test]
fn terminal_cancel_accepts_the_observed_person_override() {
    for archived in [false, true] {
        let mut f = Fixture::new();
        delivered(&mut f);
        let id = person_send(&f, false);
        crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
        stop(&mut f, archived);
        let out = call(&f, "team-cancel");
        assert_eq!(out["reply"]["state"], "cancelled", "{out}");
        assert_eq!(out["reply"]["attempt"], 2);
        assert_eq!(out["reply"]["attempt_id"], id);
    }
}

#[test]
fn terminal_cancel_refuses_a_persons_execution_after_stop_without_writing() {
    for archived in [false, true] {
        let mut f = Fixture::new();
        delivered(&mut f);
        stop(&mut f, archived);
        person_send(&f, false);
        let before = bytes(&f);
        let out = call(&f, "team-cancel");
        assert_eq!(out["reply"]["state"], "refused", "{out}");
        assert!(
            out["reply"]["error"]
                .as_str()
                .unwrap()
                .starts_with("attempt_changed:"),
            "{out}"
        );
        assert_eq!(bytes(&f), before);
        assert!(!f.config.state.join("cancel-notices").exists());
    }
}

#[test]
fn terminal_cancel_does_not_treat_an_unknown_effect_as_delivered() {
    let mut f = Fixture::new();
    assert_eq!(f.run("uncertain", false)["reply"]["state"], "uncertain");
    stop(&mut f, false);
    let before = bytes(&f);
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert!(
        out["reply"]["error"]
            .as_str()
            .unwrap()
            .starts_with("attempt_changed:"),
        "{out}"
    );
    assert_eq!(bytes(&f), before);
}

#[test]
fn terminal_cancel_refuses_an_unseen_claim_above_an_already_observed_effect() {
    let mut f = Fixture::new();
    delivered(&mut f);
    let mut doc = f.document();
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    reserve(&mut doc, &recipient(&f, "m2"), Origin::Handoff, false);
    save(&f, &mut doc);
    stop(&mut f, false);
    let before = bytes(&f);
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert!(
        out["reply"]["error"]
            .as_str()
            .unwrap()
            .starts_with("attempt_changed:"),
        "{out}"
    );
    assert_eq!(bytes(&f), before);
}

#[test]
fn terminal_cancel_refuses_an_older_effect_below_the_observed_conflict() {
    let mut f = Fixture::new();
    delivered(&mut f);
    let old = bytes(&f);
    let baseline = f.document().snapshot().unwrap();
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let to = recipient(&f, "m2");
    let mut a = TaskDoc::from_bytes(&baseline).unwrap();
    let mut b = TaskDoc::from_bytes(&baseline).unwrap();
    reserve(&mut a, &to, Origin::Handoff, true);
    reserve(&mut b, &to, Origin::Handoff, true);
    a.import(&b.snapshot().unwrap()).unwrap();
    assert!(a.view().unwrap().execution.unwrap().conflict);
    save(&f, &mut a);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    restore_files(&f, &old);
    stop(&mut f, false);
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert!(
        out["reply"]["error"]
            .as_str()
            .unwrap()
            .starts_with("attempt_changed:"),
        "{out}"
    );
    assert_eq!(bytes(&f), old);
}

#[test]
fn terminal_cancel_requires_the_effects_exact_id_team_node_generation_and_kind() {
    for change in ["id", "team", "node", "generation", "kind"] {
        let mut f = Fixture::new();
        assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
        if change == "team" {
            let mut create = ops::tests::create();
            if let ops::Operation::Create {
                home,
                integration_branch,
                ..
            } = &mut create
            {
                *home = f.lead.machine.clone();
                *integration_branch = "zerus/team/other".into();
            }
            f.h.run_for(ops::tests::TEAM_B, &person(), create).unwrap();
            f.h.run_for(
                ops::tests::TEAM_B,
                &person(),
                ops::Operation::AddTask {
                    task: ops::tests::task(2),
                    role: None,
                    pipeline: None,
                },
            )
            .unwrap();
        }
        let other = if change == "node" {
            f.h.enqueue(&person(), 2)
        } else {
            0
        };
        let db = f.h.store.db.as_ref().unwrap();
        match change {
            "id" => {
                db.execute(
                    "UPDATE effects SET request_id=?1",
                    [uuid::Uuid::new_v4().to_string()],
                )
                .unwrap();
            }
            "team" => {
                db.execute("UPDATE effects SET team=?1", [ops::tests::TEAM_B])
                    .unwrap();
            }
            "node" => {
                db.execute("UPDATE effects SET node=?1", [other]).unwrap();
            }
            "generation" => {
                db.execute("UPDATE effects SET generation=generation+1", [])
                    .unwrap();
            }
            "kind" => {
                db.execute("UPDATE effects SET kind='stage_task'", [])
                    .unwrap();
            }
            _ => unreachable!(),
        }
        stop(&mut f, false);
        let before = bytes(&f);
        let out = call(&f, "team-cancel");
        assert_eq!(out["reply"]["state"], "refused", "{change}: {out}");
        assert!(
            out["reply"]["error"]
                .as_str()
                .unwrap()
                .starts_with("attempt_changed:"),
            "{change}: {out}"
        );
        assert_eq!(bytes(&f), before, "{change}");
        assert!(!f.config.state.join("cancel-notices").exists());
    }
}
