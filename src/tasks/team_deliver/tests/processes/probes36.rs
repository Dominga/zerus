//! zerus-63rru.36: the
//! cancelled-then-moved-back task in private state. The team.s commands run
//! as real children; the person-side commands run through the exact product
//! APIs the CLI arms call after its caller proof, because the proof itself
//! refuses this harness's ancestry (captured in the log).
use super::*;

fn doc_view(f: &Fixture, uuid: &str) -> zerus_replica::TaskView {
    let store = crate::tasks::store(&f.config, "example-project").unwrap();
    let doc = store.load(uuid).unwrap();
    doc.view().unwrap()
}
fn fields(f: &Fixture, uuid: &str, step: &str) {
    let v = doc_view(f, uuid);
    eprintln!(
        "WP36 {step}: stage={} cancelled_at={:?} cancel_reason={:?} session={:?} execution={:?} writable={}",
        v.stage.as_str(),
        v.cancelled_at,
        v.cancel_reason,
        v.session,
        v.execution.as_ref().map(|e| (&e.current, &e.conflict)),
        v.writable
    );
}
fn intake(f: &Fixture, uuid: &str, team: &str, step: &str) -> Value {
    let task = TaskRef {
        project: "example-project".into(),
        uuid: uuid.to_string(),
    };
    let result = crate::teams::tasks::send(
        &crate::teams::tasks::Caller {
            state: &f.config.state,
            machine: &crate::swarm::local_node_id(&f.config).unwrap(),
            actor: Actor::Person,
        },
        &task,
        team,
        None,
        None,
    );
    let out = match &result {
        Ok(intake) => json!({
            "queued": intake.queued,
            "node": intake.node,
            "refusal": intake.refusal.as_ref().map(|p| json!({"code": p.code, "message": p.message})),
            "observation_pending": intake.observation_pending,
        }),
        Err(error) => json!({"error": {"code": error.code, "message": error.message}}),
    };
    eprintln!("WP36 {step}: intake={out}");
    out
}
fn add(f: &mut Fixture) -> (String, String) {
    let tasks = ProjectStore::open(&f.config.state.join("replica"), "example-project").unwrap();
    let (mut task, _) = tasks
        .new_named_task(
            "Reproduction task",
            "Reproduction body",
            &[],
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::End,
            "example",
            now(),
            None,
            false,
        )
        .unwrap();
    let id = task.id().to_string();
    let uuid = task.uuid().to_string();
    tasks.save(&mut task).unwrap();
    (id, uuid)
}
fn deliver_without_observation(f: &mut Fixture, uuid: &str, member: &str) -> (u32, String) {
    let out = intake(f, uuid, TEAM, "send to the team");
    assert_eq!(out["queued"], true, "{out}");
    let node = out["node"].as_u64().unwrap() as u32;
    if f.h
        .store
        .member(TEAM, member)
        .unwrap()
        .unwrap()
        .session
        .is_none()
    {
        f.bind(member, &crate::swarm::local_node_id(&f.config).unwrap());
    }
    let intent = f.h.intent(node, member);
    f.body.node = node;
    f.body.member = member.to_string();
    f.body.id = intent.clone();
    f.task.uuid = uuid.to_string();
    let delivered = f.run("submitted", false);
    assert_eq!(delivered["reply"]["state"], "submitted");
    (node, intent)
}
fn deliver(f: &mut Fixture, uuid: &str, member: &str) -> u32 {
    let out = intake(f, uuid, TEAM, "send to the team");
    assert_eq!(out["queued"], true, "{out}");
    let node = out["node"].as_u64().unwrap() as u32;
    if f.h
        .store
        .member(TEAM, member)
        .unwrap()
        .unwrap()
        .session
        .is_none()
    {
        f.bind(member, &crate::swarm::local_node_id(&f.config).unwrap());
    }
    let intent = f.h.intent(node, member);
    f.body.node = node;
    f.body.member = member.to_string();
    f.body.id = intent.clone();
    f.task.uuid = uuid.to_string();
    let delivered = f.run("submitted", false);
    eprintln!("WP36 deliver reply: {delivered}");
    assert_eq!(delivered["reply"]["state"], "submitted");
    // The deliver command itself wrote the task's send (the doc now shows In
    // progress); the observation records it for the team.
    crate::teams::tasks::observe(
        &f.config.state,
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid.to_string(),
        },
    )
    .unwrap();
    node
}
fn team_cancel(f: &mut Fixture, node: u32) -> Value {
    let owner = f.h.store.team(TEAM).unwrap().unwrap().owner.clone();
    let lead = ops::Actor::Session {
        person: owner,
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
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    f.body.node = node;
    let mut args = vec!["task".to_string(), "team-cancel".to_string()];
    args.extend(f.args(false));
    let body = json!({"schema":1,"team":TEAM,"node":node});
    let (child, path) = f.spawn("cli", &args, &body.to_string(), false);
    let result = f.wait(child);
    let reply: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    eprintln!(
        "WP36 team-cancel: reply={reply} stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    reply
}
fn move_back(f: &Fixture, uuid: &str, step: &str) {
    let store = crate::tasks::store(&f.config, "example-project").unwrap();
    let mut doc = store.load(uuid).unwrap();
    store
        .change_stage_placed(None, &mut doc, Stage::Todo, &Actor::Person, now())
        .unwrap();
    store.save(&mut doc).unwrap();
    eprintln!("WP36 {step}: moved back to To do");
}

#[test]
fn wp36_reproduction() {
    let mut f = Fixture::new();
    // The proof's refusal in a real child, captured for the record: under
    // this harness the person proof cannot be established, so the person-side
    // commands run through the product APIs the CLI calls after its proof.
    let _ = f.cli_with(
        &["task".into(), "stage".into(), "x".into(), "todo".into()],
        "{}",
        &[("HGS_TEAMS_TEST_CONTEXT", "opaque")],
    );

    // Sequence A: the team's cancel.
    let (_id_a, uuid_a) = add(&mut f);
    fields(&f, &uuid_a, "A fresh");
    let node_a = deliver(&mut f, &uuid_a, "m2");
    fields(&f, &uuid_a, "A delivered");
    let answer = team_cancel(&mut f, node_a);
    eprintln!("WP36 A team-cancel answer: {answer}");
    fields(&f, &uuid_a, "A after team cancel");
    crate::teams::tasks::observe(
        &f.config.state,
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid_a.clone(),
        },
    )
    .unwrap();
    eprintln!(
        "WP36 A node after the observation: {:?}",
        f.h.store.node(TEAM, node_a).unwrap().map(|n| n.state)
    );
    move_back(&f, &uuid_a, "A");
    fields(&f, &uuid_a, "A moved back to To do");
    let a_same = intake(&f, &uuid_a, TEAM, "A resend to the same team");
    assert_eq!(a_same["queued"], true, "{a_same}");
    fields(&f, &uuid_a, "A after the same-team resend");
    let mut create2 = ops::tests::create();
    if let ops::Operation::Create {
        home: machine,
        integration_branch,
        ..
    } = &mut create2
    {
        *machine = crate::swarm::local_node_id(&f.config).unwrap();
        *integration_branch = "other-branch".into();
    }
    f.h.run_for("00000000-0000-4000-8000-0000000000cc", &person(), create2)
        .unwrap();
    // The new node in this team is live: another team refuses the same task
    // (the existing live-node guard), so one task never runs in two teams.
    let a_other = intake(
        &f,
        &uuid_a,
        "00000000-0000-4000-8000-0000000000cc",
        "A resend to another team",
    );
    assert_eq!(
        a_other["error"]["code"], "duplicate",
        "{a_other}"
    );
    fields(&f, &uuid_a, "A after the other-team refusal");

    // Sequence B: the person's ordinary cancel.
    let (_id_b, uuid_b) = add(&mut f);
    let node_b = deliver(&mut f, &uuid_b, "m2");
    fields(&f, &uuid_b, "B delivered");
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let mut doc = store.load(&uuid_b).unwrap();
        doc.record_cancel("Obsolete", &zerus_replica::Actor::Person, now())
            .unwrap();
        store.save(&mut doc).unwrap();
    }
    fields(&f, &uuid_b, "B after person cancel");
    move_back(&f, &uuid_b, "B");
    fields(&f, &uuid_b, "B moved back to To do");
    // The team-cancel command on the revived task: no mark, no live
    // attempt of the team — it answers released, never a second cancel.
    {
        f.body.node = node_b;
        let mut args = vec!["task".to_string(), "team-cancel".to_string()];
        args.extend(f.args(false));
        let body = json!({"schema":1,"team":TEAM,"node":node_b});
        let (child, path) = f.spawn("cli", &args, &body.to_string(), false);
        let result = f.wait(child);
        let reply: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        eprintln!(
            "WP36 B team-cancel on the revived task: {reply} {}",
            String::from_utf8_lossy(&result.stdout)
        );
        // The released answer is an accepted one (exit 0), never a second
        // cancel and never a refusal.
        assert_eq!(reply["reply"]["exit"], 0, "{reply}");
    }
    // The resend with no further act by the person: the intake settles the
    // old node (the same observation the actor would apply) and queues a
    // new node in one answer; the old node ends exactly once.
    let fresh = intake(&f, &uuid_b, TEAM, "B resend");
    assert_eq!(fresh["queued"], true, "{fresh}");
    let node_new = fresh["node"].as_u64().unwrap() as u32;
    assert_ne!(node_new, node_b, "{fresh}");
    assert!(
        f.h.store
            .team(TEAM)
            .unwrap()
            .unwrap()
            .revision
            >= 1,
        "the settlement wrote"
    );
    // The new node is live: another team refuses the same task, as before.
    let other_b = intake(
        &f,
        &uuid_b,
        "00000000-0000-4000-8000-0000000000cc",
        "B resend to another team",
    );
    assert_eq!(other_b["error"]["code"], "duplicate", "{other_b}");
    fields(&f, &uuid_b, "B after the new node");
    let _ = node_b;
}

