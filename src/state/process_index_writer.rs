//! Advisory native record publisher. It grants no actor or effect authority.
use super::process_index::{self as index, *};
use super::repositories::directory::Directory;
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

mod bootstrap;
pub(super) use bootstrap::background_at;

pub(super) const WITNESS: &str = "process_index_publication";
pub(super) const UNCOVERED: &[&str] = &[
    "older-writers",
    "guarded-recovery",
    "fresh-id-owner-transfer",
    "providers",
    "resident-hosts",
    "teams-specific",
    "windows-session-host",
];
type SourceResult<T> = std::result::Result<T, String>;
fn checked<T>(result: SourceResult<T>) -> anyhow::Result<T> {
    result.map_err(anyhow::Error::msg)
}
fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn boot() -> anyhow::Result<String> {
    static BOOT: OnceLock<String> = OnceLock::new();
    let base = match BOOT.get() {
        Some(value) => value.clone(),
        None => {
            let value = checked(super::dsh::host_identity::boot())?;
            let _ = BOOT.set(value.clone());
            value
        }
    };
    let boot = format!("{base}:uid:{}", unsafe { libc::geteuid() });
    #[cfg(target_os = "linux")]
    let boot = {
        let mut boot = boot;
        for namespace in ["pid", "user"] {
            boot.push_str(&format!(
                ":{}",
                std::fs::read_link(format!("/proc/self/ns/{namespace}"))?.to_string_lossy()
            ));
        }
        boot
    };
    Ok(boot)
}
fn system() -> anyhow::Result<(String, String)> {
    static MACHINE: OnceLock<String> = OnceLock::new();
    if let Some(machine) = MACHINE.get() {
        return Ok((machine.clone(), boot()?));
    }
    #[cfg(target_os = "linux")]
    let machine = {
        let bytes = checked(super::keep_warm::read_regular(
            Path::new("/etc/machine-id"),
            128,
        ))?
        .ok_or_else(|| anyhow::anyhow!("Native machine identity unavailable"))?;
        uuid::Uuid::parse_str(std::str::from_utf8(&bytes)?.trim())?.to_string()
    };
    #[cfg(target_os = "macos")]
    let machine = {
        let mut bytes = [0u8; 16];
        let timeout = libc::timespec {
            tv_sec: 0,
            tv_nsec: 50_000_000,
        };
        anyhow::ensure!(
            unsafe { libc::gethostuuid(bytes.as_mut_ptr(), &timeout) } == 0,
            "Native machine identity unavailable"
        );
        let id = uuid::Uuid::from_bytes(bytes);
        anyhow::ensure!(!id.is_nil(), "Native machine identity unavailable");
        id.to_string()
    };
    anyhow::ensure!(
        !uuid::Uuid::parse_str(&machine)?.is_nil(),
        "Native machine identity unavailable"
    );
    let _ = MACHINE.set(machine.clone());
    Ok((machine, boot()?))
}
fn scope(root: &Path) -> anyhow::Result<Scope> {
    let path = crate::platform::canonical(root)?;
    let directory = checked(Directory::open(&path))?;
    anyhow::ensure!(
        checked(directory.owner_controls_entries())?,
        "Native state is not owned by this writer"
    );
    let (machine, boot) = system()?;
    let configuration = hash(serde_json::to_vec(&(
        path.to_string_lossy(),
        checked(directory.object())?,
    ))?);
    Ok(Scope {
        machine,
        configuration,
        boot,
    })
}
fn cache_path(root: &Path) -> anyhow::Result<PathBuf> {
    Ok(root
        .join("process-index")
        .join(format!("advisory-{}.sqlite3", hash(boot()?))))
}
fn process(pid: u32, identity: &crate::platform::ProcessIdentity) -> anyhow::Result<Process> {
    #[cfg(target_os = "linux")]
    let birth = Birth::Linux(identity.birth.parse()?);
    #[cfg(target_os = "macos")]
    let birth = {
        let (seconds, micros) = identity
            .birth
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Native process birth unavailable"))?;
        Birth::Mac {
            seconds: seconds.parse()?,
            micros: micros.parse()?,
        }
    };
    Ok(Process { pid, birth })
}
pub(super) fn capture_spawn(record: &mut Value, child: &crate::platform::ProcessIdentity) {
    let result = (|| -> anyhow::Result<index::pins::Pin> {
        let own = std::process::id();
        let supervisor = crate::platform::process_identity(own)
            .ok_or_else(|| anyhow::anyhow!("Supervisor identity unavailable"))?;
        let pid = u32::try_from(
            record["pid"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Child PID unavailable"))?,
        )?;
        index::pins::Pin::capture(
            record,
            &boot()?,
            process(pid, child)?,
            process(own, &supervisor)?,
            child.parent,
        )
    })();
    record[index::pins::FIELD] = match result {
        Ok(pin) => json!(pin),
        Err(_) => json!({"version":1,"unavailable":true}),
    };
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |t| u64::try_from(t.as_millis()).unwrap_or(u64::MAX))
}
fn eligible(record: &Value) -> bool {
    ["claude", "codex", "kimi"].contains(&record["agent"].as_str().unwrap_or(""))
        && ["", "tmux", "windows"].contains(&record["backend"].as_str().unwrap_or(""))
        && record["state"] != "archived"
        && record["rename_shadow"] != true
}
fn location(root: &Path, path: &Path, record: &Value) -> anyhow::Result<String> {
    let base = std::path::absolute(root)?;
    let at = std::path::absolute(path)?;
    let parts = at
        .strip_prefix(base)?
        .components()
        .map(|c| match c {
            std::path::Component::Normal(s) => s.to_str().map(str::to_owned),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow::anyhow!("Noncanonical source locator"))?;
    let (directory, file) = match parts.as_slice() {
        [file] => ("root", file),
        [directory, file] if ["bindings", "sessions"].contains(&directory.as_str()) => {
            (directory.as_str(), file)
        }
        _ => anyhow::bail!("Not an ordinary terminal record"),
    };
    let expected = if directory == "sessions" {
        record["session_id"].as_str().unwrap_or("").to_owned()
    } else {
        hash(record["name"].as_str().unwrap_or("").as_bytes())
    };
    anyhow::ensure!(
        !expected.is_empty() && file == &format!("{expected}.json"),
        "Record locator differs from its binding"
    );
    Ok(format!("{directory}/{file}"))
}

struct Owner {
    root: Arc<Directory>,
    index: Arc<Directory>,
    _gate: File,
    file: File,
    filename: String,
    scope: Scope,
    store: Store,
    auxiliary: Rc<Connection>,
}
fn directory(root: &Path) -> anyhow::Result<(Arc<Directory>, Arc<Directory>)> {
    let path = crate::platform::canonical(root)?;
    let root = Arc::new(checked(Directory::open(&path))?);
    anyhow::ensure!(
        checked(root.owner_controls_entries())?,
        "State directory is not owned by this writer"
    );
    let index = Arc::new(checked(root.ensure_child("process-index"))?);
    anyhow::ensure!(
        checked(index.private())?,
        "Process-index directory is not private"
    );
    Ok((root, index))
}
fn lost(directory: &Directory, epoch: Option<&str>, source: Option<&Source>, reason: &str) {
    let attempt = (|| -> anyhow::Result<()> {
        checked(directory.verify())?;
        let old = match checked(directory.read_private_optional("coverage-lost.json"))? {
            Some(file) => {
                use std::io::Read;
                let mut bytes = Vec::new();
                file.take(4097).read_to_end(&mut bytes)?;
                anyhow::ensure!(bytes.len() <= 4096, "Oversized coverage marker");
                Some(serde_json::from_slice::<Value>(&bytes)?)
            }
            None => None,
        };
        if let Some(old) = &old {
            anyhow::ensure!(
                old["version"] == 1 && old["complete"] == false,
                "Unknown coverage marker"
            );
        }
        let since = old
            .as_ref()
            .and_then(|v| v["since_ms"].as_u64())
            .unwrap_or_else(now);
        let reason = reason
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect::<String>();
        let body = json!({"version":1,"complete":false,"since_ms":since,"last_ms":now(),"epoch":epoch,"source":source,"reason":reason,"uncovered":UNCOVERED});
        checked(directory.replace_file("coverage-lost.json", format!("{body}\n").as_bytes()))?;
        Ok(())
    })();
    let _ = attempt;
}
fn gap(root: &Path, source: Option<&Source>, reason: &str) {
    if let Ok((_, dir)) = directory(root) {
        lost(&dir, None, source, reason);
    }
}
impl Owner {
    fn open(root: &Path) -> anyhow::Result<Self> {
        let (original, index) = directory(root)?;
        let gate = checked(index.open_private_lock("writer.lock"))?;
        gate.try_lock_exclusive()?;
        let scope = scope(&original.path)?;
        let filename = cache_path(&original.path)?
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("Missing cache filename"))?
            .to_owned();
        let (file, new) = match checked(index.read_private_optional(&filename))? {
            Some(file) => (file, false),
            None => (checked(index.write_new_file(&filename, &[]))?, true),
        };
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let db = Connection::open_with_flags(index.path.join(&filename), flags)?;
        let mut store = if new {
            Store::create(db, scope.clone())?
        } else {
            Store::open(db, scope.clone())?
        };
        store.incomplete()?;
        let auxiliary = Rc::new(Connection::open_with_flags(
            index.path.join(&filename),
            flags,
        )?);
        auxiliary.busy_timeout(std::time::Duration::ZERO)?;
        auxiliary.execute_batch("PRAGMA max_page_count=131072;")?;
        auxiliary.execute_batch("CREATE TABLE IF NOT EXISTS process_native_locations(source TEXT PRIMARY KEY,location TEXT NOT NULL);")?;
        let owner = Self {
            root: original,
            index,
            _gate: gate,
            file,
            filename,
            scope,
            store,
            auxiliary,
        };
        owner.verify()?;
        if checked(owner.index.read_private_optional("coverage.json"))?.is_none() {
            let epoch = owner.store.stamp()?.epoch;
            let body = json!({"version":1,"complete":false,"phase":"advisory","epoch":epoch,"uncovered":UNCOVERED});
            checked(
                owner
                    .index
                    .write_new("coverage.json", format!("{body}\n").as_bytes()),
            )?;
        }
        Ok(owner)
    }
    fn verify(&self) -> anyhow::Result<()> {
        checked(self.root.verify())?;
        checked(self.index.verify())?;
        let current = checked(self.index.read_private(&self.filename))?;
        anyhow::ensure!(
            crate::platform::FileIdentity::of(&current)?
                == crate::platform::FileIdentity::of(&self.file)?,
            "Process-index file changed"
        );
        Ok(())
    }
    fn locate(&self, source: &Source, location: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            location.len() <= 256 && !location.contains(['\n', '\r', '\\']),
            "Invalid source locator"
        );
        let key = serde_json::to_string(source)?;
        let old:Option<String>=self.auxiliary.query_row("SELECT CASE WHEN length(CAST(location AS BLOB))<=256 THEN location ELSE NULL END FROM process_native_locations WHERE source=?1",[&key],|r|r.get(0)).optional()?;
        if old.as_deref() != Some(location) {
            self.auxiliary.execute("INSERT INTO process_native_locations VALUES(?1,?2) ON CONFLICT(source) DO UPDATE SET location=excluded.location",params![key,location])?;
        }
        Ok(())
    }
}
// Compare held and current handles after the source operation. SQLite may have
// changed mtime while reserving the intent; a historical content stamp is not
// the namespace identity.
fn verify_file(
    directory: &Directory,
    filename: &str,
    original: Option<&File>,
) -> anyhow::Result<()> {
    let original = original.ok_or_else(|| anyhow::anyhow!("Index handle unavailable"))?;
    let current = checked(directory.read_private(filename))?;
    anyhow::ensure!(
        crate::platform::FileIdentity::of(&current)?
            == crate::platform::FileIdentity::of(original)?,
        "Index namespace changed after source publication"
    );
    Ok(())
}
fn fingerprint(publication: &Publication, location: &str) -> anyhow::Result<String> {
    let mut p = publication.clone();
    for a in &mut p.anchors {
        a.revision = 1;
        a.published_at = 0;
    }
    for e in &mut p.edges {
        e.revision = 1;
        e.published_at = 0;
    }
    Ok(hash(serde_json::to_vec(&(p, location))?))
}
fn witness(
    record: &mut Value,
    witness: index::publishing::Witness,
    fingerprint: &str,
    location: &str,
) {
    record[WITNESS] = json!({"witness":witness,"fingerprint":fingerprint,"location":location});
}

