//! Actor-owned original outcome retention. Rows are immutable DATA, never a
//! Terminate/cleanup command, successful receipt or resource-drain witness.
use super::{store::Store, ui_authority::Validated};
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::Deserialize;
use serde_json::{json, Value};
use zerus_client_core::desktop::termination_outcome::{self as wire, Intent, Status};

// Merge-order allocation after the actual creation-launch schema 24.
pub(super) const SCHEMA: i64 = 25;

pub(super) fn migrate(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute_batch(&format!(
        "CREATE TABLE desktop_termination_outcomes(
            request_id TEXT PRIMARY KEY NOT NULL CHECK(length(request_id)=36),
            confirmation_id TEXT NOT NULL CHECK(length(confirmation_id)=36),
            configuration TEXT NOT NULL CHECK(length(configuration)=64),
            fingerprint TEXT NOT NULL CHECK(length(fingerprint)=64),
            original_sha256 TEXT NOT NULL CHECK(length(original_sha256)=64),
            original TEXT NOT NULL CHECK(json_valid(original) AND length(CAST(original AS BLOB)) BETWEEN 1 AND {}),
            retained_at INTEGER NOT NULL CHECK(retained_at>=0),
            UNIQUE(configuration,confirmation_id)
        ) STRICT;
        CREATE TRIGGER desktop_termination_outcomes_capacity BEFORE INSERT ON desktop_termination_outcomes
        WHEN (SELECT count(*) FROM (SELECT 1 FROM desktop_termination_outcomes LIMIT {}))>={}
        BEGIN SELECT RAISE(ABORT,'termination outcome capacity'); END;
        CREATE TRIGGER desktop_termination_outcomes_immutable BEFORE UPDATE ON desktop_termination_outcomes
        BEGIN SELECT RAISE(ABORT,'original termination outcome immutable'); END;
        CREATE TRIGGER desktop_termination_outcomes_preserve BEFORE DELETE ON desktop_termination_outcomes
        BEGIN SELECT RAISE(ABORT,'original termination outcome retained'); END;",
        wire::RECORD_BYTES,
        wire::RECORDS + 1,
        wire::RECORDS,
    ))?;
    Ok(())
}

#[cfg(test)]
pub(super) fn remove_fixture(db: &Connection) {
    super::task_message::remove_fixture(db);
    db.execute_batch("DROP TABLE desktop_termination_outcomes;")
        .unwrap();
}

fn status(intent: &Intent, fingerprint: String) -> Result<Status> {
    Ok(Status {
        abi: wire::ABI.into(),
        configuration: intent.scope.configuration.clone(),
        confirmation_id: intent.confirmation_id.clone(),
        choice: intent.choice,
        original_request_id: intent.request_id.clone(),
        original_service_epoch: intent.scope.service_epoch.clone(),
        fingerprint,
        capture_sha256: wire::hash(&serde_json::to_vec(&intent.capture)?),
        request_sha256: wire::hash(intent.original.as_bytes()),
    })
}