/// Rule 2, the started variant: the member began the attempt before the
/// person's cancel. The move-back clears the mark; the observation still
/// ends the old node from the bound attempt's phase, and the resend is
/// refused before it and admitted after it.
#[test]
fn a_started_attempt_is_ended_by_the_observation_after_the_move_back() {
    let mut f = Fixture::new();
    let (_id, uuid) = add(&mut f);
    let node = deliver(&mut f, &uuid, "m2");
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let mut doc = store.load(&uuid).unwrap();
        let id = f.body.id.clone();
        let holder = doc.attempt(&id).unwrap().unwrap().executor;
        doc.begin_attempt(&id, &Actor::Agent(holder), now()).unwrap();
        store.save(&mut doc).unwrap();
    }
    crate::teams::tasks::observe(
        &f.config.state,
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid.clone(),
        },
    )
    .unwrap();
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let mut doc = store.load(&uuid).unwrap();
        doc.record_cancel("Obsolete", &zerus_replica::Actor::Person, now())
            .unwrap();
        store.save(&mut doc).unwrap();
    }
    move_back(&f, &uuid, "started variant");
    let fresh = intake(&f, &uuid, TEAM, "started: resend");
    assert_eq!(fresh["queued"], true, "{fresh}");
    assert_ne!(fresh["node"].as_u64(), Some(node as u64), "{fresh}");
    let _ = node;
}

