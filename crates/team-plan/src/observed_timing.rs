//! Display-only source observations. These facts grant no execution authority.
//! A task state or delivery is not measured implementation work.
use crate::model::{SessionKey, TaskRef};
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;
pub const MAX_FACTS: usize = 200;
pub const MAX_PAGE_BYTES: usize = 1024 * 1024;
pub const MAX_FACT_BYTES: usize = 8192;
pub const MAX_CURSOR_BYTES: usize = 8192;
pub const MAX_UNIT_KEY_BYTES: usize = 2048;
pub const DEFAULT_WINDOW_MS: u64 = 86_400_000;
pub const MAX_WINDOW_MS: u64 = 31 * DEFAULT_WINDOW_MS;
pub const WATERMARK_MS: i64 = 60_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationKey {
    pub id: String,
    pub draft_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptKey {
    pub id: String,
    pub generation: u64,
    pub fingerprint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Implementation,
    Check,
    Stage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    TeamMember,
    TaskDelivery,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingSnapshot {
    pub kind: BindingKind,
    pub member: String,
    pub session: SessionKey,
    pub run_id: Option<String>,
    pub conversation_id: Option<String>,
    /// A task-delivery fingerprint is not a qualified native incarnation.
    pub native_incarnation: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedUnitKey {
    pub team: String,
    /// The stored team's actual home UUID, never a display alias.
    pub home: String,
    pub node: u32,
    pub task: TaskRef,
    pub kind: UnitKind,
    pub phase: u32,
    pub publication: Option<PublicationKey>,
    pub node_generation: u32,
    pub attempt: Option<AttemptKey>,
    pub effect: Option<String>,
    pub binding: Option<BindingSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalSnapshot {
    pub title: String,
    pub role: Option<String>,
    /// Configured metadata at a verified task-document observation, never native telemetry.
    /// TeamJournal snapshots leave this unknown; they cannot relabel an old event.
    pub configured_model: Option<String>,
    pub native_model: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    DeliveryReserved,
    DeliverySubmitted,
    DeliveryRefused,
    DeliveryUnknown,
    TaskBacklog,
    TaskTodo,
    TaskInProgress,
    TaskInReview,
    TaskDone,
    TaskCancelled,
    ReportRecorded,
    Gap,
    Watermark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NoObservations,
    SourceUnavailable,
    UnsupportedSchema,
    NativeUnsupported,
    OldData,
    Unattributed,
    MissingBinding,
    MissingAttempt,
    PublicationUnavailable,
    UnsequencedSource,
    SampledSource,
    ObserverEpochUnavailable,
    ObserverRestart,
    ClockDiscontinuity,
    TerminalTeam,
    PartialPage,
    ByteCap,
    CohortCap,
    ScopeChanged,
    OffPage,
    UnverifiedHome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    TeamJournal,
    TaskDocument,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePosition {
    pub schema: u32,
    pub kind: SourceKind,
    pub identity: String,
    pub team_revision: u64,
    pub event_id: Option<String>,
    /// Actual source event order. Never filled with a replica byte offset.
    pub event_sequence: Option<u64>,
    pub task_revision: Option<String>,
    pub replica_generation: Option<String>,
    pub replica_offset: Option<u64>,
    pub observer_epoch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingFact {
    pub schema: u32,
    /// Append-only ledger ordinal, distinct from the source event sequence.
    pub seq: u64,
    pub unit: u64,
    pub key: ObservedUnitKey,
    pub snapshot: HistoricalSnapshot,
    pub source: SourcePosition,
    pub category: Category,
    pub event_at_ms: Option<i64>,
    pub observed_at_ms: i64,
    pub last_verified_ms: Option<i64>,
    pub break_before: bool,
    pub reasons: Vec<Reason>,
    /// Unsupported in schema 1; a supplied positive value is invalid.
    pub implementation_observed_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedWindow {
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub window_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedScope {
    pub team: String,
    pub home: String,
    pub phase: u32,
    pub node: Option<u32>,
    /// The selected read's publication, never a replacement for a fact's key.
    pub publication: Option<PublicationKey>,
    pub window: ObservedWindow,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub service_points: bool,
    pub task_points: bool,
    pub implementation_work: bool,
    pub native_work: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            service_points: true,
            task_points: true,
            implementation_work: false,
            native_work: false,
        }
    }
}

/// Stable reference to one full immutable unit key, never merely a node ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedUnitRef {
    pub id: u64,
    pub key: ObservedUnitKey,
}

/// A finite ledger cohort, not whole-phase live membership or completeness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedCohort {
    pub units: Vec<ObservedUnitRef>,
    pub more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedTimingPage {
    pub schema: u32,
    pub scope: ObservedScope,
    pub cohort: ObservedCohort,
    pub stream_id: Option<String>,
    /// Frozen read revision; it does not pretend to order task/native events.
    pub source_revision: u64,
    pub fact_ceiling: u64,
    pub last_verified_ms: Option<i64>,
    pub as_of_ms: Option<i64>,
    pub capabilities: Capabilities,
    pub rows: Vec<TimingFact>,
    pub next: Option<String>,
    pub more: bool,
    pub truncated: bool,
    pub reasons: Vec<Reason>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageKind {
    Sampled,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitCoverage {
    pub key: ObservedUnitKey,
    pub window: ObservedWindow,
    pub last_verified_ms: Option<i64>,
    pub coverage: CoverageKind,
    pub reasons: Vec<Reason>,
    pub supported_categories: Vec<Category>,
    pub implementation_observed_ms: Option<u64>,
}
