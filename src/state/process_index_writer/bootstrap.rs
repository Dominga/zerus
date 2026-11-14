use super::*;
use index::scanning::{self, Read, Source as ScanSource};
use std::collections::VecDeque;
use std::io::Read as IoRead;
use std::os::unix::fs::MetadataExt;
use std::time::Instant;

#[derive(Default, Debug)]
pub(in crate::state) struct Stats {
    pub entries: usize,
    pub bytes: usize,
    pub rows: usize,
    pub gaps: usize,
    pub done: bool,
}
struct Folder {
    directory: Arc<Directory>,
    entries: fs::ReadDir,
}
struct Files {
    root: Arc<Directory>,
    index: Arc<Directory>,
    scope: Scope,
    epoch: String,
    folders: VecDeque<Folder>,
    pending: Option<(Arc<Directory>, String)>,
    auxiliary: Rc<Connection>,
    gaps: usize,
    last_gap: Option<(Option<Source>, String)>,
}
fn stamp(meta: &fs::Metadata) -> (u64, u64, u64, i64, i64) {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}
fn read(directory: &Directory, name: &str, bytes: usize) -> anyhow::Result<Option<Vec<u8>>> {
    checked(directory.verify())?;
    let Some(mut file) = checked(directory.read_private_optional(name))? else {
        return Ok(None);
    };
    let before = file.metadata()?;
    anyhow::ensure!(
        before.len() <= bytes as u64,
        "Source exceeds this step's allowance"
    );
    let mut data = Vec::new();
    (&mut file).take(before.len()).read_to_end(&mut data)?;
    anyhow::ensure!(
        data.len() as u64 == before.len() && stamp(&file.metadata()?) == stamp(&before),
        "Source changed during bounded read"
    );
    checked(directory.verify())?;
    Ok(Some(data))
}
fn source_path(root: &Arc<Directory>, locator: &str) -> anyhow::Result<(Arc<Directory>, String)> {
    let parts = locator.split('/').collect::<Vec<_>>();
    anyhow::ensure!(
        parts.len() == 2
            && ["root", "bindings", "sessions"].contains(&parts[0])
            && !parts[1].is_empty()
            && parts[1].len() <= 128
            && parts[1]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            && parts[1] != "."
            && parts[1] != "..",
        "Invalid native locator"
    );
    let directory = if parts[0] == "root" {
        root.clone()
    } else {
        Arc::new(checked(root.child(parts[0]))?)
    };
    Ok((directory, parts[1].into()))
}
fn note_location(db: &Connection, source: &Source, location: &str) -> anyhow::Result<()> {
    db.execute("INSERT INTO process_native_locations VALUES(?1,?2) ON CONFLICT(source) DO UPDATE SET location=excluded.location",params![serde_json::to_string(source)?,location])?;
    Ok(())
}
impl Files {
    fn new(owner: &Owner) -> anyhow::Result<Self> {
        let mut folders = VecDeque::new();
        for name in ["root", "bindings", "sessions"] {
            let directory = if name == "root" {
                owner.root.clone()
            } else {
                match fs::symlink_metadata(owner.root.path.join(name)) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                    Ok(_) => Arc::new(checked(owner.root.child(name))?),
                }
            };
            checked(directory.verify())?;
            let entries = fs::read_dir(&directory.path)?;
            checked(directory.verify())?;
            folders.push_back(Folder { directory, entries });
        }
        Ok(Self {
            root: owner.root.clone(),
            index: owner.index.clone(),
            scope: owner.scope.clone(),
            epoch: owner.store.stamp()?.epoch,
            folders,
            pending: None,
            auxiliary: owner.auxiliary.clone(),
            gaps: 0,
            last_gap: None,
        })
    }
    fn gap(&mut self, source: Option<&Source>, reason: &str) {
        self.gaps += 1;
        self.last_gap = Some((source.cloned(), reason.chars().take(512).collect()));
    }
    fn row(
        &mut self,
        store: &mut Store,
        record: &Value,
        location: &str,
    ) -> anyhow::Result<Option<Snapshot>> {
        if !eligible(record) {
            return Ok(None);
        }
        let Some(pin) = index::pins::Pin::read(record, &self.scope.boot)? else {
            self.gap(
                None,
                "Legacy terminal source has no original precise launch pin",
            );
            return Ok(None);
        };
        let source = Source {
            kind: Kind::Native,
            key: pin.session.clone(),
        };
        note_location(&self.auxiliary, &source, location)?;
        if let Some(pending) = store.pending_source(&source)? {
            let witness = &record[WITNESS]["witness"];
            let publication =
                pin.publication(record, &source, location, pending.change.before + 1, 0)?;
            if pending.change.locator.as_deref() == Some(location)
                && witness["epoch"] == self.epoch
                && witness["intent"] == pending.change.id
                && witness["seq"] == pending.seq
                && witness["revision"] == pending.change.before + 1
                && fingerprint(&publication, location)?
                    == fingerprint(&pending.change.publication, location)?
            {
                if let Err(error) = store.settle(&Ticket {
                    epoch: self.epoch.clone(),
                    id: pending.change.id,
                    seq: pending.seq,
                }) {
                    self.gap(Some(&source), &error.to_string());
                    return Ok(None);
                }
            } else {
                self.gap(
                    Some(&source),
                    "Pending intent does not match the canonical publication witness",
                );
                return Ok(None);
            }
        }
        let old = store.current_source(&source)?;
        let revision = old.as_ref().map_or(1, |snapshot| snapshot.revision);
        let publication = pin.publication(record, &source, location, revision, now())?;
        let witness = &record[WITNESS]["witness"];
        if witness["epoch"] != self.epoch || witness["revision"].as_u64() != Some(revision) {
            self.gap(
                Some(&source),
                "Canonical epoch/revision witness is missing or differs from the settled index",
            );
        }
        if let Some(old) = old {
            if fingerprint(&old.publication, location)? != fingerprint(&publication, location)? {
                self.gap(
                    Some(&source),
                    "Canonical binding differs from the settled revision; retain the indexed claim",
                );
            }
            // A scan is not a new binding write. Only exact intent settlement
            // above may replace a known publication. Keep its original timestamp.
            return Ok(Some(old));
        }
        Ok(Some(Snapshot {
            source,
            revision,
            publication,
        }))
    }
}
impl ScanSource for Files {
    fn next(
        &mut self,
        store: &mut Store,
        remaining: usize,
        until: Instant,
    ) -> anyhow::Result<Read> {
        if Instant::now() >= until {
            return Ok(Read::Yield);
        }
        anyhow::ensure!(
            store.stamp()?.epoch == self.epoch,
            "Bootstrap store epoch changed"
        );
        let (directory, name) = if let Some(pending) = self.pending.take() {
            pending
        } else {
            loop {
                let Some(folder) = self.folders.front_mut() else {
                    return Ok(Read::End);
                };
                match folder.entries.next() {
                    None => {
                        self.folders.pop_front();
                    }
                    Some(Err(error)) => {
                        let message = error.to_string();
                        self.folders.pop_front();
                        self.gap(None, &message);
                        return Ok(Read::Entry {
                            bytes: 0,
                            row: None,
                        });
                    }
                    Some(Ok(entry)) => {
                        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                            return Ok(Read::Entry {
                                bytes: 0,
                                row: None,
                            });
                        };
                        break (folder.directory.clone(), name);
                    }
                }
            }
        };
        let folder = if directory.path == self.root.path {
            ""
        } else {
            directory
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
        };
        let Some(stem) = name.strip_suffix(".json") else {
            return Ok(Read::Entry {
                bytes: 0,
                row: None,
            });
        };
        if !super::super::storage::binding_stem(folder, stem) {
            return Ok(Read::Entry {
                bytes: 0,
                row: None,
            });
        }
        if Instant::now() >= until {
            self.pending = Some((directory, name));
            return Ok(Read::Yield);
        }
        let size = match checked(directory.read_private(&name))
            .and_then(|file| Ok(file.metadata()?.len()))
        {
            Ok(n) => n,
            Err(error) => {
                self.gap(None, &error.to_string());
                return Ok(Read::Entry {
                    bytes: 0,
                    row: None,
                });
            }
        };
        if size > scanning::STEP_BYTES as u64 {
            self.gap(None, "Canonical source exceeds the bootstrap byte bound");
            return Ok(Read::Entry {
                bytes: 0,
                row: None,
            });
        }
        if size > remaining as u64 {
            self.pending = Some((directory, name));
            return Ok(Read::Yield);
        }
        let data = match read(&directory, &name, remaining) {
            Ok(Some(data)) => data,
            Ok(None) => {
                return Ok(Read::Entry {
                    bytes: 0,
                    row: None,
                });
            }
            Err(error) => {
                self.gap(None, &error.to_string());
                // A changed file may have consumed the entire admitted read.
                return Ok(Read::Entry {
                    bytes: remaining,
                    row: None,
                });
            }
        };
        let result = (|| -> anyhow::Result<Option<Snapshot>> {
            let record: Value = serde_json::from_slice(&data)?;
            let locator = location(&self.root.path, &directory.path.join(&name), &record)?;
            self.row(store, &record, &locator)
        })();
        let row = match result {
            Ok(row) => row,
            Err(error) => {
                self.gap(None, &error.to_string());
                None
            }
        };
        Ok(Read::Entry {
            bytes: data.len(),
            row,
        })
    }
}

