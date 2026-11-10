use super::connection::{NETWORK_TIMEOUT, configuration_revision};
use crate::{Invocation, fleet::valid_alias};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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
    lease_id: String,
}

fn digest(value: &Value) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| INVALID)?)
    ))
}

fn fields<'a>(
    value: &'a Value,
    names: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let object = value.as_object().ok_or(INVALID)?;
    if object.len() != names.len() || names.iter().any(|name| !object.contains_key(*name)) {
        return Err(INVALID.into());
    }
    Ok(object)
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
    if Uuid::parse_str(&text).map_err(|_| INVALID)?.to_string() != text {
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

fn component(value: &Value) -> Result<(), String> {
    fields(
        value,
        &[
            "state",
            "swarm_id",
            "raw_sha256",
            "raw_len",
            "canonical_sha256",
            "count",
            "parent_closure",
        ],
    )?;
    uuid(value, "swarm_id")?;
    hash(value, "canonical_sha256")?;
    if value["parent_closure"] != true
        || !value["count"].is_u64()
        || !matches!(
            value["state"].as_str(),
            Some("missing" | "valid_empty" | "nonempty")
        )
        || !(value["raw_sha256"].is_null()
            || value["raw_sha256"]
                .as_str()
                .is_some_and(|_| hash(value, "raw_sha256").is_ok()))
        || !(value["raw_len"].is_null() || value["raw_len"].is_u64())
        || value["raw_sha256"].is_null() != value["raw_len"].is_null()
        || (value["state"] == "missing") != value["raw_sha256"].is_null()
        || (value["state"] != "nonempty" && value["count"] != 0)
    {
        return Err(INVALID.into());
    }
    Ok(())
}

fn source(value: &Value) -> Result<(), String> {
    fields(
        value,
        &[
            "node_id",
            "swarm_id",
            "imported",
            "catalog_digest",
            "catalog_raw_sha256",
            "outer_store_sha256",
            "primary",
            "twins",
            "capabilities",
            "configuration_revision",
        ],
    )?;
    for key in ["node_id", "swarm_id"] {
        uuid(value, key)?;
    }
    for key in [
        "catalog_digest",
        "catalog_raw_sha256",
        "outer_store_sha256",
        "configuration_revision",
    ] {
        hash(value, key)?;
    }
    if !value["imported"].is_boolean() {
        return Err(INVALID.into());
    }
    component(&value["primary"])?;
    component(&value["twins"])?;
    if value["primary"]["swarm_id"] != value["swarm_id"]
        || value["twins"]["swarm_id"] != value["swarm_id"]
    {
        return Err(INVALID.into());
    }
    let capabilities = value["capabilities"].as_array().ok_or(INVALID)?;
    if capabilities.len() != 3
        || !capabilities.iter().all(Value::is_string)
        || !capabilities.contains(&json!(CAPABILITY))
        || !capabilities.contains(&json!("session_id"))
        || !capabilities.contains(&json!("session_id_memberships_v1"))
    {
        return Err(INVALID.into());
    }
    Ok(())
}

fn destination(value: &Value) -> Result<(), String> {
    fields(
        value,
        &[
            "node_id",
            "swarm_id",
            "catalog_digest",
            "outer_store_sha256",
            "primary",
            "twins",
            "empty_form",
            "self_bindings_valid",
            "bindings_sha256",
            "peers_empty",
            "membership_valid",
            "membership_epoch",
            "membership_generation",
            "membership_sha256",
            "qt_absent",
            "drive_absent",
            "eligible",
            "reasons",
        ],
    )?;
    for key in ["node_id", "swarm_id"] {
        uuid(value, key)?;
    }
    for key in ["catalog_digest", "outer_store_sha256", "bindings_sha256"] {
        hash(value, key)?;
    }
    component(&value["primary"])?;
    component(&value["twins"])?;
    for key in [
        "self_bindings_valid",
        "peers_empty",
        "membership_valid",
        "qt_absent",
        "drive_absent",
        "eligible",
    ] {
        if !value[key].is_boolean() {
            return Err(INVALID.into());
        }
    }
    if !value["membership_generation"].is_u64()
        || text(value, "empty_form")?.len() > 64
        || !(value["membership_epoch"].is_null()
            || value["membership_epoch"]
                .as_str()
                .is_some_and(|_| uuid(value, "membership_epoch").is_ok()))
        || !(value["membership_sha256"].is_null()
            || value["membership_sha256"]
                .as_str()
                .is_some_and(|_| hash(value, "membership_sha256").is_ok()))
        || value["membership_epoch"].is_null() != value["membership_sha256"].is_null()
    {
        return Err(INVALID.into());
    }
    let reasons = value["reasons"].as_array().ok_or(INVALID)?;
    if reasons.len() > 32
        || reasons
            .iter()
            .any(|reason| reason.as_str().is_none_or(|text| text.len() > 2048))
    {
        return Err(INVALID.into());
    }
    Ok(())
}

fn preview_digest(request_id: &str, preview: &Value) -> Result<String, String> {
    let mut semantic = preview["source"].clone();
    semantic["configuration_revision"] = "".into();
    digest(&json!({
        "version": 1, "request_id": request_id, "direction": "initiator_to_empty_destination",
        "source": semantic, "destination": preview["destination"],
        "destination_preview_sha256": preview["preview_sha256"],
    }))
}

fn identity(preview: &Value) -> Result<(), String> {
    fields(
        preview,
        &[
            "schema",
            "capability",
            "source",
            "destination",
            "eligible",
            "preview_sha256",
            "drive_status",
        ],
    )?;
    if preview["schema"] != 1
        || preview["capability"] != CAPABILITY
        || !preview["eligible"].is_boolean()
        || preview["drive_status"] != DRIVE_UNSUPPORTED
    {
        return Err(INVALID.into());
    }
    source(&preview["source"])?;
    destination(&preview["destination"])?;
    if hash(preview, "preview_sha256")?
        != digest(&json!({
            "version": 1, "direction": "initiator_to_empty_destination",
            "source": preview["source"], "destination": preview["destination"],
        }))?
        || preview["source"]["node_id"] == preview["destination"]["node_id"]
        || preview["destination"]["primary"]["swarm_id"] != preview["destination"]["swarm_id"]
        || preview["destination"]["twins"]["swarm_id"] != preview["destination"]["swarm_id"]
    {
        return Err(INVALID.into());
    }
    Ok(())
}

impl Preview {
    pub fn checked(alias: &str, reply: Value) -> Result<Self, String> {
        preview_invocation(alias)?;
        identity(&reply)?;
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

    pub fn source_card(&self) -> (String, String) {
        (
            text(&self.reply["source"], "node_id").unwrap_or_default(),
            text(&self.reply["source"], "swarm_id").unwrap_or_default(),
        )
    }

    pub fn destination_card(&self) -> (String, String) {
        (
            text(&self.reply["destination"], "node_id").unwrap_or_default(),
            text(&self.reply["destination"], "swarm_id").unwrap_or_default(),
        )
    }

    pub fn configuration_revision(&self) -> &str {
        self.reply["source"]["configuration_revision"]
            .as_str()
            .unwrap_or_default()
    }

    pub fn adopt(&self, native_ticket: String, native_generation: u64) -> Result<Adoption, String> {
        if !self.eligible()
            || native_generation == 0
            || native_ticket.is_empty()
            || native_ticket.len() > 512
            || native_ticket.chars().any(char::is_control)
        {
            return Err(INVALID.into());
        }
        Ok(Adoption {
            preview: self.clone(),
            native_ticket,
            native_generation,
            lease_id: Uuid::new_v4().to_string(),
        })
    }
}

impl Adoption {
    fn original(&self) -> Result<Value, String> {
        let request_sha256 = preview_digest(&self.preview.request_id, &self.preview.reply)?;
        let binding_sha256 = digest(&json!({
            "request_sha256": request_sha256, "source_alias": self.preview.alias,
            "source_configuration_revision": self.preview.reply["source"]["configuration_revision"],
        }))?;
        let owner_sha256 = digest(&json!({
            "native_ticket": self.native_ticket, "native_generation": self.native_generation,
            "lease_id": self.lease_id,
        }))?;
        Ok(json!({
            "request_id": self.preview.request_id, "request_sha256": request_sha256,
            "binding_sha256": binding_sha256, "owner_sha256": owner_sha256,
            "lease_id": self.lease_id, "native_generation": self.native_generation,
            "source_alias": self.preview.alias,
            "source_configuration_revision": self.preview.reply["source"]["configuration_revision"],
            "source": self.preview.reply["source"], "destination": self.preview.reply["destination"],
            "source_catalog_raw_sha256": self.preview.reply["source"]["catalog_raw_sha256"],
            "request_raw_sha256": "0".repeat(64),
        }))
    }

    fn check_original(&self, original: &Value) -> Result<(), String> {
        fields(
            original,
            &[
                "request_id",
                "request_sha256",
                "binding_sha256",
                "owner_sha256",
                "lease_id",
                "native_generation",
                "source_alias",
                "source_configuration_revision",
                "source",
                "destination",
                "source_catalog_raw_sha256",
                "request_raw_sha256",
            ],
        )?;
        hash(original, "request_raw_sha256")?;
        let mut expected = self.original()?;
        expected["request_raw_sha256"] = original["request_raw_sha256"].clone();
        if original != &expected {
            return Err(INVALID.into());
        }
        Ok(())
    }

    pub fn invocation(&self) -> Result<Invocation, String> {
        let body = serde_json::to_vec(&json!({
            "request_id": self.preview.request_id, "preview": self.preview.reply,
            "native_ticket": self.native_ticket, "native_generation": self.native_generation,
            "lease_id": self.lease_id,
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

    pub fn request_id(&self) -> &str {
        &self.preview.request_id
    }

    fn terminal(&self, original: &Value, reply: &Value) -> Result<(), String> {
        fields(
            reply,
            &[
                "schema",
                "phase",
                "outcome",
                "request_id",
                "request_sha256",
                "binding_sha256",
                "owner_sha256",
                "original",
                "main_receipt",
                "primary_receipt",
                "twins_receipt",
                "post_destination",
                "destination_peers",
                "destination_bindings",
                "guard_cleared",
                "drive_status",
                "membership_by_id_ready",
            ],
        )?;
        if reply["schema"] != 1
            || reply["phase"] != "remote_committed"
            || reply["outcome"] != "remote_committed"
            || reply["request_id"] != original["request_id"]
            || reply["request_sha256"] != original["request_sha256"]
            || reply["binding_sha256"] != original["binding_sha256"]
            || reply["owner_sha256"] != original["owner_sha256"]
            || reply["destination_peers"]
                .as_object()
                .is_none_or(|peers| !peers.is_empty())
            || reply["destination_bindings"] != "self_only"
            || reply["guard_cleared"] != true
            || reply["drive_status"] != DRIVE_UNSUPPORTED
            || reply["membership_by_id_ready"] != false
        {
            return Err(INVALID.into());
        }
        let post = &reply["post_destination"];
        destination(post)?;
        if post["node_id"] != original["destination"]["node_id"]
            || post["swarm_id"] != original["source"]["swarm_id"]
            || post["catalog_digest"] != original["source"]["catalog_digest"]
            || post["primary"] != original["source"]["primary"]
            || post["twins"] != original["source"]["twins"]
            || post["bindings_sha256"] != original["destination"]["bindings_sha256"]
            || post["self_bindings_valid"] != true
            || post["peers_empty"] != true
            || post["membership_valid"] != true
            || post["membership_generation"]
                .as_u64()
                .is_none_or(|generation| generation == 0)
            || post["qt_absent"] != true
            || post["drive_absent"] != true
        {
            return Err(INVALID.into());
        }
        let main = &reply["main_receipt"];
        fields(
            main,
            &[
                "original_source_catalog_sha256",
                "expected_source_catalog_digest",
                "destination_catalog_digest",
                "original_destination_store_sha256",
                "destination_store_sha256",
                "destination_bindings_sha256",
                "destination_peers_empty",
                "parent_closure",
                "postread",
            ],
        )?;
        if main["original_source_catalog_sha256"] != original["source_catalog_raw_sha256"]
            || main["expected_source_catalog_digest"] != original["source"]["catalog_digest"]
            || main["destination_catalog_digest"] != original["source"]["catalog_digest"]
            || main["original_destination_store_sha256"]
                != original["destination"]["outer_store_sha256"]
            || main["destination_store_sha256"] != post["outer_store_sha256"]
            || main["destination_bindings_sha256"] != original["destination"]["bindings_sha256"]
            || main["destination_peers_empty"] != true
            || main["parent_closure"] != true
            || main["postread"] != true
        {
            return Err(INVALID.into());
        }
        for (name, expected) in [("primary_receipt", "primary"), ("twins_receipt", "twins")] {
            let component_receipt = &reply[name];
            fields(
                component_receipt,
                &[
                    "component",
                    "source",
                    "destination",
                    "destination_raw_sha256",
                    "destination_raw_len",
                    "parent_closure",
                    "postread",
                ],
            )?;
            let kind = expected;
            if component_receipt["component"] != kind
                || component_receipt["source"] != original["source"][kind]
                || component_receipt["destination"] != original["source"][kind]
                || component_receipt["destination_raw_sha256"]
                    != original["source"][kind]["raw_sha256"]
                || component_receipt["destination_raw_len"] != original["source"][kind]["raw_len"]
                || component_receipt["parent_closure"] != true
                || component_receipt["postread"] != true
            {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }

    pub fn receipt(&self, reply: &Value) -> Result<Value, String> {
        if uuid(reply, "request_id")? != self.preview.request_id {
            return Err(INVALID.into());
        }
        let original = &reply["original"];
        self.check_original(original)?;
        match reply["phase"].as_str() {
            Some("refused_not_started") => {
                fields(
                    reply,
                    &[
                        "schema",
                        "phase",
                        "outcome",
                        "request_id",
                        "request_sha256",
                        "binding_sha256",
                        "original",
                        "reasons",
                        "product_effects",
                    ],
                )?;
                if reply["schema"] != 1
                    || reply["outcome"] != "refused_not_started"
                    || reply["request_sha256"] != original["request_sha256"]
                    || reply["binding_sha256"] != original["binding_sha256"]
                    || reply["product_effects"] != false
                    || reply["reasons"]
                        .as_array()
                        .is_none_or(|reasons| reasons.len() > 32)
                {
                    return Err(INVALID.into());
                }
            }
            Some("remote_committed") => self.terminal(original, reply)?,
            Some("settled") => {
                fields(
                    reply,
                    &[
                        "schema",
                        "phase",
                        "request_id",
                        "request_sha256",
                        "binding_sha256",
                        "original",
                        "remote",
                        "source_membership",
                        "destination_peers",
                        "destination_bindings",
                        "sync",
                        "drive_status",
                        "membership_by_id_ready",
                    ],
                )?;
                self.terminal(original, &reply["remote"])?;
                let membership = &reply["source_membership"];
                fields(
                    membership,
                    &[
                        "original_source_store_sha256",
                        "source_store_sha256",
                        "destination_node_id",
                        "peer_added",
                        "postread",
                    ],
                )?;
                hash(membership, "source_store_sha256")?;
                if reply["schema"] != 1
                    || reply["request_sha256"] != original["request_sha256"]
                    || reply["binding_sha256"] != original["binding_sha256"]
                    || membership["original_source_store_sha256"]
                        != original["source"]["outer_store_sha256"]
                    || membership["source_store_sha256"] == original["source"]["outer_store_sha256"]
                    || membership["destination_node_id"] != original["destination"]["node_id"]
                    || membership["peer_added"] != original["source_alias"]
                    || membership["postread"] != true
                    || reply["destination_peers"]
                        .as_object()
                        .is_none_or(|peers| !peers.is_empty())
                    || reply["destination_bindings"] != "self_only"
                    || reply["sync"] != "sync_peer_completed"
                    || reply["drive_status"] != DRIVE_UNSUPPORTED
                    || reply["membership_by_id_ready"] != false
                {
                    return Err(INVALID.into());
                }
            }
            _ => return Err(INVALID.into()),
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
    let body = serde_json::to_vec(&json!({"request_id": request_id})).map_err(|_| INVALID)?;
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

    fn component(swarm: &str) -> Value {
        json!({"state": "missing", "swarm_id": swarm, "raw_sha256": null, "raw_len": null,
            "canonical_sha256": "b".repeat(64), "count": 0, "parent_closure": true})
    }

    fn preview(alias: &str) -> Preview {
        let source_swarm = Uuid::new_v4().to_string();
        let destination_swarm = Uuid::new_v4().to_string();
        let source = json!({
            "node_id": Uuid::new_v4().to_string(), "swarm_id": source_swarm, "imported": true,
            "catalog_digest": "c".repeat(64), "catalog_raw_sha256": "9".repeat(64),
            "outer_store_sha256": "d".repeat(64), "primary": component(&source_swarm),
            "twins": component(&source_swarm), "capabilities": [CAPABILITY, "session_id", "session_id_memberships_v1"],
            "configuration_revision": "a".repeat(64),
        });
        let destination = json!({
            "node_id": Uuid::new_v4().to_string(), "swarm_id": destination_swarm,
            "catalog_digest": "e".repeat(64), "outer_store_sha256": "f".repeat(64),
            "primary": component(&destination_swarm), "twins": component(&destination_swarm),
            "empty_form": "untouched_uninitialized", "self_bindings_valid": true, "bindings_sha256": "0".repeat(64),
            "peers_empty": true, "membership_valid": true, "membership_epoch": null, "membership_generation": 0,
            "membership_sha256": null, "qt_absent": true, "drive_absent": true, "eligible": true, "reasons": [],
        });
        let hash = digest(
            &json!({"version": 1, "direction": "initiator_to_empty_destination",
            "source": source, "destination": destination}),
        )
        .unwrap();
        Preview::checked(alias, json!({"schema": 1, "capability": CAPABILITY, "source": source,
            "destination": destination, "eligible": true, "preview_sha256": hash, "drive_status": DRIVE_UNSUPPORTED})).unwrap()
    }

    fn settled(adoption: &Adoption) -> Value {
        let original = adoption.original().unwrap();
        let mut post = original["destination"].clone();
        post["swarm_id"] = original["source"]["swarm_id"].clone();
        post["catalog_digest"] = original["source"]["catalog_digest"].clone();
        post["outer_store_sha256"] = "8".repeat(64).into();
        post["primary"] = original["source"]["primary"].clone();
        post["twins"] = original["source"]["twins"].clone();
        post["membership_generation"] = 1.into();
        post["membership_epoch"] = Uuid::new_v4().to_string().into();
        post["membership_sha256"] = "7".repeat(64).into();
        let main = json!({"original_source_catalog_sha256": original["source_catalog_raw_sha256"],
            "expected_source_catalog_digest": original["source"]["catalog_digest"],
            "destination_catalog_digest": original["source"]["catalog_digest"],
            "original_destination_store_sha256": original["destination"]["outer_store_sha256"],
            "destination_store_sha256": post["outer_store_sha256"],
            "destination_bindings_sha256": original["destination"]["bindings_sha256"],
            "destination_peers_empty": true, "parent_closure": true, "postread": true});
        let component = |kind: &str| {
            json!({"component": kind, "source": original["source"][kind],
            "destination": original["source"][kind], "destination_raw_sha256": original["source"][kind]["raw_sha256"],
            "destination_raw_len": original["source"][kind]["raw_len"], "parent_closure": true, "postread": true})
        };
        let remote = json!({"schema": 1, "phase": "remote_committed", "outcome": "remote_committed",
            "request_id": original["request_id"], "request_sha256": original["request_sha256"],
            "binding_sha256": original["binding_sha256"], "owner_sha256": original["owner_sha256"],
            "original": original, "main_receipt": main, "primary_receipt": component("primary"),
            "twins_receipt": component("twins"), "post_destination": post, "destination_peers": {},
            "destination_bindings": "self_only", "guard_cleared": true, "drive_status": DRIVE_UNSUPPORTED,
            "membership_by_id_ready": false});
        json!({"schema": 1, "phase": "settled", "request_id": remote["request_id"],
            "request_sha256": remote["request_sha256"], "binding_sha256": remote["binding_sha256"],
            "original": remote["original"], "remote": remote, "source_membership": {
                "original_source_store_sha256": remote["original"]["source"]["outer_store_sha256"],
                "source_store_sha256": "6".repeat(64), "destination_node_id": remote["original"]["destination"]["node_id"],
                "peer_added": remote["original"]["source_alias"], "postread": true,
            }, "destination_peers": {}, "destination_bindings": "self_only", "sync": "sync_peer_completed",
            "drive_status": DRIVE_UNSUPPORTED, "membership_by_id_ready": false})
    }

    #[test]
    fn adoption_has_a_distinct_request_and_directional_receipt() {
        let adoption = preview("peer-b").adopt("native-owner".into(), 1).unwrap();
        let invocation = adoption.invocation().unwrap();
        assert_eq!(invocation.args[1], "adopt");
        assert_eq!(invocation.timeout, NETWORK_TIMEOUT);
        assert!(
            invocation
                .stdin
                .as_ref()
                .is_some_and(|body| !body.windows(b"previous_lease_drained".len()).any(|w| w == b"previous_lease_drained"))
        );
        assert!(adoption.receipt(&settled(&adoption)).is_ok());
    }

    #[test]
    fn wrong_endpoint_or_component_never_releases_the_original() {
        let adoption = preview("peer-b").adopt("owner".into(), 1).unwrap();
        let mut receipt = settled(&adoption);
        receipt["original"]["destination"]["node_id"] = Uuid::new_v4().to_string().into();
        assert!(adoption.receipt(&receipt).is_err());
        let mut receipt = settled(&adoption);
        receipt["remote"]["primary_receipt"]["destination"]["count"] = 1.into();
        assert!(adoption.receipt(&receipt).is_err());
        let mut value = preview("peer-b").reply;
        value["eligible"] = false.into();
        assert!(
            Preview::checked("peer-b", value)
                .unwrap()
                .adopt("owner".into(), 1)
                .is_err()
        );
    }
}
