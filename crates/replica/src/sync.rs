//! Replicating a project between replicas: what each holds (its inventory, the
//! version of every document), the updates the other lacks, and importing them.
//! The inventory is bookkeeping, not a log, so nothing here grows without bound.
use crate::schema::{SessionKey, Stage};
use crate::store::{write_atomic, Damaged, ProjectStore};
use crate::task::TaskDoc;
use crate::{ids, Error, Result};
use loro::VersionVector;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A document's version: each Loro peer that changed it (its ID as decimal
/// text) and how many changes it made. Sorted, so equal versions compare and
/// hash alike on every replica.
pub type Version = BTreeMap<String, i32>;

/// What a replica holds of one project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    /// The project document; empty while it was never saved.
    pub project: Version,
    /// Each task document by UUID.
    pub tasks: BTreeMap<String, Version>,
}

impl Inventory {
    /// Equal for replicas that hold the same versions of the same documents.
    pub fn digest(&self) -> String {
        let canonical = serde_json::to_vec(self).expect("an inventory serializes");
        format!("{:x}", Sha256::digest(canonical))
    }
}

/// The document an update belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Document {
    Project,
    Task(String),
}

/// Loro updates for one document: what the other replica lacks of it, or the
/// whole document when it has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub document: Document,
    pub bytes: Vec<u8>,
}

/// One Done task as the phone lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoneRow {
    pub uuid: String,
    pub id: String,
    /// The start of the prompt, at most 200 characters.
    pub text: String,
    pub session: Option<SessionKey>,
    /// When it entered Done (Unix milliseconds).
    pub accepted_at: i64,
}

/// A page of the Done list; `next` is the cursor of the page after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DonePage {
    pub tasks: Vec<DoneRow>,
    pub next: Option<(i64, String)>,
    pub total: usize,
    pub recent: usize,
    /// The page's tasks that could not be read, left out of it.
    pub damaged: usize,
}

/// What an import changed, and the documents it refused.
#[derive(Debug, Default)]
pub struct Imported {
    pub changed: usize,
    pub refused: Vec<Damaged>,
}

const CACHE: &str = "inventory.json";
/// Beside the cache: the digest of the cache this build wrote. Only a build
/// that repairs the execution schema writes it, so rows an older binary
/// wrote or rewrote are never trusted and every task is read (and repaired)
/// again (zerus-cg5h.4 S2b.2).
const CACHE_MARK: &str = "inventory.repaired";

/// What the mark holds for the cache `bytes`.
fn cache_mark(bytes: &[u8]) -> String {
    format!("schema-repair-1 {:x}\n", Sha256::digest(bytes))
}

/// What tells a file changed without reading it: its length, modification
/// time and, on Unix, its inode (an atomic rewrite gets a new one).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp(u64, u64, u32, u64);

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&meta);
    #[cfg(not(unix))]
    let inode = 0;
    Some(Stamp(
        meta.len(),
        modified.as_secs(),
        modified.subsec_nanos(),
        inode,
    ))
}

/// The versions read last time, by the stamps of the files they came from.
#[derive(Default, PartialEq, Serialize, Deserialize)]
struct Cache {
    project: Option<Cached>,
    tasks: BTreeMap<String, Cached>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Cached {
    files: Vec<Option<Stamp>>,
    #[serde(default)]
    version: Version,
    /// Why the task does not load; it is read again only when its files change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// The task's stage, for the phone's view; an entry without one is read again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stage: Option<Stage>,
    /// When it entered that stage, for the Done list; an entry without it is read again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stage_at: Option<i64>,
    /// The session it was last sent to, for the Done list's filter; an entry without it is read again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sent: Option<Sent>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Sent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<SessionKey>,
}

/// A rewrite within one timestamp tick can hand a file its old inode back, so
/// stamps are trusted only for files older than this.
const SETTLE: Duration = Duration::from_secs(2);

fn settled(files: &[Option<Stamp>]) -> bool {
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };
    files
        .iter()
        .flatten()
        .all(|Stamp(_, secs, nanos, _)| Duration::new(*secs, *nanos) + SETTLE <= now)
}

fn version(vv: &VersionVector) -> Version {
    vv.iter()
        .map(|(peer, count)| (peer.to_string(), *count))
        .collect()
}

fn vector(version: &Version) -> Result<VersionVector> {
    version
        .iter()
        .map(|(peer, count)| {
            let peer = peer
                .parse::<u64>()
                .map_err(|_| Error::new(format!("invalid peer {peer:?} in a version")))?;
            Ok((peer, *count))
        })
        .collect()
}

/// Whether `theirs` holds every change of `ours`.
fn includes(theirs: &Version, ours: &Version) -> bool {
    ours.iter()
        .all(|(peer, count)| theirs.get(peer).is_some_and(|known| known >= count))
}

