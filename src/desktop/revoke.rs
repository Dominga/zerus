//! Durable revocation, owned by the actor (or an absent-service OS-lock owner).
//! This decreases authority. It never sends native input or starts another GUI.
use super::{
    handoff,
    store::{HandoffLock, Location, Store},
};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    #[default]
    Manual,
    Rollback,
}
impl Reason {
    fn name(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Rollback => "rollback",
        }
    }
    fn parse(value: &str) -> Result<Self> {
        match value {
            "manual" => Ok(Self::Manual),
            "rollback" => Ok(Self::Rollback),
            _ => anyhow::bail!("invalid revoke reason"),
        }
    }
    fn code(self) -> &'static str {
        match self {
            Self::Manual => "manual_revoke",
            Self::Rollback => "manual_rollback",
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    op: String,
    request_id: String,
    #[serde(default)]
    reason: Reason,
}
impl Command {
    pub fn id(&self) -> &str {
        &self.request_id
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.op == "revoke" && canonical_id(&self.request_id),
            "invalid revoke command"
        );
        Ok(())
    }
    fn fingerprint(&self, location: &Location) -> String {
        format!(
            "{:x}",
            Sha256::digest(json!(["revoke", location.configuration, self.reason]).to_string())
        )
    }
}
fn canonical_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
}

