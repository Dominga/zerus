//! Executable parity with the sealed parent runtime acquisition, not a
//! second production path. All files belong to this test's private fixture.
use super::*;
use std::{
    collections::BTreeMap,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
};

// Sealed parent: ca5a7d6d4cd2808e21c4de5d1f3008fd579940b0 src/state/storage/job_round.rs::try_at.
// The function body is copied verbatim; the source handoff verifies its hash.
fn parent_acquire(selected: &Path, kind: Kind) -> Result<Option<Held>> {
    private_dir(selected)?;
    let state = Directory::open(&crate::platform::canonical(selected).map_err(|e| e.to_string())?)?;
    if !state.owner_controls_entries()? {
        return Err(
            "The current user does not exclusively control the recovery state entries".into(),
        );
    }
    let recovery = state.ensure_child("recovery")?;
    if !recovery.owner_controls_entries()? {
        return Err(
            "The current user does not exclusively control the recovery round entries".into(),
        );
    }
    let file = recovery.open_private_lock(kind.name())?;
    match file.try_lock_exclusive() {
        Ok(()) => (),
        Err(error) if crate::platform::lock_contended(&error) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let identity = crate::platform::FileIdentity::of(&file).map_err(|e| e.to_string())?;
    let held = Held {
        selected: selected.to_owned(),
        state,
        recovery,
        file,
        identity,
        kind,
    };
    held.verify_at(selected)?;
    Ok(Some(held))
}

type Acquire = fn(&Path, Kind) -> Result<Option<Held>>;

fn write_private(path: &Path) {
    fs::write(path, b"").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn setup(root: &Path, kind: Kind, case: &str) -> Option<fs::File> {
    if case == "cold" {
        return None;
    }
    fs::create_dir(root).unwrap();
    fs::set_permissions(
        root,
        fs::Permissions::from_mode(if case == "readable" {
            0o755
        } else if case == "shared" {
            0o775
        } else {
            0o700
        }),
    )
    .unwrap();
    if ["readable", "shared"].contains(&case) {
        return None;
    }
    let recovery = root.join("recovery");
    fs::create_dir(&recovery).unwrap();
    fs::set_permissions(&recovery, fs::Permissions::from_mode(0o700)).unwrap();
    let path = recovery.join(kind.name());
    match case {
        "fifo" => {
            let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: this test owns the absent NUL-free fixture pathname.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        }
        "alias" => std::os::unix::fs::symlink(root.join("missing-foreign"), &path).unwrap(),
        _ => {
            write_private(&path);
            if case == "hardlink" {
                fs::hard_link(&path, recovery.join("extra-link")).unwrap();
            }
        }
    }
    if case == "busy" {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        file.try_lock_exclusive().unwrap();
        Some(file)
    } else {
        None
    }
}

fn footprint(root: &Path, kind: Kind) -> serde_json::Value {
    let mut rows = BTreeMap::new();
    for name in [
        "".to_owned(),
        "recovery".into(),
        format!("recovery/{}", kind.name()),
        "recovery/extra-link".into(),
        "recovery/retained-original".into(),
        "missing-foreign".into(),
    ] {
        let path = root.join(&name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let ty = meta.file_type();
        let mut row = serde_json::json!({"mode":meta.mode()&0o7777,"links":if ty.is_file(){meta.nlink()}else{0},
            "kind":if ty.is_symlink(){"symlink"}else if ty.is_fifo(){"fifo"}else if ty.is_dir(){"directory"}else{"regular"}});
        if ty.is_file() {
            row["bytes"] = serde_json::json!(fs::read(&path).unwrap());
        }
        if ty.is_symlink() {
            let target = fs::read_link(&path).unwrap();
            row["target"] = serde_json::json!(target.strip_prefix(root).unwrap().to_string_lossy());
        }
        rows.insert(name, row);
    }
    serde_json::json!(rows)
}

fn observe(acquire: Acquire, root: &Path, kind: Kind, case: &str) -> Vec<u8> {
    let busy = setup(root, kind, case);
    let start = std::time::Instant::now();
    let result = acquire(root, kind);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    let mut trace = serde_json::json!({"case":case,"kind":kind.name()});
    match result {
        Err(error) => trace["result"] = serde_json::json!({"error":error}),
        Ok(None) => trace["result"] = serde_json::json!({"busy":true}),
        Ok(Some(held)) => {
            let original = held.identity;
            held.verify_at(root).unwrap();
            assert!(
                matches!(acquire(root, kind), Ok(None)),
                "the original round must exclude its peer"
            );
            trace["result"] = serde_json::json!({"held":true,"peer_busy":true});
            if case == "replacement" {
                let path = root.join("recovery").join(kind.name());
                fs::rename(&path, root.join("recovery/retained-original")).unwrap();
                write_private(&path);
                let error = held
                    .verify_at(root)
                    .expect_err("a new pathname file is never the original");
                trace["verification_error"] = serde_json::json!(error);
                let old = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(root.join("recovery/retained-original"))
                    .unwrap();
                assert!(old.try_lock_exclusive().is_err());
                let meta = old.metadata().unwrap();
                assert_eq!((meta.dev(), meta.ino()), (original.device, original.inode));
                drop(old);
                drop(held);
                let original_file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(root.join("recovery/retained-original"))
                    .unwrap();
                original_file.try_lock_exclusive().unwrap();
                fs2::FileExt::unlock(&original_file).unwrap();
                trace["original_released_on_drop"] = serde_json::json!(true);
            } else {
                drop(held);
                let again = acquire(root, kind).unwrap().unwrap();
                assert!(
                    again.identity == original,
                    "release never replaces the original exclusion"
                );
                again.verify_at(root).unwrap();
                drop(again);
                trace["same_original_after_drop"] = serde_json::json!(true);
            }
        }
    }
    trace["files"] = footprint(root, kind);
    if let Some(file) = busy {
        fs2::FileExt::unlock(&file).unwrap();
        drop(file);
    }
    serde_json::to_vec(&trace).unwrap()
}

#[test]
fn both_installed_create_paths_match_the_sealed_parent_byte_for_byte() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("state");
    for kind in [Kind::Recovery, Kind::KeepWarm] {
        for case in [
            "cold",
            "readable",
            "busy",
            "fifo",
            "alias",
            "hardlink",
            "shared",
            "replacement",
        ] {
            let before = observe(parent_acquire, &root, kind, case);
            fs::remove_dir_all(&root).unwrap();
            let after = observe(try_at, &root, kind, case);
            fs::remove_dir_all(&root).unwrap();
            assert_eq!(
                before,
                after,
                "runtime observable trace changed: {} {case}",
                kind.name()
            );
            println!(
                "runtime-round-parity {} {case}: byte-identical",
                kind.name()
            );
        }
    }
}
