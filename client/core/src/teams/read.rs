//! One `hgs [@host] team <op> --json` read. Every team operation reads one
//! JSON object on standard input, so even a read without arguments sends one
//! (`HgsClient::requestTeam`): an empty input would leave stdin open.
use crate::actions::{Exit, Invocation};
use crate::fleet::valid_alias;
use crate::reads::{ReadError, READ_LIMIT};
use crate::run::{run_limited, TooLarge};
use crate::ComputerId;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// Teams one list reads (`hgs team list` takes 1 to 1000, 200 by default).
pub const LIST_LIMIT: u64 = 200;
/// Events one feed page reads (the store's own page).
pub const FEED_PAGE: u64 = 50;
/// Items one Waiting page reads.
pub const WAITING_PAGE: u64 = 100;
/// Tasks one spans page reads (`hgs team spans` caps it at its own page).
pub const SPANS_PAGE: u64 = 50;
/// Task 92: losses one scoped profile read ranks (Qt's Cache::LossPage).
pub const PROFILE_LOSS_PAGE: u64 = 20;
/// Task 92: test runs the Time view reads for its window (Qt's Cache::TestPage).
pub const TEST_WINDOW_PAGE: u64 = 100;
/// The longest team id a read names.
pub const TEAM_ID_BYTES: usize = 64;

/// One read of the team store.
#[derive(Clone, Debug, PartialEq)]
pub enum TeamRead {
    /// `team list` with `{include_cancelled: true, limit}`: every team the
    /// store keeps, newest first.
    List,
    /// `team status`: live tasks and the bounded window of archived stubs.
    Status { team: String },
    /// One live or archived task, selected by the store before decoding.
    Task { team: String, task: u32 },
    /// `team events` with `{team, limit, before?}`: one page of the team's
    /// journal, newest first; `before` is the previous page's `next_cursor`.
    Events { team: String, before: Option<i64> },
    /// `team inbox` with `{team}`: the lead's inbox.
    Inbox { team: String },
    /// `team waiting` with `{limit, before?}`: what waits for the owner across
    /// every team; `before` is the previous page's `next`.
    Waiting { before: Option<Value> },
    /// `team spans` with `{team, after, limit}`: the time spans of the tasks
    /// after task `after`.
    Spans { team: String, after: u64 },
    /// `team profile` with `{team}`: the team's profile (losses, roles, tokens).
    Profile { team: String },
    /// `team catalog get` with `{}`: the catalog and the designer's draft.
    Catalog,
    /// Task #44: `team catalog route` with `{goal}`: the template a goal would
    /// use (the routing tab's Try it).
    CatalogRoute { goal: String },
    /// Task 41: `team test` with `{team, action:"queue"}`: the test queue.
    TestQueue { team: String },
    /// Task 41: `team test` with `{team, action:"results", limit, before?}`:
    /// one page of test results, newest first.
    TestResults { team: String, before: Option<f64> },
    /// Task 92: `team profile` with `{team, limit, phase?}` for the Time view:
    /// a phase's profile, or the plan's without `phase` (Qt's Cache::load).
    ScopedProfile { team: String, phase: Option<u64> },
    /// Task 92: `team spans` with `{team, limit, phase?, after?}`: one page
    /// of the scope's task timelines (Qt's Cache::spans).
    ScopedSpans {
        team: String,
        phase: Option<u64>,
        after: u64,
    },
    /// Task 92: `team test` with `{team, action:"results", limit, from?,
    /// before?}`: the test runs of the Time view's window, one page.
    TestWindow {
        team: String,
        from: Option<f64>,
        before: Option<f64>,
    },
    /// Task 199: `team builds` with `{team}`: the build broker's folders,
    /// their build folders, what builds and waits, and recent builds.
    Builds { team: String },
}

