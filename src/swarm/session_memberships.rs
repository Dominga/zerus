//! Primary ID project membership. The compatibility name catalog and derived
//! twins retain their byte schemas. This separate graph reuses Catalog's raw
//! causal heads, canonical machine identities and placement merge rules.
use super::{
    catalog::{Catalog, Key, Operation},
    read_regular,
    session_ids::{self, Twins},
    Unread,
};
use anyhow::{ensure, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) const FILE: &str = "session-id-memberships.json";
pub(super) const CAPABILITY: &str = "session_id_memberships_v1";
pub(super) const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) struct Memberships {
    graph: Catalog,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Resolved {
    pub value: Value,
    pub heads: Vec<String>,
    pub variants: Vec<Operation>,
    pub conflict: bool,
}
fn key(machine: &str, id: &str) -> Key {
    Key::Session {
        machine: machine.into(),
        session: id.into(),
    }
}
fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| !u.is_nil() && u.to_string() == id)
}

impl Memberships {
    pub(super) fn empty(base: &Catalog) -> Self {
        Self {
            graph: Catalog {
                schema: 1,
                swarm_id: base.swarm_id.clone(),
                operations: BTreeMap::new(),
            },
        }
    }
    /// Missing is the empty primary map, not an unreadable map. Never rebuild
    /// damaged primary data from the compatibility name view.
    pub(super) fn load(dir: &Path, base: &Catalog) -> Result<Self> {
        Self::from_read(base, &read_regular(&dir.join(FILE), MAX_BYTES))
    }
    pub(super) fn from_read(
        base: &Catalog,
        bytes: &std::result::Result<Vec<u8>, Unread>,
    ) -> Result<Self> {
        Self::from_read_view(base, bytes).map(|(value, _)| value)
    }
    /// [`Self::from_read`] together with the view its single validation
    /// produced (main-catalog redirects included), so a caller that also needs
    /// the validated view does not validate the same graph twice.
    pub(super) fn from_read_view(
        base: &Catalog,
        bytes: &std::result::Result<Vec<u8>, Unread>,
    ) -> Result<(Self, Catalog)> {
        let value = match bytes {
            Ok(bytes) => Self {
                graph: serde_json::from_slice(bytes)?,
            },
            Err(Unread::Missing) => Self::empty(base),
            Err(reason) => anyhow::bail!("ID project membership unavailable: {reason:?}"),
        };
        let view = value.view(base)?;
        Ok((value, view))
    }
    /// Project liveness and machine redirects come from the already validated
    /// main catalog. They are context, never copied into this primary file or
    /// accepted from its exchange. Catalog validates every raw causal edge in
    /// that same context; presentation never becomes a merge/write frontier.
    pub(super) fn view(&self, base: &Catalog) -> Result<Catalog> {
        ensure!(
            self.graph.schema == 1 && self.graph.swarm_id == base.swarm_id,
            "different ID membership swarm"
        );
        ensure!(self.graph.operations.values().all(|op|matches!(&op.key,Key::Session {machine,session} if valid_id(machine)&&valid_id(session))),
            "ID membership contains a non-ID key");
        let mut view = self.graph.clone();
        for op in base
            .operations
            .values()
            .filter(|op| matches!(op.key, Key::Project { .. } | Key::Machine { .. }))
        {
            ensure!(
                view.operations.insert(op.id.clone(), op.clone()).is_none(),
                "conflicting ID membership operation identity"
            );
        }
        view.validate()?;
        Ok(view)
    }
    fn adopt(&mut self, view: Catalog) {
        self.graph.operations = view
            .operations
            .into_iter()
            .filter(|(_, op)| matches!(op.key, Key::Session { .. }))
            .collect();
    }
    /// The raw graph's own operations, as parsed (a missing file is empty).
    pub(super) fn operation_count(&self) -> usize {
        self.graph.operations.len()
    }
    pub(super) fn is_empty(&self) -> bool {
        self.graph.operations.is_empty()
    }
    pub(super) fn token(machine: &str, id: &str) -> String {
        format!("session_id_memberships_v1:{machine}\n{id}")
    }
    pub(super) fn write(
        &mut self,
        base: &Catalog,
        actor: &str,
        machine: &str,
        id: &str,
        value: Value,
        parents: Vec<String>,
    ) -> Result<()> {
        ensure!(valid_id(id), "invalid membership session ID");
        let mut view = self.view(base)?;
        view.write(actor, key(&base.machine(machine), id), value, parents)?;
        view.validate()?;
        self.adopt(view);
        Ok(())
    }
    pub(super) fn known(&self) -> BTreeSet<String> {
        self.graph.operations.keys().cloned().collect()
    }
    pub(super) fn missing(&self, known: &BTreeSet<String>) -> Vec<Operation> {
        self.graph
            .operations
            .iter()
            .filter(|(id, _)| !known.contains(*id))
            .map(|(_, op)| op.clone())
            .collect()
    }
    pub(super) fn digest(&self) -> String {
        self.graph.digest()
    }
    pub(super) fn merge(
        &mut self,
        base: &Catalog,
        swarm: &str,
        operations: Vec<Operation>,
    ) -> Result<()> {
        ensure!(operations.iter().all(|op|matches!(&op.key,Key::Session {machine,session} if valid_id(machine)&&valid_id(session))),"invalid ID membership key");
        let mut view = self.view(base)?;
        view.merge(swarm, operations)?;
        self.adopt(view);
        Ok(())
    }
    pub(super) fn resolve(
        &self,
        base: &Catalog,
        machine: &str,
        id: &str,
    ) -> Result<Option<Resolved>> {
        Ok(self
            .resolved(base)?
            .remove(&(base.machine(machine), id.into())))
    }
    /// One graph pass for a whole catalog/listing, never one full read/clone
    /// per session. The four placement rules are Catalog's existing rules.
    pub(super) fn resolved(&self, base: &Catalog) -> Result<BTreeMap<(String, String), Resolved>> {
        let view = self.view(base)?;
        let mut raw = view.versions();
        let effective = view.effective_heads();
        let mut resolved = BTreeMap::new();
        for (key, versions) in &effective {
            let Key::Session { machine, session } = key else {
                continue;
            };
            let heads = raw
                .remove(&key.token())
                .ok_or_else(|| anyhow::anyhow!("ID membership heads unavailable"))?;
            let distinct: BTreeSet<String> =
                versions.iter().map(|op| op.value.to_string()).collect();
            let conflict = distinct.len() > 1;
            let value = if conflict {
                Value::Null
            } else {
                Catalog::selected(key, versions)
                    .map(|op| op.value.clone())
                    .unwrap_or(Value::Null)
            };
            resolved.insert(
                (machine.clone(), session.clone()),
                Resolved {
                    value,
                    variants: heads.iter().map(|id| view.operations[id].clone()).collect(),
                    heads,
                    conflict,
                },
            );
        }
        Ok(resolved)
    }
    pub(super) fn set(
        &mut self,
        base: &Catalog,
        actor: &str,
        machine: &str,
        id: &str,
        value: Value,
    ) -> Result<()> {
        ensure!(valid_id(id), "invalid membership session ID");
        let mut view = self.view(base)?;
        view.set(actor, key(&base.machine(machine), id), value)?;
        view.validate()?;
        self.adopt(view);
        Ok(())
    }
    /// Idempotent promotion of only valid, currently clock-bound twins. An
    /// existing primary record, including null or a conflict, always wins over
    /// a later compatibility write. Run under the catalog lock before a name
    /// change, and persist with that transaction.
    pub(super) fn migrate(&mut self, base: &Catalog, twins: &Twins) -> Result<()> {
        let existing = self.resolved(base)?;
        let identities = base.identities()?;
        let mut names = BTreeMap::<Key, Vec<&Operation>>::new();
        for op in base.operations.values() {
            if let Key::Session { machine, session } = &op.key {
                names
                    .entry(Key::Session {
                        machine: identities.get(machine).unwrap_or(machine).clone(),
                        session: session.clone(),
                    })
                    .or_default()
                    .push(op);
            }
        }
        let mut pending = Vec::new();
        for twin in session_ids::current_twins(base, twins) {
            let machine = identities
                .get(&twin.machine)
                .unwrap_or(&twin.machine)
                .clone();
            if existing.contains_key(&(machine.clone(), twin.session_id.clone())) {
                continue;
            }
            let name = Key::Session {
                machine,
                session: twin.label.clone(),
            };
            // Copy the whole causal field, not the selected presentation head.
            // In particular migration may not settle two concrete choices.
            for op in names.get(&name).into_iter().flatten() {
                ensure!(
                    pending.len() + self.graph.operations.len() < 100_000,
                    "ID membership exceeds the catalog bound"
                );
                let mut mapped = (**op).clone();
                mapped.id = migration_id(&twin.session_id, &op.id);
                mapped.parents = op
                    .parents
                    .iter()
                    .map(|id| migration_id(&twin.session_id, id))
                    .collect();
                let Key::Session { machine, .. } = &op.key else {
                    unreachable!()
                };
                mapped.key = key(machine, &twin.session_id);
                pending.push(mapped.clone());
            }
        }
        let mut migrated = self.clone();
        migrated.merge(base, &base.swarm_id, pending)?;
        migrated.bytes()?;
        *self = migrated;
        Ok(())
    }
    /// Initial enrollment maps only project identities, keeping every ID's
    /// original causal history. This is an explicit join, not an older name
    /// writer's permission to change ID grouping.
    pub(super) fn rehome(
        &mut self,
        base: &Catalog,
        projects: &BTreeMap<String, String>,
    ) -> Result<()> {
        let mut next = self.clone();
        next.graph.swarm_id = base.swarm_id.clone();
        for op in next.graph.operations.values_mut() {
            if let Some(to) = op.value.as_str().and_then(|id| projects.get(id)) {
                op.value = Value::String(to.clone());
            }
        }
        next.view(base)?;
        *self = next;
        Ok(())
    }
    pub(super) fn bytes(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(&self.graph)?;
        ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "ID membership exceeds its bound"
        );
        Ok(bytes)
    }
}

