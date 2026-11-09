//! Windows: when Zerus starts, it starts the connector the person last left
//! running. An actual successful Start connector, or a verified running
//! connector of that workspace, arms its node; Stop disarms it at admission and
//! remembers the generation it stopped, so neither that draining generation nor
//! a stale reply can re-arm it. An upgrade therefore never switches on a phone
//! gateway nobody asked for, and a different workspace inherits nothing.
//!
//! The choice lives in this private native record, never in the settings the
//! page can write, so nothing the renderer sends can arm a connector.
use super::{
    connector_actions::{self, Kind},
    enrollment::Target,
    Workspace,
};
use serde::{Deserialize, Serialize};

/// The record's name in the app's own data folder.
pub const FILE: &str = "mobile-connector-autostart.json";
/// A longer record is not one this code wrote.
pub const MAX_RECORD: usize = 1024;
const VERSION: u32 = 1;

/// What Settings > Mobile connection says about automatic start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Line {
    Running,
    /// Running, but the person chose Stop for this generation.
    RunningNotArmed,
    Starting,
    /// Armed and stopped: the next time Zerus starts, it starts the connector.
    WillStart,
    /// Never started here, so nothing has asked for it yet.
    NotArmed,
    StoppedByYou,
    /// Armed, but this Zerus has no packaged connector to start.
    Unavailable,
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    node_id: String,
    armed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stopped_generation: Option<String>,
}

