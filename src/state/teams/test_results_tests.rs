use super::*;
use crate::state::teams::model::{Team, TeamStatus};
use crate::state::teams::test_queue::{run_due, Request, TestStore};
use crate::test_discipline::config::parse_toml;
use crate::test_discipline::map::Target;

fn store(dir: &Path) -> TestStore {
    TestStore::open_at(&dir.join("tests.sqlite3"), false).unwrap()
}

fn request(id: &str, suite: &str, command: &str) -> Request {
    Request {
        id: id.into(),
        team: "t".into(),
        suite: suite.into(),
        task: None,
        targets: Vec::new(),
        commit: "c1".into(),
        input_hash: "h".into(),
        asked_by: "dev".into(),
        forced: false,
        at: 1.0,
        seconds: 0.0,
        budget_minutes: 10.0,
        resources: String::new(),
        command: command.into(),
        platforms: Vec::new(),
    }
}

fn policy() -> Policy {
    Policy { retries: 1, grow_percent: 50, episodes: 3, quarantine_days: 14, full_after: 0, max_age_days: 0.0 }
}

fn team() -> Team {
    Team { id: "t".into(), status: TeamStatus::Running, ..Default::default() }
}

/// Fails on its first run in `dir`, passes after: one flaky episode.
fn flaky_command(marker: &str) -> String {
    if cfg!(windows) {
        format!("if exist {marker} (exit /b 0) else (echo first>{marker} & echo boom & exit /b 1)")
    } else {
        format!("if [ -f {marker} ]; then exit 0; else touch {marker}; echo boom; exit 1; fi")
    }
}

#[test]
fn a_failure_that_passes_on_its_retry_is_flaky_with_its_history_and_quarantine() {
    let dir = tempfile::tempdir().unwrap();
    let database = store(dir.path());
    let mut team = team();
    let mut all = Vec::new();
    for episode in 0..4 {
        let marker = format!("m{episode}");
        let ran = run_due(&database, dir.path(), &[request(&format!("r{episode}"), "gui", &flaky_command(&marker))], 100.0 + f64::from(episode), &policy()).unwrap();
        assert_eq!(ran[0].status, "flaky", "{:?}", ran[0]);
        assert_eq!(ran[0].flags, vec!["retried"]);
        // The episode keeps the failure the retry hid.
        assert!(ran[0].excerpt.contains("boom"), "{}", ran[0].excerpt);
        apply(&mut team, &ran, &database, &policy(), 100.0 + f64::from(episode), (0.0, 0)).unwrap();
        all.extend(ran);
        // Quarantined from the first episode, visible until it expires.
        assert_eq!(team.test_quarantine.len(), 1);
        assert_eq!(team.test_quarantine[0].episodes, episode as usize + 1);
        assert_eq!(team.test_quarantine[0].until, 100.0 + f64::from(episode) + 14.0 * 86_400.0);
        // The third episode is one lead item; the fourth adds none.
        let flaky: Vec<&TestAlert> = team.test_alerts.iter().filter(|alert| alert.kind == "flaky").collect();
        assert_eq!(flaky.len(), usize::from(episode >= 2), "episode {episode}: {:?}", team.test_alerts);
        if episode >= 2 {
            assert_eq!(flaky[0].episodes, 3);
            assert_eq!(flaky[0].at, 102.0, "the fourth episode does not raise it again");
            assert!(flaky[0].detail.contains("gui passed only on its automatic retry 3 times") && flaky[0].detail.contains("File a task"), "{}", flaky[0].detail);
        }
    }
    assert_eq!(database.flaky_history("t", "gui", 0.0).unwrap().len(), 4);
    let items = super::super::inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "test_alert").expect("the lead's item");
    assert!(item.command.contains("\"action\":\"dismiss\"") && item.command.contains("\"suite\":\"gui\""), "{}", item.command);
    assert!(items.iter().any(|item| item.kind == "test_quarantine" && item.summary.contains("gui is quarantined as flaky (4 episode(s))")));
    let card = items.iter().find(|item| item.kind == "test_result").unwrap();
    assert!(card.summary.contains("passed only on its automatic retry (flaky)"), "{}", card.summary);
    // The owner sees the quarantine too, until it expires.
    let owner = super::super::owner_waiting::items(std::slice::from_ref(&team), 200.0);
    assert!(owner.iter().any(|item| item["kind"] == "test_quarantine"), "{owner:?}");
    assert!(super::super::owner_waiting::items(std::slice::from_ref(&team), 100.0 + 15.0 * 86_400.0).iter().all(|item| item["kind"] != "test_quarantine"));
    // Dismissed, the item goes; the next one needs three newer episodes.
    settle(&mut team, "dismiss", &json!({"kind":"flaky","suite":"gui"}), 300.0).unwrap();
    assert!(super::super::inbox::items(&team).iter().all(|item| item.kind != "test_alert"));
    assert!(settle(&mut team, "dismiss", &json!({"kind":"flaky","suite":"gui"}), 301.0).is_err());
    for episode in 4..7 {
        let ran = run_due(&database, dir.path(), &[request(&format!("r{episode}"), "gui", &flaky_command(&format!("m{episode}")))], 400.0 + f64::from(episode), &policy()).unwrap();
        apply(&mut team, &ran, &database, &policy(), 400.0 + f64::from(episode), (0.0, 0)).unwrap();
        let open = team.test_alerts.iter().filter(|alert| alert.kind == "flaky" && !alert.dismissed).count();
        assert_eq!(open, usize::from(episode == 6), "episode {episode}");
    }
    // Expired, the quarantine leaves the team at the next result.
    apply(&mut team, &[], &database, &policy(), 1e9, (0.0, 0)).unwrap();
    assert!(team.test_quarantine.is_empty());
}

