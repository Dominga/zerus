use super::connection::{NETWORK_TIMEOUT, configuration_revision};
use crate::{Invocation, fleet::valid_alias};
use serde_json::Value;
use uuid::Uuid;

pub const INVALID: &str = super::connection::INVALID;
pub const CAPABILITY: &str = "empty_peer_adoption_v1";
pub const DRIVE_UNSUPPORTED: &str = "unsupported_uninitialized_without_reverse_peer";
pub const SUMMARY_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct Preview {
    alias: String,
    reply: Value,
    request_id: String,
}

#[derive(Clone, Debug)]
pub struct Adoption {
    preview: Preview,
    native_ticket: String,
    native_generation: u64,
    previous_lease_drained: bool,
}

fn text(value: &Value, key: &str) -> Result<String, String> {
    let text = value[key].as_str().ok_or(INVALID)?;
    if text.is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
        return Err(INVALID.into());
    }
    Ok(text.into())
}

fn uuid(value: &Value, key: &str) -> Result<String, String> {
    let text = text(value, key)?;
    if Uuid::parse_str(&text).is_err() {
        return Err(INVALID.into());
    }
    Ok(text)
}

fn hash(value: &Value, key: &str) -> Result<String, String> {
    let text = text(value, key)?;
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(INVALID.into());
    }
    Ok(text)
}

fn identity(preview: &Value) -> Result<(), String> {
    if preview["schema"] != 1
        || preview["capability"] != CAPABILITY
        || !preview["eligible"].is_boolean()
        || !preview["source"].is_object()
        || !preview["destination"].is_object()
        || preview["drive_status"] != DRIVE_UNSUPPORTED
    {
        return Err(INVALID.into());
    }
    for side in ["source", "destination"] {
        uuid(&preview[side], "node_id")?;
        uuid(&preview[side], "swarm_id")?;
        hash(&preview[side], "catalog_digest")?;
        hash(&preview[side], "outer_store_sha256")?;
        for component in ["primary", "twins"] {
            uuid(&preview[side][component], "swarm_id")?;
            hash(&preview[side][component], "canonical_sha256")?;
            if !matches!(
                preview[side][component]["state"].as_str(),
                Some("missing" | "valid_empty" | "nonempty")
            ) || !preview[side][component]["count"].is_u64()
            {
                return Err(INVALID.into());
            }
        }
    }
    hash(preview, "preview_sha256")?;
    Ok(())
}

