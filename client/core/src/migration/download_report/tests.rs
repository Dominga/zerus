// The download folder's committed original as a report source, on
// synthetic folders under the system temporary folder.
use super::*;
use crate::download_cache::{complete, reserve, Writer};
use crate::migration::creation_report::{Group, Record, Source};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ON: Writer = Writer { evidence: true };

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zerus-download-report-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn plain(&self, name: &str, body: &[u8]) {
        let part = reserve(&self.0).unwrap();
        fs::write(part.path(), body).unwrap();
        complete(part, name, None, 1).unwrap();
    }
    fn index(&self) -> Vec<u8> {
        fs::read(self.0.join(".zerus-files.json")).unwrap()
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_committed_original_is_the_source_and_the_live_index_is_not() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a");
    let original = folder.index();
    assert!(retained(&folder.0).unwrap().is_none(), "no receipt yet");
    assert_eq!(ON.trim(&folder.0, 0, 2).error, None);
    folder.plain("b.txt", b"b");
    let snapshot = retained(&folder.0).unwrap().unwrap();
    assert_eq!(snapshot.index, original, "never today's live index");
    let binding = snapshot.reference().unwrap();
    assert!(binding.valid());
    assert_eq!(binding.index_sha256, sha256(&original));
    let sources = snapshot.report_sources().unwrap();
    assert_eq!(sources.len(), 1);
}

#[test]
fn a_changed_receipt_is_another_source() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a");
    assert_eq!(ON.trim(&folder.0, 0, 2).error, None);
    let snapshot = retained(&folder.0).unwrap().unwrap();
    // The folder is replaced by another object at the same path.
    let moved = folder.0.with_extension("moved");
    fs::rename(&folder.0, &moved).unwrap();
    fs::create_dir(&folder.0).unwrap();
    assert_eq!(snapshot.reference(), Err(SourceError::SourceChanged));
    let _ = fs::remove_dir_all(&moved);
}

#[test]
fn a_folder_without_an_original_index_publishes_nothing() {
    let folder = Folder::new();
    let part = reserve(&folder.0).unwrap();
    fs::write(part.path(), b"first").unwrap();
    ON.complete(part, "first.txt", None, 1).unwrap();
    assert!(retained(&folder.0).unwrap().is_none());
    let missing = folder.0.join("never-created");
    assert!(retained(&missing).unwrap().is_none());
}

#[test]
fn download_records_keep_their_own_scope_and_preload_bytes_are_unchanged() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a");
    assert_eq!(ON.trim(&folder.0, 0, 2).error, None);
    let binding = retained(&folder.0).unwrap().unwrap().reference().unwrap();
    let configuration = "c".repeat(64);
    let record =
        Record::new_group(Group::Downloads, configuration.clone(), binding.clone(), 1).unwrap();
    assert_eq!(record.import_id, binding.capture_id);
    // Round trip through its serialized form keeps the download binding.
    let bytes = serde_json::to_vec(&record).unwrap();
    let again: Record = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(again.source, Source::Downloads(binding.clone()));
    // A download binding never fits another group's scope.
    assert!(Record::new_group(Group::Creations, configuration, binding, 1).is_err());
}

/// The folder's only evidence slot.
fn slot(f: &Folder) -> PathBuf {
    fs::read_dir(f.0.join(evidence::DIR))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
}

/// An archive that cannot be read is unreadable, never "no source": the
/// report must not find the group not applicable.
#[cfg(unix)]
#[test]
fn an_unreadable_archive_is_never_no_source() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let f = Folder::new();
    f.plain("old.txt", b"old cache");
    assert_eq!(ON.trim(&f.0, 0, 2).error, None);
    let dir = f.0.join(evidence::DIR);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
    let shown = evidence::status(&f.0);
    let held = retained(&f.0);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(shown, Evidence::Unreadable);
    assert!(matches!(held, Err(SourceError::SourceUnreadable)));
}

/// The seed is checked before the receipt is looked at: an invalid seed
/// without a receipt is unreadable, as the shown status says.
#[test]
fn an_invalid_seed_without_a_receipt_is_unreadable_never_no_source() {
    let f = Folder::new();
    f.plain("old.txt", b"old cache");
    assert_eq!(ON.trim(&f.0, 0, 2).error, None);
    let slot = slot(&f);
    fs::remove_file(slot.join("receipt.json")).unwrap();
    let seed = slot.join("identity.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&seed).unwrap()).unwrap();
    value["version"] = serde_json::json!(2);
    fs::write(seed, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(evidence::status(&f.0), Evidence::Unreadable);
    assert!(matches!(retained(&f.0), Err(SourceError::SourceUnreadable)));
}

/// The binding promises the receipt's exact bytes: a held snapshot whose
/// receipt changed, even to the same parsed receipt, is changed.
#[test]
fn the_held_reference_rechecks_the_exact_receipt_bytes() {
    let f = Folder::new();
    f.plain("old.txt", b"old cache");
    assert_eq!(ON.trim(&f.0, 0, 2).error, None);
    let held = retained(&f.0).unwrap().unwrap();
    let expected = held.reference().unwrap();
    let receipt = slot(&f).join("receipt.json");
    let mut bytes = fs::read(&receipt).unwrap();
    bytes.push(b'\n');
    fs::write(&receipt, bytes).unwrap();
    let fresh = retained(&f.0).unwrap().unwrap().reference().unwrap();
    assert_eq!(fresh.index_sha256, expected.index_sha256);
    assert_ne!(fresh.receipt_sha256, expected.receipt_sha256);
    assert_eq!(held.reference(), Err(SourceError::SourceChanged));
}

/// Windows canonicalizes a download folder to a verbatim path; the report
/// names it as its computer does, the form the download-index decoder takes
/// (zerus-5wmh.66.5.2.1.3.2). Other verbatim and device paths name no folder.
#[test]
fn a_verbatim_windows_folder_is_reported_as_its_computer_names_it() {
    use crate::paths::{absolute, Style};
    for (canonical, expected) in [
        (
            r"\\?\C:\Users\example\Downloads",
            r"C:\Users\example\Downloads",
        ),
        (r"\\?\d:\dl", r"d:\dl"),
        (
            r"\\?\UNC\files.example\share\dl",
            r"\\files.example\share\dl",
        ),
    ] {
        let folder = computer_folder(canonical).unwrap();
        assert_eq!(folder, expected);
        assert!(absolute(&folder, Style::Windows), "{folder}");
    }
    for unsupported in [
        r"\\?\Volume{00000000-0000-0000-0000-000000000000}\dl",
        r"\\?\GLOBALROOT\Device\dl",
        r"\\.\pipe\dl",
        r"\\?\C:",
    ] {
        assert_eq!(computer_folder(unsupported), None, "{unsupported}");
    }
    // Paths already in a computer's form are unchanged.
    for plain in [
        "/home/example/Downloads",
        r"C:\Users\example\Downloads",
        r"\\files.example\share\dl",
    ] {
        assert_eq!(computer_folder(plain).as_deref(), Some(plain));
    }
}
