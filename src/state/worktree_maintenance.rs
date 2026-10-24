//! Host-local policy and indexed, bounded maintenance of proven owned checkouts.
//! The queue is advisory; removal always repeats the native guards. An intent
//! reserved before removal stays unconfirmed across crashes and is never replayed.
use super::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions, ReadDir};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Policy {
    version: u32,
    revision: u64,
    pub enabled: bool,
    pub after_session: bool,
    pub unused_integrated: bool,
    pub missing: bool,
    pub grace_hours: u32,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            enabled: true,
            after_session: true,
            unused_integrated: true,
            missing: true,
            grace_hours: 24,
        }
    }
}
fn directory() -> PathBuf {
    root().join("worktree-maintenance")
}
pub(super) fn policy() -> Result<Policy> {
    let path = directory().join("policy.json");
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Policy::default()),
        Err(e) => return Err(e.to_string()),
    };
    let value: Policy = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&value)?;
    Ok(value)
}
fn validate(p: &Policy) -> Result<()> {
    if p.version != 1 || !(1..=8760).contains(&p.grace_hours) || p.revision >= 9_000_000_000_000_000
    {
        return Err(
            "Invalid worktree cleanup policy version, revision or grace period (1–8760 hours)"
                .into(),
        );
    }
    Ok(())
}
fn database() -> Result<Connection> {
    private_dir(&directory())?;
    let path = directory().join("queue.sqlite3");
    // Create privately before SQLite opens it, and refuse links/foreign files.
    match crate::platform::private_open(OpenOptions::new().write(true).create_new(true)).open(&path)
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    let file = crate::platform::refuse_link(OpenOptions::new().read(true))
        .open(&path)
        .map_err(|e| e.to_string())?;
    if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())? {
        return Err("Worktree maintenance database must be private".into());
    }
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_millis(200))
        .map_err(|e| e.to_string())?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS checkouts (
        path TEXT PRIMARY KEY, common_dir TEXT NOT NULL, owner TEXT NOT NULL,
        next_check REAL NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'pending', detail TEXT NOT NULL DEFAULT '');
        CREATE INDEX IF NOT EXISTS due ON checkouts(next_check,path) WHERE status != 'unconfirmed';")
        .map_err(|e| e.to_string())?;
    Ok(db)
}
pub(super) fn register(owner: &Value) -> Result<()> {
    let db = database()?;
    register_in(&db, owner)
}
fn register_in(db: &Connection, owner: &Value) -> Result<()> {
    // Importing old receipts is discovery only, never ownership authority.
    if owner["version"] != 1
        || !Path::new(string(owner, "path")).is_absolute()
        || !Path::new(string(owner, "common_dir")).is_absolute()
        || uuid::Uuid::parse_str(string(owner, "id")).is_err()
        || !Path::new(string(owner, "metadata")).is_dir()
    {
        return Ok(());
    }
    db.execute(
        "INSERT INTO checkouts(path,common_dir,owner) VALUES (?1,?2,?3)
        ON CONFLICT(path) DO UPDATE SET common_dir=excluded.common_dir,owner=excluded.owner,
        next_check=0,status='pending',detail='' WHERE checkouts.owner != excluded.owner",
        params![
            string(owner, "path"),
            string(owner, "common_dir"),
            string(owner, "id")
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// One import pass keeps its directory iterator between rounds. Neither import
/// nor queue reads materialize all receipts or all session journals in memory.
#[derive(Default)]
pub(super) struct Sweep {
    receipts: Option<ReadDir>,
    imported: bool,
}
impl Sweep {
    fn import(&mut self, db: &Connection) -> Result<()> {
        if self.imported {
            return Ok(());
        }
        if self.receipts.is_none() {
            match fs::read_dir(root().join("worktree-cache")) {
                Ok(entries) => self.receipts = Some(entries),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    self.imported = true;
                    return Ok(());
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        for _ in 0..64 {
            let Some(entry) = self.receipts.as_mut().unwrap().next() else {
                self.imported = true;
                self.receipts = None;
                break;
            };
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_name().to_string_lossy().starts_with("owned-") {
                continue;
            }
            if !entry.file_type().is_ok_and(|kind| kind.is_file())
                || !entry.metadata().is_ok_and(|m| m.len() <= 16384)
            {
                continue;
            }
            if let Ok(value) = fs::read(entry.path())
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()))
            {
                register_in(db, &value)?;
            }
        }
        Ok(())
    }
    pub(super) fn tick(&mut self) -> Result<Value> {
        let p = policy()?;
        if !p.enabled || (!p.unused_integrated && !p.missing) {
            return Ok(json!({"status":"disabled"}));
        }
        let db = database()?;
        self.import(&db)?;
        let Some(_guard) = storage::lock_until(&directory().join("sweep.lock"), Instant::now())?
        else {
            return Ok(json!({"status":"busy"}));
        };
        let next: Option<(String,String)> = db.query_row(
            "SELECT path,common_dir FROM checkouts WHERE status != 'unconfirmed' AND next_check <= ?1 ORDER BY next_check,path LIMIT 1",
            [now()], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(|e| e.to_string())?;
        let Some((path, common)) = next else {
            return Ok(json!({"status":"waiting","import_complete":self.imported}));
        };
        let args = vec![
            "--path".into(),
            path.clone(),
            "--common-dir".into(),
            common,
            "--fingerprint".into(),
            "automatic".into(),
            "--automatic".into(),
            "--periodic".into(),
        ];
        let outcome = super::worktree_cleanup::remove_command(&args);
        let value = match outcome {
            Ok(value) => {
                db.execute("DELETE FROM checkouts WHERE path=?1", [&path])
                    .map_err(|e| e.to_string())?;
                value
            }
            Err(reason) => {
                let status = if reason.starts_with("Removal could not be confirmed") {
                    "unconfirmed"
                } else {
                    "kept"
                };
                db.execute(
                    "UPDATE checkouts SET status=?2,detail=?3,next_check=?4 WHERE path=?1",
                    params![path, status, reason, now() + 3600.],
                )
                .map_err(|e| e.to_string())?;
                json!({"status":status,"path":path,"reason":reason})
            }
        };
        atomic(&directory().join("last-result.json"), &format!("{value}\n"))?;
        Ok(value)
    }
}

pub(super) fn command(args: &[String]) -> Result<Value> {
    match args {
        [action, rest @ ..] if action == "get" && rest.iter().all(|a| a == "--json") => {
            let p = policy()?;
            Ok(
                json!({"schema":1,"policy":p,"last_result":fs::read(directory().join("last-result.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok())}),
            )
        }
        [action, rest @ ..] if action == "set" && rest.iter().all(|a| a == "--json") => {
            let mut next: Policy =
                serde_json::from_value(stdin_json()?).map_err(|e| e.to_string())?;
            validate(&next)?;
            private_dir(&directory())?;
            let _guard = lock(Some(&directory().join("policy.lock")))?;
            if next.revision != policy()?.revision {
                return Err("Worktree settings changed; refresh before saving".into());
            }
            next.revision += 1;
            atomic(
                &directory().join("policy.json"),
                &format!(
                    "{}\n",
                    serde_json::to_string(&next).map_err(|e| e.to_string())?
                ),
            )?;
            Ok(json!({"schema":1,"policy":next}))
        }
        _ => Err(
            "usage: hgs worktrees policy get|set --json (set reads the reviewed policy from stdin)"
                .into(),
        ),
    }
}

/// Every automatic trigger, including archive cleanup, reserves in the same
/// durable queue under the repository lock. A later sweep cannot retry a lost
/// lifecycle outcome. Manual review/removal remains available.
pub(super) fn reserve(common: &Path, path: &Path) -> Result<()> {
    let owner =
        super::worktree_lifecycle::owned(common, path).ok_or("Worktree ownership changed")?;
    let db = database()?;
    register_in(&db, &owner)?;
    let changed=db.execute("UPDATE checkouts SET status='unconfirmed',detail='Automatic removal interrupted; review manually' WHERE path=?1 AND status!='unconfirmed'",
        [path.to_string_lossy().as_ref()]).map_err(|e|e.to_string())?;
    if changed != 1 {
        return Err(
            "Removal could not be confirmed: an earlier automatic attempt requires manual review"
                .into(),
        );
    }
    Ok(())
}
pub(super) fn removed(path: &Path, result: &Value) -> Result<()> {
    let db = database()?;
    db.execute(
        "DELETE FROM checkouts WHERE path=?1",
        [path.to_string_lossy().as_ref()],
    )
    .map_err(|e| e.to_string())?;
    atomic(
        &directory().join("last-result.json"),
        &format!("{result}\n"),
    )
}

pub(super) fn worker() {
    std::thread::spawn(|| {
        let mut sweep = Sweep::default();
        loop {
            if let Err(e) = sweep.tick() {
                eprintln!("hgs worktree maintenance: {e}");
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}
