//! Synthetic selected CLI and isolated source/state roots only.
#![cfg(unix)]
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use zerus_client_core::migration::{
    transport::{AdmissionError, ImportStatus, PublishError, Publisher},
    Bundle, SourceError, SourceStream,
};
use zerus_desktop_migration::{sha256, SourceRecord, SOURCE};

static COUNT: AtomicUsize = AtomicUsize::new(0);
const ID: &str = "11111111-1111-4111-8111-111111111111";

struct Fake {
    root: PathBuf,
    hgs: PathBuf,
}
impl Fake {
    fn new(options: Value) -> Self {
        let root = std::env::temp_dir().join(format!(
            "zerus-import-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let hgs = root.join("hgs");
        std::os::unix::fs::symlink(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_hgs_import.py"),
            &hgs,
        )
        .unwrap();
        let fake = Self { root, hgs };
        fake.options(options);
        fake
    }
    fn options(&self, options: Value) {
        fs::write(self.root.join("options.json"), options.to_string()).unwrap();
    }
    fn publisher(&self) -> Result<Publisher, AdmissionError> {
        let root = self.root.to_str().unwrap();
        Publisher::probe(
            &self.hgs,
            &[
                ("TEST_IMPORT_ROOT", root),
                ("HGS_CONFIG_DIR", root),
                ("HGS_STATE_DIR", root),
                ("HGS_SESSION", "must-be-removed"),
                ("HGS_RUN_ID", "must-be-removed"),
            ],
        )
    }
    fn calls(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn source(reads: Arc<AtomicUsize>, changed: bool, recheck: bool) -> SourceStream {
    let stream = SourceStream::new(
        SourceRecord {
            family: "synthetic-source".into(),
            schema: 1,
            format: "synthetic-json".into(),
            sha256: sha256(b"original source"),
            build: None,
        },
        (0..3).map(move |_| {
            reads.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"private":"retained draft"}))
        }),
    );
    if recheck {
        stream.recheck(move || {
            Ok(sha256(if changed {
                b"changed"
            } else {
                b"original source"
            }))
        })
    } else {
        stream
    }
}
fn bundle(sources: Vec<SourceStream>) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        sources,
        1 << 20,
    )
    .unwrap()
}

#[test]
fn publishes_exact_framing_and_digest_using_one_configuration_without_authority() {
    let fake = Fake::new(json!({}));
    let publisher = fake.publisher().unwrap();
    assert_eq!(publisher.configuration(), "a".repeat(64));
    let reads = Arc::new(AtomicUsize::new(0));
    let input = bundle(vec![source(reads.clone(), false, true)]);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let receipt = publisher.publish(input).unwrap();
    assert_eq!((receipt.snapshots, receipt.pages), (1, 3));
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    assert_eq!(
        publisher.status(ID),
        Ok(ImportStatus::Present {
            evidence: receipt.evidence,
            pages: receipt.pages,
            import_record: receipt.import_record
        })
    );
    let calls = fake.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c["args"] == json!(["desktop", "import-evidence", "--json"]))
            .count(),
        1
    );
    for call in calls {
        assert_eq!(call["config"], fake.root.to_str().unwrap());
        assert_eq!(call["config"], call["state"]);
        assert_eq!(call["session"], Value::Null);
        assert_eq!(call["run"], Value::Null);
    }
    assert!(!fake.root.join("handoff-v1.json").exists());
    assert!(!fake.root.join("state.sqlite3").exists());
}

#[test]
fn old_cli_and_changed_configuration_never_reach_publisher_or_decode_sources() {
    for marker in [
        "old help",
        "prefix Desktop import ABI: desktop-import-v1 suffix",
        "Desktop import ABI: desktop-import-v2",
    ] {
        let fake = Fake::new(json!({"help":marker}));
        assert_eq!(fake.publisher().err(), Some(AdmissionError::Unsupported));
        assert_eq!(fake.calls().len(), 1);
    }
    let fake = Fake::new(json!({}));
    let publisher = fake.publisher().unwrap();
    fake.options(json!({"configuration":"b".repeat(64)}));
    let reads = Arc::new(AtomicUsize::new(0));
    let error = publisher
        .publish(bundle(vec![source(reads.clone(), false, true)]))
        .unwrap_err();
    assert_eq!(
        error.reason,
        PublishError::Admission(AdmissionError::ConfigurationChanged)
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fake.calls().len(), 4);
}

