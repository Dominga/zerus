use super::*;
use crate::migration::preload::{Capture, Prepared};
use std::fs;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zerus-creation-record-{}", uuid::Uuid::new_v4()));
        drop(crate::fsx::preload::Parents::open(&path, true).unwrap());
        crate::fsx::write_private_durable(
            &path.join("creations.json"),
            br#"{"schema":1,"operations":{},"unknown":"kept"}"#,
        )
        .unwrap();
        Self(path)
    }
    fn reference(&self) -> Reference {
        let capture = Capture::new(self.0.clone());
        let Prepared::Snapshot(s) = capture.prepare().unwrap() else {
            panic!("missing")
        };
        s.reference().unwrap()
    }
    fn journal(&self) -> Journal {
        Journal::new(&self.0, &"a".repeat(64)).unwrap()
    }
    fn initial(&self) -> Stored {
        self.journal()
            .save(
                Record::new("a".repeat(64), self.reference(), 1).unwrap(),
                None,
            )
            .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn update(j: &Journal, old: Stored, change: impl FnOnce(&mut Record)) -> Stored {
    let mut r = old.record;
    change(&mut r);
    j.save(r, Some(&old.sha256)).unwrap()
}
fn receipt(r: &Record) -> StagedReceipt {
    StagedReceipt {
        abi: "desktop-import-v1".into(),
        published: true,
        evidence: Evidence::Staged,
        configuration: r.configuration.clone(),
        import_id: r.import_id.clone(),
        import_record: zerus_desktop_migration::ImportRecordRef {
            version: 1,
            sha256: "f".repeat(64),
        },
        bytes: 100,
        snapshots: 2,
        pages: 1,
    }
}
#[test]
fn original_capture_id_and_configuration_bind_once_without_qt_fields() {
    let s = Scratch::new();
    let initial = s.initial();
    assert_eq!(initial.record.import_id, s.reference().capture_id());
    let bytes = fs::read(s.0.join(FILE)).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.get("qt_sha256").is_none() && json.get("settings_sha256").is_none());
    let other = Journal::new(&s.0, &"b".repeat(64)).unwrap();
    assert!(other.load().is_err());
    assert_eq!(fs::read(s.0.join(FILE)).unwrap(), bytes);
    let mut changed = initial.record.clone();
    changed.import_id = uuid::Uuid::new_v4().to_string();
    assert!(s.journal().save(changed, Some(&initial.sha256)).is_err());
    let mut changed = initial.record;
    changed.created_at = 2;
    assert!(s.journal().save(changed, Some(&initial.sha256)).is_err());
}
#[test]
fn only_exact_negative_allows_the_same_record_next_attempt() {
    let s = Scratch::new();
    let j = s.journal();
    let initial = s.initial();
    let attempted = update(&j, initial, |r| {
        r.phase = Phase::Attempted;
        r.attempt = 1;
    });
    let original = update(&j, attempted, |r| r.expected = Some(receipt(r)));
    let negative = update(&j, original.clone(), |r| r.phase = Phase::NotCommitted);
    let next = update(&j, negative, |r| {
        r.phase = Phase::Attempted;
        r.attempt += 1;
    });
    assert_eq!(next.record.source, original.record.source);
    assert_eq!(next.record.expected, original.record.expected);
    let unknown = update(&j, next, |r| r.phase = Phase::Unknown);
    for phase in [Phase::Reserved, Phase::Attempted, Phase::NotCommitted] {
        let mut replay = unknown.record.clone();
        replay.phase = phase;
        replay.attempt += 1;
        assert!(j.save(replay, Some(&unknown.sha256)).is_err());
    }
    let settled = update(&j, unknown, |r| r.phase = Phase::Published);
    let mut wrong = settled.record.clone();
    wrong.expected.as_mut().unwrap().import_record.sha256 = "e".repeat(64);
    assert!(j.save(wrong, Some(&settled.sha256)).is_err());
}
#[test]
fn stale_compare_partial_future_and_replaced_snapshot_do_not_reset_identity() {
    let s = Scratch::new();
    let j = s.journal();
    let initial = s.initial();
    let stale = initial.clone();
    let attempted = update(&j, initial, |r| {
        r.phase = Phase::Attempted;
        r.attempt = 1;
    });
    assert!(j.save(stale.record, Some(&stale.sha256)).is_err());
    let bytes = fs::read(s.0.join(FILE)).unwrap();
    let mut changed = attempted.record.clone();
    changed.source = Scratch::new().reference();
    assert!(j.save(changed, Some(&attempted.sha256)).is_err());
    assert_eq!(fs::read(s.0.join(FILE)).unwrap(), bytes);
    crate::fsx::write_private_durable(&s.0.join(FILE), b"{partial").unwrap();
    assert!(j.load().is_err());
    assert!(j.save(attempted.record, None).is_err());
    assert_eq!(fs::read(s.0.join(FILE)).unwrap(), b"{partial");
}
#[test]
fn unknown_without_an_expected_record_cannot_invent_one_later() {
    let s = Scratch::new();
    let j = s.journal();
    let attempted = update(&j, s.initial(), |r| {
        r.phase = Phase::Attempted;
        r.attempt = 1;
    });
    let unknown = update(&j, attempted, |r| r.phase = Phase::Unknown);
    for phase in [Phase::Unknown, Phase::Published] {
        let mut fabricated = unknown.record.clone();
        fabricated.phase = phase;
        fabricated.expected = Some(receipt(&fabricated));
        assert!(j.save(fabricated, Some(&unknown.sha256)).is_err());
    }
    assert_eq!(j.load().unwrap().unwrap().sha256, unknown.sha256);
    let mut unsent = unknown.record;
    unsent.phase = Phase::BeforeRecord;
    assert!(j.save(unsent, Some(&unknown.sha256)).is_err());
}
#[test]
fn a_confirmed_before_record_failure_keeps_its_identity_without_inventing_a_receipt() {
    let s = Scratch::new();
    let j = s.journal();
    let attempted = update(&j, s.initial(), |r| {
        r.phase = Phase::Attempted;
        r.attempt = 1;
    });
    let before = update(&j, attempted, |r| r.phase = Phase::BeforeRecord);
    assert!(before.record.expected.is_none());
    assert_eq!(j.load().unwrap().unwrap().record, before.record);
    let original = before.record.clone();
    let retried = update(&j, before, |r| {
        r.phase = Phase::Attempted;
        r.attempt += 1;
    });
    assert_eq!(retried.record.import_id, original.import_id);
    assert_eq!(retried.record.source, original.source);
    assert_eq!(retried.record.configuration, original.configuration);
    assert_eq!(retried.record.created_at, original.created_at);
    assert!(retried.record.expected.is_none());
}
#[cfg(unix)]
#[test]
fn group_writable_data_directory_does_not_relax_the_record_leaf() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let s = Scratch::new();
    fs::set_permissions(&s.0, fs::Permissions::from_mode(0o775)).unwrap();
    let saved = s.initial();
    assert_eq!(s.journal().load().unwrap().unwrap().sha256, saved.sha256);
    assert_eq!(fs::metadata(s.0.join(FILE)).unwrap().mode() & 0o777, 0o600);
    fs::hard_link(s.0.join(FILE), s.0.join("alias")).unwrap();
    assert!(s.journal().load().is_err());
}