fn load(db: &Connection, id: &str, configuration: &str) -> Result<Option<Status>> {
    let stored: Option<(String, String, String, String, String)> = db
        .query_row(
            "SELECT original,fingerprint,configuration,confirmation_id,original_sha256 FROM desktop_termination_outcomes WHERE request_id=?1",
            [id],
            |row| {
                let bytes = row.get_ref(0)?.as_str()?;
                let fingerprint = row.get_ref(1)?.as_str()?;
                let configuration = row.get_ref(2)?.as_str()?;
                let confirmation = row.get_ref(3)?.as_str()?;
                let original_sha256 = row.get_ref(4)?.as_str()?;
                if bytes.len() > wire::RECORD_BYTES
                    || fingerprint.len() != 64
                    || configuration.len() != 64
                    || confirmation.len() != 36
                    || original_sha256.len() != 64
                {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                Ok((
                    bytes.into(),
                    fingerprint.into(),
                    configuration.into(),
                    confirmation.into(),
                    original_sha256.into(),
                ))
            },
        )
        .optional()?;
    stored
        .map(
            |(bytes, fingerprint, selected, confirmation, original_sha256)| {
                // Correlation deliberately excludes the current epoch. Integrity of
                // the FIRST stored epoch and every other original byte is separate.
                ensure!(
                    wire::hash(bytes.as_bytes()) == original_sha256,
                    "stored termination original changed"
                );
                let original: Intent = serde_json::from_str(&bytes)?;
                ensure!(
                    selected == configuration
                        && original.scope.configuration == configuration
                        && original.request_id == id
                        && original.confirmation_id == confirmation
                        && original.fingerprint().as_deref() == Some(&fingerprint),
                    "original termination outcome changed"
                );
                status(&original, fingerprint)
            },
        )
        .transpose()
}

fn retain(store: &mut Store, authority: Validated) -> Result<Status> {
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    // The first native/capture check is AFTER BEGIN. This is not a decoded
    // role, a preference, old receipt or notification permission.
    let intent = authority.termination_outcome_parts()?.clone();
    let service: (String, String) = tx.query_row(
        "SELECT configuration,epoch FROM service WHERE id=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    ensure!(
        service
            == (
                intent.scope.configuration.clone(),
                intent.scope.service_epoch.clone()
            ),
        "termination outcome service changed"
    );
    let version: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    ensure!(
        version == super::store::SCHEMA,
        "termination outcome schema changed"
    );
    let fingerprint = intent
        .fingerprint()
        .context("invalid termination original")?;
    let retained = load(&tx, &intent.request_id, &intent.scope.configuration)?;
    if let Some(retained) = retained {
        ensure!(retained.matches(&intent), "termination original conflict");
        authority.check_termination_outcome()?;
        tx.commit()?;
        return Ok(retained);
    }
    let original = serde_json::to_string(&intent)?;
    ensure!(
        original.len() <= wire::RECORD_BYTES,
        "termination original too large"
    );
    let rows: usize = tx.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM desktop_termination_outcomes LIMIT 65)",
        [],
        |row| row.get(0),
    )?;
    ensure!(rows < wire::RECORDS, "termination outcome capacity");
    let original_sha256 = wire::hash(original.as_bytes());
    // Recheck the SAME original permit after reads/encoding and immediately
    // before insertion. No slow preparation can renew its two-second deadline.
    authority.check_termination_outcome()?;
    tx.execute(
        "INSERT INTO desktop_termination_outcomes VALUES(?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            intent.request_id,
            intent.confirmation_id,
            intent.scope.configuration,
            fingerprint,
            original_sha256,
            original,
            chrono::Utc::now().timestamp_millis()
        ],
    )?;
    tx.commit()?;
    status(&intent, fingerprint)
}

pub(super) fn frontend(store: &mut Store, authority: Validated) -> Value {
    let id = authority.id().to_owned();
    match retain(store, authority) {
        Ok(status) => json!({"op":"termination_outcome","request_id":id,
            "accepted":true,"durable":true,"phase":"retained","status":status,
            "cleanup_authorized":false}),
        Err(_) => json!({"op":"termination_outcome","request_id":id,
            "accepted":false,"durable":false,"code":"termination_outcome_refused"}),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    op: QueryOperation,
    request_id: String,
    original_request_id: String,
    configuration: String,
}
#[derive(Deserialize)]
enum QueryOperation {
    #[serde(rename = "termination_outcome_status")]
    Status,
}
pub(super) fn query(store: &Store, configuration: &str, value: &Value) -> Value {
    let result = (|| -> Result<(Query, Option<Status>)> {
        let request: Query = serde_json::from_value(value.clone())?;
        let _ = &request.op;
        let valid_id = |id: &str| {
            uuid::Uuid::parse_str(id)
                .is_ok_and(|parsed| !parsed.is_nil() && parsed.to_string() == id)
        };
        ensure!(
            request.configuration == configuration
                && valid_id(&request.request_id)
                && valid_id(&request.original_request_id),
            "termination query scope changed"
        );
        let selected: String =
            store
                .db
                .query_row("SELECT configuration FROM service WHERE id=1", [], |row| {
                    row.get(0)
                })?;
        ensure!(
            selected == configuration,
            "termination query service changed"
        );
        let status = load(&store.db, &request.original_request_id, configuration)?;
        Ok((request, status))
    })();
    match result {
        Ok((request, status)) => json!({"op":"termination_outcome_status",
            "request_id":request.request_id,"original_request_id":request.original_request_id,
            "accepted":true,"durable":false,"found":status.is_some(),"status":status,
            "cleanup_authorized":false}),
        Err(_) => json!({"op":"termination_outcome_status","request_id":value["request_id"],
            "accepted":false,"durable":false,"code":"termination_outcome_unavailable"}),
    }
}

#[cfg(test)]
mod tests;
