//! The actor owns launch admission, fixed plans, the original launch child and
//! its actual custody. Its records stay separate from clone, worktree and
//! task-start reconciliation.
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
pub(crate) use child::adopt_custody;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use recovery::Recovery;
#[cfg_attr(windows, allow(dead_code))]
// Windows keeps bounded receipts; this executor is POSIX-only, like clone.
mod ledger;

// Merge-order allocation: the next free Store version when this lands. A
// later schema merge renumbers this one constant, never standalone DDL.
pub(super) const SCHEMA: i64 = 24;
/// Test rewind of this leaf for older-version upgrade fixtures; dropping the
/// tables drops their triggers and indexes.
#[cfg(test)]
pub(super) fn remove_fixture(db: &rusqlite::Connection) {
    db.execute_batch(
        "DROP TABLE desktop_creation_launches; DROP TABLE desktop_creation_launch_budget;",
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

/// Bounded indexed original-result read. Fresh query scope identifies the
/// service; a historical receipt remains data and grants no new effect.
/// Beside an Unknown original only, it adds the bounded observation of that
/// launch's own records: display evidence that never settles it.
pub(super) fn query(store: &super::store::Store, value: &serde_json::Value) -> serde_json::Value {
    use anyhow::{ensure, Context};
    use zerus_client_core::desktop::creation_launch as wire;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        op: String,
        query: wire::Query,
    }
    let result = (|| -> anyhow::Result<wire::Lookup> {
        let read: Read = serde_json::from_value(value.clone())?;
        ensure!(
            read.op == "creation_launch_result" && read.query.valid(),
            "invalid creation query"
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
            "creation query scope changed"
        );
        let row = ledger::load(&store.db, &read.query.launch_id)?;
        let observation = row
            .as_ref()
            .filter(|row| row.receipt.phase == wire::Phase::Unknown)
            .map(|_| observe(&read.query.launch_id));
        let mut reply = wire::Lookup {
            abi: wire::ABI.into(),
            request_id: read.query.request_id.clone(),
            scope: read.query.scope.clone(),
            read_only: true,
            found: row.is_some(),
            receipt: row.map(|r| r.receipt),
            observation,
        };
        if !reply.matches(&read.query) {
            // Evidence never decides whether an original may be read.
            reply.observation = None;
        }
        ensure!(reply.matches(&read.query), "creation original changed");
        ensure!(
            serde_json::to_vec(&reply).context("creation reply")?.len() <= wire::REPLY_BYTES,
            "creation reply exceeds bound"
        );
        Ok(reply)
    })();
    result.map(|v|serde_json::json!(v)).unwrap_or_else(|_|serde_json::json!({"accepted":false,"read_only":true,"code":"creation_result_unavailable"}))
}

/// The bounded observation, read off the serve thread. At most one read is
/// outstanding: while an earlier one has not returned, the answer is Unknown
/// and no further reader starts. A read that misses its deadline is kept until
/// it actually ends, so readers never accumulate.
fn observe(launch: &str) -> zerus_client_core::desktop::creation_launch::Observation {
    use std::sync::{mpsc, Mutex};
    use std::time::{Duration, Instant};
    use zerus_client_core::desktop::creation_launch::Observation;
    static OUTSTANDING: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
    let mut outstanding = OUTSTANDING.lock().unwrap_or_else(|e| e.into_inner());
    if outstanding
        .as_ref()
        .is_some_and(|reader| !reader.is_finished())
    {
        return Observation::Unknown;
    }
    if let Some(reader) = outstanding.take() {
        let _ = reader.join();
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let (send, receive) = mpsc::sync_channel(1);
    let launch = launch.to_owned();
    let Ok(reader) = std::thread::Builder::new()
        .name("creation-launch-observation".into())
        .spawn(move || {
            let _ = send.send(crate::state::observe_own_launch(&launch, deadline));
        })
    else {
        return Observation::Unknown;
    };
    let observation = receive
        .recv_timeout(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(50),
        )
        .unwrap_or(Observation::Unknown);
    if reader.is_finished() {
        let _ = reader.join();
    } else {
        *outstanding = Some(reader);
    }
    observation
}

#[cfg(test)]
mod tests;