impl ProjectStore {
    /// The project IDs stored below `root`.
    pub fn stored(root: &Path) -> Result<Vec<String>> {
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => return Err(error.into()),
        };
        let mut out = Vec::new();
        for entry in entries {
            if let Ok(id) = fs::read_to_string(entry?.path().join("project-id")) {
                out.push(id);
            }
        }
        out.sort();
        Ok(out)
    }

    /// The version of every document of the project, and the tasks that do not
    /// load. A task is read again only when its files changed since last time.
    pub fn inventory(&self) -> Result<(Inventory, Vec<Damaged>)> {
        let (inventory, _, damaged) = self.inventory_with_stages()?;
        Ok((inventory, damaged))
    }

    /// What a phone holds of this project: the project document and every task
    /// that is not Done (docs/superpowers/specs/2026-10-18-task-replication-loro-design.md, "Phone").
    pub fn live_inventory(&self) -> Result<(Inventory, Vec<Damaged>)> {
        let (mut inventory, stages, damaged) = self.inventory_with_stages()?;
        inventory
            .tasks
            .retain(|uuid, _| stages.get(uuid).map(|s| s.0) != Some(Stage::Done));
        Ok((inventory, damaged))
    }

    /// What a phone holding `theirs` is answered from: the live documents and
    /// every task it already holds, so a task that became Done still reaches it.
    pub fn phone_inventory(&self, theirs: &Inventory) -> Result<Inventory> {
        let (mut inventory, stages, _) = self.inventory_with_stages()?;
        inventory.tasks.retain(|uuid, _| {
            stages.get(uuid).map(|s| s.0) != Some(Stage::Done) || theirs.tasks.contains_key(uuid)
        });
        Ok(inventory)
    }

    /// The Done tasks newest first (when they entered Done, then UUID, both
    /// descending): at most `limit` after `before`, with how many there are and
    /// how many entered Done at or after `since`; with `session`, only the
    /// tasks sent to it. Only the page's documents are read; one that cannot be
    /// read is left out of its page.
    pub fn done_page(
        &self,
        before: Option<(i64, String)>,
        limit: usize,
        since: i64,
        session: Option<&SessionKey>,
    ) -> Result<DonePage> {
        let (_, stages, _) = self.inventory_with_stages()?;
        let mut done: Vec<(i64, String)> = stages
            .into_iter()
            .filter(|(_, (stage, _, sent))| {
                *stage == Stage::Done
                    && session.is_none_or(|session| {
                        sent.as_ref().is_some_and(|sent| {
                            if session.session_id.is_empty() {
                                sent.machine == session.machine && sent.session == session.session
                            } else {
                                sent == session
                            }
                        })
                    })
            })
            .map(|(uuid, (_, at, _))| (at, uuid))
            .collect();
        done.sort_unstable_by(|a, b| b.cmp(a));
        let total = done.len();
        let recent = done.iter().filter(|(at, _)| *at >= since).count();
        let start = before.map_or(0, |cursor| done.partition_point(|entry| *entry >= cursor));
        let end = (start + limit).min(total);
        let mut tasks = Vec::new();
        let mut damaged = 0;
        for (at, uuid) in &done[start..end] {
            let Ok(view) = self.load(uuid).and_then(|task| task.view()) else {
                damaged += 1;
                continue;
            };
            tasks.push(DoneRow {
                uuid: uuid.clone(),
                id: view.id,
                text: view.text.chars().take(200).collect(),
                session: view.session,
                accepted_at: *at,
            });
        }
        Ok(DonePage {
            tasks,
            next: (start < end && end < total).then(|| done[end - 1].clone()),
            total,
            recent,
            damaged,
        })
    }

    /// The inventory with each loaded task's stage and when it entered it.
    #[allow(clippy::type_complexity)]
    fn inventory_with_stages(
        &self,
    ) -> Result<(
        Inventory,
        BTreeMap<String, (Stage, i64, Option<SessionKey>)>,
        Vec<Damaged>,
    )> {
        let path = self.dir().join(CACHE);
        let mark = self.dir().join(CACHE_MARK);
        let old: Cache = fs::read(&path)
            .ok()
            .filter(|bytes| fs::read_to_string(&mark).ok() == Some(cache_mark(bytes)))
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut cache = Cache::default();
        let mut inventory = Inventory::default();
        let mut stages = BTreeMap::new();
        let mut damaged = Vec::new();
        let project = vec![stamp(&self.dir().join("project.loro"))];
        if project[0].is_some() {
            let trusted = settled(&project);
            let cached = match old
                .project
                .as_ref()
                .filter(|c| trusted && c.files == project)
            {
                Some(cached) => cached.clone(),
                None => Cached {
                    files: project,
                    version: version(&VersionVector::decode(&self.project()?.version())?),
                    error: None,
                    stage: None,
                    stage_at: None,
                    sent: None,
                },
            };
            inventory.project = cached.version.clone();
            if trusted {
                cache.project = Some(cached);
            }
        }
        for uuid in self.uuids()? {
            let files = vec![
                stamp(&self.snapshot_path(&uuid)),
                stamp(&self.updates_path(&uuid)),
            ];
            let trusted = settled(&files);
            let remembered = old.tasks.get(&uuid).filter(|c| {
                trusted
                    && c.files == files
                    && (c.error.is_some()
                        || (c.stage.is_some() && c.stage_at.is_some() && c.sent.is_some()))
            });
            let cached = match remembered {
                Some(cached) => cached.clone(),
                None => match self.load(&uuid).and_then(|task| self.repaired(task)) {
                    Ok((task, saved)) => Cached {
                        files: saved.unwrap_or(files),
                        version: version(&task.current_vv()),
                        error: None,
                        stage: Some(task.stage()?),
                        stage_at: Some(task.stage_at()),
                        sent: Some(Sent {
                            session: task.session(),
                        }),
                    },
                    Err(error) => Cached {
                        files,
                        version: Version::new(),
                        error: Some(error.to_string()),
                        stage: None,
                        stage_at: None,
                        sent: None,
                    },
                },
            };
            match &cached.error {
                Some(error) => damaged.push(Damaged {
                    uuid: uuid.clone(),
                    error: error.clone(),
                }),
                None => {
                    inventory.tasks.insert(uuid.clone(), cached.version.clone());
                    if let (Some(stage), Some(at)) = (cached.stage, cached.stage_at) {
                        let sent = cached.sent.as_ref().and_then(|sent| sent.session.clone());
                        stages.insert(uuid.clone(), (stage, at, sent));
                    }
                }
            }
            if trusted {
                cache.tasks.insert(uuid, cached);
            }
        }
        if cache != old {
            // Only a speed-up: an unwritten cache means reading the tasks again.
            // The mark follows the cache; until it does, the cache is not trusted.
            let bytes = serde_json::to_vec(&cache)?;
            let _ = fs::remove_file(&mark);
            if write_atomic(&path, &bytes).is_ok() {
                let _ = write_atomic(&mark, cache_mark(&bytes).as_bytes());
            }
        }
        Ok((inventory, stages, damaged))
    }

    /// The updates a replica holding `theirs` lacks of `ours` (this store's
    /// inventory), in pages of at most `limit` bytes; a document larger than
    /// that comes alone. True when more remain after this page.
    pub fn updates_for(
        &self,
        ours: &Inventory,
        theirs: &Inventory,
        limit: usize,
    ) -> Result<(Vec<Update>, bool)> {
        let mut page = Vec::new();
        let mut size = 0;
        let mut add = |update: Update| {
            if !page.is_empty() && size + update.bytes.len() > limit {
                return false;
            }
            size += update.bytes.len();
            page.push(update);
            true
        };
        if !ours.project.is_empty() && !includes(&theirs.project, &ours.project) {
            let bytes = self
                .project()?
                .export_since(&vector(&theirs.project)?.encode())?;
            if !add(Update {
                document: Document::Project,
                bytes,
            }) {
                return Ok((page, true));
            }
        }
        for (uuid, mine) in &ours.tasks {
            let known = theirs.tasks.get(uuid);
            if known.is_some_and(|known| includes(known, mine)) {
                continue;
            }
            let task = self.load(uuid)?;
            let bytes = match known {
                Some(known) => task.export_since_vv(&vector(known)?)?,
                None => task.snapshot()?,
            };
            if !add(Update {
                document: Document::Task(uuid.clone()),
                bytes,
            }) {
                return Ok((page, true));
            }
        }
        Ok((page, false))
    }

    /// A loaded task whose execution schema a merged lower value replaced is
    /// repaired and saved under this store's lock before its version is used,
    /// so the repair itself is what other replicas receive. Returns the task
    /// and, when it was saved, its files' new stamps.
    #[allow(clippy::type_complexity)]
    fn repaired(&self, mut task: TaskDoc) -> Result<(TaskDoc, Option<Vec<Option<Stamp>>>)> {
        if !task.repair_schema()? {
            return Ok((task, None));
        }
        self.save(&mut task)?;
        let uuid = task.uuid().to_owned();
        Ok((
            task,
            Some(vec![
                stamp(&self.snapshot_path(&uuid)),
                stamp(&self.updates_path(&uuid)),
            ]),
        ))
    }

    /// Imports another replica's updates. A document that cannot take them is
    /// refused and named; it never stops the others.
    pub fn import_updates(&self, updates: Vec<Update>) -> Result<Imported> {
        let mut imported = Imported::default();
        for update in updates {
            let (name, result) = match &update.document {
                Document::Project => ("project".to_owned(), self.import_project(&update.bytes)),
                Document::Task(uuid) => (uuid.clone(), self.import_task(uuid, &update.bytes)),
            };
            match result {
                Ok(changed) => imported.changed += usize::from(changed),
                Err(error) => imported.refused.push(Damaged {
                    uuid: name,
                    error: error.to_string(),
                }),
            }
        }
        Ok(imported)
    }

    fn import_project(&self, bytes: &[u8]) -> Result<bool> {
        let mut doc = self.project()?;
        let before = VersionVector::decode(&doc.version())?;
        doc.import(bytes)?;
        if VersionVector::decode(&doc.version())? == before {
            return Ok(false);
        }
        self.save_project(&doc)?;
        Ok(true)
    }

    fn import_task(&self, uuid: &str, bytes: &[u8]) -> Result<bool> {
        if !ids::is_uuid(uuid) {
            return Err(Error::new(format!("{uuid:?} is not a task UUID")));
        }
        let stored = self.snapshot_path(uuid).exists() || self.updates_path(uuid).exists();
        let (mut task, replacing) = if stored {
            match self.load(uuid) {
                Ok(mut task) => {
                    let before = task.current_vv();
                    if !task.import_block(bytes) {
                        return Err(Error::new("the updates could not be applied"));
                    }
                    // Every live import restores the execution schema a merged
                    // lower value replaced, before anything reads or exports it.
                    task.repair_schema()?;
                    if task.current_vv() == before {
                        return Ok(false);
                    }
                    (task, false)
                }
                // A copy here that does not load gives way to a whole document
                // from another replica only when local proof shows nothing of
                // the cutover to execution attempts can be lost (`recovered`);
                // its files are set aside, not removed. Otherwise it stays
                // damaged and closed for a person's review.
                Err(damaged) => match self.recovered(uuid, bytes) {
                    Recovery::Whole(task) | Recovery::Replaced(task) => (task, true),
                    Recovery::Unreadable => return Err(damaged),
                    Recovery::Kept => {
                        return Err(Error::new(format!(
                            "the copy here does not load ({damaged}) and may hold execution \
                             attempts no other copy is shown to hold; it is kept for a person's \
                             review and nothing is sent for it"
                        )))
                    }
                },
            }
        } else if self.was_managed(uuid) {
            // Its files are gone, but the mark says this store cut it over:
            // nothing here can show that another replica's copy holds all of
            // it. Removing the mark is the person's decision to accept one.
            return Err(Error::new(
                "the copy here is missing and it recorded execution attempts; it is kept for a \
                 person's review and nothing is sent for it",
            ));
        } else {
            (TaskDoc::from_bytes(bytes)?, false)
        };
        task.repair_schema()?;
        if task.uuid() != uuid {
            return Err(Error::new("the update is for another task"));
        }
        let view = task.view()?;
        if view.project != self.project_id() {
            return Err(Error::new("the task belongs to another project"));
        }
        if replacing {
            self.set_aside(uuid)?;
        }
        self.save(&mut task)?;
        Ok(true)
    }

    /// How a copy here that does not load meets another replica's whole
    /// document. The valid prefix of the update blocks saved here is always
    /// merged into that document before anything is decided.
    ///
    /// A copy this store cut over to the execution schema is rebuilt only
    /// from local proof: its mark is readable and fresh (the updates file
    /// still opens with the block it names, so no other writer folded or
    /// rewrote the files since), every block applies, and the result holds
    /// the snapshot version it records. The snapshot and every block are then
    /// inside the result. Anything less keeps it for a person's review.
    ///
    /// A copy saved without a mark gives way only when every local block
    /// applies and the result shows no attempts or floor; a block that does
    /// not read or apply may hold them, so it keeps the copy too.
    fn recovered(&self, uuid: &str, bytes: &[u8]) -> Recovery {
        let Ok(mut task) = TaskDoc::from_bytes(bytes) else {
            return Recovery::Unreadable;
        };
        let local = match fs::read(self.updates_path(uuid)) {
            Ok(local) => local,
            Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
            Err(_) => return Recovery::Kept,
        };
        let (found, intact) = crate::store::blocks(&local);
        let applied = found
            .into_iter()
            .all(|(payload, _)| task.import_block(payload));
        let whole = intact && applied;
        if !self.was_managed(uuid) {
            return if whole && !task.execution_managed() {
                Recovery::Replaced(task)
            } else {
                Recovery::Kept
            };
        }
        match self.fresh_snapshot(uuid) {
            Some(snapshot) if whole && task.current_vv().includes_vv(&snapshot) => {
                Recovery::Whole(task)
            }
            _ => Recovery::Kept,
        }
    }

    /// Copies a task's files that do not load to `<name>.damaged-<ms>`, kept
    /// on disk. The files themselves stay until the save that follows
    /// replaces them, so a crash before it leaves them to be read again.
    fn set_aside(&self, uuid: &str) -> Result<()> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        for path in [self.snapshot_path(uuid), self.updates_path(uuid)] {
            match fs::read(&path) {
                Ok(bytes) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    write_atomic(
                        &path.with_file_name(format!("{name}.damaged-{stamp}")),
                        &bytes,
                    )?;
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.note_change(uuid);
        Ok(())
    }
}

