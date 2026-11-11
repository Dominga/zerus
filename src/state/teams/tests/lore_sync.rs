use super::*;
use crate::state::teams::model::{Attempt, CommitRef, Dependency, DependencyKind, Finding, Hold, Phase, Report, RunRecord, TeamMemory};

const TEAM: &str = "33fbafeb-8291-4eda-a00c-6e1448ec6f20";

fn commit(sha: &str) -> CommitRef {
    CommitRef { sha: sha.into(), ..Default::default() }
}

fn passing(template: &str, at: f64, ms: u64) -> RunRecord {
    RunRecord { template: template.into(), args: json!({"filters":["lore_sync"]}), exit: Some(0), at, ms, ..Default::default() }
}

fn report(text: &str) -> Option<Report> {
    Some(Report { text: text.into(), at: 15.0, ..Default::default() })
}

/// A running team on its initial plan (phase 1 has no `Phase` record: it is
/// the approved plan itself) with one draft phase, its tasks in every state a
/// record maps, the owner's decisions and three findings.
fn team(workspace: &Path) -> Team {
    let mut team = Team { id: TEAM.into(), name: "Core".into(), goal: "Teams keep their memory".into(), status: TeamStatus::Running, ..Default::default() };
    team.settings.memory = Some(TeamMemory { mode: "team".into(), source: "team".into(), store: workspace.display().to_string(), workspace: workspace.display().to_string() });
    team.phases = vec![Phase { n: 2, title: "Later".into(), state: PhaseState::Draft, ..Default::default() }];
    let accepted = Task {
        id: 1,
        phase: 1,
        title: "Write the records".into(),
        description: "Plans, outcomes, decisions".into(),
        criteria: vec!["records pass validation".into()],
        status: TaskStatus::Accepted,
        decision_reason: "complete and tested".into(),
        attempts: vec![
            Attempt { n: 1, state: AttemptState::Rejected, rework_reason: "the digest drops the owner's decisions".into(), ..Default::default() },
            Attempt {
                n: 2,
                state: AttemptState::Reported,
                commits: vec![commit("abc1234"), commit("def5678")],
                runs: vec![passing("cargo-test", 10.0, 4_200), RunRecord { template: "cargo-test".into(), exit: Some(101), at: 20.0, ..Default::default() }],
                report: report("Kept the decisions in the digest\nand tested it"),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let cancelled = Task { id: 2, phase: 1, title: "Old idea".into(), status: TaskStatus::Cancelled, decided_by: Some(Decider::Human), decision_reason: "out of scope".into(), ..Default::default() };
    let held = Task { id: 3, phase: 1, title: "Relay".into(), hold: Some(Hold { by: "lead".into(), at: 1.0, reason: "waits for the relay".into() }), ..Default::default() };
    let open = Task { id: 4, phase: 1, title: "Export".into(), deps: vec![Dependency { task: 1, kind: DependencyKind::Soft }, Dependency { task: 9, kind: DependencyKind::Hard }], ..Default::default() };
    let draft = Task { id: 5, phase: 2, title: "Not approved".into(), ..Default::default() };
    let no_commit = Task { id: 6, phase: 1, title: "Docs".into(), status: TaskStatus::Accepted, attempts: vec![Attempt { n: 1, state: AttemptState::Reported, ..Default::default() }], ..Default::default() };
    team.tasks = vec![accepted, cancelled, held, open, draft, no_commit];
    team.proposals = vec![
        json!({"id":"p1","kind":"add-member","state":"accepted","change":{"summary":"add a QA member"},"note":"the tests lag"}),
        json!({"id":"p2","kind":"add-member","state":"open","change":{"summary":"pending"}}),
        json!({"id":"p3","kind":"switch-memory-mode","state":"rejected","reason":"try the team mode"}),
    ];
    team.findings = vec![
        Finding { id: "f1".into(), from_task: 1, task: Some(1), state: "fixed".into(), title: "The digest lost the decisions".into(), at: 5.0,
            scenario: "a lead reads the inbox after an allowed card and sees no decision".into(), file: r"src\state\teams\inbox.rs".into(), ..Default::default() },
        Finding { id: "f2".into(), from_task: 6, state: "fixed".into(), title: "Docs skipped the export".into(), scenario: "the guide names no export".into(),
            note: "named the export".into(), ..Default::default() },
        Finding { id: "f3".into(), from_task: 1, state: "open".into(), title: "Still open".into(), scenario: "s".into(), ..Default::default() },
    ];
    team
}

fn by_id(records: &[Value]) -> BTreeMap<String, Value> {
    records.iter().map(|record| (record["id"].as_str().unwrap().to_owned(), record.clone())).collect()
}

/// Task 128: the plan, each task's outcome, the owner's decisions, the faults
/// of fixed findings and reworks with their exact strings, and the readings.
#[test]
fn the_team_maps_to_records_that_carry_every_required_field() {
    let dir = tempfile::tempdir().unwrap();
    let team = team(dir.path());
    let (records, pending) = desired(&team);
    let found = by_id(&records);

    let plan = &found["t33fbafeb-phase-1"];
    assert_eq!(plan["kind"], "plan");
    assert_eq!(plan["status"], "live", "a running team's plan is live");
    assert_eq!(plan["title"], "Core: phase 1", "the initial plan has no Phase record and is written all the same");
    assert_eq!(plan["goal"], "Teams keep their memory");
    assert!(!found.keys().any(|id| id.starts_with("t33fbafeb-phase-2")), "a draft phase is no plan yet");

    let done = &found["t33fbafeb-phase-1#1"];
    assert_eq!((done["status"].as_str(), done["plan"].as_str()), (Some("done"), Some("t33fbafeb-phase-1")));
    assert_eq!(done["commits"], json!(["abc1234", "def5678"]));
    assert_eq!(done["outcome"], "complete and tested");
    assert_eq!(done["criteria"], json!(["records pass validation"]));
    let dropped = &found["t33fbafeb-phase-1#2"];
    assert_eq!((dropped["status"].as_str(), dropped["reason"].as_str(), dropped["decided_by"].as_str()), (Some("dropped"), Some("out of scope"), Some("You")));
    let deferred = &found["t33fbafeb-phase-1#3"];
    assert_eq!((deferred["status"].as_str(), deferred["reason"].as_str()), (Some("deferred"), Some("waits for the relay")));
    let open = &found["t33fbafeb-phase-1#4"];
    assert_eq!(open["status"], "open");
    assert_eq!(open["deps"], json!([{"item":"t33fbafeb-phase-1#1","kind":"soft"}]), "a dependency outside the plan is left out");
    // An accepted task with no commit is not `done` half-filled: it waits.
    assert_eq!(found["t33fbafeb-phase-1#6"]["status"], "open");
    assert!(pending.iter().any(|item| item.id == "t33fbafeb-phase-1#6" && item.missing == ["commits"]), "{pending:?}");

    let allowed = &found["t33fbafeb-p1"];
    assert_eq!(allowed["kind"], "decision");
    assert_eq!(allowed["area"], "team-33fbafeb");
    assert_eq!(allowed["question"], "add-member: add a QA member");
    assert_eq!(allowed["choice"], "allowed: add a QA member");
    assert_eq!(allowed["reason"], "the tests lag");
    assert_eq!(allowed["decided_by"], "owner");
    let denied = &found["t33fbafeb-p3"];
    assert_eq!((denied["choice"].as_str(), denied["reason"].as_str()), (Some("denied"), Some("no reason given")));
    assert!(!found.contains_key("t33fbafeb-p2"), "an open card is no decision");

    // The finding's fault keeps the exact strings; its fix is the fixing report.
    let fault = &found["t33fbafeb-finding-f1"];
    assert_eq!(fault["symptom"], "a lead reads the inbox after an allowed card and sees no decision");
    assert_eq!(fault["cause"], "The digest lost the decisions");
    assert_eq!(fault["fix"], "Kept the decisions in the digest\nand tested it");
    assert_eq!(fault["check"], "cargo-test {\"filters\":[\"lore_sync\"]}");
    assert_eq!(fault["refs"]["files"], json!(["src/state/teams/inbox.rs"]));
    // A fault without its check is never written: the lead is asked.
    assert!(!found.contains_key("t33fbafeb-finding-f2"));
    let waiting = pending.iter().find(|item| item.id == "t33fbafeb-finding-f2").expect("f2 waits");
    assert_eq!(waiting.missing, ["check"]);
    assert!(!found.contains_key("t33fbafeb-finding-f3"), "an open finding is no fault");
    let mut late = team.clone();
    late.findings[0].at = 30.0;
    let (records, pending) = desired(&late);
    assert!(!by_id(&records).contains_key("t33fbafeb-finding-f1"), "the run and the report before the finding do not fix it");
    assert!(pending.iter().any(|item| item.id == "t33fbafeb-finding-f1" && item.missing == ["fix", "check"]), "{pending:?}");
    // The rework's cause is the reviewer's exact reason.
    let rework = &found["t33fbafeb-rework-1-1"];
    assert_eq!(rework["cause"], "the digest drops the owner's decisions");
    assert_eq!(rework["status"], "cured");

    let reading = &found["t33fbafeb-reading-cargo-test"];
    assert_eq!((reading["value"].as_str(), reading["measured_at"].as_str()), (Some("4200 ms"), Some("def5678")), "the passing run, not the failing later one");

    // Every id is one lore accepts, and every record is this team's.
    for record in &records {
        let id = record["id"].as_str().unwrap();
        assert!(id.starts_with("t33fbafeb-"), "{id}");
        assert!(zerus_lore_id(id), "{id} is not a lore id");
        assert!(record["title"].as_str().is_some_and(|title| !title.is_empty() && title.chars().count() <= 120), "{record}");
    }
}

/// `ID` or `ITEM_ID` (docs/lore.md 2.2), as the lore crate checks them.
fn zerus_lore_id(id: &str) -> bool {
    let plain = |text: &str| {
        let mut chars = text.chars();
        chars.next().is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
            && (2..=48).contains(&text.len())
            && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    };
    match id.split_once('#') {
        Some((plan, number)) => plain(plan) && !number.starts_with('0') && number.len() <= 6 && number.chars().all(|ch| ch.is_ascii_digit()),
        None => plain(id),
    }
}

/// A finished team closes its plan once every item is decided.
#[test]
fn a_finished_team_closes_a_decided_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut team = team(dir.path());
    team.status = TeamStatus::Done;
    assert_eq!(by_id(&desired(&team).0)["t33fbafeb-phase-1"]["status"], "live", "open and held items keep it live");
    team.tasks.retain(|task| matches!(task.id, 1 | 2));
    let plan = by_id(&desired(&team).0)["t33fbafeb-phase-1"].clone();
    assert_eq!(plan["status"], "closed");
    assert_eq!(plan["outcome"], "1 of 2 tasks done; the team finished");
}

/// The diff adds what is missing, patches what changed, leaves decided items
/// and other teams' records alone, and turns status moves into lore commands.
#[test]
fn the_diff_writes_only_this_teams_changes() {
    let dir = tempfile::tempdir().unwrap();
    let team = team(dir.path());
    let (records, _) = desired(&team);
    let mut current = by_id(&records);
    // Stamps lore adds never count as a change.
    for record in current.values_mut() {
        record["v"] = json!(3);
        record["by"] = json!(BY);
        record["created"] = json!("2026-11-07");
    }
    let (adds, updates, commands) = diff(&team, &records, &current);
    assert!(adds.is_empty() && updates.is_empty() && commands.is_empty(), "{adds:?} {updates:?} {commands:?}");

    // A missing record is added; a changed open item is patched.
    current.remove("t33fbafeb-p1");
    current.get_mut("t33fbafeb-phase-1#4").unwrap()["title"] = json!("Old title");
    current.get_mut("t33fbafeb-phase-1#4").unwrap()["reason"] = json!("stale");
    // A done item stays as decided, even when the team's text changes.
    current.get_mut("t33fbafeb-phase-1#1").unwrap()["outcome"] = json!("older outcome");
    // Another team's record is never touched.
    current.insert("t00000000-p1".into(), json!({"id":"t00000000-p1","kind":"decision"}));
    let (adds, updates, commands) = diff(&team, &records, &current);
    assert_eq!(adds.iter().map(|record| record["id"].as_str().unwrap()).collect::<Vec<_>>(), ["t33fbafeb-p1"]);
    assert_eq!(updates, vec![json!({"id":"t33fbafeb-phase-1#4","title":"Export","reason":null})]);
    assert!(commands.is_empty());

    // A reopened task reopens its done item; a finished team closes its plan.
    let mut reopened = team.clone();
    reopened.tasks[0].status = TaskStatus::Pending;
    let (records, _) = desired(&reopened);
    let (_, _, commands) = diff(&reopened, &records, &by_id(&desired(&team).0));
    assert_eq!(commands, vec![vec!["item", "reopen", "t33fbafeb-phase-1#1", "--reason", "the task was reopened in its team"]]);
    let mut finished = team.clone();
    finished.status = TeamStatus::Done;
    finished.tasks.retain(|task| matches!(task.id, 1 | 2));
    let (records, _) = desired(&finished);
    let (_, updates, commands) = diff(&finished, &records, &by_id(&desired(&team).0));
    assert!(updates.iter().all(|patch| patch["id"] != "t33fbafeb-phase-1"), "{updates:?}");
    assert_eq!(commands, vec![vec!["plan", "close", "t33fbafeb-phase-1", "--outcome", "1 of 2 tasks done; the team finished"]]);
}

/// The lead's digest names the open items and the newest decisions, at most
/// 10 lines; a team without a workspace has none.
#[test]
fn the_lead_digest_is_bounded_and_names_the_commands() {
    let dir = tempfile::tempdir().unwrap();
    let mut team = team(dir.path());
    let plans = dir.path().join("lore").join("plans");
    std::fs::create_dir_all(&plans).unwrap();
    let (records, pending) = desired(&team);
    let (plan_records, knowledge): (Vec<&Value>, Vec<&Value>) = records.iter().partition(|record| matches!(record["kind"].as_str(), Some("plan" | "item")));
    let lines = |list: &[&Value]| list.iter().map(|record| record.to_string()).collect::<Vec<_>>().join("\n");
    std::fs::write(plans.join("t33fbafeb-phase-1.jsonl"), lines(&plan_records)).unwrap();
    std::fs::write(dir.path().join("lore").join("team-33fbafeb.jsonl"), lines(&knowledge)).unwrap();
    team.lore.pending = pending;
    let text = digest(&team);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() <= 10, "{text}");
    assert!(lines[0].contains("3 open item(s)") && lines[0].contains("hgs team lore plans") && lines[0].contains("hgs team lore find"), "{text}");
    assert!(text.contains("decision t33fbafeb-p1: allowed: add a QA member (the tests lag)"), "{text}");
    assert!(text.contains("waits for you: write the fault for finding f2"), "{text}");
    // The read of the workspace is bounded and keeps the records by id.
    assert_eq!(current(dir.path()).len(), records.len());

    team.settings.memory = Some(TeamMemory { mode: "plain".into(), ..Default::default() });
    assert_eq!(digest(&team), "", "plain memory has no workspace digest");
}

/// Without lore the team keeps plain memory and says so once; a lore that
/// does not start remembers nothing and is tried again.
#[test]
fn a_missing_or_broken_lore_leaves_the_team_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(workspace.join("lore").join("plans")).unwrap();
        std::fs::write(workspace.join("lore.toml"), "version = 1\ndir = \"lore\"\nareas = []\n").unwrap();
        let mut store = Store::open(true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team(&workspace), vec![]).unwrap();

        let first = sync(&mut store, TEAM, None).unwrap();
        assert!(first.skipped && first.written.is_empty());
        let after = store.load(TEAM).unwrap().unwrap();
        assert!(after.lore.unavailable_told);
        assert!(after.lore.hash.is_empty(), "nothing was written: the next pass tries");
        let told = store.events(TEAM, None, 50).unwrap().to_string();
        assert_eq!(told.matches("lore_unavailable").count(), 1, "{told}");
        sync(&mut store, TEAM, None).unwrap();
        let told = store.events(TEAM, None, 50).unwrap().to_string();
        assert_eq!(told.matches("lore_unavailable").count(), 1, "said once: {told}");

        let broken = dir.path().join("no-such-lore.exe");
        let pass = sync(&mut store, TEAM, Some(&broken)).unwrap();
        assert!(pass.written.is_empty());
        assert!(pass.refused.iter().any(|(id, error)| id == "lore" && error.starts_with("lore did not start")), "{pass:?}");
        let after = store.load(TEAM).unwrap().unwrap();
        assert!(after.lore.refused.is_empty(), "a lore that did not run refuses no record");
        assert!(after.lore.hash.is_empty(), "and settles nothing");
        assert!(!after.lore.pending.is_empty(), "the waiting records reach the lead's inbox");
        // The team's area went into the workspace's lore.toml for its records.
        let config = std::fs::read_to_string(workspace.join("lore.toml")).unwrap();
        assert!(config.contains("areas = [\"team-33fbafeb\"]"), "{config}");
    });
}

