// Task #393 (owner, 2026-11-08): the polled status is bounded. Report text is
// clipped, the test-function lists are dropped (their counts stay), an attempt
// carries its last run, and only the newest lead changes and proposals are
// listed; `hgs team status --task N` keeps the full record.
use super::*;
use crate::state::teams::model::{Task, TaskStatus};

fn one_open_task() -> Team {
    let mut team = Team { id: "t".into(), ..Default::default() };
    team.tasks = vec![Task { id: 1, title: "A task".into(), status: TaskStatus::Pending, ..Default::default() }];
    team
}

fn big_value() -> Value {
    let names: Vec<String> = (0..300).map(|n| format!("client/src/file_{n}.rs: a_test_name_{n}")).collect();
    let report = json!({
        "at": 1.0,
        "text": "A report line.\n".repeat(500),
        "tests": {"files": 12, "lines_added": 10, "lines_removed": 2,
            "functions_added": {"names": names, "total": 300},
            "functions_removed": {"names": ["a: b"], "total": 1}},
    });
    let runs: Vec<Value> = (0..19).map(|n| json!({"template":"cargo-test","at":n as f64,"exit":0,"ms":10,
        "args":{"filter":"state::teams"},"tail":"a long tail line\n"})).collect();
    let attempts = json!([{"n":1,"member":"dev","state":"reported","report":report,"runs":runs}]);
    json!({"tasks":[{"id":1,"title":"A task","status":"pending","attempts":attempts}],
        "lead_changes": (0..30).map(|n| json!({"id":format!("lc{n}"),"summary":"x"})).collect::<Vec<_>>(),
        "proposals": (0..30).map(|n| json!({"id":format!("p{n}"),"state":"open"})).collect::<Vec<_>>()})
}

#[test]
fn the_polled_status_clips_reports_runs_and_pages() {
    let team = one_open_task();
    let mut value = big_value();
    bound_status(&mut value, &team);
    let attempt = &value["tasks"][0]["attempts"][0];
    let report = &attempt["report"];
    let text = report["text"].as_str().unwrap();
    assert!(text.contains("clipped; hgs team status --task N"), "{}", &text[text.len().saturating_sub(120)..]);
    assert!(text.len() < 2500, "{}", text.len());
    // The function lists are gone; their counts and the line counts stay.
    assert!(report["tests"]["functions_added"].get("names").is_none(), "{}", report["tests"]["functions_added"]);
    assert_eq!(report["tests"]["functions_added"]["total"], 300);
    assert_eq!(report["tests"]["lines_added"], 10);
    // One run remains, with its template, exit code and a clipped tail.
    let runs = attempt["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["template"], "cargo-test");
    assert_eq!(runs[0]["exit"], 0);
    assert!(runs[0].get("args").is_none(), "{runs:?}");
    assert_eq!(attempt["runs_total"], 19);
    // The pages keep the newest twenty and count the whole list.
    assert_eq!(value["lead_changes"].as_array().unwrap().len(), 20);
    assert_eq!(value["lead_changes_total"], 30);
    assert_eq!(value["proposals"].as_array().unwrap().len(), 20);
    assert_eq!(value["proposals_total"], 30);
    // A small record is left as it is.
    let mut small = json!({"tasks":[{"attempts":[{"report":{"text":"short"}}]}],"lead_changes":[{"id":"lc1"}],"proposals":[]});
    bound_status(&mut small, &team);
    assert_eq!(small["tasks"][0]["attempts"][0]["report"]["text"], "short");
}

/// Task #393: the acceptance measurement on a copy of this machine's live
/// store: one status and one lead inbox, their time and the status's size.
/// It skips where the store (or the team) is absent, as on CI.
#[test]
fn a_copy_of_the_live_store_reads_bounded_and_fast() {
    const TEAM: &str = "33fbafeb-8291-4eda-a00c-6e1448ec6f20";
    let live = std::path::Path::new("C:/Users/whitl/.local/state/hgs/sessions/teams");
    let source = live.join("teams.sqlite3");
    if !source.is_file() {
        eprintln!("skipped: no live store on this machine");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("teams");
    std::fs::create_dir_all(&copy).unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let from = std::path::PathBuf::from(format!("{}{suffix}", source.display()));
        if from.is_file() {
            std::fs::copy(&from, copy.join(format!("teams.sqlite3{suffix}"))).unwrap();
        }
    }
    let status = crate::state::with_test_state_root(dir.path(), || {
        let started = std::time::Instant::now();
        let status = super::run("status", &json!({"team":TEAM}), false);
        eprintln!("status: {:?}", started.elapsed());
        status
    });
    let Ok(status) = status else {
        eprintln!("skipped: the copy has no team {TEAM}");
        return;
    };
    // A second poll of the same copy: the UI's 3 s poll is a warm read.
    crate::state::with_test_state_root(dir.path(), || {
        let started = std::time::Instant::now();
        let _ = super::run("status", &json!({"team":TEAM}), false);
        eprintln!("status warm: {:?}", started.elapsed());
    });
    let bytes = serde_json::to_vec(&status).unwrap().len();
    eprintln!("status: {bytes} bytes");
    if let Some(fields) = status.as_object() {
        let mut sizes: Vec<(String, usize)> = fields
            .iter()
            .map(|(name, value)| (name.clone(), serde_json::to_vec(value).map(|bytes| bytes.len()).unwrap_or(0)))
            .collect();
        sizes.sort_by(|a, b| b.1.cmp(&a.1));
        for (name, size) in sizes.iter().take(8) {
            eprintln!("field {name}: {size} bytes");
        }
    }
    // Where the time goes: the store load, the view's build, the bounding and
    // the serialization.
    crate::state::with_test_state_root(dir.path(), || {
        let started = std::time::Instant::now();
        let store = Store::open(false).unwrap();
        let loaded = store.load(TEAM).unwrap().expect("the team");
        let load = started.elapsed();
        let started = std::time::Instant::now();
        let pulses = store.latest_pulses(TEAM, crate::state::now());
        let pulse = started.elapsed();
        let started = std::time::Instant::now();
        let mut view = view_with_cancelled(&loaded, &[], None, false);
        let build = started.elapsed();
        let started = std::time::Instant::now();
        bound_status(&mut view, &loaded);
        let bounded = started.elapsed();
        let started = std::time::Instant::now();
        let size = serde_json::to_vec(&view).unwrap().len();
        let serialize = started.elapsed();
        eprintln!("stages: load {load:?}, pulses {pulse:?} ({}), view {build:?}, bound {bounded:?}, serialize {serialize:?} ({size} bytes)", pulses.len());
    });
    let inbox = crate::state::with_test_state_root(dir.path(), || {
        let started = std::time::Instant::now();
        let inbox = super::run("inbox", &json!({"team":TEAM,"member":"lead"}), false);
        eprintln!("inbox: {:?}", started.elapsed());
        inbox
    });
    match inbox {
        Ok(inbox) => eprintln!("inbox: {} bytes", serde_json::to_vec(&inbox).unwrap().len()),
        Err(error) => eprintln!("inbox skipped: {error:#}"),
    }
    assert!(bytes < 300 * 1024, "the polled status is {bytes} bytes");
}
