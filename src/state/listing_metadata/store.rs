use super::*;
use fs2::FileExt;
pub(super) struct Store {
    pub db: Connection,
    pub root: Directory,
    selected: PathBuf,
    dir: Directory,
    file: File,
    _gate: File,
}
impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let root = checked(Directory::open(&crate::platform::canonical(path)?))?;
        let dir = checked(root.ensure_child("listing-metadata"))?;
        anyhow::ensure!(
            !dir.path.join("disabled").try_exists()?,
            "Display index is disabled"
        );
        let gate = checked(dir.open_private_lock("writer.lock"))?;
        gate.try_lock_exclusive()?;
        let (file, new) = match checked(dir.read_private_optional("index.sqlite3"))? {
            Some(f) => (f, false),
            None => {
                anyhow::ensure!(
                    checked(dir.read_private_optional("index.sqlite3-wal"))?.is_none()
                        && checked(dir.read_private_optional("index.sqlite3-shm"))?.is_none(),
                    "Orphaned display journal files"
                );
                (checked(dir.write_new_file("index.sqlite3", &[]))?, true)
            }
        };
        let db = Connection::open_with_flags(
            dir.path.join("index.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        db.busy_timeout(Duration::ZERO)?;
        if new {
            db.execute_batch("PRAGMA synchronous=FULL; PRAGMA max_page_count=131072;")?;
            db.execute_batch("PRAGMA user_version=1; PRAGMA journal_mode=WAL;
              CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL,root TEXT NOT NULL,epoch TEXT NOT NULL,writes INTEGER NOT NULL DEFAULT 0,revision INTEGER NOT NULL DEFAULT 0,generation INTEGER NOT NULL DEFAULT 0,serial INTEGER NOT NULL DEFAULT 0,ready INTEGER NOT NULL DEFAULT 0);
              CREATE TABLE sources(source TEXT PRIMARY KEY,seed TEXT,stamp TEXT,revision INTEGER NOT NULL,tombstone INTEGER NOT NULL CHECK(tombstone IN(0,1)),issue TEXT);
              CREATE TABLE intents(id TEXT PRIMARY KEY,source TEXT NOT NULL,seed TEXT,created REAL NOT NULL);
              CREATE INDEX intents_source ON intents(source);
              CREATE INDEX sources_tombstone ON sources(tombstone,source);
              CREATE INDEX sources_name ON sources(json_extract(seed,'$.record.name')) WHERE tombstone=0;
              CREATE TABLE scan(id INTEGER PRIMARY KEY CHECK(id=1),generation INTEGER NOT NULL,phase INTEGER NOT NULL,folder INTEGER NOT NULL,after_key TEXT NOT NULL,dirs TEXT NOT NULL,writes INTEGER NOT NULL,gap TEXT NOT NULL,problems INTEGER NOT NULL);
              CREATE TABLE queue(generation INTEGER NOT NULL,source TEXT NOT NULL,PRIMARY KEY(generation,source));
              CREATE TABLE snapshots(generation INTEGER NOT NULL,source TEXT NOT NULL,seed TEXT NOT NULL,PRIMARY KEY(generation,source));
              CREATE INDEX snapshots_journal ON snapshots(generation,coalesce(json_extract(seed,'$.record.journal_name'),json_extract(seed,'$.record.name')));
              CREATE TABLE generations(generation INTEGER PRIMARY KEY,complete INTEGER NOT NULL,dirs TEXT NOT NULL,gap TEXT NOT NULL);
              CREATE INDEX generations_complete ON generations(complete,generation);")?;
            db.execute(
                "INSERT INTO meta(id,version,root,epoch) VALUES(1,1,?1,?2)",
                params![
                    serde_json::to_string(&checked(root.object())?)?,
                    uuid::Uuid::new_v4().to_string()
                ],
            )?;
        }
        let identity: String =
            db.query_row("SELECT root FROM meta WHERE id=1 AND version=1", [], |r| {
                r.get(0)
            })?;
        anyhow::ensure!(
            identity == serde_json::to_string(&checked(root.object())?)?,
            "Display root/store identity changed"
        );
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        anyhow::ensure!(version == 1, "Unknown display cache version");
        if !new {
            db.execute_batch("PRAGMA synchronous=FULL; PRAGMA max_page_count=131072;")?;
        }
        let store = Self {
            db,
            root,
            selected: path.to_path_buf(),
            dir,
            file,
            _gate: gate,
        };
        store.verify()?;
        Ok(store)
    }
    pub fn verify(&self) -> anyhow::Result<()> {
        checked(self.root.verify())?;
        if self.selected != self.root.path {
            anyhow::ensure!(
                crate::platform::canonical(&self.selected)? == self.root.path,
                "Selected display root changed"
            );
        }
        checked(self.dir.verify())?;
        let current = checked(self.dir.read_private("index.sqlite3"))?;
        let held = crate::platform::FileIdentity::of(&self.file)?;
        let at = crate::platform::FileIdentity::of(&current)?;
        anyhow::ensure!(
            (held.device, held.inode) == (at.device, at.inode),
            "Display index namespace changed"
        );
        Ok(())
    }
    pub fn reserve(&mut self, key: &Key, seed: Option<&str>) -> anyhow::Result<String> {
        self.verify()?;
        key.validate()?;
        let id = uuid::Uuid::new_v4().to_string();
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("UPDATE meta SET writes=writes+1,ready=0 WHERE id=1", [])?;
        let outstanding: i64 = tx.query_row("SELECT count(*) FROM intents", [], |r| r.get(0))?;
        anyhow::ensure!(outstanding < 4096, "Display intent capacity exceeded");
        tx.execute(
            "INSERT INTO intents VALUES(?1,?2,?3,?4)",
            params![id, key.0, seed, now()],
        )?;
        tx.commit()?;
        self.verify()?;
        Ok(id)
    }
    pub fn settle(&mut self, id: &str, stamp: Option<&str>) -> anyhow::Result<()> {
        self.verify()?;
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (source, seed): (String, Option<String>) =
            tx.query_row("SELECT source,seed FROM intents WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        tx.execute("UPDATE meta SET revision=revision+1 WHERE id=1", [])?;
        tx.execute("INSERT INTO sources(source,seed,stamp,revision,tombstone,issue) VALUES(?1,?2,?3,(SELECT revision FROM meta WHERE id=1),?4,NULL)
          ON CONFLICT(source) DO UPDATE SET seed=excluded.seed,stamp=excluded.stamp,revision=excluded.revision,tombstone=excluded.tombstone,issue=NULL",params![source,seed,stamp,i64::from(stamp.is_none())])?;
        tx.execute("DELETE FROM intents WHERE id=?1", [id])?;
        tx.commit()?;
        self.verify()
    }
    pub fn source_parent(&self, key: &Key) -> anyhow::Result<(Directory, String)> {
        key.validate()?;
        let mut parent = checked(Directory::open(&self.root.path))?;
        let path = Path::new(&key.0);
        for part in path.parent().unwrap().components() {
            parent = checked(
                parent.child(
                    part.as_os_str()
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("Invalid display source"))?,
                ),
            )?;
        }
        Ok((parent, path.file_name().unwrap().to_str().unwrap().into()))
    }
    pub fn source_stamp(&self, key: &Key) -> anyhow::Result<Option<String>> {
        let (parent, name) = self.source_parent(key)?;
        let file = checked(parent.read_private_optional(&name))?;
        let stamp = file.as_ref().map(version).transpose()?;
        checked(parent.verify())?;
        Ok(stamp)
    }
}
pub(super) fn version(file: &File) -> anyhow::Result<String> {
    let (id, m) = crate::platform::FileIdentity::with_metadata(file)?;
    stamp(&id, &m)
}
pub(super) fn publication_version(file: &File) -> anyhow::Result<(String, String)> {
    let (id, m) = crate::platform::FileIdentity::with_metadata(file)?;
    let stamp = stamp(&id, &m)?;
    // Rename can change ctime. The private written object's identity, length
    // and mtime must remain unchanged across its publication, before a full
    // post-publication stamp is certified. Both come from this one fstat.
    let stable =
        serde_json::to_string(&(id.device, id.inode, id.modified, id.modified_nanos, m.len()))?;
    Ok((stable, stamp))
}
fn stamp(id: &crate::platform::FileIdentity, m: &std::fs::Metadata) -> anyhow::Result<String> {
    anyhow::ensure!(m.is_file(), "Display source is not regular");
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        (m.ctime(), m.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = (0, 0);
    Ok(serde_json::to_string(&(
        id.device,
        id.inode,
        id.modified,
        id.modified_nanos,
        m.len(),
        changed,
    ))?)
}

impl Store {
    /// Recover only this component's disposable cache, never canonical files.
    /// One retained cache set bounds quarantine storage across repeated faults.
    pub fn recover(path: &Path, error: &anyhow::Error) -> anyhow::Result<bool> {
        let eligible = match error.downcast_ref::<rusqlite::Error>() {
            Some(rusqlite::Error::SqliteFailure(code, message)) => {
                let code = code.extended_code & 0xff;
                code == rusqlite::ffi::SQLITE_CORRUPT
                    || code == rusqlite::ffi::SQLITE_NOTADB
                    || (code == rusqlite::ffi::SQLITE_ERROR
                        && message
                            .as_deref()
                            .is_some_and(|m| m.starts_with("no such table:")))
            }
            Some(rusqlite::Error::QueryReturnedNoRows) => true,
            _ => matches!(
                error.to_string().as_str(),
                "Display root/store identity changed" | "Orphaned display journal files"
            ),
        };
        if !eligible {
            return Ok(false);
        }
        let root = checked(Directory::open(&crate::platform::canonical(path)?))?;
        let dir = checked(root.child("listing-metadata"))?;
        anyhow::ensure!(
            !dir.path.join("disabled").try_exists()?,
            "Display index is disabled"
        );
        let gate = checked(dir.open_private_lock("writer.lock"))?;
        gate.try_lock_exclusive()?;
        if checked(dir.read_private_optional("index.sqlite3"))?.is_some() {
            if let Ok(db) = Connection::open_with_flags(
                dir.path.join("index.sqlite3"),
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            ) {
                if let Ok(version) = db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                {
                    anyhow::ensure!(
                        matches!(version, 0 | 1),
                        "Unknown display cache version; rebuild refused"
                    );
                }
            }
        }
        for name in ["index.sqlite3", "index.sqlite3-wal", "index.sqlite3-shm"] {
            checked(root.verify())?;
            checked(dir.verify())?;
            let Some(file) = checked(dir.read_private_optional(name))? else {
                continue;
            };
            let backup = format!("retained-{name}");
            if checked(dir.read_private_optional(&backup))?.is_some() {
                std::fs::remove_file(dir.path.join(&backup))?;
            }
            let current = checked(dir.read_private(name))?;
            anyhow::ensure!(
                crate::platform::FileIdentity::of(&file)?
                    == crate::platform::FileIdentity::of(&current)?,
                "Display cache changed before rebuild"
            );
            std::fs::rename(dir.path.join(name), dir.path.join(backup))?;
            crate::platform::sync_dir(&dir.path)?;
        }
        checked(root.verify())?;
        checked(dir.verify())?;
        Ok(true)
    }
}
