use super::*;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
};

struct Fixture {
    root: PathBuf,
    report: Arc<Report>,
    relay: Arc<zerus_client_core::desktop::relay::Connection>,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("zerus-retained-report-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let cli = root.join("hgs");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/desktop_service/retained_import/fake_cli.py"),
            &cli,
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        let settings_path = root.join("settings.json");
        zerus_client_core::fsx::write_private_durable(
            &settings_path,
            br#"{"version":1,"values":{}}"#,
        )
        .unwrap();
        let qt = root.join("qt.ini");
        zerus_client_core::fsx::write_private_durable(&qt, b"[workspace]\nlanguage=en\n").unwrap();
        fs::write(root.join("mode"), mode).unwrap();
        let report = Report::new(Inputs {
            data: root.clone(),
            settings: Settings::open(settings_path, None),
            qt: Some(QtStore::Ini(qt)),
        });
        let relay = Arc::new(zerus_client_core::desktop::relay::Connection::select(&cli).unwrap());
        Self {
            root,
            report,
            relay,
        }
    }
    fn run(&self, explicit: bool) -> Result<(), &'static str> {
        let charge = self.report.work.admit(Role::Operations).unwrap();
        self.report.run(
            self.relay.clone(),
            "a".repeat(64),
            Arc::new(AtomicBool::new(false)),
            explicit,
            Arc::new(charge),
        )
    }
    fn mode(&self, value: &str) {
        fs::write(self.root.join("mode"), value).unwrap();
    }
    fn attempts(&self) -> usize {
        fs::read_to_string(self.root.join("publications"))
            .unwrap_or_default()
            .lines()
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.report.close();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn native_report_publishes_once_and_later_starts_only_inspect_the_original_id() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    let original = fs::read(f.root.join("qt.ini")).unwrap();
    assert_eq!(f.run(false), Ok(()));
    let first = f.report.status();
    assert_eq!(first["phase"], "published");
    assert_eq!(f.attempts(), 1);
    assert_eq!(fs::read(f.root.join("qt.ini")).unwrap(), original);
    assert!(f
        .report
        .inputs
        .settings
        .set("workspace/language", json!("en"))
        .is_ok());
    assert_eq!(f.run(true), Ok(()));
    assert_eq!(f.attempts(), 1);
    assert_eq!(f.report.status()["import_id"], first["import_id"]);
}
#[test]
fn exact_busy_retains_source_and_uuid_until_an_explicit_same_record_attempt() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    assert_eq!(f.run(false), Err("retained_import_handoff_busy"));
    let first = f.report.status();
    assert_eq!(first["phase"], "not_committed");
    assert!(f
        .report
        .inputs
        .settings
        .set("workspace/language", json!("ru"))
        .is_err());
    f.mode("success");
    assert!(f.run(false).is_err());
    assert_eq!(f.attempts(), 1);
    assert_eq!(f.run(true), Ok(()));
    assert_eq!(f.attempts(), 2);
    assert_eq!(f.report.status()["import_id"], first["import_id"]);
    assert_eq!(f.report.status()["import_record"], first["import_record"]);
}
#[test]
fn lost_partial_or_mismatched_result_never_becomes_a_second_publication() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for mode in ["lost", "partial", "mismatch"] {
        let f = Fixture::new(mode);
        assert!(f.run(false).is_err(), "{mode}");
        assert_eq!(f.report.status()["phase"], "unknown", "{mode}");
        let id = f.report.status()["import_id"].clone();
        f.mode("busy");
        assert!(f.run(true).is_err(), "{mode}");
        assert_eq!(f.attempts(), 1);
        assert_eq!(f.report.status()["import_id"], id);
        assert_eq!(f.report.status()["phase"], "unknown");
    }
}
#[test]
fn exact_committed_lookup_settles_a_lost_result_without_resending_or_new_identity() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("commit_lost");
    assert!(f.run(false).is_err());
    let id = f.report.status()["import_id"].clone();
    assert_eq!(f.report.status()["phase"], "unknown");
    assert_eq!(f.run(true), Ok(()));
    assert_eq!(f.attempts(), 1);
    assert_eq!(f.report.status()["import_id"], id);
}
#[test]
fn changed_source_after_busy_is_quarantined_without_another_commit_record() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    assert!(f.run(false).is_err());
    let id = f.report.status()["import_id"].clone();
    zerus_client_core::fsx::write_private_durable(
        &f.root.join("qt.ini"),
        b"[workspace]\nlanguage=ru\n",
    )
    .unwrap();
    f.mode("success");
    assert_eq!(f.run(true), Err("retained_import_source_kept_for_review"));
    assert_eq!(f.report.status()["phase"], "quarantined");
    assert_eq!(f.report.status()["import_id"], id);
    assert!(!f.root.join("committed.json").exists());
}
#[test]
fn absent_qt_is_explicitly_not_applicable_without_fabricated_report_or_sources() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    fs::remove_file(f.root.join("qt.ini")).unwrap();
    assert_eq!(f.run(false), Ok(()));
    assert_eq!(f.report.status()["required"], false);
    assert_eq!(f.attempts(), 0);
    assert!(!f.root.join("qt-retained-import-v1.json").exists());
}

#[test]
fn a_bad_lazy_source_keeps_original_bytes_without_a_final_manifest() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    let original = b"[attention]\npendingReads={\"not-an-exact-read-key\":{}}\n";
    zerus_client_core::fsx::write_private_durable(&f.root.join("qt.ini"), original).unwrap();
    assert_eq!(f.run(false), Err("retained_import_source_kept_for_review"));
    assert_eq!(f.report.status()["phase"], "quarantined");
    assert!(!f.root.join("committed.json").exists());
    assert_eq!(fs::read(f.root.join("qt.ini")).unwrap(), original);
    let attempts = f.attempts();
    assert!(f.run(true).is_err());
    assert_eq!(f.attempts(), attempts);
}
