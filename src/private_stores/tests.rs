use super::*;
#[cfg(windows)]
use std::process::{Command, Stdio};

/// Runs `test` with a private state root of its own (the stores, the current
/// set and the journal all live there).
fn with_root(test: impl FnOnce(&Path)) {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::platform::canonical(temp.path()).unwrap();
    crate::state::with_test_state_root(&root, || test(&root));
}

fn journal_lines(root: &Path) -> Vec<Value> {
    fs::read_to_string(root.join(JOURNAL))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// As a sandbox installer leaves it: Authenticated Users (S-1-5-11) and an
/// extra group (BUILTIN\Users, S-1-5-32-545) may read or change the entry.
#[cfg(windows)]
fn widen(path: &Path, folder: bool) {
    for grant in if folder { ["*S-1-5-11:(OI)(CI)M", "*S-1-5-32-545:(OI)(CI)RX"] } else { ["*S-1-5-11:M", "*S-1-5-32-545:RX"] } {
        let granted = Command::new("icacls").arg(path).args(["/grant", grant, "/Q"]).stdout(Stdio::null()).status().unwrap();
        assert!(granted.success(), "icacls {grant}");
    }
}
#[cfg(unix)]
fn widen(path: &Path, folder: bool) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(if folder { 0o755 } else { 0o644 })).unwrap();
}

