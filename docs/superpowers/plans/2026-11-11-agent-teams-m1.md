# Agent Teams M1 (queue team) implementation plan

> **For agentic workers:** this plan is executed package by package. One
> implementer session owns one package in its own worktree; an independent
> reviewer checks it; the coordinating session merges it into
> `integrate/teams`. Work test-first. Read the specification before your
> package.

**Goal:** a person creates a standing team from a template and sends it tasks;
each task runs through the team's pipeline and accepted work lands on one
integration branch, on one computer, behind a setting that is off by default.

**Architecture:** a pure crate (`crates/team-plan`) holds the types and every
decision about what may run. `src/teams` holds the store, the operations, the
catalog, integration and the CLI. A teams actor in the desktop service
(`src/desktop/teams`) causes every effect in sessions through existing service
actors. The desktop reads typed projections from `client/core/src/teams`.

**Tech stack:** Rust 1.99 (`cargo +1.99.1`), SQLite through `src/sqlite_file`,
serde and serde_json, Git plumbing, SolidJS and vitest in `client/`.

**Specification:** `docs/superpowers/specs/2026-11-11-agent-teams-design.md`.
Epic: `zerus-63rru`.

## Global constraints

- Teams are inactive unless the preview setting is on. With it off there is no
  store file, `hgs team` answers `teams_inactive`, the desktop shows no entry
  point, and no code path of an ordinary session changes.
- A team task is a Zerus task. The replicated task schema is not raised and no
  new `Target` variant is added. The only replica change is the optional
  `team` key in a task's field map (package 6).
- Only `crates/team-plan` decides what may run. Only the teams actor causes
  effects in sessions, and only through existing service actors. An operation
  in `src/teams/ops` never touches a session.
- An intent is recorded before an effect and its outcome after. An unknown
  outcome is never retried.
- Session identity is the computer UUID with `session_id` or `launch_id`.
  A session name is never used to identify a session.
- Every decision, card and journal entry records an actor with a person
  identifier (`"local"` today). Authority is checked in one function.
- Reads are bounded: rows and pages, never a whole team in one value.
- Zerus executes nothing that a repository or a template declares.
- Integration never runs `git reset`, never writes a checkout and never
  pushes.
- The repository language is English. No address, home path or absolute
  workspace path in source, tests or tracker text. Fixtures use synthetic paths
  and reserved example domains.
- The desktop UI never separates values with dots
  (`scripts/check-ui-separators.py`), and the client entry bundle stays under
  500,000 bytes (`client/scripts/check-bundle-size.py`).
- Credit: a file imported unchanged from `WhiteWh/zerus` at `ca4cc4481` is
  committed with its fork author as the Git author (take the identity from
  `git log -1 --format='%an <%ae>' whitewh/whitewh/main -- <path>`), with a
  message naming the source commit and path. Its adaptation is a second,
  ordinary commit. New code carries no co-author trailer for the fork.

## Rules for every package

1. Work only in your own worktree and branch, created from `integrate/teams`.
   Change only the files your package owns; if you need a change elsewhere,
   say so in your report instead of making it.
2. Tests first: write the failing test, see it fail, implement, see it pass.
3. Commit locally in small commits. The repository owner explicitly authorized
   local commits for this work on 2026-11-11; pushing and merging stay with
   the coordinating session. Never push.
4. Run, for each crate you touched, with `--manifest-path` and never at the
   repository root: `cargo +1.99.1 fmt --check`, `cargo +1.99.1 clippy
   --locked --all-targets -- -D warnings`, `cargo +1.99.1 test --locked`. For
   the `hgs` binary run only the tests of your module (`cargo +1.99.1 test
   --locked --bin hgs teams::`), not the whole suite. After changing a
   `crates/*` API also run `cargo +1.99.1 check --locked --bin hgs`.
5. The computer is shared. Keep build output inside your worktree, use at most
   eight build jobs (`-j 8`), start no stress loops, and do not use the Mac or
   the Windows computer.
6. Do not start, stop, kill or message sessions other than the coordinating
   session. Do not run `hgs` commands that change sessions. Tests that need
   `hgs` state use a temporary `HGS_CONFIG_DIR` and `HGS_STATE_DIR`.
7. Report to the coordinating session with: the branch and head commit, what
   you built, the exact commands you ran with pass and fail counts, the path
   and SHA-256 of the raw test log inside your worktree, and what is not done
   or uncertain. A result without a log counts as unverified.

## Review focus

Conditions the specification implies and that are most likely to hurt a
person using Teams. Each is pinned by a test in the package named.

1. The desktop service restarts between recording a hand-out intent and its
   outcome: the task is not sent again and the lead gets an inbox item
   (package 8).
2. Two tasks are ready and one member is free: exactly one is handed out, and
   a member with a task in progress outside the team is not free (packages 2
   and 8).
3. A task is sent to the team twice, or with the same request ID: one plan
   node exists (packages 3 and 6).
4. A reviewer is the only member of its role and it implemented the task: the
   stage is not handed to it, and the task goes to the lead with the reason
   (packages 2 and 9).
5. The integration ref moved between computing a merge and updating the ref:
   nothing is lost and the merge is computed again; a conflicting merge
   changes no ref and no reachable object (package 11).

## File structure

| Path | Responsibility | Package |
|---|---|---|
| `crates/team-plan/src/lib.rs` | Re-exports | 1 |
| `crates/team-plan/src/model.rs` | Team, member, phase, node and settings types | 1 |
| `crates/team-plan/src/catalog.rs` | Role, seat, pipeline, template, catalog, bundle types | 1 |
| `crates/team-plan/src/problem.rs` | `Problem` and the error codes | 1 |
| `crates/team-plan/src/graph.rs` | Cycles, layers, dependants (imported kernel) | 2 |
| `crates/team-plan/src/ready.rs` | Readiness, hand-out order, member choice | 2 |
| `crates/team-plan/src/overlap.rs` | Write-set overlap | 2 |
| `crates/team-plan/src/pipeline.rs` | Stage progression | 2 |
| `crates/team-plan/src/accounts.rs` | Choosing an account | 2 |
| `crates/team-plan/src/validate.rs` | Catalog and bundle validation | 4 |
| `crates/team-plan/src/builtin.rs` | The built-in catalog | 4 |
| `crates/team-plan/fixtures/*.json` | Shared fixtures | 1, then their owners |
| `src/teams/mod.rs` | Module list and the preview setting | 1 |
| `src/teams/store/` | SQLite store | 3 |
| `src/teams/ops/` | Operations and the authority function | 3, 9, 10 |
| `src/teams/catalog.rs` | A person's catalog on disk, save, import, export | 4 |
| `src/teams/integrate.rs` | Git plumbing for the integration branch | 11 |
| `src/teams/cli.rs` | `hgs team` | 5 |
| `src/agent_guide/team.md`, `team-lead.md` | Guide pages | 5 |
| `src/desktop/teams/` | Member launch, teams actor, standing confirmations | 7, 8, 17 |
| `client/core/src/teams/` | Projections and actions | 12 |
| `client/src/ui/teams/`, `client/src/store/teams*.ts` | Frontend | 13, 14 |

## Contracts (package 1 materializes these exactly)

All types derive `Clone, Debug, PartialEq, Serialize, Deserialize`. Structs
use `#[serde(deny_unknown_fields)]`; enums use `#[serde(rename_all =
"snake_case")]`. Optional and empty fields are skipped when serialized and
default when absent.

```rust
// crates/team-plan/src/model.rs
pub type TeamId = String;    // lowercase UUID
pub type MemberId = String;  // "lead", "m1", "m2", unique in a team
pub type NodeId = u32;       // counter per team, starts at 1
pub type PersonId = String;  // "local" today

pub struct TaskRef { pub project: String, pub uuid: String }
pub struct SessionKey {
    pub machine: String,          // computer UUID
    pub session_id: String,       // "launch_id:<uuid>" or "run_id:<id>"
}

pub enum TeamStatus { Draft, Planning, Running, Paused, Finished, Stopped, Archived }
pub enum MemberState { NotCreated, Active, Paused, Removed }
pub enum PhaseKind { Plan, Queue }
pub enum PhaseMode { Standard, Free }
pub enum PhaseState { Draft, Approved, AwaitingAcceptance, Done, Rejected }
pub enum Approval { Review, Auto }
pub enum Acceptance { Lead, Person, Pipeline }
pub enum DepKind { Hard, Soft }
pub enum NodeKind { Task, Check, Stage }
pub enum Severity { None, Crit, Minor }
pub enum NodeSource { Planned, Sent }
pub enum NodeState { Draft, Waiting, Pending, InProgress, Reported, Accepted, Cancelled }
pub enum Decider { Lead, Person, Pipeline }
pub enum Routing { Direct, Lead }
pub enum Integration { Branch, None }
pub enum ContextMode { Keep, Fresh }

pub struct Delegations {
    pub accept_results: bool, pub answer_members: bool, pub confirm_trust: bool,
    pub resume_exited: bool, pub merge_accepted: bool,
}                                  // Default: all true
pub struct Caps {
    pub max_members: u32,          // 6
    pub max_phase_tasks: u32,      // 50
    pub max_open_tasks: u32,       // 500
    pub max_open_questions: u32,   // 3
}
pub struct Timers {
    pub report_timeout_seconds: u32,   // 120
    pub start_timeout_seconds: u32,    // 180
    pub inbox_interval_seconds: u32,   // 120
    pub remind_lead_minutes: u32,      // 15
    pub remind_person_minutes: u32,    // 60
    pub lead_stall_minutes: u32,       // 20
    pub answer_escalate_minutes: u32,  // 10
    pub resume_limit_per_hour: u32,    // 3
}
pub struct Settings {
    pub delegations: Delegations, pub caps: Caps, pub timers: Timers,
    pub queue: bool,                 // true
    pub routing: Routing,            // Direct
    pub plan_approval: Approval,     // Review
    pub phase_acceptance: Acceptance,// Lead; Pipeline is refused here
    pub integration: Integration,    // Branch
    pub land_allowed: bool,          // false
}

pub struct Team {
    pub id: TeamId, pub project: String, pub name: String, pub goal: String,
    pub home: String,                // computer UUID
    pub owner: PersonId, pub status: TeamStatus, pub lead: MemberId,
    pub settings: Settings, pub bundle: TemplateBundle,
    pub integration_branch: String, pub base_commit: String, pub revision: u64,
}
pub struct Member {
    pub id: MemberId, pub role: String, pub agent: String, pub account: String,
    pub state: MemberState, pub session: Option<SessionKey>,
    pub worktree: String, pub ownership_nonce: String,
}
pub struct Phase {
    pub n: u32, pub order: u32, pub title: String, pub kind: PhaseKind,
    pub mode: PhaseMode, pub approval: Approval, pub state: PhaseState,
    pub acceptance: Acceptance, pub barrier: bool, pub paused: bool,
}
pub struct Dep { pub node: NodeId, pub kind: DepKind }
pub struct Node {
    pub id: NodeId, pub phase: u32, pub task: Option<TaskRef>,
    pub title: String, pub text: String,      // text only while a draft
    pub source: NodeSource, pub deps: Vec<Dep>,
    pub assignee: Option<MemberId>, pub role: String, pub pipeline: String,
    pub writes: Vec<String>, pub estimate_minutes: u32,
    pub kind: NodeKind, pub severity: Severity, pub origin: Option<NodeId>,
    pub stage_of: Option<NodeId>,   // the implementing node, for a stage
    pub stage_index: u32,           // position in the pipeline, for a stage
    pub round: u32,                 // 1 on the first pass
    pub implementer: Option<MemberId>,
    pub hold: bool, pub admitted: bool,
    pub state: NodeState,           // derived by the store from the task
    pub decided_by: Option<Decider>, pub integrated: bool,
    pub rank: String,               // the task's board rank; queue order
}
```

`NodeState` is derived, never stored as truth: `Draft` has no task;
`Waiting` is a queue node that is not admitted or not routed yet; `Pending`
is To do; `InProgress` is In progress; `Reported` is In review; `Accepted` is
Done; `Cancelled` is a cancelled task.

```rust
// crates/team-plan/src/catalog.rs
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AccountChoice { Freest, Named { account: String } }   // Default: Freest
pub enum StageKind { Review, Test, Land }
pub struct Role {
    pub id: String, pub name: String, pub prompt: String,
    pub agent: String,              // "claude", "codex", "kimi", "dsh", "opencode"
    pub account: AccountChoice, pub model: String, pub effort: String,
    pub pipeline: String,           // empty: the template's default
    pub context: ContextMode,       // Keep
}
pub struct Seat { pub role: String, pub count: u32, pub optional: bool }
pub struct Stage { pub kind: StageKind, pub role: String }
pub struct Pipeline {
    pub id: String, pub name: String, pub implementer: String, // a role id
    pub stages: Vec<Stage>, pub max_rounds: u32,               // 0 reads as 3
    pub accept: Acceptance,
}
pub struct LeadSpec {
    pub agent: String, pub account: AccountChoice,
    pub model: String, pub effort: String, pub rules: String,
}
pub struct Template {
    pub id: String, pub name: String, pub description: String,
    pub lead: LeadSpec, pub roster: Vec<Seat>, pub default_pipeline: String,
    pub settings: Settings,
}
pub struct Catalog {
    pub roles: Vec<Role>, pub pipelines: Vec<Pipeline>,
    pub templates: Vec<Template>, pub planning_rules: Vec<String>,
}
pub struct TemplateBundle {         // an exported template; a team's copy
    pub template: Template, pub roles: Vec<Role>, pub pipelines: Vec<Pipeline>,
}
```

```rust
// crates/team-plan/src/problem.rs
pub struct Problem { pub code: String, pub message: String, pub refs: Vec<String> }
```

Error codes, stable strings: `teams_inactive`, `not_found`,
`stale_revision`, `not_allowed`, `invalid_state`, `unknown_role`,
`unknown_pipeline`, `unknown_member`, `unknown_account`,
`no_account_available`, `no_seat_for_stage`, `dependency_cycle`,
`over_cap`, `duplicate`, `unknown_field`, `integration_conflict`,
`integration_unavailable`, `delivery_uncertain`.

Functions other packages rely on (package 2 unless marked):

