//! Closed cache intents. A prepared binding/hash is correlation data: only a
//! fresh native frontend permit plus the service's original frozen worker can
//! admit its exact bounded pass. No generic request can execute these values.
use super::digest;
use serde::{Deserialize, Serialize};
use zerus_desktop_migration::canonical_uuid;

pub const ABI: &str = "desktop-cache-maintenance-v1";
pub const HELP_MARKER: &str = "Desktop cache maintenance ABI: desktop-cache-maintenance-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Prepare {
        operation_id: String,
        configuration: String,
        root: String,
        limit: u64,
        revision: u64,
    },
    Execute {
        preparation_id: String,
        binding_sha256: String,
    },
}
impl Command {
    pub fn valid(&self, request_id: &str) -> bool {
        if !canonical_uuid(request_id) {
            return false;
        }
        match self {
            Self::Prepare {
                operation_id,
                configuration,
                root,
                revision,
                ..
            } => {
                canonical_uuid(operation_id)
                    && operation_id != request_id
                    && digest(configuration)
                    && *revision < i64::MAX as u64
                    && root.len() <= 8192
                    && !root.chars().any(char::is_control)
                    && (crate::paths::absolute(root, crate::paths::Style::Posix)
                        || crate::paths::absolute(root, crate::paths::Style::Windows))
            }
            Self::Execute {
                preparation_id,
                binding_sha256,
            } => {
                canonical_uuid(preparation_id)
                    && preparation_id != request_id
                    && digest(binding_sha256)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    Prepared,
    Reserved,
    Settled,
    Refused,
    Unknown,
}

/// Counts and exact original IDs only. Read-only queries cannot supply a path,
/// alter consent, recreate preparation or clear a terminal unknown fence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub abi: String,
    pub configuration: String,
    pub preparation_id: String,
    pub operation_id: String,
    pub revision: u64,
    pub phase: Phase,
    pub binding_sha256: Option<String>,
    pub removed: usize,
    pub kept: usize,
    pub unknown: usize,
    pub drained: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const PREPARE: &str = "11111111-1111-4111-8111-111111111111";
    const OPERATION: &str = "22222222-2222-4222-8222-222222222222";
    #[test]
    fn identities_phases_and_unknown_fields_stay_closed() {
        let mut command = json!({"phase":"prepare","operation_id":OPERATION,
            "configuration":"a".repeat(64),"root":"/srv/example/cache","limit":1,"revision":0});
        let parsed: Command = serde_json::from_value(command.clone()).unwrap();
        assert!(parsed.valid(PREPARE));
        assert!(!parsed.valid(OPERATION));
        command["confirmed"] = json!(true);
        assert!(serde_json::from_value::<Command>(command).is_err());
        let command = Command::Execute {
            preparation_id: PREPARE.into(),
            binding_sha256: "b".repeat(64),
        };
        assert!(command.valid(OPERATION));
        assert!(!command.valid(PREPARE));
    }
}