impl Preview {
    pub fn checked(alias: &str, reply: Value) -> Result<Self, String> {
        preview_invocation(alias)?;
        identity(&reply)?;
        if reply["source"]["node_id"] == reply["destination"]["node_id"] {
            return Err(INVALID.into());
        }
        Ok(Self {
            alias: alias.into(),
            reply,
            request_id: Uuid::new_v4().to_string(),
        })
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn eligible(&self) -> bool {
        self.reply["eligible"] == true
    }

    pub fn reason(&self) -> String {
        if self.eligible() {
            return format!(
                "{} can adopt this catalog. Drive folders remain uninitialized without reverse enrollment.",
                self.alias
            );
        }
        self.reply["destination"]["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .take(4)
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn drive_status(&self) -> &str {
        DRIVE_UNSUPPORTED
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn matches_source(&self, local: &Value) -> bool {
        local["node_id"] == self.reply["source"]["node_id"]
            && local["swarm_id"] == self.reply["source"]["swarm_id"]
            && local["digest"] == self.reply["source"]["catalog_digest"]
    }

    pub fn adopt(&self, native_ticket: String, native_generation: u64) -> Result<Adoption, String> {
        if !self.eligible() || native_generation == 0 || native_ticket.is_empty() {
            return Err(INVALID.into());
        }
        Ok(Adoption {
            preview: self.clone(),
            native_ticket,
            native_generation,
            previous_lease_drained: false,
        })
    }
}

impl Adoption {
    pub fn invocation(&self) -> Result<Invocation, String> {
        let body = serde_json::to_vec(&serde_json::json!({
            "request_id": self.preview.request_id,
            "preview": self.preview.reply,
            "native_ticket": self.native_ticket,
            "native_generation": self.native_generation,
            "previous_lease_drained": self.previous_lease_drained,
        }))
        .map_err(|_| INVALID)?;
        if body.len() > SUMMARY_BYTES {
            return Err(INVALID.into());
        }
        Ok(Invocation {
            args: vec![
                "swarm".into(),
                "adopt".into(),
                self.preview.alias.clone(),
                "--json".into(),
            ],
            stdin: Some(body),
            timeout: NETWORK_TIMEOUT,
            request_id: None,
        })
    }

    pub fn after_drain(&self) -> Self {
        Self {
            previous_lease_drained: true,
            ..self.clone()
        }
    }

    pub fn request_id(&self) -> &str {
        &self.preview.request_id
    }

    pub fn receipt(&self, reply: &Value) -> Result<Value, String> {
        if uuid(reply, "request_id")? != self.preview.request_id {
            return Err(INVALID.into());
        }
        hash(reply, "request_sha256")?;
        hash(reply, "binding_sha256")?;
        match reply["phase"].as_str() {
            Some("refused_not_started" | "remote_committed" | "settled") => {}
            _ => return Err(INVALID.into()),
        }
        if reply["phase"] == "settled"
            && (reply["destination_peers"]
                .as_object()
                .is_none_or(|peers| !peers.is_empty())
                || reply["destination_bindings"] != "self_only"
                || reply["drive_status"] != DRIVE_UNSUPPORTED
                || reply["sync"] != "sync_peer_completed")
        {
            return Err(INVALID.into());
        }
        Ok(reply.clone())
    }
}

pub fn preview_invocation(alias: &str) -> Result<Invocation, String> {
    if !valid_alias(alias) {
        return Err("Choose a configured computer.".into());
    }
    Ok(Invocation {
        args: vec![
            "swarm".into(),
            "adoption-preview".into(),
            alias.into(),
            "--json".into(),
        ],
        stdin: None,
        timeout: NETWORK_TIMEOUT,
        request_id: None,
    })
}

pub fn status_invocation(alias: &str, request_id: &str) -> Result<Invocation, String> {
    preview_invocation(alias)?;
    if Uuid::parse_str(request_id).is_err() {
        return Err(INVALID.into());
    }
    let body =
        serde_json::to_vec(&serde_json::json!({"request_id": request_id})).map_err(|_| INVALID)?;
    Ok(Invocation {
        args: vec![
            "swarm".into(),
            "adoption-status".into(),
            alias.into(),
            "--json".into(),
        ],
        stdin: Some(body),
        timeout: NETWORK_TIMEOUT,
        request_id: None,
    })
}

pub fn revision(list: &Value, alias: &str) -> Result<String, String> {
    configuration_revision(list, alias)
}

pub fn configured_aliases(list: &Value) -> Result<Vec<String>, String> {
    super::connection::configured_aliases(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn component(swarm: &str) -> Value {
        json!({
            "state": "missing",
            "swarm_id": swarm,
            "raw_sha256": null,
            "canonical_sha256": "b".repeat(64),
            "count": 0,
        })
    }

    fn preview(alias: &str) -> Preview {
        let source = Uuid::new_v4().to_string();
        let destination = Uuid::new_v4().to_string();
        let source_swarm = Uuid::new_v4().to_string();
        let destination_swarm = Uuid::new_v4().to_string();
        Preview::checked(
            alias,
            json!({
                "schema": 1,
                "capability": CAPABILITY,
                "eligible": true,
                "preview_sha256": "a".repeat(64),
                "drive_status": DRIVE_UNSUPPORTED,
                "source": {
                    "node_id": source,
                    "swarm_id": source_swarm,
                    "catalog_digest": "c".repeat(64),
                    "outer_store_sha256": "d".repeat(64),
                    "primary": component(&source_swarm),
                    "twins": component(&source_swarm),
                },
                "destination": {
                    "node_id": destination,
                    "swarm_id": destination_swarm,
                    "catalog_digest": "e".repeat(64),
                    "outer_store_sha256": "f".repeat(64),
                    "primary": component(&destination_swarm),
                    "twins": component(&destination_swarm),
                },
            }),
        )
        .unwrap()
    }

    #[test]
    fn adoption_has_a_distinct_request_and_directional_receipt() {
        let preview = preview("peer-b");
        let adoption = preview.adopt("native-owner".into(), 1).unwrap();
        let invocation = adoption.invocation().unwrap();
        assert_eq!(invocation.args[1], "adopt");
        assert_eq!(invocation.timeout, NETWORK_TIMEOUT);
        let receipt = adoption.receipt(&json!({
            "phase": "settled",
            "request_id": adoption.request_id(),
            "request_sha256": "a".repeat(64),
            "binding_sha256": "b".repeat(64),
            "destination_peers": {},
            "destination_bindings": "self_only",
            "drive_status": DRIVE_UNSUPPORTED,
            "sync": "sync_peer_completed",
        }));
        assert!(receipt.is_ok());
    }

    #[test]
    fn ineligible_or_reverse_transport_never_becomes_success() {
        let mut value = preview("peer-b").reply;
        value["eligible"] = false.into();
        assert!(
            Preview::checked("peer-b", value)
                .unwrap()
                .adopt("owner".into(), 1)
                .is_err()
        );
        let adoption = preview("peer-b").adopt("owner".into(), 1).unwrap();
        let receipt = json!({
            "phase": "settled",
            "request_id": adoption.request_id(),
            "request_sha256": "a".repeat(64),
            "binding_sha256": "b".repeat(64),
            "destination_peers": {"source": {"node": "wrong"}},
            "destination_bindings": "self_only",
            "drive_status": DRIVE_UNSUPPORTED,
            "sync": "sync_peer_completed",
        });
        assert!(adoption.receipt(&receipt).is_err());
    }
}
