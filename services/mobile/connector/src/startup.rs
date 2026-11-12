//! Startup evidence contains only fixed stage names and numeric error codes.
//! Never copy configuration, paths, driver messages or raw stderr to a report.
use crate::journal::JournalError;
use serde::{Deserialize, Serialize};

pub const STDERR_LIMIT: usize = 16 * 1024;
const PREFIX: &str = "ERROR: Connector startup diagnostic ";

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Arguments,
    Environment,
    AdmissionLock,
    Configuration,
    Revision,
    WorkspaceBinding,
    JournalOpen,
    LegacyControl,
    Runtime,
    EncryptedProfile,
    EncryptedJournal,
    GenerationLock,
    GenerationRandom,
    GenerationPublish,
    Panic,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "code",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Reason {
    Entered,
    Refused,
    Io(Option<i32>),
    Sqlite(Option<i32>),
    PanicLine(Option<u32>),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostic {
    stage: Stage,
    reason: Reason,
}

fn record(stage: Stage, reason: Reason) {
    #[cfg(not(feature = "test-hooks"))]
    let _ = (stage, reason);
    #[cfg(feature = "test-hooks")]
    if crate::hooks::capture_enabled() {
        if let Ok(line) = serde_json::to_string(&Diagnostic { stage, reason }) {
            crate::runtime::log("ERROR", &format!("Connector startup diagnostic {line}"));
        }
    }
}

pub fn refused(stage: Stage) {
    record(stage, Reason::Refused);
}

pub fn enter(stage: Stage) {
    record(stage, Reason::Entered);
}

#[cfg(feature = "test-hooks")]
pub fn install_fixture_panic_hook() {
    if crate::hooks::capture_enabled() {
        std::panic::set_hook(Box::new(|info| {
            record(
                Stage::Panic,
                Reason::PanicLine(info.location().map(|location| location.line())),
            );
        }));
    }
}

pub fn io(stage: Stage, error: &std::io::Error) {
    record(stage, Reason::Io(error.raw_os_error()));
}

pub fn journal(stage: Stage, error: &JournalError) {
    match error {
        JournalError::Refused(_) => refused(stage),
        JournalError::Io(error) => io(stage, error),
        JournalError::Storage(error) => record(
            stage,
            Reason::Sqlite(error.sqlite_error().map(|error| error.extended_code)),
        ),
    }
}

/// Re-encode only recognized diagnostics. Arbitrary stderr, including panic
/// output from dependencies, stays in the private fixture and is never uploaded.
pub fn redacted_stderr(bytes: &[u8]) -> Vec<String> {
    let bytes = &bytes[..bytes.len().min(STDERR_LIMIT)];
    bytes
        .split(|&byte| byte == b'\n')
        .filter_map(|line| {
            std::str::from_utf8(line)
                .ok()?
                .trim_end_matches('\r')
                .strip_prefix(PREFIX)
        })
        .filter_map(|line| serde_json::from_str::<Diagnostic>(line).ok())
        .filter_map(|diagnostic| serde_json::to_string(&diagnostic).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_closed_startup_diagnostics_are_exported() {
        let allowed = r#"{"stage":"generation_publish","reason":{"kind":"io","code":5}}"#;
        let raw = format!(
            "sensitive raw stderr\n{PREFIX}{allowed}\r\n{PREFIX}{}\n{PREFIX}{}\n",
            r#"{"stage":"private configuration text","reason":{"kind":"refused"}}"#,
            r#"{"stage":"runtime","reason":{"kind":"refused"},"extra":"private value"}"#
        );
        assert_eq!(redacted_stderr(raw.as_bytes()), [allowed]);
        let over_budget = format!("{}\n{PREFIX}{allowed}\n", "x".repeat(STDERR_LIMIT));
        assert!(redacted_stderr(over_budget.as_bytes()).is_empty());
    }
}
