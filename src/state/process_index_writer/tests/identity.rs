use super::*;
use std::marker::PhantomData;

pub(super) const MACHINE: &str = "00000000-0000-4000-8000-000000000001";

/// Only the private fixtures opt in. Keep the native provider unavailable too,
/// so every fixture control detects an accidental return to host identity.
pub(super) struct Machine {
    previous: Option<String>,
    _unavailable: Unavailable,
}
impl Machine {
    pub(super) fn new() -> Self {
        Self {
            previous: FIXTURE_MACHINE.with(|value| value.replace(Some(MACHINE.into()))),
            _unavailable: Unavailable::new(),
        }
    }
}
impl Drop for Machine {
    fn drop(&mut self) {
        FIXTURE_MACHINE.with(|value| value.replace(self.previous.take()));
    }
}

/// Simulates the platform refusal without changing host files, global process
/// environment or another test thread. Even a cached native ID must not mask it.
pub(super) struct Unavailable {
    previous: bool,
    _thread: PhantomData<Rc<()>>,
}
impl Unavailable {
    pub(super) fn new() -> Self {
        Self {
            previous: NATIVE_MACHINE_UNAVAILABLE.with(|value| value.replace(true)),
            _thread: PhantomData,
        }
    }
}
impl Drop for Unavailable {
    fn drop(&mut self) {
        NATIVE_MACHINE_UNAVAILABLE.with(|value| value.set(self.previous));
    }
}

#[test]
fn a_private_fixture_publishes_and_bootstraps_without_native_machine_identity() {
    let _unavailable = Unavailable::new();
    let (root, path, mut record) = fixture();
    assert_eq!(scope(root.path()).unwrap().machine, MACHINE);
    write(root.path(), &path, &mut record).unwrap();
    let key = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    let before = open(root.path()).current_source(&key).unwrap().unwrap();
    assert!(before.publication.anchors.len() >= 2);
    finish_background(root.path());
    let mut store = open(root.path());
    assert_eq!(store.current_source(&key).unwrap(), Some(before));
    assert!(!store.stamp().unwrap().complete);
    drop(store);
    drop(root);
    assert!(
        system().is_err(),
        "fixture identity must not escape its owner"
    );
}

#[test]
fn unavailable_native_identity_keeps_canonical_write_and_removal_successful() {
    let _unavailable = Unavailable::new();
    assert!(system().is_err());
    let root = tempfile::tempdir().unwrap();
    let path = root
        .path()
        .join(format!("{}.json", hash(b"claude/private/one")));
    let mut record = record(&boot().unwrap());
    let mut writes = 0;
    let result = write_at(root.path(), &path, &mut record, |record| {
        writes += 1;
        super::super::super::storage::atomic(&path, &format!("{record}\n"))?;
        Ok("original write")
    })
    .unwrap();
    assert_eq!(result, "original write");
    assert_eq!(writes, 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
        record
    );
    assert!(!cache_path(root.path()).unwrap().exists());
    let marker = root.path().join("process-index/coverage-lost.json");
    let gap: Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    assert_eq!(gap["complete"], false);
    assert!(
        gap["reason"]
            .as_str()
            .unwrap()
            .contains("Native machine identity unavailable")
    );
    let mut removals = 0;
    let result = remove_at(root.path(), &path, &record, || {
        removals += 1;
        fs::remove_file(&path).map_err(|error| error.to_string())?;
        Ok("original removal")
    })
    .unwrap();
    assert_eq!(result, "original removal");
    assert_eq!(removals, 1);
    assert!(!path.exists());
    let gap: Value = serde_json::from_slice(&fs::read(marker).unwrap()).unwrap();
    assert_eq!(gap["complete"], false);
}
