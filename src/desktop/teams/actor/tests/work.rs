use super::super::ports::{Finish, MergePlan, MergeResult, Ports};
use super::*;
use crate::teams::{
    integrate,
    ops::{HandoutOutcome, Operation, TaskStage},
};
#[derive(Default)]
struct Work {
    text: Option<String>,
    fail_observe: bool,
    allow_delivery: bool,
    deliveries: u32,
    start_ms: Option<i64>,
    clock_check: Option<std::path::PathBuf>,
    dirty: bool,
    reports: u32,
    heads: u32,
    merges: u32,
    finishes: u32,
    contains: Option<bool>,
    merge_reply: Option<MergeResult>,
    finish_reply: Option<Finish>,
    phase: Vec<&'static str>,
    ids: Vec<String>,
    lock_after_merge: Option<std::path::PathBuf>,
    held: Option<rusqlite::Connection>,
    advance: Option<std::path::PathBuf>,
    repo: Option<integrate::Repo>,
    stop_team: Option<std::path::PathBuf>,
    stop: bool,
    last_plan: Option<MergePlan>,
    conflicts_for: Option<(u32, Vec<String>)>,
    demote_after_merge: Option<std::path::PathBuf>,
}
impl Ports for Work {
    fn clock_ms(&self, fallback: i64) -> i64 {
        self.start_ms.unwrap_or(fallback)
    }
    fn stopped(&self) -> bool {
        self.stop
    }
    fn changes(&mut self, _: &str, g: &str, _: u64) -> Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: if g.is_empty() {
                None
            } else {
                Some(Default::default())
            },
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
        self.phase.push("observe");
        if self.fail_observe {
            return Err(super::super::problem("Fixture task observation failed"));
        }
        Ok(())
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        Ok(self.allow_delivery)
    }
    fn deliver(
        &mut self,
        _: &Team,
        _: &Node,
        _: &Member,
        _: &str,
    ) -> super::super::ports::DeliveryResult {
        assert!(self.allow_delivery, "work fixture must not deliver");
        self.deliveries += 1;
        HandoutOutcome::Submitted {
            receipt: "fixture hand-over".into(),
        }
        .into()
    }
    fn report(&mut self, _: &TaskRef) -> Result<Option<String>> {
        self.phase.push("report");
        self.reports += 1;
        Ok(self.text.clone())
    }
    fn reported(&mut self, _: &Team, _: &Member) -> Result<integrate::Reported> {
        self.heads += 1;
        Ok(integrate::Reported {
            commit: COMMIT.into(),
            branch: "teams/member".into(),
            dirty: self.dirty,
        })
    }
    fn repository(&mut self, _: &Team) -> Result<String> {
        Ok(self
            .repo
            .as_ref()
            .map(|r| r.git_dir.to_string_lossy().into_owned())
            .unwrap_or("fixture-repo".into()))
    }
    fn descends(&mut self, plan: &MergePlan) -> Result<bool> {
        self.phase.push("descends");
        if let Some(repo) = &self.repo {
            return integrate::descends(repo, &plan.commit, &plan.base);
        }
        Ok(true)
    }
    fn contains(&mut self, _: &MergePlan) -> Result<Option<String>> {
        self.phase.push("contains");
        self.contains
            .map(|yes| yes.then(|| "1123456789abcdef0123456789abcdef01234567".into()))
            .ok_or_else(|| super::super::problem("Fixture ancestry unreadable"))
    }
    fn merge(&mut self, plan: &MergePlan) -> MergeResult {
        if let Some(path) = &self.clock_check {
            let db = rusqlite::Connection::open(path).unwrap();
            let stamp:i64=db.query_row("SELECT json_extract(data,'$.reserved_ms') FROM actor_rows WHERE team=?1 AND key=?2",rusqlite::params![plan.team,format!("merge:{}",plan.decision)],|r|r.get(0)).unwrap();
            assert_eq!(
                stamp,
                self.start_ms.unwrap(),
                "the Git start still uses the page clock"
            );
        }
        self.phase.push("merge");
        self.merges += 1;
        self.ids.push(plan.id.clone());
        self.last_plan = Some(plan.clone());
        if let Some(path) = self.stop_team.take() {
            let mut store = Store::open(&path).unwrap();
            apply(&mut store, &person(), Operation::Stop);
        }
        if let Some(state) = self.demote_after_merge.take() {
            let store = Store::open(&state).unwrap();
            store
                .db
                .as_ref()
                .unwrap()
                .execute(
                    "UPDATE decisions SET state='awaiting_owner' WHERE seq=?1",
                    [plan.decision],
                )
                .unwrap();
        }
        if let Some(path) = self.advance.take() {
            advance_attempt(&path);
        }
        if let Some(path) = self.lock_after_merge.take() {
            let db = rusqlite::Connection::open(path).unwrap();
            db.execute_batch("BEGIN IMMEDIATE").unwrap();
            self.held = Some(db);
        }
        if let Some(repo) = &self.repo {
            return match integrate::merge(repo, &plan.branch, &plan.commit, "Teams fixture accept")
                .unwrap()
            {
                integrate::Merge::Merged { commit } => MergeResult::Merged { commit },
                integrate::Merge::Conflict { files } => MergeResult::Conflict { files },
                integrate::Merge::Stale => panic!("fixture ref moved"),
            };
        }
        if let Some((node, files)) = &self.conflicts_for {
            if *node == plan.node {
                return MergeResult::Conflict {
                    files: files.clone(),
                };
            }
        }
        self.merge_reply.clone().unwrap_or(MergeResult::Merged {
            commit: COMMIT.into(),
        })
    }
    fn finish(
        &mut self,
        _: &Team,
        _: &Node,
        _: &crate::teams::store::Decision,
        _: &str,
    ) -> Result<Finish> {
        self.phase.push("finish");
        self.finishes += 1;
        Ok(self.finish_reply.clone().unwrap_or(Finish::Done))
    }
}
fn run(h: &mut Harness, a: &mut super::super::Runtime, p: &mut Work, now: i64) {
    a.begin_pass();
    a.tick(&mut h.store, p, TEAM, now, false).unwrap();
}
fn actor(h: &Harness) -> super::super::Runtime {
    super::super::Runtime::open(h.temp.path(), &h.store.team(TEAM).unwrap().unwrap().home).unwrap()
}
#[test]
fn report_waits_for_text_and_records_the_owned_head_with_a_dirty_note_once() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, TaskStage::InReview);
    let mut a = actor(&h);
    let mut p = Work::default();
    run(&mut h, &mut a, &mut p, 100);
    assert!(h.store.report(TEAM, node, 1).unwrap().is_none());
    assert_eq!(p.heads, 0);
    run(&mut h, &mut a, &mut p, 101);
    assert_eq!(
        p.reports, 1,
        "unchanged wake must not reread the task report"
    );
    p.text = Some("Holder report".into());
    p.dirty = true;
    h.observe(node, TaskStage::InReview);
    run(&mut h, &mut a, &mut p, 102);
    let report = h.store.report(TEAM, node, 1).unwrap().unwrap();
    assert_eq!(report.member, "m1");
    assert_eq!(report.commit, COMMIT);
    assert_eq!(
        report.text, "Holder report",
        "the service's statement is not part of the member's text"
    );
    assert_eq!(
        report.notes,
        vec![crate::teams::ops::ReportNote::new("worktree_dirty")]
    );
    let inbox = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|item| item.node == node && item.kind == "report")
        .unwrap();
    assert!(
        inbox.text.contains("> Holder report"),
        "the member's block is quoted: {}",
        inbox.text
    );
    assert!(
        !inbox
            .text
            .contains("The member's worktree had uncommitted changes."),
        "the note is not in the member's block: {}",
        inbox.text
    );
    assert_eq!(
        inbox.notes,
        vec![crate::teams::ops::ReportNote::new("worktree_dirty")]
    );
    drop(a);
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 103);
    assert_eq!(p.heads, 1);
}
/// A member's report keeps its lines and tabs and loses every other control
/// character before it is bounded and recorded (zerus-63rru.59); the lead's
/// items carry it the same way.
#[test]
fn a_reports_control_characters_are_removed_and_its_lines_and_tabs_kept() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, TaskStage::InReview);
    let mut a = actor(&h);
    let mut p = Work {
        text: Some("Done:\r\n- one\tok\u{1b}[2J\u{7}\n- two\u{2028}- three\u{9b}31m\u{0}".into()),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    let report = h.store.report(TEAM, node, 1).unwrap().unwrap();
    assert_eq!(report.text, "Done:\n- one\tok[2J\n- two\n- three31m");
    for item in h.store.inbox(TEAM, 200, None).unwrap().rows {
        assert!(
            !item
                .text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
            "{:?}",
            item.text
        );
    }
}
/// A member line that reads exactly like a service sentence stays inside the
/// member's text; the service's own note is a separate field. A report that
/// ends inside a right-to-left override loses it, so the text cannot reorder
/// a reader's line (zerus-63rru.59).
#[test]
fn a_member_line_that_reads_like_a_service_sentence_stays_in_its_text() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, TaskStage::InReview);
    let mut a = actor(&h);
    let mut p = Work {
        text: Some("All done\nWorktree has uncommitted changes.\u{202E}".into()),
        dirty: true,
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    let report = h.store.report(TEAM, node, 1).unwrap().unwrap();
    assert_eq!(
        report.text, "All done\nWorktree has uncommitted changes.",
        "the member's line is kept exactly, even though it reads like the service's"
    );
    assert_eq!(
        report.notes,
        vec![crate::teams::ops::ReportNote::new("worktree_dirty")]
    );
    let inbox = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|item| item.node == node && item.kind == "report")
        .unwrap();
    assert!(inbox.text.contains("> All done\n> Worktree has uncommitted changes."));
    assert!(
        !inbox
            .text
            .contains("The member's worktree had uncommitted changes."),
        "{}",
        inbox.text
    );
    assert_eq!(
        inbox.notes,
        vec![crate::teams::ops::ReportNote::new("worktree_dirty")]
    );
}

