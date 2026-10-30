//! Reads behind the Overview, Projects, Machines, Accounts and Settings
//! screens: `hgs [@host] <command>` with the Qt client's arguments and
//! timeouts, run through [`run_limited`] so the output is bounded as well,
//! and checked for the shape each screen relies on.
//!
//! [`read`] blocks; callers run it off the UI thread. Overview needs no read
//! of its own: it takes the stream's catalog and `catalog.meta` plus
//! [`Read::Accounts`]. Machines, the shared project catalog and the version
//! are this computer's; the other reads name a computer and go to a peer as
//! `hgs @alias ...`. A reply is the command's JSON as it prints it, so the
//! view keeps every field; the core checks only the top-level keys below.
use crate::actions::{Exit, Invocation};
use crate::fleet::valid_alias;
use crate::run::{run_cancellable, run_limited, Stopped, TooLarge};
use crate::ComputerId;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// The largest reply of an ordinary read (`swarm get` is about 120 KB).
pub const READ_LIMIT: usize = 8 << 20;
/// The largest analytics reply, as the Qt client takes it.
pub const ANALYTICS_LIMIT: usize = 1 << 20;
/// The largest help text an analytics probe reads.
const HELP_LIMIT: usize = 128 << 10;
/// `hgs --version` prints one short line.
const VERSION_LIMIT: usize = 128;
/// The help text of a CLI that has the analytics read.
pub const ANALYTICS_MARKER: &str = "hgs [@host] analytics --json";
/// How much of a failed command's standard error an error keeps.
const MESSAGE_CHARS: usize = 2000;
/// A call whose standard input `hgs` did not take.
pub(crate) const NOT_TAKEN: &str = "hgs did not take the request";

/// One read.
#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    /// `hgs [@host] project ls --json`: the computer's projects, an array.
    Projects,
    /// `hgs swarm get`: the shared project catalog, with `machines` and
    /// `digest`. This computer only.
    Swarm,
    /// `hgs machine ls`: the machine settings, with `machines` and
    /// `revision`. This computer only.
    Machines,
    /// `hgs machine resolve ALIAS`: the connection an alias resolves to.
    MachineResolve { alias: String },
    /// `hgs machine check ALIAS`: SSH and the peer's hgs (up to 25 s).
    MachineCheck { alias: String },
    /// `hgs [@host] account ls`: the account catalog, with `profiles` and
    /// `revision`.
    Accounts,
    /// `hgs [@host] account inspect ID --refresh`: one account's usage.
    AccountUsage { id: String },
    /// `hgs [@host] account inspect --session NAME --refresh`: the usage of
    /// the account a session is bound to.
    SessionAccountUsage { name: String },
    /// `hgs [@host] recovery get`.
    Recovery,
    /// `hgs --version`: the version as a JSON string.
    Version,
    /// `hgs [@host] analytics --json` with `{schema: 1, action: "summary",
    /// from, through}` (UTC dates, `YYYY-MM-DD`, inclusive). A computer whose
    /// help lacks [`ANALYTICS_MARKER`] is [`ReadError::Unsupported`]: an
    /// older hgs would take the unknown verb as a session launch.
    Analytics { from: String, through: String },
    /// `hgs [@host] processes NAME --output ID --run RUN --conversation C
    /// [--archive A] [--generation G]` (45 s): one process's retained
    /// output, `{id, run_id, conversation_id, output, truncated, status}`.
    ProcessOutput {
        name: String,
        process_id: String,
        run_id: String,
        conversation_id: String,
        archive_id: Option<String>,
        generation: Option<String>,
    },
    /// `hgs [@host] archives --json` with `{from, until, limit: 50, cursor,
    /// query}` (epoch seconds; `cursor` is the keyset anchor, empty for the
    /// first page). The reply is checked as the Qt client checks it
    /// ([`check_archive_page`]); [`archive_page`] gives it typed.
    ArchivePage {
        from: f64,
        until: f64,
        cursor: String,
        query: String,
    },
    /// `hgs [@host] inspect NAME --after AFTER --skip-processes --archive ID`
    /// (5 s, 15 s on a peer): the recorded Activity of one archived session
    /// (`HgsClient::requestInspection`). [`archived_activity`] gives it typed.
    ArchivedActivity {
        name: String,
        archive_id: String,
        after: i64,
    },
    /// `hgs [@host] inspect NAME --after 0 --archive ID` (5 s, 15 s on a peer):
    /// an archived session's recorded process inventory, with processes included.
    ArchivedProcesses { name: String, archive_id: String },
}

/// Qt's `requestInspection` text for a reply that is not a JSON object.
pub const ACTIVITY_INVALID: &str = "Invalid activity response";

/// The rows an archive page asks for (`ArchiveBrowser`).
pub const ARCHIVE_PAGE_ROWS: usize = 50;
/// The longest cursor either side of an archive page may carry.
const ARCHIVE_CURSOR_CHARS: usize = 1024;
/// Qt's three archive page errors (`HgsClient::requestArchivePage`).
pub const ARCHIVE_PAGE_INVALID: &str = "Invalid archive page. Update the computer CLI and refresh.";
pub const ARCHIVE_WINDOW_INVALID: &str = "Invalid archive window. Refresh the archive.";
pub const ARCHIVE_IDENTITY_INVALID: &str =
    "Invalid archived session identity. Refresh the archive.";

impl Read {
    /// Whether the read goes to the named computer; the others are this
    /// computer's.
    pub fn per_computer(&self) -> bool {
        matches!(
            self,
            Self::Projects
                | Self::Accounts
                | Self::AccountUsage { .. }
                | Self::SessionAccountUsage { .. }
                | Self::Recovery
                | Self::Analytics { .. }
                | Self::ProcessOutput { .. }
                | Self::ArchivePage { .. }
                | Self::ArchivedActivity { .. }
                | Self::ArchivedProcesses { .. }
        )
    }

    /// The most standard output the read takes.
    pub fn limit(&self) -> usize {
        match self {
            Self::Version => VERSION_LIMIT,
            Self::Analytics { .. } => ANALYTICS_LIMIT,
            _ => READ_LIMIT,
        }
    }
}

/// Why a read gave no value. Serialized as `{"error": kind, ...fields}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum ReadError {
    /// Not a read this computer can ask for (a peer for a this-computer
    /// read, an invalid alias, account ID or date).
    BadRequest {
        message: String,
    },
    /// `hgs` did not start.
    StartFailed {
        message: String,
    },
    TimedOut,
    /// The reply passed the read's limit; the command was ended.
    TooLarge {
        limit: usize,
    },
    /// `hgs` failed; `message` is its standard error, at most 2000 characters.
    Failed {
        code: Option<i32>,
        message: String,
    },
    /// The reply is not the JSON the read expects.
    Invalid {
        message: String,
    },
    /// The computer's hgs does not have this read.
    Unsupported,
    /// An archive page failed Qt's checks (or did not parse), or an archived
    /// Activity reply was not an object; `message` is Qt's text, to show
    /// unchanged.
    ArchiveInvalid {
        message: String,
    },
    /// A newer archive page request for the computer took over; the child
    /// was ended.
    Superseded,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest { message } | Self::StartFailed { message } => f.write_str(message),
            Self::TimedOut => f.write_str("hgs did not answer in time"),
            Self::TooLarge { limit } => write!(f, "the reply passed {limit} bytes"),
            Self::Failed { message, .. } => f.write_str(message),
            Self::Invalid { message } => write!(f, "invalid reply: {message}"),
            Self::Unsupported => f.write_str("this computer's hgs does not have this read"),
            Self::ArchiveInvalid { message } => f.write_str(message),
            Self::Superseded => f.write_str("superseded"),
        }
    }
}

