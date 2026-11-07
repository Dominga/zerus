//! Session membership by durable ID (zerus-ucck P2c-1).
//!
//! Every writer of a session's membership by label ([`Key::Session`]) on the
//! session's own computer also writes its ID twin: the session ID that label
//! names, and the exact name operation the twin mirrors. Twins live beside the
//! catalog in [`FILE`], never in it, so hgs v0.47.0 on this computer, and any
//! member that does not list the `session_id` kind, never parses one (#1413
//! review). Only members that list it exchange twins.
//!
//! A twin is read only while the name operation it mirrors is still the
//! selected membership of its label. A later move by any writer, including a
//! member that never sees twins and so keeps Lamport clocks without them,
//! hides it, and a desktop falls back to the name form. Desktops read the ID
//! form only once every machine of the swarm lists the kind
//! (`membership_by_id`).
use super::catalog::{Catalog, Key, Operation};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

/// The twins' file, beside `catalog.json`.
pub const FILE: &str = "session-id-twins.json";
/// A twins file larger than this is ignored, as an unreadable one is.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// One session's membership by ID: the newest write per machine and ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Twin {
    /// This write's own ID.
    pub id: String,
    pub actor: String,
    /// Orders writes of the same session ID; never compared with the
    /// catalog's clocks. It counts from the twin this computer last had for
    /// the ID, so after a lost or unreadable twins file it starts at 1 again.
    /// A peer's older twin with a higher clock then wins until its owner
    /// writes again, and is read only while its own name operation is still
    /// selected, so the loss hides an ID membership but never shows a stale
    /// one.
    pub clock: u64,
    pub machine: String,
    pub session_id: String,
    /// The label whose [`Key::Session`] membership this twin mirrors.
    pub label: String,
    /// The ID of that name operation.
    pub op: String,
    /// Its project; None when it unassigned the label.
    pub project: Option<String>,
}

fn uuid(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok_and(|id| id.to_string() == text)
}

impl Twin {
    fn valid(&self) -> bool {
        uuid(&self.id)
            && uuid(&self.actor)
            && self.clock > 0
            && self.clock < u64::MAX
            && uuid(&self.machine)
            && uuid::Uuid::parse_str(&self.session_id)
                .is_ok_and(|id| !id.is_nil() && id.to_string() == self.session_id)
            && !self.label.is_empty()
            && self.label.len() <= 2048
            && !self.label.contains('\0')
            && uuid(&self.op)
            && self
                .project
                .as_ref()
                .is_none_or(|p| !p.is_empty() && p.len() <= 128 && !p.chars().any(char::is_control))
    }

    fn order(&self) -> (u64, &str, &str) {
        (self.clock, self.actor.as_str(), self.id.as_str())
    }
}

/// The twins of one swarm, a last-writer-wins register per session ID.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Twins {
    pub swarm_id: String,
    /// `machine\nsession ID` -> its newest twin.
    pub entries: BTreeMap<String, Twin>,
}

fn entry_key(machine: &str, session_id: &str) -> String {
    format!("{machine}\n{session_id}")
}

impl Twins {
    pub fn new(swarm_id: &str) -> Self {
        Self {
            swarm_id: swarm_id.to_owned(),
            entries: BTreeMap::new(),
        }
    }