pub(super) fn write_at<T>(
    root: &Path,
    path: &Path,
    record: &mut Value,
    source: impl FnOnce(&mut Value) -> SourceResult<T>,
) -> SourceResult<T> {
    if super::effort::picker_entered() {
        return source(record);
    }
    if !eligible(record) {
        return source(record);
    }
    let location = match location(root, path, record) {
        Ok(value) => value,
        Err(_) => return source(record),
    };
    let pin = match boot().and_then(|boot| index::pins::Pin::read(record, &boot)) {
        Ok(Some(pin)) => pin,
        _ => {
            gap(
                root,
                None,
                "An older or unverified terminal writer has no qualified launch pin",
            );
            if record.get(index::pins::FIELD).is_some() || record.get(WITNESS).is_some() {
                witness(record, index::publishing::Witness::lost(), "", &location);
            }
            return source(record);
        }
    };
    let key = Source {
        kind: Kind::Native,
        key: pin.session.clone(),
    };
    let prepared = (|| -> anyhow::Result<(Owner, Change, String, bool)> {
        let mut owner = Owner::open(root)?;
        owner.locate(&key, &location)?;
        let old = owner.store.current_source(&key)?;
        let before = old.as_ref().map_or(0, |s| s.revision);
        let publication = pin.publication(record, &key, &location, before + 1, now())?;
        let fp = fingerprint(&publication, &location)?;
        let stable = old
            .as_ref()
            .is_some_and(|s| fingerprint(&s.publication, &location).is_ok_and(|f| f == fp))
            && owner.store.pending_source(&key)?.is_none();
        Ok((
            owner,
            Change {
                id: uuid::Uuid::new_v4().to_string(),
                source: key.clone(),
                before,
                locator: Some(location.clone()),
                publication,
            },
            fp,
            stable,
        ))
    })();
    let (mut owner, change, fp, stable) = match prepared {
        Ok(value) => value,
        Err(error) => {
            gap(root, Some(&key), &error.to_string());
            witness(record, index::publishing::Witness::lost(), "", &location);
            return source(record);
        }
    };
    let epoch = owner.store.stamp().ok().map(|s| s.epoch);
    if stable {
        witness(
            record,
            index::publishing::Witness {
                epoch,
                intent: None,
                seq: None,
                revision: change.before,
            },
            &fp,
            &location,
        );
        return source(record);
    }
    let directory = owner.index.clone();
    let original = owner.root.clone();
    let filename = owner.filename.clone();
    let file = owner.file.try_clone().ok();
    let after = directory.clone();
    index::publishing::publish_checked(
        Some(&mut owner.store),
        &change,
        |w| {
            witness(record, w, &fp, &location);
            source(record).map_err(anyhow::Error::msg)
        },
        || {
            checked(original.verify())?;
            checked(after.verify())?;
            verify_file(&after, &filename, file.as_ref())
        },
        |reason| lost(&directory, epoch.as_deref(), Some(&key), reason),
    )
    .map_err(|error| error.to_string())
}

