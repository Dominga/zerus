use super::*;
use crate::state::teams::test_queue::{tick, Places, TestStore};
use std::cell::RefCell;

/// A remote reached in this process: the runner's side runs against its own
/// state folder and a synthetic person, as `hgs @alias test runner --json`
/// would on the other computer.
struct Fake {
    state: PathBuf,
    person: Person,
    down: bool,
    calls: RefCell<Vec<(String, Value)>>,
}

impl Remote for Fake {
    fn call(&self, alias: &str, input: &Value) -> anyhow::Result<Value> {
        self.calls.borrow_mut().push((alias.to_owned(), input.clone()));
        anyhow::ensure!(!self.down, "ssh: connect to host {alias} port 22: Connection refused");
        Ok(crate::state::with_test_state_root(&self.state, || serve(input, &self.person)))
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A project whose `unit` suite prints the checked-out marker, its home and
/// temporary folders, and fails when a target says so.
fn project(dir: &Path) -> (String, String) {
    let command = if cfg!(windows) {
        "type marker.txt & echo HOME=%HOME% & echo TEMP=%TEMP% & if \"{targets}\"==\"fail\" exit /b 3"
    } else {
        "cat marker.txt; echo HOME=$HOME; echo TEMP=$TEMP; if [ \"{targets}\" = fail ]; then exit 3; fi"
    };
    std::fs::create_dir_all(dir.join(".zerus")).unwrap();
    std::fs::write(
        dir.join(".zerus/tests.toml"),
        format!("version = 1\n[[provider]]\nname = \"unit\"\nkind = \"command\"\ncommand = '{command}'\nsources = [\"**\"]\nplatforms = [\"linux\"]\n"),
    )
    .unwrap();
    git(dir, &["init", "-q"]);
    std::fs::write(dir.join("marker.txt"), "marker-one\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "one"]);
    let first = git(dir, &["rev-parse", "HEAD"]);
    std::fs::write(dir.join("marker.txt"), "marker-two\n").unwrap();
    git(dir, &["commit", "-q", "-am", "two"]);
    let second = git(dir, &["rev-parse", "HEAD"]);
    (first, second)
}

fn request(id: &str, commit: &str, targets: &[&str]) -> Request {
    Request {
        id: id.into(),
        team: "t".into(),
        suite: "unit".into(),
        task: Some(3),
        targets: targets.iter().map(|target| (*target).to_owned()).collect(),
        commit: commit.into(),
        input_hash: String::new(),
        asked_by: "dev".into(),
        forced: false,
        at: 1.0,
        seconds: 0.0,
        budget_minutes: 10.0,
        resources: String::new(),
        command: "never run here".into(),
        platforms: vec!["linux".into()],
    }
}

fn fake(down: bool) -> (tempfile::TempDir, Fake) {
    let remote = tempfile::tempdir().unwrap();
    let home = remote.path().join("person-home");
    let temp = remote.path().join("person-temp");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::write(home.join(".profile"), "the person's own").unwrap();
    let person = Person { home: Some(home), temp, dumps: None, state: remote.path().join("state") };
    let state = remote.path().join("state");
    (remote, Fake { state, person, down, calls: RefCell::new(Vec::new()) })
}

fn linux() -> Vec<Runner> {
    vec![Runner { alias: "box".into(), platform: "linux".into() }]
}

#[test]
fn a_runner_of_the_class_runs_the_exact_commit_isolated_and_the_result_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _second) = project(dir.path());
    let store = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
    let (remote, fake) = fake(false);
    let policy = Policy { retries: 0, ..Default::default() };
    let done = dispatch(&store, dir.path(), &[request("r1", &first, &["ok"])], &linux(), &fake, 100.0, &policy).unwrap();
    assert_eq!(done.len(), 1);
    let record = &done[0];
    assert_eq!((record.status.as_str(), record.runner.as_str(), record.task), ("passed", "box", Some(3)), "{record:?}");
    // The exact commit, not the project's newer HEAD.
    assert!(record.excerpt.contains("marker-one") && !record.excerpt.contains("marker-two"), "{}", record.excerpt);
    // Its home and temporary folders were the run's own, below the runner's
    // Zerus state, never the person's.
    let runner_state = fake.state.join("test-runner").to_string_lossy().replace('\\', "/").to_lowercase();
    let lines: Vec<String> = record.excerpt.lines().map(|line| line.trim().replace('\\', "/").to_lowercase()).collect();
    for name in ["home=", "temp="] {
        let line = lines.iter().find(|line| line.starts_with(name)).unwrap_or_else(|| panic!("{name} in {}", record.excerpt));
        assert!(line.contains(&runner_state), "{line}");
    }
    assert_eq!(std::fs::read_to_string(fake.person.home.as_ref().unwrap().join(".profile")).unwrap(), "the person's own");
    assert!(std::fs::read_dir(&fake.person.temp).unwrap().next().is_none(), "nothing new in the person's temp");
    // The run's folder went; the mirror stays for the next run.
    let root = std::fs::read_dir(fake.state.join("test-runner")).unwrap().next().unwrap().unwrap().path();
    assert!(root.join("mirror.git").is_dir());
    assert!(std::fs::read_dir(root.join("runs")).unwrap().next().is_none());
    // The result is in the database with its runner; the queue is done.
    assert_eq!(store.results(Some("t"), None, 10, None, None).unwrap()[0].runner, "box");
    // The first run carried a bundle; the next one of the same commit none.
    let calls = fake.calls.borrow().clone();
    assert_eq!(calls.iter().map(|(_, input)| input["action"].as_str().unwrap()).collect::<Vec<_>>(), ["have", "run"]);
    assert!(!calls[1].1["bundle"].as_str().unwrap().is_empty());
    assert_eq!(calls[1].1["commit"], first);
    dispatch(&store, dir.path(), &[request("r2", &first, &["ok"])], &linux(), &fake, 101.0, &policy).unwrap();
    assert_eq!(fake.calls.borrow()[3].1["bundle"], "", "the runner had the commit");
    drop(remote);
}

#[test]
fn a_runner_runs_only_what_the_commits_project_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = project(dir.path());
    let (_remote, fake) = fake(false);
    let (key, _) = crate::test_discipline::tools::project_key(dir.path());
    let bundle = bundle(dir.path(), &first, &[]).unwrap();
    // A suite the project file does not name is refused; a sent command is never read.
    let answer = fake.call("box", &json!({"action":"run","project":key,"commit":first,"bundle":bundle,"suite":"rm-everything","command":"exit 9","request":"x"})).unwrap();
    assert!(answer["error"].as_str().unwrap().contains("No provider rm-everything"), "{answer}");
    // A commit the runner does not have is said so.
    let answer = fake.call("box", &json!({"action":"run","project":key,"commit":"0123456789abcdef0123456789abcdef01234567","bundle":"","suite":"unit","request":"y"})).unwrap();
    assert!(answer["error"].as_str().unwrap().contains("lacks commit"), "{answer}");
}

