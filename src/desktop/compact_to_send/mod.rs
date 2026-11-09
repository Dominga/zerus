//! The actor owns compact-to-send admission, the fixed plan, the original
//! `hgs compact-context` and `hgs send` children and their actual custody
//! (zerus-5wmh.66.4.8, slice 3, its third kind). Its records stay separate
//! from every other leaf.
//!
//! This slice runs the compaction only: after its drain the send is recorded
//! as never started (its child was never reserved), and the operation
//! drains. The wait for the compaction to complete and the send follow in
//! the next slice; until then no help marker is advertised, so no client
//! offers the kind.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod child;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod consent;
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
mod slot;
// Checked last before each child: by the Linux runner, and by the test
// executor that stands in for it.
#[cfg(any(target_os = "linux", all(test, target_os = "macos")))]
mod target;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use runtime::Runtime;
#[cfg_attr(windows, allow(dead_code))]
// Windows keeps bounded receipts; this executor is POSIX-only, like clone.
mod ledger;

use anyhow::{ensure, Context};
use zerus_client_core::desktop::compact_to_send as wire;

// Merge-order allocation: the next free Store version when this lands. A
// later schema merge renumbers this one constant, never standalone DDL.
pub(super) const SCHEMA: i64 = 30;
/// Test rewind of this leaf for older-version upgrade fixtures; dropping the
/// tables drops their triggers and indexes.
#[cfg(test)]
pub(super) fn remove_fixture(db: &rusqlite::Connection) {
    // Rewind the later schema-34 session resume leaf too, never leaving it
    // labelled older.
    super::session_resume::remove_fixture(db);
    db.execute_batch("DROP INDEX desktop_attention_catalog_run;")
        .unwrap();
    db.execute_batch(
        "DROP TABLE desktop_compact_to_sends; DROP TABLE desktop_compact_to_send_budget;",
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
/// (`compact_to_send_result`). It never compacts, sends, admits or retries
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
            read.op == "compact_to_send_result" && read.query.valid(),
            "invalid compact-to-send query"
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
            "compact-to-send query scope changed"
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
            "compact-to-send original changed"
        );
        ensure!(
            serde_json::to_vec(&reply)
                .context("compact-to-send reply")?
                .len()
                <= wire::REPLY_BYTES,
            "compact-to-send reply exceeds bound"
        );
        Ok(reply)
    })();
    result.map(|v| serde_json::json!(v)).unwrap_or_else(|_| {
        serde_json::json!({"accepted":false,"read_only":true,"code":"compact_to_send_result_unavailable"})
    })
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;
