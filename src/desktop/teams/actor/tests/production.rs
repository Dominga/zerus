//! Production report
//! reader `Native::work_reported` against real Git worktrees with a real
//! Zerus ownership receipt.
use super::super::*;
use crate::teams::ops::tests::*;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::AtomicBool, Arc},
};

pub(super) fn git(dir: &Path, empty: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(k);
        }
    }
    let out = cmd
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", empty)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=Review",
            "-c",
            "user.email=review@example.test",
            "-c",
            "init.templateDir=",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}
pub(super) fn native(h: &Harness) -> native::Native {
    let n = native::Native {
        config: crate::config::Config {
            home: String::new(),
            dir: h.temp.path().join("config"),
            state: h.temp.path().to_owned(),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        },
        machine: h.store.team(TEAM).unwrap().unwrap().home,
        epoch: "fixture".into(),
        stop: Arc::new(AtomicBool::new(false)),
    };
    fleet::seed(h, &n);
    n
}
struct Owned {
    wt: PathBuf,
    root: PathBuf,
    empty: PathBuf,
    nonce: String,
}
/// A Zerus-owned linked worktree on the launch branch the package 7 adapter
/// names (`zerus/team/member-{launch}`), with the receipt written where the
/// maintained ownership reader looks.
fn owned(h: &Harness) -> Owned {
    let base = crate::platform::canonical(h.temp.path()).unwrap();
    let root = base.join("repo");
    std::fs::create_dir(&root).unwrap();
    let empty = base.join("empty-config");
    std::fs::write(&empty, "").unwrap();
    git(&root, &empty, &["init", "-q", "--initial-branch", "main"]);
    git(&root, &empty, &["config", "user.name", "Teams Fixture"]);
    git(
        &root,
        &empty,
        &["config", "user.email", "fixture@example.test"],
    );
    git(&root, &empty, &["config", "commit.gpgsign", "false"]);
    git(&root, &empty, &["commit", "--allow-empty", "-qm", "base"]);
    let wt = base.join("zerus-team-launch");
    git(
        &root,
        &empty,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "zerus/team/member-launch",
            wt.to_str().unwrap(),
        ],
    );
    let metadata = PathBuf::from(git(&wt, &empty, &["rev-parse", "--absolute-git-dir"]));
    let common = root.join(".git");
    let nonce = uuid::Uuid::new_v4().to_string();
    let owner = serde_json::json!({"version":1,"id":nonce,"common_dir":common,"path":wt,"metadata":metadata,"branch":"zerus/team/member-launch","head":"fixture"});
    let receipt = crate::state::team_worktree_receipt_path(h.temp.path(), &common, &wt);
    std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
    std::fs::write(&receipt, owner.to_string()).unwrap();
    std::fs::write(metadata.join("zerus-owner.json"), owner.to_string()).unwrap();
    native_work::TEST_REPO.with(|s| *s.borrow_mut() = Some(common.to_str().unwrap().to_owned()));
    Owned {
        wt,
        root,
        empty,
        nonce,
    }
}
fn member(o: &Owned) -> Member {
    Member {
        id: "m1".into(),
        role: "developer".into(),
        agent: "codex".into(),
        account: String::new(),
        state: MemberState::Active,
        session: None,
        worktree: o.wt.to_str().unwrap().into(),
        ownership_nonce: o.nonce.clone(),
    }
}
fn commit(o: &Owned, text: &str) -> String {
    std::fs::write(o.wt.join("work.txt"), format!("{text}\n")).unwrap();
    git(&o.wt, &o.empty, &["add", "work.txt"]);
    git(&o.wt, &o.empty, &["commit", "-qm", text]);
    git(&o.wt, &o.empty, &["rev-parse", "HEAD"])
}

#[test]
fn production_report_reads_the_owned_launch_branch_and_marks_a_dirty_tree() {
    let h = Harness::new();
    let o = owned(&h);
    let head = commit(&o, "on launch branch");
    let team = h.store.team(TEAM).unwrap().unwrap();
    let r = native(&h).work_reported(&team, &member(&o));
    eprintln!("REVIEW08 launch-branch report: {r:?}");
    let r = r.unwrap();
    assert_eq!(r.commit, head);
    assert_eq!(r.branch, "zerus/team/member-launch");
    assert!(!r.dirty);
    std::fs::write(o.wt.join("untracked.txt"), "x\n").unwrap();
    let dirty = native(&h).work_reported(&team, &member(&o)).unwrap();
    eprintln!("REVIEW08 dirty report: {dirty:?}");
    assert!(dirty.dirty);
}