/// Rule 2, the team's own cancel with the observation deferred: the team
/// cancels, the person moves the task back before the observation reads the
/// mark, and the deferred observation still ends the node from the phase.
#[test]
fn the_teams_cancel_ends_the_node_even_when_the_observation_comes_after_the_move_back() {
    let mut f = Fixture::new();
    let (_id, uuid) = add(&mut f);
    let node = deliver(&mut f, &uuid, "m2");
    team_cancel(&mut f, node);
    fields(&f, &uuid, "team variant: after the team cancel");
    move_back(&f, &uuid, "team variant");
    let fresh = intake(&f, &uuid, TEAM, "team variant: resend");
    assert_eq!(fresh["queued"], true, "{fresh}");
    assert_ne!(fresh["node"].as_u64(), Some(node as u64), "{fresh}");
}


/// Round 2 B: a Review task with the mark is still refused by its stage
/// (the mark no longer matters; the stage does).
#[test]
fn a_review_task_with_the_mark_is_refused_by_its_stage() {
    let mut f = Fixture::new();
    let (_id, uuid) = add(&mut f);
    let node = deliver(&mut f, &uuid, "m2");
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let mut doc = store.load(&uuid).unwrap();
        doc.record_cancel("Obsolete", &zerus_replica::Actor::Person, now())
            .unwrap();
        store.save(&mut doc).unwrap();
    }
    // The observation ends the node on the mark, so the Review task has no
    // live binding; the send is then refused by its stage.
    crate::teams::tasks::observe(
        &f.config.state,
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid.clone(),
        },
    )
    .unwrap();
    let refused = intake(&f, &uuid, TEAM, "Review with the mark");
    assert_eq!(
        refused["error"]["message"], "Send a task in Backlog or To do to a team.",
        "{refused}"
    );
    let _ = node;
}

