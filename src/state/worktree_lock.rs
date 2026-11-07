//! zerus-ucck P3c: releasing a session's own Git worktree lock when the
//! session leaves, by archive or forget (the spec's worktree lock, rule 6).
//! Nothing creates such a lock before P3e. This build only releases one, so
//! that none lingers after a rollback to it.
//!
//! Git has no compare-and-unlock, so a release takes the lock. It renames
//! Git's `locked` file to a private name beside it, checks that the text is
//! exactly `zerus:<session ID>`, and only then removes it. Anything else is put
//! back, never over a lock that appeared meanwhile. An intent written before
//! the take lets the next holder of the writer lock put back a lock that a
//! crash left taken.
use super::*;
use std::fs;
use std::io::{self, Read};

/// The private name of a taken lock, beside Git's `locked`.
const TAKEN: &str = "locked.zerus-release-";
/// A lock file larger than this is not one Zerus wrote.
const LOCK_BYTES: u64 = 4096;
/// Intents settled per recovery, so an unexpected pile never stalls the
/// writer lock.
const INTENTS: usize = 64;

/// What a release did with the lock of a session that left.
#[derive(Debug, PartialEq)]
pub(super) enum Release {
    /// No lock has the session's exact text, so nothing was touched.
    None,
    /// The session's lock on this checkout is gone.
    Released(PathBuf),
    /// The lock on this checkout stays, for the reason given.
    Kept(PathBuf, String),
}

fn intents() -> PathBuf {
    root().join("worktree-locks")
}

/// A lock file's text: a regular file, read without following a final link
/// or blocking on a FIFO, and bounded. `None` when there is no lock.
fn lock_text(path: &Path) -> io::Result<Option<String>> {
    let file = match crate::platform::no_follow(fs::OpenOptions::new().read(true)).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(LOCK_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LOCK_BYTES {
        return Err(io::Error::other("too large for a lock"));
    }
    String::from_utf8(bytes).map(Some).map_err(io::Error::other)
}

/// Whether `text` is exactly the lock `zerus:<id>`, as `git worktree lock
/// --reason` writes it (with its newline) or without the newline.
fn exact(text: &str, id: &str) -> bool {
    text.strip_suffix('\n').unwrap_or(text) == format!("zerus:{id}")
}

/// Whether `a` and `b` are the same file, never through a link.
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
            (Ok(a), Ok(b)) => {
                a.is_file() && b.is_file() && a.dev() == b.dev() && a.ino() == b.ino()
            }
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        fs::symlink_metadata(a).is_ok_and(|m| m.is_file())
            && fs::symlink_metadata(b).is_ok_and(|m| m.is_file())
            && crate::platform::file_identity(a)
                .is_some_and(|identity| Some(identity) == crate::platform::file_identity(b))
    }
}

/// Releases the lock of `record`, a session that has just left, by archive
/// or forget. The caller holds the state writer lock, so no session starts
/// using the checkout meanwhile. It never fails the caller. A lock that
/// cannot be released stays, and the result says why (L4).
pub(super) fn release_locked(record: &Value) -> Release {
    let id = string(record, "session_id");
    let owner = &record["worktree_owner"];
    let metadata = Path::new(string(owner, "metadata"));
    // L1: only the checkout Zerus created for this session at its launch, and
    // only a lock with the session's exact text. A person's lock, another
    // session's and a plain folder are never touched.
    if !session_id::canonical(id) || string(owner, "id").is_empty() || !metadata.is_absolute() {
        return Release::None;
    }
    let lock = metadata.join("locked");
    match lock_text(&lock) {
        Ok(Some(text)) if exact(&text, id) => {}
        _ => return Release::None,
    }
    let checkout = PathBuf::from(string(owner, "path"));
    let kept = |reason: String| Release::Kept(checkout.clone(), reason);
    // Rule 3: the ownership nonce and its common-dir and metadata binding.
    let verified = worktree_lifecycle::owned(Path::new(string(owner, "common_dir")), &checkout)
        .is_some_and(|ownership| {
            ownership["id"] == owner["id"] && ownership["metadata"] == owner["metadata"]
        });
    if !verified {
        return kept("its Zerus ownership cannot be verified".into());
    }
    // L3: another live or saved session that uses the checkout keeps it, and
    // so does evidence that cannot be read.
    match worktree_cleanup::bindings() {
        Err(error) => {
            return kept(format!(
                "the sessions that may use it cannot be checked: {error}"
            ))
        }
        Ok(references) => {
            if let Some(other) = references.iter().find(|reference| {
                reference.owner == string(owner, "id")
                    || reference
                        .folders
                        .iter()
                        .any(|folder| folder.starts_with(&checkout))
            }) {
                return kept(format!("{} still uses it", other.name));
            }
        }
    }
    match take(&lock, id) {
        Ok(true) => Release::Released(checkout),
        Ok(false) => Release::None,
        Err(error) => kept(error),
    }
}

