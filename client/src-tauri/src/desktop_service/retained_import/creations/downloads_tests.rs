// G3 downloads (zerus-5wmh.66.5.2.1.3.1): the download folder's committed
// original publishes as its own historical group on the same executor.
use super::super::tests::Fixture;
use super::*;
use std::fs;
use zerus_client_core::download_cache::{complete, reserve, Writer};
const GROUP: Group = Group::Downloads;
const ON: Writer = Writer { evidence: true };

fn folder(f: &Fixture) -> PathBuf {
    f.root.join("downloads")
}
fn plain(f: &Fixture, name: &str, body: &[u8]) {
    let part = reserve(&folder(f)).unwrap();
    fs::write(part.path(), body).unwrap();
    complete(part, name, None, 1).unwrap();
}
fn run(f: &Fixture, explicit: bool) -> Result<(), &'static str> {
    // The journal lives in the creations capture's managed data folder.
    super::tests::source(f);
    let charge = f.report.work.admit(Role::Operations).unwrap();
    f.report.run_group(
        GROUP,
        f.relay.clone(),
        "a".repeat(64),
        Arc::new(AtomicBool::new(false)),
        explicit,
        Arc::new(charge),
    )
}

#[test]
fn the_committed_original_publishes_never_the_live_index() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    plain(&f, "a.txt", b"a");
    let original = fs::read(folder(&f).join(".zerus-files.json")).unwrap();
    assert_eq!(ON.trim(&folder(&f), 0, 2).error, None);
    plain(&f, "b.txt", b"b");
    assert_ne!(
        fs::read(folder(&f).join(".zerus-files.json")).unwrap(),
        original
    );
    assert_eq!(run(&f, false), Ok(()));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["scope"], GROUP.scope());
    assert_eq!(status["phase"], "published");
    let id = status["import_id"].as_str().unwrap().to_owned();
    let record: Value =
        serde_json::from_slice(&fs::read(f.root.join(format!("record-{id}.json"))).unwrap())
            .unwrap();
    assert_eq!(record["evidence"], "staged");
    let canonical = fs::canonicalize(folder(&f)).unwrap();
    let digest = |bytes: &[u8]| {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))
    };
    assert_eq!(
        record["sources"][0]["family"],
        format!(
            "download-index/{}",
            digest(canonical.to_str().unwrap().as_bytes())
        )
    );
    assert_eq!(record["sources"][0]["format"], "download-cache-json-v1");
    assert_eq!(record["sources"][0]["sha256"], digest(&original));
    // A restart looks the original ID up; it never publishes again.
    let attempts = f.attempts();
    assert_eq!(run(&f, false), Ok(()));
    assert_eq!(f.attempts(), attempts);
}

#[test]
fn a_folder_next_never_changed_publishes_nothing() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    plain(&f, "a.txt", b"a");
    assert_eq!(run(&f, false), Ok(()));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["required"], false);
    assert_eq!(status["complete"], true);
    assert!(status["import_id"].is_null());
}

#[test]
fn an_unreadable_record_is_never_published() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    plain(&f, "a.txt", b"a");
    assert_eq!(ON.trim(&folder(&f), 0, 2).error, None);
    // Damage the committed original, keeping its length.
    let slots = folder(&f).join(".zerus-files-preload-v1");
    let slot = fs::read_dir(&slots)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let length = fs::metadata(slot.join("original.bin")).unwrap().len() as usize;
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(slot.join("original.bin"), vec![b' '; length]).unwrap();
    assert_eq!(run(&f, false), Err("downloads_import_capture_unavailable"));
    assert!(f.report.group_status(GROUP)["import_id"].is_null());
}

#[test]
fn an_original_that_changes_during_publication_is_kept_never_committed() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    plain(&f, "a.txt", b"a");
    assert_eq!(ON.trim(&folder(&f), 0, 2).error, None);
    // The original is damaged after it was streamed, before the record.
    let root = folder(&f);
    f.report.group_state(GROUP).before_record = Some(Box::new(move |_| {
        let slots = root.join(".zerus-files-preload-v1");
        let slot = fs::read_dir(&slots)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let length = fs::metadata(slot.join("original.bin")).unwrap().len() as usize;
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(slot.join("original.bin"), vec![b' '; length]).unwrap();
        Box::new(())
    }));
    assert_eq!(
        run(&f, false),
        Err("downloads_import_source_kept_for_review")
    );
    let status = f.report.group_status(GROUP);
    assert_eq!(status["phase"], "quarantined");
    let id = status["import_id"].as_str().unwrap();
    assert!(!f.root.join(format!("committed-{id}.json")).exists());
}

#[test]
fn a_replaced_folder_is_never_adopted_under_the_original_id() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    plain(&f, "a.txt", b"a");
    assert_eq!(ON.trim(&folder(&f), 0, 2).error, None);
    assert_eq!(run(&f, false), Err("downloads_import_handoff_busy"));
    let id = f.report.group_status(GROUP)["import_id"].clone();
    // The folder is replaced: a new object with its own evidence.
    fs::rename(folder(&f), f.root.join("downloads-old")).unwrap();
    plain(&f, "b.txt", b"b");
    assert_eq!(ON.trim(&folder(&f), 0, 2).error, None);
    f.mode("success");
    assert_eq!(run(&f, true), Err("downloads_import_source_changed"));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["import_id"], id);
    assert_eq!(status["phase"], "quarantined");
}

/// migrate-u P1 on #1264 (zerus-5wmh.66.5.2.1.3.1.1): the folder's
/// evidence can appear, and become unreadable, after a run found nothing.
/// The group is then required again and never shown complete.
#[test]
fn a_group_found_not_applicable_is_required_again_once_evidence_exists() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    plain(&f, "a.txt", b"a");
    assert_eq!(run(&f, false), Ok(()));
    let status = f.report.group_status(GROUP);
    assert_eq!(
        (status["required"].clone(), status["complete"].clone()),
        (json!(false), json!(true))
    );
    // Next's first change records the original; then it is damaged.
    assert_eq!(ON.trim(&folder(&f), 0, 2).error, None);
    let slots = folder(&f).join(".zerus-files-preload-v1");
    let slot = fs::read_dir(&slots)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut original = fs::read(slot.join("original.bin")).unwrap();
    original.push(b' ');
    fs::write(slot.join("original.bin"), &original).unwrap();
    assert_eq!(run(&f, true), Err("downloads_import_capture_unavailable"));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["required"], true);
    assert_eq!(status["complete"], false);
}

#[test]
fn a_group_found_not_applicable_publishes_once_evidence_exists() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    plain(&f, "a.txt", b"a");
    assert_eq!(run(&f, false), Ok(()));
    assert_eq!(f.report.group_status(GROUP)["required"], false);
    assert_eq!(ON.trim(&folder(&f), 0, 2).error, None);
    assert_eq!(run(&f, true), Ok(()));
    let status = f.report.group_status(GROUP);
    assert_eq!(status["required"], true);
    assert_eq!(status["phase"], "published");
    assert_eq!(status["complete"], true);
}