/// The waiting records are lead inbox items with the workspace's command.
#[test]
fn a_waiting_record_is_a_lead_inbox_item() {
    let dir = tempfile::tempdir().unwrap();
    let mut team = team(dir.path());
    team.lead = "lead".into();
    team.lore.pending = desired(&team).1;
    let items = super::super::inbox::items(&team);
    let waiting: Vec<_> = items.iter().filter(|item| item.kind == "lore_missing").collect();
    assert_eq!(waiting.len(), team.lore.pending.len());
    assert!(waiting.iter().any(|item| item.summary.contains("finding f2") && item.command.contains("add fault")), "{waiting:?}");
    team.settings.memory = Some(TeamMemory { mode: "plain".into(), ..Default::default() });
    assert!(!super::super::inbox::items(&team).iter().any(|item| item.kind == "lore_missing"), "plain memory asks for nothing");
}

/// Plain memory (docs/lore.md 5): one digest of at most 6 KiB with each
/// task's outcome and why, the owner's decisions, the findings and the last
/// test result per template.
#[test]
fn the_plain_digest_keeps_what_the_spec_defines_within_six_kib() {
    let dir = tempfile::tempdir().unwrap();
    let mut team = team(dir.path());
    team.settings.memory = Some(TeamMemory { mode: "plain".into(), ..Default::default() });
    let text = plain_digest(&team);
    for expected in [
        "Team Core (33fbafeb-8291-4eda-a00c-6e1448ec6f20): Teams keep their memory",
        "Phase 1\n- #1 Write the records",
        "- #1 Write the records: accepted by Lead: complete and tested",
        "- #2 Old idea: cancelled by You: out of scope",
        "- #3 Relay: held: waits for the relay",
        "- allowed add-member: add a QA member (the tests lag)",
        "- denied switch-memory-mode: try the team mode (no reason given)",
        "- f3  [open]: Still open",
        "- f1  [fixed]: The digest lost the decisions",
        "- cargo-test on #1: failed (exit 101), 0 ms",
    ] {
        assert!(text.contains(expected), "{expected:?} missing from:\n{text}");
    }
    // Many long phases: the oldest go, the decisions and results stay.
    for n in 3..60 {
        team.phases.push(Phase { n, title: format!("Phase title {n}"), state: PhaseState::Approved, ..Default::default() });
        team.tasks.push(Task { id: 100 + n, phase: n, title: "x".repeat(100), ..Default::default() });
    }
    let text = plain_digest(&team);
    assert!(text.len() <= PLAIN_DIGEST_BYTES, "{}", text.len());
    assert!(text.contains("(earlier phases left out)"), "{text}");
    assert!(text.contains("Phase 59 Phase title 59") && text.contains("Last test result per template"), "{text}");
    assert!(!text.contains("Phase 3 Phase title 3\n"), "the oldest phases went first");
}

