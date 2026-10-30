//! The service actor alone owns this connection and its transactional schema.
use crate::{config::Config, platform};
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use zerus_desktop_migration::{canonical, configuration, HANDOFF_LOCK};

pub const SCHEMA: i64 = 5;
pub struct HandoffLock {
    _lock: platform::FileLock,
    root: PathBuf,
    configuration: String,
}
impl HandoffLock {
    pub fn verify(&self, location: &Location) -> Result<()> {
        ensure!(
            self.root == location.root && self.configuration == location.configuration,
            "handoff lock belongs to another configuration"
        );
        Ok(())
    }
}
pub struct Location {
    pub root: PathBuf,
    pub configuration: String,
}
impl Location {
    pub fn new(config: &Config) -> Result<Self> {
        let state = canonical(&config.state)?;
        Ok(Self {
            root: state.join("desktop"),
            configuration: configuration(&config.state, &config.dir)?,
        })
    }
    pub fn prepare(&self) -> Result<()> {
        for path in [
            self.root.parent().context("missing state folder")?,
            &self.root,
        ] {
            platform::private_dir(path)?;
            ensure!(
                platform::verify_private(path, platform::Private::Directory)?,
                "desktop folders must be private directories"
            );
        }
        Ok(())
    }
    pub fn check_existing(&self) -> Result<()> {
        for path in [
            self.root.parent().context("missing state folder")?,
            &self.root,
        ] {
            match platform::verify_private(path, platform::Private::Directory) {
                Ok(true) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                _ => anyhow::bail!("desktop folders must be private directories"),
            }
        }
        Ok(())
    }
    pub fn lock(&self) -> Result<platform::FileLock> {
        let file = private_file(&self.root.join("owner.lock"), true)?;
        fs2::FileExt::try_lock_exclusive(&file)
            .context("another desktop service owns this state")?;
        Ok(platform::FileLock::held(file))
    }
    /// Read-only preflight must not create desktop state on a never-used host.
    /// An existing owner still has to be excluded before returning a no-op.
    pub fn existing_lock(&self) -> Result<Option<platform::FileLock>> {
        self.check_existing()?;
        let file = match private_file(&self.root.join("owner.lock"), false) {
            Ok(file) => file,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(None)
            }
            Err(error) => return Err(error),
        };
        fs2::FileExt::try_lock_exclusive(&file)
            .context("another desktop service owns this state")?;
        Ok(Some(platform::FileLock::held(file)))
    }
    /// Passive inspection releases this immediately. Migrated writers will
    /// retain it for their full lifetime; the service process lock is separate.
    pub fn handoff_lock(&self) -> Result<HandoffLock> {
        self.check_existing()?;
        let file = private_file(&self.root.join(HANDOFF_LOCK), true)?;
        fs2::FileExt::try_lock_exclusive(&file).context("desktop handoff is busy")?;
        Ok(HandoffLock {
            _lock: platform::FileLock::held(file),
            root: self.root.clone(),
            configuration: self.configuration.clone(),
        })
    }
}

fn private_file(path: &Path, create: bool) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    platform::private_open(
        options
            .read(true)
            .write(true)
            .create(create)
            .truncate(false),
    );
    #[cfg(unix)]
    platform::no_follow(&mut options);
    #[cfg(windows)]
    platform::refuse_link(&mut options);
    let file = options.open(path)?;
    ensure!(
        platform::verify_private_file(&file)?,
        "desktop state must be private single-link regular files"
    );
    Ok(file)
}

/// Reused before opening SQLite in either the actor or a legacy admission probe.
fn database_files(location: &Location) -> Result<()> {
    for name in [
        "state.sqlite3",
        "state.sqlite3-wal",
        "state.sqlite3-shm",
        "state.sqlite3-journal",
    ] {
        match platform::verify_private(&location.root.join(name), platform::Private::File) {
            Ok(true) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => anyhow::bail!("desktop database files must be private single-link regular files"),
        }
    }
    Ok(())
}