#[test]
fn unchecked_consumed_overbudget_or_wrong_configuration_bundles_refuse_before_spawn() {
    let fake = Fake::new(json!({}));
    let publisher = fake.publisher().unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let unchecked = bundle(vec![source(reads.clone(), false, false)]);
    let mut consumed = bundle(Vec::new());
    consumed.next().unwrap().unwrap();
    let wrong = Bundle::new(
        SOURCE,
        false,
        "b".repeat(64),
        ID.into(),
        1,
        Vec::new(),
        1000,
    )
    .unwrap();
    let big = Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        Vec::new(),
        (64 << 20) + 1,
    )
    .unwrap();
    for input in [unchecked, consumed, wrong, big] {
        assert_eq!(
            publisher.publish(input).unwrap_err().reason,
            PublishError::InvalidBundle
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fake.calls().len(), 2);
}

#[test]
fn changed_source_never_sends_a_final_record_or_retries() {
    let fake = Fake::new(json!({}));
    let publisher = fake.publisher().unwrap();
    let error = publisher
        .publish(bundle(vec![source(Arc::default(), true, true)]))
        .unwrap_err();
    assert_eq!(error.import_id, ID);
    assert_eq!(
        error.reason,
        PublishError::Source(SourceError::SourceChanged)
    );
    assert!(!fake.root.join(format!("{ID}.json")).exists());
    assert_eq!(fake.calls().len(), 5);
}

#[test]
fn mismatched_or_unknown_reply_is_unconfirmed_and_preserves_the_uuid() {
    for patch in [
        json!({"configuration":"b".repeat(64)}),
        json!({"import_id":"22222222-2222-4222-8222-222222222222"}),
        json!({"import_record":{"version":1,"sha256":"0".repeat(64)}}),
        json!({"bytes":0}),
        json!({"pages":99}),
        json!({"snapshots":99}),
        json!({"evidence":"drained"}),
        json!({"published":false}),
        json!({"unknown":true}),
    ] {
        let fake = Fake::new(json!({"reply_patch":patch}));
        let publisher = fake.publisher().unwrap();
        let error = publisher.publish(bundle(Vec::new())).unwrap_err();
        assert_eq!(error.reason, PublishError::Unconfirmed);
        assert_eq!(error.import_id, ID);
        assert_eq!(
            fake.calls().len(),
            5,
            "no automatic status or publication retry"
        );
    }
}

#[test]
fn lost_reply_can_be_inspected_but_absence_does_not_trigger_another_import() {
    let fake = Fake::new(json!({"lost":true}));
    let publisher = fake.publisher().unwrap();
    assert_eq!(
        publisher.publish(bundle(Vec::new())).unwrap_err().reason,
        PublishError::Unconfirmed
    );
    assert!(matches!(
        publisher.status(ID),
        Ok(ImportStatus::Present { .. })
    ));
    fake.options(json!({"absent":true}));
    assert_eq!(publisher.status(ID), Ok(ImportStatus::Absent));
    let calls = fake.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c["args"] == json!(["desktop", "import-evidence", "--json"]))
            .count(),
        1
    );
    assert_eq!(
        publisher.status("not-an-import-id"),
        Err(AdmissionError::InvalidImportId)
    );
    assert_eq!(fake.calls(), calls, "invalid ID runs no command");
}

#[test]
fn malformed_status_and_oversized_stderr_never_report_success() {
    let fake = Fake::new(json!({"status_patch":{"record_present":true}}));
    let publisher = fake.publisher().unwrap();
    assert_eq!(publisher.status(ID), Err(AdmissionError::StatusUnavailable));
    fake.options(json!({"noisy":true}));
    let began = Instant::now();
    assert_eq!(
        publisher.publish(bundle(Vec::new())).unwrap_err().reason,
        PublishError::Unconfirmed
    );
    assert!(
        began.elapsed() < Duration::from_secs(8),
        "child ended for stderr overflow"
    );
    assert!(!fake.root.join(format!("{ID}.json")).exists());
}