/// Task 130: a team whose plan grew phases writes each approved one; a
/// finally rejected task is a dropped item.
#[test]
fn every_approved_phase_and_a_rejected_task_are_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut team = team(dir.path());
    team.phases.push(Phase { n: 3, title: "Export".into(), state: PhaseState::Approved, ..Default::default() });
    team.tasks.push(Task { id: 7, phase: 3, title: "Export it".into(), status: TaskStatus::Rejected, decision_reason: "wrong store".into(), ..Default::default() });
    let found = by_id(&desired(&team).0);
    assert!(found.contains_key("t33fbafeb-phase-1") && found.contains_key("t33fbafeb-phase-3"));
    assert!(!found.contains_key("t33fbafeb-phase-2"), "a draft phase is no plan");
    assert_eq!(found["t33fbafeb-phase-3"]["title"], "Core: Export");
    let rejected = &found["t33fbafeb-phase-3#7"];
    assert_eq!((rejected["status"].as_str(), rejected["reason"].as_str(), rejected["decided_by"].as_str()), (Some("dropped"), Some("wrong store"), Some("Lead")));
    assert_eq!(plan_phases(&team, false).iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1, 3]);
    assert_eq!(plan_phases(&team, true).iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1, 2, 3]);
    // A team still planning has no plan in its memory.
    team.status = TeamStatus::Planning;
    assert!(plan_phases(&team, false).is_empty());
}