#[test]
fn a_crash_is_never_retried_to_green() {
    let dir = tempfile::tempdir().unwrap();
    let database = store(dir.path());
    let counted = |marker: &str, crash: &str| {
        if cfg!(windows) {
            format!("echo run>>{marker} & {crash}")
        } else {
            format!("echo run >> {marker}; {crash}")
        }
    };
    // A sanitizer report, and (on Windows) an access violation's exit code.
    let mut cases = vec![("asan", counted("asan.txt", "echo ==1==ERROR: AddressSanitizer: heap-use-after-free & exit /b 1"))];
    if !cfg!(windows) {
        cases[0].1 = counted("asan.txt", "echo '==1==ERROR: AddressSanitizer: heap-use-after-free'; exit 1");
    } else {
        cases.push(("av", counted("av.txt", "exit /b -1073741819")));
    }
    for (name, command) in cases {
        let ran = run_due(&database, dir.path(), &[request(name, "gui", &command)], 100.0, &policy()).unwrap();
        assert_eq!(ran[0].status, "crashed", "{name}: {:?}", ran[0]);
        assert_eq!(ran[0].flags, vec!["crash"]);
        let runs = std::fs::read_to_string(dir.path().join(format!("{name}.txt"))).unwrap();
        assert_eq!(runs.lines().count(), 1, "{name} ran once");
    }
    assert!(database.flaky_history("t", "gui", 0.0).unwrap().is_empty());
    assert!(crashed(Some(1), "HEAP CORRUPTION DETECTED") && !crashed(Some(1), "assertion failed"));
}