enum Recovery {
    /// This copy's whole state with the other document merged in.
    Whole(TaskDoc),
    /// The other document, with every block saved here, replaces this copy.
    Replaced(TaskDoc),
    /// The other document does not load either.
    Unreadable,
    /// This copy stays damaged for a person's review.
    Kept,
}

#[cfg(test)]
mod tests {
    //! The live-store repair of the execution schema (zerus-cg5h.4 S2b.2):
    //! a lower value an older replica's concurrent upgrade left in a merge is
    //! raised again under the project lock before the task is saved, read for
    //! an inventory or exported.
    use super::*;
    use crate::execution::{NativeRequest, Origin, EXECUTION_TASK_SCHEMA};
    use crate::store::Mark;
    use crate::{Actor, NewTask, Target};

    const ID: &str = "11111111-1111-4111-8111-111111111111";

    fn created() -> TaskDoc {
        TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-a1b2c3",
            text: "Add rate limiting",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap()
    }

    fn reserve(task: &mut TaskDoc) {
        let executor = SessionKey {
            machine: "m1".into(),
            session: "codex/api".into(),
            session_id: "launch_id:api".into(),
        };
        let native = NativeRequest {
            machine: "m1".into(),
            name: "codex/api".into(),
            run_id: "run-api".into(),
            conversation_id: "conversation-api".into(),
            digest: "ab".repeat(32),
        };
        task.reserve_attempt(ID, Origin::Send, &executor, native, &Actor::Person, 10)
            .unwrap();
    }