```rust
// graph.rs
pub fn cycles(nodes: &[Node]) -> Vec<Vec<NodeId>>;
pub fn layers(nodes: &[Node]) -> BTreeMap<NodeId, u32>;
pub fn dependants(nodes: &[Node]) -> BTreeMap<NodeId, u32>;   // transitive count
// overlap.rs
pub fn overlaps(a: &[String], b: &[String]) -> bool;
// ready.rs
pub struct MemberView { pub id: MemberId, pub role: String, pub state: MemberState, pub free: bool }
pub struct PlanView<'a> {
    pub phases: &'a [Phase], pub nodes: &'a [Node],
    pub members: &'a [MemberView], pub lead: &'a str,
    pub integration: Integration,
}
pub struct Handout { pub node: NodeId, pub member: MemberId }
pub fn ready(view: &PlanView) -> Vec<NodeId>;
pub fn handouts(view: &PlanView) -> Vec<Handout>;   // at most one per member
// pipeline.rs
pub enum Step {
    Stage { index: u32, kind: StageKind, role: String },
    ToLead { reason: String }, ToPerson, Accept,
    ReturnToImplementer { round: u32 },
}
pub fn after_report(node: &Node, pipeline: &Pipeline) -> Step;
pub fn after_verdict(node: &Node, stage: &Node, pass: bool, pipeline: &Pipeline) -> Step;
// accounts.rs
pub struct UsageWindow { pub used_percent: f64, pub resets_at: Option<i64> }
pub struct AccountUsage {
    pub id: String, pub agent: String, pub signed_in: bool,
    pub windows: Vec<UsageWindow>,
}
pub fn pick_account(agent: &str, choice: &AccountChoice, accounts: &[AccountUsage],
    taken: &BTreeMap<String, u32>) -> Result<String, Problem>;
// validate.rs, builtin.rs (package 4)
pub fn validate_catalog(catalog: &Catalog) -> Vec<Problem>;
pub fn validate_bundle(bundle: &TemplateBundle, caps: &Caps) -> Vec<Problem>;
pub fn bundle(catalog: &Catalog, template: &str) -> Result<TemplateBundle, Problem>;
pub fn builtin() -> Catalog;
```

Readiness (`ready`): the node is `Pending`; not held; its phase is `Approved`
and not paused, or it is a queue node that is admitted; no earlier phase is
`AwaitingAcceptance`; a barrier phase has no undecided node in an earlier
phase; every hard dependency is `Accepted` and `integrated`; every soft
dependency is at least `InProgress`; a cancelled dependency releases; no node
that is `InProgress` or `Reported` has an overlapping write set. In a team
with no integration branch a hard dependency needs only `Accepted`:
`PlanView` carries the team's `integration` mode. A stage node is never a
dependency. A node whose phase or dependency is missing from the view is not
ready, so the store supplies the rows of archived dependencies.

Hand-out (`handouts`): order is stage nodes first, then plan nodes by most
transitive dependants and then by ID, then queue nodes by `rank`. A member
qualifies when it is `Active` and `free`, is not the lead, has the node's
role, and is the assignee when one is named. A stage node is never given to
its `implementer`. Each member gets at most one node per call; a node with no
qualifying member is skipped.

Choosing an account (`pick_account`): for `Named`, the account must exist,
match the agent and be signed in, else `unknown_account`. For `Freest`: take
signed-in accounts of the agent; drop any with a window at 100 percent or
more; headroom is 100 minus the highest `used_percent`; highest headroom
wins, then the lower `taken` count, then the lower ID. Accounts with no
windows are used only when no account with windows qualifies. Nothing left
gives `no_account_available`; its `refs` hold the earliest time an account
becomes usable again: for each account the latest reset among its full
windows, then the earliest of those across accounts.

