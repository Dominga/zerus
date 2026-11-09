//! Which running connector a Windows Stop may address: only the exact child of a
//! full supported Start. A service wrapper or scheduled task can run a fixed command
//! line and restart it behind a Stop, and a config revision is a content digest any
//! launch can carry, so neither that flag nor a matching path proves anything. The
//! receipt comes from the Start that owns the child's process handle, and names that
//! one process instance, so no launch can mint or replay it.
//!
//! An external manager that deliberately runs the full supported Start can still
//! restart the connector. This does not claim to prevent that.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use {
    crate::{
        error::{ConnectorError, Result},
        private_state::Store,
        workspace_cli,
    },
    zerus_windows_private::{OriginalProcess, ProcessIdentity},
};

pub const RECEIPT: &str = "start-receipt.json";
const VERSION: u32 = 1;

/// What a Start launched, and for exactly which configuration, profile and state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u32,
    pub pid: u32,
    pub birth: u64,
    pub image: String,
    pub revision: String,
    pub config: PathBuf,
    pub state_dir: PathBuf,
    pub node_id: String,
}

/// The configuration a Start or Stop is working on.
pub struct Target<'a> {
    pub revision: &'a str,
    pub config: &'a Path,
    pub state_dir: &'a Path,
    pub node_id: &'a str,
}

#[cfg(windows)]
fn unavailable() -> ConnectorError {
    ConnectorError::new("This connector's Start can't be confirmed")
}

impl Receipt {
    /// The control record is published by the running connector itself, so a
    /// generation in it belongs to the process named beside it.
    pub fn admits(&self, target: &Target<'_>, control: &Value) -> bool {
        self.version == VERSION
            && self.revision == target.revision
            && self.config == target.config
            && self.state_dir == target.state_dir
            && self.node_id == target.node_id
            && control["pid"].as_u64() == Some(u64::from(self.pid))
            && control["birth"].as_u64() == Some(self.birth)
    }
}

/// Only a Start calls this, under its lifecycle lock, with its own child's identity.
#[cfg(windows)]
pub fn record_start(lifecycle: &Store, target: &Target<'_>, child: &ProcessIdentity) -> Result<()> {
    let receipt = Receipt {
        version: VERSION,
        pid: child.pid,
        birth: child.birth,
        image: child.image.clone(),
        revision: target.revision.into(),
        config: target.config.into(),
        state_dir: target.state_dir.into(),
        node_id: target.node_id.into(),
    };
    let bytes = serde_json::to_vec(&receipt).map_err(|_| unavailable())?;
    let previous = lifecycle.read(RECEIPT).map_err(|_| unavailable())?;
    lifecycle
        .replace(RECEIPT, &Store::revision(previous.as_deref()), &bytes)
        .map_err(|_| unavailable())?;
    Ok(())
}

/// The held process this Stop may address. Reads the atomic receipt without the
/// lifecycle lock, because a Start can't be replacing it while this one runs.
#[cfg(windows)]
pub fn admit(lifecycle: &Path, target: &Target<'_>, control: &Value) -> Result<OriginalProcess> {
    let bytes = workspace_cli::read_private(lifecycle, RECEIPT)?.ok_or_else(unavailable)?;
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    if !receipt.admits(target, control) {
        return Err(unavailable());
    }
    OriginalProcess::open(&ProcessIdentity {
        pid: receipt.pid,
        birth: receipt.birth,
        image: receipt.image,
    })
    .map_err(|_| unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn receipt() -> Receipt {
        Receipt {
            version: 1,
            pid: 41,
            birth: 134_000_000_000_000_001,
            image: "connector.exe".into(),
            revision: "r".into(),
            config: "config.json".into(),
            state_dir: "state".into(),
            node_id: "node".into(),
        }
    }
    fn target() -> Target<'static> {
        Target {
            revision: "r",
            config: Path::new("config.json"),
            state_dir: Path::new("state"),
            node_id: "node",
        }
    }
    fn control() -> Value {
        json!({"version":1,"generation":"g","pid":41,"birth":134_000_000_000_000_001_u64})
    }

    #[test]
    fn the_exact_start_of_this_configuration_is_admitted() {
        assert!(receipt().admits(&target(), &control()));
    }

    #[test]
    fn a_creation_time_too_large_for_a_float_survives_the_files_exactly() {
        let bytes = serde_json::to_vec(&receipt()).unwrap();
        assert_eq!(
            serde_json::from_slice::<Receipt>(&bytes).unwrap(),
            receipt()
        );
        let published: Value = serde_json::from_slice(control().to_string().as_bytes()).unwrap();
        assert!(receipt().admits(&target(), &published));
        let mut off_by_one = published;
        off_by_one["birth"] = json!(134_000_000_000_000_002_u64);
        assert!(!receipt().admits(&target(), &off_by_one));
    }

    #[test]
    fn another_process_or_a_control_without_a_birth_is_refused() {
        for (key, value) in [
            ("pid", json!(42)),
            ("birth", json!(1)),
            ("pid", Value::Null),
        ] {
            let mut changed = control();
            changed[key] = value;
            assert!(!receipt().admits(&target(), &changed), "{changed}");
        }
        let mut older = control();
        older.as_object_mut().unwrap().remove("birth");
        assert!(!receipt().admits(&target(), &older));
    }

    #[test]
    fn another_configuration_profile_or_state_is_refused() {
        let cases = [
            Target {
                revision: "other",
                ..target()
            },
            Target {
                config: Path::new("other.json"),
                ..target()
            },
            Target {
                state_dir: Path::new("other"),
                ..target()
            },
            Target {
                node_id: "other",
                ..target()
            },
        ];
        for case in cases {
            assert!(!receipt().admits(&case, &control()));
        }
        let mut unknown = receipt();
        unknown.version = 2;
        assert!(!unknown.admits(&target(), &control()));
    }

    #[test]
    fn a_receipt_with_anything_unexpected_does_not_parse() {
        let mut value = serde_json::to_value(receipt()).unwrap();
        value["extra"] = json!(true);
        assert!(serde_json::from_value::<Receipt>(value).is_err());
    }
}
