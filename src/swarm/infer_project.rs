//! Optional initial grouping. Never initialize a catalog, replace any explicit
//! membership (including a tombstone), infer by a path prefix, or create Git data.
use super::{
    catalog::{Catalog, Key},
    store::{LockedStore, Store},
};
use crate::config::Config;
use crate::state::Pinned;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::Path,
    time::{Duration, Instant},
};

const INBOX: &str = "ungrouped";

/// Called only after a genuinely new native binding was created, never resume,
/// fork, listing, GUI inspection or adoption. Failures leave normal launch alone.
pub(crate) fn infer_new_session(config: &Config, name: &str, directory: &Path) {
    if !config.dir.join("swarm/catalog.json").is_file() {
        return;
    }
    let end = Instant::now() + Duration::from_secs(3);
    let attempt = (|| -> anyhow::Result<()> {
        let mut store = LockedStore::try_load_scoped(config)?;
        let project = infer(&store.data, name, directory, end, |directory| {
            crate::state::worktree_catalog_until(directory, end)
        });
        if let Some(project) = project {
            if Instant::now() >= end {
                return Ok(());
            }
            let key = Key::Session {
                machine: store.data.catalog.machine(&store.data.node_id),
                session: name.into(),
            };
            let actor = store.data.node_id.clone();
            store.data.catalog.set(&actor, key, json!(project))?;
            let local = [(actor.clone(), name.to_owned())];
            super::session_ids::write(&store.data.catalog, &mut store.data.twins, &actor, &local)?;
            store.save_catalog_only()?;
        }
        Ok(())
    })();
    // Contention, inaccessible folders and optional metadata failures never
    // become an agent start failure, an automatic retry, or a default assignment.
    let _ = attempt;
}