fn migration_id(session: &str, operation: &str) -> String {
    let hash = Sha256::digest(format!(
        "id-membership-migration-v1\0{session}\0{operation}"
    ));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

#[cfg(test)]
mod tests {
    use super::super::catalog::new_id;
    use super::*;
    use serde_json::json;
    fn base() -> (Catalog, String, String) {
        let mut catalog = Catalog::default();
        let actor = new_id();
        let id = new_id();
        for project in ["one", "two"] {
            catalog
                .set(&actor, Key::project(project, "alive"), json!(true))
                .unwrap();
        }
        (catalog, actor, id)
    }
    #[test]
    fn a_known_concrete_project_beats_concurrent_defaults_without_losing_raw_heads() {
        let (base, actor, id) = base();
        for default in [Value::Null, json!("ungrouped")] {
            let mut a = Memberships::empty(&base);
            let mut b = a.clone();
            a.set(&base, &actor, &actor, &id, json!("one")).unwrap();
            b.set(&base, &new_id(), &actor, &id, default).unwrap();
            a.merge(&base, &base.swarm_id, b.missing(&BTreeSet::new()))
                .unwrap();
            let result = a.resolve(&base, &actor, &id).unwrap().unwrap();
            assert_eq!(result.value, "one");
            assert!(!result.conflict);
            assert_eq!(result.heads.len(), 2);
        }
    }
    #[test]
    fn distinct_concrete_heads_are_reviewable_and_a_later_explicit_default_wins() {
        let (base, actor, id) = base();
        let mut a = Memberships::empty(&base);
        let mut b = a.clone();
        a.set(&base, &actor, &actor, &id, json!("one")).unwrap();
        b.set(&base, &new_id(), &actor, &id, json!("two")).unwrap();
        a.merge(&base, &base.swarm_id, b.missing(&BTreeSet::new()))
            .unwrap();
        let conflict = a.resolve(&base, &actor, &id).unwrap().unwrap();
        assert!(conflict.conflict);
        assert_eq!(conflict.heads.len(), 2);
        for explicit in [Value::Null, json!("ungrouped")] {
            let mut settled = a.clone();
            settled
                .set(&base, &actor, &actor, &id, explicit.clone())
                .unwrap();
            let result = settled.resolve(&base, &actor, &id).unwrap().unwrap();
            assert_eq!(result.value, explicit);
            assert!(!result.conflict);
            assert_eq!(result.heads.len(), 1);
            // Both original concrete heads are parents, not silently erased.
            assert_eq!(settled.known().len(), 3);
        }
    }
    #[test]
    fn two_ids_diverge_independently_and_survive_name_changes_and_peer_round_trips() {
        let (mut base, actor, a) = base();
        let b = new_id();
        let mut source = Memberships::empty(&base);
        source.set(&base, &actor, &actor, &a, json!("one")).unwrap();
        source.set(&base, &actor, &actor, &b, json!("two")).unwrap();
        base.set(
            &actor,
            Key::Session {
                machine: actor.clone(),
                session: "claude/example/shared".into(),
            },
            json!("two"),
        )
        .unwrap();
        base.set(
            &actor,
            Key::Session {
                machine: actor.clone(),
                session: "claude/example/renamed".into(),
            },
            json!("one"),
        )
        .unwrap();
        let mut peer = Memberships::empty(&base);
        peer.merge(&base, &base.swarm_id, source.missing(&BTreeSet::new()))
            .unwrap();
        source
            .merge(&base, &base.swarm_id, peer.missing(&source.known()))
            .unwrap();
        assert_eq!(source.digest(), peer.digest());
        assert_eq!(
            peer.resolve(&base, &actor, &a).unwrap().unwrap().value,
            "one"
        );
        assert_eq!(
            peer.resolve(&base, &actor, &b).unwrap().unwrap().value,
            "two"
        );
        assert!(peer.resolve(&base, &actor, &new_id()).unwrap().is_none());
        assert_eq!(
            serde_json::from_slice::<Catalog>(&source.bytes().unwrap())
                .unwrap()
                .operations
                .len(),
            2
        );
    }
    #[test]
    fn migration_preserves_all_raw_heads_and_never_overwrites_an_existing_id() {
        let (base, actor, id) = base();
        let name = "claude/example/shared";
        let name_key = Key::Session {
            machine: actor.clone(),
            session: name.into(),
        };
        for other in [Value::Null, json!("ungrouped"), json!("two")] {
            let mut a = base.clone();
            let mut b = base.clone();
            a.set(&actor, name_key.clone(), json!("one")).unwrap();
            b.set(&new_id(), name_key.clone(), other.clone()).unwrap();
            a.merge(&base.swarm_id, b.operations.values().cloned().collect())
                .unwrap();
            let heads = a.effective_heads();
            let chosen = Catalog::selected(&name_key, heads.get(&name_key).unwrap()).unwrap();
            let twin = session_ids::Twin {
                id: new_id(),
                actor: actor.clone(),
                clock: 1,
                machine: actor.clone(),
                session_id: id.clone(),
                label: name.into(),
                op: chosen.id.clone(),
                project: chosen.value.as_str().map(str::to_owned),
            };
            let twins = Twins {
                swarm_id: base.swarm_id.clone(),
                entries: BTreeMap::from([(format!("{actor}\n{id}"), twin)]),
            };
            let mut primary = Memberships::empty(&a);
            primary.migrate(&a, &twins).unwrap();
            let result = primary.resolve(&a, &actor, &id).unwrap().unwrap();
            assert_eq!(result.heads.len(), 2);
            assert_eq!(result.conflict, other == "two");
            if other != "two" {
                assert_eq!(result.value, "one");
            }
            let before = primary.bytes().unwrap();
            primary.migrate(&a, &twins).unwrap();
            assert_eq!(before, primary.bytes().unwrap());
            primary.set(&a, &actor, &actor, &id, Value::Null).unwrap();
            primary.migrate(&a, &twins).unwrap();
            assert_eq!(
                primary.resolve(&a, &actor, &id).unwrap().unwrap().value,
                Value::Null
            );
        }
        // A valid null twin is explicit data too, not an absent ID record.
        let mut named = base.clone();
        named.set(&actor, name_key.clone(), Value::Null).unwrap();
        let op = named
            .operations
            .values()
            .find(|op| op.key == name_key)
            .unwrap();
        let twin = session_ids::Twin {
            id: new_id(),
            actor: actor.clone(),
            clock: 1,
            machine: actor.clone(),
            session_id: id.clone(),
            label: name.into(),
            op: op.id.clone(),
            project: None,
        };
        let twins = Twins {
            swarm_id: base.swarm_id.clone(),
            entries: BTreeMap::from([(format!("{actor}\n{id}"), twin)]),
        };
        let mut primary = Memberships::empty(&named);
        primary.migrate(&named, &twins).unwrap();
        assert_eq!(
            primary.resolve(&named, &actor, &id).unwrap().unwrap().value,
            Value::Null
        );
    }
    #[test]
    fn only_missing_primary_data_is_absence_and_foreign_or_malformed_data_refuses() {
        let (base, actor, id) = base();
        let directory = tempfile::tempdir().unwrap();
        assert!(Memberships::load(directory.path(), &base)
            .unwrap()
            .resolve(&base, &actor, &id)
            .unwrap()
            .is_none());
        let path = directory.path().join(FILE);
        for invalid in [b"{}".as_slice(), b"null", b"{broken"] {
            std::fs::write(&path, invalid).unwrap();
            assert!(Memberships::load(directory.path(), &base).is_err());
        }
        let mut foreign = Memberships::empty(&base);
        foreign.graph.swarm_id = new_id();
        std::fs::write(&path, foreign.bytes().unwrap()).unwrap();
        assert!(Memberships::load(directory.path(), &base).is_err());
        let mut forbidden = Memberships::empty(&base);
        forbidden
            .graph
            .set(&actor, Key::project("foreign", "alive"), json!(true))
            .unwrap();
        std::fs::write(&path, forbidden.bytes().unwrap()).unwrap();
        assert!(Memberships::load(directory.path(), &base).is_err());
    }
}
