//! The Rust core of the Zerus desktop client: one `hgs [@host] watch --json`
//! per computer, decoded with `zerus-watch-protocol`, kept as catalog and
//! Activity state and handed out as keyed [`Change`]s that a [`Coalescer`]
//! folds into one [`Batch`] per display frame.
//!
//! Pure Rust with no Tauri or async runtime, so it tests fast. See
//! `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md` (Tasks 3 and 4)
//! and the "Event stream" section of `docs/reference.md`.

pub mod batch;
pub mod fleet;
pub mod state;
pub mod stream;
pub mod view;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub use batch::{ActivityOut, Batch, Coalescer, ComputerOut, KeyOut, ReplaceOut, RowOut};
pub use fleet::{Fleet, FollowError};
pub use state::{ActivityState, CatalogState};
pub use stream::{StreamEvent, StreamHandle, Timing};
pub use view::ViewRow;
pub use zerus_watch_protocol::SessionRef;

/// A computer by the alias `hgs @alias` takes; the empty string is this
/// computer. Peers are never keyed by the host name their stream reports.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComputerId(pub String);

impl ComputerId {
    /// This computer.
    pub fn local() -> Self {
        Self(String::new())
    }

    pub fn is_local(&self) -> bool {
        self.0.is_empty()
    }

    /// The peer's alias, None for this computer.
    pub fn alias(&self) -> Option<&str> {
        (!self.0.is_empty()).then_some(self.0.as_str())
    }
}

impl From<&str> for ComputerId {
    fn from(alias: &str) -> Self {
        Self(alias.to_owned())
    }
}

/// The connection state of one computer's stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComputerStatus {
    /// Checking `hgs [@host] -h` for the watch-v1 marker before the first stream.
    Probing,
    /// Its CLI lacks the marker: no stream runs; it is probed again every 10 min.
    Unsupported,
    /// A stream delivered its catalog snapshot.
    Live,
    /// The stream ended, failed or stayed silent; `attempt` counts the failures
    /// that set the restart delay (5 s, 15 s, 60 s, then 5 min). Its rows stay
    /// as last known.
    Reconnecting { attempt: u32 },
    /// The computer left the fleet or the fleet stopped.
    Ended,
}

impl ComputerStatus {
    /// The snake_case name a [`Batch`] carries.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Probing => "probing",
            Self::Unsupported => "unsupported",
            Self::Live => "live",
            Self::Reconnecting { .. } => "reconnecting",
            Self::Ended => "ended",
        }
    }

    pub fn attempt(&self) -> u32 {
        match self {
            Self::Reconnecting { attempt } => *attempt,
            _ => 0,
        }
    }
}

/// One change of a followed session's Activity.
#[derive(Clone, Debug, PartialEq)]
pub enum ActivityUpdate {
    /// Events after the previous cursor, oldest first.
    Events { events: Vec<Value>, cursor: i64 },
    /// A new event window that replaces what the view holds (a tail snapshot
    /// after a gap, or a DeepSeek or OpenCode window).
    Replace { events: Vec<Value>, cursor: i64 },
    /// A replacement of one open event by its stable ID; a higher `rev` wins.
    /// The core merges activity `event` frames into [`ActivityUpdate::State`]
    /// instead; this variant stays for consumers of later frame kinds.
    Event { id: String, rev: u64, event: Value },
    /// The session's latest inspection summary, with provider messages merged.
    State { value: Value, rev: u64 },
    /// The subscription ended (the session left the catalog or changed run).
    Ended,
}

/// One keyed change for the view.
// Upserts dominate and carry their row inline, as the plan's interface fixes;
// boxing it would allocate twice per upsert for no gain.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Upsert {
        computer: ComputerId,
        key: String,
        row: ViewRow,
    },
    /// `reason` is one of `replaced`, `archived`, `forgotten` or `ended`; an
    /// unknown or missing reason is None. The row leaves the list either way.
    Remove {
        computer: ComputerId,
        key: String,
        reason: Option<String>,
    },
    /// All rows of the computer, replacing what the view holds, sorted by key.
    Replace {
        computer: ComputerId,
        rows: Vec<(String, ViewRow)>,
    },
    Computer {
        computer: ComputerId,
        status: ComputerStatus,
    },
    Activity {
        computer: ComputerId,
        key: String,
        update: ActivityUpdate,
    },
}

impl Change {
    pub fn computer(&self) -> &ComputerId {
        match self {
            Self::Upsert { computer, .. }
            | Self::Remove { computer, .. }
            | Self::Replace { computer, .. }
            | Self::Computer { computer, .. }
            | Self::Activity { computer, .. } => computer,
        }
    }
}

/// The `hgs` the client runs: `ZERUS_CLIENT_HGS` when set, otherwise `hgs` on
/// `PATH`.
pub fn default_hgs() -> PathBuf {
    std::env::var_os("ZERUS_CLIENT_HGS")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("hgs"))
}
