//! The organization's write controller (`swarm::sync`) with the real runner
//! against a fake `hgs` (tests/support/fake_hgs_swarm.py) whose `swarm get`
//! and `swarm apply` replies model a second writer, a timed-out apply and its
//! resend, Qt migrating the same rename, and a catalog that changed while the
//! client was stopped with a draft. Never the real CLI.
#![cfg(unix)]

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use zerus_client_core::reads::{self, Read};
use zerus_client_core::swarm::draft;
use zerus_client_core::swarm::edit::{shared, Intent};
use zerus_client_core::swarm::sync::{Controller, Due, EditOutcome, Observation, REFRESH_MS};
use zerus_client_core::swarm::{members, projects, sessions};
use zerus_client_core::{run_limited, ComputerId, Exit};

const R: &str = "workstation\nclaude/sample-project/review";
const R2: &str = "workstation\nclaude/sample-project/review-2";
const OLD: &str = "claude/sample-project/review";
const NEW: &str = "claude/sample-project/review-2";
const LOCAL: &str = "Local catalog. Connect another computer to sync projects.";
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fake {
    dir: PathBuf,
    hgs: PathBuf,
}

impl Fake {
    fn new(config: &Value) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "zerus-swarm-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let hgs = dir.join("hgs");
        // A symlink to the checked-in (executable) script, never a copy (see
        // tests/actions.rs): a new executable written while another test
        // thread forks fails to exec with ETXTBSY. The fake keeps its config,
        // counts and log beside the link.
        std::os::unix::fs::symlink(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_hgs_swarm.py"),
            &hgs,
        )
        .unwrap();
        std::fs::write(dir.join("config.json"), serde_json::to_vec(config).unwrap()).unwrap();
        Self { dir, hgs }
    }

    fn log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("log.ndjson"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn applies(&self) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|entry| entry["argv"] == json!(["swarm", "apply"]))
            .map(|entry| serde_json::from_str(entry["stdin"].as_str().unwrap()).unwrap())
            .collect()
    }

    fn calls(&self) -> Vec<String> {
        self.log()
            .iter()
            .map(|entry| entry["argv"][1].as_str().unwrap().to_owned())
            .collect()
    }

    /// One `swarm get`, as the topic or the core's poll would deliver it.
    fn read(&self) -> Observation {
        Observation::from_reply(reads::read(&self.hgs, &ComputerId::local(), &Read::Swarm).unwrap())
            .unwrap()
    }

    fn run(&self, due: Due) -> Exit {
        let Due::Apply(invocation) = due else {
            panic!("no apply")
        };
        run_limited(&self.hgs, &invocation, &[], Read::Swarm.limit()).unwrap()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A session field's version token, as `swarm get` keys it.
fn token(session: &str) -> String {
    json!({"kind": "session", "machine": "m-1", "session": session}).to_string()
}

fn review() -> String {
    token(OLD)
}

/// `swarm get` with the review session in `project`, its heads, conflicts and what the call wrote.
fn snapshot(
    digest: &str,
    project: &str,
    heads: &[&str],
    conflicts: Value,
    written: &[&str],
) -> Value {
    let list = |id: &str| if id == project { json!([R]) } else { json!([]) };
    let mut versions = Map::new();
    versions.insert(review(), json!(heads));
    let mut wrote = Map::new();
    if !written.is_empty() {
        wrote.insert(review(), json!(written));
    }
    json!({"schema": 1, "node_id": "node-1", "digest": digest, "initialized": true, "machines": [], "peers": {}, "conflicts": conflicts,
           "organization": {"version": 2, "default_project": "ungrouped", "projects": [
               {"id": "alpha", "name": "Alpha", "color": "#3fb950", "accessible": true, "folders": [], "sessions": list("alpha")},
               {"id": "beta", "name": "Beta", "color": "#64b5f6", "accessible": true, "folders": [], "sessions": list("beta")},
               {"id": "gamma", "name": "Gamma", "color": "#d29922", "accessible": true, "folders": [], "sessions": list("gamma")},
               {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "accessible": true, "folders": [], "sessions": list("ungrouped")}]},
           "versions": versions, "written": wrote})
}

/// Each session field's heads, by session name.
fn heads(pairs: &[(&str, &[&str])]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(session, ops)| (token(session), json!(ops)))
        .collect()
}

