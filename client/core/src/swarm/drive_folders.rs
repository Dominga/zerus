//! Paired native observation and immutable-request drive write controller.
use crate::{creation::canonical_uuid, swarm::draft, Invocation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const READ_GAP: Duration = Duration::from_secs(5);
const UNKNOWN_GAP: Duration = Duration::from_secs(1);
const MAX_DRAFT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveRecord {
    pub id: Option<String>,
    pub project: String,
    pub machine: String,
    pub path: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineProof {
    pub node: String,
    pub platform: String,
    pub verified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveSetRequest {
    pub original: Option<DriveRecord>,
    pub desired: DriveRecord,
    pub versions: BTreeMap<String, Vec<String>>,
    pub request_id: String,
    pub store_nonce: Option<String>,
    pub source: Option<MachineProof>,
    pub destination: MachineProof,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveObservation {
    pub node_id: String,
    pub swarm_id: String,
    pub store_nonce: Option<String>,
    pub status: String,
    pub records: BTreeMap<String, DriveRecord>,
    pub versions: BTreeMap<String, Vec<String>>,
    pub conflicts: BTreeSet<String>,
    pub unavailable: BTreeSet<String>,
    pub complete: bool,
    pub operation_id: Option<String>,
    pub operation_found: bool,
}

impl DriveObservation {
    pub fn from_reply(value: &Value) -> Result<Self, String> {
        let status = value["status"]
            .as_str()
            .ok_or("missing drive status")?
            .to_owned();
        if !matches!(
            status.as_str(),
            "ready" | "incomplete" | "uninitialized" | "unavailable"
        ) {
            return Err("unsupported drive status".into());
        }
        let node_id = value["node_id"]
            .as_str()
            .ok_or("missing drive node")?
            .to_owned();
        let swarm_id = value["swarm_id"]
            .as_str()
            .ok_or("missing drive swarm")?
            .to_owned();
        if !canonical_uuid(&node_id) || !canonical_uuid(&swarm_id) {
            return Err("invalid drive source identity".into());
        }
        let store_nonce = value["store_nonce"].as_str().map(str::to_owned);
        if store_nonce.as_deref().is_some_and(|id| !canonical_uuid(id)) {
            return Err("invalid drive store identity".into());
        }
        let raw_records: Vec<Value> = match &value["records"] {
            Value::Array(records) => records.clone(),
            Value::Object(records) => records.values().cloned().collect(),
            _ => return Err("invalid drive records".into()),
        };
        if raw_records.len() > 200 {
            return Err("too many drive records".into());
        }
        let mut records = BTreeMap::new();
        for record in raw_records {
            let record: DriveRecord =
                serde_json::from_value(record).map_err(|_| "invalid drive record")?;
            let id = record
                .id
                .clone()
                .filter(|id| canonical_uuid(id))
                .ok_or("invalid drive folder identity")?;
            if !canonical_uuid(&record.machine)
                || record.project.is_empty()
                || record.project.len() > 128
                || record.name.len() > 320
                || record.path.len() > 8192
                || [&record.project, &record.name, &record.path]
                    .into_iter()
                    .any(|text| text.chars().any(char::is_control))
                || record.path.starts_with('/')
                || !crate::paths::absolute(&record.path, crate::paths::Style::Windows)
                || crate::paths::clean(&record.path, crate::paths::Style::Windows) != record.path
            {
                return Err("invalid drive folder fields".into());
            }
            if records.insert(id, record).is_some() {
                return Err("duplicate drive folder identity".into());
            }
        }
        let mut versions = BTreeMap::new();
        let raw_versions = value["versions"]
            .as_object()
            .ok_or("invalid drive versions")?;
        if raw_versions.len() > 200 {
            return Err("too many drive frontiers".into());
        }
        for (token, heads) in raw_versions {
            let heads: Vec<String> =
                serde_json::from_value(heads.clone()).map_err(|_| "invalid drive heads")?;
            if heads.is_empty()
                || heads.len() > 1024
                || heads.iter().any(|id| !canonical_uuid(id))
                || heads.iter().collect::<BTreeSet<_>>().len() != heads.len()
            {
                return Err("invalid drive head".into());
            }
            let Some(id) = token.strip_prefix("drive_folders_v1:") else {
                return Err("invalid drive version identity".into());
            };
            if !canonical_uuid(id) {
                return Err("invalid drive version folder".into());
            }
            versions.insert(token.clone(), heads);
        }
        let conflicts = identities(&value["conflicts"], "conflict")?;
        let unavailable = identities(&value["unavailable"], "unavailable")?;
        if conflicts.len() + unavailable.len() > 400 {
            return Err("too many drive review states".into());
        }
        let complete = value["complete"]
            .as_bool()
            .ok_or("missing drive completeness")?;
        let operation_id = value["operation_id"].as_str().map(str::to_owned);
        if operation_id
            .as_deref()
            .is_some_and(|id| !canonical_uuid(id))
        {
            return Err("invalid drive operation identity".into());
        }
        let operation_found = value["operation_found"]
            .as_bool()
            .ok_or("missing drive operation status")?;
        if operation_found && operation_id.is_none() {
            return Err("drive operation status has no identity".into());
        }
        if operation_found && !matches!(status.as_str(), "ready" | "incomplete") {
            return Err("an unavailable drive read cannot prove an operation".into());
        }
        match status.as_str() {
            "ready" if !complete => return Err("a ready drive read is incomplete".into()),
            "incomplete" if complete => {
                return Err("an incomplete drive read is marked complete".into())
            }
            "uninitialized" | "unavailable" if complete => {
                return Err("an unavailable drive read is complete".into())
            }
            _ => {}
        }
        if matches!(status.as_str(), "ready" | "incomplete") && store_nonce.is_none() {
            return Err("a ready drive read has no store identity".into());
        }
        if status != "ready"
            && status != "incomplete"
            && (!records.is_empty() || !versions.is_empty() || store_nonce.is_some())
        {
            return Err("an unavailable drive read is not empty authority".into());
        }
        Ok(Self {
            node_id,
            swarm_id,
            store_nonce,
            status,
            records,
            versions,
            conflicts,
            unavailable,
            complete,
            operation_id,
            operation_found,
        })
    }

    /// A reply may prove initialization of the pending virgin store, never a
    /// transition back to virgin or from one initialized store to another.
    pub fn same_source(&self, other: &Self) -> bool {
        self.node_id == other.node_id
            && self.swarm_id == other.swarm_id
            && (self.store_nonce == other.store_nonce
                || (other.store_nonce.is_none() && self.store_nonce.is_some()))
    }

    pub fn source_matches(&self, node_id: &str, swarm_id: &str) -> bool {
        self.node_id == node_id && self.swarm_id == swarm_id
    }

    pub fn contains_operation(&self, request_id: &str) -> bool {
        self.operation_found && self.operation_id.as_deref() == Some(request_id)
    }
}

fn identities(value: &Value, name: &str) -> Result<BTreeSet<String>, String> {
    let values: Vec<String> = match value {
        Value::Object(values) => values.keys().cloned().collect(),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("invalid drive {name} identity"))
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(format!("invalid drive {name} identities")),
    };
    if values.iter().any(|id| !canonical_uuid(id)) {
        return Err(format!("invalid drive {name} identity"));
    }
    Ok(values.into_iter().collect())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveDraft {
    pub version: u32,
    pub request: DriveSetRequest,
    pub observation: DriveObservation,
}

impl DriveDraft {
    pub fn store(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_DRAFT_BYTES {
            return Err("drive draft is too large".into());
        }
        draft::write(path, &bytes).map_err(|error| error.to_string())
    }

    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        if bytes.len() as u64 > MAX_DRAFT_BYTES {
            return Err("drive draft is too large".into());
        }
        let draft: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if draft.version != 1 || !canonical_uuid(&draft.request.request_id) {
            return Err("invalid drive draft".into());
        }
        Ok(Some(draft))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DriveSubmission {
    pub request: DriveSetRequest,
    pub invocation: Invocation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DriveReply {
    Ready(DriveObservation),
    Unknown(Invocation),
}

#[derive(Clone, Debug, Default)]
pub struct DriveController {
    observation: Option<DriveObservation>,
    pending: Option<DriveDraft>,
    draft_path: PathBuf,
    unavailable: bool,
    last_poll: Option<Instant>,
    created: Option<Instant>,
    reconcile_until: Option<Instant>,
}

impl DriveController {
    pub fn new(path: PathBuf, allowed: bool) -> Self {
        let mut controller = Self {
            draft_path: path,
            created: Some(Instant::now()),
            ..Self::default()
        };
        if allowed {
            match DriveDraft::load(&controller.draft_path) {
                Ok(draft) => controller.pending = draft,
                Err(_) => controller.unavailable = true,
            }
        }
        controller
    }

    pub fn observation(&self) -> Option<&DriveObservation> {
        self.observation.as_ref()
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn status(&self) -> Option<String> {
        if self.unavailable {
            return Some("The pending drive-folder request needs manual review.".into());
        }
        if self.pending.is_some() {
            return Some("The original drive-folder result is still unknown.".into());
        }
        let observation = self.observation.as_ref()?;
        match observation.status.as_str() {
            "ready" if observation.complete => None,
            "ready" | "incomplete" => Some("Drive folders are incomplete or need review.".into()),
            "uninitialized" => None,
            _ => Some("Drive folders are unavailable; their original files were retained.".into()),
        }
    }

    pub fn refresh(&self) -> Invocation {
        self.invocation(
            vec![
                "swarm".into(),
                "drive-folders".into(),
                "get".into(),
                "--json".into(),
            ],
            None,
        )
    }

    fn status_invocation(&self, request_id: &str) -> Invocation {
        self.invocation(
            vec![
                "swarm".into(),
                "drive-folders".into(),
                "get".into(),
                "--operation".into(),
                request_id.into(),
                "--json".into(),
            ],
            None,
        )
    }

    fn invocation(&self, args: Vec<String>, stdin: Option<Vec<u8>>) -> Invocation {
        Invocation {
            args,
            stdin,
            timeout: READ_TIMEOUT,
            request_id: None,
        }
    }

    pub fn poll(&mut self, now: Instant) -> Option<Invocation> {
        if self.unavailable || self.reconcile_until.is_some_and(|until| now < until) {
            return None;
        }
        let gap = if self.pending.is_some() {
            UNKNOWN_GAP
        } else {
            READ_GAP
        };
        if now.duration_since(self.last_poll.or(self.created).unwrap_or(now)) < gap {
            return None;
        }
        self.last_poll = Some(now);
        Some(match &self.pending {
            Some(pending) => self.status_invocation(&pending.request.request_id),
            None => self.refresh(),
        })
    }

    pub fn observe(&mut self, observation: DriveObservation) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("an ordinary read cannot settle an outstanding drive request".into());
        }
        self.observation = Some(observation);
        Ok(())
    }

    pub fn begin(&mut self, request: DriveSetRequest) -> Result<DriveSubmission, String> {
        if self.unavailable || self.pending.is_some() {
            return Err("an earlier drive request is still unresolved".into());
        }
        if !canonical_uuid(&request.request_id)
            || !canonical_uuid(request.desired.id.as_deref().unwrap_or_default())
        {
            return Err("invalid drive request identity".into());
        }
        let observation = self
            .observation
            .clone()
            .ok_or("drive observation is unavailable")?;
        let initializing = observation.status == "uninitialized"
            && request.original.is_none()
            && request.store_nonce.is_none();
        if !initializing && (observation.status != "ready" || !observation.complete) {
            return Err("drive observation is not complete".into());
        }
        if request.store_nonce != observation.store_nonce {
            return Err("drive store changed".into());
        }
        if request.original.is_none() {
            let key = format!(
                "drive_folders_v1:{}",
                request.desired.id.as_deref().unwrap_or_default()
            );
            if observation.versions.contains_key(&key) || !request.versions.is_empty() {
                return Err("drive folder identity is already used".into());
            }
        }
        let draft = DriveDraft {
            version: 1,
            request: request.clone(),
            observation,
        };
        draft.store(&self.draft_path)?;
        self.pending = Some(draft.clone());
        self.reconcile_until = Some(Instant::now() + READ_TIMEOUT);
        let invocation = self.invocation(
            vec![
                "swarm".into(),
                "drive-folders".into(),
                "apply".into(),
                "--json".into(),
            ],
            Some(serde_json::to_vec(&request).map_err(|error| error.to_string())?),
        );
        Ok(DriveSubmission {
            request,
            invocation,
        })
    }

    fn finish(&mut self, observation: DriveObservation) -> Result<DriveObservation, String> {
        draft::store(&self.draft_path, &draft::DraftWrite::Remove)
            .map_err(|error| error.to_string())?;
        self.pending = None;
        self.reconcile_until = None;
        self.observation = Some(observation.clone());
        Ok(observation)
    }

    fn unknown(&mut self, request_id: &str) -> DriveReply {
        self.reconcile_until = Some(Instant::now() + READ_TIMEOUT);
        DriveReply::Unknown(self.status_invocation(request_id))
    }

    pub fn acknowledge(&mut self, result: Result<Value, String>) -> Result<DriveReply, String> {
        let pending = self.pending.clone().ok_or("no outstanding drive request")?;
        let value = match result {
            Ok(value) => value,
            Err(_) => return Ok(self.unknown(&pending.request.request_id)),
        };
        if value["operation_id"].as_str() != Some(pending.request.request_id.as_str()) {
            return Ok(self.unknown(&pending.request.request_id));
        }
        let observation = DriveObservation::from_reply(&value["observation"])?;
        if !observation.same_source(&pending.observation)
            || !observation.contains_operation(&pending.request.request_id)
        {
            return Ok(self.unknown(&pending.request.request_id));
        }
        self.finish(observation).map(DriveReply::Ready)
    }

    pub fn acknowledge_status(&mut self, value: Value) -> Result<DriveObservation, String> {
        self.reconcile_until = None;
        let pending = self.pending.clone().ok_or("no outstanding drive request")?;
        let observation = DriveObservation::from_reply(&value)?;
        if !observation.same_source(&pending.observation)
            || !observation.contains_operation(&pending.request.request_id)
        {
            return Err("the original drive result is unknown".into());
        }
        self.finish(observation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("zerus-drive-controller-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn observation() -> DriveObservation {
        DriveObservation {
            node_id: "10000000-0000-4000-8000-000000000001".into(),
            swarm_id: "20000000-0000-4000-8000-000000000002".into(),
            store_nonce: Some("30000000-0000-4000-8000-000000000003".into()),
            status: "ready".into(),
            records: BTreeMap::new(),
            versions: BTreeMap::new(),
            conflicts: BTreeSet::new(),
            unavailable: BTreeSet::new(),
            complete: true,
            operation_id: None,
            operation_found: false,
        }
    }

    fn request() -> DriveSetRequest {
        DriveSetRequest {
            original: None,
            desired: DriveRecord {
                id: Some("60000000-0000-4000-8000-000000000006".into()),
                project: "alpha".into(),
                machine: "40000000-0000-4000-8000-000000000004".into(),
                path: "C:/Work".into(),
                name: String::new(),
            },
            versions: BTreeMap::new(),
            request_id: "50000000-0000-4000-8000-000000000005".into(),
            store_nonce: Some("30000000-0000-4000-8000-000000000003".into()),
            source: None,
            destination: MachineProof {
                node: "40000000-0000-4000-8000-000000000004".into(),
                platform: "windows".into(),
                verified: true,
            },
        }
    }

    #[test]
    fn an_unknown_result_uses_only_original_uuid_status() {
        let dir = scratch();
        let path = dir.join("drive.json");
        let mut controller = DriveController::new(path.clone(), true);
        controller.observe(observation()).unwrap();
        let submission = controller.begin(request()).unwrap();
        assert!(path.exists());
        let DriveReply::Unknown(invocation) = controller.acknowledge(Err("lost".into())).unwrap()
        else {
            panic!("a lost result is unknown");
        };
        assert!(invocation.args.contains(&submission.request.request_id));
        assert!(controller.pending());
        let mut status = observation();
        status.operation_id = Some(submission.request.request_id.clone());
        status.operation_found = true;
        controller
            .acknowledge_status(serde_json::to_value(status).unwrap())
            .unwrap();
        assert!(!controller.pending());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn changed_source_cannot_rebind_an_unknown_result() {
        let dir = scratch();
        let mut controller = DriveController::new(dir.join("drive.json"), true);
        controller.observe(observation()).unwrap();
        controller.begin(request()).unwrap();
        let mut changed = observation();
        changed.store_nonce = Some("60000000-0000-4000-8000-000000000006".into());
        changed.operation_id = Some(request().request_id);
        changed.operation_found = true;
        assert!(controller
            .acknowledge_status(serde_json::to_value(changed).unwrap())
            .is_err());
        assert!(controller.pending());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_draft_removal_failure_keeps_the_committed_request_pending() {
        let dir = scratch();
        let path = dir.join("drive.json");
        let mut controller = DriveController::new(path.clone(), true);
        controller.observe(observation()).unwrap();
        controller.begin(request()).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let mut reply = observation();
        reply.operation_id = Some(request().request_id);
        reply.operation_found = true;
        assert!(controller
            .acknowledge_status(serde_json::to_value(reply).unwrap())
            .is_err());
        assert!(controller.pending());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_first_add_acknowledges_and_its_lease_returns_only_original_status() {
        let dir = scratch();
        let mut controller = DriveController::new(dir.join("drive.json"), true);
        let mut initial = observation();
        initial.status = "uninitialized".into();
        initial.store_nonce = None;
        initial.complete = false;
        controller.observe(initial).unwrap();
        let mut request = request();
        request.store_nonce = None;
        let submission = controller.begin(request).unwrap();
        let now = Instant::now();
        assert!(controller.poll(now).is_none());
        let expired = controller
            .poll(now + READ_TIMEOUT + Duration::from_secs(1))
            .unwrap();
        assert!(expired.args.contains(&submission.request.request_id));
        let mut landed = observation();
        landed.operation_id = Some(submission.request.request_id.clone());
        landed.operation_found = true;
        let reply =
            serde_json::json!({"operation_id":submission.request.request_id,"observation":landed});
        assert!(matches!(
            controller.acknowledge(Ok(reply)).unwrap(),
            DriveReply::Ready(_)
        ));
        assert!(!controller.pending());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_first_initialization_can_reconcile_but_another_nonce_cannot() {
        let mut pending = observation();
        pending.store_nonce = None;
        let initialized = observation();
        assert!(initialized.same_source(&pending));
        let mut other = observation();
        other.store_nonce = Some("70000000-0000-4000-8000-000000000007".into());
        assert!(!initialized.same_source(&other));
    }
}
