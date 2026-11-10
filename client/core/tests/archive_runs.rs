use serde_json::Value;
use zerus_client_core::swarm::layout::{Layout, MAX_RUNS};

#[test]
fn explicit_run_projects_are_machine_scoped_and_implicit_ones_are_forgotten() {
    let mut layout = Layout::default();
    assert!(layout.remember_run("workstation", "run", Some("project")));
    assert!(layout.take_dirty().is_some());
    assert_eq!(layout.run_project("workstation", "run"), Some("project"));
    assert_eq!(layout.run_project("peer", "run"), None);
    assert!(layout.remember_run("peer", "run", Some("other")));
    assert!(layout.remember_run("workstation", "run", None));
    assert_eq!(layout.run_project("workstation", "run"), None);
    assert_eq!(layout.run_project("peer", "run"), Some("other"));
    layout.take_dirty();
    assert!(!layout.remember_run("workstation", "run", None));
    assert!(layout.take_dirty().is_none());
    for (machine, run, project) in [
        ("", "run", "p"),
        ("workstation", "", "p"),
        ("work\nstation", "run", "p"),
        ("workstation", "run\nextra", "p"),
        ("workstation", "run", ""),
        ("workstation", "run", "p\r"),
    ] {
        assert!(!layout.remember_run(machine, run, Some(project)));
    }
    assert!(layout.take_dirty().is_none());
}

#[test]
fn run_memory_evicts_the_oldest_write_not_the_oldest_read_or_key() {
    assert_eq!(MAX_RUNS, 20_000);
    let mut layout = Layout::default();
    for n in 0..MAX_RUNS {
        layout.remember_run("workstation", &format!("run-{n}"), Some("p"));
    }
    assert_eq!(layout.run_project("workstation", "run-1"), Some("p"));
    // A write of the same assignment makes that run newer. A lookup does not.
    assert!(layout.remember_run("workstation", "run-0", Some("p")));
    layout.remember_run("workstation", "new", Some("p"));
    assert_eq!(layout.run_project("workstation", "run-1"), None);
    assert_eq!(layout.run_project("workstation", "run-0"), Some("p"));
    layout.take_dirty();
    assert!(!layout.remember_run("workstation", "new", Some("p")));
    assert!(
        layout.take_dirty().is_none(),
        "already newest and unchanged"
    );
    let saved: Value = serde_json::from_slice(&layout.to_json()).unwrap();
    assert_eq!(saved["run_groups"].as_array().unwrap().len(), MAX_RUNS);
}

#[test]
fn run_memory_round_trips_with_its_write_order_and_other_layout_settings() {
    let mut layout = Layout::default();
    layout.set_collapsed("p", true);
    layout.set_default_project(Some("p".into()));
    let rank = layout.rank("p", "workstation\ncodex/p/tag");
    layout.remember_run("workstation", "z-first", Some("p"));
    layout.remember_run("workstation", "a-second", Some("q"));
    for n in 2..MAX_RUNS {
        layout.remember_run("workstation", &format!("run-{n}"), Some("q"));
    }
    let mut again = Layout::from_json(&layout.to_json());
    assert!(again.take_dirty().is_none());
    assert!(again.collapsed("p"));
    assert_eq!(again.default_project(), Some("p"));
    assert_eq!(again.rank("p", "workstation\ncodex/p/tag"), rank);
    again.remember_run("workstation", "new", Some("p"));
    assert_eq!(again.run_project("workstation", "z-first"), None);
    assert_eq!(again.run_project("workstation", "a-second"), Some("q"));
}