/// `swarm get` with alpha listing `sessions`, the session fields' heads and what the call wrote.
fn renaming(
    digest: &str,
    sessions: &[&str],
    versions: &[(&str, &[&str])],
    written: &[(&str, &[&str])],
) -> Value {
    json!({"schema": 1, "node_id": "node-1", "digest": digest, "initialized": true, "machines": [], "peers": {}, "conflicts": [],
           "organization": {"version": 2, "default_project": "ungrouped", "projects": [
               {"id": "alpha", "name": "Alpha", "color": "#3fb950", "accessible": true, "folders": [], "sessions": sessions},
               {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "accessible": true, "folders": [], "sessions": []}]},
           "versions": heads(versions), "written": heads(written)})
}

fn reply(value: Value) -> Value {
    json!({"exit": 0, "stdout": value})
}

fn to(project: &str) -> Intent {
    Intent::Move {
        members: vec![R.into()],
        project: project.into(),
        before: None,
    }
}

fn renamed() -> Intent {
    Intent::Renamed {
        from: R.into(),
        to: R2.into(),
    }
}

fn place(sync: &Controller) -> String {
    members(sync.presented().unwrap().0)[R].clone()
}

/// What the hub does after each change: the draft file follows the pending edits.
fn persist(sync: &mut Controller, path: &Path) {
    if let Some(change) = sync.take_draft() {
        draft::store(path, &change).unwrap();
        sync.draft_written();
    }
}

/// A new instance of the client: it loads the draft the last one saved.
fn restarted(path: &Path) -> Controller {
    let mut sync = Controller::default();
    match draft::load(path) {
        draft::Load::Loaded(saved) => sync.restore(saved),
        other => panic!("the last run saved a draft: {other:?}"),
    }
    sync
}

// Review Focus 1 (coordinator ruling, 2026-10-28).
#[test]
fn a_second_writer_between_read_and_apply_gives_a_conflict_not_an_overwrite() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    // The second writer moved the session to gamma after the user's read.
    let other = snapshot("d2", "gamma", &["op-2"], json!([]), &[]);
    // hgs: the client's write has parents op-1, so op-2 and op-3 are concurrent heads.
    let conflict = json!([{"key": review(), "field": {"kind": "session"}, "selected": "op-3", "variants": []}]);
    let answer = snapshot("d3", "beta", &["op-2", "op-3"], conflict.clone(), &["op-3"]);
    let after = snapshot("d3", "beta", &["op-2", "op-3"], conflict, &[]);
    let next = snapshot("d4", "alpha", &["op-4"], json!([]), &["op-4"]);
    let fake = Fake::new(
        &json!({"get": [reply(seen.clone()), reply(other.clone()), reply(after.clone())],
                                 "apply": [reply(answer), reply(next)]}),
    );
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    sync.edit(&to("beta"), 0).unwrap();
    assert!(
        !sync.observe(fake.read(), 60),
        "the newer state is not merged under the edit"
    );
    assert_eq!(place(&sync), "beta");

    let exit = fake.run(sync.due(120));
    let settled = sync.finish(exit, 400);
    let sent = &fake.applies()[0];
    assert_eq!(
        sent["base"],
        shared(&seen["organization"]),
        "the base the user saw"
    );
    assert_eq!(
        sent["versions"], seen["versions"],
        "with the versions of that same read"
    );
    assert_eq!(members(&sent["desired"])[R], "beta");
    assert_eq!(settled, [(1, EditOutcome::Done { conflicts: 1 })]);
    assert_eq!(place(&sync), "beta", "where hgs placed it");
    assert_eq!(
        sync.view().status,
        "1 project conflict(s) need review in Swarm."
    );

    assert!(
        !sync.observe(Observation::from_reply(other).unwrap(), 500),
        "a late older state is not adopted"
    );
    assert!(
        sync.observe(fake.read(), 600),
        "the next read shows the write and is the base"
    );
    assert_eq!(sync.digest(), "d3");
    sync.edit(&to("alpha"), 1000).unwrap();
    let exit = fake.run(sync.due(1120));
    assert_eq!(
        sync.finish(exit, 1500),
        [(2, EditOutcome::Done { conflicts: 0 })]
    );
    let second = &fake.applies()[1];
    assert_eq!(second["base"], shared(&after["organization"]));
    assert_eq!(second["versions"], after["versions"]);
    assert_eq!(fake.calls(), ["get", "get", "apply", "get", "apply"]);
}