fn initial_v2(s: &Scratch) -> Stored {
    s.journal()
        .save(
            Record::new_v2("a".repeat(64), s.reference(), 1).unwrap(),
            None,
        )
        .unwrap()
}
#[test]
fn v2_gate_counter_and_recovery_fence_preserve_the_original_payload() {
    let s = Scratch::new();
    let j = s.journal();
    let attempted = update(&j, initial_v2(&s), |r| {
        r.phase = Phase::Attempted;
        r.attempt = 1;
    });
    assert!(attempted.record.preparation_recoverable());
    let fenced = j.fence_preparation(&attempted).unwrap();
    assert_ne!(fenced.sha256, attempted.sha256);
    assert_eq!(fenced.record.attempt, 1);
    assert_eq!(fenced.record.recorded_attempt, Some(0));
    assert_eq!(fenced.record.source, attempted.record.source);
    let old_gate = attempted
        .record
        .with_expected(&receipt(&attempted.record))
        .unwrap();
    assert!(j.save(old_gate, Some(&attempted.sha256)).is_err());
    let next = update(&j, fenced, |r| {
        r.phase = Phase::Attempted;
        r.attempt += 1;
    });
    let gate = next.record.with_expected(&receipt(&next.record)).unwrap();
    let gate = j.save(gate, Some(&next.sha256)).unwrap();
    assert_eq!(gate.record.recorded_attempt, Some(2));
    assert!(!gate.record.preparation_recoverable());
    assert!(j.fence_preparation(&next).is_err());
    assert!(j.fence_preparation(&gate).is_err());
    let unknown = update(&j, gate, |r| r.phase = Phase::Unknown);
    assert!(j.fence_preparation(&unknown).is_err());
}
#[test]
fn v2_retry_keeps_an_old_receipt_but_must_record_the_new_attempt_gate() {
    let s = Scratch::new();
    let j = s.journal();
    let first = update(&j, initial_v2(&s), |r| {
        r.phase = Phase::Attempted;
        r.attempt = 1;
    });
    let expected = receipt(&first.record);
    let gate = j
        .save(
            first.record.with_expected(&expected).unwrap(),
            Some(&first.sha256),
        )
        .unwrap();
    let negative = update(&j, gate, |r| r.phase = Phase::NotCommitted);
    let next = update(&j, negative, |r| {
        r.phase = Phase::Attempted;
        r.attempt += 1;
    });
    assert_eq!(next.record.expected, Some(expected));
    assert_eq!(next.record.recorded_attempt, Some(1));
    assert!(next.record.preparation_recoverable());
    let mut bad = next.record.clone();
    bad.recorded_attempt = Some(0);
    assert!(!bad.valid());
    assert!(j.save(bad, Some(&next.sha256)).is_err());
    let unknown = update(&j, next, |r| r.phase = Phase::Unknown);
    assert!(!unknown.record.preparation_recoverable());
    assert!(j.fence_preparation(&unknown).is_err());
}
#[test]
fn only_a_legacy_attempted_record_without_a_receipt_can_upgrade_for_recovery() {
    for has_receipt in [false, true] {
        let s = Scratch::new();
        let j = s.journal();
        let attempted = update(&j, s.initial(), |r| {
            r.phase = Phase::Attempted;
            r.attempt = 1;
        });
        let current = if has_receipt {
            j.save(
                attempted
                    .record
                    .with_expected(&receipt(&attempted.record))
                    .unwrap(),
                Some(&attempted.sha256),
            )
            .unwrap()
        } else {
            attempted
        };
        if has_receipt {
            assert!(j.fence_preparation(&current).is_err());
        } else {
            let upgraded = j.fence_preparation(&current).unwrap();
            assert_eq!(upgraded.record.version, 2);
            assert_eq!(upgraded.record.recorded_attempt, Some(0));
            assert_eq!(upgraded.record.import_id, current.record.import_id);
            assert_eq!(upgraded.record.source, current.record.source);
            assert!(j.save(current.record, Some(&upgraded.sha256)).is_err());
        }
    }
}
#[test]
fn rollback_reader_refuses_v2_and_the_bounded_loader_never_defaults_a_missing_counter() {
    // Frozen PR1131 v1 wire shape: its deny_unknown_fields reader must refuse
    // the extra v2 field before it can interpret a new state as an old one.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Legacy {
        version: u32,
        scope: String,
        configuration: String,
        import_id: String,
        created_at: u64,
        source: Reference,
        attempt: u32,
        phase: Phase,
        expected: Option<StagedReceipt>,
    }
    let s = Scratch::new();
    let record = Record::new("a".repeat(64), s.reference(), 1).unwrap();
    assert!(serde_json::from_value::<Legacy>(serde_json::to_value(&record).unwrap()).is_ok());
    let v2 = initial_v2(&s);
    let mut value = serde_json::to_value(&v2.record).unwrap();
    assert!(serde_json::from_value::<Legacy>(value.clone()).is_err());
    value.as_object_mut().unwrap().remove("recorded_attempt");
    crate::fsx::write_private_durable(&s.0.join(FILE), &serde_json::to_vec(&value).unwrap())
        .unwrap();
    assert!(s.journal().load().is_err());
    let oversized = vec![b' '; BYTES + 1];
    crate::fsx::write_private_durable(&s.0.join(FILE), &oversized).unwrap();
    assert!(s.journal().load().is_err());
    assert_eq!(fs::read(s.0.join(FILE)).unwrap(), oversized);
}
// A recovery's refused fence and the original's final write may overlap: a
// save waits for the other run's bounded compare/write instead of failing,
// but never writes without the lock.
#[test]
fn a_concurrent_save_waits_for_the_journal_lock_and_never_writes_without_it() {
    let s = Scratch::new();
    let j = s.journal();
    let initial = s.initial();
    let path = s.0.join("creations-retained-import-v1.lock");
    let held = crate::fsx::preload::Lock::take_managed(&path).unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
    });
    let started = Instant::now();
    let attempted = update(&j, initial, |r| {
        r.phase = Phase::Attempted;
        r.attempt = 1;
    });
    assert!(started.elapsed() >= Duration::from_millis(250));
    release.join().unwrap();
    assert_eq!(j.load().unwrap().unwrap().sha256, attempted.sha256);
    // A holder that outlasts the bounded wait leaves the record unchanged.
    let held = crate::fsx::preload::Lock::take_managed(&path).unwrap();
    let mut next = attempted.record.clone();
    next.phase = Phase::Unknown;
    let started = Instant::now();
    assert!(matches!(
        j.save(next, Some(&attempted.sha256)),
        Err(SourceError::SourceUnreadable)
    ));
    assert!(started.elapsed() >= LOCK_WAIT);
    drop(held);
    assert_eq!(j.load().unwrap().unwrap().sha256, attempted.sha256);
}