/// Round 2 B: a cancel and a move-back merged on two replicas in both sync
/// orders, then sent. Every merged state answers safely: a To do state is
/// admitted and the binding write clears any surviving mark; a Review state
/// is refused by its stage. Each order runs on a fresh fixture.
#[test]
fn a_cancel_and_move_back_merged_then_sent_in_both_orders() {
    for order in 0..2 {
        let mut f = Fixture::new();
        let (_id, uuid) = add(&mut f);
        deliver(&mut f, &uuid, "m2");
        let base = {
            let store = crate::tasks::store(&f.config, "example-project").unwrap();
            let doc = store.load(&uuid).unwrap();
            doc.snapshot().unwrap()
        };
        let mut cancelling = zerus_replica::TaskDoc::from_bytes(&base).unwrap();
        let mut reviving = zerus_replica::TaskDoc::from_bytes(&base).unwrap();
        cancelling
            .record_cancel("Superseded", &zerus_replica::Actor::Person, 20)
            .unwrap();
        reviving
            .record_cancel("Superseded", &zerus_replica::Actor::Person, 20)
            .unwrap();
        reviving
            .set_stage(Stage::Todo, &zerus_replica::Actor::Person, 21)
            .unwrap();
        let (mut first, second) = if order == 0 {
            (reviving, cancelling)
        } else {
            (cancelling, reviving)
        };
        assert!(first.import(&second.export_since(&first.version()).unwrap()).is_ok());
        let view = first.view().unwrap();
        eprintln!(
            "WP36 merged order {order}: stage={} marked={}",
            view.stage.as_str(),
            view.cancelled_at.is_some()
        );
        {
            let store = crate::tasks::store(&f.config, "example-project").unwrap();
            store.save(&mut first).unwrap();
        }
        let answer = intake(&f, &uuid, TEAM, &format!("merged order {order} sent"));
        match (view.stage, view.cancelled_at.is_some()) {
            (Stage::Todo, _) => {
                assert_eq!(answer["queued"], true, "{answer}");
                {
                    let store = crate::tasks::store(&f.config, "example-project").unwrap();
                    let doc = store.load(&uuid).unwrap();
                    assert_eq!(doc.view().unwrap().cancelled_at, None);
                }
            }
            (Stage::Review, _) => {
                assert_eq!(
                    answer["error"]["message"],
                    "Send a task in Backlog or To do to a team.",
                    "{answer}"
                );
            }
            other => panic!("unsafe merged stage: {other:?}"),
        }
    }
}
/// Decision 3: the production observer path itself ends the old node after
/// the move-back — no intake settlement involved. The phase-dropped mutant
/// (the observation's current_cancelled forced to false) leaves the node
/// live and kills this test.
#[test]
fn the_production_observation_ends_the_old_node_after_the_move_back() {
    let mut f = Fixture::new();
    let (_id, uuid) = add(&mut f);
    let node = deliver(&mut f, &uuid, "m2");
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let mut doc = store.load(&uuid).unwrap();
        doc.record_cancel("Obsolete", &zerus_replica::Actor::Person, now())
            .unwrap();
        store.save(&mut doc).unwrap();
    }
    move_back(&f, &uuid, "observer path");
    crate::teams::tasks::observe(
        &f.config.state,
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid.clone(),
        },
    )
    .unwrap();
    // The old node's live binding is gone: the observation read the cancel
    // from the bound attempt's phase.
    let db = rusqlite::Connection::open(f.config.state.join("teams").join("teams-v1.sqlite3"))
        .unwrap();
    let live: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM live_tasks WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(live, 0, "the old node stayed live");
    // Round 4: the same column ends the node as cancelled, so the generated
    // node_state is cancelled and a hand-out of the old node is refused.
    let ended = f.h.store.node(TEAM, node).unwrap().unwrap();
    assert_eq!(
        ended.state,
        zerus_team_plan::model::NodeState::Cancelled,
        "the old node's state: {ended:?}"
    );
    let owner = f.h.store.team(TEAM).unwrap().unwrap().owner.clone();
    let service = ops::Actor::Service {
        person: owner,
        team: TEAM.into(),
    };
    let refusal = f
        .h
        .run(
            &service,
            ops::Operation::HandoutIntent {
                node,
                member: "m2".into(),
                effect_request_id: String::new(),
            },
        )
        .unwrap_err();
    assert_eq!(refusal.code, "invalid_state", "{refusal:?}");
}