impl TeamRead {
    /// The operation's word: `hgs team <op> --json`.
    pub fn op(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Status { .. } | Self::Task { .. } => "status",
            Self::Events { .. } => "events",
            Self::Inbox { .. } => "inbox",
            Self::Waiting { .. } => "waiting",
            Self::Spans { .. } | Self::ScopedSpans { .. } => "spans",
            Self::Profile { .. } | Self::ScopedProfile { .. } => "profile",
            Self::Catalog | Self::CatalogRoute { .. } => "catalog",
            Self::TestQueue { .. } | Self::TestResults { .. } | Self::TestWindow { .. } => "test",
            Self::Builds { .. } => "builds",
        }
    }

    /// The team the read is about, if it is about one.
    pub fn team(&self) -> Option<&str> {
        match self {
            Self::Status { team }
            | Self::Task { team, .. }
            | Self::Events { team, .. }
            | Self::Inbox { team }
            | Self::Spans { team, .. }
            | Self::Profile { team } => Some(team),
            Self::TestQueue { team } | Self::TestResults { team, .. } | Self::Builds { team } => Some(team),
            Self::ScopedProfile { team, .. }
            | Self::ScopedSpans { team, .. }
            | Self::TestWindow { team, .. } => Some(team),
            Self::List | Self::Waiting { .. } | Self::Catalog | Self::CatalogRoute { .. } => None,
        }
    }

    /// The JSON object the read sends on standard input.
    pub fn request(&self) -> Value {
        match self {
            Self::List => json!({"include_cancelled": true, "limit": LIST_LIMIT}),
            Self::Status { team } => json!({"team": team, "include_cancelled": true}),
            Self::Task { team, task } => json!({"team": team, "task": task}),
            Self::Events { team, before } => {
                let mut request = json!({"team": team, "limit": FEED_PAGE});
                if let Some(before) = before {
                    request["before"] = json!(before);
                }
                request
            }
            Self::Inbox { team } => json!({"team": team}),
            Self::Waiting { before } => {
                let mut request = json!({"limit": WAITING_PAGE});
                if let Some(before) = before.as_ref().filter(|cursor| cursor.is_object()) {
                    request["before"] = before.clone();
                }
                request
            }
            Self::Spans { team, after } => {
                json!({"team": team, "after": after, "limit": SPANS_PAGE})
            }
            Self::Profile { team } => json!({"team": team}),
            Self::Catalog => json!({}),
            Self::CatalogRoute { goal } => json!({"goal": goal}),
            Self::TestQueue { team } => json!({"team": team, "action": "queue"}),
            Self::Builds { team } => json!({"team": team}),
            Self::TestResults { team, before } => {
                let mut request = json!({"team": team, "action": "results", "limit": 20});
                if let Some(before) = before {
                    request["before"] = json!(before);
                }
                request
            }
            Self::ScopedProfile { team, phase } => {
                let mut request = json!({"team": team, "limit": PROFILE_LOSS_PAGE});
                if let Some(phase) = phase.filter(|phase| *phase > 0) {
                    request["phase"] = json!(phase);
                }
                request
            }
            Self::ScopedSpans { team, phase, after } => {
                let mut request = json!({"team": team, "limit": SPANS_PAGE});
                if let Some(phase) = phase.filter(|phase| *phase > 0) {
                    request["phase"] = json!(phase);
                }
                if *after > 0 {
                    request["after"] = json!(after);
                }
                request
            }
            Self::TestWindow { team, from, before } => {
                let mut request =
                    json!({"team": team, "action": "results", "limit": TEST_WINDOW_PAGE});
                if let Some(from) = from.filter(|at| *at > 0.0) {
                    request["from"] = json!(from);
                }
                if let Some(before) = before.filter(|at| *at > 0.0) {
                    request["before"] = json!(before);
                }
                request
            }
        }
    }

    /// The top-level keys the reply must carry, with whether each is an array.
    fn required(&self) -> &'static [(&'static str, bool)] {
        match self {
            Self::List | Self::Inbox { .. } | Self::Waiting { .. } | Self::Events { .. } => {
                &[("items", true)]
            }
            Self::Status { .. } => &[("id", false), ("members", true), ("tasks", true)],
            Self::Task { .. } => &[],
            Self::Spans { .. } | Self::ScopedSpans { .. } => &[("tasks", true)],
            Self::Profile { .. } | Self::ScopedProfile { .. } => &[],
            Self::Catalog | Self::CatalogRoute { .. } => &[],
            Self::TestQueue { .. } | Self::TestResults { .. } | Self::TestWindow { .. } => {
                &[("items", true)]
            }
            Self::Builds { .. } => &[("folders", true)],
        }
    }
}