/// Task 130: a held writer lock is not a refusal of the record.
#[test]
fn a_held_lock_is_temporary() {
    assert!(is_transient("another writer holds the store (agent:zerus); nothing was written"));
    assert!(is_transient("lore/x.jsonl: another process holds the file, so it could not be replaced; nothing was committed - try again when it is free"));
    assert!(!is_transient("refused, nothing was written: fault x: `cause` is required"));
}

/// Task #397: a search at the project root finds the team sub-stores and each
/// hit names the store it came from; `show` and `plans` stay single-store.
#[test]
fn a_find_over_every_store_names_each_one() {
    let team = Team { id: TEAM.into(), name: "Core".into(), ..Default::default() };
    // The find asks lore for every sub-store; the other reads do not.
    assert_eq!(read_args("find", &["cache".into()]), ["find", "cache", "--stores", "all"]);
    assert_eq!(read_args("show", &["t33fbafeb-p1".into()]), ["show", "t33fbafeb-p1"]);
    assert_eq!(read_args("plans", &[]), ["plans"]);
    // Hits from the project root: its own records, this team's sub-store, another team's.
    let mut value = json!({"results":[{"store":".","record":{"id":"p1"}},{"store":"teams/33fbafeb","record":{"id":"t1"}},
        {"store":"teams/bbbbbbbb","record":{"id":"t2"}}],"notes":[]});
    label_hits(&mut value, "project", &team);
    let stores: Vec<&str> = value["results"].as_array().unwrap().iter().map(|hit| hit["store"].as_str().unwrap_or_default()).collect();
    assert_eq!(stores, ["project", "team Core", "team bbbbbbbb"], "{value}");
    // The workspace's own root is this team; a hit without a store is its own.
    assert_eq!(store_label("workspace", ".", &team), "team Core");
    assert_eq!(store_label("project", "./", &team), "project");
}

