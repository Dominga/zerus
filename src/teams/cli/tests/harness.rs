//! A temporary state directory with the preview on, and callers chosen by the
//! test instead of the environment. Seeding goes through the store's own
//! operations, so every reply describes rows the store really wrote.
use super::super::actor::Caller;
use crate::config::Config;
use crate::teams::ops::{
    self, Actor, KeyState, Operation, PersonOrigin, Reply, Request, TaskObservation, TaskStage,
};
use crate::teams::store::Store;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use zerus_team_plan::model::{SessionKey, TeamFixture};

pub(super) const TEAM: &str = "00000000-0000-4000-8000-00000000beef";
pub(super) const TEAM_B: &str = "00000000-0000-4000-8000-00000000beee";
pub(super) const MACHINE: &str = "00000000-0000-4000-8000-0000000000aa";
pub(super) const LEAD_SESSION: &str = "launch_id:00000000-0000-4000-8000-000000000064";
pub(super) const M1_SESSION: &str = "launch_id:00000000-0000-4000-8000-000000000031";
pub(super) const OTHER_SESSION: &str = "run_id:other-example";
pub(super) const NOW: i64 = 1_700_000_000_000;
pub(super) const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

pub(super) fn fixture() -> TeamFixture {
    serde_json::from_str(include_str!(
        "../../../../crates/team-plan/fixtures/team-queue.json"
    ))
    .expect("the shared queue fixture")
}

/// A fixed request ID per seed step, so every reply and fixture is stable.
pub(super) fn uid(step: u32) -> String {
    format!("00000000-0000-4000-8000-{:012x}", 0x9000 + step)
}

pub(super) fn key(session_id: &str) -> SessionKey {
    SessionKey {
        machine: MACHINE.into(),
        session_id: session_id.into(),
    }
}

pub(super) fn person() -> Actor {
    Actor::Person {
        person: "local".into(),
        origin: PersonOrigin::CliInterim,
    }
}

pub(super) fn service() -> Actor {
    Actor::Service {
        person: "local".into(),
        team: TEAM.into(),
    }
}

pub(super) fn member(id: &str, session_id: &str) -> Actor {
    Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: id.into(),
        session: key(session_id),
    }
}

pub(super) fn other() -> Actor {
    Actor::Other {
        person: "local".into(),
        session: key(OTHER_SESSION),
    }
}

pub(super) struct Harness {
    temp: tempfile::TempDir,
    pub config: Config,
    step: std::cell::Cell<u32>,
}