/// Read one indexed policy row, without migration, restart or authority changes.
/// None means no database; schema one predates effect owners and stays passive.
pub(super) fn writer_active(location: &Location) -> Result<Option<bool>> {
    location.check_existing()?;
    match fs::symlink_metadata(location.root.join("state.sqlite3")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        result => {
            result?;
        }
    }
    database_files(location)?;
    let db = Connection::open_with_flags(
        location.root.join("state.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    db.busy_timeout(Duration::from_millis(250))?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!((1..=SCHEMA).contains(&version), "unknown desktop schema");
    let configuration: String = db.query_row(
        "SELECT substr(configuration,1,65) FROM service WHERE id=1",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        configuration == location.configuration,
        "desktop configuration mismatch"
    );
    if version == 1 {
        return Ok(Some(false));
    }
    let revoked: i64 = db.query_row("SELECT revoked FROM desktop_writer WHERE id=1", [], |r| {
        r.get(0)
    })?;
    ensure!(matches!(revoked, 0 | 1), "invalid desktop writer policy");
    Ok(Some(revoked == 0))
}

pub struct Store {
    pub(super) db: Connection,
    pub generation: u64,
    recovery_error: bool,
    read_recovery_error: bool,
    catalog_collection_error: bool,
}
impl Store {
    pub fn open(location: &Location, epoch: &str) -> Result<Self> {
        let path = location.root.join("state.sqlite3");
        database_files(location)?;
        let flags = OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        // A newer schema is probed read-only before any PRAGMA or migration write.
        if path.try_exists()? {
            let probe =
                Connection::open_with_flags(&path, flags | OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let version: i64 = probe.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            ensure!(
                (0..=SCHEMA).contains(&version),
                "desktop state requires a newer hgs version"
            );
        }
        let _file = private_file(&path, true)?;
        let mut db = Connection::open_with_flags(&path, flags | OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        db.busy_timeout(Duration::from_secs(2))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            let tables: i64 = db.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )?;
            ensure!(tables == 0, "unversioned desktop state is not empty");
            let tx = db.transaction()?;
            tx.execute_batch("CREATE TABLE service (id INTEGER PRIMARY KEY CHECK(id=1), configuration TEXT NOT NULL, generation INTEGER NOT NULL, epoch TEXT NOT NULL); PRAGMA user_version=1;")?;
            tx.execute(
                "INSERT INTO service VALUES(1,?1,0,'')",
                [&location.configuration],
            )?;
            tx.commit()?;
        }
        let configuration: String =
            db.query_row("SELECT configuration FROM service WHERE id=1", [], |row| {
                row.get(0)
            })?;
        ensure!(
            configuration == location.configuration,
            "desktop state belongs to another configuration"
        );
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA max_page_count=131072; PRAGMA journal_size_limit=67108864;")?;
        if version < 2 {
            let tx = db.transaction()?;
            zerus_desktop_ledger::migrate(&tx)?;
            tx.execute_batch("PRAGMA user_version=2;")?;
            tx.commit()?;
        }
        if version < 3 {
            let tx = db.transaction()?;
            tx.execute_batch(
                "CREATE TABLE desktop_commands (
                request_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL,
                reason TEXT NOT NULL CHECK(reason IN('manual','rollback')),
                writer_epoch TEXT, state TEXT NOT NULL CHECK(state IN('prepared','settled')),
                result TEXT, at INTEGER NOT NULL);
                PRAGMA user_version=3;",
            )?;
            tx.commit()?;
        }
        if version < 4 {
            let tx = db.transaction()?;
            zerus_desktop_ledger::reads::migrate(&tx)?;
            tx.execute_batch("PRAGMA user_version=4;")?;
            tx.commit()?;
        }
        if version < 5 {
            let tx = db.transaction()?;
            zerus_desktop_catalog::migrate(&tx)?;
            tx.execute_batch("PRAGMA user_version=5;")?;
            tx.commit()?;
        }
        zerus_desktop_catalog::begin_service(&mut db, epoch)?;
        // No marker or previous policy re-enables a worker after restart.
        zerus_desktop_ledger::begin_service(&mut db, epoch)?;
        zerus_desktop_ledger::recover_batch(&mut db, epoch)?;
        let read_recovery_error = zerus_desktop_ledger::reads::recover(
            &mut db,
            epoch,
            chrono::Utc::now().timestamp_millis(),
        )
        .is_err();
        let tx = db.transaction()?;
        tx.execute(
            "UPDATE service SET generation=generation+1, epoch=?1 WHERE id=1",
            [epoch],
        )?;
        let generation = tx.query_row("SELECT generation FROM service WHERE id=1", [], |row| {
            row.get(0)
        })?;
        tx.commit()?;
        platform::sync_dir(&location.root)?;
        Ok(Self {
            db,
            generation,
            recovery_error: false,
            read_recovery_error,
            catalog_collection_error: false,
        })
    }
    pub fn checkpoint(&self) -> Result<()> {
        let (busy, _, _): (i64, i64, i64) =
            self.db
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
        ensure!(
            busy == 0,
            "desktop checkpoint is busy; revocation is not settled"
        );
        Ok(())
    }
    pub fn effect_status(&self) -> serde_json::Value {
        let mut status = zerus_desktop_ledger::status(&self.db).unwrap_or_else(|_| serde_json::json!({"revoked":null,"admissions_paused":true,"error":"ledger_unavailable"}));
        if self.recovery_error {
            status["admissions_paused"] = true.into();
            status["error"] = "ledger_recovery_failed".into();
        }
        status
    }
    pub fn read_status(&self) -> serde_json::Value {
        let mut status = zerus_desktop_ledger::reads::status(&self.db).unwrap_or_else(|_| {
            serde_json::json!({"pending":null,"in_flight":null,"error":"read_queue_unavailable"})
        });
        status["admissions_paused"] = true.into();
        status["recovery_error"] = self.read_recovery_error.into();
        status
    }
    pub fn catalog_status(&self) -> serde_json::Value {
        match zerus_desktop_catalog::totals(&self.db) {
            Ok(totals) => serde_json::json!({
                "retained_sources":totals.retained_sources,"stored_rows":totals.stored_rows,
                "stored_bytes":totals.stored_bytes,"obsolete_generations":totals.obsolete_generations,
                "admissions_paused":true,"collection_error":self.catalog_collection_error,
            }),
            Err(_) => serde_json::json!({
                "retained_sources":null,"stored_rows":null,"stored_bytes":null,
                "obsolete_generations":null,"admissions_paused":true,
                "collection_error":self.catalog_collection_error,"error":"catalog_unavailable",
            }),
        }
    }
    /// One indexed batch, never a loop over all retained rows. False backs off
    /// both idle and failed collection; failure affects only this component.
    pub fn collect_catalog(&mut self) -> bool {
        match zerus_desktop_catalog::collect(&mut self.db) {
            Ok(progress) => {
                self.catalog_collection_error = false;
                progress.is_some()
            }
            Err(_) => {
                self.catalog_collection_error = true;
                false
            }
        }
    }
    pub fn recover_effects(&mut self, epoch: &str) {
        self.recovery_error = zerus_desktop_ledger::recover_batch(&mut self.db, epoch).is_err();
        self.read_recovery_error = zerus_desktop_ledger::reads::recover(
            &mut self.db,
            epoch,
            chrono::Utc::now().timestamp_millis(),
        )
        .is_err();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    #[test]
    fn catalog_collection_yields_and_failure_is_local_and_recoverable() {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: canonical(&temp.path().join("state/desktop")).unwrap(),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let _owner = location.lock().unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        let mut store = Store::open(&location, &epoch).unwrap();
        let baseline = zerus_desktop_ledger::put_baseline(
            &mut store.db,
            &zerus_desktop_ledger::Identity {
                kind: zerus_desktop_ledger::Kind::Post,
                target: zerus_desktop_ledger::Target::Session {
                    computer: "peer".into(),
                    run: "run-1".into(),
                    conversation: "conversation-1".into(),
                },
                edge: "edge-1".into(),
            },
            1,
            &serde_json::json!({"token":"synthetic"}),
        )
        .unwrap();
        zerus_desktop_ledger::reads::enqueue(
            &mut store.db,
            &uuid::Uuid::new_v4().to_string(),
            &zerus_desktop_ledger::reads::ReadMark {
                computer: "peer".into(),
                name: "codex/example/session".into(),
                run_id: "run-1".into(),
                conversation_id: "conversation-1".into(),
                reply_id: Some("reply-1".into()),
                attention_signature: None,
            },
            0,
        )
        .unwrap();
        let tx = store.db.transaction().unwrap();
        tx.execute(
            "INSERT INTO desktop_catalog_generations(computer,obsolete) VALUES('peer',1)",
            [],
        )
        .unwrap();
        let obsolete = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO desktop_catalog_generations(computer) VALUES('peer')",
            [],
        )
        .unwrap();
        let visible = tx.last_insert_rowid();
        for n in 0..385 {
            tx.execute(
                "INSERT INTO desktop_catalog_rows(generation,key,json,bytes) VALUES(?1,?2,'{}',64)",
                params![
                    if n == 384 { visible } else { obsolete },
                    format!("run:{n}")
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert!(store.collect_catalog());
        assert_eq!(store.catalog_status()["stored_rows"], 129);
        assert!(store.collect_catalog());
        assert_eq!(store.catalog_status()["stored_rows"], 1);
        assert_eq!(store.catalog_status()["obsolete_generations"], 0);
        assert!(!store.collect_catalog());
        store
            .db
            .execute_batch(
                "ALTER TABLE desktop_catalog_generations RENAME TO broken_catalog_generations",
            )
            .unwrap();
        assert!(!store.collect_catalog());
        assert_eq!(store.catalog_status()["collection_error"], true);
        assert_eq!(store.catalog_status()["error"], "catalog_unavailable");
        assert_eq!(store.read_status()["pending"], 1);
        assert!(store.effect_status()["error"].is_null());
        assert!(zerus_desktop_ledger::current(&store.db, &baseline, 1).unwrap());
        store
            .db
            .execute_batch(
                "ALTER TABLE broken_catalog_generations RENAME TO desktop_catalog_generations",
            )
            .unwrap();
        assert!(!store.collect_catalog());
        assert_eq!(store.catalog_status()["collection_error"], false);
        assert_eq!(store.catalog_status()["stored_rows"], 1);
    }
    #[test]
    fn read_queue_recovers_under_a_new_service_epoch_and_fails_locally() {
        use zerus_desktop_ledger::reads;
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: canonical(&temp.path().join("state/desktop")).unwrap(),
            configuration: "fixture".into(),
        };
        location.prepare().unwrap();
        let _owner = location.lock().unwrap();
        let first = uuid::Uuid::new_v4().to_string();
        let mut store = Store::open(&location, &first).unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        zerus_desktop_ledger::authorize(&mut store.db, &epoch, &first).unwrap();
        let mark = reads::ReadMark {
            computer: "peer".into(),
            name: "session".into(),
            run_id: "run".into(),
            conversation_id: "conversation".into(),
            reply_id: Some("reply".into()),
            attention_signature: None,
        };
        reads::enqueue(&mut store.db, &uuid::Uuid::new_v4().to_string(), &mark, 0).unwrap();
        let sent = reads::reserve(
            &mut store.db,
            &epoch,
            &first,
            &uuid::Uuid::new_v4().to_string(),
            0,
        )
        .unwrap()
        .unwrap();
        drop(store);
        let second = uuid::Uuid::new_v4().to_string();
        let mut store = Store::open(&location, &second).unwrap();
        assert_eq!(store.read_status()["pending"], 1);
        assert_eq!(store.read_status()["in_flight"], 0);
        assert_eq!(store.read_status()["admissions_paused"], true);
        assert!(reads::finish(&mut store.db, &sent, reads::Outcome::NotSent, 0).is_err());
        // A fresh in-flight job lets recovery exercise its indexed lookup.
        zerus_desktop_ledger::authorize(&mut store.db, &epoch, &second).unwrap();
        reads::reserve(
            &mut store.db,
            &epoch,
            &second,
            &uuid::Uuid::new_v4().to_string(),
            i64::MAX,
        )
        .unwrap()
        .unwrap();
        store
            .db
            .execute_batch("ALTER TABLE desktop_read_attempts RENAME TO broken_read_attempts")
            .unwrap();
        store.recover_effects(&second);
        assert_eq!(store.read_status()["recovery_error"], true);
        assert!(store.effect_status()["error"].is_null());
        store
            .db
            .execute_batch("ALTER TABLE broken_read_attempts RENAME TO desktop_read_attempts")
            .unwrap();
        store.recover_effects(&second);
        assert_eq!(store.read_status()["recovery_error"], false);
    }
    #[test]
    fn ledger_recovery_failure_is_component_local_and_clears_after_repair() {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: canonical(&temp.path().join("state/desktop")).unwrap(),
            configuration: "fixture".into(),
        };
        location.prepare().unwrap();
        let _owner = location.lock().unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        let mut store = Store::open(&location, &epoch).unwrap();
        let other = Connection::open(location.root.join("state.sqlite3")).unwrap();
        other.execute_batch("BEGIN IMMEDIATE").unwrap();
        store.recover_effects(&epoch);
        assert_eq!(store.effect_status()["error"], "ledger_recovery_failed");
        assert_eq!(store.effect_status()["revoked"], true);
        other.execute_batch("ROLLBACK").unwrap();
        store.recover_effects(&epoch);
        assert!(store.effect_status()["error"].is_null());
        assert_eq!(store.effect_status()["attempts"]["unknown"], 0);
    }
    #[test]
    #[cfg(unix)]
    fn handoff_lock_never_creates_a_leaf_through_a_replaced_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("desktop");
        let foreign = temp.path().join("foreign");
        platform::private_dir(&foreign).unwrap();
        std::os::unix::fs::symlink(&foreign, &root).unwrap();
        let location = Location {
            root,
            configuration: "fixture".into(),
        };
        assert!(location.handoff_lock().is_err());
        assert!(!foreign.join(HANDOFF_LOCK).exists());
    }
    #[test]
    fn canonical_aliases_share_the_os_lock() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("desktop");
        platform::private_dir(&root).unwrap();
        let location = Location {
            root,
            configuration: "fixture".into(),
        };
        let owner = location.lock().unwrap();
        assert!(location.lock().is_err());
        drop(owner);
        assert!(location.lock().is_ok());
        #[cfg(unix)]
        {
            let alias = temp.path().join("alias");
            std::os::unix::fs::symlink(&location.root, &alias).unwrap();
            assert_eq!(
                canonical(&alias).unwrap(),
                canonical(&location.root).unwrap()
            );
        }
    }
}