    /// The twins of `swarm_id` in `dir`. A missing, unreadable, foreign or
    /// invalid file gives none: twins are derived, and the next membership
    /// write makes them again, so they never stop the catalog.
    pub fn load(dir: &Path, swarm_id: &str) -> (Self, Vec<u8>) {
        let mut bytes = Vec::new();
        let read = std::fs::File::open(dir.join(FILE))
            .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes));
        let parsed = read
            .ok()
            .filter(|_| bytes.len() as u64 <= MAX_BYTES)
            .and_then(|_| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|twins| {
                twins.swarm_id == swarm_id
                    && twins.entries.iter().all(|(key, twin)| {
                        twin.valid() && *key == entry_key(&twin.machine, &twin.session_id)
                    })
            });
        (parsed.unwrap_or_else(|| Self::new(swarm_id)), bytes)
    }

    /// The file's bytes: none while there are no twins.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        Ok(if self.entries.is_empty() {
            Vec::new()
        } else {
            serde_json::to_vec(self)?
        })
    }

    /// Takes each newer valid twin of the same swarm; invalid ones are
    /// skipped, as derived data.
    pub fn merge(&mut self, swarm_id: &str, twins: Vec<Twin>) -> Result<()> {
        ensure!(
            swarm_id == self.swarm_id,
            "session ID memberships belong to another swarm"
        );
        for twin in twins.into_iter().filter(Twin::valid) {
            let key = entry_key(&twin.machine, &twin.session_id);
            if self
                .entries
                .get(&key)
                .is_none_or(|old| old.order() < twin.order())
            {
                self.entries.insert(key, twin);
            }
        }
        Ok(())
    }

    pub fn known(&self) -> BTreeSet<String> {
        self.entries.values().map(|twin| twin.id.clone()).collect()
    }

    /// The twins a member that knows `known` lacks.
    pub fn missing(&self, known: &BTreeSet<String>) -> Vec<Twin> {
        self.entries
            .values()
            .filter(|twin| !known.contains(&twin.id))
            .cloned()
            .collect()
    }

    /// Drops the twins whose name operation a later one superseded (#1413
    /// review): such a twin is never read again, as a superseded operation is
    /// never selected again. A twin of an operation not known here yet stays.
    /// Returns whether it dropped any.
    pub fn prune(&mut self, catalog: &Catalog) -> bool {
        let superseded: BTreeSet<&str> = catalog
            .operations
            .values()
            .flat_map(|op| op.parents.iter().map(String::as_str))
            .collect();
        let before = self.entries.len();
        self.entries
            .retain(|_, twin| !superseded.contains(twin.op.as_str()));
        self.entries.len() != before
    }

    pub fn digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.swarm_id.as_bytes());
        for twin in self.entries.values() {
            hash.update(twin.id.as_bytes());
        }
        format!("{:x}", hash.finalize())
    }
}

/// The durable session ID of this computer's live or saved session `label`.
#[cfg(not(test))]
fn session_id_of(label: &str) -> Option<String> {
    crate::state::session_id_of(label)
}

#[cfg(test)]
thread_local! {
    /// Labels and IDs the tests' sessions have; never the real state.
    pub(super) static SESSIONS: std::cell::RefCell<BTreeMap<String, String>> =
        std::cell::RefCell::default();
}

#[cfg(test)]
fn session_id_of(label: &str) -> Option<String> {
    SESSIONS.with(|sessions| sessions.borrow().get(label).cloned())
}

/// The selected name operation per label, as the name form reads it.
fn selected_names(catalog: &Catalog) -> BTreeMap<Key, Operation> {
    catalog
        .effective_heads()
        .into_iter()
        .filter(|(key, _)| matches!(key, Key::Session { .. }))
        .filter_map(|(key, heads)| Catalog::selected(&key, &heads).map(|op| (key, op.clone())))
        .collect()
}

/// After name writes of `labels` (`(machine, label)`), writes the twin of
/// each that is this computer's session with a durable ID: bound to the name
/// operation now selected for its label. An unchanged twin is not written
/// again. Call it once after a command's name writes.
pub(super) fn write(
    catalog: &Catalog,
    twins: &mut Twins,
    actor: &str,
    labels: &[(String, String)],
) -> Result<()> {
    let local = catalog.machine(actor);
    let mut named = None;
    for (machine, label) in labels {
        let machine = catalog.machine(machine);
        if machine != local {
            continue;
        }
        let Some(session_id) = session_id_of(label) else {
            continue;
        };
        let named = named.get_or_insert_with(|| selected_names(catalog));
        let Some(op) = named.get(&Key::Session {
            machine: machine.clone(),
            session: label.clone(),
        }) else {
            continue;
        };
        let key = entry_key(&machine, &session_id);
        let old = twins.entries.get(&key);
        let project = op.value.as_str().map(str::to_owned);
        if old.is_some_and(|old| old.op == op.id && old.label == *label && old.project == project) {
            continue;
        }
        let twin = Twin {
            id: super::catalog::new_id(),
            actor: actor.to_owned(),
            clock: old.map_or(0, |old| old.clock) + 1,
            machine,
            session_id,
            label: label.clone(),
            op: op.id.clone(),
            project,
        };
        ensure!(twin.valid(), "invalid session ID membership");
        twins.entries.insert(key, twin);
    }
    Ok(())
}