Operation envelope, used by the CLI and by client core: a request is a JSON
object `{"request_id": "<uuid>", "team": "<id>", "expected_revision": <n>,
...}` with the operation's own fields; a reply is `{"ok": true, "revision":
<n>, ...}` or `{"ok": false, "error": {"code", "message", "refs"}}`. The
same `request_id` with the same content returns the first reply.

## Packages

Each package lists what it owns, what it must deliver and the tests that
prove it. Packages 5, 6, 7 and 12 are in the addendum below; the rest follow
when their dependencies are merged.

### Package 1: contracts

**Owns:** `crates/team-plan` (new crate, package `zerus-team-plan`),
`src/teams/mod.rs`, the `team` entry in the command dispatch of `src/cli.rs`,
the lines in `Cargo.toml`,
`client/core/Cargo.toml`, the lock files and
`.github/workflows/checks.yml` that add the crate.

**Deliver:**

- The crate with `model.rs`, `catalog.rs`, `problem.rs` exactly as in
  Contracts, with `Default` implementations giving the values in the
  comments. Wire it the way `crates/watch-protocol` is wired, with its own
  `Cargo.lock`, serde and serde_json only, and add its fmt, clippy and test
  lines to `checks.yml` next to that crate's.
- `crates/team-plan/fixtures/catalog-builtin.json` (three templates, four pipelines and
  their roles as the specification's tables say), `team-queue.json` (a
  running team of a lead, two developers and a reviewer with six queue nodes
  in different states) and `bundle-bugfix.json`.
- `src/teams/mod.rs`: `pub fn enabled(state_dir: &Path) -> bool`, true only
  when the file `teams/preview` exists in the `hgs` state directory, and
  `pub const INACTIVE: &str = "teams_inactive"`. Register an `hgs team`
  command that answers `{"ok":false,"error":{"code":"teams_inactive",...}}`
  and a non-zero exit when it is off, and `hgs team preview on|off|status`
  that creates or removes the file. `preview on|off` is refused when the
  caller has `HGS_SESSION` and `HGS_RUN_ID` set.

**Tests:** every type round-trips through JSON; an unknown field is refused;
absent optional fields default; each fixture parses into its type; the
defaults equal the documented values; with the setting off no file is created
under the state directory by any `hgs team` call; `preview on` from a session
environment is refused.

### Package 2: plan kernel

**Owns:** `graph.rs`, `ready.rs`, `overlap.rs`, `pipeline.rs`, `accounts.rs`
in `crates/team-plan/src`.

**Deliver:** the functions in Contracts. `graph.rs` starts as an import: copy
`src/state/teams/plan.rs` from `whitewh/whitewh/main` unchanged into
`crates/team-plan/src/imported_plan.rs` in a commit authored as the Global
constraints say, then in a second commit keep only the graph kernel (cycle
detection, longest-path layers, transitive dependants, dispatch order),
adapted to `Node`, renamed to `graph.rs`, with its tests. Do the same for the
write-set overlap helpers of `src/state/teams/model.rs` into `overlap.rs`,
without the hardcoded container folders.

**Tests:** cycles of one, two and three nodes; layers of a diamond;
dependants of a chain; each readiness condition, one test per condition, each
failing when the condition is removed; Review focus 2 and 4; hand-out order
(stage before plan before queue, rank order inside the queue); `pick_account`
for each rule in Contracts, including a tie and nothing available;
`after_report` and `after_verdict` for each built-in pipeline; with three
rounds, a return in round 1 or 2 goes to the implementer and the return in
round 3 goes to the lead.

### Package 3: team store and operations

**Owns:** `src/teams/store/`, `src/teams/ops/`.

**Deliver:**

- A private SQLite file `teams/teams-v1.sqlite3` under the `hgs` state
  directory, created on the first team, opened through `src/sqlite_file`,
  with `PRAGMA user_version` and a refusal to open a newer version. Tables
  for team, member, phase, node, dependency, lead inbox, owner card, journal
  and effect intent; indexes for every read below; no JSON blob holding a
  whole team.
- `ops::apply(store, actor, request, now_ms) -> Result<Reply, Problem>`: one
  transaction with the revision check and one journal entry per operation;
  idempotent by `request_id`.
- `ops::authority(actor, operation, team) -> Result<(), Problem>`: the only
  place that decides who may do what, from the specification's authority
  table.
- Operations of M1: create team from a bundle; set member session and state;
  pause, resume, stop, archive (team and member); add a task to the queue;
  admit; route; hold, release, cancel; record hand-out intent and outcome;
  record report; accept, return, reassign; ask and reply; open and decide an
  owner card; change settings.
- Reads: team by ID; members; the queue page by rank; nodes by state; inbox
  page; cards page; journal page by sequence. Every list takes a limit of at
  most 200 and a cursor.
- Decided queue nodes move to an archive table and do not count toward
  `max_open_tasks`.

**Tests:** each operation's success, its authority refusal and its
invalid-state refusal; idempotency by `request_id`; Review focus 3; stale
revision; paging stays stable while rows are added; a newer `user_version`
is refused; 1,000 decided queue nodes leave the live queue page unchanged.

### Package 4: catalog and templates

**Owns:** `validate.rs` and `builtin.rs` in `crates/team-plan/src`,
`src/teams/catalog.rs`.

**Deliver:** `builtin()` equal to `crates/team-plan/fixtures/catalog-builtin.json`;
validation (unknown role or pipeline, a pipeline stage or implementer role
with no seat, duplicate IDs, a roster over `max_members`, `Pipeline`
acceptance used as a phase acceptance); a person's catalog as one JSON file
in the `hgs` configuration directory, merged over the built-in one by ID;
save, import and export of a `TemplateBundle`. The catalog model starts as an
import of `src/state/teams/catalog.rs` from the fork, in the two-commit form,
reduced to roles, seats, pipelines and templates.

**Tests:** each validation rule; built-in equals the fixture; an imported
bundle is complete on an empty catalog; an unknown field in a file is refused
with `unknown_field` and the file is left untouched; a person's entry
overrides a built-in one with the same ID.

### Packages 5, 6 and 7

Their briefs are in the addendum for wave 2 below.

### Package 8: teams actor

**Owns:** `src/desktop/teams/actor.rs` and its effect ports.

**Deliver:** event intake (store revision, watch stream), `handouts`, intent
then the task attempt writer then outcome, doorbells through deferred send,
one safety check a minute. Effects sit behind traits so tests use fakes.
Review focus 1 and 2. The full brief is written when packages 5 and 7 are
merged; "Decisions during wave 2" already binds it. Carried forward from
package 7: when the team store's revision changes, the actor calls the
launch worker's coalesced wake, so a person's decision on a launch card acts
on the next pass and not only at the one-minute check.

### Package 9: pipelines

**Owns:** stage operations in `src/teams/ops/`, the actor's stage step.

**Deliver:** stage nodes and tasks created at report, `verdict`, rounds,
acceptance by the lead or by the pipeline. Review focus 4.

### Package 10: lead inbox, timers, escalation

**Owns:** inbox and card operations, the actor's timers.

**Deliver:** inbox items for report, question, no report, not started,
stopped member, conflict; coalesced doorbells; reminders and escalation to
owner cards by the `Timers` values; pause and resume.

### Package 11: integration branch

**Owns:** `src/teams/integrate.rs`.

**Deliver:**

```rust
pub struct Repo { pub git_dir: PathBuf }   // the repository's common directory
pub enum Merge {
    Merged { commit: String },
    Conflict { files: Vec<String> },
    Stale,                                  // the ref moved; call again
}
pub fn create_branch(repo: &Repo, branch: &str, base: &str) -> Result<(), Problem>;
pub fn reported_commit(worktree: &Path) -> Result<Reported, Problem>;
pub struct Reported { pub commit: String, pub branch: String, pub dirty: bool }
pub fn descends(repo: &Repo, commit: &str, base: &str) -> Result<bool, Problem>;
pub fn merge(repo: &Repo, branch: &str, commit: &str, message: &str) -> Result<Merge, Problem>;
pub fn sync_base(repo: &Repo, branch: &str, base_branch: &str) -> Result<Merge, Problem>;
```

`merge` reads the branch tip, runs `git merge-tree --write-tree`, creates the
merge commit with `git commit-tree` (signed when `commit.gpgsign` is set) and
advances the ref with `git update-ref <ref> <new> <old>`. It refuses with
`integration_unavailable` when the branch is checked out in any worktree or
Git is older than 2.38. It runs no other Git command that writes, and no
`reset`, `checkout`, `push`, `fetch` or hook.

**Tests,** each on a temporary repository: a clean merge advances the ref and
both parents are right; a conflict returns the files and leaves every ref and
the object count of reachable commits unchanged; Review focus 5 (the ref is
moved by the test between the two steps through an injected hook point); a
branch checked out in a worktree is refused; a commit that does not descend
from the base is reported; a dirty worktree is reported; `sync_base` merges
the base in and reports a conflict the same way; no file in any worktree
changes during any call (compare a tree hash of the working directories).

## Addendum: wave 2 (packages 5, 6, 7 and 12)

Written 2026-11-11 against the store package as reviewed. The interface the
four packages share:

```rust
// src/teams/ops (package 3)
pub fn apply(store: &mut Store, actor: &Actor, request: &Request, now_ms: i64) -> Result<Reply, Problem>;
pub enum Actor { Person { .. }, Session { .. }, Other { .. }, Service { .. } }   // never deserialized
pub struct Request { pub request_id: String, pub team: String, pub expected_revision: u64, pub operation: Operation }
pub struct Reply { pub ok: bool, pub revision: u64, pub node: Option<NodeId>, pub item: Option<u64> }
```

`Operation` has one variant per operation of the store. Facts (`ObserveTask`,
`HandoutIntent`, `HandoutOutcome`, `ReopenIntent`, `ReopenOutcome`,
`SetMemberSession`, `RecordReport`, `RecordIntegration`,
`RecordIntegrationConflict`) are accepted only from `Actor::Service`, and no
package builds that actor from input.

**How a team comes to life.** `Create` only records the team: its copy of the
template, its members in state `NotCreated`, its settings. The desktop service
(package 7) sees members that are not created and launches them. So the CLI
and the desktop create a team the same way, and neither launches anything.

### Package 5: CLI and guides

**Owns:** `src/teams/cli.rs` (and `src/teams/cli/` if it needs more than one
file), the dispatch in `src/teams/mod.rs` that today answers `not_found`,
`src/agent_guide/team.md`, `src/agent_guide/team-lead.md` and their
registration.

**Deliver:**

- `hgs team` subcommands of M1, each reading one JSON request on stdin where
  it has input and printing one JSON reply: `catalog show|import|export`,
  `template list|show|save|import|export|remove`, `create`, `settings`,
  `pause`, `resume`, `stop`, `archive`, `decide` for the person; `status`,
  `inbox`, `queue`, `cards`, `journal`, `admit`, `route`, `accept`, `return`,
  `reassign`, `hold`, `release`, `cancel`, `reply`, `pause`, `resume` for the
  lead; `current` and `ask` for a member. `verdict` belongs to package 9.
- Actor resolution. A caller with `HGS_SESSION` and `HGS_RUN_ID` is verified
  exactly as `hgs task` verifies it (live run and process ancestry, the code
  in `src/tasks.rs`), then mapped by session identity to the team's lead or a
  member, else `Actor::Other`. A caller without them is the person only when
  no live session agent is among its process ancestors; the reply says the
  interim rule was used. A request field never names the actor.
- Reads are pages of the store: a limit of at most 200 and a cursor. `status`
  returns the team without its bundle, its members and counts; `status
  --bundle` adds the bundle.
- The reply shapes of the five reads are written as fixtures in
  `crates/team-plan/fixtures/` (`cli-status.json`, `cli-queue.json`,
  `cli-inbox.json`, `cli-cards.json`, `cli-journal.json`) and pinned by tests,
  because package 12 reads them.
- The two guide pages, each within 2,400 bytes: what a member does (the task
  protocol stays `hgs task`; `hgs team current` and `ask`), and what a lead
  does (inbox first, decide every report, never build or merge a member's
  work).

**Tests:** every command against a temporary state directory with the preview
on; each actor mapping, including a member trying a lead's command and a
session of another team; a request that tries to name its actor is refused
with `unknown_field`; with the preview off every command answers
`teams_inactive`; page stability; the fixtures round-trip.

**Known limits.** The template digest covers the template with its roles and
pipelines, not the catalog's `planning_rules`; nothing reads the planning rules
yet, and they will be bound into the digest when the lead starts to read them.

### Package 6: tasks and teams

**Owns:** the optional `team` key in `crates/replica` task fields; in
`src/tasks.rs` and `src/tasks/`: `hgs task add --team TEAM`, `hgs task target
REF team TEAM`, and the three refusals; `src/teams/tasks.rs`.

**Deliver:**

- `team` is `{ "team": <team id>, "home": <computer UUID> }` in a task's field
  map, read and written without raising the task schema. A reader at the
  current schema that does not know the key is unaffected: pin that with a
  test that decodes a document carrying the key with the key's code removed.
- `src/teams/tasks.rs`: `observe(project, uuid) -> TaskObservation` built from
  the task document this code read itself (stage, cancelled, rank, title and
  the document's revision token), and `send(actor, task, team, role,
  pipeline)`, which applies `AddTask` as the caller and then `ObserveTask` as
  the service from that observation.
- `hgs task add --team` and `hgs task target REF team` call `send`. The
  replicated `target` stays `any`.
- On the home computer, for a task whose `team` key names a live team:
  `send`, `handoff` and `stage done` are refused for every agent except that
  team's lead. On another computer they are refused for every agent. A person
  is never refused; the override is observed by the service.

**Tests:** Review focus 3 through the CLI; the key survives a sync round trip
and an edit by a build that does not know it; each refusal and its two
exceptions; a task sent twice has one node; with the preview off `--team`
answers `teams_inactive` and writes nothing.

### Package 7: member launch and identity

**Owns:** `src/desktop/teams/` (the launch part) and its registration in the
desktop service.

**Deliver:** a service actor in the service's own pattern (admission, a fixed
plan, the original child and its custody; a restart never continues an
operation). For a team that is Running:

- the integration branch is created with `integrate::create_branch` when the
  team's integration is a branch and the branch does not exist;
- the lead, then each `NotCreated` member, is launched through the service's
  existing worktree and launch actors, each member in its own generated
  worktree and as a child of the lead;
- the account is `pick_account` over the usage windows of `hgs account
  inspect` for the member's agent, with the accounts already given to this
  team's members as `taken`;
- the launch ID binds the session: when the session appears with that launch
  ID, `SetMemberSession` records its key, its worktree and the ownership
  nonce;
- a launch whose outcome is unknown becomes an owner card and is never
  retried; `no_account_available` becomes an owner card that names the reset
  time.

Effects sit behind traits so tests use fakes.

**Tests:** the order (branch, lead, members); one launch per member across a
restart at every step; an unknown launch produces one card and no second
launch; account choice spreads two seats over two accounts; a member is bound
only by its launch ID, never by its name.

### Package 12: client core

**Owns:** `client/core/src/teams/`, `client/src-tauri/src/teams.rs`, the
`teams` key of the batch, and the lines that add `zerus-team-plan` to
`client/core/Cargo.toml` and the client lock files.

**Deliver:**

- Typed projections for the desktop: `list` (teams of the computer), `team`
  (bar, members, the queue as board groups by state, each card with its
  stages), `waiting` (owner cards and person-only requests of team sessions).
  The view structures start as an import of the fork's
  `client/core/src/teams/view.rs` in the two-commit form, reduced to what M1
  shows.
- Pure functions from the read replies of package 5 (the `cli-*.json`
  fixtures) to those views. Until package 5 is merged, build them on
  `team-queue.json`.
- Actions as `TeamCall { team, action, request_id }` with the outcomes done,
  failed, uncertain and confirm; an uncertain action is never repeated
  automatically.
- Tauri commands that run the reads and actions through `hgs team`, follow
  one team at a time, and stop when the tab is hidden. The view is read again
  after the desktop's own action, when the project's tasks topic changes, and
  every 10 seconds while the Team tab is visible. A `teams` watch topic
  replaces the timer in M4.

**Tests:** each projection from the fixtures, including a card with a stage
in each state and an empty queue; the 200-row page bound; an action outcome
of each kind; the follow set never holds more than one team.

### Packages 8 to 10 and 13 to 17

8 the teams actor; 9 pipelines; 10 lead inbox, timers and escalation; 13 New
team and the Team tab with Board and task card; 14 Waiting for you and Send to
team; 15 isolated real-agent coverage; 16 guide, reference and credit; 17
standing confirmations (folder trust for a team worktree, resume of an exited
member). Their briefs are written when packages 5 and 7 are merged.

## Decisions after the first reviews

Recorded 2026-11-11 from the independent reviews of packages 2, 3, 4 and 11.
Packages 5 to 17 follow them.

**Who records what.** Facts are recorded only by the Service actor: task
observations, hand-out and reopen intents and outcomes, a member's session
binding, a report, an integration receipt. A person, a lead or a member cannot
record them, and a request never carries a task observation. The Service actor
is built only by trusted code that read the task document or performed the
effect itself; the CLI never accepts it from input.

**A lead decides, the service acts.** Accept, Return, Reassign and Cancel
record a decision. While a decision of a node is not settled, another decision
of that node is refused.

**Accept and integration.** Accept carries no receipt. In a team with an
integration branch, a non-stage node is Done only after the service merged it
and recorded the commit. The delegation `merge_accepted` says who triggers the
merge: the lead's Accept when it is on, the person's approval of an owner card
when it is off. A conflict is recorded by the service as a return with the
conflict list. With no integration branch, and for a stage node, there is no
merge and no receipt. A receipt or a conflict that arrives after the team
was stopped is still recorded. When a node's decision settles, its open
integration cards close.

**Return.** The reopen message is an effect: an intent for the new generation
before it is sent, an outcome after, never retried when uncertain. A report of
the new generation is accepted once that effect is recorded.

**Hand-out.** Only the service records a hand-out, and only for a node the
kernel says is ready. The readiness view holds the live nodes with their
direct dependencies and stage targets; an archived node is a leaf whose own
dependencies are not followed. Reassigning a node that was never handed out
only changes its assignee, and a reopen needs an earlier attempt of the node,
so neither goes around the gate. The effect row holds the current holder. A node's
`assignee` is the plan's and changes only by Route and Reassign. For a task
node `implementer` is set when its hand-out is submitted; a refusal leaves
neither. For a stage node `implementer` is the member who implemented the task
it checks, copied when the stage is created and never changed by the stage's
own hand-out. The teams actor passes `free: false` for every member with an
open hand-out, because a stage's holder is only on its effect row. A
dependency that names a stage node never releases.

**Queue.** Only the person and the lead route a task by naming a role or a
pipeline while sending; from anyone else the values are a suggestion. Only
admitted nodes count toward `max_open_tasks`; unadmitted ones are bounded at
100 per team. A task has at most one live node: a cancelled node, and every
node of an archived or stopped team, releases it.

**Pauses.** A team the person paused is not resumed by the lead, as with a
member the person stopped.

**Integration (package 11).** No hook runs during an integration call: one
defined by the repository makes integration unavailable, the person's own are
switched off for these calls. A merge driver defined by the repository makes
integration unavailable on the merge paths; the person's own driver and signer
are honoured. A commit the branch already contains is a no-op. A conflict
changes no ref and no reachable object.

**Catalog (package 4).** A seat counts only with a positive count. Duplicate
IDs are checked inside each catalog before catalogs are merged. A template's
default pipeline is required. Every role's pipeline must exist, seated or not.

**Kernel (package 2).** A stage verdict counts only when the stage node's role
equals the pipeline's stage at its index, and a stage is ready only in its
target's round. A node ID that appears twice is ambiguous everywhere: nothing
that depends on it or checks it is ready. Write sets are relative to the
repository. ASCII components are compared without regard to case; a
component with a non-ASCII character matches any component in its position
unless the two are byte-equal, because case and normalization rules differ
between file systems. An absolute path, and a parent component after a
pattern, cover the whole repository. Operations
refuse an absolute path in a write set at input.

**Shape.** One function per operation of about 60 lines or fewer; no file that
is one long match.

## Decisions during wave 2

**Member launch (package 7).** The desktop's `creation_worktree` and
`creation_launch` actors admit only the proof of a person's gesture, and they
get no second admission. The teams actor launches through the path every CLI
caller has, in the form `creation_launch::child` uses: this computer's own
`hgs` as the one original child, run by the shared `desktop::original_child`
runner with a child kind and custody of its own. It runs `worktrees create`
and then the documented launch with the reserved launch ID. The arguments
come only from the team store.

A launch that never started leaves the member `NotCreated` for a later pass.
A refused launch makes one owner card and no second launch. An unknown
result, or an exit of 0 with no session carrying the launch ID within the
bound, makes one card marked uncertain and is never retried.

**Parent link.** A launch by the service cannot prove ancestry, so the link
is written after binding with the existing `hgs parent` action `set`, and
reads `manual`. It is written only when the member has none and never
replaces one. An agent the command refuses (OpenCode) has no link,
which is a known skip. The link is presentation and attention routing: a
failed write blocks nothing and makes no card, and the lead's authority over
members comes from the team store. A source of its own for team links needs
the aligned client change and waits for M3.

**Integration facts (package 3).** `RecordIntegration` and
`RecordIntegrationConflict` name the accept decision they belong to. The
permit is that exact decision: an accept of that node, Ready or settled with
preserved authority, with no fact yet; there is no fallback to the pending or
the latest decision. The fact changes the node only when the decision's
generation is the node's current one. The service reads the authorized
accepts that have no fact as a bounded page and names the decision it acted
on.

**One caller proof (packages 5 and 6).** `crate::teams::caller`
(`src/teams/caller.rs`) answers who runs a command: `Caller::Person`,
`Caller::Session(SessionKey)` or `Caller::Refused(Problem)`. The CLI and the
task intake both use it. After its review it is a piece of its own, on its
own branch, and merges before the rest of package 5 and before package 6.

The interim person rule, exactly: for each ancestor of the command, at most
64, read its environment; an ancestor that names a session is verified
against the one saved record its name and run point to, by process ID and
birth identity. A verified live session agent among the ancestors means the
command is not a person's. The walk ends at the top of the user's own
process tree: the first ancestor of that user whose parent belongs to
another user. It ends there whether or not that process can be read, because
a person's terminal and the desktop application both descend from the
user's service manager, whose environment the same user cannot read; the
top is decided by the owners in the process table, never by a name. Below
the top, whatever cannot be established is a refusal: an ancestor that
cannot be read, a walk that stops early or hits its cap, a record that is
corrupt or unreadable. The proof changes no process environment, reads no
session inventory and prints no path. Turning the preview on or off goes
through it.

The limits are stated, not hidden. The rule guards against role confusion
and accidents; it is not a boundary against a process that removes its
markers on purpose. It does not detect a command that does not descend
through readable processes of the same user to its agent: a child
reparented away from its agent, a step through a helper that changes
credentials or is started by a daemon (`sudo` or `su` to the same user,
`crontab -e`, `at`, `cron`, `ssh` to the same computer), and a registered
agent that was started without the markers in its environment. Closing
these needs an index of sessions by process and belongs with the verified
Person authority (`zerus-cg5h.4.11`) that replaces this rule. Two cases are
refused instead: a process of this user whose parent is 0, and any command,
marked or not, outside the initial user namespace. The second also covers a
marked command, because a user namespace can own a mount namespace in which
the process table can be overmounted, and a forged table let a command pass
as another live session. The cost is stated: a lead or a member whose shell
runs in a sandbox with its own user namespace cannot use `hgs team`.

The namespace is read without crossing a mount (coordinator finding of
2026-11-11, verified by a probe inside a private namespace). Opening
`/proc/self/ns/user` by path and comparing the object is not enough: a
handle to the initial namespace taken before entering a private user and
mount namespace can be bound, with its directory, over the process's own
`ns` directory, and the path then answers with the initial namespace. The
admission opens `/proc`, requires a procfs root (file system type and root
inode), resolves `self/ns/user` from it with `openat2` and
`RESOLVE_NO_XDEV` without following the last link, and compares the link's
own text with the initial namespace. Any failure refuses, including a
kernel or a sandbox without `openat2`; there is no fallback to the path.
The same admission serves the marked and the unmarked caller. It does not
cover a caller that replaces or instruments `hgs` itself or writes the
store directly: that is the stated limit of the interim rule.

The preview is for Linux in this milestone. Where the ancestry cannot be
evaluated, an unmarked command is refused and the preview cannot be turned
on. macOS is treated that way until the real-process suite has run on a
Mac; Windows has no evaluation yet.

The first run on a Mac (2026-11-11, macOS 27.2) found that the system
withholds the environment of its own binaries, among them the shells and
`ssh`, from a reader of the same user: the read ends after the arguments
without an error. Read as an empty environment, a hidden ancestor would
look unmarked. So on macOS an environment with nothing at all after the
arguments is unreadable, and the existing rule applies: an unreadable
ancestor is tolerated only at the verified top and refused below it. A
person's command therefore passes from the Zerus window, which runs `hgs`
with no shell between, and is refused below any program whose environment
the system hides, with a sentence that says so. The third run measured
which programs those are: the ones whose code signature carries the
restrict flag (464 of 513 processes; the system's zsh, ssh and its session
helper among them), while a shell or tmux installed by the person is
readable. So the rule is about the program above the command, not about
tmux or ssh as such. The marked path of a lead or a member reads no
ancestor's environment and is not affected, and the caller's own markers
come from the process itself, never from the system read. The review of
the first candidate hardened the parser of the system's answer: a tail
with no byte other than NUL is withheld, an argument that begins with a
marker name makes the read unreadable, an answer as large as the buffer is
unreadable, and a marker named twice is unreadable on every platform. The
gate was lifted by the third Mac run (65 tests, nine mutants caught) and
merged on 2026-11-12. Not yet observed on a Mac: the hardened release
`hgs`, and whether `claude` and `codex` have readable environments; if
they do not, a person's command below them is refused.

**A caller below a resident agent host is not a person (2026-11-12, from a
real agent).** A probe from the tool shells of real sessions showed Claude
and Codex behaving as designed, and a DeepSeek session, which one resident
host process serves, passing as a person once the three markers were
removed: the host is not below any agent's process, and its shells even
carried the markers of the session that had started the host (tracker item
`zerus-ppd26`). The person path now refuses when an ancestor is a recorded
live resident agent host, matched by PID and birth from the host's own
record, never by a program's name; a recorded PID without a verifiable
birth, an unreadable record or more records than the bound refuse as
unverifiable. For OpenCode, whose shared service Zerus does not record,
the PID in OpenCode's own service file is used only to add a refusal. A
session that a resident host serves cannot be a lead or a member in this
milestone, because its commands cannot be bound to its own session by
ancestry: the proof says so plainly, and team creation and member launch
refuse such an agent kind.

**How the service reaches a session (packages 8, 10 and 17).** The desktop's
deferred-send, deferred-answer and creation actors admit only the proof of a
person's gesture. The teams actor does not enter them. Each of its effects is
this computer's own `hgs` run as the actor's one original child:

| Effect | Command |
|---|---|
| Hand-out and reopen | `hgs task team-deliver` (below) |
| Doorbell, a return's reason, a brief on resume | `hgs send SESSION --json`, bound to the run and conversation it read |
| An answer to a member's prompt | `hgs answer SESSION --json`, after the approval journal reserved it |
| Integration | the functions of `src/teams/integrate.rs`, called in the actor's worker thread as package 7 calls `create_branch`; their Git commands are the bounded children. No command arm of `hgs` starts a merge |

**Team delivery (package 6, second part).** The attempt writer is reachable
only through the task CLI, and `start-deliver` is the person's. A new
`hgs task team-deliver REF --json` reads `{schema, id, team, node, member}`
and delivers only what the team store already holds: an open hand-out or
reopen effect of that team with that request ID for that node and member, on
the team's home computer. The effect is the authority; the caller is not
asked who it is. It writes the attempt as the team's lead, with the prompt
`freeze` builds followed by the team context from the store (role and goal,
criteria, dependency reports within 2,000 characters each and 6,000 in total,
binding decisions, base commit, branch, write set). It answers
`{state: submitted|refused|uncertain|cutting_over}` in the form of
`start-deliver`, and the same ID never sends twice. `--recover-only` reads
the receipt and sends nothing.
Like `start-deliver`, its reply carries `error` only when there is one;
a reader takes a missing `error` for `submitted` alone. The four commands
below always carry the field.

Two more commands of the same kind, each run by the service and each taking
its authority from a stored effect or decision, never from the caller:
`hgs task team-done REF --json` finishes an accepted task, and
`hgs task team-stage --json` creates the Zerus task of a stage node that a
pipeline asked for. A stage's task is an ordinary task with the team key;
its kind lives on the team's node. Its identity is derived from the effect's
request ID, so a repeat creates nothing.

Both answer in the envelope of `start-deliver`, echoing the request.
`team-done`: `{schema, team, node, state, error}` with `done` (the task is
Done after the call, whether this call moved it or it already was),
`refused` (the store holds no accepted node of that team that this task
finishes; nothing was written; not repeated before the node's observation
or decision changes) or `cutting_over` (nothing was written; repeated with
the backoff). It has no uncertain state: the observation settles it, and a
repeat is safe until the observation says Done. `team-stage`:
`{schema, id, team, node, state, error}` with `created` (also when the task
of that derived identity already exists), `refused` or `cutting_over`. A
reply that does not echo its request, an unknown state, a malformed reply
and a failed child are all treated as no answer.

A fourth command of the same kind clears what a released stage leaves
behind: `hgs task team-clear REF --json` reads `{schema, team, node}` and
removes the team key from the task of a stage node that the store holds as
released for that team on this home, on an exact key match. It answers
`{schema, team, node, state, error}` with `cleared` (also when the key is
already absent), `refused` or `cutting_over`, never touches a live stage,
and writes nothing to the team store. It clears the key of a task node only
when the store holds that node's team as stopped or archived on this home
(package 10 releases a stopped team's tasks this way): the task keeps its
stage, executor and attempt and loses only the key. A task node of a team
that is planning, running or paused is refused; there the person retargets
the task.

**The CLI as the desktop's interface (packages 3, 5 and 12).** The client
core reads through `hgs team` and follows the shapes package 5 pins in
`crates/team-plan/fixtures/cli-*.json`: a page is
`{ok, actor, team, rows, next}`, `status` is
`{ok, actor, team, members, counts}`, and a read takes `--team`, `--limit`
and `--after`. Three things the desktop needs are added:

- `hgs team list`: the teams of this computer, a page of
  `{id, project, name, status, lead, home, revision}` in ID order
  (`cli-list.json`);
- `hgs team waiting`: the open owner cards of all teams, oldest first, each
  a card row with its team ID (`cli-waiting.json`);
- the person's `pause-member`, `resume-member`, `stop-member` and
  `archive-member`.

The store gains the reads behind them: `teams`, `live_teams` for the
service's pass, and `waiting`, each a bounded page that does not scan
archived teams or closed cards. A request of a team session that only the
person can answer is not a card; the desktop takes it from its session
attention data.

**A fact after integration was switched off (package 3).** With a valid
permit the fact is recorded as provenance; the node changes only when it
still needs a merge and the decision's generation is current.

**No agent drives a team's task (package 6, after its review).** The earlier
exception for the team's lead is withdrawn: the lead decides through
`hgs team` and the service acts. On a task of a live team these are refused
for every agent: `send`, `handoff`, any stage change, `target`, `start` and
`worker` as a claim, and `message --mode reopen`. An agent may still take its
own protocol steps on an attempt that was delivered to it (the
acknowledgement, the report, a worker under that attempt), comment, and add
text. A person is never refused. The team's own writes come only from two
commands the service runs, which take their authority from the team store
and not from the caller: `hgs task team-deliver` and
`hgs task team-done REF --json`, which moves a task to Done as the team's
lead only when the store shows the node's current generation accepted with
authority and, where it needs a merge, its integration fact.

**The key is the marker (package 6).** A task without a `team` key is not a
team's task, and no team check opens anything for it. A key that names
another computer refuses agents without opening the store; a key that names
this computer is checked against the store's live node. A key that does not
decode never damages the task: it reads as no usable key with an invalid
mark, and agents are refused as for another computer. Intake writes the key
with the step that makes the task a team's and removes it when the team
refuses; a person's retarget away from the team clears it; the service
clears the keys of a stopped or archived team's open tasks (package 10).
The package is one unit and merges after the repository owner confirms the
replica change; everything under `crates/replica` stays a commit of its own.

**Observation belongs to the service (packages 6 and 8).** Ordinary task
commands do not observe for the team. Intake records the first observation;
after that the service observes through one function that reads the document
and records under the project lock.

**A task that leaves the team (packages 3 and 6, after the second review).**
The observation starts from the store's node, not from the key, and records
the key's state: ours, absent, or another team's. A live node whose key is
absent or another's is released as a person's override: its slot and its
task binding are freed and a ready accept keeps its authority. On the home
computer a person's retarget clears the key and releases the node in the
same command. A key cleared or replaced on another computer is released at
the home service's next observation. Also refused for agents on a team's
task: `cancel`, `cancel-execution`, `move`, `edit` and `rename`. With the
preview off, a key that names this computer does not guard, and a key that
names another computer still refuses agents.

## Decisions during wave 3

Recorded 2026-11-11 after the review of package 9's core.

**A lead accepts only what its pipeline handed over (package 9).** For the
node's current generation a lead's Accept is refused unless the pipeline's
run reached the lead. A lead's Return, Reassign or Cancel retires a pending
stage but never opens the gate for the same attempt: when the reopen cannot
be delivered, the lead decides again (Return, Reassign, Cancel) or asks the
person. The person is never refused.

**A stage ends by its holder's verdict or by the person (package 9, second
review).** A lead may Return a stage to its holder or Reassign it; a lead's
Accept and Cancel of a stage node are refused, because either one ends the
stage without a verdict and the stage's end was then read as the person's
override, which handed the target to the lead with no review. A holder that
reports on a stage instead of giving a verdict produces a lead inbox item
and nothing else; the stage's hand-out text tells the holder to answer with
`hgs team verdict`. A report recorded by a store from before the pipeline
tables has no run and cannot be accepted by the lead; the person accepts
it. That affects development stores only and is a stated limit; so does a
lead's Cancel of a stage recorded before this rule, which a later board
cancel reads as the team's own end (the lead recovers with a Return of the
target).

**A stage node is never restored (package 9).** A stage whose creation is
not wanted, reserved or created is released. A person who moves a finished
stage's task back on the board changes that task only: the observation is
recorded and the node stays archived. The service clears the team key of a
stage task whose node is released, including one created by a late outcome
for a superseded stage (package 8, part C).

**A round is an attempt of the task (package 9).** Every reopen starts a
round, whoever caused it: a pipeline's return, a lead's Return or Reassign,
a person's reopen on the board. A pipeline hands the task to the lead when
its rounds are used up, so a reassignment brings that moment closer. This
is kept because one counter also names a stage's target round.

**A report settles the open items of its attempt (package 9).** The
member's unanswered questions on that node and an uncertain delivery are
closed by the trusted report, for every pipeline. The lead's inbox item for
a reported task carries the report's text first and then the pipeline's
reason and findings; for a pipeline without stages it is the report's text
as before.

**A resumed member keeps its launch and conversation and gets a new run
(packages 8 and 17).** A standing resume restores the member's exact session: launch ID,
native session, machine, agent, account, worktree and conversation stay, and
only the run is new: a resume continues the member's conversation, as the
native session action requires. Package 17 records the run from the completed resume
receipt and exposes `confirm::resumed_binding(state, team, member, launch)`,
a bounded read-only fact that grants neither liveness nor authority to
send. The actor takes a member's current run from that fact when it exists
and from the launch otherwise, through one function of its availability
port, and proves liveness and idleness from the fleet row of that exact
run. A member whose launch ended and who has no resumed binding is not
free.

**What the board needs to show a stage (packages 5, 9, 12 and 13).** The
Team tab follows the fork author's board: a card says which stage it is in
and in which round. The reads gain additive, optional fields and nothing is
inferred from a role's name. A queue row carries `kind` (`task` or `stage`,
absent means task) and `rounds`, the pinned pipeline's rounds; a stage row
also carries `stage: {kind, target, round}` with the kind `review`, `test`
or `land`, the target node and the target's round that the stage checks. A
member row carries `model` when its role names one. The client core passes
them through as optional values; a view shows a stage by its kind when the
kind is present and by its role's own name otherwise.

**An effect is admitted at the moment it starts (packages 8 and 17, after
their reviews).** The linearization point of an automatic effect is its
start, not the read before it. For an effect that is a child process (a
hand-out, a reopen, `team-done`, an answer, a resume) the stored facts that
allow it are read and the child is spawned inside one immediate transaction
on the team store, released as soon as the spawn has returned; the shared
runner lets an admission hold a value across the spawn for this. For a
merge the point is the update of the integration ref: the admission is read
again inside such a transaction held only across that update, not across
the tree merge before it. A pause, a stop or a switched-off delegation
committed before the lock prevents the effect; one that waits behind it is
ordered after. Facts outside the team store are read last in the same
section, and the remaining window is the spawn call itself.

**Merges (package 8, after its review).** A member reports from the branch
of its task, which its agent creates; the reported commit is the HEAD of
the member's owned worktree whatever its branch, and only the ownership
record is checked. A conflict fact carries at most 199 paths within the
ledger row's bound and then one entry that counts the rest. A merge whose
result is unknown blocks only its own decision; it is settled by reading,
and once a new worker epoch has started and the Git deadline has passed, a
negative ancestry read is a certain non-write and a new original may
start. An integration fact's commit is the commit of the integration
branch that was verified to contain the work: the new merge commit, or the
tip that already contained it; never the member's commit.

**Who is a team's session (packages 5 and 14, after their review).** The
desktop must know which sessions belong to a live team without loading
each team: Waiting for you lists the requests of members as well as of
leads, and a parent link proves nothing. A row of `hgs team list` for a
team that is planning, running or paused carries, besides `lead_session`,
an additive `member_sessions`: the bound sessions of its members as
`{member, machine, session_id}`, at most the roster's size, absent when
none is bound. A removed member's session is not the team's session and is
left out, although its binding is kept. A roster over its bound, which a
well-formed store cannot hold, answers without the field and does not
refuse the page. The client core passes it through, and the frontend takes
membership and the team's status from the same row.

**Switching off the merge delegation takes back what has not merged
(packages 3 and 8).** The authority to merge lives in the decision, and the
actor checks only that the exact accept is still Ready at the moment the
ref is updated. So the store keeps that state true: whenever a settings
change leaves the team with a branch to integrate into and without the
delegation, every Ready accept that no person decided or approved and that
has no integration fact yet goes back to awaiting the owner in the same
transaction, with its integration card. The rule is about the state a
change leaves, not about the switch itself: an accept made while the team
integrated nothing is Ready because it needed no merge, and a later change
of the integration setting must not turn it into an authority to merge. An
accept the person made, or approved on its card, stays Ready whatever the
setting.
The actor releases the store right after the ref update, so the switch can
fall between a merge and its fact, as a restart can. A refusal cannot undo
a merge: the fact of a merge for an accept that awaits the owner is
recorded, its integration card is closed as overtaken in the same
transaction, and the node goes on as after any merge fact; after the person
rejected that card the fact is late provenance, as after a stop. A conflict
wrote nothing, so a conflict fact for an accept that awaits the owner, or
whose card the person rejected, is refused: a fact row would mark the
accept as integrated, and the merge that follows the person's approval
could never be recorded. A conflict fact after a stop stays late
provenance, as before. New writes are still found only among exact Ready
accepts. The revocation marks each accept it takes back, and a stop or a
board change that settles the accept keeps the mark, so that the fact of a
merge written before the revocation is still taken afterwards. The mark is
also what tells an accept that once had the authority from one that never
did: an accept made while the delegation was off, and never approved, takes
no merge fact in any state. A development store written by the heads of
this branch before the mark existed holds taken-back accepts without it;
their late merge facts are refused like those of an accept that never had
the authority.

**What the review of the service task commands settled (package 6b).** A
team that is stopped or archived retires its nodes by that status:
`team-clear` asks for no archive row there, for a task node and a stage
node alike, because a stop leaves live nodes where they are and nothing
observes them again. Under the project lock and the team transaction a
command compares the facts its permit rests on (the effect and its state,
the generation, the member and its binding, the team's status), never the
team's revision: an unrelated write must not refuse a hand-out or retire a
stage. A reopen whose member is not the task's executor, which is what a
reassignment is, is delivered as a hand-over through the same attempt
writer as a first hand-out. `team-stage` answers `created` by the task's
identity whatever its text has become. A failure that is not a rule's
refusal fails the child and is repeated; a refusal by a project limit
(`agents_other`, `agent_limit`) carries a stable code so that the service
can open the card the specification asks for (tracker item
`zerus-63rru.23`). Text that came from a member or a task enters a
hand-out quoted, under a heading that says whose it is. A cleanup's history
entry names its cause.

**What an admission reads (package 8, after its confirmation review).**
The admission of a hand-out asks the store the same question its intent
asked, through the store's own predicate inside the held transaction: the
node's hold, its readiness with its dependencies, its state, assignee and
role, besides the team, the member, the effect and the generation. One
predicate, two call sites; a Hold or a Route committed between the intent
and the spawn refuses the delivery. Outside facts are read last in the
same section when they can be read in the process: the stop flag, the
Teams switch and the member's fleet row. The task occupancy cannot be,
because reading it spawns children; it is read as late as possible before
the admission and is a scheduling hint, not an authority. The window in
which a person's own send can still reach the member beside the hand-out
is therefore wider than that one read (named by the confirmation of
2026-11-12): the wait for the process-wide spawn turn, the admission's own
reads, the delivery command's time before its send, and the fact that the
fleet row is a snapshot, since the admission fences the team's store and
not the member's session. The team does not own a member's session against
the person; a message that arrives beside a hand-out is the person's. A held section
is short: the spawn itself, or the ref update with its own deadline of 5
seconds, after which the merge is unknown and settled by reading. Writers
of the command line wait 8 seconds for the store, longer than any held
section, and a writer that still cannot begin is told `store_busy`:
nothing was written. The actor's own admission waits 100 milliseconds,
and a busy store is a retry on a later pass, never a recorded refusal.

**What the person confirmed is bound at the create (packages 5 and 13,
after the desktop's review).** New team shows a template's roster,
pipelines and delegations, and the person confirms them; a template is
replaced by its ID, so a create that names only the ID can make a team the
person never saw. The catalog read gives each template a digest over
everything a create uses. `hgs team create` accepts `expected_template`,
the digest that was shown, and refuses with `template_changed` before
anything is written when the bundle it is about to use has another; and
`delegations`, all five, which become the team's in place of the
template's. The desktop always sends both, never creates unbound, and on
`template_changed` shows the template again for a new confirmation. A
request without the fields behaves as before, for a person at a terminal.

**A doorbell points at the inbox and carries none of it (package 10).** The
doorbell says how many items wait and names the command that reads them;
the text of an item comes from members and is never put into a message of
the service. It rings when the team is running, the lead's session is live
and idle with no pending request, an open item is newer than the mark of
the last doorbell, and the interval has passed since the last reservation.
The reservation names the lead's run and conversation and the mark and is
durable before the send; the send is admitted at its spawn. A refused
doorbell may ring again after the interval; an uncertain one is never sent
again for its mark, and a newer item rings a new one. A resumed lead gets a
new doorbell, never a replay.

**A lead's answer reaches its member through the member's own read and a
doorbell (package 10, follow-up).** A reply stays on its inbox item.
`hgs team current` shows, on each node the member holds, the member's own
questions of the current attempt with their state and the lead's answer:
newest first, at most ten, with a flag when there are more. A question of
an earlier attempt is not shown; a Return's reason carries what the next
attempt must know. A reply to a question of the current attempt marks the
member. The member's doorbell says how many answers wait and names
`hgs team current`; it never carries the text of a question or of an
answer. It follows the lead's doorbell rules: the team is running, the
member's session is live and idle with no pending request, the mark is
newer than the last doorbell's, the interval has passed, the reservation
names the member's run and conversation and the mark and is durable before
the send, the send is admitted at its spawn, a refused doorbell may ring
again after the interval and an uncertain one is never sent again for its
mark. At the admission the member must still hold that attempt: after an
accept, a return, a reassignment, a cancel or a stop the mark is dropped
and nothing is sent. The member's guide page says where an answer appears.
No other path carries a lead's text to a member outside a hand-out and a
Return.

**A card says which decisions it takes, and a row carries what a view needs
to foresee a refusal (store contract, follow-up).** `OpenCard` takes an
optional `decisions` list of at most four short tokens; the store keeps it
and returns it on the card's row, and a card opened without it takes
`acknowledged` alone. An integration card and a pipeline acceptance card
return their fixed pairs. `DecideCard` refuses a decision the card does not
list. Every opener states the decisions something reads: a decision that
nothing acts on is not listed, so a view never offers it. A queue row
carries the attempt's holder and the kind of a pending decision, and a
member row carries whether the person stopped it; all three are additive
fields of the pinned replies.

What the review of the card decisions added (2026-11-12). A card's list
is true for as long as the card is open: its opener keeps an open card
only while the list it would give now equals the card's own, and
otherwise closes that card and opens the current one in the same step; an
open card's list is never changed in place. The case that showed it: a
launch waiting on its backoff opened a card with retry, the launch then
failed for good, and the kept card went on offering a Retry that nothing
read. The holder a row shows is the holder the store's own decisions use,
one definition: a hand-out that was only reserved, or was refused or
superseded, gives nobody to return to, and a Return there is refused,
with Reassign and Cancel still open to the lead. And an optional field
that is added to a stored operation is left out when it is absent, so
that a request saved before the field existed replays byte for byte: the
list's field was first written as null, and a card request saved by an
earlier build was then answered as a different request instead of with
its saved reply.

**A timer fires once and only while its condition holds (package 10).**
The store writes a timer's inbox item or owner card in one operation that
rechecks the condition from stored facts in the same transaction, under a
uniqueness key of the team, the timer, and the node with its generation or
the inbox item. Times the store holds are read from it. Times only the
service saw (a member idle, a lead not working) live in memory, so after a
restart such a timer may be late and is never early. No timer fires while
the team is paused or stopped, and the operation that ends a condition
closes the timer's open card in the same transaction. An undecided report
makes a lead inbox item after `remind_lead_minutes` and an owner card after
`remind_person_minutes`. `answer_escalate_minutes` belongs to milestone 3.

**A cancel decision is carried out by the service (packages 6b and 8).**
The specification says a lead's Cancel moves the task to cancelled and that
Stop cancels every unfinished task; the store records both as a pending
decision of kind cancel, and until 2026-11-12 nothing carried it out. A
fifth service task command does: `hgs task team-cancel REF --json` reads
`{schema, team, node}` and answers `{schema, team, node, state, error}`
with `cancelled`, `released`, `refused` or `cutting_over`. Its only
authority is that stored decision for the node's current generation. It
does what the task document can already say, because existing commands do
the work and a cancel exists there only for work that is running: a task
that is Active with a known delivery is cancelled by the existing writer,
with the notice it already sends to the holder (`cancelled`, also when the
task is already cancelled); a task with nothing running (Backlog, To do,
In review) loses its team key and stays in its stage as an ordinary task
(`released`); an Active task whose delivery is unknown, and a Done task,
are refused and nothing is written. No writer is added that would mark a
task that never ran as cancelled. The actor runs the command for each
pending cancel decision, in a live team and after a stop, and records the
outcome with a service-only operation, `RecordCancel {node, decision,
state, reason}`: `cancelled` and `released` both end the node as
cancelled in the team, exactly as the observation of a cancelled task
does; `refused` settles the decision and, in a live team, files one lead
inbox item. The read is `Store::pending_cancels(team, limit, after)`. It
ends no agent. Two later paragraphs of this section change this one: the
command also answers `stale` ("A cancel acts on one attempt"), and in a
live team a `cancelled` answer writes nothing to the store, because the
task's observation ends the node ("In a live team a task's end is recorded
by its observation alone").

**After a stop Zerus cancels the unfinished tasks, releases them and ends
no agent (package 10).** For each node the cancel comes first and the
release after it is settled. The
service clears the team key of the stopped or archived team's open tasks in
bounded pages through `team-clear`; an answer is recorded so that the node
is not visited again, and no answer is tried on a later pass without
starving the rest of the page. A member launch that completes after the
stop, the archive or the member's removal is recorded as provenance and
gets one owner card naming the member and the session; the store never
treats that session as a live member. The record of a clear is one
mechanism for both of its users: the same service-only operation and table
take the answer for a task of a stopped or archived team (package 10's
page) and for a released stage of a live team (package 8, part C), and a
bounded read, `Store::released_stages(team, limit, after)`, returns only
released stage nodes whose clear has no final record, with the task to
name: the bound task, or the stored creation request ID of a creation that
was saved and never bound. Neither read walks settled history.

**The store follows the task's attempts (packages 6 and 10, after the
confirmation of package 10).** The service reads only the newest state of a
task, so two observations can skip what happened between them: a task that
was cancelled, moved back and sent again looks active before and after, and
an answer about the earlier execution can arrive after the later one was
observed. The task document numbers its attempts. An observation carries
`attempt`, the highest generation any attempt of the task claims (0 without
one), `attempt_id`, the ID of its current attempt (empty without one), and
`attempt_conflict`, whether two attempts claim the highest generation; the
store keeps all three with the observation. An empty ID alone is not a
conflict: a task whose only attempt was never admitted has a number and no
current attempt, and that is an ordinary pair. An attempt the team hands out
has its effect's request ID as its ID, so the store knows its own. The
identity is the pair: an attempt that never started still claims a
generation, so the current attempt can change while the highest generation
stays (measured on two replicas of one task, 2026-11-12). When an
observation shows an active task whose current attempt is not the stored
one and is not an effect of that node, a person started a new execution,
whether or not the number grew, and the store does at once what the two
observations it missed would have done in turn: the end of the earlier
attempt, then the person's restore. Pending decisions of the node are
settled, the node moves to its next generation and the event is
`person_override`. A node that is already finished takes the restore it
takes today. A task whose attempts conflict has no current attempt. The
store records that observation as it came, with its flag, and while the
newest observation is a conflict no answer about an attempt is applied; a
kept earlier pair would let a late answer about that attempt through
(measured, 2026-11-12). A conflict of a task that still runs or waits
decides nothing else: nothing of the node changes until an observation
without the flag arrives. A task that has ended, cancelled or Done, is
applied as today whether or not its attempts conflict, because its end does
not depend on who executed it. The store remembers the attempt its node
runs under, for one purpose: when an observation shows a running task, the
same attempt changes nothing, the node's own effect is the team's, and any
other is a person's new execution. That memory changes only when the node
takes an execution in: its own effect is observed, a person's attempt is
taken over, or the upgrade seeds it from what was stored. An attempt that
is merely observed while the task does not run, a send that is reserved
and not yet delivered, is not remembered, so that it is still judged when
it starts; remembering it early let an old cancel decision reach the
person's execution (measured, 2026-11-12).

A cancel acts on one attempt. `team-cancel` compares, under its locks, the
document's highest generation, current attempt and conflict flag with what
the store's newest observation of the node holds. When they differ, the store has not observed what the
document now holds: the command writes nothing and answers `stale`, and
the service observes the task before it asks again. Its reply names what
it acted on in `attempt` and `attempt_id`, `RecordCancel` carries both, and
the store applies a cancelled answer only while the newest observation
of the node's task still shows that pair; otherwise the operation is
refused and writes nothing. An
answer about an earlier attempt therefore never ends a node whose task runs
again, and both orders of the answer and the observation leave the same
state. No service command finishes, reopens or cancels a task whose current
attempt is not the one the store observed.

A cancel also never reaches an execution that the store has not judged. The
store judges an execution at the first observation that shows the task
running under it: its node's own effect is taken in, a person's is taken
over and the older cancel is settled with it. A send that is reserved,
observed while the task waits and delivered afterwards keeps all three
fields, so the comparison alone let an older cancel end a person's run
(the review of the task side, 2026-11-12). In a running or paused team
`team-cancel` therefore answers `stale` also when the document runs and the
newest observation did not yet show it running; the observation that
follows decides, and the team's own hand-out whose start was not yet
observed costs one more round. In a stopped or archived team, where no
observation follows, an equal attempt is accepted only when the document
does not run or the observation already showed it running; a running
document whose start was not observed is acted on only as the node's own
delivered effect and is otherwise refused with `attempt_changed:`. The
command reads the observed stage and nothing else of the store's memory:
the rule for a person's execution stays in the store.

**Only a layout that reached the integration branch is upgraded.** The
store's extension marks its layout, and during the reviews of package 10
that mark moved through six values, each with its own step and its own
seeding rule for what the step before had stored. None of those layouts
was ever merged or installed; the steps between them exist only for the
implementers' own test stores, and every round found one more combination
of them that the seed got wrong. The merged extension therefore knows two
stores: one from before the extension, which it extends in one
transaction to the current layout, and one at a layout that was on the
integration branch, which it steps forward. A mark that only an unmerged
development build could have written is refused with a sentence that says
so, and nothing is written; a newer mark is refused as before. A store
from before the extension holds no attempt of any task, so its first
observation is judged against no attempt, as for a task that never
recorded one. The observer writes that observation also when the task did
not change: it skips a task only when the stored revision and the stored
three fields all equal the document's. Skipping by the revision alone left
a task that nobody touched after the extension without its attempt for
ever, and every command for it answered `stale` on every pass (the review
of the task side, 2026-11-12).

**In a live team a task's end is recorded by its observation alone.** An
answer and an observation are two reports of one fact, and every rule that
let both write it produced an order in which they disagreed. The first
version let a `cancelled` answer end the node; the second kept the task
bound after the answer so that a later observation could still restore it.
Both failed on the service's real paths, found by end-to-end tests on
2026-11-12: the observer finds a task's node through the live binding
only, and the service's pass observes the nodes of the live queue only, so
an ended node was never looked at again. The rule now has one writer. In a
running or paused team a `cancelled` answer of `team-cancel` writes nothing
to the store: the service observes that task at once, and the observation
ends the node and settles the decision, or, when a person has started the
task again, takes the new execution over. A `stale` answer leads to the
same observation. Until an observation has settled it the decision stays
pending, so an interrupted visit is simply repeated: the command answers
`cancelled` again for a task that is already cancelled and writes nothing.
The store refuses `RecordCancel` with `cancelled` for a node of a live
team whose task is bound. It takes that answer where no observation can
follow: for a node without a task, and in a stopped or archived team,
whose bindings are gone. `released` and `refused` are recorded as answers
in every state, because no observation says them: a released task has
lost its key, and its binding is released with the record. A release is
recorded also while the newest observation is a conflict of attempts: it
says that the key is gone, not who executed an attempt, so only its pair
must equal the newest observation's, and the conflict gate is the
cancelled answer's alone. In a stopped or
archived team no observation follows either, so `team-cancel` never
answers `stale` there: it acts when the document's current attempt is the
one the store last observed or is the node's own effect that was delivered
and not yet observed, and otherwise it refuses with a reason that begins
`attempt_changed:`, because a person took the task after the stop. The
store takes the answer by the same rule, or the two disagree for ever: in
a stopped or archived team a cancelled or a released answer is recorded
when its pair is the newest observation's, or when it names an effect of
that node's current generation, a hand-out or a reopen, with a number not
below the observed one. After a stop a cancelled answer is not held
back by a conflict of the newest observation either: the command answers
cancelled for a conflicting document only when that document is already
cancelled, and refuses one that is not; a task's end does not depend on
who executed it; and nothing observes again. The first version compared
with the observation alone and kept the conflict gate. A team stopped
between a hand-out and the observation of its start kept its cancel
pending for ever, the service replayed the refused record every fifteen
minutes and the cleanup never reached the task's key; and a conflicted
task that the person cancelled around a stop got a new cancel command on
every pass, because the service took the conflict for a visit that an
observation would settle (the review of the actor, 2026-11-12). The
service has its own exits as well, for any refusal a later rule may
bring. A visit of a stopped or archived team ends only with a recorded
outcome: a saved refusal of a cancel's record there is recorded once as a
refused outcome, with what the command answered and what the store said,
and is never replayed; in a live team that holds for a visit that did not
change. A saved refusal that only says "ask again", a stale revision, is
not a refusal by a rule: the request's ID is spent and nothing was
decided, so the same record is made again under a new ID; taking it for a
rule's refusal recorded a true answer as refused after one lost write
(the delta review, the same day). And a live team's cancel that keeps
answering stale although its observation changes nothing is given up
after three such answers, as a refused outcome that reaches the lead. The
count is of answers that teach nothing new, not of attempts: an answer
that says "ask again" for a reason that ends by itself (a project's
cutover hold), a command that provably did not start, and an answer whose
observation failed are retried with a backoff and a card while they last,
and never become a refusal by repetition; the first version counted
attempts and gave up on a stopped team's task seven minutes into a
cutover, clearing its key while it still ran. A row that has halted is
its own state: its siblings and the other teams are served in the same
pass. Once
the store has observed a task cancelled, the team has let it go. Giving
such a task back to a team is not possible yet: a task keeps its cancelled
mark when it is moved back to To do, until it is sent, started or reopened,
and the intake refuses a cancelled task so that a duplicate cannot pass a
cancellation. That is a known limit with a tracked follow-up.

**A node with a bound task leaves the live queue only on an observation
that agrees.** The service's pass observes the nodes of the live queue, so
a node that is archived while its task is bound and not ended is lost to
it. Every writer that archives a node therefore asks the newest
observation first: the task has ended, cancelled or Done, and its attempts
do not conflict. A fact that arrives when the observation does not agree
is still recorded, because it happened, and the node stays in the queue
for the next observation to decide. The case that showed it (the
confirmation of 2026-11-12): a merge recorded for an accepted task after a
person had started it again with conflicting attempts archived the node as
accepted while the task was active. And a recorded answer that ends a
pipeline's stage does for the pipeline what the observation of that end
does: the parent returns to the lead, whether the stage had no task and
was recorded as cancelled, or was released. And a cancel that is still
pending is settled by its own answer or by an observation, never by the
archiving of its node: a stop that found a kept node, accepted in the
store while its task ran again, created the cancel, released the binding,
and the archiving step then settled that cancel with the rest, so the
service never asked for the task to be cancelled and the cleanup passed it
without an answer (the same confirmation). A node with a pending cancel
stays where the cancel step finds it. One merged path does not follow this
yet: when a lead's Return, Reassign or Cancel of a parent retires its stage,
the retirement settles every decision of the stage, a person's pending
Cancel of it included, and the stage's task only loses its key, so work
that runs is released and not cancelled (found by the delta confirmation of
2026-11-12, tracked). The rule it will follow: a retirement leaves a pending
cancel pending; a stage that is retired while its task runs gets the
service's own cancel, as a stop gives a node, and is archived and cleared
after the answer; a stage with nothing running is released as today. And a
kept node can wait for a lead's earlier Cancel as well as for a stop's: a
node that is Reported with its task Done before the merge takes a Cancel,
and is kept as accepted when the integration is then switched off.

**The service carries out an acceptance only while the newest observation
agrees with it (packages 8 and 10).** The rule above keeps an accepted node
in the queue while its task runs again or its attempts conflict, and the
node's acceptance is still pending there. The actor looked at the stored
stage only and sent a finish request for such a task (the confirmation of
2026-11-12). Before it finishes a task, starts a merge or reopens, at
scheduling and again in the admission of the effect, the actor asks the
store, and the store owns the answer through two reads of one shape.
`reviewed_task_current(team, node, generation)` is true while the
generation is current and the newest observation has the key ours, the
stage In review, no cancellation, no conflict of attempts, and as its
current attempt the execution the node took in; the finishing step uses
it. `accepted_task_current` is true in the same cases and also when that
observation shows the task Done, and a new merge uses it. A reopen and a
hand-over ask a third, `own_task_current`: the same without the stage, and
true also while the task has no current attempt. The stage is the
command's business there, because the command hands over a task that
runs, is In review or is Done and reopens one that is In review or Done,
and its refusal is recorded and seen. The first version sent reopens
through the In-review read: a lead's Reassign of a running task, which is
delivered as a hand-over, was then never carried out, and its pending
decision refused every other decision of that node (the review of the
actor, 2026-11-12). A step that a read holds back is not a success: when
the read stays false for several passes the step's row fails with a
sentence that names the node and what the newest observation shows, and
recovers when the read turns true. With
integration by branch the finishing step waits for the merge, so an
ordinary merge finds the task In review; but a task that a person closed on
the board after the accept is observed Done before its merge, its node
waits in the queue for that merge, and a question that knew In review only
would never merge it. The end of a task does not depend on who executed
it, so Done agrees under any attempt, and the conflict flag counts on Done
as it does for leaving the queue. The settlement of a merge that already
ran does not ask: its receipt is a fact. Known limit: an accepted task
that comes back In review under another execution without ever being
observed running (the service was down for the whole of a person's run) is
neither finished nor merged, and an accepted node takes no Cancel; the
person's own move on the board (closing, cancelling or starting the task
again) resolves it.

**A decision that has not started can be replaced (after the review of
the actor).** The rule that a node takes one unsettled decision at a time
protects a decision the service is carrying out. It also locked a node
whenever the service could not start one: a Return to a member that the
person then stopped or removed, a Return to a member busy elsewhere for
hours, a reopen held by a conflict of the task's executions. Four findings
of 2026-11-12 were this one lock. A lead or the person may therefore give
a node a new Return, Reassign or Cancel while its earlier Return or
Reassign is still pending and nothing of it has started, that is, no
reopen has been reserved for it; the earlier decision is settled as
superseded in the same operation and the journal says so. The member's
own earlier hand-out may still be open then, its report not yet recorded:
that is not the decision's start. A decision that has started is not
replaced: the new one is refused as before until the outcome is recorded.
A Return that replaces a Reassign makes its own target the node's
assignee, so that the superseded Reassign leaves nothing behind. A Return
or a Reassign aimed at a member that was already removed is refused when
it is decided. An acceptance is not replaced in
this milestone. And removing a member settles the Returns and Reassigns
aimed at it that have not started and hands their nodes back to the lead
with one inbox item each, as a reopen that could not be delivered does. A
member that is stopped or busy keeps them: that wait is intended, and the
lead can now decide otherwise. The same holds for the Returns the store
writes itself, after an integration conflict or a stage's failed verdict:
when their holder was already removed the fact or the verdict is recorded
and the node goes to the lead instead. And a Return that has not started
and whose holder reads as nobody is handed back in the operation that
makes it so; the service is never asked, because it makes no reopen for a
task without a member. There are three such operations: a member's
removal, which finds its Returns by the newest effect's member whatever
that effect's state (a reading used only to hand back, never to deliver);
an outcome recorded as refused that leaves nobody holding the task; and,
once, the upgrade to the layout in which only a delivered hand-out gives a
holder, for a Return that an earlier build admitted while the hand-out was
only reserved.

**A reader of a reply ignores what it does not know.** The contract's types
refuse unknown fields, and that stays the rule for what a person or a
program sends: a request, a saved template, an imported catalog. A reply
is different: reads gain fields, and a client built before a field existed
must still read the rest. The client core therefore reads every Teams
reply through types of its own or through a pass that keeps the fields it
knows, the catalog and its template rows included, although the same
shapes are strict on the way in. Found on 2026-11-12: the catalog reply
gained a digest in each template row, and the merged client core, which
read those rows through the strict template type, refused the whole
catalog. A merge that changes a reply runs the client core's tests against
the new fixtures. One kind of unknown field is never dropped: a field of
what the person is asked to confirm. A permission in a template's
delegations that the client does not know is kept as a fact of that row,
and Create stays blocked with its reason; a tolerant read that removed it
made the form show five permissions and allow a create (the desktop's
review, 2026-11-12). And what the client sends is strict at every level,
the action inside a call included: an unknown field in a request is
refused, never silently left out.

**A failed intake leaves the task's team key as it found it.** Sending a
task to a team writes the key into the task and then asks the store to
queue it. When the store's answer is certain that nothing was written, a
refusal or `store_busy`, the intake puts the key back to what it was
before the call: absent for a first send, the same team for a repeat of a
task that is already queued. When the result is not known,
`storage_failed`, the key stays: the store may hold the node, and a repeat
finds it by the task's identity. The intake never clears a key it did not
set. Clearing it on every error made a failed repeat look like a person
taking the task out of the team: the observer released the node, and the
next send queued the task a second time (the desktop's review,
2026-11-12).

**A row says whether its pending decision can be replaced.** The queue and
board rows carry `pending_decision`, and a view that disabled every
decision while it is set would lock the person out of the very way out the
replacement rule gives. The row therefore also carries
`pending_replaceable`, always written and false when nothing is pending,
computed by the rule the operations use (`DecisionState::replaceable`: a
Return or a Reassign that is still pending, never an acceptance, never a
decision whose reopen is reserved). A view keeps Return and Cancel offered
when it is true, says that the new decision replaces the pending one, and
reads an absent field as false (2026-11-12, with the desktop's card lists).

**A hold has an owner.** `nodes.hold` was one flag written by an explicit
Hold, by a pending Return or Reassign and by a hand-back to the lead, and
cleared by whichever of them ended next. Two reviews of 2026-11-12 found
the same fault from two sides: a Reassign's later settlement released a
hold the person had set before it, and a replacing Reassign that settled at
once kept the hold of the decision it superseded. The rule: an explicit
hold belongs to who set it and ends only by an explicit Release, a person's
hold only by the person's; no decision, settlement, outcome or hand-back
clears it. A pending decision's hold belongs to that decision and ends with
it, however it ends: delivered, settled without its reason, or superseded
by a decision that settles at once; a replacing decision that stays pending
takes the hold over. A hand-back's hold belongs to the lead's next
decision, which takes it over or ends it. A node under an explicit hold
still takes decisions and stays held when they end. One function answers
whether a node is held and one place writes each kind.

**An older store's holds are classified without a clock.** A store written
before a hold had an owner carries one flag per node and no record of who
set it. The first classification read event times, and two honest
sequences read the wrong way round (the second review of the reassign
route, 2026-11-12). The step that gives each held node its owner reads no
clock and no archive. An unsettled decision comes first. Otherwise the
last exactly recorded event of the node decides, in the order of the
journal's own sequence, in one bounded pass over the team's live journal:
a Hold is its actor's, a Release ends it, a current hand-back is a
hand-back. A saved reply names a row by number alone and the number can
belong to several tables, so a trace is placed only by the reply of the
kind of operation that files it. Whatever cannot be placed exactly is the
person's hold, even where an earlier record was the lead's: a removed
member's item, a pipeline active on a stage, and the item the step to
layout 10 wrote itself. The person may then have to release a hold a lead
once set; the step never gives a lead a hold that could be the person's.

**Status shows what the service keeps retrying where no card can.** A
stopped or archived team takes no cards, so a cancel that keeps failing
there, a halted row, the terminal page's fault at its retry cap and a stop
cleanup that cannot finish were visible only as the service's own rows.
`hgs team status` lists them as `service_items`: one item per failing
subject, written and cleared in the transaction of the row it mirrors,
with its reason, failure count and times; a page of 20 and a capped count.
Items exist for stopped and archived teams and for service-wide conditions
only: a live team's rows have their cards and nothing is shown twice. An
item is current only while the service runs and is not overdue; otherwise
it reads as of its last try. The service's cleanup pass removes an item
whose row is gone. A reason is bounded at 512 bytes and has its control
characters removed where it is written. Key refusals are listed for the
team's life; cancel refusals only while the team is stopped or archived,
because in a live team the lead has the inbox item.

**A ready task that no member can take goes to the lead.** The plan
kernel names such nodes (`ready::unstaffed`: no member who is not removed
could take it; a busy, paused or not yet created member still counts), and
nothing read it: a task whose assignee was removed, or whose role nobody
has, waited in To do and nobody was told. The service's hand-out step
compares the kernel's set over the uncooled readiness view with the
store's open items and calls one store operation only when they differ;
the store recomputes the set itself and opens and closes the items in one
transaction. One item per node, to the lead, quoting the kernel's reason;
it closes when the node leaves the set and is told again once if it
returns. A pass with nothing changed writes nothing, and a race between
the two answers with its own code that counts as no failure. No card: the
`silent_lead` timer is the path to the person. A stage that loses its
reviewers takes the path a stage takes at its creation: it is retired and
its parent goes to the lead with the kernel's reason. Nothing in stopped,
archived or paused teams.

**An unstaffed item stands for an episode, and the cap counts episodes.**
The first head took "told" to mean "an open item exists", so an item the
lead closed while its node stayed unstaffed was written again on every
pass, an open item kept its first reason after the gap changed, and a
store that head left with more than 500 closed items on one node refused
every reconcile for ever (the reviews of zerus-63rru.45, 2026-11-12). An
episode is a node, its generation and the kernel's reason, and the lead is
told once per episode. Its item is the node's newest unstaffed item that
the reconcile has not ended, by sequence and never by the clock. Only the
reconcile ends an episode, with a reply that says why: the node left the
set, or its reason or generation changed, and a changed reason then gets
one new item. An item closed by the lead still stands for its episode and
is not told again. The cap of 500 is judged on the current episodes, and a
queue row's unstaffed sentence is the current episode's. An older unended
item of a node stands for no episode: a service-only operation ends at
most 200 of them a pass, oldest first, in a transaction of its own before
the comparison, and its answer when none is left counts as no failure.
More current episodes than the cap is a state the product cannot write,
since a team admits at most 500 open tasks; the reconcile refuses it by
name on every pass and nothing repairs it, which is the guard failing
closed and is accepted as that. A person's Hold placed before the stage
exists survives the stage's reconcile and its retirement: the stage path
ends only the pipeline's own hand-back. A member whose row the service
cannot read still counts as one who could take the node: readability is
the service's fact, the unreadable member's card tells the person, and the
lead's item does not say it (zerus-63rru.81).

**A removed member's work goes back to the lead.** A task handed to a
member that was then removed (ArchiveMember, the only removal) was held for
ever and nobody was told (zerus-63rru.77, 2026-11-13). The removal settles
the member's open effect of a task that has not started (reserved,
submitted, uncertain, cutting over, and a reopen already reserved): the
effect is superseded, the node is held by the hand-back and the lead gets
one item per node that says the delivery's result is not known, since a
delivery child past its reservation still types once. A started task is
handed back too, with an item saying the session was not told and may still
work in its worktree, and what Reassign and Cancel do; the removal requests
no cancel, because a Stop's cancel ends the whole task. A report the removed
member writes later is not recorded; the lead is told once, by a
service-only operation, with no retry and no card. A reopen that ends
refused now tells the lead for any member. One rule for every node kind: a
pipeline stage is handed back like a task and, held, is not ready, so the
unstaffed reconcile leaves it alone until the lead decides; for a stage the
item says that the lead cannot cancel it and what the person's Cancel does.
The actor ends the ledger row of a delivery whose effect the store no
longer holds and does not count the refusal of a superseded effect as a
failed hand-out. Left open: a reservation for a paused, stopped or unbound
member (zerus-63rru.85), a paused member's started task (.86), an unbound
member's open effect (.87), and Route of a stage (.84).

**A terminal that holds input is a fact about the member.** The hand-out
command's terminal gate refuses to type into a terminal that shows a draft,
a dialog or a question, and the service counted that refusal as a failed
hand-out of the task: a failure row, retries of the same member, in the end
a card about the task (zerus-63rru.79, 2026-11-13). The command now types
the cause: a refusal because the composer is not an empty prompt, or
because the record's phase is a question or an approval, answers
`terminal_waiting`; every other refusal stays an ordinary one, the dialogs
read by their own detectors included (zerus-63rru.83). The actor reads the
cause, never the sentence, and records a fact about the member in its
ledger: while it stands the member is not free and the task goes to another
free member; it ends on a submitted delivery, when the member's row says
another session or no longer waiting (its run, launch and conversation, its
phase and its open-question count), or by the retry table's growing delay.
Work counts and activity are not part of that stamp, since they change
while a terminal waits. Past the unreadable-member period the card says the
member's terminal waits for input. The journal's hand-out outcome row says
the outcome and the cleaned reason. A refusal of an effect the store has
already settled, such as a removed member's, is no failure either.

**A project limit's refusal is a fact about the team.** A hand-out refused
by the project's own limits (`agents_other`, `agent_limit`) was counted as a
failed hand-out of the task and repeated for every task and member
(zerus-63rru.23, 2026-11-13). The command types the cause
(`project_limit`); the actor reads it, never the sentence, and keeps one
ledger row and one card per team, opened at the first refusal. While the
fact stands nothing is delivered except probes at 1, 2, 4, 8 and 15
minutes. One command answer is counted once, bound to the delivery's
identity, so a replay after a crash does not count it again. The fact ends
only on a command answer that is not `project_limit`: a submitted probe, or
the command's own refusal for another cause, which is then handled as that
cause. An uncertain probe and every non-start, the fifth that is converted
included, keep the fact, and the next probe waits as usual.

**A child's end is final for its ID.** Two hook processes race for a
session's record, so a child's Start or tool event could be applied after
its Stop and leave the session with one active child for ever: the member
was never free and no card said why (zerus-63rru.78 and .88, 2026-11-13).
A non-start event of an ended child is history only. Claude and Codex name
every subagent invocation with a fresh ID, so nothing revives an ended
child of theirs, a late Start included; a Kimi profile child resumes under
its name, so only Kimi's own start revives it (the race of such a start
after its own stop is zerus-63rru.93). An interrupted child is not an end,
and its own tool events resume it. No stored process is adopted across a
run boundary. A child whose process may still run but is unbound is counted
in `subagent_process_unknown_count`, which only the Teams fleet reads: the
member is not free and its presence reports nothing until a reconcile
settles the process, while the shared listing row keeps its numeric counts,
since unknown work never establishes current work for desktop and mobile
status. Retirement clears the process projection as an ordinary exit does.

**A recovery removes only the row it owns.** The service's gate recovery
closed its card and then removed whichever ledger row was current, so a
second runtime that had failed the same check in between lost its fresh
row and its card was tracked by nothing. A recovery removes the row only at
the version it owns, the entry version or the version its own notice
replay wrote; if another writer has moved the row it removes nothing and
does not fail the pass (the actor's cards delta, 2026-11-12).

**A test row of another component's value comes from that component.** Two
defects of 2026-11-12 passed their reviews and every mutant because the
tests read hand-written values the product never writes: the standing
folder-trust confirmation compared a card's conversation with an empty
string where a real Claude startup card carries null, and the service's
availability and presence read the fleet row keys `subagents_active` and
`state`, while a live terminal row carries `subagent_active_count` and no
`state` at all, so with a real service no member was ever free. A mutant
shows that a test notices a change of the code, never that its fixture is
what another component writes. The rule for everything the Teams service
reads from elsewhere (fleet rows, inspect views, task and launch
observations, DeepSeek records): a test's value is built by the real
producer, or from a fixture shared with it, through one helper that changes
named keys only; a guard test fails when a seeded value carries a key the
producer does not write, and when a reader's key list names one no producer
has. A reviewer traces each such key to the line that writes it and builds
its probes from what that line writes. Unknown stays unknown: a null or
missing count is not zero.

**A hosted DeepSeek tool call proves its own session; DeepSeek seats belong
to M1 (owner decision, 2026-11-13).** One DeepSeek host process serves every session of
its agent home, so process ancestry reaches only the host. The hosted caller
proof (`state::session_graph::caller_hosted`, zerus-ppd26 Part B, merged
2026-11-12) ties a tool call to its session through the host's per-call
token. For Teams: a `dsh/` claim takes that proof only; its refusal refuses
the team command with the proof's own sentence, an archived binding
refuses, and nothing falls back to another proof or to the person. A child
agent's command is its top-level session's, as an in-process subagent's is
elsewhere; a fork is its own session. A host started before the proof
existed has no tokens and proves nothing until its next own start; no host
is restarted or reloaded for it. Whether the host of an agent home can
prove its calls is asked at the member launch in the desktop, before the
worktree and again before the session is bound, and not at team creation:
the command line may see another agent home. The roster admits DeepSeek;
the member launch requires the resident host to advertise call tokens,
input witnesses and hosted lifecycle answers before worktree creation and
before binding. A missing host may start normally at launch, never during
observation. The task route retains its exact request before the first
witnessed call, with a whole-call budget of twenty seconds under the actor's
thirty-second child limit. Recovery reads only that request: Absent may offer
it with the same ID, Conflict sends nothing and returns to the lead, an
Outcome keeps its fixed classification, and unreadable evidence gets bounded
later reads before becoming uncertain. A pre-marker unsafe_content or
host_refused leaves recorded() Absent; Teams retains its final mapped reply,
and only a crash before that final write permits the same stored input again.
Unsafe content, canceled input and dropped-after-claim return the task to the
lead at once without automatic reassignment; host disposal requires a fresh
member observation; every Unknown stays uncertain without replay or promotion.
Doorbells use ordinary hosted send and its exact native receipt; cancel notices
use a witnessed hosted message with explicit refusal or uncertainty. The host
reports a member's end only from its own agent/disposed event, bounded to 4096
records per host; an unreachable host or an absent lifecycle record is unknown.
The standing terminal-confirmation scan skips DeepSeek and never resumes it
from a failed terminal observation. OpenCode and platforms without the proof
stay refused as before.

**No other session is proven below a host.** A marked claim that is not a
DeepSeek claim took the ancestry proof without asking whether the command
ran below a recorded resident agent host, so a hosted call that carried
another session's markers was accepted as that session: a host is a child
of the command that launched it for as long as that command lives, and an
older host keeps the markers it inherited (the independent review of the
DeepSeek members' first commit, 2026-11-12). The rule: a command below a
recorded host can only be a session that host serves, and only a DeepSeek
claim's token proves one; any other marked claim there is refused with a
sentence of its own. The person path and the marked branch ask one check,
which reads the host records first (a computer that records none walks no
process) and then walks the process table alone, matched by process ID and
birth; it reads no environment and no marker, and when it cannot tell
(records unreadable or over their bound, a birth unreadable, a walk past
its bound) both refuse. Its limit is the hosted proof's own: the check
stands on the host records, so a call that removes or edits its host's
same-user records is no longer seen below that host. That is deliberate
change of the state directory by a same-user process, which no proof built
on records or ancestry can stop, and it is stated where the rule is
stated. The function's body is replaced by the shared helper from main
after the next sync, with a refusal of its own for a command inside a
separate process namespace.

**A held step's card follows the hold's age, not its reason.** A step the
service holds back (a finish, a merge or a reopen it cannot carry out yet)
had its card keyed by the reason, so a change of reason closed the card and
restarted the count, and a reason that alternated every minute never opened
one (the second review of the actor, 2026-11-12). A hold is a decision and
its generation. It is silent for its first period, three one-minute checks,
whatever its reasons do; the first card opens when the hold's age passes
the period, with the sentence of that pass. A change of reason neither
restarts the age nor closes the card: the card is replaced when the new
reason has stood for the period. A dismissal lasts while the reason is the
same, Retry checks again on the next pass, and the card closes when the
hold ends or another decision or generation takes the row.

**One reader of a fleet row, and its keys are the producers'.** The actor
read a member's row by names no producer writes (`subagents_active`, and a
`state` that a live terminal row never carries) in tests whose rows were
written by hand, so a real idle member was never free and never handed a
task; the first run against real agents found it (2026-11-12). One module
reads a fleet row (`actor/fleet.rs`) and lists every key it reads. A member
is live when its `process_state` is `running`, its `runtime_state` is `live`
and its `state` is absent or `running`; it is free when it is live, ready
and idle and its three counts (active children, background work, open
questions) are zero. A count that is null, absent or of another type is
unknown: the member is never free and its presence reports nothing. The
tests' rows come from the listing's own steps through test-only seams; one
guard compares the committed fixture's key paths with them and another
fails when the reader names a key no producer writes.

**A member is found by its session, not by its launch's evidence.** A
member is bound to the launch that started it, and every later read of the
member (is it free, which run is current, who is its parent) went through
that launch's start file, which the product removes when the launch is
confirmed finished and after seven days at the latest. A member whose start
file was gone could not be read and was never handed a task (found by
tracing the producer, 2026-11-12). The store keeps a durable handle beside
the binding: the session's own ID, written once by the service when the
binding is verified, in a companion table that an older build ignores, so
the layout does not move. Every later read resolves the session's current
record through the handle with bounded point reads and requires that
record to carry the bound launch and the member's agent and folder. What
does not resolve is named and never free: `Gone` (archived, forgotten, or
rewritten without its ID), `Changed` (the record carries another launch),
`StartedFresh`, `Unavailable` (a fault or a bound) and `NoHandle`. A fresh
start ends the binding: it does not carry the launch's identity to the new
conversation, because that identity is also the task executor's key and
carrying it would change what a fresh start means for every task; that is
the owner's decision and is not taken here. One session is one member's. A
member bound before the handle existed gets it on its first verified free
check, from the launch ledger, a completed resume or the evidence while it
lasts.

**A member a ready task needs and that cannot be read gets one card.** A
member whose state could not be read (its session record gone or changed, a
count never reported, a fault) was never free, and nobody was told: a ready
task waited behind it without a word (2026-11-12). The reader gives one of
three answers, `Free`, `Busy` or `Unreadable` with its fact. A count names
the member unreadable only when the member is live, ready and idle, every
known count is zero and this one is unknown; a working member with an
unknown count is busy, and so is a member with no row. Whether anybody
needs the member is the kernel's own selection run once more, with the
unreadable members counted as free and placed after every known member and
with tasks already handed out held, so the card never names a task another
member is taking. The row uses the held steps' period: silent for the
period, then one card with the sentence alone and the decisions Retry and
Acknowledged; a changed fact or task replaces the card only when it has
stood for the period. The row ends on the first pass in which nobody needs
the member: no ready task, a paused team, the member readable again,
another member taking the task, the member stopped or archived. A team's
Stop closes the card in the Stop itself.

**A hand-out that fails before its reservation says that nothing was
sent.** The task command that hands a team's task to a member answered
every failure alike, with an error exit that the actor reads as uncertain.
So a failure that provably came before anything durable (a team store that
was busy, a setting that could not be read) raised a card and was never
tried again (2026-11-12). The window is exact: it opens when the command
has seen, under the task's admission, that no attempt exists for its ID,
and closes at the reservation's save; nothing in it is durable. Inside it a
rule's refusal stays a refusal and a stale observation stays stale; every
other failure answers `not_started` with its cause, exit 0 and the echoed
ID, which the actor reads as nothing sent and tries again with its
backoff. A member session that refuses the prepared input (its identity,
run or conversation changed) is a refusal, not an uncertainty. Outside the
window nothing changes: a failure before the attempt read or after the
reservation is still unknown, and a recovery never sends.

**A reopen is checked by the member's launch and conversation, not by its
run.** A reopen sends a member a short message for a task it already has.
Before sending, the task command required the run and the conversation the
task had been handed to. A member resumed since the hand-out has a new run
and the same conversation: it still holds the task's whole context, but its
reopen was refused, retried and ended as a card. A member that cleared its
conversation was refused too, but only after retries and without its
reason (2026-11-12). On the team route the check compares the member's
launch identity and the conversation; every caller outside a team keeps
the same-run comparison, and a source guard lists every use of the new
one. A resumed member's reopen goes through. A member that cleared its
conversation no longer has the task: its reopen is refused and final,
nothing is sent, and the task goes back to the lead at once with a
sentence that says why, without a failure gate or a retry. A new execution
for the same member with the full prompt is not part of this.

**A subagent count is settled only when every child that may run now is
counted.** A fleet row carries a member's active subagents as a count with
a flag beside it, and the reader ignored the flag. The flag does not mean
one thing: for the hook-fed sources of Claude, Codex and Kimi's own roster
it only says that ended, interrupted or pruned children are no longer
listed, which is history; for Kimi's profile groups and for a DeepSeek
session's catalog it says that children which may be running now could not
be counted (2026-11-12). The reader decides by the row's `subagent_source`.
History never keeps a member. An incomplete count about children that may
run now is unknown: the member is not free and gives no presence, and when
a ready task needs it the unreadable-member card says that the state of
some of its subagents cannot be settled. A count above zero is work,
settled or not. A source the reader does not know is unknown, and a guard
fails when a producer writes one the reader has not classified. A DeepSeek
member is never free until the host's list carries a complete job summary:
the tests' list answer holds only the keys the host sends.

**A task already handed out is held out of the hand-out pass.** A task
whose hand-out has been submitted stays ready in the plan kernel's view
until its observation moves, and the store refuses a second intent for it.
The pass chose a free member for it all the same: that member got no other
task in that pass, and the refused intent was counted as a hand-out that
failed (2026-11-12). Before members are chosen the pass now holds every
ready task that has an open effect or a delivery pending in the actor's
ledger, as it holds a cooling task. The pending-delivery half matters
after a crash between the ledger's row and the store's intent: without it
a free member loses that pass. The hold costs one indexed point read of
the ledger for each ready task without an open effect, and ready tasks are
bounded by the team's limit of open tasks.

**Only the member's own folder-trust prompt is a candidate for the
standing confirmation.** The confirmation's engine planned a trust step
whenever a member had any pending question and took the first one, so an
ordinary question the agent asked later was refused by the policy and
became a card and a row (the first real-agent run of the trust cases,
2026-11-12). The policy decides the candidate once, by the producer's
source: the folder-trust source of the member's own agent. Each source is
a fixed literal of its producer; nothing an agent, a hook payload or a
terminal's text writes can carry it. Every other pending question is the
person's, through the session's ordinary attention, and plans nothing
here: no row, no card, no audit reservation. Beside another question the
folder-trust prompt is still refused by the policy, with its notice. A
trust row written before this change is settled history and is never
repeated.

**Outside text is cleaned where it is taken.** Git's words, file paths, a
member's report and the team-clear refusal entered the service's
sentences, cards and inbox items as they came: a control character, a line
or paragraph separator or a bidirectional override could start a line of
its own, act on a terminal or reorder what is displayed (2026-11-12). Each
is cleaned at the place it is taken, by one module with three forms. A
sentence loses control characters, separators and the nine bidirectional
embedding, override and isolate characters, and is bounded. A report keeps
its line feeds and tabs and loses the rest, before the service appends its
own lines. A path is shown as it is when it is clean, and quoted with
every such character escaped when it is blank, holds one, starts with a
double quote or is longer than its limit, so that two different names
never read the same while both are shown whole; a quoted form cut at the
limit is marked after its closing quote. The marks U+200E and U+200F and
right-to-left letters stay. A team's name and IDs and task titles are not
covered here.

**Known limit: the key of a task that was closed before the stop.** After a
stop the service clears the team key of the tasks that were open at the
stop and of released stages. A task accepted or cancelled earlier keeps its
key. On the team's home computer that key guards nothing once the team is
stopped or archived. On another computer the task guard cannot read the
team's status and refuses first, so an agent there cannot send, start, move
or edit such a task until a person clears the key; a person is never
stopped by it. Clearing every closed task of a long-lived team is a walk of
its whole archive and is a tracked follow-up, not part of milestone 1.

**Known limit, now resolved.** A stage excludes the implementer of the
target's current round and every earlier implementer of the target
(zerus-63rru.20): the store records the implementers a target has had,
written by the service's own reassignment, round-change and report
operations, and the hand-out, the stage creation and the kernel's staffing
read those records. The record per target is bounded by the team's member
bound on its own, so there is no separate cap and no full marker; the
readiness read takes only the live stages' targets through the primary key.
A person's Reassign of a stage follows the hand-out's rule and writes no
record of its own.

**Rollback limit (documented and accepted).** An older build ignores the
table and reads the store exactly as before, and its own reassignments,
round changes and reports write no record. A stage can therefore later go
to a member whose earlier round only that older build carried out. The
schema version is unchanged; a one-way bump was rejected for this reason.

## Addendum: wave 3 (packages 8, 9, 10, 13, 14 and 17)

Written 2026-11-11 against `integrate/teams` with packages 1, 2, 3, 4, 7, 11
and 12 merged. "Decisions during wave 2" binds every package below.

**What the service does, in one pass.** The launch worker of package 7
already wakes on fleet events and once a minute, pages `Store::live_teams`
and ticks each team. Package 8 adds the work steps to that tick, in this
order, each bounded and each restartable:

1. observe the team's tasks that changed;
2. take in reports;
3. integrate accepted work and finish it;
4. deliver reopens;
5. hand out ready nodes.

Packages 10 and 17 add steps of their own to the same tick after package 8
is merged. A step never continues an effect across a restart: an effect
row without an outcome is uncertain, becomes a lead inbox item and is never
sent again.

### Package 8: teams actor

**Owns:** `src/desktop/teams/actor.rs`, `src/desktop/teams/actor/`, their
ports and tests, and the call of the tick from the worker.

**Consumes:**

```rust
// package 3, to be exposed for this package
pub fn readiness(store: &Store, team: &str) -> Result<Readiness, Problem>;
pub struct Readiness { pub phases: Vec<Phase>, pub nodes: Vec<Node> }  // bounded as require_ready is
// kernel
zerus_team_plan::ready::handouts(&PlanView) -> Vec<Handout>
// package 3
Store::integration_accepts, Store::effects, Store::member, Store::node
Operation::{HandoutIntent, HandoutOutcome, ReopenIntent, ReopenOutcome,
            ObserveTask, RecordReport, RecordIntegration, RecordIntegrationConflict}
