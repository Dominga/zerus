use super::*;
use crate::desktop::{
    original_child::{self, Codes, Ended, Input, Output, Spec, Stdin},
    original_store::{self, Roots},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::{
    cell::Cell,
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::MetadataExt,
    path::PathBuf,
};
use zerus_client_core::run::custody::InheritedLock;
const CUSTODY: &str = "HGS_TEAMS_CUSTODY_FD";
const CODES: Codes = Codes {
    unobserved: "team_confirmation_unobserved",
    unreadable: "team_confirmation_unreadable",
    signalled: "team_confirmation_signalled",
    timed_out: "team_confirmation_timed_out",
    retained: "team_confirmation_retained",
};
impl Native<'_> {
    pub fn deliver(&mut self, row: &Record) -> Outcome {
        let Some((snapshot, raw)) = self.capture.clone() else {
            return Outcome::NotStarted("No exact native observation is held".into());
        };
        let Ok(program) = program() else {
            return Outcome::NotStarted("The own CLI image is unavailable".into());
        };
        let Ok((args, input, folder)) = super::receipt::invocation(row) else {
            return Outcome::NotStarted("The fixed confirmation request is invalid".into());
        };
        let Ok(roots) = Roots::open(&self.config.state, folder) else {
            return Outcome::NotStarted("The original receipt store is unavailable".into());
        };
        let Some((held, path, id)) = custody(self.config, &row.original) else {
            return Outcome::NotStarted("Original confirmation custody is unavailable".into());
        };
        let mut scoped = vec!["--expect-machine".into(), self.machine.into()];
        scoped.extend(args);
        let waiting = Cell::new(false);
        let ended = original_child::run(
            &program,
            &scoped,
            specification(&input),
            &held,
            Instant::now() + Duration::from_secs(45),
            |cmd| {
                #[cfg(test)]
                delivery::before();
                configure(cmd, self.config);
                // The preflight snapshot is not permission: re-read under the lock.
                #[cfg(test)]
                if let Some(hook) = delivery::AFTER_FACTS.with(|slot| slot.borrow_mut().take()) {
                    hook();
                }
                let db = self
                    .store
                    .db
                    .as_ref()
                    .ok_or(std::io::ErrorKind::PermissionDenied)?;
                let admission = admission(db).inspect_err(|error| {
                    waiting.set(error.kind() == std::io::ErrorKind::WouldBlock);
                })?;
                self.final_admit(row, &roots, &snapshot, &raw, &admission)
                    .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
                Ok(admission)
            },
        );
        if !matches!(ended, Ended::Unknown(_)) {
            remove(&path, id);
        }
        match ended {
            Ended::NotStarted if waiting.get() => Outcome::Retry,
            Ended::NotStarted => Outcome::NotStarted(
                "Final standing-confirmation admission refused; nothing started".into(),
            ),
            Ended::Unknown(reason) => Outcome::Unknown(reason.into()),
            Ended::Exited { .. } => super::receipt::observe(&roots, row, &input),
        }
    }
    fn final_admit(
        &self,
        row: &Record,
        roots: &Roots,
        snapshot: &Snapshot,
        raw: &Value,
        db: &Connection,
    ) -> Result<()> {
        let plan = &row.plan;
        let b = &plan.binding;
        let facts = stored_facts(db, b)?;
        super::super::policy::stored(b, &facts)?;
        let delegated = if plan.kind == Kind::Trust {
            facts.team.settings.delegations.confirm_trust
        } else {
            facts.team.settings.delegations.resume_exited
        };
        if !delegated {
            return Err("Standing delegation was revoked".into());
        }
        #[cfg(test)]
        if let Some(hook) = delivery::LOCKED_FACTS.with(|slot| slot.borrow_mut().take()) {
            hook();
        }
        if !crate::teams::enabled(&self.config.state)
            || b.machine != self.machine
            || self.binding(&b.team, &b.member)?.0 != *b
            || !self.anchored(b)?
            || !roots.root_current()
            || self.pins.is_empty()
            || !self.pins.iter().all(super::ownership::Pin::current)
        {
            return Err("Delegation, root or ownership changed before spawn".into());
        }
        if crate::state::lifecycle_receipt_machine(self.config)? != self.machine {
            return Err("The local computer changed".into());
        }
        let current = held_current(roots, snapshot)?;
        unchanged_record(&current, raw)?;
        if super::observation::paused(&current) || self.stop.load(Ordering::Acquire) {
            return Err("The member was paused, stopped or replaced".into());
        }
        if plan.kind == Kind::Resume
            && !super::observation::self_exit(&current, snapshot.archive.is_some())
        {
            return Err("Exit evidence changed before spawn".into());
        }
        Ok(())
    }
}
fn specification(input: &[u8]) -> Spec<'_> {
    Spec {
        custody_env: CUSTODY,
        input: Input::Frozen {
            stdin: Stdin::Bytes(input),
            unwritten: "team_confirmation_unwritten",
        },
        output: Output::Stdout {
            bytes: 64 << 10,
            too_large: "team_confirmation_output_large",
        },
        codes: CODES,
        retain: None,
    }
}
// A busy writer is a certain non-start, not a refused permission or an Unknown.
fn admission(db: &Connection) -> std::io::Result<Transaction<'_>> {
    let prior: u64 = db
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .map_err(std::io::Error::other)?;
    db.busy_timeout(Duration::from_millis(100))
        .map_err(std::io::Error::other)?;
    let result = Transaction::new_unchecked(db, TransactionBehavior::Immediate);
    db.busy_timeout(Duration::from_millis(prior))
        .map_err(std::io::Error::other)?;
    result.map_err(|error| {
        let kind = if matches!(error, rusqlite::Error::SqliteFailure(e, _) if matches!(e.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)) {
            std::io::ErrorKind::WouldBlock
        } else { std::io::ErrorKind::PermissionDenied };
        std::io::Error::new(kind, error)
    })
}
fn stored_facts(db: &Connection, b: &Binding) -> Result<Facts> {
    use crate::teams::store::rows;
    Ok(Facts {
        team: rows::team(db, &b.team)
            .map_err(|e| e.to_string())?
            .ok_or("Team disappeared")?,
        member: rows::member(db, &b.team, &b.member)
            .map_err(|e| e.to_string())?
            .ok_or("Member disappeared")?,
        // Both are checked from their external stores last, inside the lock.
        binding: b.clone(),
        preview: true,
    })
}
fn program() -> std::io::Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = delivery::PROGRAM.with(|slot| slot.borrow().clone()) {
        return Ok(path);
    }
    crate::self_exec::current()
}
fn unchanged_record(current: &Value, raw: &Value) -> Result<()> {
    for key in [
        "name",
        "run_id",
        "conversation_id",
        "session_id",
        "launch_id",
        "agent",
        "account_id",
        "cwd",
        "worktree_owner",
        "completion_source",
        "completion_reason",
        "exited_at",
        "exit_code",
        "termination_signal",
        "archive_id",
    ] {
        if current[key] != raw[key] {
            return Err(format!("Native {key} changed before spawn"));
        }
    }
    Ok(())
}
fn held_current(roots: &Roots, snapshot: &Snapshot) -> Result<Value> {
    Ok(if let Some(id) = &snapshot.archive {
        let file = roots
            .open_file(&["archives", &format!("{id}.json")])
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        use std::io::Read;
        file.take((1 << 20) + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 1 << 20 {
            return Err("Archive exceeds its bound".into());
        }
        serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())?
    } else {
        original_store::session_record(roots, &snapshot.name, &snapshot.run, &snapshot.conversation)
            .ok_or("The exact native record changed")?
    })
}
fn configure(cmd: &mut std::process::Command, config: &Config) {
    for var in [
        "HGS_SESSION",
        "HGS_SESSION_ID",
        "HGS_RUN_ID",
        "HGS_CONVERSATION_ID",
        "HGS_LAUNCH_ID",
        "HGS_LAUNCH_SESSION_ID",
        "HGS_ACCOUNT_ID",
        "HGS_PARENT",
        "TMUX",
        "TMUX_PANE",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("HGS_CONFIG_DIR", &config.dir)
        .env("HGS_STATE_DIR", &config.state);
}
fn custody(config: &Config, id: &str) -> Option<(Arc<InheritedLock>, PathBuf, (u64, u64))> {
    if !uuid::Uuid::parse_str(id).is_ok_and(|u| !u.is_nil() && u.to_string() == id) {
        return None;
    }
    let dir = config.state.join("teams/custody");
    crate::platform::private_dir(&dir).ok()?;
    if !crate::platform::verify_private(&dir, crate::platform::Private::Directory).ok()? {
        return None;
    }
    let path = dir.join(format!("{id}.lock"));
    let mut file = crate::platform::no_follow(crate::platform::private_open(
        OpenOptions::new().create_new(true).read(true).write(true),
    ))
    .open(&path)
    .ok()?;
    if !crate::platform::verify_private_file(&file).ok()? {
        return None;
    }
    let marker =
        json!({"kind":"teams_original","id":id,"configuration":config.dir,"state":config.state});
    file.write_all(marker.to_string().as_bytes()).ok()?;
    file.sync_all().ok()?;
    File::open(&dir).ok()?.sync_all().ok()?;
    let m = file.metadata().ok()?;
    Some((
        InheritedLock::try_lock(file).ok()?,
        path,
        (m.dev(), m.ino()),
    ))
}
fn remove(path: &std::path::Path, id: (u64, u64)) {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && (m.dev(), m.ino()) == id) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_final_record_read_does_not_follow_any_replaced_native_identity_or_exit() {
        let raw = json!({"name":"claude/example/member","run_id":"run-1","conversation_id":"conversation-1","session_id":"session-1","launch_id":"launch-1","agent":"claude","account_id":"account-1","cwd":"/workspace/member","worktree_owner":{"id":"nonce-1"},"completion_source":"supervisor","completion_reason":"clean_exit","exited_at":1000,"exit_code":0,"termination_signal":null,"archive_id":"archive-1"});
        unchanged_record(&raw, &raw).unwrap();
        for key in raw.as_object().unwrap().keys() {
            let mut current = raw.clone();
            current[key] = json!("changed");
            assert!(unchanged_record(&current, &raw).is_err(), "{key}");
        }
    }
    fn config(temp: &tempfile::TempDir) -> Config {
        Config {
            home: String::new(),
            dir: temp.path().join("config"),
            state: temp.path().join("state"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        }
    }
    /// Prune until the file is gone, within a bound. A child that another
    /// test thread forked while this lock existed holds a close-on-exec
    /// duplicate of it until that child execs, and pruning rightly keeps a
    /// held file for that moment.
    fn pruned_within(config: &Config, path: &std::path::Path, bound: Duration) -> bool {
        let deadline = Instant::now() + bound;
        loop {
            super::super::super::super::child::prune_custody(config);
            if !path.exists() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn original_custody_is_private_unique_and_not_reconstructed() {
        let temp = tempfile::tempdir().unwrap();
        let config = config(&temp);
        let id = uuid::Uuid::new_v4().to_string();
        let (held, path, identity) = custody(&config, &id).unwrap();
        assert!(crate::platform::verify_private(&path, crate::platform::Private::File).unwrap());
        assert!(custody(&config, &id).is_none());
        super::super::super::super::child::prune_custody(&config);
        assert!(path.exists());
        drop(held);
        assert!(pruned_within(&config, &path, Duration::from_secs(30)));
        remove(&path, identity);
    }
    /// The deterministic fork window: a child forked before its exec keeps
    /// the dropped lock held, so its file stays; after the exec it is pruned.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_child_forked_before_its_exec_keeps_released_custody_until_it_execs() {
        use std::os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::process::CommandExt,
        };
        let pipe = || {
            let mut fds = [-1; 2];
            assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
            unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
        };
        let temp = tempfile::tempdir().unwrap();
        let config = config(&temp);
        let id = uuid::Uuid::new_v4().to_string();
        let (held, path, identity) = custody(&config, &id).unwrap();
        let ((ready_read, ready_write), (release_read, release_write)) = (pipe(), pipe());
        let (ready, release) = (ready_write.as_raw_fd(), release_read.as_raw_fd());
        let forked = std::thread::spawn(move || {
            let mut command = std::process::Command::new("/bin/true");
            // SAFETY: only async-signal-safe write and read run after fork.
            unsafe {
                command.pre_exec(move || {
                    let byte = 0u8;
                    libc::write(ready, (&byte as *const u8).cast(), 1);
                    let mut byte = 0u8;
                    libc::read(release, (&mut byte as *mut u8).cast(), 1);
                    Ok(())
                });
            }
            command.spawn().unwrap().wait().unwrap();
            drop((ready_write, release_read));
        });
        let mut byte = 0u8;
        assert_eq!(
            unsafe { libc::read(ready_read.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) },
            1
        );
        drop(held);
        super::super::super::super::child::prune_custody(&config);
        assert!(path.exists(), "the forked child still holds the lock");
        assert_eq!(
            unsafe { libc::write(release_write.as_raw_fd(), (&byte as *const u8).cast(), 1) },
            1
        );
        forked.join().unwrap();
        assert!(pruned_within(&config, &path, Duration::from_secs(30)));
        remove(&path, identity);
    }
}
#[cfg(test)]
mod delivery;