#[test]
fn a_suite_grown_past_its_recorded_time_or_its_budget_is_flagged_for_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    let database = store(dir.path());
    let slow = if cfg!(windows) { "ping -n 2 127.0.0.1 >nul" } else { "sleep 1" };
    let mut grown = request("grown", "unit", slow);
    grown.seconds = 0.2;
    // Recorded 0.2 s, ran about a second: more than half over it.
    let ran = run_due(&database, dir.path(), &[grown.clone()], 100.0, &policy()).unwrap();
    assert_eq!(ran[0].status, "passed");
    assert_eq!(ran[0].flags, vec!["slower"]);
    assert_eq!(ran[0].recorded_ms, 200);
    let mut team = team();
    apply(&mut team, &ran, &database, &policy(), 100.0, (0.0, 0)).unwrap();
    let card = super::super::inbox::items(&team).into_iter().find(|item| item.kind == "test_result").unwrap();
    assert!(card.summary.contains("slower than recorded") && card.summary.contains("recorded 0 s"), "{}", card.summary);
    // Within the share: no flag; growth off: no flag.
    grown.seconds = 60.0;
    assert!(run_due(&database, dir.path(), &[Request { id: "fine".into(), ..grown.clone() }], 101.0, &policy()).unwrap()[0].flags.is_empty());
    let off = Policy { grow_percent: 0, ..policy() };
    grown.seconds = 0.2;
    assert!(run_due(&database, dir.path(), &[Request { id: "off".into(), ..grown.clone() }], 102.0, &off).unwrap()[0].flags.is_empty());
    // Over its time budget: flagged and one lead item.
    let mut over = request("over", "unit", slow);
    over.budget_minutes = 0.005;
    let ran = run_due(&database, dir.path(), &[over], 103.0, &policy()).unwrap();
    assert!(ran[0].flags.contains(&"over_time_budget".to_owned()), "{:?}", ran[0].flags);
    apply(&mut team, &ran, &database, &policy(), 103.0, (0.0, 0)).unwrap();
    let alert = team.test_alerts.iter().find(|alert| alert.kind == "time_budget").unwrap();
    assert!(alert.detail.contains("over its time budget"), "{}", alert.detail);
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn write(dir: &Path, file: &str, text: &str) {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn section(targets: &[(&str, &str)], files: &[(&str, &[&str])]) -> ProviderMap {
    ProviderMap {
        kind: crate::test_discipline::config::Kind::Unittest,
        rule: crate::test_discipline::config::Rule::Build,
        note: String::new(),
        targets: targets.iter().map(|(id, binary)| ((*id).to_owned(), Target { run: vec![(*id).to_owned()], binary: (*binary).to_owned(), tests: 1, ..Target::default() })).collect(),
        files: files.iter().map(|(file, ids)| ((*file).to_owned(), ids.iter().map(|id| (*id).to_owned()).collect())).collect(),
        test_files: Default::default(),
        rules: Vec::new(),
        groups: Default::default(),
        seconds: 0.0,
        platforms: Vec::new(),
    }
}

fn provider(kind: &str, reuse: &str) -> Provider {
    let text = format!("[[provider]]\nname = \"p\"\nkind = \"{kind}\"\nbuild_dir = \"b\"\ncommand = \"x\"\n{reuse}");
    let config: TestsConfig = serde_json::from_value(parse_toml(&text).unwrap()).unwrap();
    config.providers[0].clone()
}

#[test]
fn unchanged_inputs_reuse_the_last_green_result_and_rust_never_does() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "scripts/tool.py", "print(1)\n");
    write(dir.path(), "tests/test_tool.py", "def test_a(): pass\n");
    let map = section(&[("test_tool", "")], &[("scripts/tool.py", &["test_tool"]), ("tests/test_tool.py", &["test_tool"])]);
    let python = provider("unittest", "");
    let first = input_hash(dir.path(), &python, &map, &[]).unwrap();
    assert_eq!(input_hash(dir.path(), &python, &map, &[]), Some(first.clone()), "the same inputs, the same hash");
    write(dir.path(), "scripts/tool.py", "print(2)\n");
    let changed = input_hash(dir.path(), &python, &map, &[]).unwrap();
    assert_ne!(first, changed, "a source change is new inputs");
    // Rust is never reused, unless a project says otherwise.
    assert_eq!(input_hash(dir.path(), &provider("cargo", ""), &map, &[]), None);
    assert!(input_hash(dir.path(), &provider("cargo", "reuse = \"inputs\""), &map, &[]).is_some());
    // Qt: the test binary and its data files, not the sources compiled in.
    write(dir.path(), "build/test_ui.exe", "binary-1");
    write(dir.path(), "gui/fixtures/rows.json", "{}");
    write(dir.path(), "gui/Window.cpp", "int a;");
    let qt_map = section(&[("test_ui", "build/test_ui.exe")], &[("gui/Window.cpp", &["test_ui"]), ("gui/fixtures/rows.json", &["test_ui"])]);
    let qt = provider("ctest", "");
    let binary = input_hash(dir.path(), &qt, &qt_map, &[]).unwrap();
    write(dir.path(), "gui/Window.cpp", "int b;");
    assert_eq!(input_hash(dir.path(), &qt, &qt_map, &[]), Some(binary.clone()), "a source counts once it is built");
    write(dir.path(), "gui/fixtures/rows.json", "{\"a\":1}");
    let data = input_hash(dir.path(), &qt, &qt_map, &[]).unwrap();
    assert_ne!(binary, data, "the data the binary reads");
    write(dir.path(), "build/test_ui.exe", "binary-2");
    assert_ne!(input_hash(dir.path(), &qt, &qt_map, &[]), Some(data));
    // Without its binary a Qt suite is never reused.
    std::fs::remove_file(dir.path().join("build/test_ui.exe")).unwrap();
    assert_eq!(input_hash(dir.path(), &qt, &qt_map, &[]), None);

    // The database: only a measured green run is reused, recorded as reused
    // from it and due for re-measuring.
    let database = store(dir.path());
    let mut green = request("g", "py", if cfg!(windows) { "exit /b 0" } else { "true" });
    green.input_hash = first.clone();
    run_due(&database, dir.path(), &[green.clone()], 100.0, &policy()).unwrap();
    let mut red = request("red", "py", if cfg!(windows) { "exit /b 3" } else { "exit 3" });
    red.input_hash = changed.clone();
    let failed = run_due(&database, dir.path(), &[red], 101.0, &Policy { retries: 0, ..policy() }).unwrap();
    assert_eq!(failed[0].status, "failed");
    assert!(database.last_green("t", "py", &changed).unwrap().is_none(), "a red run is never reused");
    let source = database.last_green("t", "py", &first).unwrap().unwrap();
    let mut again = green.clone();
    again.id = "again".into();
    again.commit = "c2".into();
    let reused = database.write_reused(&again, &source, 200.0).unwrap();
    assert_eq!((reused.status.as_str(), reused.reused_from.as_str(), reused.due), ("reused", source.id.as_str(), true));
    assert_eq!(reused.flags, vec!["reused"]);
    // A reused record is never the source of another reuse.
    assert_eq!(database.last_green("t", "py", &first).unwrap().unwrap().id, source.id);
    let mut team = team();
    apply(&mut team, std::slice::from_ref(&reused), &database, &policy(), 200.0, (0.0, 0)).unwrap();
    let card = super::super::inbox::items(&team).into_iter().find(|item| item.kind == "test_result").unwrap();
    assert!(card.summary.contains(&format!("reused from record {}", source.id)) && card.summary.contains("due for re-measuring"), "{}", card.summary);
}