impl std::error::Error for ReadError {}

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

/// A UTC date as `YYYY-MM-DD`.
fn date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// An account ID as a command-line value: no option, no control characters.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.starts_with('-') && !id.chars().any(char::is_control)
}

fn arguments(computer: &ComputerId, words: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(words.iter().map(|word| (*word).to_owned()));
    args
}

fn timeout(local: u64, peer: u64, computer: &ComputerId) -> Duration {
    Duration::from_secs(if computer.is_local() { local } else { peer })
}

/// The command of a read on `computer`.
pub fn invocation(computer: &ComputerId, read: &Read) -> Result<Invocation, ReadError> {
    if !read.per_computer() && !computer.is_local() {
        return Err(bad("this read is this computer's only"));
    }
    if computer.alias().is_some_and(|alias| !valid_alias(alias)) {
        return Err(bad("not a computer alias"));
    }
    let (args, stdin, timeout) = match read {
        Read::Projects => (
            arguments(computer, &["project", "ls", "--json"]),
            None,
            timeout(5, 15, computer),
        ),
        Read::Swarm => (
            arguments(computer, &["swarm", "get"]),
            None,
            timeout(10, 10, computer),
        ),
        Read::Machines => (
            arguments(computer, &["machine", "ls"]),
            None,
            timeout(8, 8, computer),
        ),
        Read::MachineResolve { alias } | Read::MachineCheck { alias } => {
            if !valid_alias(alias) {
                return Err(bad("not a computer alias"));
            }
            let (verb, seconds) = match read {
                Read::MachineCheck { .. } => ("check", 25),
                _ => ("resolve", 8),
            };
            (
                arguments(computer, &["machine", verb, alias]),
                None,
                timeout(seconds, seconds, computer),
            )
        }
        Read::Accounts => (
            arguments(computer, &["account", "ls"]),
            None,
            timeout(25, 25, computer),
        ),
        Read::AccountUsage { id } => {
            if !valid_id(id) {
                return Err(bad("not an account ID"));
            }
            (
                arguments(computer, &["account", "inspect", id, "--refresh"]),
                None,
                timeout(25, 25, computer),
            )
        }
        Read::SessionAccountUsage { name } => {
            if !valid_id(name) {
                return Err(bad("not a session name"));
            }
            (
                arguments(
                    computer,
                    &["account", "inspect", "--session", name, "--refresh"],
                ),
                None,
                timeout(25, 25, computer),
            )
        }
        Read::Recovery => (
            arguments(computer, &["recovery", "get"]),
            None,
            timeout(10, 25, computer),
        ),
        Read::Version => (
            arguments(computer, &["--version"]),
            None,
            timeout(3, 3, computer),
        ),
        Read::ProcessOutput {
            name,
            process_id,
            run_id,
            conversation_id,
            archive_id,
            generation,
        } => {
            if [name, process_id, run_id, conversation_id]
                .iter()
                .any(|value| value.is_empty() || value.chars().any(char::is_control))
            {
                return Err(bad(
                    "process output needs its session, process, run and conversation",
                ));
            }
            let mut words = vec![
                "processes",
                name.as_str(),
                "--output",
                process_id.as_str(),
                "--run",
                run_id.as_str(),
                "--conversation",
                conversation_id.as_str(),
            ];
            if let Some(archive) = archive_id.as_deref().filter(|id| !id.is_empty()) {
                words.extend(["--archive", archive]);
            }
            if let Some(generation) = generation.as_deref().filter(|id| !id.is_empty()) {
                words.extend(["--generation", generation]);
            }
            (arguments(computer, &words), None, timeout(45, 45, computer))
        }
        Read::Analytics { from, through } => {
            if !date(from) || !date(through) || from > through {
                return Err(bad(
                    "an analytics range is two UTC dates, the first not after the second",
                ));
            }
            let request =
                json!({"schema": 1, "action": "summary", "from": from, "through": through});
            (
                arguments(computer, &["analytics", "--json"]),
                Some(request.to_string().into_bytes()),
                timeout(30, 30, computer),
            )
        }
        Read::ArchivePage {
            from,
            until,
            cursor,
            query,
        } => {
            if !from.is_finite() || !until.is_finite() {
                return Err(bad("an archive window is two finite times"));
            }
            let request = json!({
                "from": seconds(*from), "until": seconds(*until),
                "limit": ARCHIVE_PAGE_ROWS, "cursor": cursor, "query": query,
            });
            (
                arguments(computer, &["archives", "--json"]),
                Some(request.to_string().into_bytes()),
                timeout(30, 30, computer),
            )
        }
        Read::ArchivedActivity {
            name,
            archive_id,
            after,
        } => {
            if name.is_empty() || archive_id.is_empty() {
                return Err(bad(ARCHIVE_IDENTITY_INVALID));
            }
            let after = after.to_string();
            (
                arguments(
                    computer,
                    &[
                        "inspect",
                        name,
                        "--after",
                        &after,
                        "--skip-processes",
                        "--archive",
                        archive_id,
                    ],
                ),
                None,
                timeout(5, 15, computer),
            )
        }
        Read::ArchivedProcesses { name, archive_id } => {
            if name.is_empty() || archive_id.is_empty() {
                return Err(bad(ARCHIVE_IDENTITY_INVALID));
            }
            (
                arguments(
                    computer,
                    &["inspect", name, "--after", "0", "--archive", archive_id],
                ),
                None,
                timeout(5, 15, computer),
            )
        }
    };
    Ok(Invocation {
        args,
        stdin,
        timeout,
        request_id: None,
    })
}

/// A time as a JSON number, an integer when it is whole (as Qt writes it).
fn seconds(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

/// The failure of an exit that gave no reply, or its standard output.
fn output(exit: Exit) -> Result<Vec<u8>, ReadError> {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => Ok(stdout),
        Exit::Code { code, stderr, .. } => Err(failed(Some(code), &stderr)),
        Exit::Abnormal { stderr } => Err(failed(None, &stderr)),
        Exit::TimedOut => Err(ReadError::TimedOut),
        Exit::StartFailed(message) => Err(ReadError::StartFailed { message }),
        Exit::WriteFailed => Err(failed(None, NOT_TAKEN)),
    }
}

/// The text of a call `hgs` failed: its standard error, trimmed and at most
/// `limit` characters, else its exit code, else that it ended abnormally.
pub(crate) fn failure_text(code: Option<i32>, stderr: &str, limit: usize) -> String {
    let text: String = stderr.trim().chars().take(limit).collect();
    if !text.is_empty() {
        return text;
    }
    match code {
        Some(code) => format!("hgs exited with code {code}"),
        None => "hgs ended abnormally".to_owned(),
    }
}

fn failed(code: Option<i32>, stderr: &str) -> ReadError {
    ReadError::Failed {
        code,
        message: failure_text(code, stderr, MESSAGE_CHARS),
    }
}

