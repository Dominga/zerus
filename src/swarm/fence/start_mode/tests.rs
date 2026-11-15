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
        Self::create(false)
    }
    fn legacy() -> Self {
        Self::create(true)
    }
    fn create(legacy: bool) -> Self {
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
        if legacy {
            store.save_legacy_for_test(&mut task).unwrap();
        } else {
            store.save(&mut task).unwrap();
        }
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
    fn adopt(&self) {
        let node = crate::swarm::local_node_id(&self.config).unwrap();
        let store = self.store();
        let mut task = store.load(&self.uuid).unwrap();
        // The old writer's original floor is evidence, never admission.
        task.floor().unwrap();
        store.save_legacy_for_test(&mut task).unwrap();
        let (capture, bytes) = store.prepare_cutover(&self.uuid, &node).unwrap();
        let mut proposal = TaskDoc::from_bytes(&bytes).unwrap();
        proposal
            .seal_cutover(BTreeMap::from([(node, capture.clone())]), BTreeMap::new())
            .unwrap();
        store
            .adopt_cutover(&proposal.snapshot().unwrap(), Some(&capture))
            .unwrap();
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
    let f = Fixture::legacy();
    let bytes = f.bytes();
    assert_eq!(f.mode(), Ok(Mode::Managed));
    assert_eq!(f.bytes(), bytes);
    assert!(!f.store().load(&f.uuid).unwrap().execution_managed());
    f.hold(true);
    assert_eq!(f.mode(), Ok(Mode::Legacy));
    assert_eq!(f.bytes(), bytes);
    f.hold(false);
    f.adopt();
    f.hold(true);
    let floored = f.bytes();
    assert_eq!(f.mode(), Ok(Mode::Managed));
    assert_eq!(f.bytes(), floored);
}

#[test]
fn unknown_scan_or_members_and_damaged_bytes_never_choose_legacy() {
    let f = Fixture::legacy();
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
        let f = Fixture::legacy();
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
        let refused = if planned == Mode::Managed {
            Closed {
                node: None,
                reason: "held",
            }
        } else {
            changed()
        };
        assert_eq!(result, Admission::Refused(refused));
        assert_eq!(f.bytes(), bytes);
        assert!(!f.store().load(&f.uuid).unwrap().execution_managed());
    }
}

#[test]
fn an_already_managed_task_never_uses_a_frozen_legacy_acknowledgement() {
    let f = Fixture::legacy();
    f.hold(true);
    assert_eq!(f.mode(), Ok(Mode::Legacy));
    f.adopt();
    // Isolate the frozen-mode check after adoption; an active hold has its
    // own earlier refusal and must not hide this mismatch control.
    f.hold(false);
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
fn a_legacy_plan_grants_no_admission_and_an_adopted_managed_plan_can_reserve() {
    let f = Fixture::legacy();
    f.hold(true);
    let before = f.bytes();
    assert!(matches!(
        admit(
            &f.config,
            &NoNetwork,
            "p",
            &f.uuid,
            until(),
            Some(Mode::Legacy),
            |_, _| -> Result<std::result::Result<(), Closed>> {
                panic!("A legacy plan grants no reservation")
            }
        )
        .unwrap(),
        Admission::Refused(Closed { reason: "held", .. })
    ));
    assert_eq!(f.bytes(), before);
    f.hold(false);
    f.adopt();
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
fn a_native_birth_seal_plans_managed_even_when_the_project_gate_is_open_or_unknown() {
    let f = Fixture::new();
    f.hold(true);
    let before = f.bytes();
    assert_eq!(f.mode(), Ok(Mode::Managed));
    assert_eq!(
        read(&f.config, "p", &f.uuid, until(), 0).unwrap(),
        Ok(Mode::Managed)
    );
    assert_eq!(f.bytes(), before);
    assert!(!f.store().load(&f.uuid).unwrap().execution_managed());
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