    /// A newer replica reserves the first attempt while an older one, forked
    /// from the same task, makes enough changes ending in its own schema
    /// upgrade that the merge keeps that lower value. Returns the newer copy
    /// and the older one's changes since the fork.
    fn race() -> (TaskDoc, Vec<u8>) {
        let base = created();
        let bytes = base.snapshot().unwrap();
        let version = base.version();
        let mut newer = TaskDoc::from_bytes(&bytes).unwrap();
        reserve(&mut newer);
        let mut older = TaskDoc::from_bytes(&bytes).unwrap();
        for n in 0..8 {
            older
                .comment(&format!("older {n}"), &Actor::Person, 2)
                .unwrap();
        }
        older.set_stored_schema_for_test(5);
        (newer, older.export_since(&version).unwrap())
    }

    /// The merge as an importer without the repair leaves it.
    fn merged_unrepaired() -> TaskDoc {
        let (newer, older) = race();
        let mut merged = TaskDoc::from_bytes(&newer.snapshot().unwrap()).unwrap();
        merged.import_unrepaired_for_test(&older);
        assert_eq!(merged.stored_schema(), 5, "the older value wins this merge");
        assert!(merged.needs_schema_repair());
        merged
    }

    fn update(task: &TaskDoc, bytes: Vec<u8>) -> Update {
        Update {
            document: Document::Task(task.uuid().to_owned()),
            bytes,
        }
    }