/// Releases the lock of `record`, which has just left, and reports what
/// happened on standard error. The caller holds the state writer lock.
pub(super) fn left(record: &Value) {
    match release_locked(record) {
        Release::None => {}
        Release::Released(checkout) => {
            eprintln!(
                "hgs: released the Git worktree lock on {}",
                checkout.display()
            );
        }
        Release::Kept(checkout, reason) => {
            eprintln!(
                "hgs: kept the Git worktree lock on {}: {reason}",
                checkout.display()
            );
        }
    }
}

/// L2: takes the lock by renaming it, removes it only when its text is
/// exactly `zerus:<id>`, and otherwise puts it back. `Ok(false)` when the
/// lock went before the take. Every failure after the take puts the lock
/// back, and one that cannot be put back yet stays in its intent for
/// `recover_locked`.
fn take(lock: &Path, id: &str) -> Result<bool> {
    let dir = lock.parent().ok_or("invalid lock path")?;
    let nonce = uuid::Uuid::new_v4();
    let taken = dir.join(format!("{TAKEN}{nonce}"));
    let intent = intents().join(format!("{nonce}.json"));
    let value = json!({"version":1,"lock":lock,"taken":taken,"session_id":id});
    atomic(&intent, &format!("{value}\n"))?;
    let taking =
        storage::cut_write().and_then(|()| fs::rename(lock, &taken).map_err(|e| e.to_string()));
    if let Err(error) = taking {
        // Nothing was taken: the lock is where it was, or already gone.
        let absent = |path: &Path| {
            fs::symlink_metadata(path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
        };
        let gone = absent(lock) && absent(&taken);
        let _ = archive::remove_synced(&intent);
        return if gone {
            Ok(false)
        } else {
            Err(format!("the lock could not be taken: {error}"))
        };
    }
    let checked = (|| -> Result<()> {
        crate::platform::sync_dir(dir).map_err(|e| e.to_string())?;
        match lock_text(&taken) {
            Ok(Some(text)) if exact(&text, id) => {}
            _ => return Err("the lock changed before its release; it was put back".into()),
        }
        storage::cut_write()?;
        fs::remove_file(&taken).map_err(|e| e.to_string())
    })();
    if let Err(error) = checked {
        return Err(match restore(lock, &taken, id) {
            Ok(()) => {
                let _ = archive::remove_synced(&intent);
                error
            }
            Err(failed) => format!("{error}; it could not be put back yet: {failed}"),
        });
    }
    let _ = crate::platform::sync_dir(dir);
    // A removal that fails leaves an intent whose taken lock is gone, which
    // recovery simply discards.
    let _ = archive::remove_synced(&intent);
    Ok(true)
}

/// Puts a taken lock back as Git's `locked`, never over a lock there. When
/// one appeared meanwhile, a taken lock with the session's exact text may go,
/// since the checkout stays locked. Any other stays beside it for review and
/// is never deleted (L2).
fn restore(lock: &Path, taken: &Path, id: &str) -> Result<()> {
    let dir = lock.parent().ok_or("invalid lock path")?;
    storage::cut_write()?;
    match fs::hard_link(taken, lock) {
        Ok(()) => {}
        // Nothing to put back: the taken lock, or its whole entry, is gone.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !same_file(taken, lock)
                && !matches!(lock_text(taken), Ok(Some(text)) if exact(&text, id))
            {
                return Err(format!(
                    "another lock appeared; the taken one stays at {} for review",
                    taken.display()
                ));
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    crate::platform::sync_dir(dir).map_err(|e| e.to_string())?;
    storage::cut_write()?;
    match fs::remove_file(taken) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    crate::platform::sync_dir(dir).map_err(|e| e.to_string())
}

/// A release intent: Git's `locked`, its taken name beside it, and the
/// session. An intent that is not exactly one of these is never acted on.
fn load(path: &Path) -> Option<(PathBuf, PathBuf, String)> {
    let mut bytes = Vec::new();
    crate::platform::no_follow(fs::OpenOptions::new().read(true))
        .open(path)
        .ok()?
        .take(LOCK_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let lock = PathBuf::from(value["lock"].as_str()?);
    let taken = PathBuf::from(value["taken"].as_str()?);
    let id = value["session_id"].as_str()?.to_owned();
    (value["version"] == 1
        && lock.is_absolute()
        && lock.file_name()? == "locked"
        && taken.parent() == lock.parent()
        && taken.file_name()?.to_str()?.starts_with(TAKEN)
        && session_id::canonical(&id))
    .then_some((lock, taken, id))
}

/// Called right after the writer lock is taken: puts back every lock a
/// release took and did not finish, after a crash included (L2). It never
/// fails the caller. An intent it cannot settle stays for the next holder.
pub(super) fn recover_locked() {
    let Ok(entries) = fs::read_dir(intents()) else {
        return;
    };
    for entry in entries.flatten().take(INTENTS) {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Some((lock, taken, id)) = load(&path) else {
            continue;
        };
        if restore(&lock, &taken, &id).is_ok() {
            let _ = archive::remove_synced(&path);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;

    /// Runs one case of the ignored fixture in a child with a private state
    /// root, a private Git repository and no tmux server.
    fn control(case: &str) {
        let state = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::worktree_lock::tests::fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", state.path().join("sessions-root"))
            .env("TMUX_TMPDIR", state.path().join("tmux"))
            .env("ZERUS_WORKTREE_LOCK_CONTROL", case)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", state.path().join("gitconfig"))
            .env_remove("TMUX")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{case}: {stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// `control`, but a child that is still running after `within` is
    /// killed and fails the test: a blocked scan must fail, never hang.
    fn control_within(case: &str, within: std::time::Duration) {
        use std::io::Read;
        let state = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::worktree_lock::tests::fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", state.path().join("sessions-root"))
            .env("TMUX_TMPDIR", state.path().join("tmux"))
            .env("ZERUS_WORKTREE_LOCK_CONTROL", case)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", state.path().join("gitconfig"))
            .env_remove("TMUX")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let until = std::time::Instant::now() + within;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{case}: the control was still blocked after {within:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(
            status.success() && stdout.contains("test result: ok. 1 passed"),
            "{case}: {stdout}{stderr}"
        );
    }

    #[test]
    fn its_own_lock_is_released_and_no_other_is_touched() {
        control("own");
    }
    /// zerus-lr1l2: the initial binding scan reads each file bounded: a
    /// canonical FIFO, a link or an oversized binding keeps the lock for
    /// review, promptly, and never blocks the release.
    #[test]
    fn a_fifo_link_or_oversized_binding_keeps_the_lock_without_blocking() {
        control_within("unreadable", std::time::Duration::from_secs(60));
    }
    #[test]
    fn a_shared_checkout_unverified_ownership_or_unreadable_evidence_keeps_the_lock() {
        control("kept");
    }
    #[test]
    fn a_lock_replaced_before_the_take_or_beside_a_restore_is_never_deleted() {
        control("replaced");
    }
    #[test]
    fn a_crash_at_every_write_puts_back_a_lock_it_did_not_release() {
        control("crash");
    }
    #[test]
    fn an_archive_stands_when_its_lock_is_kept_and_forget_releases_it() {
        control("archive");
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn taken_left(lock: &Path) -> Vec<String> {
        fs::read_dir(lock.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(TAKEN))
            .collect()
    }

    fn intents_left() -> usize {
        fs::read_dir(intents()).map_or(0, |entries| entries.count())
    }

    fn saved(name: &str, run: &str, folder: &Path) -> Value {
        let mut record = json!({"version":1,"agent":"claude","name":name,"run_id":run,
            "conversation_id":format!("conversation-{run}"),"created":100,
            "session_id":uuid::Uuid::new_v4().to_string(),"cwd":folder,"launch_dir":folder});
        storage::write(&mut record).unwrap();
        read(name).unwrap()
    }

    #[test]
    #[ignore = "run by the control tests"]
    fn fixture() {
        let Ok(case) = std::env::var("ZERUS_WORKTREE_LOCK_CONTROL") else {
            return;
        };
        fs::create_dir_all(root()).unwrap();
        let base = root().parent().unwrap().join("git");
        let main = base.join("main");
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(
            &main,
            &[
                "-c",
                "user.name=Zerus",
                "-c",
                "user.email=zerus@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        git(&main, &["worktree", "add", "-q", "../work", "-b", "work"]);
        let common = crate::platform::canonical(main.join(".git")).unwrap();
        let checkout = crate::platform::canonical(base.join("work")).unwrap();
        let head = git(&checkout, &["rev-parse", "HEAD"]);
        worktree_lifecycle::created(&common, &checkout, "work", head.trim()).unwrap();
        let ownership = worktree_lifecycle::owned(&common, &checkout).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let record = json!({"version":1,"agent":"claude","name":"claude/example/locked","session_id":id,
            "run_id":"run-locked","conversation_id":"conversation-locked","created":100,
            "worktree_owner":ownership,"cwd":checkout,"launch_dir":checkout});
        let lock = PathBuf::from(ownership["metadata"].as_str().unwrap()).join("locked");
        let own = format!("zerus:{id}");
        let place = |reason: &str| {
            git(
                &main,
                &[
                    "worktree",
                    "lock",
                    "--reason",
                    reason,
                    checkout.to_str().unwrap(),
                ],
            );
        };
        let unlock = || {
            git(&main, &["worktree", "unlock", checkout.to_str().unwrap()]);
        };
        let clean = || {
            assert!(taken_left(&lock).is_empty(), "{:?}", taken_left(&lock));
            assert_eq!(intents_left(), 0);
        };
        match case.as_str() {
            "own" => {
                place(&own);
                assert_eq!(release_locked(&record), Release::Released(checkout.clone()));
                assert!(!lock.exists());
                let listed = git(&main, &["worktree", "list", "--porcelain"]);
                assert!(
                    !listed.lines().any(|line| line.starts_with("locked")),
                    "{listed}"
                );
                clean();
                // Exactly its own text without Git's newline is its own too.
                fs::write(&lock, &own).unwrap();
                assert_eq!(release_locked(&record), Release::Released(checkout.clone()));
                // A person's lock, another session's and near misses are
                // never touched.
                let other = format!("zerus:{}", uuid::Uuid::new_v4());
                let spaced = format!("{own} ");
                let capital = format!("Zerus:{id}");
                for reason in [
                    "my own reason",
                    other.as_str(),
                    spaced.as_str(),
                    capital.as_str(),
                ] {
                    place(reason);
                    let before = fs::read(&lock).unwrap();
                    assert_eq!(release_locked(&record), Release::None, "{reason}");
                    assert_eq!(fs::read(&lock).unwrap(), before, "{reason}");
                    unlock();
                }
                // A link where Git's lock is, never followed.
                let elsewhere = base.join("elsewhere");
                fs::write(&elsewhere, format!("{own}\n")).unwrap();
                std::os::unix::fs::symlink(&elsewhere, &lock).unwrap();
                assert_eq!(release_locked(&record), Release::None);
                assert!(fs::symlink_metadata(&lock)
                    .unwrap()
                    .file_type()
                    .is_symlink());
                fs::remove_file(&lock).unwrap();
                // Without the checkout's ownership at launch, or without a
                // session ID, there is nothing to release.
                place(&own);
                for (key, value) in [("worktree_owner", Value::Null), ("session_id", json!(""))] {
                    let mut plain = record.clone();
                    plain[key] = value;
                    assert_eq!(release_locked(&plain), Release::None, "{key}");
                }
                assert_eq!(fs::read_to_string(&lock).unwrap(), format!("{own}\n"));
                clean();
            }
            "kept" => {
                place(&own);
                let before = fs::read(&lock).unwrap();
                let kept = |record: &Value, why: &str| match release_locked(record) {
                    Release::Kept(path, reason) => {
                        assert_eq!(path, checkout);
                        assert!(reason.contains(why), "{reason}");
                        assert_eq!(fs::read(&lock).unwrap(), before);
                    }
                    other => panic!("{why}: {other:?}"),
                };
                // Rule 3: another ownership nonce does not verify.
                let mut forged = record.clone();
                forged["worktree_owner"]["id"] = json!(uuid::Uuid::new_v4().to_string());
                kept(&forged, "ownership cannot be verified");
                // L3: a saved session in the checkout, or one launched with
                // its ownership from another folder.
                let inside = saved("claude/example/fork", "run-fork", &checkout.join("sub"));
                kept(&record, "claude/example/fork still uses it");
                storage::remove_record(&inside).unwrap();
                let mut owner = saved("claude/example/owner", "run-owner", &base);
                owner["worktree_owner"] = ownership.clone();
                storage::write(&mut owner).unwrap();
                kept(&record, "claude/example/owner still uses it");
                storage::remove_record(&owner).unwrap();
                // Evidence that cannot be read keeps it.
                let broken = root().join(format!("{}.json", "0".repeat(64)));
                fs::write(&broken, "{").unwrap();
                kept(&record, "cannot be checked");
                fs::remove_file(&broken).unwrap();
                clean();
                assert_eq!(release_locked(&record), Release::Released(checkout.clone()));
            }
            "unreadable" => {
                place(&own);
                let before = fs::read(&lock).unwrap();
                let kept = |why: &str| match release_locked(&record) {
                    Release::Kept(path, reason) => {
                        assert_eq!(path, checkout);
                        assert!(reason.contains("cannot be checked"), "{why}: {reason}");
                        assert_eq!(fs::read(&lock).unwrap(), before, "{why}");
                    }
                    other => panic!("{why}: {other:?}"),
                };
                // A FIFO at a canonical ID-layout binding path: no writer
                // ever opens it, so a blocking read would wait forever.
                let sessions = root().join("sessions");
                fs::create_dir_all(&sessions).unwrap();
                let fifo = sessions.join(format!("{}.json", uuid::Uuid::new_v4()));
                let name = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                kept("a FIFO binding");
                fs::remove_file(&fifo).unwrap();
                // A link at a legacy binding path, to a valid record elsewhere.
                let target = base.join("record.json");
                fs::write(&target, serde_json::to_vec(&record).unwrap()).unwrap();
                let linked = root().join(format!("{}.json", "1".repeat(64)));
                std::os::unix::fs::symlink(&target, &linked).unwrap();
                kept("a linked binding");
                fs::remove_file(&linked).unwrap();
                // A binding past the record bound (sparse: no disk used).
                let huge = root().join(format!("{}.json", "2".repeat(64)));
                fs::File::create(&huge)
                    .unwrap()
                    .set_len(storage::ID_RECORD_BYTES + 1)
                    .unwrap();
                kept("an oversized binding");
                fs::remove_file(&huge).unwrap();
                clean();
                // With only readable bindings left, the lock is released.
                assert_eq!(release_locked(&record), Release::Released(checkout.clone()));
            }
            "replaced" => {
                // Replaced between the read and the take: put back as it was.
                place("my own reason");
                let before = fs::read(&lock).unwrap();
                assert!(take(&lock, &id)
                    .unwrap_err()
                    .contains("changed before its release"));
                assert_eq!(fs::read(&lock).unwrap(), before);
                clean();
                unlock();
                // A lock that appears while a taken one is put back is left
                // alone. A taken lock that is not exactly this session's stays
                // beside it for review; one that is goes.
                let taken = lock.parent().unwrap().join(format!("{TAKEN}manual"));
                fs::write(&lock, "appeared\n").unwrap();
                fs::write(&taken, "my own reason\n").unwrap();
                assert!(restore(&lock, &taken, &id)
                    .unwrap_err()
                    .contains("for review"));
                assert_eq!(fs::read_to_string(&lock).unwrap(), "appeared\n");
                assert_eq!(fs::read_to_string(&taken).unwrap(), "my own reason\n");
                fs::write(&taken, format!("{own}\n")).unwrap();
                restore(&lock, &taken, &id).unwrap();
                assert_eq!(fs::read_to_string(&lock).unwrap(), "appeared\n");
                assert!(!taken.exists());
                fs::remove_file(&lock).unwrap();
                // A lock that went before the take is no release at all.
                assert!(!take(&lock, &id).unwrap());
                clean();
            }
            "crash" => {
                // Each durable write in turn is the last before a crash; the
                // next holder of the writer lock settles what is left. A lock
                // goes only when its exact text was checked and it was
                // removed; any other comes back byte for byte.
                for (text, releases) in [
                    (format!("{own}\n"), true),
                    ("my own reason\n".to_owned(), false),
                ] {
                    let mut finished = false;
                    for budget in 0..8 {
                        fs::write(&lock, &text).unwrap();
                        storage::cut::arm(budget);
                        let result = if releases {
                            release_locked(&record) == Release::Released(checkout.clone())
                        } else {
                            take(&lock, &id).is_ok()
                        };
                        storage::cut::disarm();
                        assert!(
                            !result || releases,
                            "budget {budget}: a foreign lock was released"
                        );
                        drop(storage::lock(None).unwrap());
                        let gone = !lock.exists();
                        assert_eq!(gone, releases && budget >= 3, "budget {budget}: {text}");
                        if !gone {
                            assert_eq!(fs::read_to_string(&lock).unwrap(), text, "budget {budget}");
                        }
                        clean();
                        finished |= budget >= 5;
                    }
                    assert!(finished);
                }
            }
            "archive" => {
                let name = "claude/example/locked";
                let write_run = |run: &str| {
                    let mut session = record.clone();
                    session["run_id"] = json!(run);
                    session["conversation_id"] = json!(format!("conversation-{run}"));
                    storage::write(&mut session).unwrap();
                    read(name).unwrap()
                };
                // Every way a session leaves releases its own lock: an archive,
                // terminate --forget of a saved session, and terminate --forget
                // of one already archived.
                place(&own);
                let session = write_run("run-archive");
                let guard = storage::lock(None).unwrap();
                archive::save(&session).unwrap();
                drop(guard);
                assert!(!lock.exists());
                place(&own);
                write_run("run-forget");
                terminate::dispatch_scoped(&[name.to_owned()], false, None, true).unwrap();
                assert!(read(name).is_err());
                assert!(!lock.exists());
                place(&own);
                let session = write_run("run-archived");
                let fork = saved("claude/example/fork", "run-fork", &checkout);
                let guard = storage::lock(None).unwrap();
                archive::save(&session).unwrap();
                drop(guard);
                storage::remove_record(&fork).unwrap();
                assert!(lock.exists());
                write_run("run-archived");
                terminate::dispatch_scoped(&[name.to_owned()], false, None, true).unwrap();
                assert!(read(name).is_err());
                assert!(!lock.exists());
                clean();
                // L4: the archive stands and the lock stays while another
                // session uses the checkout; forgetting the archive once none
                // does releases it.
                place(&own);
                let mut session = record.clone();
                storage::write(&mut session).unwrap();
                let session = read("claude/example/locked").unwrap();
                let fork = saved("claude/example/fork", "run-fork", &checkout);
                let guard = storage::lock(None).unwrap();
                let archive_id = archive::save(&session).unwrap();
                drop(guard);
                assert!(read("claude/example/locked").is_err());
                assert_eq!(fs::read_to_string(&lock).unwrap(), format!("{own}\n"));
                storage::remove_record(&fork).unwrap();
                archive::forget("claude/example/locked", &archive_id, false).unwrap();
                assert!(!lock.exists());
                clean();
                // A take that fails keeps the lock and never fails the archive.
                if unsafe { libc::geteuid() } != 0 {
                    use std::os::unix::fs::PermissionsExt;
                    place(&own);
                    let mut session = record.clone();
                    session["run_id"] = json!("run-again");
                    storage::write(&mut session).unwrap();
                    let session = read("claude/example/locked").unwrap();
                    let entry = lock.parent().unwrap().to_path_buf();
                    fs::set_permissions(&entry, fs::Permissions::from_mode(0o500)).unwrap();
                    let guard = storage::lock(None).unwrap();
                    let archived = archive::save(&session);
                    // What that archive's release met, once its record left.
                    let outcome = release_locked(&session);
                    drop(guard);
                    fs::set_permissions(&entry, fs::Permissions::from_mode(0o700)).unwrap();
                    archived.unwrap();
                    assert!(read("claude/example/locked").is_err());
                    assert!(
                        matches!(&outcome, Release::Kept(_, reason) if reason.contains("could not be taken")),
                        "{outcome:?}"
                    );
                    assert_eq!(fs::read_to_string(&lock).unwrap(), format!("{own}\n"));
                    clean();
                }
            }
            other => panic!("unknown control {other}"),
        }
    }
}
