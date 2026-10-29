//! The shared project organization in the Sessions list (`hgs swarm`), on
//! top of the core's `organization` topic (`crate::organization`):
//!
//! - [`member`], [`members`] and [`default_project`]: Qt's membership rules
//!   (tray/src/SessionOrganization.cpp);
//! - [`edit`]: the menus' edits, as the desired side of `swarm apply`;
//! - [`layout`]: this device's collapsed projects, session order and default;
//! - [`grouping`]: each row's project and rank and the headers, sent to the
//!   webview only when they change;
//! - [`sync`]: the write controller, a port of tray/src/SwarmController.cpp;
//! - [`draft`]: its pending edits on disk, so a restart sends them again.
//!
//! Pure Rust: the Tauri hub (client/src-tauri/src/organization.rs) runs
//! `hgs swarm apply` and passes the clock in. Placement itself (concurrent
//! Inbox and default markers against a concrete project, causal heads) is
//! resolved by `hgs swarm apply` and the replica (src/swarm/model.rs:126-201,
//! src/swarm/catalog.rs:369-394), never here.
pub mod draft;
pub mod edit;
pub mod grouping;
pub mod layout;
pub mod runs;
pub mod sync;

use serde_json::Value;
use std::collections::BTreeMap;

/// The Inbox: the project of sessions no project lists, unless this device
/// or the catalog names another (SessionOrganization.cpp:52-58).
pub const INBOX: &str = "ungrouped";
/// The Inbox's name and colour when the catalog lists no project at all
/// (SessionOrganization.cpp:53).
pub const INBOX_NAME: &str = "General";
pub const INBOX_COLOR: &str = "#8d9baa";
/// The most projects the list shows (phase 3a's bound).
pub const MAX_PROJECTS: usize = 500;
/// Shown while project sync has not read the catalog yet.
pub const STARTING: &str = "Project sync is starting.";

/// A session as `organization.projects[].sessions[]` names it: the machine
/// (this computer's host name or the peer's alias, as `ViewRow::machine`)
/// and the session name (SessionOrganization.cpp:225).
pub fn member(machine: &str, name: &str) -> String {
    format!("{machine}\n{name}")
}

/// The archived session's organization identity (SessionOrganization.cpp:225).
pub fn archive_member(machine: &str, archive_id: &str) -> String {
    format!("{machine}\narchive\n{archive_id}")
}

/// The listed projects, in the catalog's order.
pub fn projects(organization: &Value) -> &[Value] {
    organization
        .get("projects")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub fn project_id(project: &Value) -> &str {
    project.get("id").and_then(Value::as_str).unwrap_or("")
}

/// A project's explicit members.
pub fn sessions(project: &Value) -> impl Iterator<Item = &str> {
    project
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

pub fn listed(organization: &Value, id: &str) -> bool {
    projects(organization)
        .iter()
        .any(|project| project_id(project) == id)
}

/// The project of sessions no project lists (SessionOrganization.cpp:55-58):
/// this device's choice when listed, then the catalog's `default_project`
/// when listed, then the first project, then the Inbox.
pub fn default_project(organization: &Value, local: Option<&str>) -> String {
    if let Some(id) = local.filter(|id| listed(organization, id)) {
        return id.to_owned();
    }
    let catalog = organization
        .get("default_project")
        .and_then(Value::as_str)
        .unwrap_or(INBOX);
    if listed(organization, catalog) {
        return catalog.to_owned();
    }
    projects(organization)
        .iter()
        .map(project_id)
        .find(|id| !id.is_empty())
        .unwrap_or(INBOX)
        .to_owned()
}

/// Every explicit member's project. A member listed by two projects stays in
/// the first (indexSessions, SessionOrganization.cpp:98-104); `swarm apply`
/// refuses such a catalog (src/swarm/model.rs:107-110), so a valid snapshot
/// lists each member once.
pub fn members(organization: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for project in projects(organization) {
        for member in sessions(project) {
            out.entry(member.to_owned())
                .or_insert_with(|| project_id(project).to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn organization() -> Value {
        json!({"default_project": "ungrouped", "projects": [
            {"id": "alpha", "name": "Alpha", "sessions": ["workstation\nclaude/sample-project/review", "studio\ncodex/aurora/orders-v3"]},
            {"id": "beta", "name": "Beta", "sessions": ["workstation\nclaude/sample-project/review"]},
            {"id": "ungrouped", "name": "Inbox", "sessions": []}]})
    }

    #[test]
    fn a_member_is_the_machine_and_the_session_name() {
        assert_eq!(
            member("workstation", "claude/sample-project/review"),
            "workstation\nclaude/sample-project/review"
        );
    }

    #[test]
    fn the_first_project_that_lists_a_member_keeps_it() {
        let members = members(&organization());
        assert_eq!(
            members["workstation\nclaude/sample-project/review"],
            "alpha"
        );
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn the_default_project_follows_qt() {
        let org = organization();
        assert_eq!(default_project(&org, None), "ungrouped");
        assert_eq!(default_project(&org, Some("beta")), "beta");
        assert_eq!(default_project(&org, Some("gone")), "ungrouped");
        let without =
            json!({"default_project": "ungrouped", "projects": [{"id": "alpha"}, {"id": "beta"}]});
        assert_eq!(default_project(&without, None), "alpha");
        assert_eq!(default_project(&json!({"projects": []}), None), INBOX);
        assert_eq!(default_project(&json!(null), None), INBOX);
    }
}
