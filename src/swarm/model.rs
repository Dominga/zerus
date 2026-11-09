use super::{
    catalog::{Catalog, Key},
    store::Store,
};
use crate::config::Config;
use anyhow::{ensure, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// A drive (`C:\`, `c:/`) or UNC (`\\server\share`, `//server/share`) path: on this
/// machine, a local Windows folder of a desktop (D15 of the Windows native host design).
/// WhiteWh fork: the catalog shares drive folders of every machine
/// (`catalog::drive_path`); a UNC folder of this machine still stays in that
/// desktop's organization. A peer's `//server/share` is a POSIX path, which a Windows
/// GUI can store (`QDir::cleanPath` keeps the leading `//` there). Matches
/// `MachinePlatform::windowsShaped` in the GUI.
fn windows_folder(path: &str) -> bool {
    let separator = |c: &u8| *c == b'/' || *c == b'\\';
    match path.as_bytes() {
        [drive, b':', next, ..] if drive.is_ascii_alphabetic() && separator(next) => true,
        // Two separators, a server name, a separator and a share name.
        [first, second, rest @ ..] if separator(first) && separator(second) => {
            let end = rest.iter().position(separator);
            end.is_some_and(|end| end > 0 && rest.get(end + 1).is_some_and(|c| !separator(c)))
        }
        _ => false,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub base: Value,
    pub desired: Value,
    pub versions: BTreeMap<String, Vec<String>>,
}

impl Store {
    pub fn flatten(&mut self, organization: &Value) -> Result<BTreeMap<Key, Value>> {
        let mut fields = BTreeMap::new();
        let local = self.catalog.machine(&self.node_id);
        let mut machines = BTreeMap::<String, String>::new();
        let mut machine_id = |alias: &str| -> Result<String> {
            if let Some(id) = machines.get(alias) {
                return Ok(id.clone());
            }
            let id = self.machine_id(alias)?;
            machines.insert(alias.into(), id.clone());
            Ok(id)
        };
        let projects = organization["projects"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("organization needs projects"))?;
        ensure!(projects.len() <= 5000, "too many projects");
        for p in projects {
            let id = p["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing project ID"))?;
            ensure!(
                !fields.contains_key(&Key::project(id, "alive")),
                "duplicate project ID"
            );
            fields.insert(Key::project(id, "alive"), json!(true));
            for field in ["name", "color"] {
                fields.insert(
                    Key::project(id, field),
                    p.get(field).cloned().unwrap_or_else(|| match field {
                        "color" => json!("#64b5f6"),
                        _ => json!("Project"),
                    }),
                );
            }
            if let Some(folders) = p["folders"].as_array() {
                for f in folders {
                    let alias = f["machine_id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| f["machine"].as_str())
                        .unwrap_or_default();
                    // A local folder on a server share is skipped whole. Drive paths
                    // are shared (WhiteWh fork); other invalid paths fail when written.
                    let share = f["path"].as_str().is_some_and(|p| windows_folder(p) && !super::catalog::drive_path(p));
                    if share && machine_id(alias)? == local {
                        continue;
                    }
                    let folder = f["id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing folder ID"))?;
                    let machine = machine_id(alias)?;
                    let key = Key::Folder { id: folder.into() };
                    ensure!(!fields.contains_key(&key), "duplicate folder ID");
                    fields.insert(key,json!({"project":id,"machine":machine,"path":f["path"],"name":f.get("name").cloned().unwrap_or(json!(""))}));
                }
            }
            if let Some(sessions) = p["sessions"].as_array() {
                for s in sessions {
                    // The marker is local presentation, even if an importer or
                    // older UI bypassed the current desktop's payload filter.
                    if organization["unassigned_sessions"].as_array()
                        .is_some_and(|unassigned| unassigned.contains(s)) { continue; }
                    let (machine, session) = s
                        .as_str()
                        .and_then(|s| s.split_once('\n'))
                        .ok_or_else(|| anyhow::anyhow!("invalid session identity"))?;
                    let machine = machine_id(machine)?;
                    let key = Key::Session {
                        machine,
                        session: session.into(),
                    };
                    ensure!(
                        !fields.contains_key(&key),
                        "session belongs to more than one project"
                    );
                    fields.insert(key, json!(id));
                }
            }
        }
        // Only desktops that know the shared order send it; an older desktop's
        // local order stays local and never overrides the swarm's.
        if let Some(order) = organization.get("project_order") {
            fields.insert(Key::project_order(), order.clone());
        }
        Ok(fields)
    }

    pub fn apply(&mut self, patch: Patch) -> Result<()> {
        let before = self.flatten(&patch.base)?;
        let after = self.flatten(&patch.desired)?;
        let current = self.catalog.values();
        let observed: BTreeMap<Key, Vec<String>> = patch
            .versions
            .iter()
            .filter_map(|(token, ids)| {
                serde_json::from_str::<Key>(token)
                    .ok()
                    .map(|key| (self.catalog.canonical_key(&key), ids.clone()))
            })
            .fold(BTreeMap::new(), |mut result, (key, ids)| {
                result.entry(key).or_default().extend(ids);
                result
            });
        let keys: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
        let mut labels = Vec::new();
        for key in keys {
            if before.get(&key) == after.get(&key) {
                continue;
            }
            // Older desktops publish passive default placement as membership.
            // Keep their late observations from undoing shared assignments.
            // New desktops omit passive rows and identify explicit membership,
            // whose unseen concurrent edits must remain genuine conflicts.
            if matches!(key, Key::Session { .. })
                && !before.contains_key(&key)
                && current.contains_key(&key)
                && patch.desired["session_assignments_explicit"] != true
            {
                continue;
            }
            let value = match after.get(&key) {
                Some(value) => value.clone(),
                None => match &key {
                    Key::Project { field, .. } if field == "alive" => json!(false),
                    Key::Folder { .. } | Key::Session { .. } => Value::Null,
                    _ => continue,
                },
            };
            if matches!(key, Key::Folder { .. }) {
                if let Some(old) = before.get(&key).filter(|v| !v.is_null()) {
                    // The UI coalesces duplicate locations from independent imports.
                    // Editing/removing that location also removes the observed
                    // duplicates, so a hidden duplicate cannot reappear afterwards.
                    for (duplicate, location) in &current {
                        if duplicate == &key
                            || !matches!(duplicate, Key::Folder { .. })
                            || location.is_null()
                        {
                            continue;
                        }
                        let same = location["project"] == old["project"]
                            && location["path"] == old["path"]
                            && self
                                .catalog
                                .machine(location["machine"].as_str().unwrap_or_default())
                                == old["machine"].as_str().unwrap_or_default();
                        if same {
                            if let Some(parents) = observed.get(duplicate) {
                                self.catalog.write(
                                    &self.node_id,
                                    duplicate.clone(),
                                    Value::Null,
                                    parents.clone(),
                                )?;
                            }
                        }
                    }
                }
            }
            let parents = observed.get(&key).cloned().unwrap_or_default();
            if let Key::Session { machine, session } = &key {
                labels.push((machine.clone(), session.clone()));
            }
            self.catalog.write(&self.node_id, key, value, parents)?;
        }
        // This computer's sessions also move by ID (zerus-ucck P2c-1).
        super::session_ids::write(&self.catalog, &mut self.twins, &self.node_id, &labels)?;
        if let Some(primary) = &mut self.id_memberships {
            // New clients carry the ID graph's observed raw heads unchanged in
            // versions. Old name-only clients cannot supersede ID data. Never
            // use the current selected value as a causal write frontier.
            for twin in super::session_ids::current_twins(&self.catalog,&self.twins) {
                let machine=self.catalog.machine(&twin.machine);
                if !labels.contains(&(machine.clone(),twin.label.clone())) { continue; }
                let token=super::session_memberships::Memberships::token(&machine,&twin.session_id);
                if let Some(parents)=patch.versions.get(&token) {
                    let key=Key::Session {machine:machine.clone(),session:twin.label.clone()};
                    let value=after.get(&key).cloned().unwrap_or(Value::Null);
                    primary.write(&self.catalog,&self.node_id,&machine,&twin.session_id,value,parents.clone())?;
                }
            }
            primary.migrate(&self.catalog,&self.twins)?;
        }
        Ok(())
    }

    pub fn initialize(&mut self, organization: &Value) -> Result<()> {
        if self.imported {
            return Ok(());
        }
        self.import_missing(organization)
    }

    /// A node-bound pending GUI import can fill absent fields after a CLI
    /// bootstrap. Existing values, tombstones and concurrent heads stay intact.
    pub fn import_missing(&mut self, organization: &Value) -> Result<()> {
        let fields = self.flatten(organization)?;
        let existing = self.catalog.values();
        for (key, value) in fields {
            // The GUI may first open after this node joined. Import only fields
            // absent from the received catalog, never replay stale local values.
            if !existing.contains_key(&key) {
                self.catalog.write(&self.node_id, key, value, vec![])?;
            }
        }
        self.imported = true;
        Ok(())
    }

    pub fn snapshot(&self, config: &Config) -> Value {
        let values = self.catalog.values();
        let identities = self.catalog.identities().unwrap_or_default();
        let canonical = |id: &str| identities.get(id).cloned().unwrap_or_else(|| id.into());
        let name = |id: &str| {
            values
                .get(&Key::machine(id, "name"))
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_owned()
        };
        let mut aliases = BTreeMap::from([(self.node_id.clone(), config.host().to_owned())]);
        for alias in &config.peers {
            if let Some(binding) = self.bindings.get(alias) {
                aliases
                    .entry(canonical(&binding.id))
                    .or_insert_with(|| alias.clone());
            }
        }
        let mut projects: BTreeMap<String, Value> = BTreeMap::new();
        for (key, value) in &values {
            if let Key::Project { id, field } = key {
                if field == "alive" && value == true {
                    let field = |f| {
                        values
                            .get(&Key::project(id, f))
                            .cloned()
                            .unwrap_or(Value::Null)
                    };
                    projects.insert(id.clone(),json!({"id":id,"name":field("name"),"color":field("color"),"folders":[],"sessions":[],"accessible":false}));
                }
            }
        }
        for (key, value) in &values {
            match key {
                Key::Folder { id } if !value.is_null() => {
                    if let Some(p) = value["project"]
                        .as_str()
                        .and_then(|id| projects.get_mut(id))
                    {
                        let machine = canonical(value["machine"].as_str().unwrap_or_default());
                        let alias = aliases.get(&machine).cloned();
                        if alias.is_some() {
                            p["accessible"] = json!(true);
                        }
                        let folders = p["folders"].as_array_mut().unwrap();
                        // Independently imported folder locations can have distinct IDs.
                        // Keep both records durable; present the same location only once.
                        if !folders
                            .iter()
                            .any(|f| f["machine_id"] == machine && f["path"] == value["path"])
                        {
                            folders.push(json!({"id":id,"machine":alias.unwrap_or(machine.clone()),"machine_id":machine,"machine_name":name(&machine),"path":value["path"],"name":value["name"]}));
                        }
                    }
                }
                Key::Session { machine, session } if !value.is_null() => {
                    if let Some(p) = value.as_str().and_then(|id| projects.get_mut(id)) {
                        let alias = aliases.get(machine).cloned();
                        if alias.is_some() {
                            p["accessible"] = json!(true);
                        }
                        p["sessions"].as_array_mut().unwrap().push(json!(format!(
                            "{}\n{session}",
                            alias.unwrap_or(machine.clone())
                        )));
                    }
                }
                _ => {}
            }
        }
        for p in projects.values_mut() {
            if p["folders"].as_array().unwrap().is_empty()
                && p["sessions"].as_array().unwrap().is_empty()
            {
                p["accessible"] = json!(true);
            }
        }
        // Primary ID data wins even when it is null or conflicted. Derived
        // name twins are a fallback ONLY when this exact ID has no record.
        // Unavailable primary data never becomes an absent record.
        let primary = self
            .id_memberships
            .as_ref()
            .and_then(|p| p.resolved(&self.catalog).ok());
        let mut memberships = BTreeMap::new();
        if let Some(primary) = &primary {
            for (machine, id, project) in super::session_ids::readable(&self.catalog, &self.twins) {
                if !primary.contains_key(&(machine.clone(), id.clone())) {
                    memberships.insert((machine, id), project);
                }
            }
            for ((machine, id), record) in primary {
                if let Some(project) = record.value.as_str() {
                    memberships.insert((machine.clone(), id.clone()), project.to_owned());
                }
            }
        }
        for ((machine, session_id), project) in memberships {
            if let Some(p) = projects.get_mut(&project) {
                let machine = aliases.get(&machine).cloned().unwrap_or(machine);
                p
                    .as_object_mut()
                    .unwrap()
                    .entry("session_ids")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .unwrap()
                    .push(json!(format!("{machine}\n{session_id}")));
            }
        }
        let mut versions=self.catalog.versions();
        for ((machine,id),record) in primary.as_ref().into_iter().flat_map(|p|p.iter()) {
            versions.insert(super::session_memberships::Memberships::token(machine,id),record.heads.clone());
        }
        let records: Vec<_> = primary.as_ref().into_iter().flat_map(|map| map.iter()).map(|((machine,id), record)|
            json!({"machine":machine,"session_id":id,"project":record.value,"heads":record.heads,"variants":record.variants,"conflict":record.conflict})).collect();
        let id_state = json!({"available":primary.is_some(),"records":records});
        let digest = self.id_memberships.as_ref().map_or_else(
            || self.catalog.digest(),
            |p| {
                use sha2::{Digest, Sha256};
                format!(
                    "{:x}",
                    Sha256::digest(format!("{}:{}", self.catalog.digest(), p.digest()))
                )
            },
        );
        let conflicts: Vec<_> = self
            .catalog
            .effective_heads()
            .into_iter()
            // Concurrent preference and order edits resolve to the newest head
            // and the next choice on any device supersedes them; nothing to review.
            .filter(|(key, _)| !matches!(key, Key::Preference { .. } | Key::Order { .. }))
            .filter_map(|(key, heads)| {
                let variants: BTreeSet<_> = heads.iter().map(|o| o.value.to_string()).collect();
                if variants.len() <= 1 {
                    return None;
                }
                let selected = Catalog::selected(&key, &heads).map(|o| o.id.clone());
                Some(json!({"key":key.token(),"field":key,"selected":selected,"variants":heads}))
            })
            .collect();
        let machines: Vec<_> = values.keys().filter_map(|key| {
            let Key::Machine { id,field } = key else { return None; };
            if field != "name" || canonical(id) != *id { return None; }
            Some(json!({"id":id,"name":name(id),"connection":aliases.get(id),"local":id == &self.node_id}))
        }).collect();
        // The shared order first, then projects it does not name yet, by ID.
        let order = values.get(&Key::project_order());
        let mut ordered = Vec::with_capacity(projects.len());
        for id in order.and_then(Value::as_array).into_iter().flatten() {
            if let Some(project) = id.as_str().and_then(|id| projects.remove(id)) {
                ordered.push(project);
            }
        }
        ordered.extend(projects.into_values());
        let ids: Vec<Value> = ordered.iter().map(|p| p["id"].clone()).collect();
        let mut organization =
            json!({"version":2,"default_project":"ungrouped","projects":ordered});
        if order.is_some() {
            organization["project_order"] = Value::Array(ids);
        }
        json!({"schema":1,"node_id":self.node_id,"swarm_id":self.catalog.swarm_id,"digest":digest,"initialized":self.imported,"session_id_memberships":id_state,
            "organization":organization,
            "versions":versions,"conflicts":conflicts,"preferences":self.catalog.preferences(),"machines":machines,"peers":self.peers})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::{catalog::new_id, store::Binding};
    fn store() -> Store {
        let node_id = new_id();
        let catalog = Catalog::default();
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
            twins: Default::default(),
            id_memberships: Some(primary),
        }
    }
    fn org(name: &str) -> Value {
        json!({"projects":[{"id":"project","name":name,"color":"#123456","vivid":false,"folders":[],"sessions":[]}]})
    }
    #[test]
    fn an_observed_id_move_consumes_raw_heads_but_an_old_name_only_move_does_not() {
        let mut store=store();
        store.initialize(&org("One")).unwrap();
        store.twins=super::super::session_ids::Twins::new(&store.catalog.swarm_id);
        for (field,value) in [("alive",json!(true)),("name",json!("Two")),("color",json!("#123456"))] {
            store.catalog.set(&store.node_id,Key::project("two",field),value).unwrap();
        }
        let label="claude/example/one";let id=new_id();
        super::super::session_ids::SESSIONS.with(|s|s.borrow_mut().insert(label.into(),id.clone()));
        let before=store.snapshot(&config());
        let mut desired=before["organization"].clone();
        desired["projects"][0]["sessions"]=json!([format!("local\n{label}")]);
        store.apply(Patch{base:before["organization"].clone(),desired,versions:serde_json::from_value(before["versions"].clone()).unwrap()}).unwrap();
        let before=store.snapshot(&config());
        let mut desired=before["organization"].clone();
        desired["projects"][0]["sessions"]=json!([]);
        desired["projects"][1]["sessions"]=json!([format!("local\n{label}")]);
        store.apply(Patch{base:before["organization"].clone(),desired,versions:serde_json::from_value(before["versions"].clone()).unwrap()}).unwrap();
        let primary=store.id_memberships.as_ref().unwrap();
        assert_eq!(primary.resolve(&store.catalog,&store.node_id,&id).unwrap().unwrap().value,"two");
        store.catalog.set(&store.node_id,Key::Session {machine:store.node_id.clone(),session:label.into()},json!("project")).unwrap();
        let shown=store.snapshot(&config());
        let two=shown["organization"]["projects"].as_array().unwrap().iter().find(|p|p["id"]=="two").unwrap();
        assert_eq!(two["session_ids"],json!([format!("local\n{id}")]));
        store.id_memberships.as_mut().unwrap().set(&store.catalog,&store.node_id,&store.node_id,&id,Value::Null).unwrap();
        let shown=store.snapshot(&config());
        assert!(shown["organization"]["projects"].as_array().unwrap().iter().all(|p|p["session_ids"].as_array().is_none_or(|ids|ids.is_empty())));
        assert_eq!(shown["session_id_memberships"]["records"][0]["project"],Value::Null);
        let mut other=store.id_memberships.clone().unwrap();
        other.set(&store.catalog,&new_id(),&store.node_id,&id,json!("two")).unwrap();
        let primary=store.id_memberships.as_mut().unwrap();
        primary.set(&store.catalog,&store.node_id,&store.node_id,&id,json!("project")).unwrap();
        primary.merge(&store.catalog,&store.catalog.swarm_id,other.missing(&BTreeSet::new())).unwrap();
        let shown=store.snapshot(&config());
        let record=&shown["session_id_memberships"]["records"][0];
        assert_eq!(record["conflict"],true);
        assert_eq!(record["project"],Value::Null);
        assert_eq!(record["variants"].as_array().unwrap().iter().map(|op|op["value"].as_str().unwrap()).collect::<BTreeSet<_>>(),BTreeSet::from(["project","two"]));
    }
    #[test]
    fn patches_use_observed_versions_and_do_not_overwrite_unseen_edits() {
        let mut a = store();
        a.initialize(&org("Initial")).unwrap();
        let versions = a.catalog.versions();
        let mut b = a.clone();
        b.node_id = new_id();
        b.apply(Patch {
            base: org("Initial"),
            desired: org("Peer edit"),
            versions: versions.clone(),
        })
        .unwrap();
        a.catalog
            .merge(
                &b.catalog.swarm_id,
                b.catalog.operations.into_values().collect(),
            )
            .unwrap();
        a.apply(Patch {
            base: org("Initial"),
            desired: org("Local edit"),
            versions,
        })
        .unwrap();
        assert_eq!(a.catalog.heads()[&Key::project("project", "name")].len(), 2);
    }
    #[test]
    fn local_preferences_are_not_exported_and_import_is_idempotent() {
        let mut a = store();
        let mut document = org("Initial");
        document["default_project"] = json!("project");
        document["ssh_private_key"] = json!("SECRET");
        document["projects"][0]["collapsed"] = json!(true);
        a.initialize(&document).unwrap();
        let before = a.catalog.digest();
        a.initialize(&org("Outdated GUI cache")).unwrap();
        assert_eq!(a.catalog.digest(), before);
        let wire = serde_json::to_string(&a.catalog).unwrap();
        for private in ["SECRET", "collapsed", "default_project", "ssh_private_key"] {
            assert!(!wire.contains(private));
        }
    }
    #[test]
    fn a_pending_gui_import_fills_only_missing_fields_and_keeps_causal_heads() {
        let mut a = store();
        a.initialize(&org("Initial")).unwrap();
        let mut b = a.clone();
        b.node_id = new_id();
        a.catalog.set(&a.node_id, Key::project("project", "name"), json!("Local choice"),
            ).unwrap();
        b.catalog.set(&b.node_id, Key::project("project", "name"), json!("Peer choice"),
            ).unwrap();
        a.catalog.merge(&b.catalog.swarm_id, b.catalog.operations.into_values().collect(),
            ).unwrap();
        a.catalog.set(&a.node_id, Key::Folder { id: "retained-deletion".into(),
                }, Value::Null,
            ).unwrap();
        let heads: BTreeMap<_, Vec<String>> = a.catalog.heads().into_iter().map(|(key, ops)| (key, ops.into_iter().map(|op| op.id.clone()).collect())).collect();
        assert_eq!(heads[&Key::project("project", "name")].len(), 2);
        let values = a.catalog.values();
        let mut gui = org("Stale cached name");
        gui["projects"][0]["folders"] = json!([{"id":"retained-deletion","machine":"local","path":"/example/project","name":""}]);
        gui["projects"].as_array_mut().unwrap().push(json!({"id":"extra","name":"Imported later","color":"#654321","sessions":[],"folders":[]}));
        assert!(a.imported, "the CLI bootstrap retains the legacy flag");
        a.import_missing(&gui).unwrap();
        for (key, prior) in heads {
            assert_eq!(a.catalog.heads()[&key].iter().map(|op| op.id.clone()).collect::<Vec<_>>(), prior, "an import must preserve causal heads");
            assert_eq!(a.catalog.values()[&key], values[&key]);
        }
        assert_eq!(a.catalog.values()[&Key::project("extra", "name")], json!("Imported later"));
        let imported = a.catalog.digest();
        a.import_missing(&gui).unwrap();
        assert_eq!(a.catalog.digest(), imported, "a crash before marker removal is idempotent");
        a.initialize(&org("An old caller remains a no-op")).unwrap();
        assert_eq!(a.catalog.digest(), imported);
    }

    #[test]
    fn fallback_markers_never_become_shared_assignments_even_on_import() {
        let mut a = store();
        let mut document = org("Initial");
        document["projects"][0]["sessions"] = json!(["local\ncodex/work/new"]);
        document["unassigned_sessions"] = json!(["local\ncodex/work/new"]);
        a.initialize(&document).unwrap();
        assert!(!a.catalog.values().keys().any(|key| matches!(key, Key::Session { .. })));
        let before = a.snapshot(&config());
        a.apply(Patch { base: before["organization"].clone(), desired: document, versions: a.catalog.versions(),
        }).unwrap();
        assert!(!a.catalog.values().keys().any(|key| matches!(key, Key::Session { .. })));
    }
    #[test]
    fn observing_a_session_does_not_undo_a_remote_project_assignment() {
        let mut a = store();
        a.initialize(&org("Initial")).unwrap();
        let versions = a.catalog.versions();
        let key = Key::Session {
            machine: a.node_id.clone(),
            session: "codex/test/one".into(),
        };
        a.catalog
            .set(&new_id(), key.clone(), json!("another-project"))
            .unwrap();
        let mut desired = org("Initial");
        desired["projects"][0]["sessions"] = json!(["local\ncodex/test/one"]);
        a.apply(Patch {
            base: org("Initial"),
            desired,
            versions,
        })
        .unwrap();
        assert_eq!(a.catalog.values()[&key], "another-project");
    }
    #[test]
    fn explicit_initial_assignment_preserves_an_unseen_competing_choice() {
        let mut a = store();
        a.initialize(&org("Initial")).unwrap();
        let versions = a.catalog.versions();
        let key = Key::Session {
            machine: a.node_id.clone(),
            session: "codex/test/one".into(),
        };
        a.catalog
            .set(&new_id(), key.clone(), json!("another-project"))
            .unwrap();
        let mut desired = org("Initial");
        desired["projects"][0]["sessions"] = json!(["local\ncodex/test/one"]);
        desired["session_assignments_explicit"] = json!(true);
        a.apply(Patch {
            base: org("Initial"),
            desired,
            versions,
        })
        .unwrap();
        let heads = a.catalog.heads();
        assert_eq!(heads[&key].len(), 2);
        let values: BTreeSet<_> = heads[&key]
            .iter()
            .map(|op| op.value.as_str().unwrap())
            .collect();
        assert_eq!(values, BTreeSet::from(["project", "another-project"]));
    }
    /// Each Session key's heads, by operation: a write adds or replaces one.
    fn assignment_heads(store: &Store) -> BTreeMap<Key, Vec<String>> {
        store
            .catalog
            .heads()
            .into_iter()
            .filter(|(key, _)| matches!(key, Key::Session { .. }))
            .map(|(key, heads)| (key, heads.iter().map(|op| op.id.clone()).collect()))
            .collect()
    }
    #[test]
    fn a_legacy_project_import_proposal_keeps_every_session_assignment() {
        // zerus-5wmh.25.7.3: the desktop's actual legacy-project planner omits
        // session assignments from both sides of its one proposal, so Root
        // writes no Session key: every assignment, also one this desktop never
        // saw, keeps its exact heads while the import adds its project and folders.
        use zerus_client_core::paths::Style;
        use zerus_client_core::swarm::legacy_projects::{plan, Binding as Scope};
        let mut a = store();
        let mut organization = org("Initial");
        organization["projects"][0]["sessions"] = json!((0..300)
            .map(|n| format!("local\ncodex/test/{n:04}"))
            .collect::<Vec<_>>());
        organization["projects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"other","name":"Other",
            "color":"#654321","folders":[],"sessions":["local\ncodex/test/other"]}));
        a.initialize(&organization).unwrap();
        let unseen = Key::Session {
            machine: a.node_id.clone(),
            session: "codex/test/unseen".into(),
        };
        let snapshot = a.snapshot(&config());
        a.catalog
            .set(&new_id(), unseen.clone(), json!("other"))
            .unwrap();
        let before = assignment_heads(&a);
        assert_eq!(before.len(), 302);
        let scope = Scope {
            machine: a.node_id.clone(),
            node: a.node_id.clone(),
            swarm: snapshot["swarm_id"].as_str().unwrap().into(),
            source: "a".repeat(64),
        };
        let rows = json!([{"name":"Legacy","dir":"/srv/legacy","src":"local"},
            {"name":"Initial","dir":"/srv/initial","src":"local"}]);
        let proposal = plan(&snapshot, scope, "local", &rows, Style::Posix).unwrap();
        let input = proposal.input();
        a.apply(Patch {
            base: input["base"].clone(),
            desired: input["desired"].clone(),
            versions: serde_json::from_value(input["versions"].clone()).unwrap(),
        })
        .unwrap();
        assert_eq!(assignment_heads(&a), before);
        assert_eq!(a.catalog.values()[&unseen], "other");
        let after = a.snapshot(&config());
        let projects = after["organization"]["projects"].as_array().unwrap();
        let named = |name: &str| projects.iter().find(|p| p["name"] == name).unwrap();
        assert_eq!(named("Initial")["sessions"].as_array().unwrap().len(), 300);
        assert_eq!(named("Initial")["folders"][0]["path"], "/srv/initial");
        assert_eq!(named("Legacy")["folders"][0]["path"], "/srv/legacy");
        assert_eq!(named("Other")["sessions"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn windows_folder_paths_are_drive_or_unc() {
        for path in [
            "C:\\work",
            "c:/",
            "D:\\",
            "C:\\work/sub\\dir",
            "\\\\server\\share",
            "//server/share/dir",
            "\\/server/share",
            "\\\\?\\C:\\work",
        ] {
            assert!(windows_folder(path), "{path}");
        }
        for path in [
            "/work",
            "/",
            "//server",
            "//server/",
            "//server//share",
            "///share",
            "C:",
            "C:work",
            "1:/work",
            "/work/C:/x",
            "work",
            "",
        ] {
            assert!(!windows_folder(path), "{path}");
        }
    }
    #[test]
    fn local_windows_folders_stay_local_and_other_invalid_paths_still_fail() {
        let mut a = store();
        a.initialize(&org("Initial")).unwrap();
        let versions = a.catalog.versions();
        let digest = a.catalog.digest();
        let with = |folder: Value| {
            let mut document = org("Initial");
            document["projects"][0]["folders"] = json!([folder]);
            document
        };
        // This machine by its alias, or by its ID once a snapshot named it.
        let node = a.node_id.clone();
        let local = |path: &str| {
            [
                with(json!({"id":"local-folder","machine":"local","path":path,"name":""})),
                with(
                    json!({"id":"local-folder","machine":"desktop","machine_id":node,"path":path,"name":""}),
                ),
            ]
        };
        for path in ["\\\\server\\share\\dir", "//server/share"] {
            for document in local(path) {
                a.apply(Patch {
                    base: org("Initial"),
                    desired: document.clone(),
                    versions: versions.clone(),
                })
                .unwrap();
                a.apply(Patch {
                    base: document,
                    desired: org("Initial"),
                    versions: versions.clone(),
                })
                .unwrap();
                assert_eq!(a.catalog.digest(), digest, "{path}");
            }
        }
        for document in local("\\\\server\\share") {
            assert!(a
                .flatten(&document)
                .unwrap()
                .keys()
                .all(|key| !matches!(key, Key::Folder { .. })));
        }
        // WhiteWh fork: drive folders of this machine and of a Windows peer are shared.
        let peer = new_id();
        for document in [
            local("C:\\work")[0].clone(),
            local("c:/Users/user/project")[1].clone(),
            with(json!({"id":"local-folder","machine":"laptop","path":"D:/x","name":""})),
            with(json!({"id":"local-folder","machine":"laptop","machine_id":peer,"path":"d:\\work","name":""})),
        ] {
            let path = document["projects"][0]["folders"][0]["path"].clone();
            let versions = a.catalog.versions();
            a.apply(Patch {
                base: org("Initial"),
                desired: document.clone(),
                versions: versions.clone(),
            })
            .unwrap();
            assert_eq!(a.catalog.values()[&Key::Folder { id: "local-folder".into() }]["path"], path);
            a.apply(Patch {
                base: document,
                desired: org("Initial"),
                versions: a.catalog.versions(),
            })
            .unwrap();
        }
        let versions = a.catalog.versions();
        let [work, work_by_id] = local("work");
        let [drive_relative, _] = local("C:work");
        let [rooted, _] = local("\\work");
        for desired in [work, work_by_id, drive_relative, rooted] {
            assert!(
                a.apply(Patch {
                    base: org("Initial"),
                    desired: desired.clone(),
                    versions: versions.clone(),
                })
                .is_err(),
                "{desired}"
            );
        }
        a.apply(Patch {
            base: org("Initial"),
            desired: local("/work")[0].clone(),
            versions,
        })
        .unwrap();
        assert_eq!(
            a.catalog.values()[&Key::Folder {
                id: "local-folder".into()
            }]["path"],
            "/work"
        );
    }
    #[test]
    fn a_peers_unc_shaped_folder_is_a_posix_path() {
        // A Windows GUI can store "//server/share" for a POSIX peer.
        let mut a = store();
        a.initialize(&org("Initial")).unwrap();
        let peer = a.machine_id("laptop").unwrap();
        let key = Key::Folder {
            id: "peer-folder".into(),
        };
        let with = |machine: &str, name: &str| {
            let mut document = org("Initial");
            document["projects"][0]["folders"] = json!([{"id":"peer-folder","machine":machine,"path":"//server/share/dir","name":name}]);
            document
        };
        let mut edit = |base: Value, desired: Value| {
            let versions = a.catalog.versions();
            a.apply(Patch {
                base,
                desired,
                versions,
            })
            .unwrap();
            a.catalog.values().get(&key).cloned().unwrap_or(Value::Null)
        };
        let written = edit(org("Initial"), with("laptop", ""));
        assert_eq!(written["path"], "//server/share/dir");
        assert_eq!(written["machine"], peer.as_str());
        // A snapshot names the peer by its ID; renaming and removing it sync.
        assert_eq!(
            edit(with("laptop", ""), with(&peer, "Share"))["name"],
            "Share"
        );
        assert!(edit(with(&peer, "Share"), org("Initial")).is_null());
        assert_eq!(a.catalog.heads()[&key].len(), 1);
    }
    #[test]
    fn verified_connection_cannot_silently_change_machine_identity() {
        let mut a = store();
        let placeholder = a.machine_id("peer").unwrap();
        let real = new_id();
        a.bind("peer", &real).unwrap();
        assert_eq!(a.catalog.machine(&placeholder), real);
        assert!(a.bind("peer", &new_id()).is_err());
    }
    fn config() -> Config {
        Config {
            home: "/home/example".into(),
            dir: "/home/example/.config/hgs".into(),
            state: "/home/example/.local/state/hgs".into(),
            selves: vec!["local".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        }
    }
    fn projects(ids: &[&str]) -> Value {
        let projects: Vec<_> = ids
            .iter()
            .map(|id| json!({"id":id,"name":id.to_uppercase(),"color":"#123456","folders":[],"sessions":[]}))
            .collect();
        json!({ "projects": projects })
    }
    fn shown(store: &Store) -> (Vec<String>, Option<Value>) {
        let organization = store.snapshot(&config())["organization"].clone();
        let ids = organization["projects"].as_array().unwrap().iter();
        let ids = ids.map(|p| p["id"].as_str().unwrap().to_owned()).collect();
        (ids, organization.get("project_order").cloned())
    }
    #[test]
    fn the_shared_project_order_orders_snapshots_and_unnamed_projects_follow_by_id() {
        let mut a = store();
        a.initialize(&projects(&["b", "c", "a"])).unwrap();
        // Without a shared order the snapshot keeps its ID order and says so.
        assert_eq!(shown(&a), (vec!["a".into(), "b".into(), "c".into()], None));
        let before = a.snapshot(&config());
        let mut desired = projects(&["c", "a", "b"]);
        desired["project_order"] = json!(["c", "a", "b"]);
        a.apply(Patch {
            base: before["organization"].clone(),
            desired,
            versions: a.catalog.versions(),
        })
        .unwrap();
        let order = |ids: &[&str]| {
            (
                ids.iter().map(|id| id.to_string()).collect(),
                Some(json!(ids)),
            )
        };
        assert_eq!(shown(&a), order(&["c", "a", "b"]));
        // A project the order does not name yet follows it; a removed one leaves it.
        a.catalog
            .set(&a.node_id.clone(), Key::project("0", "alive"), json!(true))
            .unwrap();
        a.catalog
            .set(&a.node_id.clone(), Key::project("a", "alive"), json!(false))
            .unwrap();
        assert_eq!(shown(&a), order(&["c", "b", "0"]));
        // Republishing the same order writes nothing.
        let before = a.snapshot(&config());
        let operations = a.catalog.operations.len();
        a.apply(Patch {
            base: before["organization"].clone(),
            desired: before["organization"].clone(),
            versions: a.catalog.versions(),
        })
        .unwrap();
        assert_eq!(a.catalog.operations.len(), operations);
    }
    #[test]
    fn a_desktop_without_the_shared_order_never_changes_it() {
        let mut a = store();
        let mut initial = projects(&["a", "b"]);
        initial["project_order"] = json!(["b", "a"]);
        a.initialize(&initial).unwrap();
        assert_eq!(shown(&a).1, Some(json!(["b", "a"])));
        // An older desktop keeps the snapshot's field in its base and drops it
        // from its desired local order, or omits it from both.
        let before = a.snapshot(&config());
        let base = before["organization"].clone();
        for (base, desired) in [
            (base.clone(), projects(&["a", "b"])),
            (projects(&["b", "a"]), projects(&["a", "b"])),
        ] {
            a.apply(Patch {
                base,
                desired,
                versions: a.catalog.versions(),
            })
            .unwrap();
            assert_eq!(shown(&a).1, Some(json!(["b", "a"])));
        }
        assert_eq!(a.catalog.heads()[&Key::project_order()].len(), 1);
    }
    #[test]
    fn concurrent_project_orders_settle_on_the_newest_without_a_review() {
        let mut a = store();
        a.initialize(&projects(&["a", "b", "c"])).unwrap();
        let mut b = a.clone();
        b.node_id = new_id();
        let base = a.snapshot(&config())["organization"].clone();
        for (store, order) in [(&mut a, ["c", "b", "a"]), (&mut b, ["b", "a", "c"])] {
            let mut desired = projects(&order);
            desired["project_order"] = json!(order);
            store
                .apply(Patch {
                    base: base.clone(),
                    desired,
                    versions: store.catalog.versions(),
                })
                .unwrap();
        }
        let operations: Vec<_> = b.catalog.operations.values().cloned().collect();
        a.catalog.merge(&b.catalog.swarm_id, operations).unwrap();
        let operations: Vec<_> = a.catalog.operations.values().cloned().collect();
        b.catalog.merge(&a.catalog.swarm_id, operations).unwrap();
        assert_eq!(a.catalog.heads()[&Key::project_order()].len(), 2);
        assert_eq!(shown(&a), shown(&b));
        assert_eq!(a.snapshot(&config())["conflicts"], json!([]));
    }
}