#[test]
fn enough_reused_results_propose_a_full_run_with_its_estimate() {
    let dir = tempfile::tempdir().unwrap();
    let database = store(dir.path());
    let mut green = request("g", "py", if cfg!(windows) { "exit /b 0" } else { "true" });
    green.input_hash = "same".into();
    let source = run_due(&database, dir.path(), &[green.clone()], 100.0, &policy()).unwrap().remove(0);
    let by_count = Policy { full_after: 3, ..policy() };
    let mut team = team();
    for n in 0..3 {
        let record = database.write_reused(&Request { id: format!("u{n}"), ..green.clone() }, &source, 200.0 + f64::from(n)).unwrap();
        apply(&mut team, &[record], &database, &by_count, 200.0 + f64::from(n), (1500.0, 2)).unwrap();
        assert_eq!(team.test_full_proposal.is_some(), n == 2, "after {} reuse(s)", n + 1);
    }
    let proposal = team.test_full_proposal.clone().unwrap();
    assert_eq!((proposal.reused, proposal.seconds, proposal.unknown, proposal.reason.as_str()), (3, 1500.0, 2, "count"));
    let text = super::super::inbox::full_run_text(&proposal);
    assert!(text.contains("3 result(s) reused") && text.contains("about 25 min") && text.contains("2 target(s) without a recorded time"), "{text}");
    // The lead's card and the owner's list both carry it.
    assert!(super::super::inbox::items(&team).iter().any(|item| item.kind == "test_full_run" && item.command.contains("full-done")));
    let owner = super::super::owner_waiting::items(std::slice::from_ref(&team), 300.0);
    let item = owner.iter().find(|item| item["kind"] == "test_full_run").expect("the owner's item");
    assert_eq!(item["seconds"], 1500.0);
    // A full run recorded: the count starts again from it.
    settle(&mut team, "full-done", &json!({}), 400.0).unwrap();
    assert!(team.test_full_proposal.is_none());
    assert_eq!(database.reused_since("t", team.test_full_at).unwrap().0, 0);
    // By age: a reused measurement older than the bound proposes one at once.
    let by_age = Policy { max_age_days: 1.0, ..policy() };
    let late = 100.0 + 2.0 * 86_400.0;
    let record = database.write_reused(&Request { id: "late".into(), ..green }, &source, late).unwrap();
    apply(&mut team, &[record], &database, &by_age, late, (60.0, 0)).unwrap();
    assert_eq!(team.test_full_proposal.as_ref().map(|proposal| proposal.reason.as_str()), Some("age"));
}