/// The version `hgs --version` prints, `hgs X.Y.Z` with an optional
/// pre-release and build.
fn version(text: &str) -> Option<&str> {
    let version = text.trim().strip_prefix("hgs ")?;
    let core = version.split(['-', '+']).next()?;
    let numbers: Vec<&str> = core.split('.').collect();
    let plain = numbers.len() == 3
        && numbers
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    let rest = version.get(core.len()..)?;
    let tail = rest
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-+.".contains(&b));
    (plain && tail && (rest.is_empty() || rest.len() > 1)).then_some(version)
}

/// The keys each object reply must have, with the array ones marked.
fn required(read: &Read) -> &'static [(&'static str, bool)] {
    match read {
        Read::Swarm => &[("machines", true), ("digest", false)],
        Read::Machines => &[("machines", true), ("revision", false)],
        Read::Accounts => &[("profiles", true), ("revision", false)],
        Read::Analytics { .. } => &[("schema", false)],
        _ => &[],
    }
}

/// Checks a reply: its exit, then the JSON shape the read expects.
pub fn decode(read: &Read, exit: Exit) -> Result<Value, ReadError> {
    let stdout = output(exit)?;
    if *read == Read::Version {
        let text = std::str::from_utf8(&stdout).map_err(|_| invalid("not text"))?;
        return version(text)
            .map(|version| Value::String(version.to_owned()))
            .ok_or_else(|| invalid("not an hgs version"));
    }
    // One parse; the archive reads word a reply they cannot use as Qt does.
    let value = match (read, serde_json::from_slice::<Value>(&stdout)) {
        (Read::ArchivedActivity { .. }, Ok(value @ Value::Object(_))) => value,
        (Read::ArchivedActivity { .. }, _) => return Err(archive_invalid(ACTIVITY_INVALID)),
        (Read::ArchivePage { .. }, Err(_)) => return Err(archive_invalid(ARCHIVE_PAGE_INVALID)),
        (_, Ok(value)) => value,
        (_, Err(error)) => return Err(invalid(&error.to_string())),
    };
    if let Read::ArchivePage {
        from,
        until,
        cursor,
        query,
    } = read
    {
        check_archive_page(*from, *until, cursor, query, &value).map_err(archive_invalid)?;
        return Ok(value);
    }
    if *read == Read::Projects {
        return match value {
            Value::Array(_) => Ok(value),
            _ => Err(invalid("project ls gives an array")),
        };
    }
    let Value::Object(object) = &value else {
        return Err(invalid(if matches!(read, Read::ProcessOutput { .. }) {
            "Invalid process response"
        } else {
            "not a JSON object"
        }));
    };
    // Some commands report a refusal as {"error": "..."} on success.
    if let Some(error) = object.get("error").and_then(Value::as_str) {
        if required(read)
            .iter()
            .any(|(key, _)| !object.contains_key(*key))
        {
            return Err(failed(Some(0), error));
        }
    }
    for (key, array) in required(read) {
        match object.get(*key) {
            Some(found) if !array || found.is_array() => {}
            _ => return Err(invalid(&format!("no {key}"))),
        }
    }
    if matches!(read, Read::Analytics { .. }) && object.get("schema") != Some(&json!(1)) {
        return Err(invalid("unknown analytics schema"));
    }
    Ok(value)
}

/// An archive UUID as Qt keeps it: `QUuid(id).toString(WithoutBraces) == id`,
/// that is 8-4-4-4-12 lowercase hexadecimal digits.
fn canonical_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

fn archive_invalid(text: &'static str) -> ReadError {
    ReadError::ArchiveInvalid {
        message: text.into(),
    }
}

/// Qt's `requestArchivePage` validation of a reply to the query
/// (`from`, `until`, `cursor`, `query`); the Err is one of its three texts,
/// the last failing check winning as in Qt (identity over window over page).
pub fn check_archive_page(
    from: f64,
    until: f64,
    cursor: &str,
    query: &str,
    data: &Value,
) -> Result<(), &'static str> {
    let rows: &[Value] = data
        .get("sessions")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let next = data.get("next_cursor").and_then(Value::as_str);
    let total = data.get("total").and_then(Value::as_f64);
    let mut error = None;
    let page_ok = data.is_object()
        && data.get("version").and_then(Value::as_f64) == Some(1.0)
        && data.get("sessions").is_some_and(Value::is_array)
        && rows.len() <= ARCHIVE_PAGE_ROWS
        && next.is_some_and(|next| {
            next.chars().count() <= ARCHIVE_CURSOR_CHARS
                && !(rows.is_empty() && !next.is_empty())
                && (cursor.is_empty() || next != cursor)
        })
        && total.is_some_and(|total| total >= rows.len() as f64)
        && data.get("from").and_then(Value::as_f64) == Some(from)
        && data.get("until").and_then(Value::as_f64) == Some(until)
        && data.get("query").and_then(Value::as_str) == Some(query);
    if !page_ok {
        error = Some(ARCHIVE_PAGE_INVALID);
    }
    if data.get("previous_cursor").is_some() || data.get("position").is_some() {
        let previous = data.get("previous_cursor").and_then(Value::as_str);
        let position = data.get("position").and_then(Value::as_f64);
        let window = previous
            .is_some_and(|previous| previous.chars().count() <= ARCHIVE_CURSOR_CHARS)
            && position.is_some_and(|position| {
                position >= 0.0
                    && position == position.floor()
                    && position + rows.len() as f64 <= total.unwrap_or(0.0)
            });
        if !window {
            error = Some(ARCHIVE_WINDOW_INVALID);
        }
    }
    for row in rows {
        let at = row.get("archived_at").and_then(Value::as_f64);
        let named = row
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| !name.is_empty());
        let identified = row
            .get("archive_id")
            .and_then(Value::as_str)
            .is_some_and(canonical_uuid);
        if row.get("state").and_then(Value::as_str) != Some("archived")
            || !named
            || !identified
            || !at.is_some_and(|at| at >= from && at < until)
        {
            error = Some(ARCHIVE_IDENTITY_INVALID);
        }
    }
    error.map_or(Ok(()), Err)
}

/// One checked archive page.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArchivePage {
    pub rows: Vec<crate::view::ViewRow>,
    pub next_cursor: String,
    pub previous_cursor: Option<String>,
    pub total: u64,
    pub position: Option<u64>,
}

/// Reads and checks one archive page of `computer` (30 s, at most
/// [`READ_LIMIT`]). A reply that fails Qt's checks is
/// [`ReadError::Invalid`] whose message is Qt's text, with no rows.
pub fn archive_page(
    hgs: &Path,
    computer: &ComputerId,
    from: f64,
    until: f64,
    cursor: &str,
    query: &str,
) -> Result<ArchivePage, ReadError> {
    archive_page_cancellable(
        hgs,
        computer,
        from,
        until,
        cursor,
        query,
        &AtomicBool::new(false),
    )
}