#[test]
fn an_owned_store_with_extra_access_is_repaired_logged_and_keeps_its_content() {
    with_root(|root| {
        let store = root.join("read-marks.json");
        fs::write(&store, br#"{"version":1,"sessions":{}}"#).unwrap();
        widen(&store, false);
        assert!(!platform::verify_private(&store, Private::File).unwrap(), "the check refuses the widened store");
        // The reader's path: the handle it holds sees the repaired file.
        let file = platform::refuse_link(fs::OpenOptions::new().read(true)).open(&store).unwrap();
        assert!(ensure_file(&store, &file).unwrap());
        drop(file);
        assert!(platform::verify_private(&store, Private::File).unwrap());
        assert_eq!(fs::read(&store).unwrap(), br#"{"version":1,"sessions":{}}"#);
        // One journal line names the path and what was removed; no content.
        let lines = journal_lines(root);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["path"], store.display().to_string());
        assert_eq!(lines[0]["kind"], "file");
        let removed = lines[0]["removed"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_owned()).collect::<Vec<_>>().join(" | ");
        #[cfg(windows)]
        assert!(removed.contains("S-1-5-11") && removed.contains("S-1-5-32-545"), "{removed}");
        #[cfg(unix)]
        assert!(removed.contains("0644") && removed.contains("0600"), "{removed}");
        assert!(!lines[0].to_string().contains("sessions"), "the journal never holds a store's content");
        assert!(platform::verify_private(&root.join(JOURNAL), Private::File).unwrap(), "the journal is owner-only");
        assert!(refused().is_empty());
        // Already private: nothing more is written.
        assert!(ensure(&store, Private::File).unwrap());
        assert_eq!(journal_lines(root).len(), 1);
    });
}

#[test]
fn an_owned_folder_store_is_repaired_and_its_new_files_stay_private() {
    with_root(|root| {
        let folder = root.join("analytics");
        fs::create_dir(&folder).unwrap();
        widen(&folder, true);
        assert!(!platform::verify_private(&folder, Private::Directory).unwrap());
        assert!(ensure(&folder, Private::Directory).unwrap());
        assert!(platform::verify_private(&folder, Private::Directory).unwrap());
        // Windows inherits the repaired directory ACL; Unix callers must also
        // request a private creation mode, as the actual store writers do.
        let mut file = platform::private_open(fs::OpenOptions::new().write(true).create_new(true))
            .open(folder.join("analytics.sqlite3")).unwrap();
        file.write_all(b"x").unwrap();
        drop(file);
        assert!(platform::verify_private(&folder.join("analytics.sqlite3"), Private::File).unwrap());
        assert_eq!(journal_lines(root)[0]["kind"], "folder");
    });
}

#[test]
fn a_store_that_stays_refused_is_reported_with_its_fix_until_it_is_private() {
    with_root(|root| {
        // Outside Zerus's folders: reported, never changed.
        let outside = tempfile::tempdir().unwrap();
        let foreign = outside.path().join("store.json");
        fs::write(&foreign, b"{}").unwrap();
        widen(&foreign, false);
        assert!(!ensure(&foreign, Private::File).unwrap());
        assert!(!platform::verify_private(&foreign, Private::File).unwrap(), "a path outside the roots is left as it is");
        let stores = refused();
        assert_eq!(stores.len(), 1);
        assert_eq!(stores[0]["path"], foreign.display().to_string());
        assert!(stores[0]["reason"].as_str().unwrap().contains("outside Zerus's folders"));
        let fix = stores[0]["fix"].as_str().unwrap();
        #[cfg(windows)]
        assert_eq!(fix, format!("icacls \"{}\" /inheritance:r /grant:r \"%USERNAME%:F\"", foreign.display()));
        #[cfg(unix)]
        assert_eq!(fix, format!("chmod 600 '{}'", foreign.display()));
        // The same refusal again keeps its first record (a notice is raised once).
        let at = stores[0]["at"].clone();
        assert!(!ensure(&foreign, Private::File).unwrap());
        assert_eq!(refused()[0]["at"], at);
        // Once the owner applies the fix, the next check clears it.
        platform::owner_only_file(&foreign).unwrap();
        assert!(ensure(&foreign, Private::File).unwrap());
        assert!(refused().is_empty());
        assert!(!root.join(CURRENT).exists(), "an empty set leaves no file to read");
    });
}

#[test]
fn a_link_in_place_of_a_store_stays_refused() {
    with_root(|root| {
        let target = root.join("elsewhere");
        fs::create_dir(&target).unwrap();
        let link = root.join("approval-audit");
        #[cfg(windows)]
        assert!(Command::new("cmd").args(["/c", "mklink", "/J"]).arg(&link).arg(&target).stdout(Stdio::null()).status().unwrap().success());
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(!ensure(&link, Private::Directory).unwrap());
        let store = refused().into_iter().next().unwrap();
        assert!(store["reason"].as_str().unwrap().contains("link"), "{store}");
        assert!(store["fix"].as_str().unwrap().contains(".refused"), "{store}");
    });
}

#[cfg(windows)]
#[test]
fn a_file_owned_by_another_account_is_refused_unchanged() {
    // TrustedInstaller owns the system's own folders.
    let system = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("System32");
    match platform::repair_private(&system, Private::Directory).unwrap() {
        PrivateRepair::Refused(reason) => assert!(reason.contains("owned by"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(fix(&system, Private::File, "it is owned by NT SERVICE\\TrustedInstaller").starts_with("takeown /f "));
}

#[test]
fn a_failed_write_is_reported_until_the_store_is_found_private() {
    with_root(|root| {
        let store = root.join("read-marks.json");
        fs::write(&store, b"{}").unwrap();
        platform::owner_only_file(&store).unwrap();
        write_failed(&store, Private::File, &"Access is denied. (os error 5)");
        let stores = refused();
        assert_eq!(stores.len(), 1);
        assert_eq!(stores[0]["source"], "write");
        assert!(stores[0]["reason"].as_str().unwrap().starts_with("a write failed"));
        assert!(ensure(&store, Private::File).unwrap());
        assert!(refused().is_empty());
    });
}

#[test]
fn the_startup_check_repairs_the_known_stores_and_skips_missing_ones() {
    with_root(|root| {
        let store = root.join("read-marks.json");
        fs::write(&store, b"{}").unwrap();
        widen(&store, false);
        check_known();
        assert!(platform::verify_private(&store, Private::File).unwrap());
        assert!(refused().is_empty(), "missing stores are not reported");
        assert_eq!(journal_lines(root).len(), 1);
    });
}

#[test]
fn the_journal_keeps_the_newest_lines_within_its_bounds() {
    with_root(|root| {
        let long = "x".repeat(400);
        for index in 0..260 {
            journal(json!({"path":format!("{index}"),"kind":"file","refused":long}));
        }
        let text = fs::read_to_string(root.join(JOURNAL)).unwrap();
        assert!(text.len() as u64 <= JOURNAL_BYTES, "{} bytes", text.len());
        let lines = journal_lines(root);
        assert!(lines.len() <= JOURNAL_LINES);
        assert_eq!(lines.last().unwrap()["path"], "259");
        assert!(platform::verify_private(&root.join(JOURNAL), Private::File).unwrap());
    });
}

#[test]
fn a_canonical_windows_path_is_compared_without_its_verbatim_prefix() {
    // The desktop presence fence opens `\\?\C:\...`; the roots are plain.
    assert_eq!(plain(Path::new(r"\\?\C:\Users\a\.local\state\hgs\x.lock")).as_ref(),
        Path::new(r"C:\Users\a\.local\state\hgs\x.lock"));
    assert_eq!(plain(Path::new(r"\\?\UNC\server\share\x")).as_ref(), Path::new(r"\\?\UNC\server\share\x"));
    assert_eq!(plain(Path::new("/home/user/x")).as_ref(), Path::new("/home/user/x"));
}

/// Task #383: a refusal whose path is gone - a test's temporary file, a
/// removed folder - drops out at the next read and is never shown again.
#[test]
fn a_refusal_whose_path_vanished_drops_out() {
    with_root(|root| {
        let gone = root.join("tmp-test").join("read-marks.json");
        let kept = root.join("kept.json");
        fs::write(&kept, b"{}").unwrap();
        write_failed(&gone, Private::File, &"it is not private");
        write_failed(&kept, Private::File, &"it is not private");
        let paths = |stores: Vec<Value>| stores.iter().map(|store| store["path"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
        assert_eq!(paths(refused()), vec![kept.display().to_string()]);
        // The current set no longer holds it either.
        assert_eq!(paths(stores()), vec![kept.display().to_string()]);
        fs::remove_file(&kept).unwrap();
        assert!(refused().is_empty());
        assert!(!current_path().exists(), "an empty set leaves no file");
    });
}

/// Task #383: tests never write the person's private-store list. A test that
/// sets no state folder of its own records in the test process's folder
/// (storage::test_default_root), and the person's file is left as it was.
#[test]
fn a_test_never_writes_the_persons_private_store_list() {
    let person = crate::state::watch_source::state_root();
    let home = std::path::PathBuf::from(platform::home().unwrap_or_default());
    let real = home.join(".local/state/hgs/sessions");
    assert_ne!(person, real, "a test's state folder is never the person's");
    assert!(!person.starts_with(&real));
    let before = fs::read(real.join(CURRENT)).ok();
    let probe = std::env::temp_dir().join(format!("zerus-383-{}", std::process::id())).join("read-marks.json");
    write_failed(&probe, Private::File, &"it is not private");
    assert_eq!(fs::read(real.join(CURRENT)).ok(), before, "the person's list is untouched");
    // The refusal went to the test's own list, where the vanished path drops out.
    assert!(refused().iter().all(|store| store["path"] != probe.display().to_string()));
}