/// The ID memberships a desktop may read: `(machine, session ID, project)`
/// for each twin whose name operation is still the selected membership of
/// its label, with the same project.
pub(super) fn readable(catalog: &Catalog, twins: &Twins) -> Vec<(String, String, String)> {
    let identities = catalog.identities().unwrap_or_default();
    current_twins(catalog, twins)
        .into_iter()
        .filter_map(|twin| {
            Some((
                identities
                    .get(&twin.machine)
                    .unwrap_or(&twin.machine)
                    .clone(),
                twin.session_id.clone(),
                twin.project.clone()?,
            ))
        })
        .collect()
}

/// Includes a valid explicit unassignment; migration must preserve null as
/// primary data too. The derived format and its existing clock rule stay the
/// same. This does not select one raw concurrent name head for the new graph.
pub(super) fn current_twins<'a>(catalog: &Catalog, twins: &'a Twins) -> Vec<&'a Twin> {
    if twins.swarm_id != catalog.swarm_id {
        return Vec::new();
    }
    let Ok(identities) = catalog.identities() else {
        return Vec::new();
    };
    let named = selected_names(catalog);
    twins
        .entries
        .values()
        .filter_map(|twin| {
            let machine = identities
                .get(&twin.machine)
                .unwrap_or(&twin.machine)
                .clone();
            let op = named.get(&Key::Session {
                machine: machine.clone(),
                session: twin.label.clone(),
            })?;
            (twin.valid() && op.id == twin.op && op.value.as_str() == twin.project.as_deref())
                .then_some(twin)
        })
        .collect()
}

