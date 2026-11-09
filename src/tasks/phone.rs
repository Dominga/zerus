//! What a phone exchanges with this computer through the relay: `hgs task
//! digests --json` for the heartbeat and `hgs task exchange --json` for one
//! page (docs/superpowers/specs/2026-10-18-task-replication-loro-design.md, "Phone").
use super::index::{Failure, Index};
use super::{fail, Result};
use crate::config::Config;
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;
use zerus_replica::phone::{answer_with, Inventories, Request};
use zerus_replica::{Inventory, ProjectStore};

/// The most a request may be: the relay's canonical 1 MiB.
const MAX_REQUEST: u64 = 1 << 20;
/// How long an exchange waits for a project another command holds.
const PROJECT_WAIT: Duration = Duration::from_secs(2);
/// How long the digests wait for each busy project: one held project is left
/// out this time, and the others keep theirs within the connector's budget.
const DIGEST_WAIT: Duration = Duration::from_millis(250);

fn root(config: &Config) -> PathBuf {
    config.state.join("replica")
}

/// The projects this computer answers for: alive in the catalog or stored here.
fn known(config: &Config) -> Result<(Vec<String>, Vec<String>)> {
    let alive = crate::swarm::projects(config)
        .map_err(fail)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let stored = ProjectStore::stored(&root(config)).map_err(fail)?;
    Ok((alive, stored))
}

/// `hgs task digests --json`: each project's phone-view digest. A project
/// not stored here has the empty view; one that cannot be read now is left
/// out and named on stderr.
pub(super) fn digests(config: &Config) -> Result<Value> {
    let (alive, stored) = known(config)?;
    let mut projects = serde_json::Map::new();
    for id in alive.iter().filter(|id| !stored.contains(id)) {
        projects.insert(id.clone(), json!({"digest": Inventory::default().digest()}));
    }
    for id in stored {
        match ProjectStore::open_within(&super::replica_root(config).map_err(fail)?, &id, DIGEST_WAIT) {
            Ok(Some(store)) => match Indexed::new(&store).live_digest() {
                Ok(digest) => {
                    projects.insert(id, json!({"digest": digest}));
                }
                Err(error) => eprintln!("hgs: the tasks of {id:?} cannot be read: {error}"),
            },
            Ok(None) => eprintln!("hgs: the tasks of {id:?} are busy; they are left out this time"),
            Err(error) => eprintln!("hgs: the tasks of {id:?} cannot be read: {error}"),
        }
    }
    Ok(json!({"schema": 1, "projects": projects}))
}

/// `hgs task exchange --json`: imports the phone's updates and answers with a
/// page of what it lacks (`zerus_replica::phone::answer`).
pub(super) fn exchange(config: &Config) -> Result<Value> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_REQUEST + 1)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() as u64 > MAX_REQUEST {
        return Err(fail("the exchange request exceeds 1 MiB"));
    }
    // WhiteWh fork (H2): PowerShell 5.1 pipes JSON with a UTF-8 BOM.
    let request: Request = serde_json::from_slice(crate::state::without_bom(&bytes))
        .map_err(|e| fail(format!("invalid exchange request: {e}")))?;
    request.check().map_err(fail)?;
    let (alive, stored) = known(config)?;
    if !alive.contains(&request.project) && !stored.contains(&request.project) {
        return Err(fail(format!("no project {:?} here", request.project)));
    }
    let store = ProjectStore::open_within(&super::replica_root(config).map_err(fail)?, &request.project, PROJECT_WAIT)
        .map_err(fail)?
        .ok_or_else(|| fail("the project's tasks are busy; try again"))?;
    let answered = answer_with(&store, request, &mut Indexed::new(&store)).map_err(fail)?;
    serde_json::to_value(answered).map_err(fail)
}

/// The phone's inventories from the project's task index (zerus-cg5h.21),
/// so a request reads only what changed instead of stamping every task file
/// and rewriting the store's inventory cache; whenever the index cannot
/// answer, from the store as before. The index opens on first use, after
/// the request's updates were imported.
struct Indexed<'a> {
    store: &'a ProjectStore,
    index: Option<Index<'a>>,
    tried: bool,
}

impl<'a> Indexed<'a> {
    fn new(store: &'a ProjectStore) -> Self {
        Self { store, index: None, tried: false }
    }

    fn index(&mut self) -> Option<&mut Index<'a>> {
        if !self.tried {
            self.tried = true;
            self.index = Index::open(self.store);
        }
        self.index.as_mut()
    }

    /// Closes an index that could not answer; the store answers from now on.
    fn give_up(&mut self, failure: Failure) {
        if let Some(index) = self.index.take() {
            index.discard(&failure);
        }
    }
}

impl Inventories for Indexed<'_> {
    fn phone(&mut self, theirs: &Inventory) -> zerus_replica::Result<Inventory> {
        if let Some(index) = self.index() {
            match index.phone_inventory(theirs) {
                Ok(inventory) => return Ok(inventory),
                Err(failure) => self.give_up(failure),
            }
        }
        self.store.phone_inventory(theirs)
    }

    fn live_digest(&mut self) -> zerus_replica::Result<String> {
        if let Some(index) = self.index() {
            match index.live_inventory() {
                Ok(inventory) => return Ok(inventory.digest()),
                Err(failure) => self.give_up(failure),
            }
        }
        Ok(self.store.live_inventory()?.0.digest())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_replica::{Actor, NewTask, Stage, Target, TaskDoc};

    /// review-win-e on #1736: a task an older replica's merged upgrade left
    /// below the execution schema is repaired before the indexed digest and
    /// answer read it, and a phone holding the stale version receives the
    /// repair, as from the store's own inventories.
    #[test]
    fn indexed_digests_and_answers_repair_a_task_first() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project-1").unwrap();
        let mut stale = TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-repair",
            text: "Repair me",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        assert!(stale.floor().unwrap());
        stale.lower_stored_schema_for_test(5);
        store.save(&mut stale).unwrap();
        let old = stale.inventory_version();
        let digest = Indexed::new(&store).live_digest().unwrap();
        assert!(!store.load(stale.uuid()).unwrap().needs_schema_repair());
        assert_eq!(digest, store.live_inventory().unwrap().0.digest());
        let request: Request = serde_json::from_value(json!({
            "schema": 1, "project": "project-1",
            "inventory": {"project": {}, "tasks": {stale.uuid(): old}},
        }))
        .unwrap();
        let answered = answer_with(&store, request, &mut Indexed::new(&store)).unwrap();
        assert!(answered.updates.iter().any(|u| u.task.as_deref() == Some(stale.uuid())));
        assert_eq!(answered.digest, digest);
    }
}