#[derive(Debug)]
struct Declined(&'static str);
impl std::fmt::Display for Declined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Declined {}
fn step<T>(value: Result<T>, code: &'static str) -> Result<T> {
    value.context(Declined(code))
}

struct Record {
    fingerprint: String,
    reason: String,
    writer_epoch: Option<String>,
    state: String,
    result: Option<String>,
}
fn record(store: &Store, id: &str) -> Result<Option<Record>> {
    Ok(store.db.query_row("SELECT substr(fingerprint,1,65),substr(reason,1,33),substr(writer_epoch,1,37),substr(state,1,16),substr(result,1,4097) FROM desktop_commands WHERE request_id=?1",[id],|r|Ok(Record {fingerprint:r.get(0)?,reason:r.get(1)?,writer_epoch:r.get(2)?,state:r.get(3)?,result:r.get(4)?})).optional()?)
}

/// The drain callback runs only after policy revocation commits. Current kernel
/// callers have no effect workers. Any future dispatcher must cancel its lease
/// and join/settle owned workers here before marker/checkpoint success is possible.
fn execute(
    location: &Location,
    store: &mut Store,
    command: &Command,
    held: Option<&HandoffLock>,
    drain: impl FnOnce() -> Result<()>,
) -> Result<Value> {
    command.validate()?;
    let acquired = if held.is_none() {
        Some(step(location.handoff_lock(), "handoff_unavailable")?)
    } else {
        None
    };
    let lock = held.or(acquired.as_ref()).context("missing handoff lock")?;
    lock.verify(location)?;
    let marker = step(handoff::marker(location, lock), "unsupported_handoff")?;
    let current_epoch = marker.as_ref().map(|m| m.writer_epoch.clone());
    let fingerprint = command.fingerprint(location);
    let prior = step(record(store, &command.request_id), "audit_unavailable")?;
    let record = if let Some(record) = prior {
        ensure!(
            record.fingerprint == fingerprint,
            Declined("request_conflict")
        );
        record
    } else {
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO desktop_commands(request_id,fingerprint,reason,writer_epoch,state,at) VALUES(?1,?2,?3,?4,'prepared',?5)",params![command.request_id,fingerprint,command.reason.name(),current_epoch,chrono::Utc::now().timestamp_millis()])?;
        zerus_desktop_ledger::revoke(&tx, command.reason.code())?;
        tx.commit()?;
        Record {
            fingerprint,
            reason: command.reason.name().into(),
            writer_epoch: current_epoch.clone(),
            state: "prepared".into(),
            result: None,
        }
    };
    // A replay stays bound to its original target, even after a new handoff.
    ensure!(
        record.writer_epoch == current_epoch,
        Declined("writer_epoch_changed")
    );
    ensure!(
        zerus_desktop_ledger::status(&store.db)?["revoked"] == true,
        Declined("writer_authority_changed")
    );
    if record.state == "settled" {
        ensure!(
            marker
                .as_ref()
                .is_none_or(|m| m.phase == zerus_desktop_migration::Phase::Revoked),
            Declined("writer_authority_changed")
        );
        return receipt(location, command, &record);
    }
    ensure!(record.state == "prepared", "invalid revoke state");
    step(drain(), "drain_not_settled")?;
    if let Some(marker) = marker {
        step(
            handoff::write_revoked(location, lock, &marker),
            "marker_not_durable",
        )?;
    }
    step(store.checkpoint(), "checkpoint_not_settled")?;
    let result = json!({"request_id":command.request_id,"accepted":true,"durable":true,
        "revoked":true,"checkpointed":true,"configuration":location.configuration,
        "writer_epoch":record.writer_epoch});
    let encoded = serde_json::to_string(&result)?;
    // Accepted work was checkpointed; the final receipt is itself committed
    // with synchronous=FULL. An unfinished prepared row can only reconcile
    // this same revocation; it cannot create another native delivery.
    let changed=store.db.execute("UPDATE desktop_commands SET state='settled',result=?2 WHERE request_id=?1 AND state='prepared'",params![command.request_id,encoded])?;
    ensure!(changed == 1, "revoke receipt was not saved");
    Ok(result)
}

fn response(location: &Location, store: &mut Store, command: &Command) -> Value {
    match execute(location, store, command, None, || Ok(())) {
        Ok(value) => value,
        Err(error) => {
            let accepted = record(store, &command.request_id)
                .ok()
                .flatten()
                .is_some_and(|r| r.fingerprint == command.fingerprint(location));
            json!({"request_id":command.request_id,"accepted":accepted,"durable":accepted,
                "revoked":false,"checkpointed":false,
                "code":error.downcast_ref::<Declined>().map(|e|e.0).unwrap_or("revocation_not_settled")})
        }
    }
}

fn receipt(location: &Location, command: &Command, record: &Record) -> Result<Value> {
    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Receipt {
        request_id: String,
        configuration: String,
        writer_epoch: Option<String>,
        accepted: bool,
        durable: bool,
        revoked: bool,
        checkpointed: bool,
    }
    let bytes = record.result.as_ref().context("missing revoke receipt")?;
    ensure!(bytes.len() <= 4096, "invalid revoke receipt");
    let value: Receipt = serde_json::from_str(bytes)?;
    ensure!(
        value.request_id == command.request_id
            && value.configuration == location.configuration
            && value.writer_epoch == record.writer_epoch
            && value.accepted
            && value.durable
            && value.revoked
            && value.checkpointed,
        "invalid revoke receipt identity"
    );
    Ok(serde_json::to_value(value)?)
}

/// An indexed, read-only lookup. A pending reservation remains pending until
/// the exact command is explicitly reconciled; a query never drains or writes.
fn query(location: &Location, store: &Store, id: &str) -> Result<Value> {
    let record = record(store, id)?.context(Declined("request_not_found"))?;
    let command = Command {
        op: "revoke".into(),
        request_id: id.into(),
        reason: Reason::parse(&record.reason)?,
    };
    ensure!(
        record.fingerprint == command.fingerprint(location),
        "invalid audit fingerprint"
    );
    let lock = step(location.handoff_lock(), "handoff_unavailable")?;
    let marker = step(handoff::marker(location, &lock), "unsupported_handoff")?;
    ensure!(
        record.writer_epoch == marker.as_ref().map(|m| m.writer_epoch.clone()),
        Declined("writer_epoch_changed")
    );
    ensure!(
        zerus_desktop_ledger::status(&store.db)?["revoked"] == true
            && marker
                .as_ref()
                .is_none_or(|m| m.phase == zerus_desktop_migration::Phase::Revoked),
        Declined("revocation_pending")
    );
    ensure!(record.state == "settled", Declined("revocation_pending"));
    receipt(location, &command, &record)
}

pub fn request(location: &Location, store: &mut Store, value: &Value) -> Value {
    if value["op"] == "revoke_status" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Query {
            op: String,
            request_id: String,
        }
        if let Ok(query) = serde_json::from_value::<Query>(value.clone()) {
            if query.op == "revoke_status" && canonical_id(&query.request_id) {
                return query_result(location, store, &query.request_id);
            }
        }
    } else if let Ok(command) = serde_json::from_value::<Command>(value.clone()) {
        if command.validate().is_ok() {
            return response(location, store, &command);
        }
    }
    json!({"request_id":value.get("request_id"),"accepted":false,"code":"invalid_revoke_request"})
}
fn query_result(location: &Location, store: &Store, id: &str) -> Value {
    query(location, store, id).unwrap_or_else(|error| {
        json!({"request_id":id,
        "accepted":false,"revoked":false,"checkpointed":false,
        "code":error.downcast_ref::<Declined>().map(|e|e.0).unwrap_or("audit_unavailable")})
    })
}

