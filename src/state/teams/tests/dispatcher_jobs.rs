//! Task 278 (owner, 2026-11-08): ui1 and core waited for their own background
//! shells (Claude's "1 shell still running") and Zerus called them "uncertain:
//! hooks say working; the screen shows an idle prompt". The jobs a member's
//! hooks register (a Claude background shell, a Codex exec session, a
//! DeepSeek job), its Zerus builds and its budgeted Background work are what
//! it waits on: look, the stuck rule and task 269's check name them, the team
//! record shows them, and the working tier keeps each one's start and end.
use super::super::look::{self, Hooks, Screen, WaitJob};
use super::support::*;
use super::*;

fn listing(record: Value) -> Vec<Seen> {
    seen(&json!([{"sessions":[record]}]))
}

fn dev_record(extra: Value) -> Value {
    let mut record = json!({"name":"codex/x/dev","run_id":"r1","conversation_id":"c","state":"running","activity":"busy","phase":"working"});
    for (key, value) in extra.as_object().unwrap() {
        record[key] = value.clone();
    }
    record
}

#[test]
fn the_listing_names_the_jobs_a_session_waits_on() {
    let found = listing(dev_record(json!({
        "build":{"label":"cargo test","command":"cargo test --bin hgs","since":100.0,"request_id":"b1"},
        "background_jobs":[{"command":"npm run build","description":"Build the web client","started_at":200.0}],
        "process_work":{"count":1,"jobs":[{"command":"pytest -x","started_at":300.0}]},
    })));
    let jobs = &found[0].jobs;
    let kinds: Vec<(&str, f64)> = jobs.iter().map(|job| (job.kind.as_str(), job.since)).collect();
    assert_eq!(kinds, [("build", 100.0), ("shell", 200.0), ("job", 300.0)]);
    assert_eq!(jobs[0].what, "its own Zerus build (cargo test)");
    assert_eq!(jobs[1].what, "its background job `Build the web client`");
    assert_eq!(jobs[1].command, "npm run build");
    assert_eq!(jobs[2].what, "its job `pytest -x`");
    // A stopped session waits on nothing; jobs nothing waits for (services) are not jobs.
    let stopped = listing(dev_record(json!({"state":"stopped","background_jobs":[{"command":"npm run build","started_at":200.0}]})));
    assert!(stopped[0].jobs.is_empty());
    let services = listing(dev_record(json!({"process_work":{"count":0,"services":[{"name":"vite","command":"vite","since":1.0,"kind":"job"}]}})));
    assert!(services[0].jobs.is_empty());
}

fn waiting_hooks(jobs: Vec<WaitJob>) -> Hooks {
    Hooks { live: true, activity: "busy".into(), phase: "working".into(), jobs, quiet: 3_600.0, limit: 600.0, ..Default::default() }
}

#[test]
fn a_member_waiting_on_its_background_shell_is_never_uncertain() {
    let job = WaitJob { what: "its background job `cargo test`".into(), since: 1_794_000_000.0 };
    // The owner's case: hooks say working, the screen shows the idle prompt.
    let compared = look::compare(&waiting_hooks(vec![job.clone()]), None, Some((Screen::Prompt, "")));
    assert!(compared.disagree.is_empty(), "{:?}", compared.disagree);
    assert_ne!(compared.state, "uncertain");
    let waiting = compared.waiting.clone().unwrap();
    assert!(waiting.starts_with("its background job `cargo test` since "), "{waiting}");
    assert_eq!(waiting, format!("its background job `cargo test` since {}", look::clock(job.since)));
    assert_eq!(compared.sources["waiting"], json!(waiting));
    // Without the job the same views disagree, as before.
    let bare = look::compare(&waiting_hooks(vec![]), None, Some((Screen::Prompt, "")));
    assert!(!bare.disagree.is_empty());
    // An asking panel is still a disagreement: the job does not answer it.
    let asking = look::compare(&waiting_hooks(vec![job.clone()]), None, Some((Screen::Asking, "Do you want to proceed?")));
    assert!(!asking.disagree.is_empty());
    // Several jobs: the first and how many more.
    let two = look::job_words(&[job.clone(), WaitJob { what: "its own Zerus build".into(), since: 0.0 }]).unwrap();
    assert!(two.ends_with("(and 1 more)"), "{two}");
}