/// Spec, Folders, branches and integration: "Each task gets a branch. The
/// hand-out names the base commit (the integration tip) and the branch; the
/// member's agent creates it."
#[test]
fn production_report_reads_the_member_created_task_branch() {
    let h = Harness::new();
    let o = owned(&h);
    git(&o.wt, &o.empty, &["switch", "-q", "-c", "teams/task-1"]);
    let head = commit(&o, "task one on its own branch");
    let team = h.store.team(TEAM).unwrap().unwrap();
    let r = native(&h).work_reported(&team, &member(&o));
    eprintln!("REVIEW08 task-branch report (HEAD {head}): {r:?}");
    let report = r.expect("the owned task branch must be reportable");
    assert_eq!(report.commit, head);
    assert_eq!(report.branch, "teams/task-1");
}

#[test]
fn production_report_refuses_a_replaced_worktree_at_the_same_path() {
    let h = Harness::new();
    let o = owned(&h);
    commit(&o, "original");
    git(
        &o.root,
        &o.empty,
        &["worktree", "remove", "--force", o.wt.to_str().unwrap()],
    );
    git(
        &o.root,
        &o.empty,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "replacement",
            o.wt.to_str().unwrap(),
        ],
    );
    let team = h.store.team(TEAM).unwrap().unwrap();
    let r = native(&h).work_reported(&team, &member(&o));
    eprintln!("REVIEW08 replaced worktree report: {r:?}");
    assert!(r.is_err());
}

