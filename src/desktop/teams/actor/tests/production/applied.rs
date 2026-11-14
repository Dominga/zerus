//! Git write authority also needs a store that has applied the accepted
//! task's current state (zerus-63rru.37 round 4): native merge admission
//! reads the task's current revision through the reader that builds
//! ObserveTask and refuses unless the node's applied revision equals it.
//! Real repositories, real task documents and the product's own writers; no
//! store row is written by hand.
use super::*;
use zerus_replica::{
    execution::{NativeRequest, Origin, Receipt, ReportKind},
    Actor as TaskActor, Settlement, Stage,
};

const NOT_APPLIED: &str = "The team store has not applied the accepted task's current state";

fn executor(machine: &str, who: &str) -> zerus_replica::SessionKey {
    zerus_replica::SessionKey {
        machine: machine.into(),
        session: format!("codex/example/{who}"),
        session_id: format!("run_id:{who}"),
    }
}
/// An attempt reserved and settled as its native delivery records it: the
/// attempt with its frozen request, then its own submitted receipt.
fn deliver(
    doc: &mut zerus_replica::TaskDoc,
    id: &str,
    origin: Origin,
    to: &zerus_replica::SessionKey,
    now: i64,
) {
    doc.floor().unwrap();
    let native = NativeRequest {
        machine: to.machine.clone(),
        name: to.session.clone(),
        run_id: "fixture-run".into(),
        conversation_id: "fixture-conversation".into(),
        digest: "a".repeat(64),
    };
    doc.reserve_attempt(id, origin, to, native.clone(), &TaskActor::Person, now)
        .unwrap();
    doc.settle_attempt(
        id,
        Settlement::Receipt(&Receipt {
            request_id: id.into(),
            name: native.name,
            run_id: native.run_id,
            conversation_id: native.conversation_id,
            first_message: false,
            digest: native.digest,
            submitted: true,
        }),
        &TaskActor::Person,
        now + 1,
    )
    .unwrap();
}
/// The attempt's executor begins it and reports: the task goes In review.
fn finish(doc: &mut zerus_replica::TaskDoc, id: &str, by: &zerus_replica::SessionKey, now: i64) {
    let actor = TaskActor::Agent(by.clone());
    doc.begin_attempt(id, &actor, now).unwrap();
    let reported = doc
        .report_attempt(id, "Ready", ReportKind::Ready, None, &actor, now + 1)
        .unwrap();
    assert!(reported.moves, "{reported:?}");
}
fn tip(o: &Owned, plan: &ports::MergePlan) -> String {
    git(&o.root, &o.empty, &["rev-parse", &plan.branch])
}
fn refused_with(result: &ports::MergeResult, reason: &str) -> bool {
    matches!(result, ports::MergeResult::Unavailable { reason: r } if r == reason)
}

