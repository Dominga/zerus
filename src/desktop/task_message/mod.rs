//! The actor owns task message admission, fixed plans, the original
//! `hgs task message` child and its actual custody. Its records stay separate
//! from the creation leaves and task-start reconciliation.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod child;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod consent;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod runtime;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use runtime::Runtime;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod recovery;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod staging;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use child::adopt_custody;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use recovery::Recovery;
#[cfg_attr(windows, allow(dead_code))]
// The reference form's custody inputs; migrated everywhere, used on POSIX.
mod inputs;
#[cfg_attr(windows, allow(dead_code))]
// Windows keeps bounded receipts; this executor is POSIX-only, like clone.
mod ledger;

// Merge-order allocation: the next free Store version when this lands. A
// later schema merge renumbers this one constant, never standalone DDL.
pub(super) const SCHEMA: i64 = 26;
/// The custody-input records of reference originals, the next Store version.
pub(super) const INPUTS_SCHEMA: i64 = inputs::SCHEMA;
/// Test rewind of this leaf for older-version upgrade fixtures; dropping the
/// tables drops their triggers and indexes.
#[cfg(test)]
pub(super) fn remove_fixture(db: &rusqlite::Connection) {
    remove_inputs_fixture(db);
    db.execute_batch("DROP TABLE desktop_task_messages; DROP TABLE desktop_task_message_budget;")
        .unwrap();
}
/// Test rewind of the custody-input records alone (schema 27 to 26).
#[cfg(test)]
pub(super) fn remove_inputs_fixture(db: &rusqlite::Connection) {
    db.execute_batch(
        "DROP TABLE desktop_task_message_inputs; DROP TABLE desktop_task_message_input_budget;",
    )
    .unwrap();
}
pub(super) fn migrate(tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    ledger::migrate(tx)
}
pub(super) fn migrate_inputs(tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    inputs::migrate(tx)
}
pub(super) fn undrained(db: &rusqlite::Connection) -> anyhow::Result<u64> {
    ledger::undrained(db)
}
pub(super) fn summary(db: &rusqlite::Connection) -> anyhow::Result<serde_json::Value> {
    let mut summary = ledger::summary(db)?;
    // The reference form's custody inputs carry their own storage charge.
    let (rows, bytes) = inputs::totals(db)?;
    summary["inputs"] = serde_json::json!({"charged": rows, "bytes": bytes});
    Ok(summary)
}
pub(super) fn startup(db: &mut rusqlite::Connection) -> anyhow::Result<()> {
    ledger::startup(db)
}

/// The read-only discovery of this service's staging directory for the
/// reference form: routing data for the exact current scope, never
/// permission. Windows, which executes no task message, has none.
pub(super) fn staging_route(
    store: &super::store::Store,
    location: &super::store::Location,
    value: &serde_json::Value,
) -> serde_json::Value {
    use zerus_client_core::desktop::task_message as wire;
    let unavailable = serde_json::json!({"abi": wire::ABI, "read_only": true,
        "code": "task_message_staging_unavailable"});
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (store, location, value);
        unavailable
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use anyhow::ensure;
        use std::os::unix::fs::OpenOptionsExt;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Read {
            op: String,
            scope: wire::Scope,
        }
        let result = (|| -> anyhow::Result<wire::Staging> {
            let read: Read = serde_json::from_value(value.clone())?;
            ensure!(
                read.op == "task_message_staging" && read.scope.valid(),
                "invalid task message staging query"
            );
            let scope: (String, String) = store.db.query_row(
                "SELECT configuration,epoch FROM service WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            ensure!(
                scope
                    == (
                        read.scope.configuration.clone(),
                        read.scope.service_epoch.clone()
                    ),
                "task message staging scope changed"
            );
            // Only a held, private directory is named: the actor creates it
            // when it starts, and a client never creates it.
            let staging = location.root.join(staging::STAGING);
            let held = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(
                    libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
                .open(&staging)?;
            ensure!(
                crate::platform::verify_private_directory(&held)?,
                "task message staging is not private"
            );
            Ok(wire::Staging {
                abi: wire::ABI.into(),
                scope: read.scope,
                read_only: true,
                staging: staging.to_str().unwrap_or_default().into(),
            })
        })();
        match result {
            Ok(staging) => serde_json::json!(staging),
            Err(_) => unavailable,
        }
    }
}

/// Bounded indexed original-result read. Fresh query scope identifies the
/// service; a historical receipt remains data and grants no new effect.
pub(super) fn query(store: &super::store::Store, value: &serde_json::Value) -> serde_json::Value {
    use anyhow::{ensure, Context};
    use zerus_client_core::desktop::task_message as wire;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        op: String,
        query: wire::Query,
    }
    let result = (|| -> anyhow::Result<wire::Lookup> {
        let read: Read = serde_json::from_value(value.clone())?;
        ensure!(
            read.op == "task_message_result" && read.query.valid(),
            "invalid task message query"
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
            "task message query scope changed"
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
        ensure!(reply.matches(&read.query), "task message original changed");
        ensure!(
            serde_json::to_vec(&reply)
                .context("task message reply")?
                .len()
                <= wire::REPLY_BYTES,
            "task message reply exceeds bound"
        );
        Ok(reply)
    })();
    result.map(|v|serde_json::json!(v)).unwrap_or_else(|_|serde_json::json!({"accepted":false,"read_only":true,"code":"task_message_result_unavailable"}))
}

#[cfg(test)]
mod tests;
