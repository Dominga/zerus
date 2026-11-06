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
/// Whether the private database exists. A failed check is an error, never an
/// empty store: records that cannot be read must not read as absent.
fn stored() -> Result<bool> {
    directory()
        .join("queue.sqlite3")
        .try_exists()
        .map_err(|e| format!("Worktree maintenance records are unavailable: {e}"))
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
        CREATE INDEX IF NOT EXISTS due ON checkouts(next_check,path) WHERE status != 'unconfirmed';
        CREATE TABLE IF NOT EXISTS removals (
        path TEXT PRIMARY KEY, common_dir TEXT NOT NULL, branch TEXT NOT NULL, head TEXT NOT NULL,
        fingerprint TEXT NOT NULL, request_id TEXT NOT NULL, reserved_at REAL NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending','partial')), detail TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS removals_repository ON removals(common_dir,path);
        CREATE TABLE IF NOT EXISTS removal_volumes (
        path TEXT PRIMARY KEY, folder TEXT NOT NULL, repository TEXT NOT NULL);")
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
        let (outcome, attempted) = super::worktree_cleanup::remove_with_outcome(&args);
        let value = match outcome {
            Ok(value) => {
                db.execute("DELETE FROM checkouts WHERE path=?1", [&path])
                    .map_err(|e| e.to_string())?;
                value
            }
            Err(reason) => {
                // Once Git may have started, the outcome stays for manual review,
                // whatever the message; it is never rescheduled as kept.
                let status = if attempted || reason.starts_with("Removal could not be confirmed") {
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

/// Removal records. Every `git worktree remove` reserves one first, so a crash
/// between Git's deregistration and its file deletion is covered. A confirmed
/// outcome drops the record; a folder Git no longer tracks but that still
/// exists keeps it as `partial` for a fresh manual review, and an interruption
/// leaves it `pending`. Nothing automatic acts on these records, and a
/// recorded removal is never replayed.
pub(super) const MAX_REMOVALS: i64 = 512;

#[derive(Clone, Debug, Serialize)]
pub(super) struct Removal {
    pub path: String,
    pub common_dir: String,
    pub branch: String,
    pub head: String,
    pub fingerprint: String,
    pub request_id: String,
    pub reserved_at: f64,
    pub status: String,
    pub detail: String,
    /// The filesystems recorded at reservation, in the separate
    /// `removal_volumes` table older CLIs never read. `None` for a record an
    /// older CLI wrote: it keeps the plain missing-path check.
    #[serde(skip)]
    pub volumes: Option<Volumes>,
}

/// Which filesystem held a removal's folder and repository. An empty identity
/// could not be recorded: that path is never taken as gone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Volumes {
    pub folder: String,
    pub repository: String,
}

const SELECT_REMOVALS: &str = "SELECT r.path,r.common_dir,r.branch,r.head,r.fingerprint,
    r.request_id,r.reserved_at,r.status,r.detail,v.folder,v.repository
    FROM removals r LEFT JOIN removal_volumes v ON v.path=r.path";

fn removal(r: &rusqlite::Row) -> rusqlite::Result<Removal> {
    let folder: Option<String> = r.get(9)?;
    let repository: Option<String> = r.get(10)?;
    Ok(Removal {
        path: r.get(0)?,
        common_dir: r.get(1)?,
        branch: r.get(2)?,
        head: r.get(3)?,
        fingerprint: r.get(4)?,
        request_id: r.get(5)?,
        reserved_at: r.get(6)?,
        status: r.get(7)?,
        detail: r.get(8)?,
        volumes: folder
            .zip(repository)
            .map(|(folder, repository)| Volumes { folder, repository }),
    })
}

/// The filesystem holding `path`: on Linux its device and `statfs` filesystem
/// ID (derived from the filesystem UUID on ext4, btrfs and xfs), on macOS its
/// filesystem ID and mount point. `None` when it cannot be identified,
/// including a zero filesystem ID (some tmpfs and network filesystems).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn volume(path: &Path) -> Option<String> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(name.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    // fsid_t keeps its value private; compare its bytes.
    let fsid = unsafe {
        std::slice::from_raw_parts(
            (&stat.f_fsid as *const libc::fsid_t).cast::<u8>(),
            std::mem::size_of::<libc::fsid_t>(),
        )
    };
    if fsid.iter().all(|b| *b == 0) {
        return None;
    }
    let fsid: String = fsid.iter().map(|b| format!("{b:02x}")).collect();
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let dev = fs::metadata(path).ok()?.dev();
        Some(format!("linux:{dev}:{fsid}"))
    }
    #[cfg(target_os = "macos")]
    {
        let on = unsafe { std::ffi::CStr::from_ptr(stat.f_mntonname.as_ptr()) };
        Some(format!("macos:{fsid}:{}", on.to_string_lossy()))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn volume(_path: &Path) -> Option<String> {
    None
}

/// What a new record notes about its filesystems. Platforms without a volume
/// identity write no row and keep the plain missing-path check.
fn recorded_volumes(removal: &Removal) -> Option<Volumes> {
    cfg!(any(target_os = "linux", target_os = "macos")).then(|| Volumes {
        folder: volume(Path::new(&removal.path)).unwrap_or_default(),
        repository: volume(Path::new(&removal.common_dir)).unwrap_or_default(),
    })
}

fn upsert(db: &Connection, r: &Removal) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO removals VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            r.path,
            r.common_dir,
            r.branch,
            r.head,
            r.fingerprint,
            r.request_id,
            r.reserved_at,
            r.status,
            r.detail
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Fails closed at the bound: unresolved leftovers must stay visible, so a new
/// removal is refused rather than evicting an older record.
pub(super) fn reserve_removal(removal: &Removal) -> Result<()> {
    let mut db = database()?;
    // Count and insert in one write transaction: concurrent removals in other
    // repositories cannot exceed the bound together.
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let count = |tx: &Connection| -> Result<i64> {
        tx.query_row(
            "SELECT count(*) FROM removals WHERE path != ?1",
            [&removal.path],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    };
    if count(&tx)? >= MAX_REMOVALS {
        resolve_vanished(&tx)?;
        if count(&tx)? >= MAX_REMOVALS {
            // Resolving a vanished record is independent of admitting this
            // removal. A capacity refusal must not resurrect those records.
            tx.commit().map_err(|e| e.to_string())?;
            return Err(format!(
                "Worktree removal refused: {MAX_REMOVALS} earlier removals still need review. \
                 Review their leftover folders (`hgs worktrees leftovers` lists those whose \
                 repository is gone) and keep or clean them up first."
            ));
        }
    }
    upsert(&tx, removal)?;
    if let Some(volumes) = recorded_volumes(removal) {
        tx.execute(
            "INSERT OR REPLACE INTO removal_volumes VALUES (?1,?2,?3)",
            params![removal.path, volumes.folder, volumes.repository],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// Nothing has this name: not even a link. An unreadable path is not missing.
fn missing(path: &str) -> bool {
    fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}

/// Whether a missing path is gone for good. With a recorded filesystem, the
/// nearest existing folder above it must be on that same filesystem: a volume
/// that is unmounted, replaced or unidentifiable leaves the path uncertain,
/// never gone. A record without a volume row (an older CLI) keeps the plain
/// missing-path check.
pub(super) fn vanished(path: &str, recorded: Option<&str>) -> bool {
    if !missing(path) {
        return false;
    }
    let Some(recorded) = recorded else {
        return true;
    };
    if recorded.is_empty() {
        return false;
    }
    for above in Path::new(path).ancestors().skip(1) {
        match fs::symlink_metadata(above) {
            Ok(_) => return volume(above).as_deref() == Some(recorded),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return false,
        }
    }
    false
}

fn forget(db: &Connection, path: &str) -> Result<usize> {
    let changed = db
        .execute("DELETE FROM removals WHERE path=?1", [path])
        .map_err(|e| e.to_string())?;
    db.execute("DELETE FROM removal_volumes WHERE path=?1", [path])
        .map_err(|e| e.to_string())?;
    Ok(changed)
}

/// A record whose repository and folder are both gone has nothing left to
/// review; it must not hold the bound forever.
fn resolve_vanished(db: &Connection) -> Result<usize> {
    let mut statement = db
        .prepare(&format!("{SELECT_REMOVALS} ORDER BY r.path LIMIT ?1"))
        .map_err(|e| e.to_string())?;
    let vanished: Vec<String> = statement
        .query_map([MAX_REMOVALS + 1], removal)
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|r| {
            let volumes = r.volumes.as_ref();
            vanished(&r.common_dir, volumes.map(|v| v.repository.as_str()))
                && vanished(&r.path, volumes.map(|v| v.folder.as_str()))
        })
        .map(|r| r.path)
        .collect();
    for path in &vanished {
        forget(db, path)?;
    }
    // Rows an older CLI left behind when it forgot their records.
    db.execute(
        "DELETE FROM removal_volumes WHERE path NOT IN (SELECT path FROM removals)",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(vanished.len())
}

/// Records whose repository no longer exists, after resolving those whose
/// folder is gone too. Bounded by `MAX_REMOVALS`.
pub(super) fn orphan_removals() -> Result<Vec<Removal>> {
    if !stored()? {
        return Ok(Vec::new());
    }
    let db = database()?;
    resolve_vanished(&db)?;
    let mut statement = db
        .prepare(&format!("{SELECT_REMOVALS} ORDER BY r.path LIMIT ?1"))
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([MAX_REMOVALS], removal)
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    // Listed wherever the repository cannot be found, gone or unmounted.
    Ok(rows
        .into_iter()
        .filter(|r| missing(&r.common_dir))
        .collect())
}

/// `None` drops the record after a confirmed outcome; `Some` keeps the folder
/// Git no longer tracks for review. Written whole, so a concurrent review that
/// dropped a pending record cannot lose a later partial outcome.
pub(super) fn settle_removal(removal: &Removal, partial: Option<&str>) -> Result<()> {
    let db = database()?;
    match partial {
        None => forget(&db, &removal.path).map(|_| ()),
        Some(detail) => upsert(
            &db,
            &Removal {
                status: "partial".into(),
                detail: detail.chars().take(1024).collect(),
                ..removal.clone()
            },
        ),
    }
}

/// One repository's records through its index; at most `MAX_REMOVALS` exist.
/// Reading never creates the private database.
pub(super) fn removals(common: &Path) -> Result<Vec<Removal>> {
    if !stored()? {
        return Ok(Vec::new());
    }
    let db = database()?;
    let mut statement = db
        .prepare(&format!(
            "{SELECT_REMOVALS} WHERE r.common_dir=?1 ORDER BY r.path LIMIT ?2"
        ))
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params![common.to_string_lossy(), MAX_REMOVALS], removal)
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())
}

/// How many records one repository has, through its index and at most
/// `MAX_REMOVALS`. Reading never creates the private database.
pub(super) fn removal_count(common: &Path) -> Result<i64> {
    if !stored()? {
        return Ok(0);
    }
    database()?
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM removals WHERE common_dir=?1 LIMIT ?2)",
            params![common.to_string_lossy(), MAX_REMOVALS],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}

pub(super) fn forget_removal(path: &str) -> Result<bool> {
    Ok(forget(&database()?, path)? == 1)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// zerus-ty94 #5: a missing path is gone only on its recorded filesystem.
    #[test]
    fn a_missing_path_is_gone_only_while_its_recorded_filesystem_is_there() {
        let base = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(base.path()).unwrap();
        let text = |p: &Path| p.to_string_lossy().into_owned();
        let folder = base.join("worktree");
        let repository = base.join("repository/.git");
        // Something is there, even a dangling link: never gone.
        fs::create_dir(&folder).unwrap();
        assert!(!vanished(&text(&folder), None));
        #[cfg(unix)]
        {
            let link = base.join("link");
            std::os::unix::fs::symlink(base.join("nowhere"), &link).unwrap();
            // The plain missing check: a link that follows nowhere is still there.
            assert!(!vanished(&text(&link), None));
        }
        fs::remove_dir(&folder).unwrap();
        // A record from an older CLI keeps the plain missing check; one whose
        // volume could not be recorded, or another filesystem's, is uncertain.
        assert!(vanished(&text(&folder), None));
        assert!(!vanished(&text(&folder), Some("")));
        assert!(!vanished(&text(&folder), Some("linux:1:00000000000000ff")));
        let Some(here) = volume(&base) else {
            // Every macOS filesystem has an identity: the log must prove it ran.
            if cfg!(target_os = "macos") {
                panic!("no macOS volume identity for {}", base.display());
            }
            eprintln!("skipped: no volume identity here (Windows, or a zero Linux fsid)");
            return;
        };
        assert!(vanished(&text(&folder), Some(&here)));
        // A deleted repository: the nearest existing folder above decides.
        assert!(vanished(&text(&repository), Some(&here)));
    }

    /// Unmounting or replacing the volume leaves the path uncertain. Runs the
    /// probe in a private user and mount namespace when available.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_missing_path_on_an_unmounted_volume_is_never_gone() {
        let base = tempfile::tempdir().unwrap();
        let Ok(output) = std::process::Command::new("unshare")
            .args(["--user", "--map-root-user", "--mount", "--"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::worktree_maintenance::tests::volume_probe",
                "--nocapture",
            ])
            .env("HGS_TEST_VOLUME_PROBE", base.path())
            .env_remove("HGS_SESSION")
            .env_remove("HGS_RUN_ID")
            .output()
        else {
            eprintln!("skipped: unshare is unavailable");
            return;
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.contains("volume probe ran") {
            eprintln!("skipped: user and mount namespaces are unavailable: {stderr}");
            return;
        }
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("volume probe passed"), "{stdout}\n{stderr}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn volume_probe() {
        use std::os::unix::ffi::OsStrExt;
        let Some(base) = std::env::var_os("HGS_TEST_VOLUME_PROBE") else {
            return;
        };
        println!("volume probe ran");
        let target = PathBuf::from(base).join("volume");
        fs::create_dir(&target).unwrap();
        let path = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
        let mount = || {
            let rc = unsafe {
                libc::mount(
                    c"none".as_ptr(),
                    path.as_ptr(),
                    c"tmpfs".as_ptr(),
                    0,
                    std::ptr::null(),
                )
            };
            assert_eq!(rc, 0, "{}", std::io::Error::last_os_error());
        };
        let text = |p: &Path| p.to_string_lossy().into_owned();
        mount();
        let folder = target.join("worktrees/leftover");
        fs::create_dir_all(&folder).unwrap();
        // What a reservation records; a zero fsid records nothing usable.
        let recorded = volume(&folder).unwrap_or_default();
        println!(
            "volume probe recorded {}",
            if recorded.is_empty() {
                "nothing (zero fsid)"
            } else {
                "an identity"
            }
        );
        fs::remove_dir(&folder).unwrap();
        assert_eq!(
            vanished(&text(&folder), Some(&recorded)),
            !recorded.is_empty()
        );
        // Unmounted: the empty mount point is on the outer filesystem.
        assert_eq!(unsafe { libc::umount(path.as_ptr()) }, 0);
        assert!(!vanished(&text(&folder), Some(&recorded)));
        // Another volume at the same place, with the same folders: not it.
        mount();
        fs::create_dir_all(folder.parent().unwrap()).unwrap();
        assert!(!vanished(&text(&folder), Some(&recorded)));
        assert_eq!(unsafe { libc::umount(path.as_ptr()) }, 0);
        println!("volume probe passed");
    }
}
