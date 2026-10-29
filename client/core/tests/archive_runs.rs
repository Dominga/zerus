use serde_json::{json, Map, Value};
use zerus_client_core::settings::QtStore;
use zerus_client_core::swarm::layout::{seed_runs_from_qt, Layout, MAX_RUNS};

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

#[test]
fn qt_run_seed_runs_once_validates_entries_and_preserves_newer_native_writes() {
    let mut layout = Layout::default();
    layout.remember_run("workstation", "current", Some("native"));
    let qt = json!({
        "workstation\ncurrent": "old-qt",
        "workstation\narchived": "p",
        "peer\narchived": "q",
        "no-separator": "p",
        "\nrun": "p",
        "workstation\n": "p",
        "workstation\nrun\nextra": "p",
        "workstation\nnumber": 1,
        "workstation\nempty": "",
        "workstation\ncontrol": "p\r"
    });
    assert_eq!(layout.seed_runs(qt.as_object().unwrap()), 2);
    assert!(layout.runs_seeded());
    assert_eq!(layout.run_project("workstation", "current"), Some("native"));
    assert_eq!(layout.run_project("workstation", "archived"), Some("p"));
    assert_eq!(layout.run_project("peer", "archived"), Some("q"));
    assert!(layout.take_dirty().is_some());
    let mut again = Layout::from_json(&layout.to_json());
    assert_eq!(
        again.seed_runs(json!({"workstation\nlate": "p"}).as_object().unwrap()),
        0
    );
    assert_eq!(again.run_project("workstation", "late"), None);
    assert!(again.take_dirty().is_none());
}

#[test]
fn even_an_empty_qt_seed_is_persisted_once() {
    let mut layout = Layout::default();
    assert!(!layout.runs_seeded());
    assert_eq!(layout.seed_runs(&Map::new()), 0);
    assert!(layout.runs_seeded());
    assert!(layout.take_dirty().is_some());
    assert_eq!(layout.seed_runs(&Map::new()), 0);
    assert!(layout.take_dirty().is_none());
    assert!(Layout::from_json(&layout.to_json()).runs_seeded());
}

#[test]
fn qt_seed_and_layout_reload_keep_only_the_newest_twenty_thousand_entries() {
    let mut qt = Map::new();
    let mut saved = Vec::new();
    for n in 0..MAX_RUNS + 3 {
        let token = format!("workstation\nrun-{n:05}");
        qt.insert(token.clone(), json!("p"));
        saved.push((token, "p"));
    }
    let mut layout = Layout::default();
    assert_eq!(layout.seed_runs(&qt), MAX_RUNS);
    let loaded = Layout::from_json(&serde_json::to_vec(&json!({"run_groups": saved})).unwrap());
    for memory in [&layout, &loaded] {
        assert_eq!(memory.run_project("workstation", "run-00000"), None);
        assert_eq!(memory.run_project("workstation", "run-00002"), None);
        assert_eq!(memory.run_project("workstation", "run-00003"), Some("p"));
        assert_eq!(memory.run_project("workstation", "run-20002"), Some("p"));
        let data: Value = serde_json::from_slice(&memory.to_json()).unwrap();
        assert_eq!(data["run_groups"].as_array().unwrap().len(), MAX_RUNS);
    }
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zerus-archive-run-seed-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn qt_ini(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("[workspace]\norganization=\"@ByteArray({escaped})\"\n").into_bytes()
}

#[test]
fn the_qt_seed_reads_ini_bytearray_and_plist_data_without_writing_either() {
    let dir = Scratch::new("formats");
    let bytes = serde_json::to_vec(&json!({
        "run_groups": {"workstation\nrun": "p"},
        "runs": {"workstation\nrun": "workstation\ncodex/p/tag"},
        "projects": [{"id": "qt-only", "name": "Qt"}]
    }))
    .unwrap();
    let ini = dir.0.join("qt.ini");
    std::fs::write(&ini, qt_ini(&bytes)).unwrap();
    let plist = dir.0.join("qt.plist");
    let mut values = plist::Dictionary::new();
    values.insert("workspace.organization".into(), plist::Value::Data(bytes));
    plist::Value::Dictionary(values)
        .to_file_xml(&plist)
        .unwrap();
    for store in [
        QtStore::Ini(ini),
        QtStore::Plist {
            domain: None,
            path: plist,
        },
    ] {
        let before = std::fs::read(store.path()).unwrap();
        let mut layout = Layout::default();
        assert_eq!(seed_runs_from_qt(&mut layout, &store).unwrap(), 1);
        assert_eq!(layout.run_project("workstation", "run"), Some("p"));
        assert_eq!(
            layout.default_project(),
            None,
            "the Qt catalog is not imported"
        );
        assert_eq!(layout.len(), 0, "Qt rename tokens are separate");
        assert_eq!(std::fs::read(store.path()).unwrap(), before);
        std::fs::write(store.path(), b"malformed now").unwrap();
        assert_eq!(seed_runs_from_qt(&mut layout, &store).unwrap(), 0);
        assert_eq!(std::fs::read(store.path()).unwrap(), b"malformed now");
    }
}

#[test]
fn unreadable_or_malformed_qt_seed_preserves_native_runs_and_can_be_retried() {
    let dir = Scratch::new("errors");
    let path = dir.0.join("qt.ini");
    std::fs::create_dir(&path).unwrap();
    let store = QtStore::Ini(path.clone());
    let mut layout = Layout::default();
    layout.remember_run("workstation", "native", Some("p"));
    layout.take_dirty();
    assert!(seed_runs_from_qt(&mut layout, &store).is_err());
    assert!(!layout.runs_seeded());
    assert!(layout.take_dirty().is_none());
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, qt_ini(b"not json")).unwrap();
    assert!(seed_runs_from_qt(&mut layout, &store).is_err());
    assert!(!layout.runs_seeded());
    assert_eq!(layout.run_project("workstation", "native"), Some("p"));
    assert!(layout.take_dirty().is_none());
    std::fs::write(&path, qt_ini(br#"{"run_groups":{"peer\nold":"q"}}"#)).unwrap();
    assert_eq!(seed_runs_from_qt(&mut layout, &store).unwrap(), 1);
    assert!(layout.runs_seeded());
    assert_eq!(layout.run_project("peer", "old"), Some("q"));
}
