//! The Projects screen from `hgs swarm get`: the shared organization, its
//! order, folders and default project, with SwarmController's status line
//! (SwarmController.cpp:191-219) and ProjectsDialog's unconnected-computer
//! notice (ProjectsDialog.cpp:251-263).
use super::schedule::{Poll, SWARM_INTERVAL};
use super::{items, text};
use crate::swarm::sync::{unavailable, OUTDATED};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

/// The most projects, and folders per project, the screen lists.
const PROJECTS: usize = 500;
const FOLDERS: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Folder {
    pub id: String,
    /// `name`, else the path's last part (ProjectsDialog.cpp:300).
    pub name: String,
    pub path: String,
    /// The Computer column: `machine_name`, else `machine` (ProjectsDialog.cpp:303).
    pub computer: String,
    pub machine: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// The project's colour (hex); None uses the palette slot.
    pub color: Option<String>,
    /// False for a project on a computer without a configured connection ("All swarm projects").
    pub accessible: bool,
    pub is_default: bool,
    /// The palette slot by list position, until colour preferences arrive (3c).
    pub palette: usize,
    pub folders: Vec<Folder>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProjectsView {
    pub loaded: bool,
    pub refreshing: bool,
    pub status: String,
    pub status_error: bool,
    pub conflicts: usize,
    pub unconnected: Vec<String>,
    pub notice: Option<String>,
    pub projects: Vec<Project>,
}

/// The shared catalog's reads so far.
#[derive(Clone, Debug)]
pub struct SwarmState {
    pub poll: Poll,
    valid: Option<Value>,
    status: Option<(String, bool)>,
}

impl Default for SwarmState {
    fn default() -> Self {
        Self {
            poll: Poll::new(SWARM_INTERVAL),
            valid: None,
            status: None,
        }
    }
}

/// The last part of a path, with either separator.
pub fn leaf(path: &str) -> String {
    path.split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}

/// SwarmController's status line for a catalog (SwarmController.cpp:212-218),
/// shared with the write controller ([`crate::swarm::sync`]).
pub(crate) fn status_of(snapshot: &Value) -> (String, bool) {
    let conflicts = items(snapshot, "conflicts").len();
    let peers = snapshot.get("peers").and_then(Value::as_object);
    let total = peers.map_or(0, |peers| peers.len());
    let offline = peers.map_or(0, |peers| {
        peers
            .values()
            .filter(|peer| !text(peer, "error").is_empty())
            .count()
    });
    if conflicts > 0 {
        (
            format!("{conflicts} project conflict(s) need review in Swarm."),
            true,
        )
    } else if offline > 0 {
        (format!("{offline} connection(s) unavailable. Changes are saved here and will sync when reachable."), true)
    } else if total == 0 {
        (
            "Local catalog. Connect another computer to sync projects.".into(),
            false,
        )
    } else {
        (
            format!("Project sync connected to {total} computer(s)"),
            false,
        )
    }
}

fn folder(value: &Value) -> Folder {
    let path = text(value, "path");
    let name = text(value, "name");
    let machine = text(value, "machine");
    let machine_name = text(value, "machine_name");
    Folder {
        id: text(value, "id"),
        name: if name.is_empty() { leaf(&path) } else { name },
        computer: if machine_name.is_empty() {
            machine.clone()
        } else {
            machine_name
        },
        machine,
        path,
    }
}

impl SwarmState {
    /// Files a `swarm get` reply; anything but a schema 1 catalog keeps the last good one.
    pub fn finish(&mut self, reply: Result<Value, String>) {
        self.poll.finish();
        match reply {
            Err(error) => self.status = Some((unavailable(&error), true)),
            Ok(value)
                if value.get("schema").and_then(Value::as_i64) != Some(1)
                    || !value.get("organization").is_some_and(Value::is_object) =>
            {
                self.status = Some((OUTDATED.into(), true));
            }
            Ok(value) => {
                self.status = Some(status_of(&value));
                self.valid = Some(value);
            }
        }
    }

    /// The screen; `peers` are the fleet's peer aliases.
    pub fn view(&self, peers: &[String]) -> ProjectsView {
        let (status, status_error) = self.status.clone().unwrap_or_default();
        let Some(snapshot) = &self.valid else {
            return ProjectsView {
                refreshing: self.poll.pending(),
                status,
                status_error,
                ..ProjectsView::default()
            };
        };
        let organization = &snapshot["organization"];
        let default = text(organization, "default_project");
        let listed: Vec<&Value> = items(organization, "projects")
            .iter()
            .take(PROJECTS)
            .collect();
        // The shared order first (SwarmController::presentation), then the rest as listed.
        let mut seen = BTreeSet::new();
        let mut ordered: Vec<&Value> = Vec::new();
        for id in items(organization, "project_order")
            .iter()
            .filter_map(Value::as_str)
        {
            if let Some(project) = listed.iter().find(|p| text(p, "id") == id) {
                if seen.insert(id.to_owned()) {
                    ordered.push(project);
                }
            }
        }
        for project in &listed {
            if seen.insert(text(project, "id")) {
                ordered.push(project);
            }
        }
        let projects = ordered
            .iter()
            .enumerate()
            .map(|(index, p)| Project {
                id: text(p, "id"),
                name: text(p, "name"),
                color: Some(text(p, "color")).filter(|color| !color.is_empty()),
                accessible: p.get("accessible").and_then(Value::as_bool).unwrap_or(true),
                is_default: text(p, "id") == default,
                palette: index % 8,
                folders: items(p, "folders")
                    .iter()
                    .take(FOLDERS)
                    .map(folder)
                    .collect(),
            })
            .collect();
        // A configured SSH connection permits session access; it does not join
        // independent project catalogs. Only the swarm snapshot proves that.
        let known: BTreeSet<&str> = snapshot
            .get("peers")
            .and_then(Value::as_object)
            .map(|peers| peers.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let unconnected: Vec<String> = if text(snapshot, "node_id").is_empty() {
            Vec::new()
        } else {
            peers
                .iter()
                .filter(|peer| !known.contains(peer.as_str()))
                .cloned()
                .collect()
        };
        let notice = (!unconnected.is_empty()).then(|| {
            format!(
                "Session connections for {} do not share project catalogs yet. Preview the other catalog, match projects, then explicitly join to share their grouping.",
                unconnected.join(", ")
            )
        });
        ProjectsView {
            loaded: true,
            refreshing: self.poll.pending(),
            status,
            status_error,
            conflicts: items(snapshot, "conflicts").len(),
            unconnected,
            notice,
            projects,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::fixture;
    use serde_json::json;

    fn peers() -> Vec<String> {
        vec!["studio".into(), "build-01".into()]
    }

    #[test]
    fn the_catalog_keeps_the_shared_order_folders_and_unconnected_peers() {
        let mut swarm = SwarmState::default();
        assert!(!swarm.view(&peers()).loaded);
        swarm.finish(Ok(fixture("swarm.json")));
        let view = swarm.view(&peers());
        assert!(view.loaded);
        let ids: Vec<&str> = view.projects.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["sample", "ungrouped", "remote-only"]);
        assert!(view.projects[1].is_default && !view.projects[0].is_default);
        assert!(!view.projects[2].accessible);
        assert_eq!(view.projects[0].color.as_deref(), Some("#3fb950"));
        assert_eq!(view.projects[1].color, None);
        assert_eq!(view.projects[1].palette, 1);
        let folders: Vec<(&str, &str)> = view.projects[0]
            .folders
            .iter()
            .map(|f| (f.name.as_str(), f.computer.as_str()))
            .collect();
        assert_eq!(
            folders,
            [("sample-project", "workstation"), ("Docs", "Windows lab")]
        );
        assert_eq!(view.unconnected, ["build-01"]);
        assert_eq!(
            view.notice.as_deref(),
            Some("Session connections for build-01 do not share project catalogs yet. Preview the other catalog, match projects, then explicitly join to share their grouping.")
        );
        assert_eq!(
            (view.status.as_str(), view.status_error, view.conflicts),
            ("1 project conflict(s) need review in Swarm.", true, 1)
        );
        assert_eq!(leaf("C:\\Users\\example\\docs"), "docs");
        assert_eq!(leaf("/"), "/");
    }

    #[test]
    fn the_status_line_follows_qt_precedence() {
        let status = |change: &dyn Fn(&mut Value)| {
            let mut snapshot = fixture("swarm.json");
            change(&mut snapshot);
            let mut swarm = SwarmState::default();
            swarm.finish(Ok(snapshot));
            let view = swarm.view(&[]);
            (view.status, view.status_error)
        };
        assert_eq!(
            status(&|s: &mut Value| s["conflicts"] = json!([])),
            (
                "1 connection(s) unavailable. Changes are saved here and will sync when reachable."
                    .to_owned(),
                true
            )
        );
        assert_eq!(
            status(&|s: &mut Value| {
                s["conflicts"] = json!([]);
                s["peers"]["mac"] = json!({"node": "node-3"});
            }),
            ("Project sync connected to 2 computer(s)".to_owned(), false)
        );
        assert_eq!(
            status(&|s: &mut Value| {
                s["conflicts"] = json!([]);
                s["peers"] = json!({});
            }),
            (
                "Local catalog. Connect another computer to sync projects.".to_owned(),
                false
            )
        );
        let unknown = status(&|s: &mut Value| {
            s["node_id"] = json!("");
        });
        assert_eq!(unknown.0, "1 project conflict(s) need review in Swarm.");
    }

    #[test]
    fn an_older_hgs_or_a_failed_read_keeps_the_last_catalog() {
        let mut swarm = SwarmState::default();
        swarm.finish(Ok(fixture("swarm-old.json")));
        let view = swarm.view(&peers());
        assert_eq!(
            (view.loaded, view.status.as_str(), view.status_error),
            (
                false,
                "Project sync needs an updated HGS installation.",
                true
            )
        );
        swarm.finish(Ok(fixture("swarm.json")));
        swarm.finish(Ok(json!({"schema": 1, "machines": [], "digest": "d2"})));
        let view = swarm.view(&peers());
        assert_eq!(
            view.status,
            "Project sync needs an updated HGS installation."
        );
        assert_eq!(view.projects.len(), 3, "the last good catalog stays");
        swarm.finish(Err("hgs did not answer in time".into()));
        let view = swarm.view(&peers());
        assert_eq!(
            (view.status.as_str(), view.status_error),
            ("Project sync unavailable: hgs did not answer in time", true)
        );
        assert_eq!(view.projects.len(), 3);
    }
}