/// [`archive_page`] that ends its child and gives [`ReadError::Superseded`]
/// once `cancel` is set (Qt's `cancelArchivePages`).
pub fn archive_page_cancellable(
    hgs: &Path,
    computer: &ComputerId,
    from: f64,
    until: f64,
    cursor: &str,
    query: &str,
    cancel: &AtomicBool,
) -> Result<ArchivePage, ReadError> {
    let request = Read::ArchivePage {
        from,
        until,
        cursor: cursor.to_owned(),
        query: query.to_owned(),
    };
    let invocation = invocation(computer, &request)?;
    let exit =
        run_cancellable(hgs, &invocation, &[], request.limit(), cancel).map_err(|stopped| {
            match stopped {
                Stopped::TooLarge(TooLarge { cap, .. }) => ReadError::TooLarge { limit: cap },
                Stopped::Cancelled => ReadError::Superseded,
            }
        })?;
    if let Some(error) = archive_failure(hgs, &exit) {
        return Err(error);
    }
    Ok(page(&decode(&request, exit)?))
}

/// Standard error an archive page failure shows, at most (Qt's `left(4000)`).
const ARCHIVE_FAILURE_CHARS: usize = 4000;

/// Qt's `failureDetail` (HgsClient.cpp 466-479) for an archive page or an
/// archived Activity whose `hgs` did not end with exit 0: "timed out", "cannot start '{hgs}':
/// {error}", or the trimmed standard error, else "exit code {n}". Only the
/// archive read words its failures so; the other reads keep theirs.
fn archive_failure(hgs: &Path, exit: &Exit) -> Option<ReadError> {
    let detail = |stderr: &str| -> Option<String> {
        let text: String = stderr.trim().chars().take(ARCHIVE_FAILURE_CHARS).collect();
        (!text.is_empty()).then_some(text)
    };
    match exit {
        Exit::Code { code: 0, .. } => None,
        Exit::TimedOut => Some(ReadError::Failed {
            code: None,
            message: "timed out".into(),
        }),
        Exit::StartFailed(error) => Some(ReadError::StartFailed {
            message: format!("cannot start '{}': {error}", hgs.display()),
        }),
        Exit::Code { code, stderr, .. } => Some(ReadError::Failed {
            code: Some(*code),
            message: detail(stderr).unwrap_or_else(|| format!("exit code {code}")),
        }),
        Exit::Abnormal { stderr } => detail(stderr).map(|message| ReadError::Failed {
            code: None,
            message,
        }),
        Exit::WriteFailed => None,
    }
}

/// The typed page of a reply [`check_archive_page`] accepted.
fn page(data: &Value) -> ArchivePage {
    let text = |key: &str| data.get(key).and_then(Value::as_str).map(str::to_owned);
    ArchivePage {
        rows: data["sessions"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(crate::view::row)
            .collect(),
        next_cursor: text("next_cursor").unwrap_or_default(),
        previous_cursor: text("previous_cursor"),
        total: data["total"].as_f64().unwrap_or(0.0) as u64,
        position: data
            .get("position")
            .and_then(Value::as_f64)
            .map(|p| p as u64),
    }
}

/// The recorded Activity of one archived session: one `inspect` reply.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArchivedActivity {
    /// The reply's `events`, unchanged.
    pub events: Vec<Value>,
    pub replace_events: bool,
    pub conversation_id: String,
    pub run_id: String,
    pub cleared_conversations: Vec<String>,
    /// The journal cursor the next read asks `--after` (Qt's `data.cursor`).
    pub cursor: i64,
    /// The other top-level fields, what the transcript reads of a `state`.
    pub summary: Value,
}

/// Reads the Activity window of an archived session after `after` (5 s here,
/// 15 s on a peer, at most [`READ_LIMIT`]). Failures are worded as Qt's
/// `failureDetail`; a reply that is not an object is "Invalid activity
/// response". An empty name or archive id is refused before any call.
pub fn archived_activity(
    hgs: &Path,
    computer: &ComputerId,
    name: &str,
    archive_id: &str,
    after: i64,
) -> Result<ArchivedActivity, ReadError> {
    let request = Read::ArchivedActivity {
        name: name.to_owned(),
        archive_id: archive_id.to_owned(),
        after,
    };
    let invocation = invocation(computer, &request)?;
    let exit = limited(hgs, &invocation, request.limit())?;
    if let Some(error) = archive_failure(hgs, &exit) {
        return Err(error);
    }
    let Value::Object(mut object) = decode(&request, exit)? else {
        return Err(archive_invalid(ACTIVITY_INVALID));
    };
    let text = |object: &mut serde_json::Map<String, Value>, key: &str| match object.remove(key) {
        Some(Value::String(text)) => text,
        _ => String::new(),
    };
    let events = match object.remove("events") {
        Some(Value::Array(events)) => events,
        _ => Vec::new(),
    };
    let replace_events = object.remove("replace_events") == Some(Value::Bool(true));
    let cleared_conversations = match object.remove("cleared_conversations") {
        Some(Value::Array(ids)) => ids
            .into_iter()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    let cursor = object
        .remove("cursor")
        .and_then(|cursor| {
            cursor
                .as_i64()
                .or_else(|| cursor.as_f64().map(|n| n as i64))
        })
        .unwrap_or(0);
    Ok(ArchivedActivity {
        events,
        replace_events,
        conversation_id: text(&mut object, "conversation_id"),
        run_id: text(&mut object, "run_id"),
        cleared_conversations,
        cursor,
        summary: Value::Object(object),
    })
}

fn limited(hgs: &Path, invocation: &Invocation, limit: usize) -> Result<Exit, ReadError> {
    run_limited(hgs, invocation, &[], limit)
        .map_err(|TooLarge { cap, .. }| ReadError::TooLarge { limit: cap })
}

/// Runs one read on `computer` through `hgs` and checks its reply. Blocks for
/// at most the read's timeout plus a few seconds; an analytics read first
/// reads the computer's help for [`ANALYTICS_MARKER`].
pub fn read(hgs: &Path, computer: &ComputerId, read: &Read) -> Result<Value, ReadError> {
    let invocation = invocation(computer, read)?;
    if matches!(read, Read::Analytics { .. }) {
        let probe = Invocation {
            args: arguments(computer, &["--help"]),
            stdin: None,
            timeout: Duration::from_secs(30),
            request_id: None,
        };
        let help = output(limited(hgs, &probe, HELP_LIMIT)?)?;
        if !String::from_utf8_lossy(&help).contains(ANALYTICS_MARKER) {
            return Err(ReadError::Unsupported);
        }
    }
    decode(read, limited(hgs, &invocation, read.limit())?)
}

/// The largest attachment reply: 10 MiB as base64 plus its metadata.
pub const ATTACHMENT_LIMIT: usize = crate::attachments::MAX_ATTACHMENT_BYTES * 4 / 3 + 8192;

/// One attachment of an Activity message, as its message names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct AttachmentRef {
    /// The session's name.
    pub name: String,
    /// The message's `request_id`.
    pub request_id: String,
    /// The file's `index` in the message, -1 when it has none.
    pub index: i64,
    pub conversation_id: String,
    #[serde(default)]
    pub archive_id: Option<String>,
    /// A subagent's ID for a subagent's message.
    #[serde(default)]
    pub agent_id: Option<String>,
}

/// An attachment read back: its metadata without `data_base64`, and the
/// bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentRead {
    pub metadata: Value,
    pub bytes: Vec<u8>,
}

/// An attachment kept in the private cache for viewing or opening.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CachedAttachment {
    pub metadata: Value,
    pub local_path: std::path::PathBuf,
}