// Review Focus 2 (coordinator ruling, 2026-10-28): SwarmController.cpp:67-73, 96-110.
#[test]
fn a_timed_out_apply_stays_pending_and_the_next_tick_sends_it_once_more() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    // The first apply landed before the timeout (op-2); the topic shows it while the resend runs.
    let landed = snapshot("d2", "beta", &["op-2"], json!([]), &[]);
    // hgs writes the resend's diff again with the parents the user saw (src/swarm/model.rs:197-198):
    // a second head with the same value, which is no conflict (298-312).
    let again = snapshot("d3", "beta", &["op-2", "op-3"], json!([]), &["op-3"]);
    let fake = Fake::new(&json!({"get": [reply(seen.clone())],
                                 "apply": [{"exit": 0, "sleep": 5, "stdout": null}, reply(again)]}));
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    let ticket = sync.edit(&to("beta"), 0).unwrap();
    let Due::Apply(mut invocation) = sync.due(120) else {
        panic!("no apply")
    };
    invocation.timeout = Duration::from_millis(300);
    let exit = run_limited(&fake.hgs, &invocation, &[], Read::Swarm.limit()).unwrap();
    assert!(matches!(exit, Exit::TimedOut));
    let failed = 1000;
    let status = "Project sync unavailable: hgs did not answer in time";
    assert_eq!(
        sync.finish(exit, failed),
        [(
            ticket,
            EditOutcome::Kept {
                detail: status.into()
            }
        )]
    );
    assert!(sync.pending(), "the edit stays");
    assert_eq!(place(&sync), "beta", "the list keeps showing it");
    let view = sync.view();
    assert_eq!(
        (view.status.as_str(), view.status_error, view.ready),
        (status, true, true)
    );
    for now in (failed..failed + REFRESH_MS).step_by(250) {
        assert!(
            matches!(sync.due(now), Due::Nothing),
            "nothing before the tick, at {now}"
        );
    }
    let tick = failed + REFRESH_MS;
    let resend = sync.due(tick);
    assert!(
        matches!(sync.due(tick), Due::Nothing),
        "no duplicate while one is in flight"
    );
    assert!(
        !sync.observe(Observation::from_reply(landed).unwrap(), tick + 50),
        "not adopted under the edit"
    );
    assert!(
        matches!(sync.due(tick + 100), Due::Nothing),
        "and a state sends nothing while the resend runs"
    );
    let exit = fake.run(resend);
    assert!(
        sync.finish(exit, tick + 400).is_empty(),
        "the edit was answered by the first apply"
    );
    assert!(!sync.pending());
    assert_eq!((sync.digest(), sync.view().conflicts), ("d3", 0));
    assert_eq!(sync.view().status, LOCAL);
    let applies = fake.applies();
    assert_eq!(applies.len(), 2);
    assert_eq!(
        applies[1], applies[0],
        "the same base, desired and versions"
    );
    assert_eq!(applies[0]["base"], shared(&seen["organization"]));
    assert_eq!(applies[0]["versions"], seen["versions"]);
    assert_eq!(members(&applies[0]["desired"])[R], "beta");
    assert_eq!(fake.calls(), ["get", "apply", "apply"]);
}

// Qt never drops a failed draft, and hgs exits 1 for every swarm error
// (src/swarm/mod.rs:168-173), so a refusal is sent again on each tick.
#[test]
fn a_refused_apply_is_kept_and_sent_once_per_tick_until_it_lands() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    let landed = snapshot("d2", "beta", &["op-2"], json!([]), &["op-2"]);
    let refused = json!({"exit": 1, "stderr": "hgs: session belongs to more than one project\n"});
    let fake = Fake::new(
        &json!({"get": [reply(seen)], "apply": [refused.clone(), refused, reply(landed)]}),
    );
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    let ticket = sync.edit(&to("beta"), 0).unwrap();
    let mut now = 120;
    let exit = fake.run(sync.due(now));
    let status = "Project sync unavailable: hgs: session belongs to more than one project";
    assert_eq!(
        sync.finish(exit, now),
        [(
            ticket,
            EditOutcome::Kept {
                detail: status.into()
            }
        )]
    );
    for _ in 0..2 {
        assert!(
            matches!(sync.due(now + REFRESH_MS - 1), Due::Nothing),
            "one apply per tick"
        );
        now += REFRESH_MS;
        let exit = fake.run(sync.due(now));
        assert!(sync.finish(exit, now).is_empty());
    }
    assert!(!sync.pending(), "the third one landed");
    assert_eq!(place(&sync), "beta");
    let applies = fake.applies();
    assert_eq!(applies.len(), 3);
    assert!(
        applies.iter().all(|sent| *sent == applies[0]),
        "the same request each time"
    );
    assert_eq!(fake.calls(), ["get", "apply", "apply", "apply"]);
}

