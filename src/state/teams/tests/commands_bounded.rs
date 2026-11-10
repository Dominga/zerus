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
/// It runs only when `ZERUS_LIVE_TEAMS_DIR` names a live teams folder (the
/// `sessions/teams` folder of a Zerus state) and skips otherwise, as on CI.
/// No machine's own path is written here (the source privacy guard).
#[test]
fn a_copy_of_the_live_store_reads_bounded_and_fast() {
    const TEAM: &str = "33fbafeb-8291-4eda-a00c-6e1448ec6f20";
    let Some(live) = std::env::var_os("ZERUS_LIVE_TEAMS_DIR").map(std::path::PathBuf::from) else {
        eprintln!("skipped: ZERUS_LIVE_TEAMS_DIR names no live teams folder");
        return;
    };
    let source = live.join("teams.sqlite3");
    if !source.is_file() {
        eprintln!("skipped: no live store in ZERUS_LIVE_TEAMS_DIR");
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
        sizes.sort_by_key(|size| std::cmp::Reverse(size.1));
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

/// Task #57: a team of 200 tasks with long descriptions, criteria, reports,
/// questions and replies polls well inside the bound: an accepted task is a
/// stub, an archived one carries its timing once, a live one keeps the start of
/// its description, its first criteria and its last attempt (earlier ones are
/// their number, id and state). The whole record stays `status --task N`.
fn crowded_team() -> Team {
    use crate::state::teams::model::{ArchivedTask, Attempt, AttemptState, Question, Report};
    let long = |n: usize| "A long line of the plan, written in full. ".repeat(n);
    let attempt = |n: u32, state: AttemptState| Attempt {
        n,
        id: format!("a{n}"),
        member: "dev".into(),
        state,
        report: Some(Report { text: long(500), at: 1.0, ..Default::default() }),
        questions: (0..6).map(|q| Question { id: format!("q{q}"), text: long(20), at: 1.0, answer: "yes".into(), answered_at: Some(2.0), ..Default::default() }).collect(),
        last_reply: long(250),
        ..Default::default()
    };
    let mut team = Team { id: "big".into(), ..Default::default() };
    team.tasks = (1..=200u32)
        .map(|id| {
            let mut task = Task {
                id,
                title: format!("Task {id}: {}", long(2)),
                description: long(100),
                criteria: (0..12).map(|_| long(10)).collect(),
                assignee: "dev".into(),
                phase: 1 + id / 50,
                ..Default::default()
            };
            match id % 5 {
                0 | 1 => {
                    task.status = TaskStatus::Accepted;
                    task.attempts = vec![attempt(1, AttemptState::Rejected), attempt(2, AttemptState::Reported)];
                }
                2 => {
                    task.status = TaskStatus::Accepted;
                    task.archived = Some(ArchivedTask { accepted_at: Some(3.0), started_at: Some(1.0), timing: json!({"running_since":null,"waits":{"deps_s":0,"lead_s":5,"member_s":9},"worked_s":60}), ..Default::default() });
                }
                3 => task.status = TaskStatus::Pending,
                _ => {
                    task.status = TaskStatus::Submitted;
                    task.attempts = vec![attempt(1, AttemptState::Rejected), attempt(2, AttemptState::Rejected), attempt(3, AttemptState::Reported)];
                }
            }
            task
        })
        .collect();
    team
}

#[test]
fn a_team_of_200_tasks_with_long_reports_polls_well_inside_the_bound() {
    let team = crowded_team();
    let mut whole = view_with(&team, &[], None);
    let unbounded = serde_json::to_vec(&whole).unwrap().len();
    bound_status(&mut whole, &team);
    let bytes = serde_json::to_vec(&whole).unwrap().len();
    eprintln!("200 tasks: {unbounded} bytes whole, {bytes} bytes polled");
    let mut fields: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for task in whole["tasks"].as_array().unwrap() {
        for (name, value) in task.as_object().unwrap() {
            *fields.entry(name.clone()).or_default() += serde_json::to_vec(value).unwrap().len();
        }
    }
    let mut fields: Vec<_> = fields.into_iter().collect();
    fields.sort_by_key(|field| std::cmp::Reverse(field.1));
    eprintln!("task fields: {:?}", &fields[..fields.len().min(10)]);
    // Well inside: two thirds of the bound, with the room left for the team's
    // other lists.
    assert!(bytes < 300 * 1024 * 2 / 3, "the polled status is {bytes} bytes");
    let tasks = whole["tasks"].as_array().unwrap();
    // A live task keeps every attempt for its count, its last one in full.
    let reported = tasks.iter().find(|task| task["id"] == 4).unwrap();
    let attempts = reported["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[0], json!({"n":1,"id":"a1","state":"rejected"}));
    assert_eq!(attempts[2]["state"], "reported");
    assert!(attempts[2]["report"]["text"].as_str().unwrap().contains("clipped; hgs team status --task N"));
    assert!(attempts[2].get("last_reply").is_none());
    assert_eq!(attempts[2]["questions"], json!([]), "answered questions are history");
    // The start of the description and the first criteria, with their count.
    assert!(reported["description"].as_str().unwrap().len() < 300);
    assert_eq!(reported["criteria"].as_array().unwrap().len(), 3);
    assert_eq!(reported["criteria_total"], 12);
    // An archived stub has its timing once, in `archived`.
    let archived = tasks.iter().find(|task| task["id"] == 2).unwrap();
    assert!(archived.get("timing").is_none() && archived.get("attempts").is_none(), "{archived}");
    assert_eq!(archived["archived"]["timing"]["worked_s"], 60);
    assert!(archived["archived"].get("merge_commit").is_none() && archived["archived"]["timing"].get("running_since").is_none());
}

/// Task #57: the desktop's plan edit sends the whole plan back from its read,
/// so that read (`"full_tasks":true`) keeps every unfinished task whole.
#[test]
fn a_plan_edit_reads_whole_tasks() {
    let team = crowded_team();
    let mut full = view_with(&team, &[], None);
    bound_status_with(&mut full, &team, true);
    let pending = full["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 3).unwrap();
    assert_eq!(pending["description"].as_str().unwrap(), team.task(3).unwrap().description);
    assert_eq!(pending["criteria"].as_array().unwrap().len(), 12);
    let reported = full["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 4).unwrap();
    assert!(reported["attempts"][0].get("report").is_some(), "earlier attempts stay whole");
    assert!(reported["attempts"][2].get("last_reply").is_some());
}
