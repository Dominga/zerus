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
use crate::run::{run_limited, TooLarge};
use crate::ComputerId;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
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

/// One read.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// `hgs [@host] recovery get`.
    Recovery,
    /// `hgs --version`: the version as a JSON string.
    Version,
    /// `hgs [@host] analytics --json` with `{schema: 1, action: "summary",
    /// from, through}` (UTC dates, `YYYY-MM-DD`, inclusive). A computer whose
    /// help lacks [`ANALYTICS_MARKER`] is [`ReadError::Unsupported`]: an
    /// older hgs would take the unknown verb as a session launch.
    Analytics { from: String, through: String },
}

impl Read {
    /// Whether the read goes to the named computer; the others are this
    /// computer's.
    pub fn per_computer(&self) -> bool {
        matches!(
            self,
            Self::Projects
                | Self::Accounts
                | Self::AccountUsage { .. }
                | Self::Recovery
                | Self::Analytics { .. }
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
    };
    Ok(Invocation {
        args,
        stdin,
        timeout,
        request_id: None,
    })
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
        Exit::WriteFailed => Err(failed(None, "hgs did not take the request")),
    }
}

fn failed(code: Option<i32>, stderr: &str) -> ReadError {
    let text: String = stderr.trim().chars().take(MESSAGE_CHARS).collect();
    let message = if text.is_empty() {
        match code {
            Some(code) => format!("hgs exited with code {code}"),
            None => "hgs ended abnormally".to_owned(),
        }
    } else {
        text
    };
    ReadError::Failed { code, message }
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
    let value: Value =
        serde_json::from_slice(&stdout).map_err(|error| invalid(&error.to_string()))?;
    if *read == Read::Projects {
        return match value {
            Value::Array(_) => Ok(value),
            _ => Err(invalid("project ls gives an array")),
        };
    }
    let Value::Object(object) = &value else {
        return Err(invalid("not a JSON object"));
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
}