// Two writers migrate one rename: Qt and this client side by side both observe it
// (SessionsWindow.cpp:1942; SessionOrganization.cpp:223-230). Here Qt was first.
#[test]
fn a_rename_the_other_writer_already_migrated_sends_nothing() {
    let migrated = renaming("d2", &[R2], &[(OLD, &["op-q1"]), (NEW, &["op-q2"])], &[]);
    let fake = Fake::new(&json!({"get": [reply(migrated)], "apply": [reply(json!(null))]}));
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    let ticket = sync.edit(&renamed(), 0).unwrap();
    match sync.due(120) {
        Due::Settled(settled) => {
            assert_eq!(settled, [(ticket, EditOutcome::Done { conflicts: 0 })])
        }
        _ => panic!("the old name is no member any more: nothing to write"),
    }
    assert!(!sync.pending());
    let presented = sync.presented().unwrap().0;
    assert_eq!(
        sessions(&projects(presented)[0]).collect::<Vec<_>>(),
        [R2],
        "listed once, in alpha"
    );
    assert_eq!(sync.view().conflicts, 0);
    assert_eq!(fake.calls(), ["get"], "no apply");
}

// Two writers migrate one rename: Qt writes it between the client's read and its apply.
#[test]
fn a_rename_the_other_writer_migrates_between_read_and_apply_merges_cleanly() {
    let seen = renaming("d1", &[R], &[(OLD, &["op-1"])], &[]);
    let by_qt = renaming("d2", &[R2], &[(OLD, &["op-q1"]), (NEW, &["op-q2"])], &[]);
    // hgs writes the client's diff with the parents it read (src/swarm/model.rs:197-198): the old
    // name's removal beside Qt's, the new name's alpha beside Qt's. Equal values are no conflict
    // (298-312), and each session field holds one value, so membership cannot duplicate.
    let merged = renaming(
        "d3",
        &[R2],
        &[(OLD, &["op-q1", "op-c1"]), (NEW, &["op-q2", "op-c2"])],
        &[(OLD, &["op-c1"]), (NEW, &["op-c2"])],
    );
    let fake =
        Fake::new(&json!({"get": [reply(seen.clone()), reply(by_qt)], "apply": [reply(merged)]}));
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    let ticket = sync.edit(&renamed(), 0).unwrap();
    assert!(
        !sync.observe(fake.read(), 60),
        "Qt's migration is not merged under the client's"
    );
    let exit = fake.run(sync.due(120));
    assert_eq!(
        sync.finish(exit, 400),
        [(ticket, EditOutcome::Done { conflicts: 0 })]
    );
    let sent = &fake.applies()[0];
    assert_eq!(
        sent["base"],
        shared(&seen["organization"]),
        "the base the client read"
    );
    assert_eq!(sent["versions"], seen["versions"]);
    assert_eq!(
        members(&sent["desired"]),
        BTreeMap::from([(R2.to_owned(), "alpha".to_owned())])
    );
    let presented = sync.presented().unwrap().0;
    assert_eq!(
        sessions(&projects(presented)[0]).collect::<Vec<_>>(),
        [R2],
        "listed once, in alpha"
    );
    assert!(!members(presented).contains_key(R));
    let view = sync.view();
    assert_eq!(
        (view.conflicts, view.status.as_str(), view.status_error),
        (0, LOCAL, false)
    );
    assert_eq!(fake.calls(), ["get", "get", "apply"]);
}