/// Task #19 (team ede724c4, owner 2026-11-10): a member that commits with git
/// itself leaves no commit Zerus made; a landed task gives lore its landing
/// commit (and the accepted report's), with no "name its commit" notice.
#[test]
fn a_landed_task_gives_lore_its_commit() {
    let dir = tempfile::tempdir().unwrap();
    let mut team = team(dir.path());
    let no_commit = team.tasks.iter_mut().find(|task| task.id == 6).unwrap();
    no_commit.landed = Some(crate::state::teams::model::Landed { commit: "feed123".into(), by: "lead".into(), at: 30.0 });
    let (records, pending) = desired(&team);
    let found = by_id(&records);
    assert_eq!(found["t33fbafeb-phase-1#6"]["status"], "done");
    assert_eq!(found["t33fbafeb-phase-1#6"]["commits"], json!(["feed123"]));
    assert!(!pending.iter().any(|item| item.id == "t33fbafeb-phase-1#6"), "{pending:?}");
    // The accepted report's commit counts as well.
    let task = team.tasks.iter_mut().find(|task| task.id == 6).unwrap();
    task.landed = None;
    task.attempts[0].folder_head = "beef456".into();
    let (records, pending) = desired(&team);
    assert_eq!(by_id(&records)["t33fbafeb-phase-1#6"]["commits"], json!(["beef456"]));
    assert!(!pending.iter().any(|item| item.id == "t33fbafeb-phase-1#6"), "{pending:?}");
}

