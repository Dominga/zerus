use serde::{Deserialize, Serialize};
use zerus_team_plan::{catalog::TemplateBundle, model::*};

/// Construct only after the caller's native authority has been verified.
/// This type deliberately cannot be deserialized from an operation request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    Person {
        person: String,
        origin: PersonOrigin,
    },
    Session {
        person: String,
        team: String,
        member: String,
        session: SessionKey,
    },
    Other {
        person: String,
        session: SessionKey,
    },
    /// Internal observer/recorder, never an authority available over the CLI.
    Service {
        person: String,
        team: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonOrigin {
    DesktopUi,
    CliInterim,
}

impl Actor {
    pub fn person(&self) -> &str {
        match self {
            Self::Person { person, .. }
            | Self::Session { person, .. }
            | Self::Other { person, .. }
            | Self::Service { person, .. } => person,
        }
    }
}

/// The task's team/home key as read by the trusted observer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    #[default]
    Ours,
    Absent,
    Other,
}
impl KeyState {
    fn is_ours(&self) -> bool {
        *self == Self::Ours
    }
}

/// The task writer's authoritative snapshot, not an independently editable
/// node state. Revisions are the task document's opaque tokens. Each update
/// compares the previously projected token so delayed observations cannot
/// overwrite a newer projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskObservation {
    pub revision: String,
    #[serde(default, skip_serializing_if = "KeyState::is_ours")]
    pub key: KeyState,
    #[serde(default)]
    pub previous_revision: String,
    pub stage: TaskStage,
    #[serde(default)]
    pub cancelled: bool,
    /// The task's current attempt is in phase Cancelled: the cancel this
    /// team owns, read from the attempt pair after rule 1 cleared the mark
    /// (zerus-63rru.36). The node whose effect matches the attempt's ID
    /// ends on it; a newer node for the same task does not. Carried in
    /// memory beside the observation; never stored, so the stored JSON
    /// keeps the base field set (round 3).
    #[serde(default, skip)]
    pub current_cancelled: bool,
    pub rank: String,
    #[serde(default)]
    pub title: String,
    /// The highest generation any attempt of the task claims; 0 without one.
    #[serde(default)]
    pub attempt: u64,
    /// The ID of the task's current attempt; empty without one. An attempt
    /// the team hands out has its effect's request ID. At most 256 bytes.
    #[serde(default)]
    pub attempt_id: String,
    /// Two attempts claim the highest generation; the ID is then empty. An
    /// empty ID without this flag is an ordinary pair: no current attempt.
    #[serde(default)]
    pub attempt_conflict: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStage {
    Backlog,
    Todo,
    InProgress,
    InReview,
    Done,
}

/// A hand-out's or reopen's outcome. `HandedBack` is a final refusal handed
/// to the lead at once, never retried: a reopen whose conversation changed,
/// or a hosted hand-out refused as unsafe, canceled or dropped after claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum HandoutOutcome {
    Submitted { receipt: String },
    Refused { reason: String },
    CuttingOver { reason: String },
    Uncertain { reason: String },
    HandedBack { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum StageTaskOutcome {
    Created { task: TaskRef },
    Refused { reason: String },
    Uncertain { reason: String },
}

/// A service timer of package 10; each fires at most once per key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimerKind {
    NoReport,
    NotStarted,
    RemindLead,
    RemindPerson,
    SilentLead,
}

/// The final answer of the team-clear child for one task key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyClearState {
    Cleared,
    Refused,
}

