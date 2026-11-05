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
/// The longest team id a read names.
pub const TEAM_ID_BYTES: usize = 64;

/// One read of the team store.
#[derive(Clone, Debug, PartialEq)]
pub enum TeamRead {
    /// `team list` with `{include_cancelled: true, limit}`: every team the
    /// store keeps, newest first.
    List,
    /// `team status` with `{team, include_cancelled: true}`: one team whole.
    Status { team: String },
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
    /// `team test` with `{team, action:"queue"}`: the test queue.
    TestQueue { team: String },
    /// `team test` with `{team, action:"results", limit, before?}`: one page
    /// of test results, newest first.
    TestResults { team: String, before: Option<f64> },
}

impl TeamRead {
    /// The operation's word: `hgs team <op> --json`.
    pub fn op(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Status { .. } => "status",
            Self::Events { .. } => "events",
            Self::Inbox { .. } => "inbox",
            Self::Waiting { .. } => "waiting",
            Self::Spans { .. } => "spans",
            Self::Profile { .. } => "profile",
            Self::Catalog => "catalog",
            Self::TestQueue { .. } | Self::TestResults { .. } => "test",
        }
    }

    /// The team the read is about, if it is about one.
    pub fn team(&self) -> Option<&str> {
        match self {
            Self::Status { team }
            | Self::Events { team, .. }
            | Self::Inbox { team }
            | Self::Spans { team, .. }
            | Self::Profile { team } => Some(team),
            Self::TestQueue { team } | Self::TestResults { team, .. } => Some(team),
            Self::List | Self::Waiting { .. } | Self::Catalog => None,
        }
    }

    /// The JSON object the read sends on standard input.
    pub fn request(&self) -> Value {
        match self {
            Self::List => json!({"include_cancelled": true, "limit": LIST_LIMIT}),
            Self::Status { team } => json!({"team": team, "include_cancelled": true}),
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
            Self::TestQueue { team } => json!({"team": team, "action": "queue"}),
            Self::TestResults { team, before } => {
                let mut request = json!({"team": team, "action": "results", "limit": 20});
                if let Some(before) = before {
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
            Self::Spans { .. } => &[("tasks", true)],
            Self::Profile { .. } => &[],
            Self::Catalog => &[],
            Self::TestQueue { .. } | Self::TestResults { .. } => &[("items", true)],
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
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    if matches!(read, &TeamRead::Catalog) {
        args.extend(["team".to_owned(), "catalog".to_owned(), "get".to_owned(), "--json".to_owned()]);
    } else if matches!(read, &TeamRead::TestQueue { .. }) || matches!(read, &TeamRead::TestResults { .. }) {
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
