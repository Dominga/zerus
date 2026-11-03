use super::*;
use std::fs;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("zerus-cache-preload-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
    fn put(&self, bytes: &[u8]) {
        crate::fsx::write_private_durable(&self.0.join("worktree-snapshots.json"), bytes).unwrap();
    }
    fn capture(&self) -> Arc<Capture> {
        Capture::worktree_cache(self.0.clone())
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.0.join("worktree-snapshots.json")).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn catalog() -> Value {
    serde_json::json!({"state":"ok","sampled_at":1,"stale":true,"worktrees":[],"private":{"kept":true}})
}
fn raw() -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({"a".repeat(64):catalog()})).unwrap()
}
fn snapshot(c: &Capture) -> Arc<Snapshot> {
    match c.prepare().unwrap() {
        Prepared::Snapshot(s) => s,
        _ => panic!("missing"),
    }
}
#[test]
fn exact_original_and_provenance_survive_later_cache_writes_and_restarts() {
    let s = Scratch::new();
    s.put(&raw());
    let c = s.capture();
    let original = snapshot(&c);
    let reference = original.reference().unwrap();
    let (saved, mut live) = c.worktree_owner().unwrap();
    assert_eq!(saved, Some(serde_json::from_slice(&raw()).unwrap()));
    live.write(&serde_json::json!({})).unwrap();
    assert_eq!(s.bytes(), b"{}");
    assert_eq!(original.bytes().unwrap(), raw());
    assert_eq!(original.reference().unwrap(), reference);
    let restart = s.capture();
    assert_eq!(snapshot(&restart).reference().unwrap(), reference);
    assert_eq!(
        restart.worktree_owner().unwrap().0,
        Some(serde_json::json!({}))
    );
    let streams = original.report_sources().unwrap();
    assert_eq!(streams[0].identity().family, "next-worktree-snapshots");
    assert_eq!(
        streams[1].identity().family,
        "next-worktree-cache-preload-provenance"
    );
    let values = streams
        .into_iter()
        .flat_map(SourceStream::into_chunks)
        .collect::<SourceResult<Vec<_>>>()
        .unwrap();
    let provenance = values.last().unwrap();
    assert_eq!(
        provenance["boundary"],
        "next-worktree-cache-before-first-load-v1"
    );
    assert_eq!(provenance["historical"], true);
    assert_eq!(provenance["drained"], false);
    assert_eq!(provenance["current_source_complete"], false);
}
#[test]
fn old_receipt_never_validates_unsupported_or_over_cap_current_cache() {
    let s = Scratch::new();
    s.put(&raw());
    let original = snapshot(&s.capture()).reference().unwrap();
    let over = serde_json::to_vec(
        &(0..65)
            .map(|i| (format!("{i:064x}"), catalog()))
            .collect::<serde_json::Map<_, _>>(),
    )
    .unwrap();
    for bytes in [b"{bad".to_vec(), b"[]".to_vec(), over] {
        s.put(&bytes);
        let c = s.capture();
        assert_eq!(snapshot(&c).reference().unwrap(), original);
        assert!(c.worktree_owner().is_err());
        assert_eq!(s.bytes(), bytes);
        assert_eq!(snapshot(&c).reference().unwrap(), original);
    }
}
#[test]
fn failed_or_uncertain_write_never_adopts_a_replaced_generation() {
    let s = Scratch::new();
    s.put(&raw());
    let c = s.capture();
    snapshot(&c);
    let (_, mut live) = c.worktree_owner().unwrap();
    assert_eq!(
        live.write_with(&serde_json::json!({}), |_, _| Err(
            io::ErrorKind::PermissionDenied.into()
        )),
        Err(SourceError::SourceUnreadable)
    );
    assert_eq!(s.bytes(), raw());
    live.recheck().unwrap();
    assert_eq!(
        live.write_with(&serde_json::json!({}), |p, b| {
            crate::fsx::write_private_durable(p, b)?;
            Err(io::ErrorKind::Other.into())
        }),
        Err(SourceError::SourceUnreadable)
    );
    assert_eq!(s.bytes(), b"{}");
    assert_eq!(live.recheck(), Err(SourceError::SourceChanged));
    assert_eq!(
        live.write(&serde_json::json!({})),
        Err(SourceError::SourceChanged)
    );
}
#[test]
fn missing_source_is_distinct_and_partial_capture_never_recaptures_changed_data() {
    let s = Scratch::new();
    let c = s.capture();
    assert!(matches!(c.prepare(), Ok(Prepared::Missing { .. })));
    assert_eq!(c.worktree_owner().unwrap().0, None);
    assert!(!s.0.join("worktree-cache-preload-v1/original.bin").exists());
    let s = Scratch::new();
    s.put(&raw());
    let c = s.capture();
    let snap = snapshot(&c);
    let reference = snap.reference().unwrap();
    fs::remove_file(s.0.join("worktree-cache-preload-v1/receipt.json")).unwrap();
    s.put(b"{}");
    assert!(c.prepare().is_err());
    assert_eq!(s.bytes(), b"{}");
    assert_eq!(
        fs::read(s.0.join("worktree-cache-preload-v1/original.bin")).unwrap(),
        raw()
    );
    assert!(reference.valid());
}
#[cfg(unix)]
#[test]
fn only_this_legacy_cache_can_read_owned_0644_without_chmod_or_relaxing_archives() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let s = Scratch::new();
    s.put(&raw());
    let file = s.0.join("worktree-snapshots.json");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let before = fs::metadata(&file).unwrap();
    let c = s.capture();
    let snap = snapshot(&c);
    assert_eq!(s.bytes(), raw());
    let after = fs::metadata(&file).unwrap();
    assert_eq!(
        (
            before.ino(),
            before.mode(),
            before.mtime(),
            before.mtime_nsec()
        ),
        (after.ino(), after.mode(), after.mtime(), after.mtime_nsec())
    );
    let archive = s.0.join("worktree-cache-preload-v1/original.bin");
    assert_eq!(fs::metadata(&archive).unwrap().mode() & 0o777, 0o600);
    // The general private reader must refuse the ORIGINAL 0644 source;
    // the scoped legacy-cache reader accepts it without changing its mode.
    assert!(disk::read_source(&file, 16 << 20).is_err());
    assert!(disk::read_legacy_cache(&file, 16 << 20).is_ok());
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(snap.bytes().is_err());
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o600)).unwrap();
    let (_, mut live) = c.worktree_owner().unwrap();
    live.write(&serde_json::json!({})).unwrap();
    assert_eq!(fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
    let s = Scratch::new();
    s.put(&raw());
    fs::set_permissions(
        s.0.join("worktree-snapshots.json"),
        fs::Permissions::from_mode(0o664),
    )
    .unwrap();
    assert!(s.capture().prepare().is_err());
    crate::fsx::write_private_durable(
        &s.0.join("attention-marks.json"),
        br#"{"version":1,"marks":[],"replies":[]}"#,
    )
    .unwrap();
    fs::set_permissions(
        s.0.join("attention-marks.json"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(Capture::local_reads(s.0.clone()).prepare().is_err());
}
#[cfg(unix)]
#[test]
fn hardlinks_and_same_bytes_replacement_refuse_and_symlinked_data_stays_pinned() {
    let s = Scratch::new();
    s.put(&raw());
    let alias = s.0.with_extension("alias");
    std::os::unix::fs::symlink(&s.0, &alias).unwrap();
    let c = Capture::worktree_cache(alias.clone());
    snapshot(&c);
    let (_, live) = c.worktree_owner().unwrap();
    fs::remove_file(&alias).unwrap();
    let other = Scratch::new();
    std::os::unix::fs::symlink(&other.0, &alias).unwrap();
    assert_eq!(c.live_path().unwrap(), s.0.join("worktree-snapshots.json"));
    live.recheck().unwrap();
    fs::remove_file(&alias).unwrap();
    let c = s.capture();
    snapshot(&c);
    let (_, live2) = c.worktree_owner().unwrap();
    s.put(&raw());
    assert_eq!(live2.recheck(), Err(SourceError::SourceChanged));
    drop(live);
    fs::hard_link(s.0.join("worktree-snapshots.json"), s.0.join("link")).unwrap();
    assert!(c.worktree_owner().is_err());
}