/// The own-cancelled end reads only an OPEN effect of the attempt
/// (zerus-63rru.36 round 4): an effect the observation already closed stays
/// closed, and the node is not ended again by a later observation of the
/// same cancelled attempt. The reviewer's open-state-filter-dropped mutant
/// kills this test.
#[test]
fn an_observed_closed_effect_is_not_ended_by_the_later_cancel() {
    let mut f = Fixture::new();
    let (_id, uuid) = add(&mut f);
    let (node, intent) = deliver_without_observation(&mut f, &uuid, "m2");
    // The outcome closes the effect (observed_done); the cancel then comes
    // from the attempt phase only, and a closed effect must not end the
    // node again.
    let owner = f.h.store.team(TEAM).unwrap().unwrap().owner.clone();
    f.h.run(
        &ops::Actor::Service {
            person: owner,
            team: TEAM.into(),
        },
        ops::Operation::HandoutOutcome {
            effect_request_id: intent,
            outcome: ops::HandoutOutcome::Refused {
                reason: "handed back".into(),
            },
        },
    )
    .unwrap();
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let mut doc = store.load(&uuid).unwrap();
        doc.record_cancel("Obsolete", &zerus_replica::Actor::Person, now())
            .unwrap();
        store.save(&mut doc).unwrap();
    }
    move_back(&f, &uuid, "closed effect");
    crate::teams::tasks::observe(
        &f.config.state,
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid.clone(),
        },
    )
    .unwrap();
    // The own-cancelled write does not fire for a closed effect: the column
    // stays 0 (the decision's own state may already read cancelled).
    let db = rusqlite::Connection::open(f.config.state.join("teams").join("teams-v1.sqlite3"))
        .unwrap();
    let column: i64 = db
        .query_row(
            "SELECT task_cancelled FROM nodes WHERE team=?1 AND id=?2",
            rusqlite::params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(column, 0, "the closed effect was ended by the own path");
}

/// A conflicting cancelled attempt is never the node's own cancel
/// (zerus-63rru.36 round 4): the person's review decides a conflict. The
/// reviewer's conflict-filter-dropped mutant kills this test.
#[test]
fn a_conflicting_cancelled_attempt_is_not_the_nodes_own_cancel() {
    let mut f = Fixture::new();
    let (_id, uuid) = add(&mut f);
    // The pre-deliver document, for the other computer's conflicting send.
    let before = {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let doc = store.load(&uuid).unwrap();
        doc.snapshot().unwrap()
    };
    let (node, _) = deliver_without_observation(&mut f, &uuid, "m2");
    {
        // The other computer sent the same task to its own session and
        // cancelled that attempt; the merge leaves the execution conflicted,
        // with no current identity the team can safely act on.
        let other = zerus_replica::SessionKey {
            machine: "other-machine".into(),
            session: "codex/other".into(),
            session_id: String::new(),
        };
        let mut twin = zerus_replica::TaskDoc::from_bytes(&before).unwrap();
        // The twin's writes stay causally ahead of the deliver's concurrent
        // path, so the merged stage is deterministically the cancel's Review
        // and the later move back is allowed: the scenario's rule, not the
        // Loro operation counters, decides the test.
        for n in 0..50 {
            twin.comment(
                &format!("pad {n}"),
                &zerus_replica::Actor::Agent(other.clone()),
                now(),
            )
            .unwrap();
        }
        twin.record_send(
            &other,
            "request-2",
            &zerus_replica::Actor::Agent(other.clone()),
            now(),
        )
        .unwrap();
        twin.record_cancel("Superseded", &zerus_replica::Actor::Agent(other), now())
            .unwrap();
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let imported = store
            .import_updates(vec![zerus_replica::Update {
                document: zerus_replica::Document::Task(uuid.clone()),
                bytes: twin.snapshot().unwrap(),
            }])
            .unwrap();
        assert_eq!(imported.changed, 1, "{imported:?}");
        let merged = store.load(&uuid).unwrap();
        let view = merged.view().unwrap();
        assert!(
            view.execution.as_ref().is_some_and(|e| e.conflict),
            "the merged execution is not conflicted: {view:?}"
        );
    }
    move_back(&f, &uuid, "conflicting cancel");
    crate::teams::tasks::observe(
        &f.config.state,
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid.clone(),
        },
    )
    .unwrap();
    // The conflicted projection's cancel is not the node's own: the column
    // stays 0.
    let db = rusqlite::Connection::open(f.config.state.join("teams").join("teams-v1.sqlite3"))
        .unwrap();
    let column: i64 = db
        .query_row(
            "SELECT task_cancelled FROM nodes WHERE team=?1 AND id=?2",
            rusqlite::params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(column, 0, "the conflicting cancel ended the node");
}

