//! Bind the actor's original SQLite connection. Never open/close a raw Unix
//! descriptor of its live database: that would release SQLite's POSIX locks.
#[cfg(unix)]
use crate::tasks::controls::wire::canonical;
use crate::{config::Config, desktop::store::Location};
use anyhow::{ensure, Result};
use rusqlite::Connection;

pub(super) struct Store {
    configuration: String,
    #[cfg(unix)]
    state: std::path::PathBuf,
    #[cfg(unix)]
    config: std::path::PathBuf,
    #[cfg(unix)]
    path: std::path::PathBuf,
    #[cfg(unix)]
    identity: crate::sqlite_file::Identity,
    digest: String,
}
impl Store {
    pub fn capture(config: &Config, location: &Location, db: &Connection) -> Result<Self> {
        #[cfg(unix)]
        {
            location.check_existing()?;
            ensure!(
                Location::new(config)?.configuration == location.configuration
                    && zerus_desktop_migration::canonical(&config.state)?.join("desktop")
                        == location.root,
                "task-control configuration namespace changed"
            );
            let path = location.root.join("state.sqlite3");
            let identity = crate::sqlite_file::private(&path)?;
            crate::sqlite_file::bind_private(db, &path, identity)?;
            let digest = canonical::tagged_digest(
                b"zerus-task-control-store-unix-v1\0",
                &serde_json::json!({"schema":1,"configuration":location.configuration,
                    "device":identity.device,"inode":identity.inode}),
            )
            .map_err(anyhow::Error::msg)?;
            Ok(Self {
                configuration: location.configuration.clone(),
                state: config.state.clone(),
                config: config.dir.clone(),
                path,
                identity,
                digest,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (config, location, db);
            anyhow::bail!("Task-control store object binding is unqualified on this platform")
        }
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn verify(&self, db: &Connection, configuration: &str) -> Result<()> {
        ensure!(
            self.configuration == configuration,
            "task-control configuration changed"
        );
        #[cfg(unix)]
        {
            ensure!(
                zerus_desktop_migration::configuration(&self.state, &self.config)?
                    == self.configuration
                    && zerus_desktop_migration::canonical(&self.state)?.join("desktop")
                        == self.path.parent().unwrap(),
                "task-control configuration namespace changed"
            );
            Location {
                root: self.path.parent().unwrap().into(),
                configuration: self.configuration.clone(),
            }
            .check_existing()?;
            crate::sqlite_file::bind_private(db, &self.path, self.identity)?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = db;
            anyhow::bail!("Task-control store object binding is unqualified on this platform")
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Config, Location, Connection) {
        let dir = tempfile::tempdir().unwrap();
        crate::platform::restrict_dir(dir.path()).unwrap();
        let config = Config {
            home: dir.path().to_string_lossy().into(),
            state: dir.path().join("state"),
            dir: dir.path().join("config"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        std::fs::create_dir(&config.state).unwrap();
        crate::platform::restrict_dir(&config.state).unwrap();
        std::fs::create_dir(&config.dir).unwrap();
        crate::platform::restrict_dir(&config.dir).unwrap();
        let location = Location::new(&config).unwrap();
        location.prepare().unwrap();
        let path = location.root.join("state.sqlite3");
        crate::sqlite_file::create(&path).unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE original(id)")
            .unwrap();
        (dir, config, location, db)
    }
    #[test]
    fn binding_uses_the_original_vfs_connection_without_releasing_its_locks() {
        let (_dir, config, location, db) = fixture();
        let guard = Store::capture(&config, &location, &db).unwrap();
        db.execute_batch("BEGIN IMMEDIATE; INSERT INTO original VALUES(1)")
            .unwrap();
        assert!(crate::sqlite_file::foreign_lock_refused(
            &location.root.join("state.sqlite3")
        ));
        for _ in 0..3 {
            guard.verify(&db, &location.configuration).unwrap();
            assert_eq!(
                Store::capture(&config, &location, &db).unwrap().digest(),
                guard.digest()
            );
            assert!(crate::sqlite_file::foreign_lock_refused(
                &location.root.join("state.sqlite3")
            ));
        }
        assert!(guard.verify(&db, &"b".repeat(64)).is_err());
        db.execute_batch("ROLLBACK").unwrap();
    }
    #[test]
    fn a_replaced_database_or_a_connection_to_another_file_never_borrows_the_guard() {
        let (_dir, config, location, db) = fixture();
        let guard = Store::capture(&config, &location, &db).unwrap();
        let other = Connection::open_in_memory().unwrap();
        assert!(guard.verify(&other, &location.configuration).is_err());
        let path = location.root.join("state.sqlite3");
        std::fs::rename(&path, location.root.join("retained.sqlite3")).unwrap();
        crate::sqlite_file::create(&path).unwrap();
        assert!(guard.verify(&db, &location.configuration).is_err());
        assert!(Store::capture(&config, &location, &db).is_err());
    }
}