/// `hgs [@host] attachment NAME --request ID --index N --conversation C
/// [--archive A] [--agent G]`, 10 s here and 45 s on a peer
/// (`HgsClient::requestAttachment`).
pub fn attachment_invocation(
    computer: &ComputerId,
    file: &AttachmentRef,
) -> Result<Invocation, ReadError> {
    if computer.alias().is_some_and(|alias| !valid_alias(alias)) {
        return Err(bad("not a computer alias"));
    }
    if [&file.name, &file.request_id, &file.conversation_id]
        .iter()
        .any(|value| value.is_empty() || value.chars().any(char::is_control))
    {
        return Err(bad(
            "an attachment needs its session, message and conversation",
        ));
    }
    let index = file.index.to_string();
    let mut words = vec![
        "attachment",
        file.name.as_str(),
        "--request",
        file.request_id.as_str(),
        "--index",
        index.as_str(),
        "--conversation",
        file.conversation_id.as_str(),
    ];
    if let Some(archive) = file.archive_id.as_deref().filter(|id| !id.is_empty()) {
        words.extend(["--archive", archive]);
    }
    if let Some(agent) = file.agent_id.as_deref().filter(|id| !id.is_empty()) {
        words.extend(["--agent", agent]);
    }
    Ok(Invocation {
        args: arguments(computer, &words),
        stdin: None,
        timeout: timeout(10, 45, computer),
        request_id: None,
    })
}

/// Checks an attachment reply: the same message and index, and nonempty
/// strictly decoded bytes of at most 10 MiB.
pub fn decode_attachment(file: &AttachmentRef, exit: Exit) -> Result<AttachmentRead, ReadError> {
    use base64::Engine;
    let stdout = output(exit)?;
    if stdout.len() > ATTACHMENT_LIMIT {
        return Err(ReadError::TooLarge {
            limit: ATTACHMENT_LIMIT,
        });
    }
    let invalid_reply = || invalid("Invalid attachment response. Try again.");
    let Ok(Value::Object(mut object)) = serde_json::from_slice::<Value>(&stdout) else {
        return Err(invalid_reply());
    };
    let encoded = object.remove("data_base64");
    let bytes = encoded
        .as_ref()
        .and_then(Value::as_str)
        .and_then(|text| base64::engine::general_purpose::STANDARD.decode(text).ok())
        .ok_or_else(invalid_reply)?;
    if object.get("request_id").and_then(Value::as_str) != Some(file.request_id.as_str())
        || object.get("index").and_then(Value::as_f64) != Some(file.index as f64)
        || bytes.is_empty()
        || bytes.len() > crate::attachments::MAX_ATTACHMENT_BYTES
    {
        return Err(invalid_reply());
    }
    Ok(AttachmentRead {
        metadata: Value::Object(object),
        bytes,
    })
}

/// Reads one attachment of an Activity message. Blocks; callers run it off
/// the UI thread.
pub fn read_attachment(
    hgs: &Path,
    computer: &ComputerId,
    file: &AttachmentRef,
) -> Result<AttachmentRead, ReadError> {
    let invocation = attachment_invocation(computer, file)?;
    decode_attachment(file, limited(hgs, &invocation, ATTACHMENT_LIMIT)?)
}

/// Reads one attachment and keeps it in the private cache under `cache_root`
/// ([`crate::attachments::store_at`]), so a preview and Open share one copy.
pub fn fetch_attachment(
    hgs: &Path,
    computer: &ComputerId,
    file: &AttachmentRef,
    cache_root: &Path,
) -> Result<CachedAttachment, String> {
    let read = read_attachment(hgs, computer, file).map_err(|error| attachment_message(&error))?;
    let name = read
        .metadata
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let local_path = crate::attachments::store_at(cache_root, name, &read.bytes)
        .ok_or_else(|| "Could not save the attachment.".to_owned())?;
    Ok(CachedAttachment {
        metadata: read.metadata,
        local_path,
    })
}

