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
        let old: Cache = fs::read(&path)
            .ok()
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
            let _ = write_atomic(&path, &serde_json::to_vec(&cache)?);
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
                // from another replica; its files are set aside, not removed.
                Err(damaged) => (TaskDoc::from_bytes(bytes).map_err(|_| damaged)?, true),
            }
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

    /// Renames a task's files that do not load to `<name>.damaged-<ms>`, kept on disk.
    fn set_aside(&self, uuid: &str) -> Result<()> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        for path in [self.snapshot_path(uuid), self.updates_path(uuid)] {
            if path.exists() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                fs::rename(
                    &path,
                    path.with_file_name(format!("{name}.damaged-{stamp}")),
                )?;
            }
        }
        self.note_change(uuid);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! The live-store repair of the execution schema (zerus-cg5h.4 S2b.2):
    //! a lower value an older replica's concurrent upgrade left in a merge is
    //! raised again under the project lock before the task is saved, read for
    //! an inventory or exported.
    use super::*;
    use crate::execution::{NativeRequest, Origin, EXECUTION_TASK_SCHEMA};
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
}
