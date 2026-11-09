//! Per-fixture paths only. No process environment mutation, real installer
//! inventory, or mocked inventory/cache result enters these unit tests.
use super::*;
use std::cell::RefCell;
type Locations = (PathBuf, Vec<PathBuf>);

thread_local! {
    static PATHS: RefCell<BTreeMap<PathBuf, Locations>> = RefCell::default();
}

pub(super) fn paths(config: &Config) -> Option<(PathBuf, Vec<PathBuf>)> {
    PATHS.with(|paths| paths.borrow().get(&config.dir).cloned())
}

pub(super) struct Fixture {
    _home: tempfile::TempDir,
    config_dir: PathBuf,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        PATHS.with(|paths| paths.borrow_mut().remove(&self.config_dir));
    }
}

pub(super) fn fixture() -> (Fixture, Config) {
    let home = tempfile::tempdir().unwrap();
    let config = Config {
        home: home.path().to_str().unwrap().into(),
        dir: home.path().join("config"),
        state: home.path().join("state"),
        selves: vec!["fixture".into()],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: BTreeMap::new(),
        tab: false,
        tab_colors: String::new(),
    };
    crate::platform::private_dir(&config.dir).unwrap();
    crate::platform::private_dir(&config.state).unwrap();
    // LockedStore creates a fresh synthetic catalog/node, never the host's
    // machine identity. Inventory still reads actual private fixture files.
    let mut store = store::LockedStore::load(&config).unwrap();
    store.save().unwrap();
    drop(store);
    let connector = home.path().join("connector/connector.json");
    let units = vec![home.path().join("installer/connector.service")];
    PATHS.with(|paths| {
        assert!(paths
            .borrow_mut()
            .insert(config.dir.clone(), (connector, units))
            .is_none());
    });
    (
        Fixture {
            _home: home,
            config_dir: config.dir.clone(),
            _thread: std::marker::PhantomData,
        },
        config,
    )
}

#[test]
fn independent_fixtures_never_share_connector_or_installer_observations() {
    let (_first, a) = fixture();
    let (_second, b) = fixture();
    assert_ne!(context(&a).unwrap().machine, context(&b).unwrap().machine);
    let (path_a, units_a) = paths(&a).unwrap();
    let (path_b, units_b) = paths(&b).unwrap();
    assert_ne!(path_a, path_b);
    assert_ne!(units_a, units_b);
    assert!(path_a.starts_with(&a.home) && units_a[0].starts_with(&a.home));
    assert!(path_b.starts_with(&b.home) && units_b[0].starts_with(&b.home));
    assert!(observe_absent(&a).is_ok() && observe_absent(&b).is_ok());
    crate::platform::private_dir(units_a[0].parent().unwrap()).unwrap();
    std::fs::write(&units_a[0], b"synthetic installer unit").unwrap();
    assert!(observe_absent(&a).is_err());
    assert!(observe_absent(&b).is_ok());
    std::fs::remove_file(&units_a[0]).unwrap();
    crate::platform::private_dir(path_a.parent().unwrap()).unwrap();
    std::fs::write(&path_a, b"{}").unwrap();
    crate::platform::make_private(&std::fs::File::open(&path_a).unwrap()).unwrap();
    assert!(observe_absent(&a).is_err());
    assert!(observe_absent(&b).is_ok());
}