/// A team id as a read sends it: a UUID in practice; nonempty, short, no
/// option and no control or space characters.
pub fn valid_team(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= TEAM_ID_BYTES
        && !id.starts_with('-')
        && !id.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn bad(message: &str) -> ReadError {
    ReadError::BadRequest {
        message: message.into(),
    }
}

fn invalid(message: &str) -> ReadError {
    ReadError::Invalid {
        message: message.into(),
    }
}

/// The command of a read on `computer`: `hgs [@alias] team <op> --json` with
/// the request on standard input, 15 s here and 45 s on a peer.
pub fn invocation(computer: &ComputerId, read: &TeamRead) -> Result<Invocation, ReadError> {
    if computer.alias().is_some_and(|alias| !valid_alias(alias)) {
        return Err(bad("not a computer alias"));
    }
    if read.team().is_some_and(|team| !valid_team(team)) {
        return Err(bad("not a team id"));
    }
    if matches!(read, TeamRead::Task { task: 0, .. }) {
        return Err(bad("not a positive task id"));
    }
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    if let TeamRead::CatalogRoute { goal } = read {
        if goal.chars().count() > 4000 || goal.chars().any(|ch| ch.is_control() && ch != '\n') {
            return Err(bad("not a goal"));
        }
    }
    if matches!(read, &TeamRead::Catalog) {
        args.extend(["team".to_owned(), "catalog".to_owned(), "get".to_owned(), "--json".to_owned()]);
    } else if matches!(read, &TeamRead::CatalogRoute { .. }) {
        args.extend(["team".to_owned(), "catalog".to_owned(), "route".to_owned(), "--json".to_owned()]);
    } else if matches!(
        read,
        &TeamRead::TestQueue { .. } | &TeamRead::TestResults { .. } | &TeamRead::TestWindow { .. }
    ) {
        args.extend(["team".to_owned(), "test".to_owned(), "--json".to_owned()]);
    } else {
        args.extend(["team".to_owned(), read.op().to_owned(), "--json".to_owned()]);
    }
    Ok(Invocation {
        args,
        stdin: Some(read.request().to_string().into_bytes()),
        timeout: Duration::from_secs(if computer.is_local() { 15 } else { 45 }),
        request_id: None,
    })
}

/// Checks a reply: its exit, then the shape the read relies on.
pub fn decode(read: &TeamRead, exit: Exit) -> Result<Value, ReadError> {
    let stdout = crate::reads::output(exit)?;
    let value: Value =
        serde_json::from_slice(&stdout).map_err(|error| invalid(&error.to_string()))?;
    let Value::Object(object) = &value else {
        return Err(invalid("not a JSON object"));
    };
    if let TeamRead::Task { task, .. } = read {
        if object.get("id").and_then(Value::as_u64) != Some(u64::from(*task)) {
            return Err(invalid("team status gives a different task"));
        }
    }
    for (key, array) in read.required() {
        let fits = match object.get(*key) {
            Some(found) if *array => found.is_array(),
            Some(found) => found.is_string(),
            None => false,
        };
        if !fits {
            return Err(invalid(&format!("team {} gives no {key}", read.op())));
        }
    }
    if matches!(read, &TeamRead::Catalog)
        && !object.get("catalog").is_some_and(Value::is_object)
    {
        return Err(invalid("team catalog gives no catalog"));
    }
    Ok(value)
}

/// Runs one read through `hgs` and checks its reply. Blocks for at most the
/// read's timeout plus a few seconds; callers run it off the UI thread.
pub fn read(hgs: &Path, computer: &ComputerId, read: &TeamRead) -> Result<Value, ReadError> {
    let invocation = invocation(computer, read)?;
    let exit = run_limited(hgs, &invocation, &[], READ_LIMIT)
        .map_err(|TooLarge { cap, .. }| ReadError::TooLarge { limit: cap })?;
    decode(read, exit)
}
