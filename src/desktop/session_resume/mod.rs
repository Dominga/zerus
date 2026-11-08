//! The actor owns resume-then-send admission, the fixed plan, the original
//! `hgs session-action` resume and `hgs send` children and their actual
//! custody (zerus-5wmh.66.4.9, slice 3, its fourth kind). Its records stay
//! separate from every other leaf.
//!
//! One operation resumes a local Claude, Kimi or Codex session through the
//! receipted session action and, only in the same live admitted operation
//! after that resume's own exact receipt recorded the resumed run and the
//! resumed agent shows it takes a message, sends the person's message bound
//! to that run. A resumed Codex shows that only on its terminal, so the
//! send's own effect-free observation decides it (zerus-5wmh.66.4.10).
//! Restart never continues an operation.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod child;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod consent;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod ready;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod receipt;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod recovery;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod runtime;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use child::adopt_custody;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use recovery::{Owned, Recovery};
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod target;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use runtime::Runtime;
#[cfg_attr(windows, allow(dead_code))]
// Windows keeps bounded receipts; this executor is POSIX-only, like clone.
mod ledger;

use anyhow::{ensure, Context};
use zerus_client_core::desktop::session_resume as wire;

// Merge-order allocation: the next free Store version when this lands. A
// later schema merge renumbers this one constant, never standalone DDL.
pub(super) const SCHEMA: i64 = 34;
/// Test rewind of this leaf for older-version upgrade fixtures; dropping the
/// tables drops their triggers and indexes.
#[cfg(test)]
pub(super) fn remove_fixture(db: &rusqlite::Connection) {
    super::creation_worktree::rewind_slots(db);
    db.execute_batch(
        "DROP TABLE desktop_session_resumes; DROP TABLE desktop_session_resume_budget;",
    )
    .unwrap();
}
pub(super) fn migrate(tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    ledger::migrate(tx)
}
pub(super) fn undrained(db: &rusqlite::Connection) -> anyhow::Result<u64> {
    ledger::undrained(db)
}
pub(super) fn summary(db: &rusqlite::Connection) -> anyhow::Result<serde_json::Value> {
    ledger::summary(db)
}
pub(super) fn startup(db: &mut rusqlite::Connection) -> anyhow::Result<()> {
    ledger::startup(db)
}

/// The scoped read-only lookup of one operation's receipt
/// (`session_resume_result`). It never resumes, sends, admits or retries
/// anything.
pub(super) fn query(store: &super::store::Store, value: &serde_json::Value) -> serde_json::Value {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        op: String,
        query: wire::Query,
    }
    let result = (|| -> anyhow::Result<wire::Lookup> {
        let read: Read = serde_json::from_value(value.clone())?;
        ensure!(
            read.op == "session_resume_result" && read.query.valid(),
            "invalid session resume query"
        );
        let scope: (String, String) = store.db.query_row(
            "SELECT configuration,epoch FROM service WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            scope
                == (
                    read.query.scope.configuration.clone(),
                    read.query.scope.service_epoch.clone()
                ),
            "session resume query scope changed"
        );
        let row = ledger::load(&store.db, &read.query.request_id)?;
        let reply = wire::Lookup {
            abi: wire::ABI.into(),
            request_id: read.query.request_id.clone(),
            scope: read.query.scope.clone(),
            read_only: true,
            found: row.is_some(),
            receipt: row.map(|r| r.receipt),
        };
        ensure!(
            reply.matches(&read.query),
            "session resume original changed"
        );
        ensure!(
            serde_json::to_vec(&reply)
                .context("session resume reply")?
                .len()
                <= wire::REPLY_BYTES,
            "session resume reply exceeds bound"
        );
        Ok(reply)
    })();
    result.map(|v| serde_json::json!(v)).unwrap_or_else(|_| {
        serde_json::json!({"accepted":false,"read_only":true,"code":"session_resume_result_unavailable"})
    })
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;