/// The Qt client's text for a failed attachment read.
pub fn attachment_message(error: &ReadError) -> String {
    match error {
        ReadError::TooLarge { .. } => "Attachment response is too large.".into(),
        ReadError::Invalid { message } => message.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> ComputerId {
        ComputerId::from("studio")
    }

    fn code(stdout: &str) -> Exit {
        Exit::Code {
            code: 0,
            stdout: stdout.as_bytes().to_vec(),
            stderr: String::new(),
        }
    }

    fn file_ref() -> AttachmentRef {
        AttachmentRef {
            name: "claude/sample-project/review".into(),
            request_id: "req-1".into(),
            index: 0,
            conversation_id: "conv-1".into(),
            archive_id: None,
            agent_id: None,
        }
    }

    #[test]
    fn attachment_reads_use_qt_arguments_and_timeouts() {
        let local = attachment_invocation(&ComputerId::local(), &file_ref()).unwrap();
        assert_eq!(
            local.args,
            [
                "attachment",
                "claude/sample-project/review",
                "--request",
                "req-1",
                "--index",
                "0",
                "--conversation",
                "conv-1"
            ]
        );
        assert_eq!(local.timeout, Duration::from_secs(10));
        let mut file = file_ref();
        file.archive_id = Some("a-7".into());
        file.agent_id = Some("agent-2".into());
        let remote = attachment_invocation(&peer(), &file).unwrap();
        assert_eq!(&remote.args[0], "@studio");
        assert_eq!(
            &remote.args[9..],
            ["--archive", "a-7", "--agent", "agent-2"]
        );
        assert_eq!(remote.timeout, Duration::from_secs(45));
        file.request_id.clear();
        assert!(matches!(
            attachment_invocation(&peer(), &file),
            Err(ReadError::BadRequest { .. })
        ));
    }

    #[test]
    fn attachment_replies_must_match_and_decode() {
        let decoded = decode_attachment(
            &file_ref(),
            code(r#"{"request_id":"req-1","index":0,"name":"a.txt","mime":"text/plain","data_base64":"aGk="}"#),
        )
        .unwrap();
        assert_eq!(decoded.bytes, b"hi");
        assert_eq!(
            decoded.metadata,
            json!({"request_id": "req-1", "index": 0, "name": "a.txt", "mime": "text/plain"})
        );
        let error = ReadError::Invalid {
            message: "Invalid attachment response. Try again.".into(),
        };
        let invalid = Err(error.clone());
        for reply in [
            r#"{"request_id":"req-2","index":0,"data_base64":"aGk="}"#,
            r#"{"request_id":"req-1","index":1,"data_base64":"aGk="}"#,
            r#"{"request_id":"req-1","index":0,"data_base64":"not base64!"}"#,
            r#"{"request_id":"req-1","index":0,"data_base64":""}"#,
            r#"{"request_id":"req-1","index":0}"#,
            r#"["not an object"]"#,
        ] {
            assert_eq!(
                decode_attachment(&file_ref(), code(reply)),
                invalid,
                "{reply}"
            );
        }
        assert_eq!(
            attachment_message(&error),
            "Invalid attachment response. Try again."
        );
        assert_eq!(
            attachment_message(&ReadError::TooLarge {
                limit: ATTACHMENT_LIMIT
            }),
            "Attachment response is too large."
        );
    }

    #[test]
    fn process_output_reads_name_the_run_and_need_an_object() {
        let read = Read::ProcessOutput {
            name: "claude/sample-project/review".into(),
            process_id: "job-1".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            archive_id: None,
            generation: Some("g-2".into()),
        };
        let inv = invocation(&peer(), &read).unwrap();
        assert_eq!(
            inv.args,
            [
                "@studio",
                "processes",
                "claude/sample-project/review",
                "--output",
                "job-1",
                "--run",
                "run-1",
                "--conversation",
                "conv-1",
                "--generation",
                "g-2"
            ]
        );
        assert_eq!(inv.timeout, Duration::from_secs(45));
        assert_eq!(
            decode(&read, code("[]")),
            Err(ReadError::Invalid {
                message: "Invalid process response".into()
            })
        );
        assert_eq!(
            decode(&read, code(r#"{"id":"job-1","output":"ok"}"#)),
            Ok(json!({"id": "job-1", "output": "ok"}))
        );
        let Read::ProcessOutput {
            name,
            process_id,
            run_id: _,
            conversation_id,
            archive_id,
            generation,
        } = read
        else {
            unreachable!()
        };
        let empty_run = Read::ProcessOutput {
            name,
            process_id,
            run_id: String::new(),
            conversation_id,
            archive_id,
            generation,
        };
        assert!(matches!(
            invocation(&peer(), &empty_run),
            Err(ReadError::BadRequest { .. })
        ));
    }

    #[test]
    fn reads_use_the_qt_arguments_timeouts_and_limits() {
        let cases = [
            (
                Read::Projects,
                peer(),
                vec!["@studio", "project", "ls", "--json"],
                15,
            ),
            (
                Read::Projects,
                ComputerId::local(),
                vec!["project", "ls", "--json"],
                5,
            ),
            (Read::Swarm, ComputerId::local(), vec!["swarm", "get"], 10),
            (
                Read::Machines,
                ComputerId::local(),
                vec!["machine", "ls"],
                8,
            ),
            (
                Read::MachineResolve {
                    alias: "studio".into(),
                },
                ComputerId::local(),
                vec!["machine", "resolve", "studio"],
                8,
            ),
            (
                Read::MachineCheck {
                    alias: "studio".into(),
                },
                ComputerId::local(),
                vec!["machine", "check", "studio"],
                25,
            ),
            (Read::Accounts, peer(), vec!["@studio", "account", "ls"], 25),
            (
                Read::AccountUsage {
                    id: "claude-1".into(),
                },
                peer(),
                vec!["@studio", "account", "inspect", "claude-1", "--refresh"],
                25,
            ),
            (
                Read::SessionAccountUsage {
                    name: "claude/sample-project/review".into(),
                },
                peer(),
                vec![
                    "@studio",
                    "account",
                    "inspect",
                    "--session",
                    "claude/sample-project/review",
                    "--refresh",
                ],
                25,
            ),
            (
                Read::Recovery,
                ComputerId::local(),
                vec!["recovery", "get"],
                10,
            ),
            (
                Read::Recovery,
                peer(),
                vec!["@studio", "recovery", "get"],
                25,
            ),
            (Read::Version, ComputerId::local(), vec!["--version"], 3),
        ];
        for (read, computer, args, seconds) in cases {
            let invocation = invocation(&computer, &read).unwrap();
            assert_eq!(invocation.args, args, "{read:?}");
            assert_eq!(invocation.timeout, Duration::from_secs(seconds), "{read:?}");
            assert_eq!(invocation.stdin, None);
        }
        let analytics = Read::Analytics {
            from: "2026-10-01".into(),
            through: "2026-10-28".into(),
        };
        let invocation = invocation(&peer(), &analytics).unwrap();
        assert_eq!(invocation.args, ["@studio", "analytics", "--json"]);
        let request: Value = serde_json::from_slice(&invocation.stdin.unwrap()).unwrap();
        assert_eq!(
            request,
            json!({"schema": 1, "action": "summary", "from": "2026-10-01", "through": "2026-10-28"})
        );
        assert_eq!(
            (
                Read::Version.limit(),
                analytics.limit(),
                Read::Swarm.limit()
            ),
            (128, ANALYTICS_LIMIT, READ_LIMIT)
        );
    }

    #[test]
    fn requests_a_computer_cannot_take_are_refused_before_running() {
        let refused = |computer: &ComputerId, read: Read| {
            matches!(
                invocation(computer, &read),
                Err(ReadError::BadRequest { .. })
            )
        };
        assert!(refused(&peer(), Read::Swarm));
        assert!(refused(&peer(), Read::Machines));
        assert!(refused(&peer(), Read::Version));
        assert!(refused(
            &ComputerId::from("-oProxyCommand=x"),
            Read::Accounts
        ));
        assert!(refused(
            &ComputerId::local(),
            Read::MachineCheck {
                alias: "two words".into()
            }
        ));
        assert!(refused(
            &ComputerId::local(),
            Read::AccountUsage { id: "--all".into() }
        ));
        assert!(refused(
            &ComputerId::local(),
            Read::AccountUsage { id: String::new() }
        ));
        let range = |from: &str, through: &str| Read::Analytics {
            from: from.into(),
            through: through.into(),
        };
        assert!(refused(
            &ComputerId::local(),
            range("2026-10-28", "2026-10-01")
        ));
        assert!(refused(
            &ComputerId::local(),
            range("2026-10-1", "2026-10-28")
        ));
        assert!(refused(
            &ComputerId::local(),
            range("yesterday", "2026-10-28")
        ));
        assert!(!refused(
            &ComputerId::local(),
            range("2026-10-28", "2026-10-28")
        ));
    }

    #[test]
    fn exits_map_to_errors_and_replies_are_checked_for_their_shape() {
        assert_eq!(decode(&Read::Projects, code("[]")), Ok(json!([])));
        assert!(matches!(
            decode(&Read::Projects, code("{}")),
            Err(ReadError::Invalid { .. })
        ));
        let accounts = json!({"defaults": {}, "host": "workstation", "profiles": [], "removed_profiles": [], "revision": "r1"});
        assert_eq!(
            decode(&Read::Accounts, code(&accounts.to_string())),
            Ok(accounts)
        );
        assert!(matches!(
            decode(
                &Read::Accounts,
                code(r#"{"profiles": {}, "revision": "r1"}"#)
            ),
            Err(ReadError::Invalid { .. })
        ));
        assert!(matches!(
            decode(&Read::Machines, code("not json")),
            Err(ReadError::Invalid { .. })
        ));
        assert_eq!(
            decode(
                &Read::Machines,
                code(r#"{"error": "machine settings are locked"}"#)
            ),
            Err(ReadError::Failed {
                code: Some(0),
                message: "machine settings are locked".into()
            })
        );
        assert_eq!(
            decode(
                &Read::Recovery,
                Exit::Code {
                    code: 2,
                    stdout: vec![],
                    stderr: format!("  {}\n", "e".repeat(3000))
                }
            ),
            Err(ReadError::Failed {
                code: Some(2),
                message: "e".repeat(MESSAGE_CHARS)
            })
        );
        assert_eq!(
            decode(
                &Read::Recovery,
                Exit::Code {
                    code: 3,
                    stdout: vec![],
                    stderr: String::new()
                }
            ),
            Err(ReadError::Failed {
                code: Some(3),
                message: "hgs exited with code 3".into()
            })
        );
        assert_eq!(
            decode(&Read::Recovery, Exit::TimedOut),
            Err(ReadError::TimedOut)
        );
        assert!(matches!(
            decode(&Read::Recovery, Exit::StartFailed("missing".into())),
            Err(ReadError::StartFailed { .. })
        ));
        assert!(matches!(
            decode(
                &Read::Recovery,
                Exit::Abnormal {
                    stderr: String::new()
                }
            ),
            Err(ReadError::Failed { code: None, .. })
        ));
        let analytics = Read::Analytics {
            from: "2026-10-01".into(),
            through: "2026-10-02".into(),
        };
        assert!(decode(&analytics, code(r#"{"schema": 1, "buckets": []}"#)).is_ok());
        assert!(matches!(
            decode(&analytics, code(r#"{"schema": 2}"#)),
            Err(ReadError::Invalid { .. })
        ));
        let error = serde_json::to_value(ReadError::TooLarge { limit: 7 }).unwrap();
        assert_eq!(error, json!({"error": "too_large", "limit": 7}));
    }

    #[test]
    fn the_version_is_one_hgs_line() {
        for (text, version) in [
            ("hgs 1.56.0\n", Some("1.56.0")),
            ("hgs 0.48.0-dev.1472", Some("0.48.0-dev.1472")),
            ("hgs 1.2.3+build.5", Some("1.2.3+build.5")),
            ("hgs 1.2", None),
            ("hgs 1.2.3-", None),
            ("hgs 1.2.3 extra", None),
            ("zerus 1.2.3", None),
        ] {
            assert_eq!(super::version(text), version, "{text:?}");
        }
        assert_eq!(
            decode(&Read::Version, code("hgs 1.56.0\n")),
            Ok(json!("1.56.0"))
        );
        assert!(matches!(
            decode(&Read::Version, code("usage")),
            Err(ReadError::Invalid { .. })
        ));
    }

    const ID: &str = "00000000-0000-4000-8000-000000000001";

    fn archive_read(cursor: &str) -> Read {
        Read::ArchivePage {
            from: 1000.0,
            until: 2000.0,
            cursor: cursor.into(),
            query: "q".into(),
        }
    }

    fn archived(id: &str, at: f64) -> Value {
        json!({"name": "alpha", "state": "archived", "archive_id": id, "archived_at": at})
    }

    fn page_with(change: impl FnOnce(&mut Value)) -> Value {
        let mut page = json!({"version": 1, "sessions": [archived(ID, 1500.0)],
            "next_cursor": "n", "total": 3, "from": 1000, "until": 2000, "query": "q"});
        change(&mut page);
        page
    }

    fn verdict(cursor: &str, page: Value) -> Option<String> {
        match decode(&archive_read(cursor), code(&page.to_string())) {
            Ok(_) => None,
            Err(ReadError::ArchiveInvalid { message }) => Some(message),
            Err(other) => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_archive_page_asks_as_qt_does() {
        let read = Read::ArchivePage {
            from: 1000.0,
            until: 2000.5,
            cursor: "c".into(),
            query: "q".into(),
        };
        let invocation = invocation(&peer(), &read).unwrap();
        assert_eq!(invocation.args, ["@studio", "archives", "--json"]);
        assert_eq!(invocation.timeout, Duration::from_secs(30));
        assert_eq!(read.limit(), READ_LIMIT);
        let request: Value = serde_json::from_slice(&invocation.stdin.unwrap()).unwrap();
        assert_eq!(
            request,
            json!({"from": 1000, "until": 2000.5, "limit": 50, "cursor": "c", "query": "q"})
        );
        let text = String::from_utf8(
            super::invocation(&ComputerId::local(), &read)
                .unwrap()
                .stdin
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains("\"from\":1000,"), "{text}");
        assert_eq!(
            super::invocation(&ComputerId::local(), &read).unwrap().args,
            ["archives", "--json"]
        );
    }

    #[test]
    fn an_archived_activity_asks_as_qt_does() {
        let read = Read::ArchivedActivity {
            name: "alpha".into(),
            archive_id: "a1".into(),
            after: 42,
        };
        let peer = invocation(&peer(), &read).unwrap();
        assert_eq!(
            peer.args,
            [
                "@studio",
                "inspect",
                "alpha",
                "--after",
                "42",
                "--skip-processes",
                "--archive",
                "a1"
            ]
        );
        assert_eq!(peer.timeout, Duration::from_secs(15));
        let local = invocation(&ComputerId::local(), &read).unwrap();
        assert_eq!(local.args[0], "inspect");
        assert_eq!(local.timeout, Duration::from_secs(5));
        assert_eq!(read.limit(), READ_LIMIT);
        for (name, id) in [("", "a1"), ("alpha", "")] {
            let bad = Read::ArchivedActivity {
                name: name.into(),
                archive_id: id.into(),
                after: 0,
            };
            assert!(matches!(
                invocation(&ComputerId::local(), &bad),
                Err(ReadError::BadRequest { message }) if message == ARCHIVE_IDENTITY_INVALID
            ));
        }
        assert!(matches!(
            decode(&read, code("[]")),
            Err(ReadError::ArchiveInvalid { message }) if message == ACTIVITY_INVALID
        ));
    }

    #[test]
    fn an_archived_process_inventory_asks_as_qt_does() {
        let read = Read::ArchivedProcesses {
            name: "alpha".into(),
            archive_id: "a1".into(),
        };
        let peer = invocation(&peer(), &read).unwrap();
        assert_eq!(
            peer.args,
            [
                "@studio",
                "inspect",
                "alpha",
                "--after",
                "0",
                "--archive",
                "a1"
            ]
        );
        assert_eq!(peer.timeout, Duration::from_secs(15));
        let local = invocation(&ComputerId::local(), &read).unwrap();
        assert_eq!(local.args[0], "inspect");
        assert_eq!(local.timeout, Duration::from_secs(5));
        assert_eq!(read.limit(), READ_LIMIT);
        for (name, id) in [("", "a1"), ("alpha", "")] {
            let bad = Read::ArchivedProcesses {
                name: name.into(),
                archive_id: id.into(),
            };
            assert!(matches!(
                invocation(&ComputerId::local(), &bad),
                Err(ReadError::BadRequest { message }) if message == ARCHIVE_IDENTITY_INVALID
            ));
        }
        let inventory = json!({"processes": {"items": [{"id": "p1"}], "complete": true}});
        assert_eq!(
            decode(&read, code(&inventory.to_string())).unwrap(),
            inventory
        );
    }

    #[test]
    fn a_valid_archive_page_passes_and_is_typed() {
        assert_eq!(verdict("", page_with(|_| {})), None);
        assert_eq!(verdict("c", page_with(|_| {})), None);
        let windowed = page_with(|p| {
            p["previous_cursor"] = json!("p");
            p["position"] = json!(2);
        });
        assert_eq!(verdict("", windowed.clone()), None);
        let typed = page(&windowed);
        assert_eq!(typed.total, 3);
        assert_eq!(typed.position, Some(2));
        assert_eq!(typed.previous_cursor.as_deref(), Some("p"));
        assert_eq!(typed.next_cursor, "n");
        assert_eq!(typed.rows[0].archive_id.as_deref(), Some(ID));
        assert_eq!(typed.rows[0].archived_at, Some(1500.0));
        // The last page: no rows, no cursor.
        let end = page_with(|p| {
            p["sessions"] = json!([]);
            p["next_cursor"] = json!("");
            p["total"] = json!(0);
        });
        assert_eq!(verdict("", end), None);
    }

    #[test]
    fn archive_failures_give_qt_texts() {
        let hgs = Path::new("/x/hgs");
        let text = |exit: Exit| archive_failure(hgs, &exit).map(|error| error.to_string());
        assert_eq!(text(code("{}")), None);
        assert_eq!(text(Exit::TimedOut).as_deref(), Some("timed out"));
        assert_eq!(
            text(Exit::StartFailed("No such file or directory".into())).as_deref(),
            Some("cannot start '/x/hgs': No such file or directory")
        );
        let failed = |code: i32, stderr: &str| Exit::Code {
            code,
            stdout: Vec::new(),
            stderr: stderr.to_owned(),
        };
        assert_eq!(
            text(failed(2, "  no archive here \n")).as_deref(),
            Some("no archive here")
        );
        assert_eq!(text(failed(2, " \n")).as_deref(), Some("exit code 2"));
        let long = text(failed(1, &"\u{e9}".repeat(5000))).unwrap();
        assert_eq!(long.chars().count(), 4000);
        assert_eq!(
            text(Exit::Abnormal {
                stderr: "killed".into()
            })
            .as_deref(),
            Some("killed")
        );
    }

    #[test]
    fn archive_page_checks_give_qt_texts() {
        let page_err = Some(ARCHIVE_PAGE_INVALID.to_owned());
        let cases: Vec<(&str, Value)> = vec![
            ("version", page_with(|p| p["version"] = json!(2))),
            (
                "no version",
                page_with(|p| {
                    p.as_object_mut().unwrap().remove("version");
                }),
            ),
            ("sessions", page_with(|p| p["sessions"] = json!({}))),
            (
                "51 rows",
                page_with(|p| {
                    p["sessions"] = json!(vec![archived(ID, 1500.0); 51]);
                    p["total"] = json!(60);
                }),
            ),
            ("cursor type", page_with(|p| p["next_cursor"] = json!(1))),
            (
                "cursor long",
                page_with(|p| p["next_cursor"] = json!("x".repeat(1025))),
            ),
            ("total type", page_with(|p| p["total"] = json!("3"))),
            ("total low", page_with(|p| p["total"] = json!(0))),
            ("from", page_with(|p| p["from"] = json!(999))),
            ("until", page_with(|p| p["until"] = json!(2001))),
            ("query", page_with(|p| p["query"] = json!("other"))),
            (
                "cursor without rows",
                page_with(|p| p["sessions"] = json!([])),
            ),
            ("not an object", json!([])),
        ];
        for (name, page) in cases {
            assert_eq!(verdict("", page), page_err, "{name}");
        }
        assert_eq!(
            verdict("n", page_with(|_| {})),
            page_err,
            "next_cursor equal to the query cursor"
        );
        for text in ["not json", ""] {
            assert_eq!(
                decode(&archive_read(""), code(text)),
                Err(ReadError::ArchiveInvalid {
                    message: ARCHIVE_PAGE_INVALID.into()
                })
            );
        }
        // No total is Qt's 0, so a window with a row fails as the window.
        assert_eq!(
            verdict(
                "",
                page_with(|p| {
                    p.as_object_mut().unwrap().remove("total");
                    p["previous_cursor"] = json!("");
                    p["position"] = json!(0);
                })
            ),
            Some(ARCHIVE_WINDOW_INVALID.to_owned())
        );
        assert_eq!(
            verdict(
                "",
                page_with(|p| p["next_cursor"] = json!("x".repeat(1024)))
            ),
            None
        );
        assert_eq!(
            verdict(
                "",
                page_with(|p| {
                    p["sessions"] = json!(vec![archived(ID, 1500.0); 50]);
                    p["total"] = json!(50);
                    p["next_cursor"] = json!("");
                })
            ),
            None
        );
    }

    #[test]
    fn archive_window_checks_give_the_window_text() {
        let window = Some(ARCHIVE_WINDOW_INVALID.to_owned());
        let cases: Vec<(&str, Value)> = vec![
            ("position alone", page_with(|p| p["position"] = json!(0))),
            (
                "previous alone",
                page_with(|p| p["previous_cursor"] = json!("")),
            ),
            (
                "previous type",
                page_with(|p| {
                    p["previous_cursor"] = json!(1);
                    p["position"] = json!(0);
                }),
            ),
            (
                "previous long",
                page_with(|p| {
                    p["previous_cursor"] = json!("x".repeat(1025));
                    p["position"] = json!(0);
                }),
            ),
            (
                "position type",
                page_with(|p| {
                    p["previous_cursor"] = json!("");
                    p["position"] = json!("0");
                }),
            ),
            (
                "position negative",
                page_with(|p| {
                    p["previous_cursor"] = json!("");
                    p["position"] = json!(-1);
                }),
            ),
            (
                "position fraction",
                page_with(|p| {
                    p["previous_cursor"] = json!("");
                    p["position"] = json!(0.5);
                }),
            ),
            (
                "position past total",
                page_with(|p| {
                    p["previous_cursor"] = json!("");
                    p["position"] = json!(3);
                }),
            ),
        ];
        for (name, page) in cases {
            assert_eq!(verdict("", page), window, "{name}");
        }
        let edge = page_with(|p| {
            p["previous_cursor"] = json!("");
            p["position"] = json!(2);
        });
        assert_eq!(verdict("", edge), None);
    }

    #[test]
    fn archive_row_checks_give_the_identity_text() {
        let identity = Some(ARCHIVE_IDENTITY_INVALID.to_owned());
        let bad_row = |row: Value| verdict("", page_with(|p| p["sessions"] = json!([row])));
        let mut live = archived(ID, 1500.0);
        live["state"] = json!("running");
        assert_eq!(bad_row(live), identity, "state");
        let mut unnamed = archived(ID, 1500.0);
        unnamed["name"] = json!("");
        assert_eq!(bad_row(unnamed), identity, "name");
        for id in [
            "00000000-0000-4000-8000-00000000000A",
            "{00000000-0000-4000-8000-000000000001}",
            "00000000000040008000000000000001",
            "00000000-0000-4000-8000-00000000000g",
            "",
            "00000000-0000-4000-8000-0000000000011",
        ] {
            assert_eq!(bad_row(archived(id, 1500.0)), identity, "{id}");
        }
        let mut no_id = archived(ID, 1500.0);
        no_id.as_object_mut().unwrap().remove("archive_id");
        assert_eq!(bad_row(no_id), identity);
        for at in [999.0, 2000.0, 2500.0] {
            assert_eq!(bad_row(archived(ID, at)), identity, "{at}");
        }
        assert_eq!(bad_row(archived(ID, 1000.0)), None, "from is inclusive");
        let mut text_at = archived(ID, 1500.0);
        text_at["archived_at"] = json!("1500");
        assert_eq!(bad_row(text_at), identity);
        // The identity error wins over the others, as in Qt.
        let both = page_with(|p| {
            p["version"] = json!(2);
            p["sessions"] = json!([archived("x", 1500.0)]);
        });
        assert_eq!(verdict("", both), identity);
    }
}