#[test]
fn an_unreachable_runner_or_none_of_the_class_leaves_the_suite_queued_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = project(dir.path());
    let store = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
    let (_remote, down) = fake(true);
    let queued = request("q1", &first, &["ok"]);
    store.enqueue(&queued).unwrap();
    let done = dispatch(&store, dir.path(), std::slice::from_ref(&queued), &linux(), &down, 100.0, &Policy::default()).unwrap();
    assert!(done.is_empty());
    let pending = store.pending("t").unwrap();
    assert_eq!((pending.len(), pending[0].state.as_str()), (1, "queued"));
    assert!(pending[0].reason.contains("box unreachable") && pending[0].reason.contains("Connection refused"), "{}", pending[0].reason);
    // No runner of its class: it waits, never on another platform.
    let (_up_remote, up) = fake(false);
    let windows_only = vec![Runner { alias: "win".into(), platform: "windows".into() }];
    let done = dispatch(&store, dir.path(), std::slice::from_ref(&queued), &windows_only, &up, 101.0, &Policy::default()).unwrap();
    assert!(done.is_empty() && up.calls.borrow().is_empty(), "the windows runner was never asked");
    assert!(store.pending("t").unwrap()[0].reason.starts_with("no test runner for linux"));
    assert!(store.results(Some("t"), None, 10, None, None).unwrap().is_empty());
}

#[test]
fn the_tick_sends_a_suite_of_another_class_to_its_runner_and_runs_the_rest_here() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = project(dir.path());
    let store = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
    let (_remote, fake) = fake(false);
    let mut here = request("local", &first, &[]);
    here.platforms = Vec::new();
    here.command = "echo ran here".into();
    store.enqueue(&here).unwrap();
    store.enqueue(&request("remote", &first, &["ok"])).unwrap();
    let places = Places { host: vec!["windows".into()], runners: linux(), remote: &fake };
    let load = || crate::state::teams::test_queue::Load { cpu_free: 8, ram_free_mb: 8000, gpu_free: true, at_night: false };
    let (records, read) =
        tick(&store, dir.path(), "t", &Default::default(), &Default::default(), 12, 0.0, 100.0, load, &Policy::default(), &places).unwrap();
    assert!(read);
    let mut where_ran: Vec<(String, String)> = records.iter().map(|record| (record.suite.clone() + ":" + &record.id, record.runner.clone())).collect();
    where_ran.sort();
    assert_eq!(records.len(), 2, "{records:?}");
    assert!(records.iter().any(|record| record.runner.is_empty() && record.excerpt.contains("ran here")), "{where_ran:?}");
    assert!(records.iter().any(|record| record.runner == "box" && record.excerpt.contains("marker-one")), "{where_ran:?}");
    // The classes: none or this computer's runs here.
    assert!(runs_here(&[], &["linux".into()]) && runs_here(&["linux".into(), "macos".into()], &["linux".into()]));
    assert!(!runs_here(&["android".into()], &["windows".into()]));
}

#[test]
fn the_owner_makes_a_machine_a_test_runner_of_one_class() {
    let mut config = crate::config::Config::load().unwrap();
    config.machines.clear();
    config.machines.insert("box".into(), crate::machines::Connection { test_runner: "linux".into(), ..Default::default() });
    config.machines.insert("off".into(), crate::machines::Connection { test_runner: "macos".into(), enabled: false, ..Default::default() });
    config.machines.insert("plain".into(), crate::machines::Connection::default());
    assert_eq!(configured(&config), vec![Runner { alias: "box".into(), platform: "linux".into() }]);
    // Only the four classes; the JSON the desktop writes carries it.
    let parsed: crate::machines::Connection = serde_json::from_value(json!({"hostname":"box.example.com","test_runner":"macos"})).unwrap();
    assert_eq!(parsed.test_runner, "macos");
    assert!(crate::machines::TEST_RUNNER_CLASSES.contains(&"android"));
}