impl Harness {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().expect("a temporary folder");
        let state = temp.path().join("state");
        let dir = temp.path().join("config");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        crate::platform::private_dir(&state.join("teams")).unwrap();
        std::fs::write(state.join("teams/preview"), "on\n").unwrap();
        Self {
            config: Config {
                home: temp.path().display().to_string(),
                dir,
                state,
                selves: vec!["example-host".into()],
                peers: Vec::new(),
                base_peers: Vec::new(),
                peers_from_environment: false,
                machines: BTreeMap::new(),
                tab: false,
                tab_colors: String::new(),
            },
            temp,
            step: std::cell::Cell::new(0),
        }
    }

    pub fn state(&self) -> &Path {
        &self.config.state
    }

    pub fn revision(&self) -> u64 {
        self.revision_of(TEAM)
    }

    pub fn revision_of(&self, team: &str) -> u64 {
        self.store()
            .team(team)
            .unwrap()
            .map_or(0, |team| team.revision)
    }

    pub fn store(&self) -> Store {
        Store::open(&self.config.state).unwrap()
    }

    /// One operation on the first team, at a fixed clock and request ID.
    pub fn apply(&self, actor: &Actor, operation: Operation) -> Reply {
        self.apply_for(TEAM, actor, operation)
    }

    /// One operation on the first team at `now`.
    /// Only the stopped status fixture uses it, made where the service runs.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn apply_at(&self, actor: &Actor, operation: Operation, now: i64) -> Reply {
        let request = self.request_for(TEAM, operation);
        let mut store = self.store();
        ops::apply(&mut store, actor, &request, now).expect("the seed operation succeeds")
    }

    /// One operation on `team`.
    pub fn apply_for(&self, team: &str, actor: &Actor, operation: Operation) -> Reply {
        let request = self.request_for(team, operation);
        let mut store = self.store();
        ops::apply(&mut store, actor, &request, NOW).expect("the seed operation succeeds")
    }

    pub fn request(&self, operation: Operation) -> Request {
        self.request_for(TEAM, operation)
    }

    pub fn request_for(&self, team: &str, operation: Operation) -> Request {
        let step = self.step.get() + 1;
        self.step.set(step);
        Request {
            request_id: uid(step),
            team: team.into(),
            expected_revision: self.revision_of(team),
            operation,
        }
    }

    /// The CLI reply for one command line, with `input` as its stdin. The
    /// preview gate is the production one, so an off switch answers before any
    /// command is looked at.
    pub fn cli(&self, caller: &Caller, line: &str, input: &str) -> (Value, i32) {
        if !crate::teams::enabled(&self.config.state) {
            return crate::teams::inactive();
        }
        let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        super::super::run(&self.config, caller, &words, Some(input))
    }

    pub fn create(&self) {
        self.create_team(TEAM, "Fixes", "zerus/team/fixes");
    }

    /// A second team on this computer, for cross-team reads and `list`.
    pub fn create_team(&self, id: &str, name: &str, branch: &str) {
        let mut team = fixture().team;
        // General CLI fixtures isolate the command protocol from pipelines.
        // Verdict tests construct and complete a real reviewed stage.
        team.bundle.pipelines[0].stages.clear();
        self.apply_for(
            id,
            &person(),
            Operation::Create {
                expected_template: None,
                delegations: None,
                project: team.project,
                name: name.into(),
                goal: team.goal,
                home: team.home,
                bundle: Box::new(team.bundle),
                integration_branch: branch.into(),
                base_commit: team.base_commit,
            },
        );
    }

    /// Binds a member to its session through the service actor, as package 7
    /// does when a launch appears.
    pub fn bind(&self, id: &str, session_id: &str) {
        self.bind_for(TEAM, id, session_id);
    }

    pub fn bind_for(&self, team: &str, id: &str, session_id: &str) {
        let member = fixture()
            .members
            .into_iter()
            .find(|member| member.id == id)
            .expect("the fixture's member");
        let session = key(session_id);
        let service = Actor::Service {
            person: "local".into(),
            team: team.into(),
        };
        self.apply_for(
            team,
            &service,
            Operation::SetMemberSession {
                member: id.into(),
                session,
                account: member.account,
                worktree: member.worktree,
                ownership_nonce: member.ownership_nonce,
            },
        );
    }

    /// A task routed and admitted by the person, so the queue owns it.
    pub fn add(&self, n: u32) -> u32 {
        let node = self
            .apply(
                &person(),
                Operation::AddTask {
                    task: task(n),
                    role: None,
                    pipeline: None,
                },
            )
            .node
            .expect("a new node");
        self.observe(node, TaskStage::Todo);
        node
    }

    /// A task handed to `m1`, worked to Done and reported.
    pub fn reported(&self, n: u32) -> u32 {
        let node = self.add(n);
        let effect = uid(0x8000 + n);
        self.apply(
            &service(),
            Operation::HandoutIntent {
                node,
                member: "m1".into(),
                effect_request_id: effect.clone(),
            },
        );
        self.apply(
            &service(),
            Operation::HandoutOutcome {
                effect_request_id: effect,
                outcome: ops::HandoutOutcome::Submitted {
                    receipt: "receipt-example".into(),
                },
            },
        );
        for stage in [TaskStage::InProgress, TaskStage::InReview, TaskStage::Done] {
            self.observe(node, stage);
        }
        self.apply(
            &service(),
            Operation::RecordReport {
                node,
                generation: 1,
                member: "m1".into(),
                text: "Trimmed the label and added a case for an empty one.".into(),
                commit: COMMIT.into(),
            },
        );
        node
    }

    pub fn observe(&self, node: u32, stage: TaskStage) {
        let previous = self
            .store()
            .task_observation(TEAM, node)
            .unwrap()
            .map_or(String::new(), |observation| observation.revision);
        let observation = TaskObservation {
            revision: format!("token-{node}-{}", stage_name(stage)),
            key: KeyState::Ours,
            previous_revision: previous,
            stage,
            cancelled: false,
            current_cancelled: false,
            rank: format!("a{node:02}"),
            title: format!("Task {node}"),
            attempt: 0,
            attempt_id: String::new(),
            attempt_conflict: false,
        };
        self.apply(&service(), Operation::ObserveTask { node, observation });
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.temp.path());
    }
}

fn stage_name(stage: TaskStage) -> &'static str {
    match stage {
        TaskStage::Backlog => "backlog",
        TaskStage::Todo => "todo",
        TaskStage::InProgress => "in_progress",
        TaskStage::InReview => "in_review",
        TaskStage::Done => "done",
    }
}

pub(super) fn task(n: u32) -> zerus_team_plan::model::TaskRef {
    zerus_team_plan::model::TaskRef {
        project: "example-project".into(),
        uuid: format!("00000000-0000-4000-8000-{:012x}", n),
    }
}

pub(super) fn code(reply: &Value) -> &str {
    reply["error"]["code"].as_str().unwrap_or("")
}

/// The five fixtures' own team: two live nodes, one open card, one open inbox
/// item and one answered question.
pub(super) fn seeded() -> Harness {
    let harness = Harness::new();
    harness.create();
    harness.bind("lead", LEAD_SESSION);
    harness.bind("m1", M1_SESSION);
    harness.add(1);
    let reported = harness.reported(2);
    harness.apply(
        &person(),
        Operation::OpenCard {
            kind: "note".into(),
            text: "Tell the team the release moved to Monday.".into(),
            node: None,
            decisions: None,
                parameters: None,
        },
    );
    let asked = harness.apply(
        &member("m1", M1_SESSION),
        Operation::Ask {
            node: reported,
            generation: 1,
            text: "Should the empty case keep its old width?".into(),
        },
    );
    harness.apply(
        &person(),
        Operation::Reply {
            item: asked.item.expect("the question's inbox item"),
            text: "Keep it, and say so in the commit message.".into(),
        },
    );
    harness
}