    #[test]
    fn a_live_import_saves_the_execution_schema_a_merge_lowered() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        let (mut newer, older) = race();
        store.save(&mut newer).unwrap();
        let imported = store.import_updates(vec![update(&newer, older)]).unwrap();
        assert_eq!((imported.changed, imported.refused.len()), (1, 0));
        let saved = store.load(newer.uuid()).unwrap();
        assert_eq!(saved.stored_schema(), EXECUTION_TASK_SCHEMA);
        // Attempts and their facts are unchanged by the repair.
        assert_eq!(
            saved.execution_records().unwrap(),
            newer.execution_records().unwrap()
        );
        // An import that changes nothing still repairs a copy that needs it.
        let mut stale = merged_unrepaired();
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        store.save(&mut stale).unwrap();
        let again = store
            .import_updates(vec![update(&stale, stale.snapshot().unwrap())])
            .unwrap();
        assert_eq!(again.refused.len(), 0);
        assert_eq!(
            store.load(stale.uuid()).unwrap().stored_schema(),
            EXECUTION_TASK_SCHEMA
        );
    }

    #[test]
    fn an_inventory_repairs_a_copy_an_older_importer_saved_and_exports_the_repair() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        let mut stale = merged_unrepaired();
        store.save(&mut stale).unwrap();
        let stale_version = version(&stale.current_vv());
        let (inventory, damaged) = store.inventory().unwrap();
        assert!(damaged.is_empty());
        let saved = store.load(stale.uuid()).unwrap();
        assert_eq!(saved.stored_schema(), EXECUTION_TASK_SCHEMA);
        let listed = &inventory.tasks[stale.uuid()];
        assert_eq!(listed, &version(&saved.current_vv()));
        assert_ne!(listed, &stale_version);
        // A replica that holds the stale copy receives the repair.
        let theirs = Inventory {
            project: Version::new(),
            tasks: BTreeMap::from([(stale.uuid().to_owned(), stale_version)]),
        };
        let (page, _) = store.updates_for(&inventory, &theirs, 1 << 20).unwrap();
        let mut peer = TaskDoc::from_bytes(&stale.snapshot().unwrap()).unwrap();
        let bytes = page
            .into_iter()
            .find(|u| u.document == Document::Task(stale.uuid().to_owned()))
            .unwrap()
            .bytes;
        peer.import_unrepaired_for_test(&bytes);
        assert_eq!(peer.stored_schema(), EXECUTION_TASK_SCHEMA);
    }

    #[test]
    fn an_older_binary_s_inventory_cache_never_bypasses_the_repair() {
        // review-n on #1402: an older importer persisted a lower-won copy and
        // its own valid cache row for it (same format, matching stamps).
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        let mut stale = merged_unrepaired();
        store.save(&mut stale).unwrap();
        let old_time = SystemTime::now() - Duration::from_secs(60);
        for path in [
            store.snapshot_path(stale.uuid()),
            store.updates_path(stale.uuid()),
        ] {
            if path.exists() {
                fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_times(fs::FileTimes::new().set_modified(old_time))
                    .unwrap();
            }
        }
        let files = vec![
            stamp(&store.snapshot_path(stale.uuid())),
            stamp(&store.updates_path(stale.uuid())),
        ];
        assert!(settled(&files));
        let before = version(&stale.current_vv());
        let older = Cache {
            project: None,
            tasks: BTreeMap::from([(
                stale.uuid().to_owned(),
                Cached {
                    files,
                    version: before.clone(),
                    error: None,
                    stage: Some(Stage::Todo),
                    stage_at: Some(1),
                    sent: Some(Sent { session: None }),
                },
            )]),
        };
        fs::write(store.dir().join(CACHE), serde_json::to_vec(&older).unwrap()).unwrap();
        let (ours, damaged) = store.inventory().unwrap();
        assert!(damaged.is_empty());
        assert_ne!(ours.tasks[stale.uuid()], before);
        assert_eq!(
            store.load(stale.uuid()).unwrap().stored_schema(),
            EXECUTION_TASK_SCHEMA
        );
        let (page, _) = store
            .updates_for(&ours, &Inventory::default(), 1 << 20)
            .unwrap();
        let exported = page
            .iter()
            .find(|u| u.document == Document::Task(stale.uuid().to_owned()))
            .unwrap();
        assert_eq!(
            TaskDoc::from_bytes(&exported.bytes)
                .unwrap()
                .stored_schema(),
            EXECUTION_TASK_SCHEMA
        );
        // The cache this build wrote is trusted again.
        let (again, _) = store.inventory().unwrap();
        assert_eq!(again, ours);
        let mark = fs::read_to_string(store.dir().join(CACHE_MARK)).unwrap();
        assert_eq!(
            mark,
            cache_mark(&fs::read(store.dir().join(CACHE)).unwrap())
        );
    }

    #[test]
    fn a_floored_task_without_attempts_is_repaired_by_imports_and_inventories() {
        // zerus-cg5h.4 S2b.2 C1: the floor before a first attempt is kept at 6
        // just like attempts are.
        let base = created();
        let bytes = base.snapshot().unwrap();
        let version = base.version();
        let mut floored = TaskDoc::from_bytes(&bytes).unwrap();
        assert!(floored.floor().unwrap());
        let mut older = TaskDoc::from_bytes(&bytes).unwrap();
        for n in 0..8 {
            older
                .comment(&format!("older {n}"), &Actor::Person, 2)
                .unwrap();
        }
        older.set_stored_schema_for_test(5);
        let older_update = older.export_since(&version).unwrap();
        // A live import saves it at 6.
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        store.save(&mut floored).unwrap();
        store
            .import_updates(vec![update(&floored, older_update.clone())])
            .unwrap();
        let saved = store.load(floored.uuid()).unwrap();
        assert_eq!(saved.stored_schema(), EXECUTION_TASK_SCHEMA);
        assert!(!saved.executes_attempts());
        // A copy an older importer persisted at 5 is repaired by the inventory
        // and exported at 6.
        let mut stale = TaskDoc::from_bytes(&floored.snapshot().unwrap()).unwrap();
        stale.import_unrepaired_for_test(&older_update);
        assert_eq!(stale.stored_schema(), 5);
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        store.save(&mut stale).unwrap();
        let (inventory, _) = store.inventory().unwrap();
        assert_eq!(
            store.load(stale.uuid()).unwrap().stored_schema(),
            EXECUTION_TASK_SCHEMA
        );
        let (page, _) = store
            .updates_for(&inventory, &Inventory::default(), 1 << 20)
            .unwrap();
        let exported = page
            .iter()
            .find(|u| u.document == Document::Task(stale.uuid().to_owned()))
            .unwrap();
        assert_eq!(
            TaskDoc::from_bytes(&exported.bytes)
                .unwrap()
                .stored_schema(),
            EXECUTION_TASK_SCHEMA
        );
    }

    #[test]
    fn every_write_keeps_the_schema_and_a_future_one_is_never_lowered() {
        let mut stale = merged_unrepaired();
        stale
            .comment("A newer comment", &Actor::Person, 30)
            .unwrap();
        assert_eq!(stale.stored_schema(), EXECUTION_TASK_SCHEMA);
        // A document of a newer schema is left as it is: read-only here.
        let mut newer = created();
        reserve(&mut newer);
        newer.set_stored_schema_for_test(EXECUTION_TASK_SCHEMA + 1);
        assert!(!newer.repair_schema().unwrap());
        let other = TaskDoc::from_bytes(&newer.snapshot().unwrap()).unwrap();
        newer.import(&other.snapshot().unwrap()).unwrap();
        assert_eq!(newer.stored_schema(), EXECUTION_TASK_SCHEMA + 1);
        assert!(!newer.view().unwrap().writable);
        // A task without attempts is never raised.
        let mut plain = created();
        assert!(!plain.repair_schema().unwrap());
        assert!(plain.stored_schema() < EXECUTION_TASK_SCHEMA);
    }

    /// A store copy of `created()` beside a peer's copy forked before the
    /// first attempt (after a saved floor when `floored`): the store's copy
    /// reserves it as an update block after its snapshot. Returns the store,
    /// its directory, the task's UUID and the peer's whole document, with
    /// an offline comment unless floored (a phone cannot edit a floored task).
    fn beside_a_peer(floored: bool) -> (tempfile::TempDir, ProjectStore, String, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        let mut task = created();
        store.save(&mut task).unwrap();
        if floored {
            assert!(task.floor().unwrap());
            store.save(&mut task).unwrap();
        }
        let mut peer = TaskDoc::from_bytes(&task.snapshot().unwrap()).unwrap();
        if !floored {
            peer.comment("Offline on the peer", &Actor::Person, 5)
                .unwrap();
        }
        let mut here = store.load(task.uuid()).unwrap();
        reserve(&mut here);
        store.save(&mut here).unwrap();
        assert!(store.updates_path(task.uuid()).exists());
        let uuid = task.uuid().to_owned();
        (dir, store, uuid, peer.snapshot().unwrap())
    }

    fn damaged_beside_a_pre_attempt_peer() -> (tempfile::TempDir, ProjectStore, String, Vec<u8>) {
        beside_a_peer(false)
    }

    fn damage(store: &ProjectStore, uuid: &str) {
        fs::write(store.snapshot_path(uuid), b"unreadable snapshot fixture").unwrap();
    }

    /// The files beside a task's name, as bytes (a directory or link by kind).
    fn files(store: &ProjectStore, uuid: &str) -> BTreeMap<String, String> {
        fs::read_dir(store.snapshot_path(uuid).parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(uuid))
            .map(|p| {
                let meta = fs::symlink_metadata(&p).unwrap();
                let shown = if meta.is_file() {
                    format!("{:?}", fs::read(&p).map_err(|e| e.kind()))
                } else {
                    format!("{:?}", meta.file_type())
                };
                (p.file_name().unwrap().to_string_lossy().into_owned(), shown)
            })
            .collect()
    }

    /// Two imports of `peer` are refused for a person's review, and neither
    /// renames, removes or copies a file.
    fn kept(store: &ProjectStore, uuid: &str, peer: &[u8]) {
        let before = files(store, uuid);
        for _ in 0..2 {
            let refusals = refused(store, uuid, peer.to_vec());
            assert_eq!(refusals.len(), 1);
            assert!(
                refusals[0].error.contains("person's review"),
                "{}",
                refusals[0].error
            );
            assert!(store.load(uuid).is_err());
            assert_eq!(files(store, uuid), before);
        }
    }

    /// A cut-over task's updates file after a fold: one block of no changes.
    fn only_the_opening_block(store: &ProjectStore, uuid: &str) {
        let bytes = fs::read(store.updates_path(uuid)).unwrap();
        let (found, intact) = crate::store::blocks(&bytes);
        assert!(intact && found.len() == 1, "{} blocks", found.len());
        let mut task = store.load(uuid).unwrap();
        let before = task.current_vv();
        assert!(task.import_block(found[0].0));
        assert_eq!(task.current_vv(), before);
    }

    /// What a build before the mark does when its importer folds: the
    /// snapshot rewritten from its merged copy, the updates file removed,
    /// the mark left as it was.
    fn fold_as_an_older_build(store: &ProjectStore, uuid: &str, peer: &[u8]) {
        let mut task = store.load(uuid).unwrap();
        let mut other = TaskDoc::from_bytes(peer).unwrap();
        for n in 0..6 {
            let version = other.version();
            other
                .comment(&format!("Peer comment {n}"), &Actor::Person, 20 + n)
                .unwrap();
            task.import_unrepaired_for_test(&other.export_since(&version).unwrap());
        }
        write_atomic(&store.snapshot_path(uuid), &task.snapshot().unwrap()).unwrap();
        fs::remove_file(store.updates_path(uuid)).unwrap();
    }

    fn set_aside_files(store: &ProjectStore, uuid: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(store.snapshot_path(uuid).parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(uuid) && name.contains(".damaged-"))
            .collect();
        names.sort();
        names
    }

    fn refused(store: &ProjectStore, uuid: &str, bytes: Vec<u8>) -> Vec<Damaged> {
        let update = Update {
            document: Document::Task(uuid.to_owned()),
            bytes,
        };
        store.import_updates(vec![update]).unwrap().refused
    }

    fn mark_version(store: &ProjectStore, uuid: &str) -> Option<VersionVector> {
        store.mark(uuid)?.version()
    }

    #[test]
    fn the_managed_mark_names_the_snapshot_and_the_block_that_follows_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        let mut task = created();
        store.save(&mut task).unwrap();
        assert!(!store.was_managed(task.uuid()));
        let snapshot = task.current_vv();
        // The cutover saved as an update block: the mark names the snapshot
        // below it and that block, the first of the updates file.
        reserve(&mut task);
        store.save(&mut task).unwrap();
        assert_eq!(mark_version(&store, task.uuid()), Some(snapshot.clone()));
        assert_eq!(store.fresh_snapshot(task.uuid()), Some(snapshot.clone()));
        let mark = store.mark(task.uuid());
        task.comment("Appended", &Actor::Person, 11).unwrap();
        store.save(&mut task).unwrap();
        assert_eq!(store.mark(task.uuid()), mark);
        // A fold moves the mark with the snapshot and opens a new updates
        // file with a block of no changes, which this build and older ones
        // read as nothing.
        task.needs_fold = true;
        task.comment("Folded", &Actor::Person, 12).unwrap();
        store.save(&mut task).unwrap();
        only_the_opening_block(&store, task.uuid());
        assert_eq!(store.fresh_snapshot(task.uuid()), Some(task.current_vv()));
        assert_ne!(mark_version(&store, task.uuid()), Some(snapshot));
        // A stale mark beside readable files is corrected by the next save.
        store
            .write_mark(task.uuid(), &Mark::new(&created().current_vv(), b"stale"))
            .unwrap();
        assert!(store.fresh_snapshot(task.uuid()).is_none());
        task.comment("Corrected", &Actor::Person, 13).unwrap();
        store.save(&mut task).unwrap();
        let reloaded = store.load(task.uuid()).unwrap();
        assert_eq!(store.fresh_snapshot(task.uuid()), reloaded.folded);
        // A task that never cut over still folds to no updates file.
        let mut plain = created();
        store.save(&mut plain).unwrap();
        plain.needs_fold = true;
        plain.comment("Plain", &Actor::Person, 2).unwrap();
        store.save(&mut plain).unwrap();
        assert!(!store.updates_path(plain.uuid()).exists());
        assert!(!store.was_managed(plain.uuid()));
    }

    /// review-n's control on #1409: a damaged snapshot beside valid update
    /// blocks is rebuilt from the peer's document and those blocks, keeping
    /// every attempt fact, and the damaged files are set aside.
    #[test]
    fn a_damaged_managed_copy_is_rebuilt_from_its_own_updates() {
        let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
        let before = store.load(&uuid).unwrap();
        let facts = before.execution_records().unwrap();
        damage(&store, &uuid);
        assert!(store.load(&uuid).is_err());
        assert!(refused(&store, &uuid, peer).is_empty());
        let rebuilt = store.load(&uuid).unwrap();
        assert_eq!(rebuilt.execution_records().unwrap(), facts);
        assert!(rebuilt.held().unwrap().is_some());
        assert_eq!(rebuilt.stored_schema(), EXECUTION_TASK_SCHEMA);
        assert!(rebuilt.check_send().is_err());
        let comments = rebuilt
            .view()
            .unwrap()
            .log
            .iter()
            .filter(|e| e.kind == "comment")
            .count();
        assert_eq!(comments, 1, "the peer's comment is merged in");
        assert_eq!(set_aside_files(&store, &uuid).len(), 2);
        assert_eq!(store.fresh_snapshot(&uuid), rebuilt.folded);
        only_the_opening_block(&store, &uuid);
    }

    #[test]
    fn a_damaged_managed_copy_the_peer_cannot_cover_is_kept_for_review() {
        // The attempt only the lost snapshot held: this build's fold took it in.
        let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
        let mut folded = store.load(&uuid).unwrap();
        folded.needs_fold = true;
        store.save(&mut folded).unwrap();
        only_the_opening_block(&store, &uuid);
        damage(&store, &uuid);
        kept(&store, &uuid, &peer);
        assert!(store.tasks().unwrap().1.iter().any(|d| d.uuid == uuid));

        // A peer that holds the attempt but not what a later fold took in
        // (here the settlement of its delivery) cannot cover it either.
        let (_dir, store, uuid, _) = damaged_beside_a_pre_attempt_peer();
        let mut here = store.load(&uuid).unwrap();
        let cut_over = here.snapshot().unwrap();
        here.cancel_attempt(ID, "Settled only here", &Actor::Person, 11)
            .unwrap();
        here.needs_fold = true;
        store.save(&mut here).unwrap();
        damage(&store, &uuid);
        kept(&store, &uuid, &cut_over);

        // Update blocks whose base the peer lacks cannot rebuild it either.
        let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
        let mut here = store.load(&uuid).unwrap();
        here.comment("Only here", &Actor::Person, 11).unwrap();
        here.needs_fold = true;
        store.save(&mut here).unwrap();
        here.comment("After the fold", &Actor::Person, 12).unwrap();
        store.save(&mut here).unwrap();
        damage(&store, &uuid);
        kept(&store, &uuid, &peer);

        // A torn last block, a torn first block, unreadable updates or an
        // unreadable mark keeps the copy too.
        for spoil in 0..4 {
            let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
            let path = store.updates_path(&uuid);
            let mut updates = fs::read(&path).unwrap();
            match spoil {
                0 => {
                    updates.push(7);
                    fs::write(&path, &updates).unwrap();
                }
                1 => {
                    updates.truncate(updates.len() / 2);
                    fs::write(&path, &updates).unwrap();
                }
                2 => {
                    fs::remove_file(&path).unwrap();
                    fs::create_dir(&path).unwrap();
                }
                _ => fs::write(store.managed_path(&uuid), b"unreadable mark").unwrap(),
            }
            damage(&store, &uuid);
            kept(&store, &uuid, &peer);
        }
    }

    /// review-n on #1415 (R2): a build before the mark folds the copy here
    /// after the attempt was saved as an update block, leaving the mark as
    /// it was. That mark no longer describes the files, so no other copy is
    /// accepted, floored or not. The fold is reproduced as that build's save
    /// does it; the literal older build does the same in the S2b.2 harness.
    #[test]
    fn a_stale_mark_after_an_older_build_s_fold_never_accepts_another_copy() {
        for floored in [false, true] {
            let (_dir, store, uuid, peer) = beside_a_peer(floored);
            let mark = fs::read(store.managed_path(&uuid)).unwrap();
            fold_as_an_older_build(&store, &uuid, &peer);
            assert_eq!(fs::read(store.managed_path(&uuid)).unwrap(), mark);
            let folded = store.load(&uuid).unwrap();
            assert!(folded.held().unwrap().is_some(), "floored={floored}");
            assert!(store.fresh_snapshot(&uuid).is_none());
            damage(&store, &uuid);
            kept(&store, &uuid, &peer);
            // And after that build appends again.
            let (_dir, store, uuid, peer) = beside_a_peer(floored);
            fold_as_an_older_build(&store, &uuid, &peer);
            let mut task = store.load(&uuid).unwrap();
            let version = task.current_vv();
            task.import_unrepaired_for_test(&peer);
            task.comment("Older build", &Actor::Person, 40).unwrap();
            let mut file = fs::File::options()
                .create(true)
                .append(true)
                .open(store.updates_path(&uuid))
                .unwrap();
            std::io::Write::write_all(
                &mut file,
                &crate::store::block_for_test(&task.export_since_vv(&version).unwrap()),
            )
            .unwrap();
            damage(&store, &uuid);
            kept(&store, &uuid, &peer);
        }
        // A mark restored from before this build's own fold is stale too.
        let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
        let mark = fs::read(store.managed_path(&uuid)).unwrap();
        let mut task = store.load(&uuid).unwrap();
        task.needs_fold = true;
        store.save(&mut task).unwrap();
        fs::write(store.managed_path(&uuid), mark).unwrap();
        damage(&store, &uuid);
        kept(&store, &uuid, &peer);
    }

    /// review-n on #1415 (R2): without a mark, the valid prefix of the local
    /// blocks is read before anything is decided; attempts or a floor in it
    /// keep the copy, and so does a block that cannot be read or applied.
    #[test]
    fn an_unmarked_copy_keeps_whatever_its_blocks_may_hold() {
        for torn in [false, true] {
            let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
            fs::remove_file(store.managed_path(&uuid)).unwrap();
            if torn {
                let mut updates = fs::read(store.updates_path(&uuid)).unwrap();
                updates.push(9);
                fs::write(store.updates_path(&uuid), updates).unwrap();
            }
            damage(&store, &uuid);
            kept(&store, &uuid, &peer);
        }
        let plain = |spoil: bool| {
            let dir = tempfile::tempdir().unwrap();
            let store = ProjectStore::open(dir.path(), "project-1").unwrap();
            let mut task = created();
            store.save(&mut task).unwrap();
            let other = task.snapshot().unwrap();
            task.comment("Only here", &Actor::Person, 11).unwrap();
            store.save(&mut task).unwrap();
            if spoil {
                let mut updates = fs::read(store.updates_path(task.uuid())).unwrap();
                updates.push(9);
                fs::write(store.updates_path(task.uuid()), updates).unwrap();
            }
            damage(&store, task.uuid());
            (dir, store, task.uuid().to_owned(), other)
        };
        // A torn tail may have held the cutover, and so may updates that
        // cannot be read at all.
        let (_dir, store, uuid, other) = plain(true);
        kept(&store, &uuid, &other);
        let (_dir, store, uuid, other) = plain(false);
        fs::remove_file(store.updates_path(&uuid)).unwrap();
        fs::create_dir(store.updates_path(&uuid)).unwrap();
        kept(&store, &uuid, &other);
        // A copy whose every block applies and shows none gives way, its
        // own blocks merged in.
        let (_dir, store, uuid, other) = plain(false);
        assert!(refused(&store, &uuid, other).is_empty());
        assert_eq!(set_aside_files(&store, &uuid).len(), 2);
        let replaced = store.load(&uuid).unwrap();
        assert!(replaced.execution_records().unwrap().0.is_empty());
        assert!(replaced
            .view()
            .unwrap()
            .log
            .iter()
            .any(|e| e.text == "Only here"));
        assert!(!store.was_managed(&uuid));
        // With no blocks at all it gives way as before.
        let (_dir, store, uuid, other) = plain(false);
        fs::remove_file(store.updates_path(&uuid)).unwrap();
        assert!(refused(&store, &uuid, other).is_empty());
    }

    /// review-n on #1415 (R2): a mark that is impossible, unreadable or only
    /// a dangling link keeps the copy; only a verified absence is unmarked.
    #[test]
    fn an_impossible_or_unreadable_mark_keeps_the_copy() {
        type Spoil = Box<dyn Fn(&Path)>;
        let spoils: Vec<Spoil> = vec![
            Box::new(|p| fs::write(p, br#"{"snapshot":{}}"#).unwrap()),
            Box::new(|p| fs::write(p, br#"{"snapshot":{"1":0}}"#).unwrap()),
            Box::new(|p| fs::write(p, br#"{"snapshot":{"1":-1}}"#).unwrap()),
            Box::new(|p| fs::write(p, br#"{"snapshot":{"peer":3}}"#).unwrap()),
            Box::new(|p| {
                fs::remove_file(p).unwrap();
                fs::create_dir(p).unwrap();
            }),
        ];
        for spoil in &spoils {
            let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
            let mut fresh = store.mark(&uuid).unwrap();
            spoil(&store.managed_path(&uuid));
            // An impossible version beside the right block is still refused.
            if let Some(mark) = store.mark(&uuid) {
                fresh.snapshot = mark.snapshot;
                store.write_mark(&uuid, &fresh).unwrap();
            }
            assert!(store.was_managed(&uuid));
            assert!(store.fresh_snapshot(&uuid).is_none());
            damage(&store, &uuid);
            kept(&store, &uuid, &peer);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
            fs::remove_file(store.managed_path(&uuid)).unwrap();
            symlink("absent-mark-target", store.managed_path(&uuid)).unwrap();
            assert!(store.was_managed(&uuid));
            damage(&store, &uuid);
            kept(&store, &uuid, &peer);
            let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
            let path = store.managed_path(&uuid);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            // A reader that ignores permissions cannot show this case.
            if fs::read(&path).is_err() {
                assert!(store.was_managed(&uuid));
                damage(&store, &uuid);
                kept(&store, &uuid, &peer);
            }
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn only_a_person_who_also_removes_the_mark_accepts_another_copy() {
        let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
        damage(&store, &uuid);
        assert_eq!(
            refused(&store, &uuid, created().snapshot().unwrap()).len(),
            1
        );
        // Moving the damaged files away is not enough: the mark keeps it.
        for path in [store.snapshot_path(&uuid), store.updates_path(&uuid)] {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            fs::rename(&path, path.with_file_name(format!("{name}.reviewed"))).unwrap();
        }
        let refusals = refused(&store, &uuid, peer.clone());
        assert_eq!(refusals.len(), 1);
        assert!(
            refusals[0].error.contains("missing"),
            "{}",
            refusals[0].error
        );
        assert!(!store.snapshot_path(&uuid).exists());
        // Removing the mark too is the person's decision to accept it.
        fs::remove_file(store.managed_path(&uuid)).unwrap();
        assert!(refused(&store, &uuid, peer).is_empty());
        let accepted = store.load(&uuid).unwrap();
        assert!(accepted.execution_records().unwrap().0.is_empty());
        assert!(!store.was_managed(&uuid));
    }

    /// The mark is written before the files it describes. A crash between
    /// them leaves a mark ahead of the snapshot, or a first mark naming a
    /// block that never landed: the first only asks more of a recovery, the
    /// second keeps the copy. Neither loses what the copy held.
    #[test]
    fn a_mark_ahead_of_its_files_never_loses_the_copy() {
        // Ahead: the mark names a version the snapshot never reached.
        let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
        let mut ahead = store.load(&uuid).unwrap();
        ahead.comment("Never saved", &Actor::Person, 11).unwrap();
        let first = fs::read(store.updates_path(&uuid)).unwrap();
        store
            .write_mark(&uuid, &Mark::new(&ahead.current_vv(), &first))
            .unwrap();
        damage(&store, &uuid);
        kept(&store, &uuid, &peer);
        // The peer that holds it too rebuilds the copy.
        let mut covering = TaskDoc::from_bytes(&peer).unwrap();
        covering.import(&ahead.snapshot().unwrap()).unwrap();
        assert!(refused(&store, &uuid, covering.snapshot().unwrap()).is_empty());
        assert!(store.load(&uuid).unwrap().held().unwrap().is_some());

        // First mark, then a crash before the block that cut the task over.
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "project-1").unwrap();
        let mut task = created();
        store.save(&mut task).unwrap();
        store
            .write_mark(task.uuid(), &Mark::new(&task.current_vv(), b"never landed"))
            .unwrap();
        damage(&store, task.uuid());
        kept(&store, task.uuid(), &task.snapshot().unwrap());
    }

    #[test]
    fn a_recovery_keeps_the_damaged_files_in_place_until_its_save_replaces_them() {
        let (_dir, store, uuid, peer) = damaged_beside_a_pre_attempt_peer();
        let updates = fs::read(store.updates_path(&uuid)).unwrap();
        damage(&store, &uuid);
        // The copies come first; the files stay until the save replaces them.
        store.set_aside(&uuid).unwrap();
        assert_eq!(
            fs::read(store.snapshot_path(&uuid)).unwrap(),
            b"unreadable snapshot fixture"
        );
        assert_eq!(fs::read(store.updates_path(&uuid)).unwrap(), updates);
        // A crash here leaves the same recovery to run again.
        assert!(refused(&store, &uuid, peer).is_empty());
        only_the_opening_block(&store, &uuid);
        let kept = set_aside_files(&store, &uuid);
        assert!(kept.len() >= 2, "{kept:?}");
        assert!(store.load(&uuid).unwrap().held().unwrap().is_some());
    }
}