// Review Focus 10 (coordinator ruling, 2026-10-28). Qt saves its draft
// (SessionsWindow.cpp:2496) and, after a restart, sends it once its first read
// answered (SwarmController.cpp:76-89, 191-206).
#[test]
fn a_pending_draft_survives_a_restart_and_is_sent_once_after_the_first_state() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    let landed = snapshot("d2", "beta", &["op-2"], json!([]), &["op-2"]);
    let fake = Fake::new(&json!({"get": [reply(seen.clone())], "apply": [reply(landed)]}));
    let path = fake.dir.join(draft::FILE);
    // The first run: the edit is pending, and its draft is on disk before any apply.
    let mut first = Controller::default();
    assert!(first.observe(fake.read(), 0));
    first.edit(&to("beta"), 0).unwrap();
    persist(&mut first, &path);
    assert!(first.pending() && path.exists());
    drop(first); // the client stops (before its flush; during or after a failed apply the draft is the same)

    let mut sync = restarted(&path);
    assert_eq!(place(&sync), "beta", "the new instance shows the draft");
    assert!(!sync.ready(), "edits wait for the first state");
    for now in (0..20_000).step_by(250) {
        assert!(
            matches!(sync.due(now), Due::Nothing),
            "nothing before the first state, at {now}"
        );
    }
    assert!(fake.applies().is_empty());
    // The first state: not adopted under the draft; the flush sends it (197, 203-205).
    assert!(!sync.observe(fake.read(), 20_000));
    assert!(matches!(sync.due(20_000 + 119), Due::Nothing));
    let exit = fake.run(sync.due(20_000 + 120));
    assert!(
        matches!(sync.due(20_300), Due::Nothing),
        "one apply at a time"
    );
    assert!(
        sync.finish(exit, 20_400).is_empty(),
        "nobody waits for a restored edit"
    );
    let applies = fake.applies();
    assert_eq!(applies.len(), 1, "exactly one apply");
    assert_eq!(
        applies[0]["base"],
        shared(&seen["organization"]),
        "the saved base"
    );
    assert_eq!(
        applies[0]["versions"], seen["versions"],
        "the saved versions"
    );
    assert_eq!(
        members(&applies[0]["desired"])[R],
        "beta",
        "the saved desired state"
    );
    assert!(!sync.pending());
    assert_eq!(place(&sync), "beta");
    persist(&mut sync, &path);
    assert!(!path.exists(), "a settled draft is removed");
    assert_eq!(fake.calls(), ["get", "get", "apply"]);
}

// While the client was stopped, another writer changed the catalog: the draft
// goes out with the saved base and versions, and hgs records both writes.
#[test]
fn a_restored_draft_against_a_newer_catalog_gives_a_conflict_not_an_overwrite() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    let newer = snapshot("d2", "gamma", &["op-2"], json!([]), &[]);
    // hgs: the draft's write has parents op-1, so op-2 and op-3 are concurrent heads.
    let conflict = json!([{"key": review(), "field": {"kind": "session"}, "selected": "op-3", "variants": []}]);
    let answer = snapshot("d3", "beta", &["op-2", "op-3"], conflict, &["op-3"]);
    let fake = Fake::new(
        &json!({"get": [reply(seen.clone()), reply(newer.clone())], "apply": [reply(answer)]}),
    );
    let path = fake.dir.join(draft::FILE);
    let mut first = Controller::default();
    assert!(first.observe(fake.read(), 0));
    first.edit(&to("beta"), 0).unwrap();
    persist(&mut first, &path);
    drop(first);

    let mut sync = restarted(&path);
    assert!(
        !sync.observe(fake.read(), 1000),
        "the newer catalog is not merged under the draft"
    );
    let exit = fake.run(sync.due(1120));
    let sent = &fake.applies()[0];
    assert_eq!(
        sent["base"],
        shared(&seen["organization"]),
        "the saved base, not the newer catalog"
    );
    assert_eq!(
        sent["versions"], seen["versions"],
        "the saved versions, so hgs keeps both writes"
    );
    assert_ne!(sent["versions"], newer["versions"]);
    assert!(sync.finish(exit, 1400).is_empty());
    let view = sync.view();
    assert_eq!(
        (view.status.as_str(), view.status_error, view.conflicts),
        ("1 project conflict(s) need review in Swarm.", true, 1)
    );
    assert_eq!(place(&sync), "beta", "where hgs placed it");
    assert_eq!(fake.calls(), ["get", "get", "apply"]);
}
