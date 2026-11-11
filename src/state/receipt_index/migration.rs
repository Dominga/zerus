//! One coalesced worker per state root; each transaction has a count/time budget.
//! Restart reopens discovery in the background and retains every committed row
//! and sequence. No directory walk (including restart work) runs in inspection.
use super::*;
use fs2::FileExt;
use std::fs::ReadDir;
use std::process::{Command, Stdio};
use std::time::Instant;

const BATCH: usize = 64;
const SLICE: Duration = Duration::from_millis(25);

pub(super) fn stamp(path: &Path) -> Result<String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            let (seconds, nanos) = crate::platform::modified(&metadata);
            Ok(format!(
                "{}:{seconds}:{nanos}:{:?}",
                metadata.len(),
                metadata.created().ok()
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("absent".into()),
        _ => Err("Receipt source is unavailable".into()),
    }
}
fn try_guard(path: &Path) -> Result<Option<Guard>> {
    private_dir(path.parent().ok_or("Invalid index lock")?)?;
    let file = lock_file(path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(Guard::held(file))),
        Err(error) if crate::platform::lock_contended(&error) => Ok(None),
        Err(_) => Err("Receipt index worker lock is unavailable".into()),
    }
}
pub(super) fn request(state: &Path) {
    let launch = (|| -> Result<()> {
        let canonical =
            crate::platform::canonical(state).map_err(|_| "Receipt state is unavailable")?;
        let state = canonical.as_path();
        let Some(_launch) = try_guard(&state.join("receipt-index-launch.lock"))? else {
            return Ok(());
        };
        let Some(probe) = try_guard(&state.join("receipt-index-worker.lock"))? else {
            return Ok(());
        };
        let attempt = state.join("receipt-index-attempt.json");
        let last = fs::File::open(&attempt)
            .ok()
            .and_then(|file| serde_json::from_reader::<_, Value>(file.take(4096)).ok())
            .and_then(|value| value["at"].as_f64())
            .unwrap_or(0.0);
        if now() - last < 5.0 {
            return Ok(());
        }
        atomic(&attempt, &json!({"at":now()}).to_string())?;
        let mut command =
            Command::new(std::env::current_exe().map_err(|_| "Receipt worker is unavailable")?);
        command
            .args(["__state", "receipt-index-worker"])
            .env("HGS_STATE_DIR", state)
            .env_remove("HGS_SESSION")
            .env_remove("HGS_RUN_ID")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_AGENT")
            .current_dir(state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        drop(probe);
        let _child = crate::platform::spawn_detached(&mut command)
            .map_err(|_| "Receipt worker is unavailable")?;
        Ok(())
    })();
    let _ = launch;
}

struct Walk {
    source: Source,
    entries: ReadDir,
    started: String,
}
pub(super) struct Migration {
    catalog: Catalog,
    sources: Vec<Source>,
    force: bool,
    next_source: usize,
    walk: Option<Walk>,
}
impl Migration {
    pub(super) fn new(state: &Path) -> Result<Self> {
        Self::with_sources(state, Source::ALL.to_vec(), false)
    }
    pub(super) fn questions(state: &Path) -> Result<Self> {
        Self::with_sources(state, vec![Source::Question], true)
    }
    fn with_sources(state: &Path, sources: Vec<Source>, force: bool) -> Result<Self> {
        Ok(Self {
            catalog: Catalog::open(state, true)?,
            sources,
            force,
            next_source: 0,
            walk: None,
        })
    }
    /// A bounded slice. Holding ReadDir across slices avoids rescanning a
    /// prefix on every batch; committed locators survive process interruption.
    pub(super) fn step(&mut self) -> Result<bool> {
        let deadline = Instant::now() + SLICE;
        let transaction = sql(self.catalog.db.unchecked_transaction())?;
        let mut examined = 0;
        while examined < BATCH && Instant::now() < deadline {
            if self.walk.is_none() {
                let Some(source) = self.sources.get(self.next_source).copied() else {
                    sql(transaction.commit())?;
                    return Ok(true);
                };
                self.next_source += 1;
                let directory = self.catalog.state.join(source.directory());
                let started = match stamp(&directory) {
                    Ok(stamp) => stamp,
                    Err(_) => {
                        self.catalog
                            .error(source, "@source", "source_unavailable")?;
                        sql(self.catalog.db.execute(
                            "UPDATE receipt_sources SET complete=0 WHERE source=?",
                            [source.key()],
                        ))?;
                        continue;
                    }
                };
                let prior: Option<(String, bool, i64)> = sql(self
                    .catalog
                    .db
                    .query_row(
                        "SELECT stamp,complete,errors FROM receipt_sources WHERE source=?",
                        [source.key()],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional())?;
                if !self.force
                    && prior.is_some_and(|(stamp, complete, errors)| {
                        complete && errors == 0 && stamp == started
                    })
                {
                    continue;
                }

                sql(self.catalog.db.execute(
                    "UPDATE receipt_sources SET complete=0 WHERE source=?",
                    [source.key()],
                ))?;
                if started == "absent" {
                    // An empty fresh namespace is a usable empty snapshot. A
                    // disappeared store with known receipts is explicitly unknown.
                    sql(self.catalog.db.execute("UPDATE receipt_sources SET stamp='absent',complete=(indexed=0) WHERE source=?",[source.key()]))?;
                    continue;
                }
                let entries =
                    fs::read_dir(&directory).map_err(|_| "Receipt source is unavailable")?;
                self.walk = Some(Walk {
                    source,
                    entries,
                    started,
                });
            }
            let walk = self.walk.as_mut().unwrap();
            match walk.entries.next() {
                Some(Ok(entry)) => {
                    examined += 1;
                    let filename = entry.file_name();
                    let Some(filename) = filename.to_str().filter(|name| name.ends_with(".json"))
                    else {
                        continue;
                    };
                    let loaded = read_original(&entry.path());
                    match loaded {
                        Ok(value) => {
                            if self
                                .catalog
                                .remember(walk.source, filename, &value)
                                .is_err()
                            {
                                self.catalog.error(walk.source, filename, "corrupt")?;
                            }
                        }
                        Err(reason) => self.catalog.error(walk.source, filename, reason)?,
                    }
                }
                Some(Err(_)) => {
                    examined += 1;
                    self.catalog
                        .error(walk.source, "@enumeration", "enumeration_incomplete")?;
                    self.walk = None;
                }
                None => {
                    let source = walk.source;
                    let current = stamp(&self.catalog.state.join(source.directory()));
                    let complete = current.as_ref().is_ok_and(|stamp| *stamp == walk.started);
                    sql(self.catalog.db.execute(
                        "UPDATE receipt_sources SET stamp=?,complete=? WHERE source=?",
                        params![walk.started, complete, source.key()],
                    ))?;
                    sql(self.catalog.db.execute(
                        "DELETE FROM receipt_errors WHERE source=? AND name='@source'",
                        [source.key()],
                    ))?;
                    self.walk = None;
                }
            }
        }
        sql(transaction.commit())?;
        Ok(false)
    }
}
pub(super) fn worker(state: &Path) -> Result<i32> {
    let Some(_guard) = try_guard(&state.join("receipt-index-worker.lock"))? else {
        return Ok(0);
    };
    let mut migration = Migration::new(state)?;
    while !migration.step()? {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(0)
}
