//! Session membership by durable ID (zerus-ucck P2c-1). Every writer of a
//! session's membership by label ([`Key::Session`]) also writes its ID twin
//! ([`Key::SessionId`], valued `{project, label}`) for this computer's own
//! sessions, whose ID it can read. The name form stays the one readers use
//! until P3. A desktop reads the ID form only once every machine of the
//! swarm lists the `session_id` kind (`membership_by_id`), and only where the
//! twin is at least as new as the name membership for its recorded label, so
//! an older writer's later name-only move still wins.
use super::catalog::{Catalog, Key, Operation};
use anyhow::Result;
use serde_json::{json, Value};

/// The durable session ID of this computer's live or saved session `label`.
#[cfg(not(test))]
fn session_id_of(label: &str) -> Option<String> {
    crate::state::session_id_of(label)
}

#[cfg(test)]
thread_local! {
    /// Labels and IDs the tests' sessions have; never the real state.
    pub(super) static SESSIONS: std::cell::RefCell<std::collections::BTreeMap<String, String>> =
        std::cell::RefCell::default();
}

#[cfg(test)]
fn session_id_of(label: &str) -> Option<String> {
    SESSIONS.with(|sessions| sessions.borrow().get(label).cloned())
}

/// The order of operations the clock rule compares.
fn order(op: &Operation) -> (u64, &str, &str) {
    (op.clock, op.actor.as_str(), op.id.as_str())
}

/// After `machine`'s session `label` was given `project` (a project ID, or
/// null to unassign) by name, writes its ID twin when this computer's record
/// of that label carries a session ID. An unchanged twin at least as new as
/// that name membership is not written again.
pub(super) fn twin(
    catalog: &mut Catalog,
    actor: &str,
    machine: &str,
    label: &str,
    project: &Value,
) -> Result<()> {
    if catalog.machine(machine) != catalog.machine(actor) {
        return Ok(());
    }
    let Some(session_id) = session_id_of(label) else {
        return Ok(());
    };
    let key = Key::SessionId {
        machine: machine.to_owned(),
        session_id,
    };
    let value = match project {
        Value::Null => Value::Null,
        project => json!({"project": project, "label": label}),
    };
    let heads = catalog.heads();
    let name = Key::Session {
        machine: machine.to_owned(),
        session: label.to_owned(),
    };
    let named = heads
        .get(&catalog.canonical_key(&name))
        .and_then(|heads| Catalog::selected(&name, heads));
    let current = heads.get(&catalog.canonical_key(&key));
    let fresh = current
        .and_then(|heads| Catalog::selected(&key, heads))
        .is_some_and(|op| op.value == value && named.is_none_or(|named| order(op) >= order(named)));
    if fresh {
        return Ok(());
    }
    let parents = current
        .map(|heads| heads.iter().map(|op| op.id.clone()).collect())
        .unwrap_or_default();
    catalog.write(actor, key, value, parents)
}