pub(super) fn remove_at<T>(
    root: &Path,
    path: &Path,
    record: &Value,
    source: impl FnOnce() -> SourceResult<T>,
) -> SourceResult<T> {
    if super::effort::picker_entered() {
        return source();
    }
    if !eligible(record) {
        return source();
    }
    let location = match location(root, path, record) {
        Ok(value) => value,
        Err(_) => return source(),
    };
    let id = record["session_id"].as_str().unwrap_or("");
    let key = Source {
        kind: Kind::Native,
        key: id.into(),
    };
    let prepared = (|| -> anyhow::Result<(Owner, Change)> {
        let mut owner = Owner::open(root)?;
        owner.locate(&key, &location)?;
        let old = owner.store.current_source(&key)?;
        let before = old.as_ref().map_or(0, |s| s.revision);
        if let Some(old) =
            old.filter(|s| !s.publication.anchors.is_empty() || !s.publication.edges.is_empty())
        {
            let pin = index::pins::Pin::read(record, &owner.scope.boot)?
                .ok_or_else(|| anyhow::anyhow!("Original removal pin is unavailable"))?;
            let expected = pin.publication(record, &key, &location, old.revision, 0)?;
            anyhow::ensure!(
                fingerprint(&expected, &location)? == fingerprint(&old.publication, &location)?,
                "Removal binding changed"
            );
            let bytes = checked(super::keep_warm::read_regular(
                path,
                index::scanning::STEP_BYTES as u64,
            ))?
            .ok_or_else(|| anyhow::anyhow!("Original removal source is absent"))?;
            let actual: Value = serde_json::from_slice(&bytes)?;
            let actual_pin = index::pins::Pin::read(&actual, &owner.scope.boot)?
                .ok_or_else(|| anyhow::anyhow!("Current removal pin is unavailable"))?;
            let actual = actual_pin.publication(&actual, &key, &location, old.revision, 0)?;
            anyhow::ensure!(
                fingerprint(&actual, &location)? == fingerprint(&expected, &location)?,
                "Canonical removal preimage changed"
            );
        }
        Ok((
            owner,
            Change {
                id: uuid::Uuid::new_v4().to_string(),
                source: key.clone(),
                before,
                locator: Some(location.clone()),
                publication: Publication::default(),
            },
        ))
    })();
    let (mut owner, change) = match prepared {
        Ok(v) => v,
        Err(error) => {
            gap(root, Some(&key), &error.to_string());
            return source();
        }
    };
    let epoch = owner.store.stamp().ok().map(|s| s.epoch);
    let directory = owner.index.clone();
    let original = owner.root.clone();
    let filename = owner.filename.clone();
    let file = owner.file.try_clone().ok();
    let after = directory.clone();
    index::publishing::publish_checked(
        Some(&mut owner.store),
        &change,
        |_| source().map_err(anyhow::Error::msg),
        || {
            checked(original.verify())?;
            checked(after.verify())?;
            verify_file(&after, &filename, file.as_ref())
        },
        |reason| lost(&directory, epoch.as_deref(), Some(&key), reason),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
