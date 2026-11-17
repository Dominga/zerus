//! Product catalog writers for provider inspections; no invented result rows.
use super::*;
pub(crate) fn seed(config: &Config, machine: &str, project: &str, root: &std::path::Path) {
    fixture_project(config, machine, project);
    let mut store = LockedStore::load_bootstrap(config).unwrap();
    store
        .data
        .catalog
        .set(
            machine,
            catalog::Key::Folder {
                id: "provider-fixture".into(),
            },
            json!({"project":project,"machine":machine,"path":root,"name":"Example checkout"}),
        )
        .unwrap();
    store.save().unwrap();
}

pub(crate) fn change_home(config: &Config, machine: &str) {
    let mut store = LockedStore::load_bootstrap(config).unwrap();
    store.data.node_id = machine.into();
    store.save().unwrap();
}