#[test]
fn a_member_waiting_on_its_job_is_not_suspect_and_not_stuck() {
    let at = 10_000.0;
    let mut t = team();
    delivered(&mut t);
    let dev = t.member("dev").unwrap().clone();
    let job = WaitJob { what: "its background job `cargo test`".into(), since: at - 3_000.0 };
    assert_eq!(look::due(&t, &dev, &waiting_hooks(vec![job.clone()]), at), None, "a quiet member with a job is not suspect");
    assert!(look::due(&t, &dev, &waiting_hooks(vec![]), at).is_some(), "without it, it is");
    // Idle with an unreported hand-out but waiting on its job: not suspect.
    let idle = Hooks { activity: "idle".into(), phase: "idle".into(), ..waiting_hooks(vec![job]) };
    assert_eq!(look::due(&t, &dev, &idle, at), None);

    // The stuck rule: busy and quiet past the bound, but waiting on its job.
    let t = team();
    let lead = session("claude/x/lead", "r0", true);
    let mut quiet = session("codex/x/dev", "r1", true);
    quiet.activity = "busy".into();
    quiet.phase = "working".into();
    quiet.last_event_at = Some(at - 11.0 * 60.0);
    let stuck = |seen: &Seen| steps(&t, &[lead.clone(), seen.clone()], at).iter().any(|step| matches!(step, Step::MarkStuck { .. }));
    assert!(stuck(&quiet), "quiet without a job is stuck");
    let mut waiting = quiet.clone();
    waiting.jobs = vec![SeenJob { kind: "shell".into(), what: "its background job `cargo test`".into(), command: "cargo test".into(), since: at - 600.0 }];
    assert!(!stuck(&waiting), "waiting on its own job is not stuck");
    // The member's jobs are recorded when they change.
    let found = steps(&t, &[lead.clone(), waiting.clone()], at);
    assert!(found.iter().any(|step| matches!(step, Step::Jobs { member, jobs } if member == "dev" && jobs.len() == 1)), "{found:?}");
}

#[test]
fn jobs_are_recorded_with_their_start_and_end_for_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let t = team();
    stored(&mut store, &t);
    let job = SeenJob { kind: "shell".into(), what: "its background job `cargo test`".into(), command: "cargo test".into(), since: 1_000.0 };
    record_jobs(&mut store, &t, "dev", std::slice::from_ref(&job), 1_100.0).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert_eq!(saved.member("dev").unwrap().jobs, vec![MemberJob { kind: "shell".into(), what: job.what.clone(), command: "cargo test".into(), since: 1_000.0 }]);
    let rows = super::super::working::jobs(store.connection(), &t.id, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0]["member"].as_str(), rows[0]["command"].as_str(), rows[0]["started_at"].as_f64()), (Some("dev"), Some("cargo test"), Some(1_000.0)));
    assert!(rows[0]["ended_at"].is_null(), "it still runs");
    // Status shows what the member waits on.
    let status = crate::state::teams::commands::view_with(&saved, &[], None);
    let member = status["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap().clone();
    assert!(member["waiting_for"].as_str().unwrap().starts_with("its background job `cargo test`"), "{member}");
    // It ended: the row keeps its end; the member waits on nothing.
    record_jobs(&mut store, &saved, "dev", &[], 1_700.0).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert!(saved.member("dev").unwrap().jobs.is_empty());
    let rows = super::super::working::jobs(store.connection(), &t.id, 10).unwrap();
    assert_eq!(rows[0]["ended_at"].as_f64(), Some(1_700.0));
    assert_eq!(event_kinds(&store, &t.id).iter().filter(|kind| kind.starts_with("job_")).collect::<Vec<_>>(), ["job_started", "job_ended"]);
    // The lead's brief names it.
    let recent = store.events(&t.id, None, 50).unwrap()["items"].as_array().unwrap().clone();
    let lines = super::super::handoff::pulse_lines(&saved, &saved.lead, &recent, crate::state::now());
    assert!(lines.iter().any(|line| line.contains("started waiting for its background job `cargo test`")), "{lines:?}");
    assert!(lines.iter().any(|line| line.contains("its background job `cargo test` ended")), "{lines:?}");
}