/// A report over the member-text bound is cut and the fact is a service note,
/// never a line inside the member's text.
#[test]
fn a_shortened_report_keeps_its_text_and_says_so_in_its_notes() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, TaskStage::InReview);
    let mut a = actor(&h);
    let long = "x".repeat(16_000);
    let mut p = Work {
        text: Some(long.clone()),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    let report = h.store.report(TEAM, node, 1).unwrap().unwrap();
    assert_eq!(report.text.len(), 15_000);
    assert_eq!(report.text, &long[..15_000]);
    assert_eq!(
        report.notes,
        vec![crate::teams::ops::ReportNote::new("text_shortened")]
    );
    let inbox = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|item| item.node == node && item.kind == "report")
        .unwrap();
    assert!(!inbox.text.contains("[Report text shortened.]"));
    assert_eq!(
        inbox.notes,
        vec![crate::teams::ops::ReportNote::new("text_shortened")]
    );
}
#[test]
fn integration_records_the_exact_decision_before_finishing() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    let mut a = actor(&h);
    let mut p = Work::default();
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.merges, 1);
    assert_eq!(p.finishes, 1);
    let f = h.store.integration_fact(TEAM, node).unwrap().unwrap();
    assert_eq!(f.decision, decision);
    assert_eq!(f.commit.as_deref(), Some(COMMIT));
    assert!(h.node(node).integrated);
    let merge = p.phase.iter().position(|s| *s == "merge").unwrap();
    let done = p.phase.iter().position(|s| *s == "finish").unwrap();
    assert!(merge < done);
}
#[test]
fn unknown_merge_is_only_read_and_makes_one_card_until_ancestry_settles_it() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    let mut a = actor(&h);
    let mut p = Work {
        merge_reply: Some(MergeResult::Unknown {
            reason: "Lost original write result".into(),
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    drop(a);
    let mut a = actor(&h);
    for now in [101, 60_101, 180_101] {
        run(&mut h, &mut a, &mut p, now);
    }
    assert_eq!(p.merges, 1);
    assert_eq!(p.finishes, 0);
    assert_eq!(
        h.store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .filter(|c| c.state == "open")
            .count(),
        1
    );
    p.contains = Some(true);
    run(&mut h, &mut a, &mut p, 900_101);
    assert_eq!(p.merges, 1);
    assert_eq!(
        h.store
            .integration_fact(TEAM, node)
            .unwrap()
            .unwrap()
            .commit
            .as_deref(),
        Some("1123456789abcdef0123456789abcdef01234567")
    );
    assert_eq!(
        h.store
            .integration_fact(TEAM, node)
            .unwrap()
            .unwrap()
            .decision,
        decision
    );
    assert_eq!(p.finishes, 1);
}
#[test]
fn an_integration_refusal_before_start_retries_with_backoff() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        merge_reply: Some(MergeResult::Unavailable {
            reason: "Git busy before write".into(),
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    run(&mut h, &mut a, &mut p, 101);
    assert_eq!(p.merges, 1);
    p.merge_reply = None;
    run(&mut h, &mut a, &mut p, 60_101);
    assert_eq!(p.merges, 2);
    assert!(h.node(node).integrated);
}
#[test]
fn no_merge_accepts_finish_and_a_refusal_waits_for_observation_or_decision_change() {
    let mut h = Harness::new();
    h.settings(|s| s.integration = Integration::None);
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        finish_reply: Some(Finish::Refused("Task key differs".into())),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    for now in [101, 60_101, 900_101] {
        run(&mut h, &mut a, &mut p, now);
    }
    assert_eq!(p.merges, 0);
    assert_eq!(p.finishes, 1);
    h.observe(node, TaskStage::InReview);
    p.finish_reply = None;
    run(&mut h, &mut a, &mut p, 900_102);
    assert_eq!(p.finishes, 2);
    h.observe(node, TaskStage::Done);
    run(&mut h, &mut a, &mut p, 960_103);
    assert_eq!(p.finishes, 2);
}
#[test]
fn finishing_cutover_retries_with_backoff_and_unchanged_wakes_do_not_repeat_done() {
    let mut h = Harness::new();
    h.settings(|s| s.integration = Integration::None);
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        finish_reply: Some(Finish::CuttingOver("Cutover".into())),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    run(&mut h, &mut a, &mut p, 101);
    assert_eq!(p.finishes, 1);
    p.finish_reply = None;
    run(&mut h, &mut a, &mut p, 60_101);
    run(&mut h, &mut a, &mut p, 60_102);
    assert_eq!(p.finishes, 2);
}
fn apply(store: &mut Store, who: &crate::teams::ops::Actor, op: Operation) {
    let revision = store.team(TEAM).unwrap().unwrap().revision;
    crate::teams::ops::apply(store, who, &request(revision, op), 1234).unwrap();
}
fn observation(store: &mut Store, stage: TaskStage) {
    let old = store.task_observation(TEAM, 1).unwrap().unwrap();
    apply(
        store,
        &service(),
        Operation::ObserveTask {
            node: 1,
            observation: crate::teams::ops::TaskObservation {
                revision: uuid::Uuid::new_v4().to_string(),
                previous_revision: old.revision.clone(),
                stage,
                ..old
            },
        },
    );
}
fn advance_attempt(path: &std::path::Path) {
    let mut store = Store::open(path).unwrap();
    observation(&mut store, TaskStage::Done);
    observation(&mut store, TaskStage::Todo);
    let id = uuid::Uuid::new_v4().to_string();
    apply(
        &mut store,
        &service(),
        Operation::HandoutIntent {
            node: 1,
            member: "m1".into(),
            effect_request_id: id.clone(),
        },
    );
    apply(
        &mut store,
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: id,
            outcome: HandoutOutcome::Submitted {
                receipt: "new attempt".into(),
            },
        },
    );
    observation(&mut store, TaskStage::InReview);
    apply(
        &mut store,
        &service(),
        Operation::RecordReport {
            node: 1,
            generation: 2,
            member: "m1".into(),
            text: "new report".into(),
            commit: "1123456789abcdef0123456789abcdef01234567".into(),
            notes: Vec::new(),
        },
    );
    apply(&mut store, &person(), Operation::Accept { node: 1 });
}
#[test]
fn integration_fact_never_uses_a_newer_pending_decision() {
    for outcome in [
        MergeResult::Merged {
            commit: COMMIT.into(),
        },
        MergeResult::Conflict {
            files: vec!["conflict.txt".into()],
        },
    ] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let old = h.accept_decision(node);
        let mut a = actor(&h);
        let mut p = Work {
            advance: Some(h.temp.path().to_owned()),
            merge_reply: Some(outcome),
            ..Default::default()
        };
        run(&mut h, &mut a, &mut p, 100);
        let new = h.accept_decision(node);
        assert_ne!(old, new);
        assert_eq!(
            h.store
                .integration_fact(TEAM, node)
                .unwrap()
                .unwrap()
                .decision,
            old
        );
        assert!(!h.node(node).integrated);
        assert!(h
            .store
            .pending_accepts(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|d| d.seq == new));
        assert_eq!(p.finishes, 0);
    }
}
#[test]
fn restart_at_every_merge_boundary_never_repeats_a_write() {
    for point in ["reserved", "effect", "outcome", "recorded"] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let mut a = actor(&h);
        let mut p = Work::default();
        super::super::integration::CRASH.with(|c| c.set(Some(point)));
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(&mut h, &mut a, &mut p, 100)
        }));
        assert!(crashed.is_err(), "{point}");
        let expected = u32::from(point != "reserved");
        assert_eq!(p.merges, expected);
        drop(a);
        let mut a = actor(&h);
        for now in [101, 60_101, 180_101] {
            run(&mut h, &mut a, &mut p, now);
        }
        assert_eq!(p.merges, expected, "restart continued {point}");
        if matches!(point, "outcome" | "recorded") {
            assert!(h.node(node).integrated);
        } else {
            assert!(!h.node(node).integrated);
            assert_eq!(
                h.store
                    .cards(TEAM, 200, None)
                    .unwrap()
                    .rows
                    .iter()
                    .filter(|c| c.state == "open")
                    .count(),
                1
            );
        }
    }
}
#[test]
fn losing_a_merge_result_to_a_ledger_lock_never_repeats_the_write() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        lock_after_merge: Some(h.temp.path().join("teams/actor-v1.sqlite3")),
        ..Default::default()
    };
    a.begin_pass();
    assert!(a.tick(&mut h.store, &mut p, TEAM, 100, false).is_err());
    assert_eq!(p.merges, 1);
    drop(p.held.take());
    drop(a);
    let mut a = actor(&h);
    for now in [101, 60_101, 180_101] {
        run(&mut h, &mut a, &mut p, now);
    }
    assert_eq!(p.merges, 1);
    assert!(!h.node(node).integrated);
}
#[test]
fn a_lost_fact_write_is_retried_as_a_fact_with_no_second_merge() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        lock_after_merge: Some(h.temp.path().join("teams/teams-v1.sqlite3")),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.merges, 1);
    drop(p.held.take());
    drop(a);
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 60_101);
    assert_eq!(p.merges, 1);
    assert!(h.node(node).integrated);
}
mod git;
#[test]
fn report_restart_before_and_after_the_fact_keeps_one_report() {
    for point in ["read", "recorded"] {
        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        h.submit(node, "m1");
        h.observe(node, TaskStage::InReview);
        let mut a = actor(&h);
        let mut p = Work {
            text: Some("Holder report".into()),
            ..Default::default()
        };
        super::super::reports::CRASH.with(|c| c.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(
                &mut h, &mut a, &mut p, 100
            )))
            .is_err()
        );
        drop(a);
        let mut a = actor(&h);
        run(&mut h, &mut a, &mut p, 101);
        assert!(h.store.report(TEAM, node, 1).unwrap().is_some());
        assert_eq!(
            h.store
                .inbox(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .filter(|r| r.kind == "report")
                .count(),
            1
        );
        assert_eq!(p.heads, if point == "read" { 2 } else { 1 });
    }
}
#[test]
fn finishing_may_repeat_after_restart_until_observation_settles_it() {
    for point in ["before", "after"] {
        let mut h = Harness::new();
        h.settings(|s| s.integration = Integration::None);
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let mut a = actor(&h);
        let mut p = Work::default();
        super::super::finish::CRASH.with(|c| c.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(
                &mut h, &mut a, &mut p, 100
            )))
            .is_err()
        );
        drop(a);
        let mut a = actor(&h);
        run(&mut h, &mut a, &mut p, 101);
        assert_eq!(p.finishes, if point == "before" { 1 } else { 2 });
        h.observe(node, TaskStage::Done);
        run(&mut h, &mut a, &mut p, 102);
        assert_eq!(p.finishes, if point == "before" { 1 } else { 2 });
    }
}
#[test]
fn a_negative_ancestry_read_never_authorizes_another_merge() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        contains: Some(false),
        merge_reply: Some(MergeResult::Unknown {
            reason: "Lost receipt".into(),
        }),
        ..Default::default()
    };
    for now in [100, 60_101, 180_101, 900_101] {
        run(&mut h, &mut a, &mut p, now);
    }
    assert_eq!(p.merges, 1);
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
}
#[test]
fn paused_teams_take_in_reports_but_start_no_merge_or_finish() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, TaskStage::InReview);
    h.run(&person(), Operation::Pause).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        text: Some("Report while paused".into()),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    assert!(h.store.report(TEAM, node, 1).unwrap().is_some());
    assert_eq!(p.merges, 0);
    assert_eq!(p.finishes, 0);
}
#[test]
fn a_standing_team_prunes_finish_cooldowns_after_observation() {
    let mut h = Harness::new();
    h.settings(|s| s.integration = Integration::None);
    let lead = h.bind("lead");
    h.bind("m1");
    let mut a = actor(&h);
    let mut p = Work::default();
    for n in 1..=20 {
        let node = h.add(&person(), n);
        h.submit(node, "m1");
        h.report(node, "m1");
        h.run(&lead, Operation::Accept { node }).unwrap();
        run(&mut h, &mut a, &mut p, i64::from(n) * 60_000);
        h.observe(node, TaskStage::Done);
        run(&mut h, &mut a, &mut p, i64::from(n) * 60_000 + 1);
    }
    assert!(a
        .ledger
        .prefix_page(TEAM, "gate:finish", "")
        .unwrap()
        .is_empty());
    assert_eq!(p.finishes, 20);
}
#[test]
fn a_known_outcome_waiting_for_its_fact_does_not_stop_other_rows() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let other = h.add(&person(), 2);
    h.submit(other, "m1");
    h.report(other, "m1");
    h.run(&lead, Operation::Accept { node: other }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        lock_after_merge: Some(h.temp.path().join("teams/teams-v1.sqlite3")),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.merges, 2);
    drop(p.held.take());
    run(&mut h, &mut a, &mut p, 60_101);
    assert_eq!(p.merges, 2);
    assert!(h.node(node).integrated && h.node(other).integrated);
}
#[test]
fn service_stop_starts_no_new_work_and_an_inflight_known_merge_still_records_its_fact() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    let mut a = actor(&h);
    let mut p = Work {
        stop: true,
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    assert!(p.phase.is_empty());
    p.stop = false;
    p.stop_team = Some(h.temp.path().to_owned());
    run(&mut h, &mut a, &mut p, 101);
    assert_eq!(p.merges, 1);
    assert_eq!(p.finishes, 0);
    assert_eq!(
        h.store
            .integration_fact(TEAM, node)
            .unwrap()
            .unwrap()
            .decision,
        decision
    );
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Stopped
    );
}
#[test]
fn merge_recovery_pages_past_backed_off_rows() {
    let h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let mut a = actor(&h);
    let mut p = Work::default();
    for i in 0..250 {
        let key = format!("merge:{i:08}");
        let node = i + 1000;
        let plan = MergePlan {
            team: TEAM.into(),
            owner: t.owner.clone(),
            machine: t.home.clone(),
            project: t.project.clone(),
            branch: t.integration_branch.clone(),
            base: t.base_commit.clone(),
            repo: "fixture-repo".into(),
            node,
            generation: 1,
            decision: u64::from(node),
            member: "m1".into(),
            commit: COMMIT.into(),
            id: uuid::Uuid::new_v4().to_string(),
        };
        let outcome = if i < 249 {
            serde_json::json!({"Merged":{"commit":COMMIT}})
        } else {
            serde_json::json!({"Unavailable":{"reason":"Known no write"}})
        };
        a.put(
            TEAM,
            &key,
            &serde_json::json!({"plan":plan,"outcome":outcome,"fact_id":null,"fact_revision":null}),
        )
        .unwrap();
        if i < 249 {
            a.put(TEAM,&format!("gate:integration:{node}"),&serde_json::json!({"revision":format!("{node}:1"),"failures":1,"at":1_000_000,"card":null,"notice":null})).unwrap();
        }
    }
    let mut store = h.store;
    a.recover_merges(&mut store, &mut p, &t, 100).unwrap();
    a.recover_merges(&mut store, &mut p, &t, 101).unwrap();
    assert!(a.ledger.get(TEAM, "merge:00000249").unwrap().is_none());
    assert_eq!(p.merges, 0);
}
#[test]
fn retry_after_a_saved_unavailable_result_checks_again_instead_of_replaying_the_failure() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    let mut a = actor(&h);
    let mut p = Work {
        merge_reply: Some(MergeResult::Unavailable {
            reason: "Not started".into(),
        }),
        ..Default::default()
    };
    for now in [100, 60_100, 180_100] {
        run(&mut h, &mut a, &mut p, now);
    }
    assert_eq!(p.merges, 3);
    let card = h.store.cards(TEAM, 200, None).unwrap().rows[0].seq;
    a.put(TEAM,&format!("merge:{decision}"),&serde_json::json!({"plan":p.last_plan,"outcome":{"Unavailable":{"reason":"Not started"}},"fact_id":null,"fact_revision":null})).unwrap();
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "retry".into(),
        },
    )
    .unwrap();
    p.merge_reply = None;
    run(&mut h, &mut a, &mut p, 180_101);
    assert_eq!(p.merges, 4);
    assert!(h.node(node).integrated);
    assert!(h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|c| c.state != "open"));
}
#[test]
fn a_saved_unavailable_outcome_counts_as_one_failure_after_a_restart() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    let mut a = actor(&h);
    let mut p = Work {
        merge_reply: Some(MergeResult::Unavailable {
            reason: "Not started".into(),
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    // The failure was saved, but a crash lost removal of its cached outcome.
    a.put(TEAM,&format!("merge:{decision}"),&serde_json::json!({"plan":p.last_plan,"outcome":{"Unavailable":{"reason":"Not started"}},"fact_id":null,"fact_revision":null})).unwrap();
    drop(a);
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 101);
    let gate = a
        .ledger
        .get(TEAM, &format!("gate:integration:{node}"))
        .unwrap()
        .unwrap();
    assert_eq!(gate.data["failures"], 1);
    assert_eq!(gate.data["at"], 60_100);
    assert_eq!(p.merges, 1);
}

#[test]
fn conflict_facts_bound_three_two_hundred_one_and_three_hundred_paths() {
    for n in [3, 201, 300] {
        let mut h = Harness::new();
        h.bind("m1");
        let lead = h.bind("lead");
        for node in [1, 2] {
            h.add(&person(), node);
            h.submit(node, "m1");
            h.observe(node, TaskStage::InReview);
            h.run(
                &service(),
                Operation::RecordReport {
                    node,
                    generation: 1,
                    member: "m1".into(),
                    text: "Report".into(),
                    commit: COMMIT.into(),
                    notes: Vec::new(),
                },
            )
            .unwrap();
            h.run(&lead, Operation::Accept { node }).unwrap();
        }
        let width = if n == 300 { 5000 } else { 8 };
        let mut p = Work {
            conflicts_for: Some((
                1,
                (0..n)
                    .map(|i| format!("{i:04}/{}", "é".repeat(width)))
                    .collect(),
            )),
            ..Default::default()
        };
        let mut a = actor(&h);
        run(&mut h, &mut a, &mut p, 100);
        let fact = h
            .store
            .integration_fact(TEAM, 1)
            .unwrap()
            .expect("certain conflict was lost");
        assert!(fact.commit.is_none());
        assert!(fact.conflicts.len() <= 200);
        assert!(fact.conflicts.iter().all(|s| s.len() <= 4096));
        if n == 3 {
            assert_eq!(fact.conflicts.len(), 3);
        } else {
            let kept = fact.conflicts.len() - 1;
            assert_eq!(
                fact.conflicts.last().unwrap(),
                &format!("and {} more files", n - kept)
            );
        }
        assert!(
            h.store
                .integration_fact(TEAM, 2)
                .unwrap()
                .unwrap()
                .commit
                .is_some(),
            "a conflict stopped the next independent merge"
        );
        assert_eq!(p.merges, 2);
        assert!(!a.ledger.merge_pending(TEAM).unwrap());
    }
}

#[test]
fn an_unread_merge_fences_only_its_own_decision() {
    let mut h = Harness::new();
    h.bind("m1");
    let lead = h.bind("lead");
    for node in [1, 2] {
        h.add(&person(), node);
        h.submit(node, "m1");
        h.observe(node, TaskStage::InReview);
        h.run(
            &service(),
            Operation::RecordReport {
                node,
                generation: 1,
                member: "m1".into(),
                text: "Report".into(),
                commit: COMMIT.into(),
                notes: Vec::new(),
            },
        )
        .unwrap();
        h.run(&lead, Operation::Accept { node }).unwrap();
    }
    let mut p = Work {
        merge_reply: Some(MergeResult::Unknown {
            reason: "Lost result".into(),
        }),
        ..Default::default()
    };
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.merges, 2, "an unread decision blocked another decision");
    run(&mut h, &mut a, &mut p, 60_100);
    assert_eq!(p.merges, 2, "an unread decision started another original");
}
#[test]
fn a_negative_read_allows_a_new_original_only_after_a_new_epoch_and_deadline() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        contains: Some(false),
        merge_reply: Some(MergeResult::Unknown {
            reason: "Lost result".into(),
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    let first = p.ids[0].clone();
    run(&mut h, &mut a, &mut p, 10_000);
    assert_eq!(p.merges, 1);
    drop(a);
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 20_000);
    assert_eq!(p.merges, 1, "the original deadline has not elapsed");
    p.merge_reply = None;
    run(&mut h, &mut a, &mut p, 60_100);
    assert_eq!(p.merges, 2);
    assert_ne!(p.ids[1], first);
    assert!(h.node(node).integrated);
    assert!(h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|c| c.state != "open"));
}
#[test]
fn stopped_team_maintenance_preserves_unread_merge_evidence() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let d = h.accept_decision(node);
    let mut a = actor(&h);
    let mut p = Work {
        merge_reply: Some(MergeResult::Unknown {
            reason: "Unread".into(),
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    h.run(&person(), Operation::Stop).unwrap();
    a.maintain(&h.store).unwrap();
    assert!(a.ledger.get(TEAM, &format!("merge:{d}")).unwrap().is_some());
}

#[test]
fn finishing_does_not_use_a_previous_generations_acceptance() {
    let mut h = Harness::new();
    h.settings(|s| s.integration = Integration::None);
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let old = h
        .store
        .pending_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .remove(0);
    advance_attempt(h.temp.path());
    let mut a = actor(&h);
    let mut p = Work::default();
    let t = h.store.team(TEAM).unwrap().unwrap();
    a.finish_one(&mut h.store, &mut p, &t, &old, 100, &mut 8)
        .unwrap();
    assert_eq!(p.finishes, 0);
}
#[test]
fn a_reserved_merge_in_a_new_epoch_still_waits_for_its_original_deadline() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        contains: Some(false),
        ..Default::default()
    };
    super::super::integration::CRASH.with(|s| s.set(Some("reserved")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(
            &mut h, &mut a, &mut p, 100
        )))
        .is_err()
    );
    assert_eq!(p.merges, 0);
    drop(a);
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 200);
    assert_eq!(p.merges, 0);
    run(&mut h, &mut a, &mut p, 60_200);
    assert_eq!(p.merges, 1);
}

