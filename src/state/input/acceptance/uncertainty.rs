//! Unknown does not cancel an input. Keep a bounded set of indistinguishable
//! inputs per native conversation, across runs, renames and record replacement.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const LIMIT: usize = 32;
const BYTES: u64 = 8 * 1024;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    agent: String,
    conversation_id: String,
    all: bool,
    requests: BTreeMap<String, String>,
}

pub(super) struct State {
    data: Stored,
    writable: bool,
}

fn path(record: &Value) -> Result<PathBuf> {
    let agent = string(record, "agent");
    let conversation = string(record, "conversation_id");
    if !composer::supported(agent)
        || (conversation.is_empty()
            || conversation.len() > 256
            || conversation.chars().any(char::is_control))
    {
        return Err("This input cannot be acknowledged now; check Terminal".into());
    }
    let key = format!("{:x}", Sha256::digest(format!("{agent}\0{conversation}")));
    Ok(absolute_root()?
        .join("input_acceptance_barriers")
        .join(format!("{key}.json")))
}

fn fingerprint(text: &str) -> String {
    // This over-approximates Claude's native text equivalence (including image
    // path/whitespace rendering). Coarser keys only refuse more; they never
    // make retrospective evidence stronger than the native proof comparator.
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&settle::significant(text)).unwrap_or_default())
    )
}

impl State {
    fn empty(record: &Value) -> Self {
        Self {
            data: Stored {
                version: 1,
                agent: string(record, "agent").into(),
                conversation_id: string(record, "conversation_id").into(),
                ..Stored::default()
            },
            writable: true,
        }
    }
    fn unavailable(record: &Value) -> Self {
        let mut state = Self::empty(record);
        state.data.all = true;
        state.writable = false;
        state
    }
    pub(super) fn uncertain(&self) -> bool {
        self.data.all || !self.data.requests.is_empty()
    }
    pub(super) fn blocks(&self, text: &str) -> bool {
        let key = fingerprint(text);
        self.data.all || self.data.requests.values().any(|stored| stored == &key)
    }
}

pub(super) fn load(record: &Value) -> State {
    let Ok(path) = path(record) else {
        return State::unavailable(record);
    };
    let Some(parent) = path.parent() else {
        return State::unavailable(record);
    };
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.is_dir() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return State::unavailable(record),
    }
    let mut state = match labels::read_file(&path, BYTES) {
        Ok(None) => State::empty(record),
        Ok(Some(bytes)) => {
            let Ok(data) = serde_json::from_slice::<Stored>(&bytes) else {
                return State::unavailable(record);
            };
            if data.version != 1
                || data.agent != string(record, "agent")
                || data.conversation_id != string(record, "conversation_id")
                || data.requests.len() > LIMIT
                || (!data.all && data.requests.is_empty())
                || data.requests.iter().any(|(id, key)| {
                    !zerus_client_core::input_acknowledgment::canonical(id)
                        || key.len() != 64
                        || !key
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                })
            {
                return State::unavailable(record);
            }
            State {
                data,
                writable: true,
            }
        }
        Err(_) => return State::unavailable(record),
    };
    // Legacy acknowledgments can precede the bounded conversation ledger. If
    // their original identity is not covered, history is incomplete, not clear.
    if let Some(ack) = record.get("input_acknowledged_unknown") {
        if ack["conversation_id"].as_str().is_none()
            || (ack["conversation_id"] == record["conversation_id"]
                && !state
                    .data
                    .requests
                    .contains_key(string(ack, "original_request_id")))
        {
            state.data.all = true;
        }
    }
    state
}

/// Called after audit reservation and before the record gate opens. The file
/// stays below 8 KiB; overflow becomes permanently unknown for this conversation.
pub(in crate::state) fn remember_unknown(record: &Value) -> Result<()> {
    let mut state = load(record);
    if !state.writable {
        return Err("This input cannot be acknowledged now; check Terminal".into());
    }
    if let Some(receipt) = pending_receipt(record) {
        let id = string(&receipt, "request_id").to_owned();
        let key = fingerprint(string(&receipt, "submitted_text"));
        if state.data.requests.get(&id).is_some_and(|old| old != &key)
            || (!state.data.requests.contains_key(&id) && state.data.requests.len() == LIMIT)
        {
            state.data.all = true;
        } else {
            state.data.requests.insert(id, key);
        }
    } else {
        state.data.all = true;
    }
    if state.data.all {
        state.data.requests.clear();
    }
    let bytes = serde_json::to_string(&state.data).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > BYTES {
        return Err("Input uncertainty is unavailable; check Terminal".into());
    }
    atomic(&path(record)?, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertainty_covers_native_text_equivalence_without_loosening_proof() {
        assert_eq!(fingerprint("same\n\ntext"), fingerprint(" same \ntext "));
        assert_eq!(
            fingerprint("[Image #1] same\n/srv/fixture/image.png"),
            fingerprint("same")
        );
        assert_ne!(fingerprint("same"), fingerprint("different"));
    }
}