struct Current {
    h: Harness,
    o: Owned,
    task: TaskRef,
    node: u32,
    first: u64,
    plan: ports::MergePlan,
}
/// The round-3 confirmation's current-decision shape with real documents: a
/// lead's acceptance whose merge conflicted, the team's reopen, the person's
/// acceptance of the second report (the node's latest decision), and then
/// the person's own run that ended its turn: Todo, their own send, their own
/// report In review, all while the service observed nothing.
fn current_acceptance() -> Current {
    let mut h = Harness::empty();
    let o = owned(&h);
    let base = git(&o.root, &o.empty, &["rev-parse", "HEAD"]);
    let mut op = create();
    if let Operation::Create {
        base_commit,
        integration_branch,
        bundle,
        ..
    } = &mut op
    {
        *base_commit = base.clone();
        *integration_branch = "teams/integration".into();
        bundle.pipelines[0].stages.clear();
    }
    h.run(&person(), op).unwrap();
    let branch = h.store.team(TEAM).unwrap().unwrap().integration_branch;
    git(&o.root, &o.empty, &["branch", &branch, &base]);
    let lead = h.bind("lead");
    h.bind("m1");
    let (node, task) = real_task(&h);
    let state = h.temp.path().to_owned();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let member = executor(&home, "member");
    // The team's hand-out and its report.
    let handout = h.intent(node, "m1");
    edit(&state, &task, |doc| {
        deliver(doc, &handout, Origin::Send, &member, 10)
    });
    h.outcome(
        &handout,
        crate::teams::ops::HandoutOutcome::Submitted {
            receipt: "fixture receipt".into(),
        },
    );
    observe_task(&state, &task).unwrap();
    edit(&state, &task, |doc| finish(doc, &handout, &member, 20));
    observe_task(&state, &task).unwrap();
    let first_commit = commit(&o, "first report");
    let report = |h: &mut Harness, commit: &str| {
        let generation = h.store.generation(TEAM, node).unwrap().unwrap();
        h.run(
            &service(),
            Operation::RecordReport {
                node,
                generation,
                member: "m1".into(),
                text: "Report".into(),
                commit: commit.into(),
            },
        )
        .unwrap();
    };
    report(&mut h, &first_commit);
    h.run(&lead, Operation::Accept { node }).unwrap();
    let first = h.accept_decision(node);
    // Its merge conflicted: the service's fact, and the team's reopen.
    h.run(
        &service(),
        Operation::RecordIntegrationConflict {
            node,
            decision: first,
            conflicts: vec!["work.txt".into()],
        },
    )
    .unwrap();
    let reopen = h.reopen(node, "m1");
    edit(&state, &task, |doc| {
        deliver(doc, &reopen, Origin::Reopen, &member, 30)
    });
    h.reopen_outcome(
        &reopen,
        crate::teams::ops::HandoutOutcome::Submitted {
            receipt: "fixture receipt".into(),
        },
    );
    observe_task(&state, &task).unwrap();
    edit(&state, &task, |doc| finish(doc, &reopen, &member, 40));
    observe_task(&state, &task).unwrap();
    let second_commit = commit(&o, "second report");
    report(&mut h, &second_commit);
    h.run(&person(), Operation::Accept { node }).unwrap();
    let plan = merge_plan(&h, &o, node, &second_commit);
    assert!(plan.decision > first);
    // The person's own run, ended In review, never observed.
    let own = executor(&home, "person");
    let run = uuid::Uuid::new_v4().to_string();
    edit(&state, &task, |doc| {
        doc.set_stage(Stage::Todo, &TaskActor::Person, 50).unwrap();
        deliver(doc, &run, Origin::Send, &own, 51);
        finish(doc, &run, &own, 60);
    });
    Current {
        h,
        o,
        task,
        node,
        first,
        plan,
    }
}

/// The reviewer's P1: the supersede that should end the current Ready
/// acceptance fails as storage, so no later decision exists and the store
/// still shows the acceptance current. The store has not applied the task's
/// current state, so the native merge is refused and the real ref stays.
#[test]
fn an_injected_supersede_failure_on_the_current_acceptance_cannot_reach_git() {
    let Current {
        h,
        o,
        task,
        node,
        first,
        plan,
    } = current_acceptance();
    let state = h.temp.path().to_owned();
    let failure = {
        let _injected = crate::teams::ops::injected::fail_supersede(&h.store);
        observe_task(&state, &task).unwrap_err()
    };
    assert_eq!(failure.code, zerus_team_plan::problem::code::STORAGE_FAILED);
    assert_eq!(
        failure.message,
        "Teams storage is unavailable: injected supersede failure"
    );
    // The current acceptance is still the latest decision, Ready and live.
    assert_eq!(
        h.store.latest_decision(TEAM, node).unwrap(),
        Some(plan.decision)
    );
    assert!(h.store.node_live(TEAM, node).unwrap());
    assert!(h
        .store
        .integration_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|d| d.seq == plan.decision));
    let before = tip(&o, &plan);
    let result = native(&h).work_merge(&plan);
    assert!(refused_with(&result, NOT_APPLIED), "{result:?}");
    assert_eq!(tip(&o, &plan), before, "the current acceptance moved Git");
    // The older acceptance stays revoked by the later decision.
    let mut older = plan.clone();
    older.decision = first;
    let result = native(&h).work_merge(&older);
    assert!(
        refused_with(&result, "A later decision owns this node's integration"),
        "{result:?}"
    );
    assert_eq!(tip(&o, &plan), before, "the older acceptance moved Git");
    // The observation applies once storage recovers; nothing merges then.
    observe_task(&state, &task).unwrap();
    assert!(!h.store.node_live(TEAM, node).unwrap());
    let result = native(&h).work_merge(&plan);
    assert!(
        matches!(result, ports::MergeResult::Unavailable { .. }),
        "{result:?}"
    );
    assert_eq!(tip(&o, &plan), before);
    let items = h.store.inbox(TEAM, 200, None).unwrap().rows;
    assert_eq!(
        items
            .iter()
            .filter(|item| item.kind == "accepted_superseded")
            .count(),
        1
    );
}

