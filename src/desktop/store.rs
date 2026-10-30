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

pub const SCHEMA: i64 = 1;
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
        let file = private_file(&self.root.join("owner.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file)
            .context("another desktop service owns this state")?;
        Ok(platform::FileLock::held(file))
    }
    /// Passive inspection releases this immediately. Migrated writers will
    /// retain it for their full lifetime; the service process lock is separate.
    pub fn handoff_lock(&self) -> Result<platform::FileLock> {
        self.check_existing()?;
        let file = private_file(&self.root.join(HANDOFF_LOCK))?;
        fs2::FileExt::try_lock_exclusive(&file).context("desktop handoff is busy")?;
        Ok(platform::FileLock::held(file))
    }
}

fn private_file(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    platform::private_open(options.read(true).write(true).create(true).truncate(false));
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

pub struct Store {
    db: Connection,
    pub generation: u64,
}
impl Store {
    pub fn open(location: &Location, epoch: &str) -> Result<Self> {
        let path = location.root.join("state.sqlite3");
        // Check every existing SQLite leaf before SQLite can open its sidecars.
        for name in [
            "state.sqlite3",
            "state.sqlite3-wal",
            "state.sqlite3-shm",
            "state.sqlite3-journal",
        ] {
            match platform::verify_private(&location.root.join(name), platform::Private::File) {
                Ok(true) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                _ => anyhow::bail!(
                    "desktop database files must be private single-link regular files"
                ),
            }
        }
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
        let _file = private_file(&path)?;
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
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA max_page_count=131072; PRAGMA journal_size_limit=67108864;")?;
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
        Ok(Self { db, generation })
    }
    pub fn checkpoint(&self) -> Result<()> {
        self.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
