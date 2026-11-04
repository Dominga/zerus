//! What this computer last learned of each member's task replica, kept
//! beside the catalog in `replica-members.json`: the task schema its hello
//! advertised, its replica store and each project's last exchange with it
//! (zerus-cg5h.4 S2b.2 C2). Information only: it grants nothing, and a
//! member's entry goes when its membership does.
use super::store::{replace, LockedStore};
use crate::config::Config;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::PathBuf;

const FILE: &str = "replica-members.json";
/// A file larger than this is damaged and read as empty.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// The verified node this entry belongs to.
    pub node: String,
    /// The highest task schema its last hello advertised; none for an hgs
    /// that does not say, which counts as older.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_schema: Option<i64>,
    /// The identity of its replica store from that hello.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    /// When that hello was answered, in milliseconds.
    #[serde(default)]
    pub seen_at: u64,
    /// Each project's last exchange with it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<String, Exchanged>,
}

/// How one project's last exchange with a member ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchanged {
    /// Both sides' inventories were exchanged in full and nothing was refused.
    pub complete: bool,
    /// When it ended, in milliseconds.
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<Incomplete>,
}

/// Why an exchange did not complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Incomplete {
    /// The member could not be reached, or its answer could not be read.
    Unreachable,
    /// The project here was held by another command, or could not be read.
    Busy,
    /// A document was refused on either side.
    RefusedDocuments,
    /// The pages one sync may take ran out; the next sync continues.
    PageBudget,
    /// The member answered as another computer.
    ChangedIdentity,
}

impl Incomplete {
    pub fn as_str(self) -> &'static str {
        match self {
            Incomplete::Unreachable => "unreachable",
            Incomplete::Busy => "busy",
            Incomplete::RefusedDocuments => "refused_documents",
            Incomplete::PageBudget => "page_budget",
            Incomplete::ChangedIdentity => "changed_identity",
        }
    }
}

/// Now, in milliseconds.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn path(config: &Config) -> PathBuf {
    config.dir.join("swarm").join(FILE)
}

/// Every member's entry, read without the lock. A missing, oversized or
/// damaged file reads as none.
pub fn read(config: &Config) -> BTreeMap<String, Member> {
    let mut bytes = Vec::new();
    let read = fs::File::open(path(config))
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes));
    if read.is_err() || bytes.len() as u64 > MAX_BYTES {
        return BTreeMap::new();
    }
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Changes the entry of the member behind `alias` under the catalog lock,
/// keeping only entries of current members. An entry of another node starts
/// again; projects no longer held or alive here are dropped.
fn update(
    config: &Config,
    alias: &str,
    projects: Option<&BTreeSet<String>>,
    change: impl FnOnce(&mut Member),
) -> Result<()> {
    let store = LockedStore::load(config)?;
    let Some(peer) = store.data.peers.get(alias) else {
        return Ok(());
    };
    let mut members = read(config);
    members.retain(|alias, member| {
        store
            .data
            .peers
            .get(alias)
            .is_some_and(|peer| peer.node == member.node)
    });
    let member = members.entry(alias.to_owned()).or_default();
    if member.node != peer.node {
        *member = Member {
            node: peer.node.clone(),
            ..Member::default()
        };
    }
    change(member);
    if let Some(projects) = projects {
        for member in members.values_mut() {
            member.projects.retain(|project, _| projects.contains(project));
        }
    }
    let bytes = serde_json::to_vec(&members)?;
    replace(&path(config), &bytes)?;
    drop(store);
    Ok(())
}

/// Records the replica part of the hello the member behind `alias`
/// answered, once its identity was verified.
pub fn seen(config: &Config, alias: &str, replica: &Value, at: u64) -> Result<()> {
    update(config, alias, None, |member| {
        member.task_schema = replica["task_schema"].as_i64();
        member.store = replica["store"].as_str().map(str::to_owned);
        member.seen_at = at;
    })
}

/// Records how projects' exchanges with the member behind `alias` ended,
/// in one write; `projects` are those this computer still holds or knows
/// alive. Equal digests are a complete exchange.
pub fn exchanged(
    config: &Config,
    alias: &str,
    outcomes: &[(String, std::result::Result<(), Incomplete>)],
    projects: &BTreeSet<String>,
    at: u64,
) -> Result<()> {
    if outcomes.is_empty() {
        return Ok(());
    }
    update(config, alias, Some(projects), |member| {
        for (project, outcome) in outcomes {
            member.projects.insert(
                project.clone(),
                Exchanged {
                    complete: outcome.is_ok(),
                    at,
                    reason: outcome.err(),
                },
            );
        }
    })
}