#[test]
fn a_merge_reserves_room_for_its_whole_result_before_git() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work::default();
    let db = rusqlite::Connection::open(h.temp.path().join("teams/actor-v1.sqlite3")).unwrap();
    db.execute("UPDATE actor_budget SET bytes=33554432-5000 WHERE id=1", [])
        .unwrap();
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(
        p.merges, 0,
        "the result had no durable budget when Git started"
    );
}

#[test]
fn a_refused_conflict_fact_is_settled_after_its_accept_is_demoted() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        demote_after_merge: Some(h.temp.path().to_owned()),
        merge_reply: Some(MergeResult::Conflict {
            files: vec!["conflict.txt".into()],
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    for now in [101, 60_100, 180_100] {
        run(&mut h, &mut a, &mut p, now);
    }
    assert_eq!(p.merges, 1);
    assert!(!a.ledger.merge_pending(TEAM).unwrap());
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    assert!(h.store.cards(TEAM, 200, None).unwrap().rows.is_empty());
    assert!(!h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|r| r.kind == "integration_conflict"));
    // The store owner's approval makes it discoverable for a new original.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE decisions SET state='ready' WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, node],
        )
        .unwrap();
    p.merge_reply = None;
    run(&mut h, &mut a, &mut p, 180_101);
    assert_eq!(p.merges, 2);
    assert!(h.node(node).integrated);
}