pub(in crate::state) struct Background {
    owner: Owner,
    scan: scanning::Scan<Files>,
    phase: u8,
    after: String,
    change_after: u64,
}
struct Gate(File);
impl Drop for Gate {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
impl Background {
    fn new(root: &Path) -> anyhow::Result<Self> {
        let mut owner = Owner::open(root)?;
        let source = Files::new(&owner)?;
        let scan = scanning::Scan::new(&mut owner.store, source)?;
        FileExt::unlock(&owner._gate)?;
        Ok(Self {
            owner,
            scan,
            phase: 0,
            after: String::new(),
            change_after: 0,
        })
    }
    fn sweep(&mut self, until: Instant) -> anyhow::Result<Stats> {
        let mut stats = Stats::default();
        let mut query=self.owner.auxiliary.prepare("SELECT substr(source,1,513),CASE WHEN length(CAST(location AS BLOB))<=256 THEN location ELSE NULL END FROM process_native_locations INDEXED BY sqlite_autoindex_process_native_locations_1 WHERE source>?1 ORDER BY source LIMIT 128")?;
        let input = query.query_map([&self.after], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut rows = Vec::new();
        let mut bytes = 0;
        for row in input {
            let row = row?;
            let size = row.0.len() + row.1.as_ref().map_or(0, String::len);
            if bytes + size > index::PAGE_BYTES {
                break;
            }
            bytes += size;
            rows.push(row);
        }
        drop(query);
        if rows.is_empty() {
            self.phase = 2;
            return Ok(stats);
        }
        for (key, locator) in rows {
            if Instant::now() >= until
                || stats.entries >= scanning::STEP_ENTRIES
                || stats.bytes >= scanning::STEP_BYTES
            {
                break;
            }
            let source: Source = serde_json::from_str(&key)?;
            let locator = locator.ok_or_else(|| anyhow::anyhow!("Oversized source location"))?;
            let Some(current) = self.owner.store.current_source(&source)? else {
                self.after = key;
                continue;
            };
            if current.publication.anchors.is_empty() && current.publication.edges.is_empty() {
                self.after = key;
                continue;
            }
            let mut consumed = 0;
            let inspected = (|| -> anyhow::Result<()> {
                let (directory, name) = source_path(&self.owner.root, &locator)?;
                let data = match read(&directory, &name, scanning::STEP_BYTES - stats.bytes) {
                    Ok(data) => data,
                    Err(error) => {
                        consumed = scanning::STEP_BYTES - stats.bytes;
                        return Err(error);
                    }
                }
                .ok_or_else(|| {
                    anyhow::anyhow!("Indexed source has no canonical epoch/high-water witness")
                })?;
                consumed = data.len();
                let record: Value = serde_json::from_slice(&data)?;
                let pin =
                    index::pins::Pin::read(&record, &self.owner.scope.boot)?.ok_or_else(|| {
                        anyhow::anyhow!("Indexed source lost its original launch pin")
                    })?;
                let publication =
                    pin.publication(&record, &source, &locator, current.revision, 0)?;
                anyhow::ensure!(
                    record[WITNESS]["witness"]["epoch"] == self.scan.source.epoch
                        && fingerprint(&publication, &locator)?
                            == fingerprint(&current.publication, &locator)?,
                    "Canonical source and index publication diverged"
                );
                Ok(())
            })();
            stats.entries += 1;
            stats.bytes += consumed;
            if let Err(error) = inspected {
                stats.gaps += 1;
                self.scan.source.gap(Some(&source), &error.to_string());
            }
            self.after = key;
        }
        Ok(stats)
    }
    fn catchup(&mut self, until: Instant) -> anyhow::Result<Stats> {
        let mut stats = Stats::default();
        let page = self
            .owner
            .store
            .changes(self.change_after, index::PAGE_ROWS)?;
        if page.rows.is_empty() {
            self.phase = 3;
            stats.done = true;
            return Ok(stats);
        }
        for row in page.rows {
            if Instant::now() >= until || stats.bytes >= scanning::STEP_BYTES {
                break;
            }
            if !row.settled && row.change.source.kind == Kind::Native {
                let mut consumed = 0;
                let reconciled = (|| -> anyhow::Result<()> {
                    // An absent file has no surviving publication witness.
                    // Retain uncertain retirements; never replay a removal.
                    anyhow::ensure!(
                        !row.change.publication.anchors.is_empty()
                            || !row.change.publication.edges.is_empty(),
                        "Retirement has no surviving canonical publication witness"
                    );
                    let locator = row.change.locator.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("Older intent has no immutable source locator")
                    })?;
                    let (directory, name) = source_path(&self.owner.root, locator)?;
                    let data = match read(&directory, &name, scanning::STEP_BYTES - stats.bytes) {
                        Ok(data) => data,
                        Err(error) => {
                            consumed = scanning::STEP_BYTES - stats.bytes;
                            return Err(error);
                        }
                    }
                    .ok_or_else(|| anyhow::anyhow!("Pending source is absent"))?;
                    consumed = data.len();
                    let record: Value = serde_json::from_slice(&data)?;
                    let witness = &record[WITNESS]["witness"];
                    let pin = index::pins::Pin::read(&record, &self.owner.scope.boot)?
                        .ok_or_else(|| anyhow::anyhow!("Pending source is unqualified"))?;
                    let publication = pin.publication(
                        &record,
                        &row.change.source,
                        locator,
                        row.change.before + 1,
                        0,
                    )?;
                    anyhow::ensure!(
                        witness["epoch"] == self.scan.source.epoch
                            && witness["intent"] == row.change.id
                            && witness["seq"] == row.seq
                            && witness["revision"] == row.change.before + 1
                            && fingerprint(&publication, locator)?
                                == fingerprint(&row.change.publication, locator)?,
                        "Pending publication differs from canonical source"
                    );
                    self.owner.verify()?;
                    self.owner.store.settle(&Ticket {
                        epoch: self.scan.source.epoch.clone(),
                        id: row.change.id.clone(),
                        seq: row.seq,
                    })?;
                    Ok(())
                })();
                stats.bytes += consumed;
                if let Err(error) = reconciled {
                    stats.gaps += 1;
                    self.scan
                        .source
                        .gap(Some(&row.change.source), &error.to_string());
                }
            }
            stats.entries += 1;
            self.change_after = row.seq;
        }
        Ok(stats)
    }
    fn step(&mut self) -> anyhow::Result<Stats> {
        let until = Instant::now() + scanning::STEP_TIME;
        let file = self.owner._gate.try_clone()?;
        file.try_lock_exclusive()?;
        let _gate = Gate(file);
        self.owner.verify()?;
        let result = match self.phase {
            0 => {
                let before = self.scan.source.gaps;
                let step = self
                    .scan
                    .step_until(&mut self.owner.store, Instant::now, until)?;
                let stats = Stats {
                    entries: step.entries,
                    bytes: step.bytes,
                    rows: step.rows,
                    gaps: self.scan.source.gaps - before,
                    done: false,
                };
                if step.done {
                    self.phase = 1;
                }
                Ok(stats)
            }
            1 => self.sweep(until),
            2 => self.catchup(until),
            _ => Ok(Stats {
                done: true,
                ..Stats::default()
            }),
        };
        if Instant::now() < until {
            if let Some((source, reason)) = self.scan.source.last_gap.take() {
                lost(
                    &self.scan.source.index,
                    Some(&self.scan.source.epoch),
                    source.as_ref(),
                    &reason,
                );
            }
        }
        result
    }
}
pub(in crate::state) fn background_at(
    root: &Path,
    state: &mut Option<Background>,
) -> anyhow::Result<Stats> {
    if state.is_none() {
        *state = Some(Background::new(root)?);
    }
    let result = state
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Background bootstrap unavailable"))?
        .step();
    // A busy writer is a yield of the same cursor. Other failures invalidate
    // this pass's held namespace/epoch; rebuild in a later background step.
    let busy = result
        .as_ref()
        .err()
        .and_then(|error| error.downcast_ref::<std::io::Error>())
        .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock);
    if result.is_err() && !busy {
        *state = None;
    }
    result
}