/// A fork's initial grouping, written once by this computer's catalog owner
/// after the fork's binding exists and its launch released the state lock:
/// the project of its source session (or of the archived source), or Inbox
/// when that project no longer exists. Never initializes a catalog or replaces
/// an explicit membership (including a tombstone). Each attempt holds the state
/// lock only to try the provider's binding lock once, confirm that `name` is
/// still run `run_id` with `conversation`, and try the catalog lock once, so
/// neither a busy catalog nor a busy provider lock holds the state lock
/// (zerus-wx3k, zerus-jx5o). Attempts share one 3 s cap; a failure never fails
/// the fork.
pub(crate) fn inherit_fork_project(
    config: &Config,
    name: &str,
    source: &str,
    archive: Option<&str>,
    run_id: &str,
    conversation: &Value,
) {
    if !config.dir.join("swarm/catalog.json").is_file() {
        return;
    }
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let attempt = crate::state::with_pinned_session_binding(name, run_id, conversation, end, || {
            let Ok(mut store) = LockedStore::try_load_scoped(config) else {
                return Ok(false);
            };
            if let Some(project) = inherited(&store.data, name, source, archive) {
                let key = Key::Session {
                    machine: store.data.catalog.machine(&store.data.node_id),
                    session: name.into(),
                };
                let actor = store.data.node_id.clone();
                store.data.catalog.set(&actor, key, json!(project))?;
                let local = [(actor.clone(), name.to_owned())];
                super::session_ids::write(&store.data.catalog, &mut store.data.twins, &actor, &local)?;
                store.save_catalog_only()?;
            }
            anyhow::Ok(true)
        });
        if !retry(&attempt) || Instant::now() >= end {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
/// Whether a fork's project attempt may run again: a busy lock or catalog,
/// with nothing held now. Done, a changed binding or a failure ends it.
fn retry(attempt: &anyhow::Result<Pinned<anyhow::Result<bool>>>) -> bool {
    matches!(attempt, Ok(Pinned::Busy | Pinned::Written(Ok(false))))
}
/// The fork's membership, or `None` to write nothing. A source without a
/// membership (or explicitly unassigned) leaves the fork without one, so both
/// follow the same default project. Inbox, a project that no longer exists and
/// an unresolved concurrent choice all become an explicit Inbox membership.
fn inherited(store: &Store, name: &str, source: &str, archive: Option<&str>) -> Option<String> {
    if !store.imported {
        return None;
    }
    let machine = store.catalog.machine(&store.node_id);
    let key = Key::Session {
        machine: machine.clone(),
        session: name.into(),
    };
    if store.catalog.heads().contains_key(&key) {
        return None;
    }
    let from = Key::Session {
        machine,
        session: match archive.filter(|id| !id.is_empty()) {
            Some(id) => format!("archive\n{id}"),
            None => source.into(),
        },
    };
    let heads = store.catalog.effective_heads();
    let [only] = heads.get(&from)?.as_slice() else {
        return Some(INBOX.into());
    };
    let project = only.value.as_str()?;
    let alive = |id: &str| {
        let key = Key::project(id, "alive");
        heads
            .get(&key)
            .and_then(|versions| Catalog::selected(&key, versions))
            .is_some_and(|op| op.value == true)
    };
    Some(if alive(project) { project } else { INBOX }.into())
}

/// An additive hint for `dirs`: optional read of an existing catalog, never
/// enrollment, initialization or membership mutation. Older peers omit it.
pub(crate) fn directory_project(config: &Config, directory: &Path) -> Option<Value> {
    if !config.dir.join("swarm/catalog.json").is_file() {
        return None;
    }
    let end = Instant::now() + Duration::from_secs(3);
    let store = LockedStore::try_load_scoped(config).ok()?;
    let project = infer_directory(&store.data, directory, end, |path| {
        crate::state::cached_worktree_catalog(path).unwrap_or(Value::Null)
    })?;
    Some(json!({"schema":1,"swarm_id":store.data.catalog.swarm_id,"project_id":project}))
}
fn infer(
    store: &Store,
    name: &str,
    directory: &Path,
    end: Instant,
    catalog: impl FnOnce(&Path) -> Value,
) -> Option<String> {
    let key = Key::Session {
        machine: store.catalog.machine(&store.node_id),
        session: name.into(),
    };
    if store.catalog.heads().contains_key(&key) {
        return None;
    }
    infer_directory(store, directory, end, catalog)
}
fn infer_directory(
    store: &Store,
    directory: &Path,
    end: Instant,
    catalog: impl FnOnce(&Path) -> Value,
) -> Option<String> {
    if !store.imported || Instant::now() >= end {
        return None;
    }
    let local = store.catalog.machine(&store.node_id);
    let directory = crate::platform::canonical(directory)
        .ok()
        .filter(|path| path.is_dir())?;
    let values = store.catalog.values();
    let mut anchors = Vec::new();
    for (key, value) in &values {
        if !matches!(key, Key::Folder { .. })
            || value.is_null()
            || value["machine"]
                .as_str()
                .is_none_or(|id| store.catalog.machine(id) != local)
        {
            continue;
        }
        let project = value["project"].as_str()?;
        if project == "ungrouped"
            || values.get(&Key::project(project, "alive")) != Some(&json!(true))
        {
            continue;
        }
        // A bounded complete candidate set; never decide from a truncated scan.
        if anchors.len() >= 512 || Instant::now() >= end {
            return None;
        }
        let path = crate::platform::canonical(value["path"].as_str()?)
            .ok()
            .filter(|path| path.is_dir())?;
        anchors.push((project.to_owned(), path));
    }
    if anchors.is_empty() || Instant::now() >= end {
        return None;
    }
    let mut projects: BTreeSet<String> = anchors
        .iter()
        .filter(|(_, path)| *path == directory)
        .map(|(id, _)| id.clone())
        .collect();
    let evidence = catalog(&directory);
    if Instant::now() >= end {
        return None;
    }
    if evidence["state"] == "ok"
        && evidence["stale"] == false
        && evidence["partial"] == false
        && evidence["path"].as_str() == directory.to_str()
        && evidence["common_dir"]
            .as_str()
            .is_some_and(|path| Path::new(path).is_absolute())
    {
        let entries = evidence["worktrees"].as_array()?;
        let contains = |path: &Path| {
            entries.iter().any(|tree| {
                tree["available"] == true
                    && matches!(tree["kind"].as_str(), Some("main" | "linked"))
                    && tree["path"].as_str() == path.to_str()
            })
        };
        if contains(&directory) {
            for (id, path) in anchors {
                if contains(&path) {
                    projects.insert(id);
                }
            }
        }
    }
    // Exact canonical folder membership stands independently of Git. Missing
    // or stale repository evidence never establishes a related worktree.
    if projects.len() == 1 {
        projects.pop_first()
    } else {
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::swarm::catalog::{new_id, Catalog};
    use std::collections::BTreeMap;
    /// zerus-jx5o: a busy provider lock, state lock or catalog retries
    /// within the cap; a write, a changed binding or a failure ends it.
    #[test]
    fn a_busy_attempt_retries_and_every_other_outcome_ends() {
        assert!(retry(&Ok(Pinned::Busy)));
        assert!(retry(&Ok(Pinned::Written(Ok(false)))));
        assert!(!retry(&Ok(Pinned::Written(Ok(true)))));
        assert!(!retry(&Ok(Pinned::Changed)));
        assert!(!retry(&Ok(Pinned::Written(Err(anyhow::anyhow!("save failed"))))));
        assert!(!retry(&Err(anyhow::anyhow!("lock failed"))));
    }
    fn store() -> Store {
        Store {
            node_id: new_id(),
            catalog: Catalog::default(),
            imported: true,
            bindings: BTreeMap::new(),
            peers: BTreeMap::new(),
            twins: Default::default(),
            id_memberships: None,
        }
    }
    fn folder(store: &mut Store, id: &str, project: &str, machine: &str, path: &Path) {
        store
            .catalog
            .set(&store.node_id, Key::project(project, "alive"), json!(true))
            .unwrap();
        store
            .catalog
            .set(
                &store.node_id,
                Key::Folder { id: id.into() },
                json!({"project":project,"machine":machine,"path":path,"name":""}),
            )
            .unwrap();
    }
    fn choose(store: &Store, path: &Path, evidence: Value) -> Option<String> {
        infer(
            store,
            "codex/example/new",
            path,
            Instant::now() + Duration::from_secs(3),
            |_| evidence,
        )
    }
    /// zerus-zjsj: a fork takes its source's live project, else an explicit
    /// Inbox; an unplaced source leaves it unplaced. Nothing fails the fork.
    #[test]
    fn a_fork_inherits_its_sources_project_or_inbox() {
        let mut store = store();
        let local = store.node_id.clone();
        let machine = store.catalog.machine(&local);
        let member = |session: &str| Key::Session {
            machine: machine.clone(),
            session: session.into(),
        };
        let set = |store: &mut Store, key: Key, value: Value| {
            store.catalog.set(&local, key, value).unwrap();
        };
        for project in ["work", "other"] {
            set(&mut store, Key::project(project, "alive"), json!(true));
        }
        set(&mut store, member("codex/p/source"), json!("work"));
        set(&mut store, member("archive\narchive-1"), json!("work"));
        set(&mut store, member("codex/p/inbox"), json!("ungrouped"));
        set(&mut store, member("codex/p/lost"), json!("never-created"));
        set(&mut store, member("codex/p/cleared"), Value::Null);
        // Two devices chose different projects; the desktop asks for a review.
        let mut remote = store.catalog.clone();
        remote
            .set(&new_id(), member("codex/p/split"), json!("other"))
            .unwrap();
        set(&mut store, member("codex/p/split"), json!("work"));
        store
            .catalog
            .merge(
                &remote.swarm_id.clone(),
                remote.operations.into_values().collect(),
            )
            .unwrap();
        let from = |store: &Store, source: &str, archive: Option<&str>| {
            inherited(store, "codex/p/fork", source, archive)
        };
        let work = Some("work".to_owned());
        let inbox = Some("ungrouped".to_owned());
        assert_eq!(from(&store, "codex/p/source", None), work);
        assert_eq!(from(&store, "codex/p/gone", Some("archive-1")), work);
        for source in ["codex/p/inbox", "codex/p/lost", "codex/p/split"] {
            assert_eq!(from(&store, source, None), inbox, "{source}");
        }
        // No membership or an explicit unassignment: the same default project.
        for source in ["codex/p/unplaced", "codex/p/cleared"] {
            assert_eq!(from(&store, source, None), None, "{source}");
        }
        assert_eq!(from(&store, "codex/p/gone", Some("archive-2")), None);
        // An explicit membership of the fork, even Inbox or a tombstone, stays.
        for value in [json!("ungrouped"), Value::Null, json!("other")] {
            let mut explicit = store.clone();
            set(&mut explicit, member("codex/p/fork"), value);
            assert_eq!(from(&explicit, "codex/p/source", None), None);
        }
        // A project deleted since: Inbox.
        let mut retired = store.clone();
        set(&mut retired, Key::project("work", "alive"), json!(false));
        assert_eq!(from(&retired, "codex/p/source", None), inbox);
        // An uninitialized catalog is never written.
        store.imported = false;
        assert_eq!(from(&store, "codex/p/source", None), None);
    }

    #[cfg(unix)]
    #[test]
    fn canonical_exact_non_git_folder_is_initial_metadata_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        let mut store = store();
        let local = store.node_id.clone();
        folder(&mut store, "folder", "work", &local, &path);
        assert_eq!(
            choose(&store, &path, json!({"state":"not_repo"})),
            Some("work".into())
        );
        assert!(!path.join(".git").exists());
        let link = path.join("alias");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(
            choose(&store, &link, json!({"state":"not_repo"})),
            Some("work".into())
        );
        let nested = path.join("nested");
        std::fs::create_dir(&nested).unwrap();
        assert_eq!(choose(&store, &nested, json!({"state":"not_repo"})), None); // no prefix guess
        for value in [Value::Null, json!("ungrouped"), json!("manual")] {
            let mut explicit = store.clone();
            explicit
                .catalog
                .set(
                    &local,
                    Key::Session {
                        machine: local.clone(),
                        session: "codex/example/new".into(),
                    },
                    value,
                )
                .unwrap();
            assert_eq!(choose(&explicit, &path, json!({"state":"not_repo"})), None);
        }
    }
    #[cfg(unix)]
    #[test]
    fn exact_catalog_worktrees_require_complete_same_machine_unambiguous_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let anchor = root.join("main");
        let linked = root.join("linked");
        std::fs::create_dir(&anchor).unwrap();
        std::fs::create_dir(&linked).unwrap();
        let evidence = json!({"state":"ok","stale":false,"partial":false,"path":linked,"common_dir":root.join("common"),
            "worktrees":[{"path":anchor,"kind":"main","available":true},{"path":linked,"kind":"linked","available":true}]});
        let mut store = store();
        let local = store.node_id.clone();
        folder(&mut store, "anchor", "work", &local, &anchor);
        assert_eq!(
            choose(&store, &linked, evidence.clone()),
            Some("work".into())
        );
        for field in ["stale", "partial"] {
            let mut changed = evidence.clone();
            changed[field] = json!(true);
            assert_eq!(choose(&store, &linked, changed), None);
        }
        for changed in [
            json!({"state":"timeout"}),
            json!({"state":"ok","stale":false,"partial":false,"path":linked,"common_dir":"relative","worktrees":[]}),
        ] {
            assert_eq!(choose(&store, &linked, changed), None);
        }
        for field in ["available", "kind"] {
            let mut changed = evidence.clone();
            changed["worktrees"][0][field] = if field == "available" {
                json!(false)
            } else {
                json!("bare")
            };
            assert_eq!(choose(&store, &linked, changed), None);
        }
        let mut remote = store.clone();
        let other = new_id();
        folder(&mut remote, "anchor", "work", &other, &anchor);
        assert_eq!(choose(&remote, &linked, evidence.clone()), None);
        folder(&mut store, "second", "other", &local, &linked);
        assert_eq!(choose(&store, &linked, evidence), None);
    }
    #[cfg(unix)]
    #[test]
    fn expired_or_uninitialized_inference_does_not_consult_git() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store();
        let never = |_: &Path| -> Value {
            panic!("Git must not run");
        };
        assert_eq!(
            infer(&store, "one", dir.path(), Instant::now(), never),
            None
        );
        store.imported = false;
        assert_eq!(
            infer(
                &store,
                "one",
                dir.path(),
                Instant::now() + Duration::from_secs(3),
                never
            ),
            None
        );
    }
}
