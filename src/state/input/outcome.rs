//! The opted-in terminal negative is never re-admitted. Its original private
//! refusal record alone proves it; current state, text and absence never do.
use super::*;
use crate::state::repositories::directory::Directory;
use serde::{Deserialize, Serialize};
use std::io::Read;
use zerus_client_core::desktop::deferred_send::{refusal, Refusal};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Terminal {
    refusal_schema: u8,
    request_id: String,
    name: String,
    run_id: String,
    conversation_id: String,
    first_message: bool,
    digest: String,
    status: String,
    transport: String,
    reason: String,
    delivery_binding: Option<receipt_index::DeliveryBinding>,
}

impl Terminal {
    fn response(self) -> Result<Value> {
        let Some(binding) = self.delivery_binding else {
            return Err("Terminal refusal identity was not established".into());
        };
        let identity = Refusal {
            schema: 1,
            request_id: self.request_id.clone(),
            session: self.name.clone(),
            run_id: self.run_id.clone(),
            conversation_id: self.conversation_id.clone(),
            input_sha256: self.digest.clone(),
            reason: self.reason.clone(),
        };
        if self.refusal_schema != 2
            || self.status != "not_offered"
            || self.transport != "terminal"
            || self.first_message != self.conversation_id.is_empty()
            || !identity.valid()
            || !binding.valid()
            || !matches!(binding.provider.as_str(), "claude" | "codex" | "kimi")
        {
            return Err("Invalid terminal refusal evidence".into());
        }
        Ok(
            json!({"schema":2,"status":"not_offered","proof":"native_input_lock",
            "request_id":self.request_id,"name":self.name,"run_id":self.run_id,
            "conversation_id":self.conversation_id,"native_digest":self.digest,
            "machine_id":binding.machine_id,"session_id":binding.session_id,
            "provider":binding.provider,"reason":self.reason}),
        )
    }
}

/// Called only while the live NotOffered witness's original input lock is held.
/// Read one selected slot and an existing machine identity; never initialize it.
pub(super) fn binding(witness: &NotOffered) -> Option<Value> {
    let record = storage::read_selected_metadata(&witness.name).ok()?;
    if record["name"] != witness.name
        || record["run_id"] != witness.run_id
        || string(&record, "conversation_id") != witness.conversation_id
        || !matches!(string(&record, "agent"), "claude" | "codex" | "kimi")
    {
        return None;
    }
    let config = crate::config::Config::load().ok()?;
    let machine = session_action::status_machine(&config).ok()?;
    let target = session_graph::target_receipt_for_machine(&record, &machine)?;
    receipt_index::delivery_binding(&record, &target)
}

pub(super) fn decode(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > refusal::BYTES {
        return Err("Terminal refusal exceeds its bound".into());
    }
    serde_json::from_slice::<Terminal>(bytes)
        .map_err(|_| "Invalid terminal refusal evidence".to_owned())?
        .response()
}

fn controlled(directory: &Directory) -> Result<()> {
    #[cfg(unix)]
    let safe = directory.owner_controls_entries()?;
    #[cfg(windows)]
    let safe = directory.private()?;
    if safe {
        Ok(())
    } else {
        Err("Terminal refusal source changed".into())
    }
}

/// A bounded original-only read, with no SQL, current record, repair or replay.
pub(in crate::state) fn lookup(state: &Path, id: &str) -> Result<Value> {
    if !state.is_absolute()
        || !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id && !value.is_nil())
    {
        return Err("Invalid terminal refusal source".into());
    }
    let root = Directory::open(state)?;
    controlled(&root)?;
    let directory = root.child("input_receipts")?;
    controlled(&directory)?;
    let file = directory.read_private(&format!("{id}.json"))?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    if before.len() > refusal::BYTES as u64 {
        return Err("Terminal refusal exceeds its bound".into());
    }
    let mut bytes = Vec::new();
    (&file)
        .take(refusal::BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    if before.len() != after.len()
        || bytes.len() as u64 != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("Terminal refusal changed while reading".into());
    }
    root.verify()?;
    directory.verify()?;
    let response = decode(&bytes)?;
    if response["request_id"] != id {
        return Err("Terminal refusal UUID changed".into());
    }
    Ok(response)
}

/// Only the opt-in public send can replay the exact spent negative, under Root.
pub(super) fn duplicate(
    state: &Path,
    name: &str,
    request: &Request,
    digest: &str,
) -> Option<Value> {
    let id = uuid::Uuid::parse_str(&request.request_id).ok()?.to_string();
    let value = lookup(state, &id).ok()?;
    if value["name"] != name
        || value["run_id"] != request.expected_run_id
        || value["conversation_id"] != request.expected_conversation_id
        || value["native_digest"] != digest
        || session_id::selected().is_some_and(|selected| value["session_id"] != selected.session_id)
    {
        return None;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn possible_settings_input_never_publishes_a_terminal_negative() {
        let root = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .env_clear()
            .env("HOME", root.path())
            .env("USERPROFILE", root.path())
            .env("HGS_STATE_DIR", root.path().join("state"))
            .env("HGS_CONFIG_DIR", root.path().join("config"))
            .env("ZERUS_OUTCOME_GUARD_CONTROL", "1")
            .args([
                "--exact",
                "state::input::outcome::tests::typed_control",
                "--ignored",
                "--test-threads=1",
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "isolated typed-input fixture, invoked by its outer control"]
    fn typed_control() {
        assert_eq!(std::env::var("ZERUS_OUTCOME_GUARD_CONTROL").unwrap(), "1");
        private_dir(&root()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let path = root().join("input_receipts").join(format!("{id}.json"));
        let witness = NotOffered {
            request_id: id,
            name: "codex/private-outcome-control".into(),
            run_id: "run".into(),
            conversation_id: "conversation".into(),
            digest: "a".repeat(64),
            receipts: path.clone(),
            reason: "settings may have been typed".into(),
            waiting: true,
            dialog: None,
        };
        assert!(matches!(
            remember_send_refusal(true, witness, true, true),
            Failure::Unknown(_)
        ));
        assert!(
            !path.exists(),
            "possible settings input must not publish terminal-negative evidence"
        );
    }
}
