//! Opt-in real CLI publisher interoperability, with synthetic sources and
//! isolated HGS state/config. No service, native agent or system manager starts.
#![cfg(unix)]
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use zerus_client_core::migration::{
    sources::{self, Origin},
    transport::{ImportStatus, Publisher},
    Bundle, SourceError,
};
use zerus_desktop_migration::{sha256, Evidence, ImportRecord, SOURCE};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires HGS_IMPORT_TEST_BIN selecting a locally built desktop-import-v1 CLI"]
fn real_publisher_accepts_core_stream_without_replay_or_source_changes() {
    let hgs =
        PathBuf::from(std::env::var_os("HGS_IMPORT_TEST_BIN").expect("select a built test CLI"));
    assert!(hgs.is_absolute());
    let root = std::env::temp_dir().join(format!("zerus-import-native-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let scratch = Scratch(root);
    let config = scratch.0.join("config");
    let state = scratch.0.join("state");
    fs::create_dir(&config).unwrap();
    let source_file = scratch.0.join("synthetic-creations.json");
    let id = "11111111-1111-4111-8111-111111111111";
    let original = serde_json::to_vec(&json!({"schema":1,"future_header":"preserve",
        "operations":{id:{"id":id,"creation_schema":1,"state":"unknown","cmd":"codex",
        "tag":"synthetic","target":"/srv/example","created":1,"launch_id":id,
        "launch_dispatched":true,"private_field":"preserve in snapshot only"}}}))
    .unwrap();
    fs::write(&source_file, &original).unwrap();
    let mtime = fs::metadata(&source_file).unwrap().modified().unwrap();
    let publisher = Publisher::probe(
        &hgs,
        &[
            ("HGS_CONFIG_DIR", config.to_str().unwrap()),
            ("HGS_STATE_DIR", state.to_str().unwrap()),
            ("HGS_SELF", "example"),
            ("HGS_PEERS", ""),
        ],
    )
    .unwrap();
    assert!(!state.exists(), "capability/status probes create no state");
    let make_bundle = || {
        let path = source_file.clone();
        let source = sources::creations(Origin::Qt, &original)
            .unwrap()
            .recheck(move || {
                let bytes = fs::read(path).map_err(|_| SourceError::SourceUnreadable)?;
                Ok(sha256(&bytes))
            });
        Bundle::new(
            SOURCE,
            false,
            publisher.configuration().into(),
            id.into(),
            1,
            vec![source],
            1 << 20,
        )
        .unwrap()
    };
    let receipt = publisher.publish(make_bundle()).unwrap();
    assert_eq!(receipt.evidence, Evidence::Staged);
    assert_eq!((receipt.snapshots, receipt.pages), (2, 2));
    let desktop = state.join("desktop");
    let target = desktop.join("imports").join(id);
    let manifest = fs::read(target.join("record-v1.json")).unwrap();
    assert_eq!(receipt.import_record.sha256, sha256(&manifest));
    let record = ImportRecord::parse(&manifest).unwrap();
    let mut private_found = false;
    for reference in &record.pages {
        let bytes = fs::read(desktop.join(record.page_relative_path(reference).unwrap())).unwrap();
        let page = record.snapshot_page(reference, &bytes).unwrap();
        let bytes = fs::read(desktop.join(page.snapshot.relative_path(id).unwrap())).unwrap();
        page.snapshot.verify(&bytes).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        private_found |= value["content"]["private_field"] == "preserve in snapshot only";
    }
    assert!(private_found);
    assert!(!String::from_utf8_lossy(&manifest).contains("private_field"));
    assert_eq!(
        publisher.status(id),
        Ok(ImportStatus::Present {
            evidence: Evidence::Staged,
            pages: 2,
            import_record: receipt.import_record
        })
    );
    // An explicit duplicate request is refused by G1; no automatic retry exists.
    assert!(publisher.publish(make_bundle()).is_err());
    assert_eq!(fs::read(target.join("record-v1.json")).unwrap(), manifest);
    assert_eq!(fs::read(&source_file).unwrap(), original);
    assert_eq!(
        fs::metadata(&source_file).unwrap().modified().unwrap(),
        mtime
    );
    for leaf in [
        "owner.lock",
        "state.sqlite3",
        "handoff-v1.json",
        "service.sock",
    ] {
        assert!(!desktop.join(leaf).exists(), "{leaf}");
    }
}