#[test]
fn a_report_carries_the_test_lines_and_functions_it_adds_and_removes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    write(root, "src/lib.rs", "pub fn a() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn old_case() {}\n    #[test]\n    fn kept() {}\n}\n");
    write(root, "tests/test_cli.py", "def test_one():\n    pass\n");
    write(root, "src/plain.rs", "pub fn b() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = super::super::dispatcher::git_at(&root.to_string_lossy(), &["rev-parse", "HEAD"]).unwrap();
    write(root, "src/lib.rs", "pub fn a() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn kept() {}\n    #[test]\n    fn new_case() {}\n    #[tokio::test]\n    async fn waits() {}\n}\n");
    write(root, "tests/test_cli.py", "def test_one():\n    pass\n\ndef test_two():\n    pass\n");
    write(root, "src/plain.rs", "pub fn b() {}\npub fn c() {}\n");
    git(root, &["commit", "-q", "-am", "work"]);
    let config: TestsConfig = serde_json::from_value(parse_toml("[[provider]]\nname = \"py\"\nkind = \"unittest\"\ntests = [\"tests/test_*.py\"]\n").unwrap()).unwrap();
    let delta = report_delta(&root.to_string_lossy(), &base, &config).unwrap();
    // A plain source file is no test file.
    assert_eq!(delta["files"], 2, "{delta}");
    assert_eq!(delta["functions_added"]["total"], 3, "{delta}");
    let added: Vec<&str> = delta["functions_added"]["names"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(added, ["src/lib.rs: new_case", "src/lib.rs: waits", "tests/test_cli.py: test_two"]);
    assert_eq!(delta["functions_removed"]["names"], json!(["src/lib.rs: old_case"]));
    assert!(delta["lines_added"].as_u64().unwrap() >= 5 && delta["lines_removed"].as_u64().unwrap() >= 2, "{delta}");
    let text = super::super::inbox::tests_text(&delta);
    assert!(text.starts_with("Tests: +") && text.contains("functions +3") && text.contains("-1 (src/lib.rs: old_case)"), "{text}");
    assert_eq!(super::super::inbox::tests_text(&json!({"files":0})), "Tests: no test file changed.");
}