/// Exact CLI spelling for installers; callers may retain an explicit request
/// UUID. No stdin prompt or manager process is involved.
pub fn cli(args: &[&str]) -> Result<Command> {
    let mut request_id = None;
    let mut reason = Reason::Manual;
    let mut reason_set = false;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            "--json" if !json => {
                json = true;
                index += 1;
            }
            "--request-id" if request_id.is_none() && index + 1 < args.len() => {
                request_id = Some(args[index + 1].to_owned());
                index += 2;
            }
            "--reason" if !reason_set && index + 1 < args.len() => {
                reason = Reason::parse(args[index + 1])?;
                reason_set = true;
                index += 2;
            }
            _ => anyhow::bail!(
                "usage: hgs desktop revoke --json [--request-id UUID] [--reason rollback]"
            ),
        }
    }
    ensure!(json, "desktop revoke requires --json");
    let command = Command {
        op: "revoke".into(),
        request_id: request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        reason,
    };
    command.validate()?;
    Ok(command)
}
pub fn payload(command: &Command) -> Result<Value> {
    Ok(serde_json::to_value(command)?)
}

/// CLI-only absent-service path. The caller holds owner.lock; a running but
/// unreachable actor cannot be bypassed. No listener, manager or worker starts.
pub fn offline(location: &Location, command: &Command) -> Result<Value> {
    command.validate()?;
    let exists = |name: &str| -> Result<bool> {
        match std::fs::symlink_metadata(location.root.join(name)) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    };
    // A missing service/handoff preserves today's plain Qt rollback. Checking
    // for any leaf (including unsafe ones) never repairs or follows it. Exclude
    // an existing live owner, and repeat the state check under that lock.
    {
        let _existing_owner = location.existing_lock()?;
        if !exists("state.sqlite3")? && !exists(zerus_desktop_migration::HANDOFF_FILE)? {
            for sidecar in [
                "state.sqlite3-wal",
                "state.sqlite3-shm",
                "state.sqlite3-journal",
            ] {
                ensure!(
                    !exists(sidecar)?,
                    "desktop state is incomplete; keep Qt stopped for inspection"
                );
            }
            return Ok(
                json!({"request_id":command.request_id,"configuration":location.configuration,
                "applicable":false,"handoff":"none","revoked":true,"nothing_written":true}),
            );
        }
    }
    location.prepare()?;
    let _owner = location.lock()?;
    let epoch = uuid::Uuid::new_v4().to_string();
    let mut store = Store::open(location, &epoch)?;
    Ok(response(location, &mut store, command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_desktop_migration::{canonical, Handoff, Phase, HANDOFF_FILE};
    struct Fixture {
        _temp: tempfile::TempDir,
        location: Location,
        store: Store,
        epoch: String,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let location = Location {
                root: canonical(&temp.path().join("state/desktop")).unwrap(),
                configuration: "a".repeat(64),
            };
            location.prepare().unwrap();
            let epoch = uuid::Uuid::new_v4().to_string();
            let store = Store::open(&location, &epoch).unwrap();
            Self {
                _temp: temp,
                location,
                store,
                epoch,
            }
        }
        fn command(&self) -> Command {
            cli(&["--json", "--reason", "rollback"]).unwrap()
        }
        fn marker(&self) -> Handoff {
            let marker: Handoff = serde_json::from_value(
                json!({"version":1,"configuration":self.location.configuration,
                "source":"com.hgdev.hgs-tray","import_id":uuid::Uuid::new_v4().to_string(),
                "writer_epoch":uuid::Uuid::new_v4().to_string(),"phase":"complete",
                "import_record":{"version":1,"sha256":"b".repeat(64)},"automatic_approvals":false}),
            )
            .unwrap();
            let lock = self.location.handoff_lock().unwrap();
            handoff::write_revoked(&self.location, &lock, &marker).unwrap();
            // Fixture publishes a complete marker without authorizing any worker.
            std::fs::write(
                self.location.root.join(HANDOFF_FILE),
                serde_json::to_vec(&marker).unwrap(),
            )
            .unwrap();
            marker
        }
    }
    #[test]
    fn revokes_only_phase_and_drains_after_the_policy_fence() {
        let mut f = Fixture::new();
        let before = f.marker();
        zerus_desktop_ledger::authorize(&mut f.store.db, &before.writer_epoch, &f.epoch).unwrap();
        let command = f.command();
        let lock = f.location.handoff_lock().unwrap();
        // A live dispatcher can retain its own lock across authority and drain.
        let observer = rusqlite::Connection::open(f.location.root.join("state.sqlite3")).unwrap();
        let result = execute(&f.location, &mut f.store, &command, Some(&lock), || {
            assert_eq!(zerus_desktop_ledger::status(&observer)?["revoked"], true);
            Ok(())
        })
        .unwrap();
        assert_eq!(result["checkpointed"], true);
        let mut expected = before.clone();
        expected.phase = Phase::Revoked;
        assert_eq!(handoff::marker(&f.location, &lock).unwrap(), Some(expected));
        drop(lock);
        assert_eq!(
            query(&f.location, &f.store, &command.request_id).unwrap(),
            result
        );
        assert_eq!(response(&f.location, &mut f.store, &command), result);
        assert_eq!(f.store.effect_status()["reason"], "manual_rollback");
    }
    #[test]
    fn failed_drain_keeps_a_durable_pending_command_and_reconciles_after_reopen() {
        let mut f = Fixture::new();
        let before = f.marker();
        let command = f.command();
        assert!(
            execute(&f.location, &mut f.store, &command, None, || anyhow::bail!(
                "busy worker"
            ))
            .is_err()
        );
        assert_eq!(
            query_result(&f.location, &f.store, &command.request_id)["code"],
            "revocation_pending"
        );
        assert_eq!(
            record(&f.store, &command.request_id)
                .unwrap()
                .unwrap()
                .state,
            "prepared"
        );
        let lock = f.location.handoff_lock().unwrap();
        assert_eq!(handoff::marker(&f.location, &lock).unwrap(), Some(before));
        drop(lock);
        f.store = Store::open(&f.location, &uuid::Uuid::new_v4().to_string()).unwrap();
        assert_eq!(
            response(&f.location, &mut f.store, &command)["revoked"],
            true
        );
        let mut conflict = command.clone();
        conflict.reason = Reason::Manual;
        let result = response(&f.location, &mut f.store, &conflict);
        assert_eq!(result["code"], "request_conflict");
        assert_eq!(result["accepted"], false);
    }
    #[test]
    fn a_busy_checkpoint_cannot_be_reported_as_settled() {
        let mut f = Fixture::new();
        f.marker();
        let command = f.command();
        let reader = rusqlite::Connection::open(f.location.root.join("state.sqlite3")).unwrap();
        reader
            .execute_batch("BEGIN; SELECT * FROM service;")
            .unwrap();
        let result = response(&f.location, &mut f.store, &command);
        assert_eq!(result["code"], "checkpoint_not_settled");
        assert_eq!(result["accepted"], true);
        assert_eq!(result["durable"], true);
        assert_eq!(result["revoked"], false);
        assert_eq!(result["checkpointed"], false);
        assert_eq!(
            query_result(&f.location, &f.store, &command.request_id)["code"],
            "revocation_pending"
        );
        reader.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            response(&f.location, &mut f.store, &command)["checkpointed"],
            true
        );
    }
    #[test]
    fn exact_replay_never_revokes_a_new_handoff_or_new_authority() {
        let mut f = Fixture::new();
        f.marker();
        let command = f.command();
        assert_eq!(
            response(&f.location, &mut f.store, &command)["revoked"],
            true
        );
        let newer = f.marker();
        zerus_desktop_ledger::authorize(&mut f.store.db, &newer.writer_epoch, &f.epoch).unwrap();
        assert_eq!(
            response(&f.location, &mut f.store, &command)["code"],
            "writer_epoch_changed"
        );
        assert_eq!(
            query_result(&f.location, &f.store, &command.request_id)["code"],
            "writer_epoch_changed"
        );
        assert_eq!(f.store.effect_status()["revoked"], false);
        let next = f.command();
        assert_eq!(
            response(&f.location, &mut f.store, &next)["writer_epoch"],
            newer.writer_epoch
        );
        // Even the same marker epoch reauthorized by a future handoff needs a
        // new explicit command. A historical receipt is not current authority.
        zerus_desktop_ledger::authorize(&mut f.store.db, &newer.writer_epoch, &f.epoch).unwrap();
        assert_eq!(
            response(&f.location, &mut f.store, &next)["code"],
            "writer_authority_changed"
        );
        assert_eq!(f.store.effect_status()["revoked"], false);
    }
    #[test]
    fn malformed_marker_and_failed_audit_never_return_success() {
        let mut f = Fixture::new();
        let command = f.command();
        let path = f.location.root.join(HANDOFF_FILE);
        std::fs::write(&path, b"{future}").unwrap();
        crate::platform::make_private(&std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(
            response(&f.location, &mut f.store, &command)["code"],
            "unsupported_handoff"
        );
        assert!(record(&f.store, &command.request_id).unwrap().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"{future}");
        std::fs::remove_file(path).unwrap();
        zerus_desktop_ledger::authorize(
            &mut f.store.db,
            &uuid::Uuid::new_v4().to_string(),
            &f.epoch,
        )
        .unwrap();
        f.store.db.execute_batch("CREATE TRIGGER full_audit BEFORE INSERT ON desktop_commands BEGIN SELECT RAISE(ABORT,'audit refused'); END;").unwrap();
        let result = response(&f.location, &mut f.store, &command);
        assert_eq!(result["accepted"], false);
        assert_eq!(result["revoked"], false);
        assert_eq!(f.store.effect_status()["revoked"], false);
    }
    #[test]
    fn queries_are_indexed_non_mutating_and_receipts_are_identity_checked() {
        let mut f = Fixture::new();
        let command = f.command();
        assert_eq!(
            query_result(&f.location, &f.store, &command.request_id)["code"],
            "request_not_found"
        );
        assert!(record(&f.store, &command.request_id).unwrap().is_none());
        let result = response(&f.location, &mut f.store, &command);
        assert_eq!(result["writer_epoch"], Value::Null);
        f.store
            .db
            .execute(
                "UPDATE desktop_commands SET result=?1",
                [result.to_string().replace("true", "false")],
            )
            .unwrap();
        assert_eq!(
            query_result(&f.location, &f.store, &command.request_id)["code"],
            "audit_unavailable"
        );
        assert_eq!(
            response(&f.location, &mut f.store, &command)["revoked"],
            false
        );
    }
    #[test]
    fn wrong_scope_or_contended_handoff_lock_refuses_before_acceptance() {
        let mut f = Fixture::new();
        let command = f.command();
        let other = Fixture::new();
        let lock = other.location.handoff_lock().unwrap();
        assert!(execute(&f.location, &mut f.store, &command, Some(&lock), || Ok(())).is_err());
        let _held = f.location.handoff_lock().unwrap();
        assert_eq!(
            response(&f.location, &mut f.store, &command)["code"],
            "handoff_unavailable"
        );
        assert!(record(&f.store, &command.request_id).unwrap().is_none());
    }
}