/// Decision 3: the older build's document — To do with the mark it left —
/// is queued and the binding write clears the mark; the mark-check-restored
/// mutant (the intake refuses on the mark again) kills this test.
#[test]
fn an_older_builds_marked_todo_task_is_queued_and_cleared() {
    let mut f = Fixture::new();
    let (_id, uuid) = add(&mut f);
    deliver(&mut f, &uuid, "m2");
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let mut doc = store.load(&uuid).unwrap();
        doc.record_cancel("Obsolete", &zerus_replica::Actor::Person, now())
            .unwrap();
        doc.set_stage(Stage::Todo, &zerus_replica::Actor::Person, now())
            .unwrap();
        // The older build's leftover: its set_stage kept the mark.
        doc.set_cancelled_for_test(now(), "Obsolete");
        store.save(&mut doc).unwrap();
    }
    let queued = intake(&f, &uuid, TEAM, "older-build marked To do");
    assert_eq!(queued["queued"], true, "{queued}");
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let doc = store.load(&uuid).unwrap();
        let view = doc.view().unwrap();
        assert_eq!(view.cancelled_at, None, "the binding write clears the mark");
        assert_eq!(view.cancel_reason, "");
    }
}

/// Decision 2: every observation the build writes through
/// rows::record_observation parses with the base build's exact field set
/// (deny_unknown_fields, no current_cancelled), including one for a
/// cancelled attempt.
#[test]
fn stored_observations_parse_with_the_base_field_set() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct BaseObservation {
        revision: String,
        #[serde(default, skip_serializing_if = "crate::teams::ops::KeyState::is_ours")]
        key: crate::teams::ops::KeyState,
        #[serde(default)]
        previous_revision: String,
        stage: crate::teams::ops::TaskStage,
        #[serde(default)]
        cancelled: bool,
        rank: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        attempt: u64,
        #[serde(default)]
        attempt_id: String,
        #[serde(default)]
        attempt_conflict: bool,
    }
    let mut f = Fixture::new();
    let (_id, uuid) = add(&mut f);
    deliver(&mut f, &uuid, "m2");
    {
        let store = crate::tasks::store(&f.config, "example-project").unwrap();
        let mut doc = store.load(&uuid).unwrap();
        doc.record_cancel("Obsolete", &zerus_replica::Actor::Person, now())
            .unwrap();
        store.save(&mut doc).unwrap();
    }
    move_back(&f, &uuid, "stored-layout");
    crate::teams::tasks::observe(
        &f.config.state,
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid.clone(),
        },
    )
    .unwrap();
    let db = rusqlite::Connection::open(f.config.state.join("teams").join("teams-v1.sqlite3"))
        .unwrap();
    let rows: Vec<String> = db
        .prepare("SELECT data FROM task_observations")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        serde_json::from_str::<BaseObservation>(&row).unwrap_or_else(|e| {
            panic!("the base field set rejected a stored observation: {e}: {row}")
        });
    }
}
