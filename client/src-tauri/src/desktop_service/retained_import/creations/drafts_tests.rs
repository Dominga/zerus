// G3 drafts (zerus-5wmh.66.5.2.1.2): the drafts capture's committed
// originals publish as their own historical group on the same executor.
use super::super::tests::Fixture;
use super::*;
use std::fs;
const GROUP: Group = Group::Drafts;

fn store(f: &Fixture) -> PathBuf {
    f.root.join(zerus_client_core::drafts::dir(false))
}
fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
/// A valid draft of `key` in the fixture's Next draft store.
fn draft(f: &Fixture, key: &str, text: &str) {
    let folder = store(f).join(digest(key.as_bytes()));
    fs::create_dir_all(&folder).unwrap();
    let manifest = json!({"version": 1, "key": key, "text": text});
    fs::write(folder.join("draft.json"), manifest.to_string()).unwrap();
}
/// The draft store's first job: the capture's one attempt.
fn capture(f: &Fixture) -> &'static str {
    f.report.inputs.drafts.prepare();
    f.report.inputs.drafts.status().code
}
fn run(f: &Fixture, explicit: bool) -> Result<(), &'static str> {
    // The journal lives in the creations capture's managed data folder.
    super::tests::source(f);
    let charge = super::super::tests::admit_drained(&f.report);
    f.report.run_group(
        GROUP,
        f.relay.clone(),
        "a".repeat(64),
        Arc::new(AtomicBool::new(false)),
        explicit,
        Arc::new(charge),
    )
}
fn record(f: &Fixture, id: &str) -> Value {
    serde_json::from_slice(&fs::read(f.root.join(format!("record-{id}.json"))).unwrap()).unwrap()
}

#[test]
fn the_committed_drafts_publish_as_their_own_group() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    draft(&f, "codex/sample/a", "before the first job");
    assert_eq!(capture(&f), "draft_capture_captured");
    // Typing after the capture never reaches the report.
    draft(&f, "codex/sample/a", "typed later");
    assert_eq!(run(&f, false), Ok(()));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["scope"], "next-drafts-preload-v1");
    assert_eq!(status["phase"], "published");
    assert_eq!(status["required"], true);
    assert_eq!(status["complete"], true);
    assert_eq!(status["partial"], false);
    let id = status["import_id"].as_str().unwrap().to_owned();
    assert_eq!(
        Some(id.clone()),
        f.report.inputs.drafts.status().capture_id,
        "the record is the capture's own ID"
    );
    let record = record(&f, &id);
    assert_eq!(record["evidence"], "staged");
    let families: Vec<&str> = record["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| source["family"].as_str().unwrap())
        .collect();
    assert_eq!(
        families,
        ["next-drafts-preload", "next-drafts-preload-provenance"]
    );
    // A restart looks the original ID up; it never publishes again.
    let attempts = f.attempts();
    assert_eq!(run(&f, false), Ok(()));
    assert_eq!(f.attempts(), attempts);
}

#[test]
fn the_report_waits_for_the_draft_store_first_job() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    super::tests::source(&f);
    draft(&f, "codex/sample/a", "kept");
    assert_eq!(
        f.report.inputs.drafts.status().code,
        "draft_capture_waiting"
    );
    assert!(!f.report.source_ready(GROUP));
    capture(&f);
    assert!(f.report.source_ready(GROUP));
}

#[test]
fn no_draft_anywhere_is_not_applicable() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    assert_eq!(capture(&f), "draft_capture_not_applicable");
    assert_eq!(run(&f, false), Ok(()));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["required"], false);
    assert_eq!(status["complete"], true);
    assert!(status["import_id"].is_null());
    assert_eq!(f.attempts(), 0);
}

/// Not captured, permanently: unavailable and incomplete, even after an
/// explicit Retry, and nothing is published.
#[test]
fn a_not_captured_outcome_stays_unavailable_never_complete() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    draft(&f, "codex/sample/a", "kept");
    // One manifest over the 1 MiB manifest cap.
    let big = store(&f).join(digest(b"codex/sample/big"));
    fs::create_dir_all(&big).unwrap();
    fs::write(big.join("draft.json"), vec![b' '; (1 << 20) + 1]).unwrap();
    assert_eq!(capture(&f), "draft_capture_too_large");
    // The outcome is final, so the report runs and shows it.
    super::tests::source(&f);
    assert!(f.report.source_ready(GROUP));
    for explicit in [false, true] {
        assert_eq!(run(&f, explicit), Err("drafts_import_capture_unavailable"));
        let status = f.report.group_status(GROUP);
        assert_eq!(status["required"], true);
        assert_eq!(status["complete"], false);
        assert!(status["import_id"].is_null());
    }
    assert_eq!(f.attempts(), 0);
}

/// Next's store written by an earlier uncaptured run: the report publishes
/// what was recorded and stays incomplete, never complete.
#[test]
fn an_earlier_run_publishes_and_stays_incomplete() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    draft(&f, "codex/sample/a", "typed after an import");
    fs::write(
        store(&f).join(zerus_client_core::drafts::IMPORT_MARKER),
        b"",
    )
    .unwrap();
    assert_eq!(capture(&f), "draft_capture_next_earlier_run");
    assert_eq!(run(&f, false), Ok(()));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["phase"], "published");
    assert_eq!(status["required"], true);
    assert_eq!(status["partial"], true);
    assert_eq!(status["complete"], false);
    let record = record(&f, status["import_id"].as_str().unwrap());
    assert_eq!(
        record["sources"][0]["family"],
        "next-drafts-preload-provenance"
    );
    assert_eq!(record["sources"].as_array().unwrap().len(), 1);
    // Still incomplete after a restart: the record itself says so.
    assert_eq!(run(&f, true), Ok(()));
    assert_eq!(f.report.group_status(GROUP)["complete"], false);
}

/// The commit gate checks the receipt's exact bytes: one appended byte
/// before the record keeps the source for review and commits nothing.
#[test]
fn a_receipt_changed_before_the_record_is_kept_never_committed() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    draft(&f, "codex/sample/a", "kept");
    assert_eq!(capture(&f), "draft_capture_captured");
    let receipt = f.root.join("drafts-preload-v1/receipt.json");
    f.report.group_state(GROUP).before_record = Some(Box::new(move |_| {
        let mut bytes = fs::read(&receipt).unwrap();
        bytes.push(b'\n');
        fs::write(&receipt, bytes).unwrap();
        Box::new(())
    }));
    assert_eq!(run(&f, false), Err("drafts_import_source_kept_for_review"));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["phase"], "quarantined");
    assert_eq!(status["complete"], false);
    let id = status["import_id"].as_str().unwrap();
    assert!(!f.root.join(format!("committed-{id}.json")).exists());
}