/// The same shape without an injected failure: the observation supersedes
/// the current acceptance, and its plan cannot reach Git.
#[test]
fn an_applied_supersede_of_the_current_acceptance_cannot_reach_git() {
    let Current {
        h,
        o,
        task,
        node,
        plan,
        ..
    } = current_acceptance();
    observe_task(h.temp.path(), &task).unwrap();
    assert!(!h.store.node_live(TEAM, node).unwrap());
    let before = tip(&o, &plan);
    let result = native(&h).work_merge(&plan);
    assert!(
        matches!(result, ports::MergeResult::Unavailable { .. }),
        "{result:?}"
    );
    assert_eq!(tip(&o, &plan), before, "the ended acceptance moved Git");
}

/// A task revision the store has not applied refuses the merge; the same
/// plan merges once the production observation applies it.
#[test]
fn an_unapplied_task_revision_holds_the_merge_until_it_is_observed() {
    let (h, o, plan, task) = merge_fixture_task();
    edit(h.temp.path(), &task, |doc| {
        doc.comment("A note after the acceptance", &TaskActor::Person, 9)
            .unwrap()
    });
    let result = native(&h).work_merge(&plan);
    assert!(refused_with(&result, NOT_APPLIED), "{result:?}");
    assert_eq!(tip(&o, &plan), plan.base);
    observe_task(h.temp.path(), &task).unwrap();
    let result = native(&h).work_merge(&plan);
    assert!(
        matches!(result, ports::MergeResult::Merged { .. }),
        "{result:?}"
    );
    assert_ne!(tip(&o, &plan), plan.base);
}

/// The ref update's own admission reads the task again: a change written
/// after the first admission and before the ref update refuses the write.
#[test]
fn a_task_change_before_the_ref_update_refuses_the_write() {
    let (h, o, plan, task) = merge_fixture_task();
    let state = h.temp.path().to_owned();
    crate::teams::integrate::BEFORE_ADMISSION.with(|s| {
        *s.borrow_mut() = Some(Box::new(move || {
            edit(&state, &task, |doc| {
                doc.comment("A note during the merge", &TaskActor::Person, 9)
                    .unwrap()
            })
        }))
    });
    let result = native(&h).work_merge(&plan);
    assert!(refused_with(&result, NOT_APPLIED), "{result:?}");
    assert_eq!(tip(&o, &plan), plan.base);
}

/// A busy, unreadable or missing task document admits nothing.
#[test]
fn a_busy_unreadable_or_missing_task_refuses_the_merge() {
    for mode in ["busy", "unreadable", "missing"] {
        let (h, o, plan, task) = merge_fixture_task();
        let root = h.temp.path().join("replica");
        let files = zerus_replica::ProjectStore::open(&root, &task.project)
            .unwrap()
            .dir()
            .join("tasks");
        let snapshot = files.join(format!("{}.loro", task.uuid));
        assert!(snapshot.exists(), "{mode}");
        let mut held = None;
        match mode {
            "busy" => held = Some(zerus_replica::ProjectStore::open(&root, &task.project).unwrap()),
            "unreadable" => std::fs::write(&snapshot, b"not a task document").unwrap(),
            _ => {
                std::fs::remove_file(&snapshot).unwrap();
                let _ = std::fs::remove_file(files.join(format!("{}.updates", task.uuid)));
            }
        }
        let result = native(&h).work_merge(&plan);
        assert!(
            matches!(&result, ports::MergeResult::Unavailable { reason }
                if reason.starts_with("The accepted task cannot be read: ")),
            "{mode}: {result:?}"
        );
        assert_eq!(tip(&o, &plan), plan.base, "{mode}");
        drop(held);
    }
}

/// The ordinary lead acceptance and the person's Integration card Approve
/// still merge once the store applied the task's current state.
#[test]
fn ordinary_accept_and_owner_approved_merge_still_merge() {
    for owner in [false, true] {
        let Reported {
            mut h,
            o,
            lead,
            node,
            commit,
            ..
        } = reported_fixture(false);
        if owner {
            h.settings(|s| s.delegations.merge_accepted = false);
        }
        let item = h.run(&lead, Operation::Accept { node }).unwrap().item;
        if owner {
            let card = item.expect("owner integration card");
            h.run(
                &person(),
                Operation::DecideCard {
                    card,
                    decision: "approve".into(),
                },
            )
            .unwrap();
        }
        let plan = merge_plan(&h, &o, node, &commit);
        assert_eq!(
            h.store.latest_decision(TEAM, node).unwrap(),
            Some(plan.decision)
        );
        let result = native(&h).work_merge(&plan);
        assert!(
            matches!(result, ports::MergeResult::Merged { .. }),
            "{owner}: {result:?}"
        );
        assert_ne!(tip(&o, &plan), plan.base, "{owner}: no merge");
    }
}