/// The ID memberships a desktop may read: `(machine, session ID, project)`
/// for each twin whose selected operation is at least as new as the name
/// membership for its recorded label, or whose label has no name membership.
pub(super) fn readable(
    catalog: &Catalog,
    heads: &std::collections::BTreeMap<Key, Vec<&Operation>>,
) -> Vec<(String, String, String)> {
    let mut result = Vec::new();
    for (key, versions) in heads {
        let Key::SessionId {
            machine,
            session_id,
        } = key
        else {
            continue;
        };
        let Some(op) = Catalog::selected(key, versions) else {
            continue;
        };
        let (Some(project), Some(label)) =
            (op.value["project"].as_str(), op.value["label"].as_str())
        else {
            continue;
        };
        let name = catalog.canonical_key(&Key::Session {
            machine: machine.clone(),
            session: label.to_owned(),
        });
        let newer_name = heads
            .get(&name)
            .and_then(|heads| Catalog::selected(&name, heads))
            .is_some_and(|named| order(named) > order(op));
        if !newer_name {
            result.push((machine.clone(), session_id.clone(), project.to_owned()));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::swarm::catalog::{accepted, kinds, new_id, LEGACY_KINDS};
    use crate::swarm::model::Patch;
    use crate::swarm::store::{Binding, Store};
    use std::collections::{BTreeMap, BTreeSet};

    const ID: &str = "3f2a9c1d-0b4e-4d6a-9c1d-2e3f4a5b6c7d";
    const LABEL: &str = "codex/work/one";

    fn store() -> Store {
        let node_id = new_id();
        Store {
            node_id: node_id.clone(),
            catalog: Catalog::default(),
            imported: false,
            bindings: BTreeMap::from([(
                "local".into(),
                Binding {
                    id: node_id,
                    verified: true,
                },
            )]),
            peers: BTreeMap::new(),
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
        let snapshot = store.snapshot(&config());
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

    fn named(store: &Store, project: &str) -> Vec<String> {
        let snapshot = store.snapshot(&config());
        let projects = snapshot["organization"]["projects"]
            .as_array()
            .unwrap()
            .clone();
        let found = projects.into_iter().find(|p| p["id"] == project).unwrap();
        found["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect()
    }

    fn twin_heads(store: &Store) -> usize {
        store
            .catalog
            .heads()
            .keys()
            .filter(|key| matches!(key, Key::SessionId { .. }))
            .count()
    }

    /// A desktop's move of this computer's session also moves it by ID; the
    /// ID twin never reaches the versions a desktop parses.
    #[test]
    fn a_local_move_writes_the_id_twin() {
        known(LABEL, ID);
        let mut a = store();
        a.initialize(&organization(&[], &[])).unwrap();
        let member = format!("local\n{LABEL}");
        let moves = [
            (
                organization(&[], &[]),
                organization(std::slice::from_ref(&member), &[]),
            ),
            (
                organization(std::slice::from_ref(&member), &[]),
                organization(&[], std::slice::from_ref(&member)),
            ),
        ];
        for (expected, (base, desired)) in [("work", &moves[0]), ("play", &moves[1])] {
            let versions = a.catalog.versions();
            a.apply(Patch {
                base: base.clone(),
                desired: desired.clone(),
                versions,
            })
            .unwrap();
            let shown = by_id(&a);
            assert_eq!(shown[expected], [ID], "{expected}");
            assert!(shown
                .iter()
                .filter(|(p, _)| *p != expected)
                .all(|(_, ids)| ids.is_empty()));
            let value = &a.catalog.values()[&Key::SessionId {
                machine: a.node_id.clone(),
                session_id: ID.into(),
            }];
            assert_eq!(value, &json!({"project": expected, "label": LABEL}));
        }
        let snapshot = a.snapshot(&config());
        let tokens = snapshot["versions"].as_object().unwrap();
        assert!(tokens.keys().all(|token| !token.contains("session_id")));
        assert!(a
            .catalog
            .versions()
            .keys()
            .any(|token| token.contains("session_id")));
        // Unassigning by name unassigns by ID.
        let versions = a.catalog.versions();
        a.apply(Patch {
            base: moves[1].1.clone(),
            desired: organization(&[], &[]),
            versions,
        })
        .unwrap();
        assert!(by_id(&a).values().all(Vec::is_empty));
        assert_eq!(twin_heads(&a), 1);
    }

    /// The coordinator's case: a peer's session moved by ID on its own
    /// computer, then moved by name only from another computer (a Qt or older
    /// desktop, whose CLI cannot know a peer's IDs). The newer name move wins,
    /// so a desktop reading by ID falls back to it and shows the new project.
    #[test]
    fn a_later_name_only_move_of_a_peers_session_wins() {
        known(LABEL, ID);
        let mut owner = store();
        owner.initialize(&organization(&[], &[])).unwrap();
        let mut other = owner.clone();
        other.node_id = new_id();
        other.bindings = BTreeMap::from([
            (
                "local".into(),
                Binding {
                    id: other.node_id.clone(),
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
        // On its own computer the session moves to `work`, by name and by ID.
        let mine = format!("local\n{LABEL}");
        let versions = owner.catalog.versions();
        owner
            .apply(Patch {
                base: organization(&[], &[]),
                desired: organization(std::slice::from_ref(&mine), &[]),
                versions,
            })
            .unwrap();
        assert_eq!(by_id(&owner)["work"], [ID]);
        let sync = |to: &mut Store, from: &Store| {
            to.catalog
                .merge(
                    &from.catalog.swarm_id,
                    from.catalog.operations.values().cloned().collect(),
                )
                .unwrap();
        };
        sync(&mut other, &owner);
        assert_eq!(by_id(&other)["work"], [ID]);
        // The other computer moves it to `play` by name only.
        let theirs = format!("peer\n{LABEL}");
        let versions = other.catalog.versions();
        other
            .apply(Patch {
                base: organization(std::slice::from_ref(&theirs), &[]),
                desired: organization(&[], std::slice::from_ref(&theirs)),
                versions,
            })
            .unwrap();
        sync(&mut owner, &other);
        for store in [&owner, &other] {
            assert!(
                by_id(store).values().all(Vec::is_empty),
                "the older ID twin is not read"
            );
        }
        assert_eq!(
            named(&owner, "play"),
            [format!("{}\n{LABEL}", config().host())]
        );
        assert_eq!(named(&other, "play"), [theirs]);
        // The owner's next move writes both forms again, and the ID is read.
        let versions = owner.catalog.versions();
        owner
            .apply(Patch {
                base: organization(&[], std::slice::from_ref(&mine)),
                desired: organization(std::slice::from_ref(&mine), &[]),
                versions,
            })
            .unwrap();
        assert_eq!(by_id(&owner)["work"], [ID]);
    }

    /// An older hgs on this computer moves the session by name only: the ID
    /// twin is no longer read until a writer of both forms moves it again.
    #[test]
    fn an_older_writer_s_name_only_move_hides_the_twin() {
        known(LABEL, ID);
        let mut a = store();
        a.initialize(&organization(&[], &[])).unwrap();
        let node = a.node_id.clone();
        let name = Key::Session {
            machine: node.clone(),
            session: LABEL.into(),
        };
        a.catalog.set(&node, name.clone(), json!("work")).unwrap();
        twin(&mut a.catalog, &node, &node, LABEL, &json!("work")).unwrap();
        assert_eq!(by_id(&a)["work"], [ID]);
        a.catalog.set(&node, name, json!("play")).unwrap();
        assert!(by_id(&a).values().all(Vec::is_empty));
        // Writing the same twin again only when it is not fresh.
        twin(&mut a.catalog, &node, &node, LABEL, &json!("play")).unwrap();
        let operations = a.catalog.operations.len();
        twin(&mut a.catalog, &node, &node, LABEL, &json!("play")).unwrap();
        assert_eq!(
            a.catalog.operations.len(),
            operations,
            "a fresh twin is not written again"
        );
        assert_eq!(by_id(&a)["play"], [ID]);
        // A session without a known ID, or on another computer, gets no twin.
        twin(
            &mut a.catalog,
            &node,
            &node,
            "codex/work/unknown",
            &json!("play"),
        )
        .unwrap();
        twin(&mut a.catalog, &node, &new_id(), LABEL, &json!("play")).unwrap();
        assert_eq!(a.catalog.operations.len(), operations);
    }

    #[test]
    fn only_members_that_list_the_kind_receive_a_twin() {
        let key = Key::SessionId {
            machine: new_id(),
            session_id: ID.into(),
        };
        assert!(accepted(Some(&kinds()), &key));
        assert!(!accepted(None, &key), "a legacy member never receives one");
        let older: BTreeSet<String> = kinds()
            .into_iter()
            .filter(|kind| kind != "session_id")
            .collect();
        assert!(!accepted(Some(&older), &key));
        assert!(!LEGACY_KINDS.contains(&"session_id"));
        let mut catalog = Catalog::default();
        let actor = new_id();
        let write = |catalog: &mut Catalog, key: Key, value: Value| {
            catalog.write(&actor, key, value, vec![])
        };
        assert!(write(
            &mut catalog,
            key.clone(),
            json!({"project": "work", "label": LABEL})
        )
        .is_ok());
        assert!(write(&mut catalog, key.clone(), Value::Null).is_ok());
        for value in [
            json!("work"),
            json!({"project": "work"}),
            json!({"project": "work", "label": ""}),
            json!({"project": "work", "label": LABEL, "extra": 1}),
            json!({"project": "", "label": LABEL}),
        ] {
            assert!(
                write(&mut catalog, key.clone(), value.clone()).is_err(),
                "{value}"
            );
        }
        for session_id in [
            ID.to_uppercase(),
            "not-an-id".into(),
            "00000000-0000-0000-0000-000000000000".into(),
        ] {
            let bad = Key::SessionId {
                machine: new_id(),
                session_id,
            };
            assert!(write(
                &mut catalog,
                bad,
                json!({"project": "work", "label": LABEL})
            )
            .is_err());
        }
    }
}