/// A twins file on disk, for a reader without the lock.
pub(super) fn read(dir: &Path, swarm_id: &str) -> Twins {
    Twins::load(dir, swarm_id).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::swarm::catalog::{
        advertised, exported, kinds, new_id, FLOOR_KINDS, SESSION_ID_KIND,
    };
    use crate::swarm::model::Patch;
    use crate::swarm::store::{Binding, Store};
    use serde_json::{json, Value};

    const ID: &str = "3f2a9c1d-0b4e-4d6a-9c1d-2e3f4a5b6c7d";
    const LABEL: &str = "codex/work/one";

    fn store_as(node_id: String) -> Store {
        let catalog=Catalog::default();
        let primary=super::super::session_memberships::Memberships::empty(&catalog);
        Store {
            node_id: node_id.clone(),
            catalog,
            imported: false,
            bindings: BTreeMap::from([(
                "local".into(),
                Binding {
                    id: node_id,
                    verified: true,
                },
            )]),
            peers: BTreeMap::new(),
            twins: Twins::default(),
            id_memberships: Some(primary),
        }
    }

    fn config() -> Config {
        Config {
            home: "/home/example".into(),
            dir: "/home/example/.config/hgs".into(),
            state: "/home/example/.local/state/hgs".into(),
            selves: vec!["local".into()],
            peers: vec!["peer".into()],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        }
    }

    /// Two projects, `work` and `play`, with the members each lists.
    fn organization(work: &[String], play: &[String]) -> Value {
        json!({"projects": [
            {"id": "work", "name": "Work", "color": "#123456", "folders": [], "sessions": work},
            {"id": "play", "name": "Play", "color": "#654321", "folders": [], "sessions": play},
        ]})
    }

    fn known(label: &str, id: &str) {
        SESSIONS.with(|sessions| sessions.borrow_mut().insert(label.into(), id.into()));
    }

    /// The snapshot's members by ID, per project.
    fn by_id(store: &Store) -> BTreeMap<String, Vec<String>> {
        // These pre-primary controls pin the DERIVED twin semantics. Primary
        // precedence, conflicts and explicit null are tested separately.
        let mut legacy = store.clone();
        legacy.id_memberships = Some(super::super::session_memberships::Memberships::empty(&store.catalog));
        let snapshot = legacy.snapshot(&config());
        snapshot["organization"]["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|project| {
                let ids = project["session_ids"].as_array().into_iter().flatten();
                let ids = ids.map(|member| {
                    member
                        .as_str()
                        .unwrap()
                        .split_once('\n')
                        .unwrap()
                        .1
                        .to_owned()
                });
                (project["id"].as_str().unwrap().to_owned(), ids.collect())
            })
            .collect()
    }

    /// The project the name form lists `LABEL` in, if any.
    fn named(store: &Store) -> Option<String> {
        let snapshot = store.snapshot(&config());
        snapshot["organization"]["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|p| {
                p["sessions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s.as_str().unwrap().ends_with(&format!("\n{LABEL}")))
                    .then(|| p["id"].as_str().unwrap().to_owned())
            })
    }

    /// Where a desktop reading by ID finds the session.
    fn by_id_project(store: &Store) -> Option<String> {
        let found: Vec<_> = by_id(store)
            .into_iter()
            .filter(|(_, ids)| ids.iter().any(|id| id == ID))
            .collect();
        assert!(found.len() <= 1, "{found:?}");
        found.into_iter().next().map(|(project, _)| project)
    }

    /// A desktop's patch from `base` to `desired`.
    fn moved(store: &mut Store, base: Value, desired: Value) {
        let versions = store.catalog.versions();
        store
            .apply(Patch {
                base,
                desired,
                versions,
            })
            .unwrap();
    }

    /// The catalog operations a member without the kind receives: all of
    /// them, and never a twin, which is not one (what transport sends).
    fn catalog_sync(to: &mut Store, from: &Store) {
        to.catalog
            .merge(
                &from.catalog.swarm_id,
                from.catalog.operations.values().cloned().collect(),
            )
            .unwrap();
    }

    /// A member that lists the kind also receives the twins.
    fn full_sync(to: &mut Store, from: &Store) {
        catalog_sync(to, from);
        if to.twins.swarm_id.is_empty() {
            to.twins = Twins::new(&to.catalog.swarm_id);
        }
        to.twins
            .merge(
                &from.catalog.swarm_id,
                from.twins.entries.values().cloned().collect(),
            )
            .unwrap();
    }

    fn initialized(node: String) -> Store {
        let mut s = store_as(node);
        s.initialize(&organization(&[], &[])).unwrap();
        s.twins = Twins::new(&s.catalog.swarm_id);
        s
    }

    /// A second member of `owner`'s swarm, holding its catalog; `twins`
    /// whether it takes twins.
    fn member(owner: &Store, node: String, twins: bool) -> Store {
        let mut other = owner.clone();
        other.node_id = node.clone();
        other.bindings = BTreeMap::from([
            (
                "local".into(),
                Binding {
                    id: node,
                    verified: true,
                },
            ),
            (
                "peer".into(),
                Binding {
                    id: owner.node_id.clone(),
                    verified: true,
                },
            ),
        ]);
        other.twins = if twins {
            owner.twins.clone()
        } else {
            Twins::new(&owner.catalog.swarm_id)
        };
        other
    }

    /// A desktop's move of this computer's session also moves it by ID,
    /// beside the catalog: `versions` and the catalog never carry a twin.
    #[test]
    fn a_local_move_writes_a_twin_bound_to_its_name_operation() {
        known(LABEL, ID);
        let mut a = initialized(new_id());
        let me = format!("local\n{LABEL}");
        let previous: BTreeSet<String> = a.catalog.operations.keys().cloned().collect();
        moved(
            &mut a,
            organization(&[], &[]),
            organization(std::slice::from_ref(&me), &[]),
        );
        assert_eq!(by_id_project(&a).as_deref(), Some("work"));
        // #1413 review, finding 3: what `swarm apply` reports as written
        // (`mod.rs`) is in the versions a desktop reads, so its sync guard
        // adopts the next state at once.
        let versions = a.snapshot(&config())["versions"].clone();
        for op in a
            .catalog
            .operations
            .values()
            .filter(|op| !previous.contains(&op.id))
        {
            let token = a.catalog.canonical_key(&op.key).token();
            assert!(
                versions[&token].as_array().unwrap().contains(&json!(op.id)),
                "{token}"
            );
        }
        let twin = a.twins.entries.values().next().unwrap().clone();
        let name = a.catalog.canonical_key(&Key::Session {
            machine: a.node_id.clone(),
            session: LABEL.into(),
        });
        let heads = a.catalog.effective_heads();
        assert_eq!(twin.op, Catalog::selected(&name, &heads[&name]).unwrap().id);
        assert_eq!(
            (
                twin.project.as_deref(),
                twin.label.as_str(),
                twin.session_id.as_str()
            ),
            (Some("work"), LABEL, ID)
        );
        moved(
            &mut a,
            organization(std::slice::from_ref(&me), &[]),
            organization(&[], std::slice::from_ref(&me)),
        );
        assert_eq!(by_id_project(&a).as_deref(), Some("play"));
        assert_eq!(a.twins.entries.len(), 1, "one register per session ID");
        assert_eq!(
            a.twins.entries.values().next().unwrap().clock,
            twin.clock + 1
        );
        assert!(serde_json::to_string(&a.catalog)
            .unwrap()
            .find("session_id")
            .is_none());
        // Unassigning by name unassigns by ID.
        moved(
            &mut a,
            organization(&[], std::slice::from_ref(&me)),
            organization(&[], &[]),
        );
        assert_eq!(by_id_project(&a), None);
        assert_eq!(a.twins.entries.values().next().unwrap().project, None);
        // Writing again changes nothing while the name operation stands.
        let before = a.twins.clone();
        write(
            &a.catalog,
            &mut a.twins,
            &a.node_id.clone(),
            &[(a.node_id.clone(), LABEL.into())],
        )
        .unwrap();
        assert_eq!(a.twins, before);
    }

    /// #1413 review, finding 1: a member that never receives twins keeps
    /// Lamport clocks without them. Whether its name-only move supersedes the
    /// owner's or is concurrent with it, and whichever actor sorts first, a
    /// desktop reading by ID never shows a project the name form does not.
    #[test]
    fn a_name_move_by_a_member_without_twins_is_never_contradicted() {
        known(LABEL, ID);
        let low = "00000000-0000-4000-8000-000000000000".to_owned();
        let high = "ffffffff-ffff-4fff-bfff-ffffffffffff".to_owned();
        for (owner_node, legacy_node) in [(high.clone(), low.clone()), (low.clone(), high.clone())]
        {
            for concurrent in [false, true] {
                let case = format!("owner {owner_node}, concurrent {concurrent}");
                let mut owner = initialized(owner_node.clone());
                let me = format!("local\n{LABEL}");
                // The legacy member's catalog before the owner's move, for the
                // concurrent case; after it, for the superseding one.
                let mut legacy = member(&owner, legacy_node.clone(), false);
                moved(
                    &mut owner,
                    organization(&[], &[]),
                    organization(std::slice::from_ref(&me), &[]),
                );
                if !concurrent {
                    catalog_sync(&mut legacy, &owner);
                }
                // v0.47.0 moves it by name, with Catalog::set.
                let name = Key::Session {
                    machine: owner_node.clone(),
                    session: LABEL.into(),
                };
                legacy
                    .catalog
                    .set(&legacy_node, name, json!("play"))
                    .unwrap();
                catalog_sync(&mut owner, &legacy);
                let shown = named(&owner);
                match by_id_project(&owner) {
                    Some(project) => assert_eq!(Some(project), shown, "{case}"),
                    None => assert!(!concurrent || shown.is_some(), "{case}"),
                }
                if !concurrent {
                    assert_eq!(
                        (shown.as_deref(), by_id_project(&owner)),
                        (Some("play"), None),
                        "{case}"
                    );
                }
                // The member updates and takes the twins: still never contradicted.
                let mut upgraded = member(&owner, legacy_node.clone(), true);
                catalog_sync(&mut upgraded, &legacy);
                full_sync(&mut upgraded, &owner);
                if let Some(project) = by_id_project(&upgraded) {
                    assert_eq!(Some(project), named(&upgraded), "{case}");
                }
            }
        }
    }

    /// The review's trailing case: a dual writer writes the twin after a
    /// legacy name move reached it. The twin follows the name operation now
    /// selected, never the project the writer last knew.
    /// #1413 review: a twin whose name operation a later one superseded is
    /// pruned; a twin of the current operation, or of one not known here yet,
    /// stays.
    #[test]
    fn prune_drops_only_twins_of_superseded_name_operations() {
        known(LABEL, ID);
        let mut owner = initialized(new_id());
        let me = format!("local\n{LABEL}");
        moved(
            &mut owner,
            organization(&[], &[]),
            organization(std::slice::from_ref(&me), &[]),
        );
        assert!(
            !owner.twins.clone().prune(&owner.catalog),
            "its operation is the head"
        );
        let mut unknown = owner.twins.entries.values().next().unwrap().clone();
        unknown.session_id = "7be01d44-0b4e-4d6a-9c1d-2e3f4a5b6c7d".into();
        unknown.op = new_id();
        let key = format!("{}\n{}", unknown.machine, unknown.session_id);
        owner.twins.entries.insert(key.clone(), unknown);
        let name = Key::Session {
            machine: owner.node_id.clone(),
            session: LABEL.into(),
        };
        let actor = owner.node_id.clone();
        owner.catalog.set(&actor, name, json!("play")).unwrap();
        assert!(owner.twins.prune(&owner.catalog));
        assert_eq!(
            owner.twins.entries.keys().collect::<Vec<_>>(),
            [&key],
            "an unknown operation stays"
        );
    }

    #[test]
    fn a_twin_written_after_a_legacy_move_follows_it() {
        known(LABEL, ID);
        let mut owner = initialized(new_id());
        let me = format!("local\n{LABEL}");
        moved(
            &mut owner,
            organization(&[], &[]),
            organization(std::slice::from_ref(&me), &[]),
        );
        let mut legacy = member(&owner, new_id(), false);
        let name = Key::Session {
            machine: owner.node_id.clone(),
            session: LABEL.into(),
        };
        legacy
            .catalog
            .set(&legacy.node_id.clone(), name, json!("play"))
            .unwrap();
        catalog_sync(&mut owner, &legacy);
        assert_eq!(by_id_project(&owner), None);
        write(
            &owner.catalog,
            &mut owner.twins,
            &owner.node_id.clone(),
            &[(owner.node_id.clone(), LABEL.into())],
        )
        .unwrap();
        assert_eq!(
            (named(&owner).as_deref(), by_id_project(&owner).as_deref()),
            (Some("play"), Some("play"))
        );
    }

    /// A label moved away and back by a member without twins may now name
    /// another session (a label reused in P3): the same project is not
    /// enough, only the exact name operation the twin mirrors.
    #[test]
    fn a_twin_is_read_only_for_the_exact_name_operation() {
        known(LABEL, ID);
        let mut owner = initialized(new_id());
        let me = format!("local\n{LABEL}");
        moved(
            &mut owner,
            organization(&[], &[]),
            organization(std::slice::from_ref(&me), &[]),
        );
        let mut legacy = member(&owner, new_id(), false);
        let name = Key::Session {
            machine: owner.node_id.clone(),
            session: LABEL.into(),
        };
        let actor = legacy.node_id.clone();
        legacy
            .catalog
            .set(&actor, name.clone(), json!("play"))
            .unwrap();
        legacy.catalog.set(&actor, name, json!("work")).unwrap();
        catalog_sync(&mut owner, &legacy);
        assert_eq!(
            (named(&owner).as_deref(), by_id_project(&owner)),
            (Some("work"), None)
        );
    }

    /// The coordinator's case: a peer's session moved by ID on its own
    /// computer, then by name only from a computer that cannot know its ID.
    #[test]
    fn a_later_name_only_move_of_a_peers_session_wins() {
        known(LABEL, ID);
        let mut owner = initialized(new_id());
        let mine = format!("local\n{LABEL}");
        moved(
            &mut owner,
            organization(&[], &[]),
            organization(std::slice::from_ref(&mine), &[]),
        );
        let mut other = member(&owner, new_id(), true);
        full_sync(&mut other, &owner);
        assert_eq!(by_id_project(&other).as_deref(), Some("work"));
        let theirs = format!("peer\n{LABEL}");
        moved(
            &mut other,
            organization(std::slice::from_ref(&theirs), &[]),
            organization(&[], std::slice::from_ref(&theirs)),
        );
        full_sync(&mut owner, &other);
        for store in [&owner, &other] {
            assert_eq!(
                (named(store).as_deref(), by_id_project(store)),
                (Some("play"), None)
            );
        }
        // The owner's next move writes both forms again, and the ID is read.
        moved(
            &mut owner,
            organization(&[], std::slice::from_ref(&mine)),
            organization(std::slice::from_ref(&mine), &[]),
        );
        assert_eq!(by_id_project(&owner).as_deref(), Some("work"));
        full_sync(&mut other, &owner);
        assert_eq!(by_id_project(&other).as_deref(), Some("work"));
    }

    #[test]
    fn only_this_computer_s_sessions_with_an_id_get_a_twin() {
        known(LABEL, ID);
        let mut a = initialized(new_id());
        let node = a.node_id.clone();
        for (machine, label) in [(node.clone(), "codex/work/unknown"), (new_id(), LABEL)] {
            let key = Key::Session {
                machine: machine.clone(),
                session: label.into(),
            };
            a.catalog.set(&node, key, json!("work")).unwrap();
            write(&a.catalog, &mut a.twins, &node, &[(machine, label.into())]).unwrap();
        }
        assert!(a.twins.entries.is_empty());
    }

    /// Twins travel only between members that list the kind, never inside
    /// the catalog, and a merge keeps the newest valid write per session ID.
    #[test]
    fn twins_merge_newest_first_and_stay_out_of_the_catalog_kinds() {
        assert!(advertised().contains(SESSION_ID_KIND));
        assert!(
            !kinds().contains(SESSION_ID_KIND),
            "no catalog key has the kind"
        );
        assert_eq!(
            kinds(),
            FLOOR_KINDS
                .into_iter()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        );
        let swarm = new_id();
        let twin = |clock: u64, project: &str| Twin {
            id: new_id(),
            actor: new_id(),
            clock,
            machine: "6b1f3a2e-1c4d-4e5f-8a9b-0c1d2e3f4a5b".into(),
            session_id: ID.into(),
            label: LABEL.into(),
            op: new_id(),
            project: Some(project.into()),
        };
        let mut twins = Twins::new(&swarm);
        let (old, new) = (twin(1, "work"), twin(2, "play"));
        twins.merge(&swarm, vec![new.clone(), old.clone()]).unwrap();
        assert_eq!(twins.entries.values().collect::<Vec<_>>(), [&new]);
        assert_eq!(twins.missing(&BTreeSet::from([new.id.clone()])), []);
        assert_eq!(twins.missing(&BTreeSet::new()), std::slice::from_ref(&new));
        assert!(
            twins.merge(&new_id(), vec![twin(3, "x")]).is_err(),
            "another swarm"
        );
        let invalid = [
            Twin {
                session_id: ID.to_uppercase(),
                ..twin(9, "work")
            },
            Twin {
                session_id: "00000000-0000-0000-0000-000000000000".into(),
                ..twin(9, "work")
            },
            Twin {
                label: String::new(),
                ..twin(9, "work")
            },
            Twin {
                op: "not-an-op".into(),
                ..twin(9, "work")
            },
            Twin {
                project: Some(String::new()),
                ..twin(9, "work")
            },
            Twin {
                clock: 0,
                ..twin(9, "work")
            },
        ];
        twins.merge(&swarm, invalid.to_vec()).unwrap();
        assert_eq!(
            twins.entries.values().collect::<Vec<_>>(),
            [&new],
            "invalid twins are skipped"
        );
        let digest = twins.digest();
        twins.merge(&swarm, vec![twin(3, "play")]).unwrap();
        assert_ne!(twins.digest(), digest);
    }

    /// `swarm export` gives a requester only the kinds it lists, and one that
    /// lists none (v0.47.0 and older) only the kinds v0.47.0 parses.
    #[test]
    fn export_keeps_the_kinds_its_requester_parses() {
        let mut a = initialized(new_id());
        let node = a.node_id.clone();
        a.catalog
            .set(&node, Key::preference("mobile_delivery"), json!("away"))
            .unwrap();
        let all = exported(&a.catalog, &Value::Null);
        assert_eq!(all.operations.len(), a.catalog.operations.len());
        let legacy = exported(
            &a.catalog,
            &json!(["project", "folder", "session", "machine"]),
        );
        assert!(legacy
            .operations
            .values()
            .all(|op| op.key.kind() != "preference"));
        assert!(legacy.operations.len() < a.catalog.operations.len());
        legacy.validate().unwrap();
    }
}
