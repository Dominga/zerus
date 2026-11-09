use super::*;
use std::fs;
use zerus_replica::{Actor, NewTask, Stage, Target};

struct Fixture {
    _root: tempfile::TempDir,
    config: Config,
    uuid: String,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        let config = Config {
            home: base.to_str().unwrap().into(),
            dir: base.join("config"),
            state: base.join("state"),
            selves: vec!["example-local".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        let mut catalog = LockedStore::load(&config).unwrap();
        catalog.save().unwrap();
        drop(catalog);
        let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
        let mut task = TaskDoc::create(NewTask {
            project: "p",
            id: &store.unused_short_id().unwrap(),
            text: "Original Start",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        store.save(&mut task).unwrap();
        Self {
            _root: root,
            config,
            uuid: task.uuid().into(),
        }
    }
    fn store(&self) -> ProjectStore {
        ProjectStore::open(&self.config.state.join("replica"), "p").unwrap()
    }
    fn hold(&self, on: bool) {
        let store = self.store();
        let mut project = store.project().unwrap();
        project.set_cutover_hold(on, &Actor::Person, 2).unwrap();
        store.save_project(&project).unwrap();
    }
    fn bytes(&self) -> (Vec<u8>, Option<Vec<u8>>) {
        let store = self.store();
        let path = store
            .dir()
            .join("tasks")
            .join(format!("{}.loro", self.uuid));
        (
            fs::read(&path).unwrap(),
            fs::read(path.with_extension("updates")).ok(),
        )
    }
    fn mode(&self) -> std::result::Result<Mode, Closed> {
        read(&self.config, "p", &self.uuid, until(), SCAN_ENTRIES).unwrap()
    }
}
fn until() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
struct NoNetwork;
impl Net for NoNetwork {
    fn call(&self, _: &str, _: Route<'_>, _: &str, _: Option<&Value>, _: Instant) -> Result<Value> {
        panic!("No remote writer exists in this fixture")
    }
}

#[test]
fn planning_reads_the_exact_gate_without_writing_a_floor_or_task_bytes() {
    let f = Fixture::new();
    let bytes = f.bytes();
    assert_eq!(f.mode(), Ok(Mode::Managed));
    assert_eq!(f.bytes(), bytes);
    assert!(!f.store().load(&f.uuid).unwrap().execution_managed());
    f.hold(true);
    assert_eq!(f.mode(), Ok(Mode::Legacy));
    assert_eq!(f.bytes(), bytes);
    f.hold(false);
    let store = f.store();
    let mut task = store.load(&f.uuid).unwrap();
    task.floor().unwrap();
    store.save(&mut task).unwrap();
    drop(store);
    f.hold(true);
    let floored = f.bytes();
    assert_eq!(f.mode(), Ok(Mode::Managed));
    assert_eq!(f.bytes(), floored);
}

#[test]
fn unknown_scan_or_members_and_damaged_bytes_never_choose_legacy() {
    let f = Fixture::new();
    let bytes = f.bytes();
    assert!(read(&f.config, "p", &f.uuid, until(), 0).unwrap().is_err());
    let folder = f.config.dir.join("swarm");
    // A malformed strict member journal is unknown even with no direct peers.
    let records = folder.join("replica-members.json");
    fs::write(&records, b"malformed").unwrap();
    assert!(f.mode().is_err());
    fs::remove_file(records).unwrap();
    assert_eq!(f.bytes(), bytes);
    let path = f
        .store()
        .dir()
        .join("tasks")
        .join(format!("{}.updates", f.uuid));
    fs::write(&path, b"a truncated retained local update").unwrap();
    let damaged = f.bytes();
    assert_eq!(f.mode().unwrap_err().reason, "damaged");
    assert_eq!(f.bytes(), damaged);
}

#[test]
fn both_mismatch_directions_refuse_before_floor_reservation_or_native_work() {
    for planned in [Mode::Managed, Mode::Legacy] {
        let f = Fixture::new();
        f.hold(planned == Mode::Legacy);
        assert_eq!(f.mode(), Ok(planned));
        f.hold(planned == Mode::Managed);
        let bytes = f.bytes();
        let result = admit(
            &f.config,
            &NoNetwork,
            "p",
            &f.uuid,
            until(),
            Some(planned),
            |_, _| -> Result<std::result::Result<(), Closed>> {
                panic!("A mismatch never reserves")
            },
        )
        .unwrap();
        assert_eq!(result, Admission::Refused(changed()));
        assert_eq!(f.bytes(), bytes);
        assert!(!f.store().load(&f.uuid).unwrap().execution_managed());
    }
}

#[test]
fn an_already_managed_task_never_uses_a_frozen_legacy_acknowledgement() {
    let f = Fixture::new();
    f.hold(true);
    assert_eq!(f.mode(), Ok(Mode::Legacy));
    let store = f.store();
    let mut task = store.load(&f.uuid).unwrap();
    task.floor().unwrap();
    store.save(&mut task).unwrap();
    drop(store);
    let bytes = f.bytes();
    let result = admit(
        &f.config,
        &NoNetwork,
        "p",
        &f.uuid,
        until(),
        Some(Mode::Legacy),
        |_, _| -> Result<std::result::Result<(), Closed>> { panic!("No reserve") },
    )
    .unwrap();
    assert_eq!(result, Admission::Refused(changed()));
    assert_eq!(f.bytes(), bytes);
}

#[test]
fn matching_modes_keep_the_existing_legacy_or_managed_admission() {
    let f = Fixture::new();
    f.hold(true);
    assert!(matches!(
        admit(
            &f.config,
            &NoNetwork,
            "p",
            &f.uuid,
            until(),
            Some(Mode::Legacy),
            |_, _| -> Result<std::result::Result<(), Closed>> {
                panic!("Legacy is not a reservation")
            }
        )
        .unwrap(),
        Admission::Legacy(_)
    ));
    f.hold(false);
    let result = admit(
        &f.config,
        &NoNetwork,
        "p",
        &f.uuid,
        until(),
        Some(Mode::Managed),
        |_, _| Ok(Ok(42)),
    )
    .unwrap();
    assert!(
        matches!(result, Admission::Reserved { value: 42, .. }),
        "{result:?}"
    );
}

#[test]
fn the_closed_gate_performs_no_planning_access_even_with_no_store_or_time() {
    let f = Fixture::new();
    let mut config = f.config.clone();
    config.dir = f._root.path().join("absent-config");
    config.state = f._root.path().join("absent-state");
    assert_eq!(
        planned(&config, "absent", &f.uuid, Instant::now()).unwrap(),
        None
    );
    assert!(!config.dir.exists());
    assert!(!config.state.exists());
}