// package 6
crate::teams::tasks::observe(state, &TaskRef)
crate::teams::tasks::report(state, &TaskRef) -> Result<Option<String>>   // 6b: the holder's report text of the current attempt
zerus_replica::ProjectStore::changes_since(generation, offset) -> Changes
hgs task team-deliver REF --json   {schema, id, team, node, member}
hgs task team-done REF --json      {schema, team, node}
// package 11
integrate::{reported_commit, descends, merge, sync_base}
// package 7
desktop::teams::Worker::wake, the original child kind and its custody
```

**Deliver:**

- *Observe.* The actor keeps its place in each project's change journal
  (`changes_since`: generation and offset) in its ledger. One read per
  project per pass names the changed documents; `observe` is called only
  for a live node whose task is among them, and for every live node, paged,
  when the journal cannot tell, at the first pass after a start and on the
  one-minute pass.
- *Free members.* A member is free when its session is live and idle (not
  working, not `Background`, no pending request), the store holds no open
  effect or attempt for it, and it is not paused. Liveness comes from the
  fleet rows the worker already receives; a member whose row is unknown is
  not free.
- *Hand-out.* `readiness`, then `handouts` with the free members, then for
  each: `HandoutIntent` with a new request ID, `team-deliver` as the one
  original child, `HandoutOutcome` from its reply. Before the intent the
  actor rechecks the task's merged stage and executor, because a task can
  be driven on another computer before its key syncs.
- *Reports.* A node whose task is observed In review with the holder's
  report gets `RecordReport` with the commit read by
  `integrate::reported_commit` from the member's worktree, without writing.
  A dirty worktree is named in the report text.
- *Integration.* For each row of `integration_accepts`: `descends` and
  `merge` after one ledger reservation, then `RecordIntegration` or
  `RecordIntegrationConflict` naming the decision. An accepted node that
  needs no merge, or whose fact is recorded, is finished with `team-done`.
- *Reopens.* An open reopen effect is delivered with `team-deliver` and
  settled with `ReopenOutcome`.
- *Wake.* A change of the team store's revision calls `Worker::wake`.
- *Status.* `hgs team status` gains the service's presence and
  `desktop::teams::health` (two fields of the reply and of
  `cli-status.json`).

Every effect sits behind a trait; production adapters are this computer's
own `hgs` as an original child. Functions of about 60 lines or fewer.

The package is assigned in parts. A: the tick, observe, free members,
hand-out and reopens. B: reports, integration, finish and wake. C, after
packages 6, 6b, 9 and the CLI are merged: the stage-task step between
reports and integration (package 9's read of stage nodes that need their
task, `hgs task team-stage`), the adapters for `observe` and `report`, and
the two status fields. A refused hand-out is not tried again before that
task's observation changes or the retry policy of package 7 allows it. A
merge whose result is unknown is settled by reading (`descends`), never by
a second write while it is unread.

**Tests:** Review focus 1 and 2; each step at a restart before and after
its effect; an uncertain delivery makes one inbox item and no second send;
a refused delivery leaves no assignee; two free members and three ready
nodes; a member that becomes busy between the view and the intent; a task
driven elsewhere before the hand-out; a conflict changes no ref; a second
pass with nothing changed has no effect and reads no task document.

### Package 9: pipelines

**Owns:** stage operations in `src/teams/ops/` (a new `pipeline.rs` and its
tests), the `verdict` command in `src/teams/cli/`, and the kernel only
where a rule is missing.

**Deliver:**

- At `RecordReport` the store asks `pipeline::after_report` and
  `pipeline::staffed`. A `Stage` step creates the stage node (kind stage,
  target and round of the reported node, role of the stage) and its Zerus
  task through the intake of package 6, as the service; `ToLead` files the
  lead inbox item; `ToPerson` opens an owner card; `Accept` opens the accept
  decision as the pipeline, subject to the same merge rule as the lead's.
- `Operation::Verdict { node, generation, pass, text }` from the member that
  holds the stage node, and from nobody else. `after_verdict` decides the
  next step; a return reopens the target to its implementer with the
  findings through a reopen effect, and counts a round; after the
  pipeline's rounds the node goes to the lead.
- A stage is never given to the member that implemented its target.
- `hgs team verdict` for a member: `{request_id, team, expected_revision,
  node, generation, pass, text}`.

**Tests:** Review focus 4; each built-in pipeline from report to done on
the store alone; a return in each round and the escalation; a verdict by
the wrong member, for a stage of another round, twice; a pipeline whose
stage role has no free member; the person's override of a staged task.

### Package 10: lead inbox, timers, stop

**Owns:** `src/desktop/teams/timers.rs` and its step in the tick; inbox and
card operations it needs in `src/teams/ops/`.

**Deliver:**

- Doorbells: one `hgs send` to the lead saying how many items wait, at most
  one every 120 seconds, bound to the lead's run and conversation; an
  uncertain doorbell is not sent again.
- Timers from the team's `Timers`: a turn that ended without a report after
  120 seconds, a hand-out not started after 180, a reminder for an
  undecided report after 15 minutes, the person after 60, a silent lead
  after 20, each as an inbox item or an owner card, each once.
- Pause and resume: a paused team hands out nothing; a paused member takes
  nothing; a team paused by the person is not resumed by the lead.
- Stop and archive: the keys of the team's open tasks are cleared in
  bounded pages through the service command `team-clear` (this brief first
  called it `clear_team_key`); a launch that completes after a
  stop is recorded as provenance (tracker item `zerus-63rru.11`).

**Tests:** each timer firing once and not after its condition passed; the
120-second coalescing; a doorbell with an unknown result; pause at each
step of a hand-out; stop with tasks in every state.

### Package 13: New team and the Team tab

**Owns:** `client/src/ui/teams/`, `client/src/store/teams*.ts`, the Teams
entries in the i18n catalogs and skins, and the lines that register the
lazy views.

**Deliver:** the screens of the specification's Desktop section for M1: the
New team dialog from a template (roster, pipelines, the settings a person
confirms, Save as template), and the Team tab on the lead's session: team
bar, member cards, the Board with the queue as its own group, the task card
with its stages, Add task. Views that the fork has are imported unchanged in
one commit with the fork's author and then adapted, as the specification's
reuse table says. Data comes only from the `teams` key of the batch and the
Tauri commands of package 12 (`teams_follow`, `teams_hidden`,
`teams_tasks_changed`, `teams_page`, `team_action`, `team_action_again`,
`team_intent_dismiss`); an action has five outcomes including `pending`,
and an uncertain one offers Check and Dismiss. Teams views load lazily, the
entry bundle stays under 500,000 bytes, strings go through the catalogs with
Russian in the supplement, and no value is separated with a dot. With the
preview off or `hgs` unavailable no entry point shows.

**Tests:** render tests for each screen from the pinned fixtures; the five
outcomes; a destructive action without confirmation is impossible; the
bundle bound; `scripts/check-ui-separators.py`.

### Package 14: Waiting for you and Send to team

**Owns:** the Waiting rail entry and its page, the Send to team action on a
task card and the team chooser of the New task form.

**Deliver:** a rail entry with a count from the `waiting` key, listing open
owner cards with their decisions and, from the desktop's own session
attention data, the requests of team sessions that only a person can
answer; a card in Backlog or To do gets Send to team, which runs
`hgs task target REF team TEAM`; the New task form gets a team chooser that
runs `hgs task add --team`. A refusal shows the team's reason.

**Tests:** the count and its page bound; each card decision; a task sent
twice; a refusal; the preview off.

### Package 17: standing confirmations

**Owns:** `src/desktop/teams/confirm.rs` and its step; the admission of the
team's kinds in `src/approval_log.rs`.

**Deliver:** under the team's `confirm_trust` and `resume_exited`
delegations and nothing else: the startup folder trust prompt of a member
whose worktree Zerus created for this team (verified ownership record) is
answered at once; a member whose agent exited by itself is resumed with a
brief, at most three times an hour. Each answer follows the rules of
automatic approvals: exact run, conversation, question and hash, the
journal entry reserved before delivery, `hgs answer` as the original child,
the entry finished from its receipt, and an unknown delivery never
repeated. A prompt of any other kind, a folder Zerus did not create, and a
member a person stopped go to the person.

**Tests:** each refusal; a prompt whose hash changed between the read and
the answer; the journal entry before the delivery; three resumes in an
hour and the fourth refused; the delegation switched off.

## Order

| Wave | Packages | Starts when |
|---|---|---|
| 0 | 1, 11 | now |
| 1 | 2, 3, 4 | package 1 is merged |
| 2 | 5, 6, 7, 12 | package 3 is merged |
| 3 | 8, 9, 10, 13, 14, 17 | packages 2, 5 and 7 are merged |
| 4 | 15, 16 | package 8 is merged |

The seam with the task queue work (`zerus-cg5h.4.8`) is recorded on that
tracker item before package 8 starts: the teams actor calls the task attempt
writer today and the per-executor queue when it exists, and adds no other way
to deliver a task into a session.