/// The service's record of a cancel decision it carried out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelState {
    /// The task was cancelled under that attempt. In a live team a bound
    /// task's end is recorded by its observation, so there this answer is
    /// refused; it ends a node without a task and settles after a stop.
    Cancelled,
    Refused,
    /// The key was already gone from the task: in every team state the
    /// answer is recorded and the node's binding goes with it.
    Released,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Create {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_template: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delegations: Option<Delegations>,
        project: String,
        name: String,
        goal: String,
        home: String,
        bundle: Box<TemplateBundle>,
        integration_branch: String,
        base_commit: String,
    },
    SetMemberSession {
        member: String,
        session: SessionKey,
        account: String,
        worktree: String,
        ownership_nonce: String,
    },
    /// A launch-bound member's durable handle: the session ID of the record
    /// its binding verified (zerus-63rru.53). Written once, by the service.
    SetMemberHandle {
        member: String,
        session_id: String,
        #[serde(default)]
        label_hint: String,
    },
    SetMemberState {
        member: String,
        state: MemberState,
    },
    Pause,
    Resume,
    Stop,
    Archive,
    PauseMember {
        member: String,
    },
    ResumeMember {
        member: String,
    },
    StopMember {
        member: String,
    },
    ArchiveMember {
        member: String,
    },
    AddTask {
        task: TaskRef,
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        pipeline: Option<String>,
    },
    Admit {
        node: NodeId,
    },
    Route {
        node: NodeId,
        role: String,
        pipeline: String,
        #[serde(default)]
        deps: Vec<Dep>,
        #[serde(default)]
        writes: Vec<String>,
        #[serde(default)]
        assignee: Option<MemberId>,
    },
    Hold {
        node: NodeId,
    },
    Release {
        node: NodeId,
    },
    Cancel {
        node: NodeId,
        reason: String,
    },
    HandoutIntent {
        node: NodeId,
        member: String,
        effect_request_id: String,
    },
    HandoutOutcome {
        effect_request_id: String,
        outcome: HandoutOutcome,
    },
    ReopenIntent {
        node: NodeId,
        member: MemberId,
        effect_request_id: String,
    },
    ReopenOutcome {
        effect_request_id: String,
        outcome: HandoutOutcome,
    },
    ObserveTask {
        node: NodeId,
        observation: TaskObservation,
    },
    RecordReport {
        node: NodeId,
        generation: u32,
        member: MemberId,
        text: String,
        commit: String,
    },
    StageTaskIntent {
        node: NodeId,
        effect_request_id: String,
    },
    StageTaskOutcome {
        effect_request_id: String,
        outcome: StageTaskOutcome,
    },
    Verdict {
        node: NodeId,
        generation: u32,
        pass: bool,
        text: String,
    },
    Accept {
        node: NodeId,
    },
    RecordIntegration {
        node: NodeId,
        decision: u64,
        commit: String,
    },
    RecordIntegrationConflict {
        node: NodeId,
        decision: u64,
        conflicts: Vec<String>,
    },
    Return {
        node: NodeId,
        reason: String,
    },
    Reassign {
        node: NodeId,
        member: String,
        reason: String,
    },
    Ask {
        node: NodeId,
        generation: u32,
        text: String,
    },
    Reply {
        item: u64,
        text: String,
    },
    OpenCard {
        kind: String,
        text: String,
        #[serde(default)]
        node: Option<NodeId>,
        /// The decisions the card takes: at most four short tokens of
        /// lower-case ASCII and underscores. Without it, acknowledged alone.
        /// Left out when absent, so that a request saved before the list
        /// existed replays byte for byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decisions: Option<Vec<String>>,
        /// The bounded parameters a localized window substitutes: at most 8
        /// keys of [a-z_], each value at most 256 characters after the
        /// sentence's rules (zerus-63rru.34). Left out when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameters: Option<Vec<(String, String)>>,
    },
    CloseCard {
        card: u64,
        reason: String,
    },
    DecideCard {
        card: u64,
        decision: String,
    },
    ChangeSettings {
        settings: Settings,
    },
    FireTimer {
        timer: TimerKind,
        node: NodeId,
        generation: u32,
        #[serde(default)]
        item: u64,
        #[serde(default)]
        since_ms: i64,
    },
    RecordKeyClear {
        node: NodeId,
        state: KeyClearState,
        #[serde(default)]
        reason: String,
    },
    RecordLateSession {
        member: MemberId,
        launch_id: String,
        session: SessionKey,
        name: String,
    },
    RecordCancel {
        node: NodeId,
        decision: u64,
        state: CancelState,
        #[serde(default)]
        reason: String,
        /// The task attempt the team-cancel child acted on: its generation
        /// and its ID (at most 256 bytes).
        #[serde(default)]
        attempt: u64,
        #[serde(default)]
        attempt_id: String,
    },
    /// A report that a removed member wrote after its removal took the task
    /// back (zerus-63rru.77): not recorded as the team's, told to the lead
    /// once per node and generation.
    RecordLateReport {
        node: NodeId,
        generation: u32,
        member: MemberId,
    },
    /// The service saw the kernel's unstaffed nodes differ from the lead's
    /// open items. The store recomputes the set itself, from its own view, and
    /// tells the lead of each new node once and closes the items of the nodes
    /// that left it; an unstaffed stage is retired and its parent handed to the
    /// lead, as at the stage's creation. A call that finds nothing to change is
    /// refused with `unstaffed_unchanged`.
    ReconcileUnstaffed,
    /// The service saw unended unstaffed items older than their node's
    /// newest one: stale items an earlier layout left, which stand for no
    /// episode. It ends at most a page of them, oldest first, in its own
    /// operation, before the reconcile judges the cap on the current
    /// episodes. A call that finds none is refused with
    /// `unstaffed_unchanged`.
    EndStaleUnstaffed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Request {
    pub request_id: String,
    pub team: String,
    pub expected_revision: u64,
    #[serde(flatten)]
    pub operation: Operation,
}

// Splitting the envelope explicitly keeps unknown operation fields strict;
// serde's flattened fields and deny_unknown_fields cannot be combined.
impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut fields = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        let mut take = |key: &str| {
            fields.remove(key).ok_or_else(|| {
                D::Error::missing_field(match key {
                    "request_id" => "request_id",
                    "team" => "team",
                    _ => "expected_revision",
                })
            })
        };
        let request_id = serde_json::from_value(take("request_id")?).map_err(D::Error::custom)?;
        let team = serde_json::from_value(take("team")?).map_err(D::Error::custom)?;
        let expected_revision =
            serde_json::from_value(take("expected_revision")?).map_err(D::Error::custom)?;
        let operation: Operation =
            serde_json::from_value(serde_json::Value::Object(fields.clone()))
                .map_err(D::Error::custom)?;
        // Internally tagged unit variants otherwise ignore leftover fields.
        let known = serde_json::to_value(&operation).map_err(D::Error::custom)?;
        if let Some(unknown) = fields.keys().find(|key| known.get(*key).is_none()) {
            return Err(D::Error::custom(format!("unknown field `{unknown}`")));
        }
        Ok(Self {
            request_id,
            team,
            expected_revision,
            operation,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub ok: bool,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u64>,
}