fn generation_shape(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Record {
    fn parse(bytes: &[u8]) -> Option<Record> {
        if bytes.len() > MAX_RECORD {
            return None;
        }
        let record: Record = serde_json::from_slice(bytes).ok()?;
        (record.version == VERSION
            && !record.node_id.is_empty()
            && record.node_id.len() <= 128
            && !(record.armed && record.stopped_generation.is_some())
            && record
                .stopped_generation
                .as_deref()
                .is_none_or(generation_shape))
        .then_some(record)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Attempt {
    #[default]
    None,
    Starting,
    Unavailable,
    Failed(String),
}

/// The saved choice and this GUI start's one attempt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    record: Option<Record>,
    unsaved: bool,
    save_problem: bool,
    attempt: Attempt,
    /// The last admitted Stop's disarm and what it replaced.
    stop: Option<AdmittedStop>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AdmittedStop {
    disarm: Record,
    before: Option<Record>,
}

impl State {
    /// `None`, unreadable and malformed bytes all count as unarmed.
    pub fn load(bytes: Option<&[u8]>) -> State {
        State {
            record: bytes.and_then(Record::parse),
            ..State::default()
        }
    }

    fn own(&self, node: &str) -> Option<&Record> {
        self.record.as_ref().filter(|record| record.node_id == node)
    }

    fn store(&mut self, record: Record) {
        if self.record.as_ref() != Some(&record) {
            self.record = Some(record);
            self.unsaved = true;
        }
    }

    fn arm(&mut self, node: &str) {
        self.store(Record {
            version: VERSION,
            node_id: node.to_owned(),
            armed: true,
            stopped_generation: None,
        });
        self.attempt = Attempt::None;
    }

    fn verified_running(workspace: &Workspace) -> bool {
        workspace.encrypted
            && workspace.state == "active"
            && workspace.connector_state == "running"
            && !workspace.node_id.is_empty()
            && workspace.generation.is_some()
            && Target::capture(workspace).is_ok()
    }

    /// A native status read answered. Only a verified running connector changes
    /// anything, and never the generation the person stopped.
    pub fn observe(&mut self, workspace: &Workspace) {
        if !Self::verified_running(workspace) {
            return;
        }
        let stopped_here = self.own(&workspace.node_id).is_some_and(|record| {
            !record.armed && record.stopped_generation == workspace.generation
        });
        if !stopped_here {
            self.arm(&workspace.node_id);
        }
    }

    /// The person's Start connector succeeded with this status.
    pub fn person_started(&mut self, workspace: &Workspace) {
        if Self::verified_running(workspace) {
            self.arm(&workspace.node_id);
        }
    }

    /// The person chose a Stop for this running generation, at its admission.
    pub fn stop_chosen(&mut self, node: &str, generation: Option<&str>) {
        let disarm = Record {
            version: VERSION,
            node_id: node.to_owned(),
            armed: false,
            stopped_generation: generation.map(str::to_owned),
        };
        self.stop = Some(AdmittedStop {
            disarm: disarm.clone(),
            before: self.record.clone(),
        });
        self.store(disarm);
    }

    /// The connector positively refused this exact Stop before offering it, so the
    /// choice goes back to what it was, but only while that disarm is still the
    /// current record: a later Stop, workspace change or arm always stands. An
    /// unknown outcome never comes here and stays disarmed, as a crash would.
    pub fn stop_refused(&mut self, node: &str, generation: Option<&str>) {
        let current = self.stop.as_ref().is_some_and(|stop| {
            stop.disarm.node_id == node
                && stop.disarm.stopped_generation.as_deref() == generation
                && self.record.as_ref() == Some(&stop.disarm)
        });
        if !current {
            return;
        }
        match self.stop.take().and_then(|stop| stop.before) {
            Some(record) => self.store(record),
            // There was no choice, so a refusal must not make one: this reads as
            // never started, and only a Start or a fresh running read can arm it.
            None => self.store(Record {
                version: VERSION,
                node_id: node.to_owned(),
                armed: false,
                stopped_generation: None,
            }),
        }
    }

    /// What to start, when this workspace is armed and can be started now.
    pub fn target(&self, workspace: &Workspace) -> Option<Target> {
        self.own(&workspace.node_id)
            .is_some_and(|record| record.armed)
            .then(|| connector_actions::admit(workspace, Kind::Start).ok())
            .flatten()
    }

    pub fn started(&mut self) {
        self.attempt = Attempt::Starting;
    }

    pub fn unavailable(&mut self) {
        self.attempt = Attempt::Unavailable;
    }

    /// The attempt ended; a connector seen running since has already cleared it.
    pub fn finished(&mut self, failure: Option<String>) {
        if self.attempt == Attempt::Starting {
            self.attempt = failure.map_or(Attempt::None, Attempt::Failed);
        }
    }

    /// The record to save, once.
    pub fn take_unsaved(&mut self) -> Option<Vec<u8>> {
        if !std::mem::take(&mut self.unsaved) {
            return None;
        }
        let bytes = self
            .record
            .as_ref()
            .and_then(|record| serde_json::to_vec(record).ok());
        if bytes.is_none() {
            self.unsaved = true;
            self.save_problem = true;
        }
        bytes
    }

    pub fn save_failed(&mut self) {
        self.unsaved = true;
        self.save_problem = true;
    }

    pub fn save_succeeded(&mut self) {
        self.save_problem = false;
    }

    /// The last save failed, so the next start may not honor this choice.
    pub fn save_problem(&self) -> bool {
        self.save_problem
    }

    pub fn line(&self, workspace: &Workspace) -> Option<Line> {
        if !workspace.encrypted || workspace.state != "active" || workspace.pending.is_some() {
            return None;
        }
        let record = self.own(&workspace.node_id);
        let armed = record.is_some_and(|record| record.armed);
        match workspace.connector_state.as_str() {
            "running" if armed => Some(Line::Running),
            "running" => Some(Line::RunningNotArmed),
            "stopped" => Some(match record {
                None => Line::NotArmed,
                Some(record) if !armed && record.stopped_generation.is_none() => Line::NotArmed,
                Some(_) if !armed => Line::StoppedByYou,
                Some(_) => match &self.attempt {
                    Attempt::None => Line::WillStart,
                    Attempt::Starting => Line::Starting,
                    Attempt::Unavailable => Line::Unavailable,
                    Attempt::Failed(reason) => Line::Failed {
                        reason: reason.clone(),
                    },
                },
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