#[test]
fn a_failed_conflict_fact_write_keeps_its_known_outcome_until_the_store_recovers() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        lock_after_merge: Some(h.temp.path().join("teams/teams-v1.sqlite3")),
        merge_reply: Some(MergeResult::Conflict {
            files: vec!["conflict.txt".into()],
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.merges, 1);
    assert!(a.ledger.merge_pending(TEAM).unwrap());
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    drop(p.held.take());
    drop(a);
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 60_101);
    assert_eq!(p.merges, 1);
    assert_eq!(
        h.store
            .integration_fact(TEAM, node)
            .unwrap()
            .unwrap()
            .conflicts,
        vec!["conflict.txt"]
    );
    assert!(!a.ledger.merge_pending(TEAM).unwrap());
}

mod r2;

mod r3;

mod reviewed;

mod finish_backoff;

mod held;

/// A finish command's own sentence, refused or cutting over, reaches the
/// retry card cleaned, whichever step carries it.
#[test]
fn a_finish_commands_sentence_reaches_its_card_clean() {
    for refused in [false, true] {
        let mut h = Harness::new();
        h.settings(|s| s.integration = Integration::None);
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let mut a = actor(&h);
        let sentence = super::hostile_sentence();
        let mut p = Work {
            finish_reply: Some(if refused {
                Finish::Refused(sentence)
            } else {
                Finish::CuttingOver(sentence)
            }),
            ..Default::default()
        };
        for now in [100, 60_100, 180_100, 420_100] {
            run(&mut h, &mut a, &mut p, now);
        }
        super::assert_clean(&super::blocked_cards(&h), &format!("refused={refused}"));
    }
}