/// RogueDOD (2026-11-11): a project that keeps its records in its repository
/// gets the team's export there, never under Zerus data, and Zerus never edits
/// the repository's lore.toml: the area must be one it names.
#[test]
fn an_export_into_a_repository_store_needs_an_area_the_repository_names() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("game");
    std::fs::create_dir_all(repo.join("Sub")).unwrap();
    let git = |args: &[&str]| assert!(Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap().status.success(), "{args:?}");
    git(&["init", "-q", "-b", "develop"]);
    let config = "version = 1\nareas = [\"game\", \"docs\"]\n\n[commit]\nmode = \"shared-tree\"\nbranch = \"develop\"\n";
    std::fs::write(repo.join("lore.toml"), config).unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(workspace.join("lore")).unwrap();
    std::fs::write(workspace.join("lore").join("faults.jsonl"), "{\"id\":\"t33fbafeb-finding-f1\",\"kind\":\"fault\",\"area\":\"team-33fbafeb\"}\n").unwrap();
    let mut team = team(&workspace);
    team.folder = repo.join("Sub").display().to_string();
    let found = repo_of(&team).expect("the team folder's git root keeps the records");
    assert!(found.join("lore.toml").is_file());
    let refused = export(&team, None).unwrap_err().to_string();
    assert!(refused.contains("has no area team-33fbafeb") && refused.contains("--area AREA") && refused.contains("game, docs"), "{refused}");
    assert_eq!(std::fs::read_to_string(repo.join("lore.toml")).unwrap(), config, "the repository's lore.toml is never edited");
    // A repository hit is marked as the repository's.
    let mut value = json!({"results":[{"store":"."},{"store":"teams/x"}]});
    label_hits(&mut value, "repository", &team);
    assert_eq!(value["results"][0]["store"], "repository");
    assert_eq!(value["results"][1]["store"], "repository teams/x");
}