impl Drop for Owned {
    fn drop(&mut self) {
        native_work::TEST_REPO.with(|s| *s.borrow_mut() = None);
        native_work::AFTER_REPORTED.with(|s| {
            s.borrow_mut().take();
        });
        crate::teams::integrate::BEFORE_ADMISSION.with(|s| {
            s.borrow_mut().take();
        });
        crate::teams::integrate::BEFORE_REF_WRITE.with(|s| {
            s.borrow_mut().take();
        });
    }
}
#[test]
fn production_report_checks_ownership_before_reading_the_head() {
    let h = Harness::new();
    let o = owned(&h);
    commit(&o, "original");
    let mut m = member(&o);
    m.ownership_nonce = uuid::Uuid::new_v4().to_string();
    let read = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = read.clone();
    native_work::AFTER_REPORTED
        .with(|s| *s.borrow_mut() = Some(Box::new(move || observed.set(true))));
    let t = h.store.team(TEAM).unwrap().unwrap();
    assert!(native(&h).work_reported(&t, &m).is_err());
    assert!(!read.get(), "an unowned checkout's HEAD was read");
}
#[test]
fn production_report_checks_ownership_again_after_reading_the_head() {
    let h = Harness::new();
    let o = owned(&h);
    commit(&o, "original");
    let common = o.root.join(".git");
    let receipt = crate::state::team_worktree_receipt_path(h.temp.path(), &common, &o.wt);
    let metadata = PathBuf::from(git(&o.wt, &o.empty, &["rev-parse", "--absolute-git-dir"]));
    native_work::AFTER_REPORTED.with(|s| {
        *s.borrow_mut() = Some(Box::new(move || {
            let mut owner: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
            owner["id"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
            std::fs::write(receipt, owner.to_string()).unwrap();
            std::fs::write(metadata.join("zerus-owner.json"), owner.to_string()).unwrap();
        }))
    });
    let t = h.store.team(TEAM).unwrap().unwrap();
    assert!(
        native(&h).work_reported(&t, &member(&o)).is_err(),
        "ownership changed during the read"
    );
}

#[test]
fn production_recovery_returns_the_integration_tip_and_a_known_negative() {
    let h = Harness::new();
    let o = owned(&h);
    let incoming = commit(&o, "member task");
    git(
        &o.root,
        &o.empty,
        &["merge", "--no-ff", "--no-edit", "zerus/team/member-launch"],
    );
    let tip = git(&o.root, &o.empty, &["rev-parse", "HEAD"]);
    assert_ne!(tip, incoming);
    let t = h.store.team(TEAM).unwrap().unwrap();
    git(&o.root, &o.empty, &["branch", &t.integration_branch, &tip]);
    let mut plan = ports::MergePlan {
        team: t.id.clone(),
        owner: t.owner.clone(),
        machine: t.home.clone(),
        project: t.project.clone(),
        branch: t.integration_branch.clone(),
        base: t.base_commit.clone(),
        repo: o.root.join(".git").to_string_lossy().into_owned(),
        node: 1,
        generation: 1,
        decision: 1,
        member: "m1".into(),
        commit: incoming,
        id: uuid::Uuid::new_v4().to_string(),
    };
    assert_eq!(native(&h).work_contains(&plan).unwrap(), Some(tip.clone()));
    plan.commit = commit(&o, "not integrated");
    assert_eq!(native(&h).work_contains(&plan).unwrap(), None);
    assert_eq!(
        git(&o.root, &o.empty, &["rev-parse", &t.integration_branch]),
        tip
    );
}

fn merge_fixture() -> (Harness, Owned, ports::MergePlan) {
    let mut h = Harness::empty();
    let o = owned(&h);
    let base = git(&o.root, &o.empty, &["rev-parse", "HEAD"]);
    let commit = commit(&o, "member report");
    let mut op = create();
    if let Operation::Create {
        base_commit,
        integration_branch,
        ..
    } = &mut op
    {
        *base_commit = base.clone();
        *integration_branch = "teams/integration".into();
    }
    h.run(&person(), op).unwrap();
    h.bind("lead");
    h.bind("m1");
    h.add(&person(), 1);
    h.submit(1, "m1");
    h.observe(1, ops::TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node: 1,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: commit.clone(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    h.run(&person(), Operation::Accept { node: 1 }).unwrap();
    let t = h.store.team(TEAM).unwrap().unwrap();
    git(&o.root, &o.empty, &["branch", &t.integration_branch, &base]);
    let plan = ports::MergePlan {
        team: t.id,
        owner: t.owner,
        machine: t.home,
        project: t.project,
        branch: t.integration_branch,
        base,
        repo: o.root.join(".git").to_string_lossy().into_owned(),
        node: 1,
        generation: 1,
        decision: h.accept_decision(1),
        member: "m1".into(),
        commit,
        id: uuid::Uuid::new_v4().to_string(),
    };
    (h, o, plan)
}
#[test]
fn pause_committed_during_tree_computation_prevents_the_ref_update() {
    let (h, o, plan) = merge_fixture();
    let state = h.temp.path().to_owned();
    crate::teams::integrate::BEFORE_ADMISSION.with(|s| {
        *s.borrow_mut() = Some(Box::new(move || {
            let mut store = Store::open(&state).unwrap();
            let t = store.team(TEAM).unwrap().unwrap();
            ops::apply(
                &mut store,
                &person(),
                &crate::teams::ops::tests::request(t.revision, Operation::Pause),
                100,
            )
            .unwrap();
        }))
    });
    let result = native(&h).work_merge(&plan);
    eprintln!("MERGE_ADMISSION_RESULT {result:?}");
    assert!(
        matches!(result, ports::MergeResult::Unavailable { .. }),
        "paused merge still moved the ref: {result:?}"
    );
    assert_eq!(
        git(&o.root, &o.empty, &["rev-parse", &plan.branch]),
        plan.base
    );
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Paused
    );
}
#[test]
fn a_ready_accept_is_checked_again_at_the_ref_update() {
    let (h, o, plan) = merge_fixture();
    let state = h.temp.path().to_owned();
    let decision = plan.decision;
    crate::teams::integrate::BEFORE_ADMISSION.with(|s| {
        *s.borrow_mut() = Some(Box::new(move || {
            // Fixture the store owner's atomic demotion seam. Ready, rather than
            // a copy of the delegation flag in this actor, is the authority.
            let store = Store::open(&state).unwrap();
            store
                .db
                .as_ref()
                .unwrap()
                .execute(
                    "UPDATE decisions SET state=?2 WHERE seq=?1",
                    rusqlite::params![decision, crate::teams::store::DecisionState::AwaitingOwner],
                )
                .unwrap();
        }))
    });
    let result = native(&h).work_merge(&plan);
    assert!(
        matches!(result, ports::MergeResult::Unavailable { .. }),
        "a demoted acceptance still moved the ref: {result:?}"
    );
    assert_eq!(
        git(&o.root, &o.empty, &["rev-parse", &plan.branch]),
        plan.base
    );
}
#[test]
fn the_store_admission_is_held_only_across_the_ref_writer() {
    let (h, o, plan) = merge_fixture();
    let path = h.temp.path().join("teams/teams-v1.sqlite3");
    let checked = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = checked.clone();
    crate::teams::integrate::BEFORE_REF_WRITE.with(|s| {
        *s.borrow_mut() = Some(Box::new(move || {
            let db = rusqlite::Connection::open(path).unwrap();
            db.busy_timeout(std::time::Duration::ZERO).unwrap();
            assert!(
                db.execute_batch("BEGIN IMMEDIATE").is_err(),
                "the ref writer lost its admission"
            );
            observed.set(true);
        }))
    });
    let result = native(&h).work_merge(&plan);
    assert!(
        matches!(result, ports::MergeResult::Merged { .. }),
        "{result:?}"
    );
    assert!(checked.get());
    let guard = Store::open(h.temp.path()).unwrap().admission().unwrap();
    drop(guard);
    assert_ne!(
        git(&o.root, &o.empty, &["rev-parse", &plan.branch]),
        plan.base
    );
}

#[test]
fn an_explicit_ready_accept_survives_the_merge_delegation_being_off() {
    let (mut h, o, plan) = merge_fixture();
    // This fixture's acceptance is the person's, so the store keeps it Ready.
    h.settings(|s| s.delegations.merge_accepted = false);
    assert_eq!(
        h.store.pending_accepts(TEAM, 200, None).unwrap().rows[0].seq,
        plan.decision
    );
    let result = native(&h).work_merge(&plan);
    assert!(
        matches!(result, ports::MergeResult::Merged { .. }),
        "{result:?}"
    );
    assert_ne!(
        git(&o.root, &o.empty, &["rev-parse", &plan.branch]),
        plan.base
    );
}

struct PauseAtDelivery {
    native: native::Native,
    team_pause: bool,
    calls: usize,
}
impl ports::Ports for PauseAtDelivery {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> ports::Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> ports::Result<()> {
        Ok(())
    }
    fn free(&mut self, _: &Team, m: &Member) -> ports::Result<bool> {
        Ok(m.id == "m1")
    }
    fn deliver(&mut self, t: &Team, n: &Node, m: &Member, id: &str) -> ports::DeliveryResult {
        self.calls += 1;
        let mut store = Store::open(&self.native.config.state).unwrap();
        assert!(store
            .open_effects(&t.id, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|e| e.request_id == id && e.state == "reserved"));
        let current = store.team(&t.id).unwrap().unwrap();
        let operation = if self.team_pause {
            Operation::Pause
        } else {
            Operation::PauseMember {
                member: m.id.clone(),
            }
        };
        ops::apply(
            &mut store,
            &person(),
            &crate::teams::ops::tests::request(current.revision, operation),
            100,
        )
        .unwrap();
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
            *s.borrow_mut() = Some(
                serde_json::json!({"schema":1,"id":id,"state":"submitted","error":""})
                    .to_string()
                    .into_bytes(),
            )
        });
        ports::Ports::deliver(&mut self.native, t, n, m, id)
    }
}
#[test]
fn pause_between_handout_intent_and_delivery_records_refusal_without_an_assignee() {
    for team_pause in [true, false] {
        let mut h = Harness::new();
        h.bind("m1");
        h.bind("m2");
        let node = h.add(&person(), 1);
        let machine = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
        let mut p = PauseAtDelivery {
            native: native(&h),
            team_pause,
            calls: 0,
        };
        a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
        assert_eq!(p.calls, 1);
        let effect = h.store.effects(TEAM, 200, None).unwrap().rows.remove(0);
        assert_eq!(effect.state, "refused");
        let outcome: ops::HandoutOutcome = serde_json::from_value(effect.outcome.unwrap()).unwrap();
        assert!(matches!(outcome, ops::HandoutOutcome::Refused { .. }));
        assert!(h
            .store
            .node(TEAM, node)
            .unwrap()
            .unwrap()
            .assignee
            .is_none());
    }
}

mod admission_r2;

pub(super) mod fleet;

mod deferred;

mod reviewed;

mod answers;

mod handovers;
