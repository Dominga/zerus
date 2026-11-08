use super::ledger::Child;
use super::*;
use crate::desktop::store::{Location, Store};
use rusqlite::TransactionBehavior;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use zerus_client_core::desktop::session_resume::{Intent, Settled, Step, Verified};
use zerus_client_core::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};

const SESSION: &str = "claude/private-resume";
const RUN: &str = "run-1";
const RESUMED: &str = "00000000-0000-4000-8000-0000000000b1";
const CONVERSATION: &str = "conversation-1";
const MACHINE: &str = "22222222-2222-4222-8222-222222222222";

/// A saved version 3 resume-then-send checkpoint with the exact resume stdin
/// and send template the client builds for it.
fn original(configuration: &str, epoch: &str) -> Verified {
    original_of(configuration, epoch, SESSION)
}
/// [`original`] for the session `session`.
fn original_of(configuration: &str, epoch: &str, session: &str) -> Verified {
    let text = "after resume";
    let checkpoint = Checkpoint::reserve_session_resume(checkpoint::Intent {
        kind: checkpoint::Kind::ResumeToSend,
        draft_revision: 3,
        message_sha256: checkpoint::message_sha256(text, Vec::<(&str, &str, &str, &[u8])>::new()),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
        question: None,
        start: Some(checkpoint::Start::Resume),
        retry_of: None,
        consent: true,
    })
    .unwrap();
    let resume = json!({"request_id": checkpoint.request_id, "action": "resume",
        "expected_run_id": RUN, "expected_conversation_id": CONVERSATION});
    let stdin = json!({"request_id": checkpoint.send_request_id, "text": text, "attachments": [],
        "expected_run_id": RUN, "expected_conversation_id": CONVERSATION});
    Intent {
        scope: wire::Scope {
            configuration: configuration.into(),
            service_epoch: epoch.into(),
        },
        session: session.into(),
        request_id: checkpoint.request_id.clone(),
        send_request_id: checkpoint.send_request_id.clone().unwrap(),
        checkpoint: serde_json::to_string(&checkpoint).unwrap(),
        resume: resume.to_string(),
        original: stdin.to_string(),
    }
    .verify()
    .unwrap()
}
/// The resume's own exact outcome for `plan`.
fn resumed(plan: &wire::Plan) -> wire::Resumed {
    wire::Resumed {
        request_id: plan.resume_request_id.clone(),
        status: wire::COMPLETED.into(),
        result_target: wire::ResultTarget {
            name: plan.session.clone(),
            run_id: RESUMED.into(),
            conversation_id: plan.conversation_id.clone(),
        },
    }
}
fn mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}
/// Writes `body` atomically as the CLI's private receipt `<request>.json` in
/// `folder` under the CLI state root `state`.
fn write_receipt(state: &Path, folder: &str, request: &str, body: &[u8]) -> PathBuf {
    let folder = state.join(folder);
    if !folder.exists() {
        std::fs::create_dir(&folder).unwrap();
    }
    mode(&folder, 0o700);
    let path = folder.join(format!("{request}.json"));
    let temp = folder.join(format!(".{request}.tmp"));
    std::fs::write(&temp, body).unwrap();
    mode(&temp, 0o600);
    std::fs::rename(&temp, &path).unwrap();
    path
}
/// The exact action receipt `hgs session-action` writes for the resume
/// stdin `stdin` of `plan` with `status` (completed: resumed to
/// [`RESUMED`]).
fn action_receipt(state: &Path, plan: &wire::Plan, stdin: &[u8], status: &str) -> PathBuf {
    let value = crate::state::lifecycle_fixture_receipt(
        &plan.session,
        stdin,
        status,
        (status == "completed").then_some(RESUMED),
    );
    write_receipt(
        state,
        receipt::RESUME_FOLDER,
        &plan.resume_request_id,
        value.to_string().as_bytes(),
    )
}
/// The CLI's own exact input receipt of `plan`'s send with the stdin
/// `stdin`, bound to [`RESUMED`].
fn sent_receipt(state: &Path, plan: &wire::Plan, stdin: &[u8]) {
    let digest = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(stdin));
    write_receipt(
        state,
        receipt::INPUT_FOLDER,
        &plan.send_request_id,
        json!({"request_id": plan.send_request_id, "name": plan.session, "run_id": RESUMED,
            "conversation_id": plan.conversation_id, "digest": digest,
            "status": "submitted", "transport": "terminal"})
        .to_string()
        .as_bytes(),
    );
}
/// The process start identity the CLI records for `pid`.
fn started(pid: u32) -> String {
    crate::platform::process_start_time(pid)
        .or_else(|| crate::platform::process_start_fallback(pid))
        .unwrap_or_default()
}
/// The CLI's own session record of `name` under the state root `state`,
/// with the extra `fields`.
fn session_record(state: &Path, name: &str, agent: &str, run: &str, fields: Value) {
    use sha2::Digest;
    let file = state.join(format!("{:x}.json", sha2::Sha256::digest(name.as_bytes())));
    let temp = state.join(".record.tmp");
    let mut record = json!({"name": name, "agent": agent, "run_id": run,
        "conversation_id": CONVERSATION});
    for (key, value) in fields.as_object().unwrap() {
        record[key] = value.clone();
    }
    std::fs::write(&temp, record.to_string()).unwrap();
    mode(&temp, 0o600);
    std::fs::rename(&temp, file).unwrap();
}
/// The resumed run's record of a confirmed, idle, live Claude: ready.
fn resumed_record(state: &Path, ready: bool) {
    resumed_record_of(state, SESSION, ready)
}
/// [`resumed_record`] for the session `session`.
fn resumed_record_of(state: &Path, session: &str, ready: bool) {
    let pid = std::process::id();
    let start = started(pid);
    let (activity, phase) = if ready {
        ("idle", "idle")
    } else {
        ("busy", "working")
    };
    session_record(
        state,
        session,
        "claude",
        RESUMED,
        json!({"expected_id": "", "activity": activity, "phase": phase, "error": "",
            "pid": pid, "process_start": start}),
    );
}
fn fixture() -> (tempfile::TempDir, Location, Store, String) {
    let temp = tempfile::tempdir().unwrap();
    let location = Location {
        root: temp.path().canonicalize().unwrap().join("state/desktop"),
        configuration: "a".repeat(64),
    };
    location.prepare().unwrap();
    let epoch = uuid::Uuid::new_v4().to_string();
    let store = Store::open(&location, &epoch).unwrap();
    (temp, location, store, epoch)
}
fn tables(db: &rusqlite::Connection) -> i64 {
    db.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn exact_previous_store_upgrade_adds_only_the_independent_session_resume_leaf() {
    assert_eq!(
        SCHEMA - 1,
        crate::desktop::delivery::CATEGORIES_SCHEMA,
        "rebase this test onto the actual merge-order predecessor"
    );
    let (_temp, location, store, _) = fixture();
    remove_fixture(&store.db);
    store
        .db
        .pragma_update(None, "user_version", SCHEMA - 1)
        .unwrap();
    let before = tables(&store.db);
    drop(store);
    let store = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(
        store
            .db
            .query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        crate::desktop::store::SCHEMA
    );
    assert_eq!(tables(&store.db), before + 2);
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    assert_eq!(
        store.operation_status()["kinds"]["session_resume"]["undrained"],
        0
    );
}

fn admit(store: &mut Store, verified: &Verified, writer: &str) -> ledger::Row {
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::admit(&tx, verified, writer, 1).unwrap();
    tx.commit().unwrap();
    row
}

#[test]
fn admission_is_immutable_and_the_same_operation_returns_only_its_original_receipt() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let writer = uuid::Uuid::new_v4().to_string();
    let row = admit(&mut store, &verified, &writer);
    assert_eq!(row.receipt.resume.step, Step::Queued);
    assert_eq!(row.receipt.send.step, Step::Queued);
    assert!(row.resume.is_none() && row.send.is_none() && row.bound().is_none());
    // The same operation again: its original receipt, nothing new.
    let again = admit(&mut store, &verified, &uuid::Uuid::new_v4().to_string());
    assert_eq!(again.receipt, row.receipt);
    assert_eq!(again.writer_epoch, writer);
    // Another send under the same resume conflicts.
    let mut other = verified.intent.clone();
    let mut stdin: Value = serde_json::from_str(&other.original).unwrap();
    stdin["text"] = json!("something else");
    other.original = stdin.to_string();
    assert!(other.verify().is_err());
    // The original columns are immutable and its receipts are retained.
    for sql in [
        "UPDATE desktop_session_resumes SET source='{}'",
        "UPDATE desktop_session_resumes SET resume='{}'",
        "UPDATE desktop_session_resumes SET checkpoint='{}'",
        "UPDATE desktop_session_resumes SET plan='{}'",
        "UPDATE desktop_session_resumes SET writer_epoch='x'",
        "UPDATE desktop_session_resumes SET send_request_id='x'",
        "DELETE FROM desktop_session_resumes",
    ] {
        assert!(store.db.execute(sql, []).is_err(), "{sql}");
    }
    assert_eq!(ledger::undrained(&store.db).unwrap(), 1);
}

#[test]
fn retirement_proves_only_unreserved_children_never_started() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let row = admit(&mut store, &verified, &uuid::Uuid::new_v4().to_string());
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::retire(&tx, &row).unwrap();
    tx.commit().unwrap();
    let row = ledger::load(&store.db, row.id()).unwrap().unwrap();
    assert!(row.receipt.drained);
    assert_eq!(row.receipt.code, ledger::INTERRUPTED);
    assert_eq!(
        (row.receipt.resume.step, row.receipt.send.step),
        (Step::NotStarted, Step::NotStarted)
    );
    assert_eq!(
        row.receipt.result(&verified.plan),
        Some(Settled::NotStarted)
    );
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    // Nothing follows the drain.
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::retire(&tx, &row).is_err());
}

fn running(
    store: &mut Store,
    location: &Location,
    verified: &Verified,
    epoch: &str,
) -> (
    zerus_desktop_ledger::operations::custody::Record,
    std::sync::Arc<zerus_client_core::run::custody::InheritedLock>,
    zerus_desktop_ledger::operations::Ticket,
) {
    let writer = uuid::Uuid::new_v4().to_string();
    zerus_desktop_ledger::authorize(&mut store.db, &writer, epoch).unwrap();
    let row = admit(store, verified, &writer);
    let ticket = zerus_desktop_ledger::operations::Ticket {
        operation_id: verified.binding.operation_id().unwrap(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        revision: row.receipt.revision + 1,
        admission_id: verified.plan.resume_request_id.clone(),
        binding_sha256: verified.binding.fingerprint().unwrap(),
        writer_epoch: writer,
        service_epoch: epoch.into(),
        lease: uuid::Uuid::new_v4().to_string(),
    };
    // The send can never be reserved before the resume, nor while it runs.
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let (prepared, held) = crate::desktop::custody::prepare(location, &ticket).unwrap();
    let record = zerus_desktop_ledger::operations::custody::Record {
        prepared,
        helper: None,
    };
    assert!(ledger::reserve(&tx, &row, Child::Send, &ticket, &record).is_err());
    ledger::reserve(&tx, &row, Child::Resume, &ticket, &record).unwrap();
    tx.commit().unwrap();
    let row = ledger::load(&store.db, &verified.plan.resume_request_id)
        .unwrap()
        .unwrap();
    assert_eq!(row.receipt.resume.step, Step::Running);
    assert_eq!(row.draining(), Some(Child::Resume));
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::reserve(&tx, &row, Child::Send, &ticket, &record).is_err());
    drop(tx);
    // A reservation is immutable once made.
    assert!(store
        .db
        .execute("UPDATE desktop_session_resumes SET resume_custody='{}'", [])
        .is_err());
    (record, held, ticket)
}

/// Observes `record` until its release, within a bound.
fn released(
    location: &Location,
    record: &zerus_desktop_ledger::operations::custody::Record,
) -> crate::desktop::custody::Observation {
    use crate::desktop::custody::{probe, Observation};
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let observation = probe(location, record);
        if matches!(observation, Observation::Released { .. }) || std::time::Instant::now() >= until
        {
            return observation;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn state_root(parent: &Path) -> PathBuf {
    let state = parent.join("state");
    std::fs::create_dir(&state).unwrap();
    mode(&state, 0o700);
    state
}
/// An absence: a read of a held store that has no receipt of the resume.
fn missing(plan: &wire::Plan, original: &str) -> receipt::Observed {
    let temp = tempfile::tempdir().unwrap();
    let state = state_root(&temp.path().canonicalize().unwrap());
    let folder = state.join(receipt::RESUME_FOLDER);
    std::fs::create_dir(&folder).unwrap();
    mode(&folder, 0o700);
    let roots = receipt::Roots::open(&state, receipt::RESUME_FOLDER).unwrap();
    let observed = receipt::resume(&roots, plan, original.as_bytes(), &|| ()).0;
    assert!(
        matches!(observed, receipt::Observed::Missing(_)),
        "{observed:?}"
    );
    observed
}

/// Only the drain settles the resume, from its own exact receipt: a
/// recorded resume fixes the bound send and waits for it; an exact refusal
/// or anything that is not the resume's own record ends the send unstarted;
/// an absence proves nothing ran only with the runner's no-spawn result.
#[test]
fn only_the_released_custody_settles_the_resume_and_only_its_exact_cli_record_decides() {
    use receipt::{Observed, MISSING};
    #[derive(Clone, Copy, Debug)]
    enum Ending {
        Exited,
        NotStarted,
    }
    let refused = |_: &wire::Plan, _: &str| Observed::Refused;
    let exact = |plan: &wire::Plan, _: &str| Observed::Resumed(resumed(plan));
    let other = |plan: &wire::Plan, _: &str| {
        let mut outcome = resumed(plan);
        outcome.result_target.conversation_id = "other".into();
        Observed::Resumed(outcome)
    };
    let uncertain = |_: &wire::Plan, _: &str| Observed::Unknown(wire::RECEIPT_UNCERTAIN);
    type Read = fn(&wire::Plan, &str) -> Observed;
    use Step::*;
    let cases: [(Ending, Read, Step, &str, Option<Settled>); 8] = [
        (Ending::Exited, exact, Recorded, "", None),
        // An exact record wins over the runner's own no-spawn result.
        (Ending::NotStarted, exact, Recorded, "", None),
        (
            Ending::Exited,
            refused,
            Failed,
            wire::REFUSED,
            Some(Settled::NotSent { resume: Failed }),
        ),
        (
            Ending::NotStarted,
            refused,
            Failed,
            wire::REFUSED,
            Some(Settled::NotSent { resume: Failed }),
        ),
        (
            Ending::NotStarted,
            missing,
            NotStarted,
            wire::NOT_STARTED,
            Some(Settled::NotStarted),
        ),
        // After a possible spawn, an absence proves nothing.
        (
            Ending::Exited,
            missing,
            Unknown,
            MISSING,
            Some(Settled::NotSent { resume: Unknown }),
        ),
        (
            Ending::Exited,
            uncertain,
            Unknown,
            wire::RECEIPT_UNCERTAIN,
            Some(Settled::NotSent { resume: Unknown }),
        ),
        (
            Ending::Exited,
            other,
            Unknown,
            wire::RECEIPT_MISMATCH,
            Some(Settled::NotSent { resume: Unknown }),
        ),
    ];
    for (ending, read, step, code, settled) in cases {
        let (_temp, location, mut store, epoch) = fixture();
        let verified = original(&location.configuration, &epoch);
        let (record, held, _) = running(&mut store, &location, &verified, &epoch);
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let row = ledger::load(&tx, &verified.plan.resume_request_id)
            .unwrap()
            .unwrap();
        match ending {
            Ending::Exited => ledger::ended(&tx, &row, ledger::AWAITING_DRAIN).unwrap(),
            Ending::NotStarted => ledger::not_started(&tx, &row).unwrap(),
        }
        tx.commit().unwrap();
        // Held custody never settles.
        assert!(matches!(
            crate::desktop::custody::probe(&location, &record),
            crate::desktop::custody::Observation::Held
        ));
        drop(held);
        let observation = released(&location, &record);
        let observed = read(&verified.plan, &verified.intent.resume);
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let row = ledger::load(&tx, &verified.plan.resume_request_id)
            .unwrap()
            .unwrap();
        let next = ledger::drained(
            tx,
            row,
            observation,
            &observed,
            receipt::Evidence::default(),
        )
        .unwrap();
        let label = format!("{ending:?} {step:?}");
        assert_eq!(next.resume.step, step, "{label}");
        assert_eq!(next.resume.code, code, "{label}");
        let row = ledger::load(&store.db, &verified.plan.resume_request_id)
            .unwrap()
            .unwrap();
        match settled {
            // A recorded resume fixes the bound send and leaves the send
            // queued for the same live operation's wait.
            None => {
                assert!(!next.drained, "{label}");
                assert_eq!(next.send.step, Queued, "{label}");
                let (_, stdin, digest) = row.bound().unwrap();
                assert_eq!(next.bound_sha256.as_deref(), Some(digest.as_str()));
                let stdin: Value = serde_json::from_str(&stdin).unwrap();
                assert_eq!(stdin["expected_run_id"], RESUMED);
                assert_eq!(ledger::undrained(&store.db).unwrap(), 1);
            }
            // Anything else ends the send, which was never reserved.
            Some(settled) => {
                assert!(next.drained, "{label}");
                assert_eq!(next.send.step, NotStarted, "{label}");
                assert!(row.send.is_none(), "{label}");
                assert_eq!(next.result(&verified.plan), Some(settled), "{label}");
                assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
            }
        }
    }
}

/// A recorded resume's bound send is fixed: a stored receipt whose digest
/// is not the template bound to that resume never loads.
#[test]
fn a_changed_bound_send_never_loads() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let (record, held, _) = running(&mut store, &location, &verified, &epoch);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::load(&tx, &verified.plan.resume_request_id)
        .unwrap()
        .unwrap();
    ledger::ended(&tx, &row, ledger::AWAITING_DRAIN).unwrap();
    tx.commit().unwrap();
    drop(held);
    let observation = released(&location, &record);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::load(&tx, &verified.plan.resume_request_id)
        .unwrap()
        .unwrap();
    let observed = receipt::Observed::Resumed(resumed(&verified.plan));
    ledger::drained(
        tx,
        row,
        observation,
        &observed,
        receipt::Evidence::default(),
    )
    .unwrap();
    let stored: String = store
        .db
        .query_row("SELECT receipt FROM desktop_session_resumes", [], |r| {
            r.get(0)
        })
        .unwrap();
    let mut forged: Value = serde_json::from_str(&stored).unwrap();
    forged["bound_sha256"] = json!("f".repeat(64));
    store
        .db
        .execute(
            "UPDATE desktop_session_resumes SET receipt=?1",
            [forged.to_string()],
        )
        .unwrap();
    assert!(ledger::load(&store.db, &verified.plan.resume_request_id).is_err());
}

/// Restart retires a reserved resume for its read-only drain; store
/// recovery never reads an unheld receipt, and nothing follows it.
#[test]
fn store_recovery_never_reads_an_unheld_receipt_and_never_continues() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let (record, held, _) = running(&mut store, &location, &verified, &epoch);
    // A restart: the queued send can never start; the running resume waits
    // for its custody drain.
    ledger::startup(&mut store.db).unwrap();
    let row = ledger::load(&store.db, &verified.plan.resume_request_id)
        .unwrap()
        .unwrap();
    assert_eq!(row.receipt.resume.step, Step::Running);
    assert_eq!(row.receipt.send.step, Step::NotStarted);
    assert!(!row.receipt.drained);
    drop(held);
    let observation = released(&location, &record);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::load(&tx, &verified.plan.resume_request_id)
        .unwrap()
        .unwrap();
    let next = ledger::unheld(tx, row, observation).unwrap();
    assert!(next.drained);
    assert_eq!(
        (next.resume.step, next.resume.code.as_str()),
        (Step::Unknown, receipt::UNHELD)
    );
    assert_eq!(
        next.result(&verified.plan),
        Some(Settled::NotSent {
            resume: Step::Unknown
        })
    );
}

fn observe(state: &Path, verified: &Verified) -> receipt::Observed {
    let roots = receipt::Roots::open(state, receipt::RESUME_FOLDER).unwrap();
    receipt::resume(
        &roots,
        &verified.plan,
        verified.intent.resume.as_bytes(),
        &|| (),
    )
    .0
}

/// The resume's receipt is read from the held store by the CLI's own
/// observation rule: only the exact `completed` record resumes, only the
/// exact `failed` one refuses, a `claimed` one is uncertain, and another
/// request's record under this name is never this original's.
#[test]
fn the_resume_observation_reads_only_the_exact_private_action_receipt() {
    use receipt::Observed;
    let temp = tempfile::tempdir().unwrap();
    let state = state_root(&temp.path().canonicalize().unwrap());
    let verified = original(&"a".repeat(64), &uuid::Uuid::new_v4().to_string());
    // No folder yet, then an empty one: a read of the held store finds none.
    assert!(matches!(observe(&state, &verified), Observed::Missing(_)));
    action_receipt(
        &state,
        &verified.plan,
        verified.intent.resume.as_bytes(),
        "completed",
    );
    assert_eq!(
        observe(&state, &verified),
        Observed::Resumed(resumed(&verified.plan))
    );
    action_receipt(
        &state,
        &verified.plan,
        verified.intent.resume.as_bytes(),
        "failed",
    );
    assert_eq!(observe(&state, &verified), Observed::Refused);
    for status in ["claimed", "uncertain"] {
        action_receipt(
            &state,
            &verified.plan,
            verified.intent.resume.as_bytes(),
            status,
        );
        assert_eq!(
            observe(&state, &verified),
            Observed::Unknown(wire::RECEIPT_UNCERTAIN),
            "{status}"
        );
    }
    // Another request's record written under this request's name, by
    // another session's action: never this original's.
    let other = original(&"a".repeat(64), &uuid::Uuid::new_v4().to_string());
    let foreign = crate::state::lifecycle_fixture_receipt(
        SESSION,
        other.intent.resume.as_bytes(),
        "failed",
        None,
    );
    let path = state
        .join(receipt::RESUME_FOLDER)
        .join(format!("{}.json", verified.plan.resume_request_id));
    std::fs::write(&path, foreign.to_string()).unwrap();
    assert_eq!(
        observe(&state, &verified),
        Observed::Unknown(wire::RECEIPT_INVALID)
    );
    // A group-readable receipt is not the CLI's private record.
    action_receipt(
        &state,
        &verified.plan,
        verified.intent.resume.as_bytes(),
        "completed",
    );
    mode(&path, 0o644);
    assert_eq!(
        observe(&state, &verified),
        Observed::Unknown(receipt::UNAVAILABLE)
    );
}

/// The readiness rule: only the resumed run's own record of a confirmed,
/// idle, live agent lets the send start; until the supervisor writes it,
/// and while the agent starts or works, the wait reads again; a failed or
/// changed session ends it.
#[test]
fn only_the_resumed_runs_ready_record_lets_the_send_start() {
    use ready::{Readiness, READY_ENDED, ROOT_CHANGED};
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().canonicalize().unwrap();
    let state = state_root(&parent);
    let verified = original(&"a".repeat(64), &uuid::Uuid::new_v4().to_string());
    let plan = &verified.plan;
    let read = |state: &Path| {
        let roots = receipt::Roots::open(state, receipt::RESUME_FOLDER).unwrap();
        ready::read(&roots, plan, RESUMED)
    };
    // The original run's record still: not written yet.
    session_record(&state, SESSION, "claude", RUN, json!({}));
    assert_eq!(read(&state), Readiness::Waiting);
    resumed_record(&state, false);
    assert_eq!(read(&state), Readiness::Waiting);
    resumed_record(&state, true);
    assert_eq!(read(&state), Readiness::Ready);
    // Not yet confirmed by SessionStart.
    let pid = std::process::id();
    let start = started(pid);
    session_record(
        &state,
        SESSION,
        "claude",
        RESUMED,
        json!({"expected_id": CONVERSATION, "activity": "idle", "phase": "idle", "error": "",
            "pid": pid, "process_start": start}),
    );
    assert_eq!(read(&state), Readiness::Waiting);
    // The supervisor failed.
    session_record(
        &state,
        SESSION,
        "claude",
        RESUMED,
        json!({"expected_id": "", "activity": "idle", "phase": "idle",
            "error": "saved binding changed before agent launch", "pid": pid,
            "process_start": start}),
    );
    assert_eq!(read(&state), Readiness::Ended(READY_ENDED));
    // A replaced root ends the wait.
    resumed_record(&state, true);
    let roots = receipt::Roots::open(&state, receipt::RESUME_FOLDER).unwrap();
    std::fs::rename(&state, parent.join("moved")).unwrap();
    std::fs::create_dir(&state).unwrap();
    mode(&state, 0o700);
    assert_eq!(
        ready::read(&roots, plan, RESUMED),
        Readiness::Ended(ROOT_CHANGED)
    );
}

#[test]
fn the_lookup_reads_only_its_exact_operation() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let row = admit(&mut store, &verified, &uuid::Uuid::new_v4().to_string());
    let scope = verified.intent.scope.clone();
    let asked = wire::Query::for_plan(scope.clone(), &verified.plan).unwrap();
    let value = query(
        &store,
        &json!({"op": "session_resume_result", "query": asked}),
    );
    let lookup: wire::Lookup = serde_json::from_value(value).unwrap();
    assert!(lookup.matches(&asked) && lookup.found);
    assert_eq!(lookup.receipt.unwrap(), row.receipt);
    // Another plan under this request, another epoch or another op answers
    // nothing.
    let unavailable =
        json!({"accepted":false,"read_only":true,"code":"session_resume_result_unavailable"});
    let mut other = asked.clone();
    other.plan_sha256 = "f".repeat(64);
    assert_eq!(
        query(
            &store,
            &json!({"op": "session_resume_result", "query": other})
        ),
        unavailable
    );
    let mut epoch_changed = asked.clone();
    epoch_changed.scope.service_epoch = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        query(
            &store,
            &json!({"op": "session_resume_result", "query": epoch_changed})
        ),
        unavailable
    );
    assert_eq!(
        query(
            &store,
            &json!({"op": "compact_to_send_result", "query": asked})
        ),
        unavailable
    );
}

mod actor {
    use super::super::child::Ended;
    use super::*;
    use crate::{config::Config, desktop::ui_authority};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};
    use zerus_client_core::run::custody::InheritedLock;

    type Execute = dyn Fn(
            &Path,
            &wire::Plan,
            &[String],
            &[u8],
            &Arc<InheritedLock>,
            &Arc<dyn Send + Sync>,
        ) -> Ended
        + Send
        + Sync;

    struct Fixture {
        _temp: tempfile::TempDir,
        store: Store,
        runtime: runtime::Runtime,
        exclusion: Arc<crate::desktop::store::HandoffLock>,
        location: Location,
        epoch: String,
        state: PathBuf,
    }
    impl Fixture {
        fn with(execute: Arc<Execute>) -> Self {
            Self::build(Some(execute))
        }
        /// No executor: the original path runs through the pinned image.
        #[cfg(target_os = "linux")]
        fn unexecuted() -> Self {
            Self::build(None)
        }
        fn build(execute: Option<Arc<Execute>>) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let config = Config {
                home: root.to_str().unwrap().into(),
                dir: root.join("config"),
                state: root.join("state"),
                selves: vec![],
                peers: vec![],
                base_peers: vec![],
                peers_from_environment: false,
                machines: Default::default(),
                tab: false,
                tab_colors: String::new(),
            };
            std::fs::create_dir(&config.dir).unwrap();
            let location = Location::new(&config).unwrap();
            location.prepare().unwrap();
            assert_eq!(location.root.parent().unwrap(), config.state);
            // The stopped session at the original run, before its resume.
            session_record(&config.state, SESSION, "claude", RUN, json!({}));
            let epoch = uuid::Uuid::new_v4().to_string();
            let mut store = Store::open(&location, &epoch).unwrap();
            zerus_desktop_ledger::authorize(
                &mut store.db,
                &uuid::Uuid::new_v4().to_string(),
                &epoch,
            )
            .unwrap();
            let owner = Arc::new(location.lock().unwrap());
            let exclusion = Arc::new(location.handoff_lock().unwrap());
            let mut runtime = runtime::Runtime::new(&config, &epoch, owner).unwrap();
            let state = config.state.clone();
            match execute {
                Some(execute) => runtime.fixture_executor(
                    MACHINE,
                    Arc::new(move |plan, args, input, lock, resources| {
                        execute(&state, plan, args, input, lock, resources)
                    }),
                ),
                None => runtime.fixture_machine(MACHINE),
            }
            runtime.fixture_mac_fence(false);
            runtime.poll(&mut store, Some(exclusion.clone()));
            Self {
                _temp: temp,
                store,
                runtime,
                exclusion,
                location,
                epoch,
                state: config.state,
            }
        }
        fn original(&self) -> Verified {
            super::original(&self.location.configuration, &self.epoch)
        }
        fn admit(&mut self, original: &Verified) -> wire::Admission {
            let request = json!(
                zerus_client_core::desktop::ui_requests::Request::SessionResume {
                    intent: Box::new(original.intent.clone()),
                }
            );
            serde_json::from_value(self.runtime.frontend(
                &mut self.store,
                ui_authority::Validated::fixture(&request),
                Some(self.exclusion.clone()),
            ))
            .unwrap()
        }
        fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
            let until = Instant::now() + Duration::from_secs(8);
            while !done(self) {
                assert!(
                    Instant::now() < until,
                    "timed out: {what}, {}",
                    self.runtime.status()
                );
                self.runtime
                    .poll(&mut self.store, Some(self.exclusion.clone()));
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        fn row(&self, original: &Verified) -> ledger::Row {
            ledger::load(&self.store.db, &original.plan.resume_request_id)
                .unwrap()
                .unwrap()
        }
        fn revoke(&mut self) {
            self.store
                .db
                .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
                .unwrap();
        }
    }

    /// Counts each child the executor ran, by its command.
    #[derive(Default)]
    struct Ran {
        resumes: AtomicUsize,
        sends: AtomicUsize,
    }
    impl Ran {
        fn counts(&self) -> (usize, usize) {
            (
                self.resumes.load(Ordering::SeqCst),
                self.sends.load(Ordering::SeqCst),
            )
        }
    }
    /// How the executor's resume ends.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Resume {
        /// `completed`; the resumed record is ready (or busy, `false`).
        Completed {
            ready: bool,
        },
        Failed,
        Uncertain,
        /// No receipt at all.
        Silent,
    }
    /// An executor whose resume writes the CLI's own action receipt and the
    /// resumed record as `resume` says, and whose send writes its exact
    /// input receipt.
    fn executor(ran: Arc<Ran>, resume: Resume) -> Arc<Execute> {
        Arc::new(move |state, plan, args, input, _, _| {
            let digest = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(input));
            match args[0].as_str() {
                "session-action" => {
                    ran.resumes.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(args, ["session-action", &plan.session, "--json"]);
                    assert_eq!(digest, plan.resume_sha256);
                    match resume {
                        Resume::Completed { ready } => {
                            action_receipt(state, plan, input, "completed");
                            resumed_record_of(state, &plan.session, ready);
                        }
                        Resume::Failed => {
                            action_receipt(state, plan, input, "failed");
                        }
                        Resume::Uncertain => {
                            action_receipt(state, plan, input, "uncertain");
                        }
                        Resume::Silent => {}
                    }
                }
                "send" => {
                    ran.sends.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(args, ["send", &plan.session, "--json"]);
                    let stdin: Value = serde_json::from_slice(input).unwrap();
                    assert_eq!(stdin["expected_run_id"], RESUMED);
                    assert_eq!(stdin["expected_conversation_id"], CONVERSATION);
                    assert_eq!(stdin["request_id"], plan.send_request_id.as_str());
                    assert!(stdin.get("expected_compaction_id").is_none());
                    sent_receipt(state, plan, input);
                }
                other => panic!("unexpected child {other}"),
            }
            Ended::Exited { code: 0 }
        })
    }

    #[test]
    fn a_recorded_resume_sends_once_bound_to_the_resumed_run_and_settles_from_both_receipts() {
        let ran = Arc::new(Ran::default());
        let mut f = Fixture::with(executor(ran.clone(), Resume::Completed { ready: true }));
        let original = f.original();
        let admission = f.admit(&original);
        assert!(admission.accepted && admission.durable && admission.matches(&original.intent));
        f.until("settled", |f| f.row(&original).receipt.drained);
        let row = f.row(&original);
        assert_eq!(row.receipt.resume.step, Step::Recorded);
        assert_eq!(row.receipt.send.step, Step::Recorded);
        assert!(row.resume.is_some() && row.send.is_some());
        // The send's outcome is exactly the template bound to the resume.
        let (_, _, digest) = row.bound().unwrap();
        let sent = row.receipt.send.outcome.clone().unwrap();
        assert_eq!(sent.input_sha256, digest);
        assert_eq!(
            row.receipt.result(&original.plan),
            Some(Settled::Sent(sent))
        );
        assert!(row.receipt.matches(&original.intent));
        // The same operation admitted again returns its settled receipt.
        assert_eq!(f.admit(&original).receipt.unwrap(), row.receipt);
        f.runtime.request_stop();
        f.until("stopped", |f| f.runtime.drained());
        assert_eq!(ran.counts(), (1, 1), "each child once");
    }

    /// A refused, uncertain or silent resume never sends: without the
    /// resume's own exact receipt there is no resumed run to bind.
    #[test]
    fn a_resume_that_is_not_recorded_never_sends() {
        for (resume, step, code) in [
            (Resume::Failed, Step::Failed, wire::REFUSED),
            (Resume::Uncertain, Step::Unknown, wire::RECEIPT_UNCERTAIN),
            (Resume::Silent, Step::Unknown, receipt::MISSING),
        ] {
            let ran = Arc::new(Ran::default());
            let mut f = Fixture::with(executor(ran.clone(), resume));
            let original = f.original();
            f.admit(&original);
            f.until("settled", |f| f.row(&original).receipt.drained);
            let row = f.row(&original);
            assert_eq!(
                (row.receipt.resume.step, row.receipt.resume.code.as_str()),
                (step, code),
                "{resume:?}"
            );
            assert_eq!(row.receipt.send.step, Step::NotStarted, "{resume:?}");
            assert!(row.send.is_none(), "{resume:?}: never reserved");
            assert_eq!(
                row.receipt.result(&original.plan),
                Some(Settled::NotSent { resume: step }),
                "{resume:?}"
            );
            assert_eq!(ran.counts(), (1, 0), "{resume:?}");
        }
    }

    /// The wait ends without a send when the resumed agent never becomes
    /// ready within the budget, when its session ends, when the operation
    /// is revoked, or at the operation's deadline. The send was never
    /// reserved, so the ledger proves it never started.
    #[test]
    fn the_wait_ends_without_a_send_for_its_closed_reason() {
        type Case = (&'static str, bool, Duration, &'static str);
        let cases: [Case; 4] = [
            ("busy", false, Duration::from_secs(10), ledger::WAIT_EXPIRED),
            ("failed", true, Duration::from_secs(10), ready::READY_ENDED),
            ("revoked", false, Duration::from_secs(10), ledger::VETOED),
            (
                "chain",
                false,
                Duration::from_millis(150),
                ledger::CHAIN_EXPIRED,
            ),
        ];
        for (name, ready_record, chain, code) in cases {
            let ran = Arc::new(Ran::default());
            let inner = executor(
                ran.clone(),
                Resume::Completed {
                    ready: ready_record,
                },
            );
            let mut f = Fixture::with(Arc::new(
                move |state, plan, args, input, lock, resources| {
                    let ended = inner(state, plan, args, input, lock, resources);
                    if name == "failed" {
                        let pid = std::process::id();
                        session_record(
                            state,
                            SESSION,
                            "claude",
                            RESUMED,
                            json!({"expected_id": "", "activity": "idle", "phase": "idle",
                            "error": "saved binding changed before agent launch",
                            "pid": pid, "process_start": started(pid)}),
                        );
                    }
                    ended
                },
            ));
            let wait = if name == "busy" {
                Duration::from_millis(80)
            } else {
                Duration::from_secs(5)
            };
            f.runtime
                .fixture_wait(wait, Duration::from_millis(5), chain);
            let original = f.original();
            f.admit(&original);
            f.until(name, |f| {
                let row = f.row(&original);
                row.receipt.resume.step == Step::Recorded || row.receipt.drained
            });
            if name == "revoked" {
                f.revoke();
            }
            f.until(name, |f| f.row(&original).receipt.drained);
            let receipt = f.row(&original).receipt;
            assert_eq!(receipt.resume.step, Step::Recorded, "{name}");
            assert_eq!(
                (receipt.send.step, receipt.code.as_str()),
                (Step::NotStarted, code),
                "{name}"
            );
            assert_eq!(
                receipt.result(&original.plan),
                Some(Settled::NotSent {
                    resume: Step::Recorded
                }),
                "{name}"
            );
            assert!(f.row(&original).send.is_none(), "{name}: never reserved");
            assert_eq!(ran.counts(), (1, 0), "{name}");
        }
    }

    /// The send starts only once the resumed run's record is ready: the
    /// supervisor's record appearing later, or the agent becoming idle
    /// later, still sends within the same live operation.
    #[test]
    fn the_send_waits_for_the_resumed_agent_to_be_ready() {
        let ran = Arc::new(Ran::default());
        let mut f = Fixture::with(executor(ran.clone(), Resume::Completed { ready: false }));
        f.runtime.fixture_wait(
            Duration::from_secs(5),
            Duration::from_millis(5),
            runtime::CHAIN,
        );
        let original = f.original();
        f.admit(&original);
        f.until("resumed", |f| {
            f.row(&original).receipt.resume.step == Step::Recorded
        });
        for _ in 0..30 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(ran.counts(), (1, 0), "no send while the agent is busy");
        assert_eq!(f.row(&original).receipt.send.step, Step::Queued);
        resumed_record(&f.state, true);
        f.until("settled", |f| f.row(&original).receipt.drained);
        assert_eq!(f.row(&original).receipt.send.step, Step::Recorded);
        assert_eq!(ran.counts(), (1, 1));
    }

    /// A restart never continues a wait or starts the send: the operation
    /// it finds is retired, its send recorded as never started.
    #[test]
    fn a_restart_during_the_wait_never_sends() {
        let ran = Arc::new(Ran::default());
        let mut f = Fixture::with(executor(ran.clone(), Resume::Completed { ready: false }));
        f.runtime.fixture_wait(
            Duration::from_secs(5),
            Duration::from_millis(5),
            runtime::CHAIN,
        );
        let original = f.original();
        f.admit(&original);
        f.until("resumed", |f| {
            f.row(&original).receipt.resume.step == Step::Recorded
        });
        // The service restarts: a new runtime, the store reopened.
        f.runtime.request_stop();
        f.until("stopped", |f| {
            f.runtime.drained() || f.row(&original).receipt.drained
        });
        ledger::startup(&mut f.store.db).unwrap();
        let receipt = f.row(&original).receipt;
        assert!(receipt.drained);
        assert_eq!(receipt.resume.step, Step::Recorded);
        assert_eq!(receipt.send.step, Step::NotStarted);
        assert!(f.row(&original).send.is_none());
        resumed_record(&f.state, true);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(ran.counts(), (1, 0));
    }

    /// macOS refuses every original before any row or child.
    #[test]
    fn the_mac_fence_refuses_before_any_row() {
        let ran = Arc::new(Ran::default());
        let mut f = Fixture::with(executor(ran.clone(), Resume::Completed { ready: true }));
        f.runtime.fixture_mac_fence(true);
        let original = f.original();
        let admission = f.admit(&original);
        assert!(!admission.accepted && !admission.durable && admission.receipt.is_none());
        assert_eq!(admission.code, runtime::MAC_UNAVAILABLE);
        assert!(ledger::load(&f.store.db, &original.plan.resume_request_id)
            .unwrap()
            .is_none());
        assert_eq!(ran.counts(), (0, 0));
    }

    /// zerus-qw7wp's clock, shared through `operation_clock`: a second
    /// operation queued behind another's wait keeps its whole bound, counted
    /// from its own first turn.
    #[test]
    fn a_second_operation_queued_behind_a_wait_still_runs() {
        let ran = Arc::new(Ran::default());
        let first_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let gate = first_done.clone();
        let inner = executor(ran.clone(), Resume::Completed { ready: true });
        let mut f = Fixture::with(Arc::new(
            move |state, plan, args, input, lock, resources| {
                // The first operation's resumed agent never becomes ready.
                if args[0] == "session-action" && !gate.swap(true, Ordering::SeqCst) {
                    ran_busy(state, plan, input);
                    return Ended::Exited { code: 0 };
                }
                inner(state, plan, args, input, lock, resources)
            },
        ));
        // The first waits 300 ms of a 345 ms chain; a clock from admission
        // would expire the second before its first turn.
        f.runtime.fixture_wait(
            Duration::from_millis(300),
            Duration::from_millis(5),
            Duration::from_millis(345),
        );
        // Two stopped sessions: one resume each.
        let first = f.original();
        let other = "claude/private-second";
        session_record(&f.state, other, "claude", RUN, json!({}));
        let second = original_of(&f.location.configuration, &f.epoch, other);
        f.admit(&first);
        let admitted = Instant::now();
        f.admit(&second);
        f.until("both settled", |f| {
            f.row(&first).receipt.drained && f.row(&second).receipt.drained
        });
        assert!(Instant::now() > admitted + Duration::from_millis(345));
        assert_eq!(f.row(&first).receipt.code, ledger::WAIT_EXPIRED);
        assert_eq!(f.row(&second).receipt.send.step, Step::Recorded);
        assert!(first_done.load(Ordering::SeqCst));
    }
    /// The first operation's resume: recorded, its agent busy.
    fn ran_busy(state: &Path, plan: &wire::Plan, input: &[u8]) {
        action_receipt(state, plan, input, "completed");
        resumed_record_of(state, &plan.session, false);
    }

    /// A pinned stand-in image for `hgs`: its resume copies the prepared
    /// exact action receipt and writes the resumed run's ready record, after
    /// marking its custody descriptor close-on-exec as `hgs` does first and
    /// starting a daemon the way tmux starts its server; its send writes its
    /// exact input receipt, or only the `sent` marker.
    #[cfg(target_os = "linux")]
    fn image(path: &Path, state: &Path, dir: &Path, receipt: bool) {
        let pid = std::process::id();
        std::fs::write(
            path,
            format!(
                r#"#!/usr/bin/env python3
import fcntl,hashlib,json,os,subprocess,sys,time
data=sys.stdin.buffer.read()
request=json.loads(data)
state={state:?}
dir={dir:?}
custody=os.environ.get("HGS_SESSION_RESUME_CUSTODY_FD")
with open(os.path.join(dir,"seen"),"a") as log:
    log.write(json.dumps({{"argv":sys.argv[1:],"stdin":data.decode(),"custody":custody}})+"\n")
# As hgs does first: its custody lock lives only as long as this child.
fd=int(custody)
fcntl.fcntl(fd,fcntl.F_SETFD,fcntl.fcntl(fd,fcntl.F_GETFD)|fcntl.FD_CLOEXEC)
def write(path,value):
    fd=os.open(path+".tmp",os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    os.write(fd,value)
    os.close(fd)
    os.rename(path+".tmp",path)
name=sys.argv[2]
if sys.argv[1]=="session-action":
    # A server that outlives this child, as tmux's does: it inherits this
    # child's output, then detaches and closes it.
    daemon="import os,sys,time\nif os.fork():\n    os._exit(0)\nos.setsid()\nnull=os.open('/dev/null',os.O_RDWR)\nfor n in (0,1,2):\n    os.dup2(null,n)\nopen(sys.argv[1],'w').write(' '.join(os.listdir('/proc/self/fd')))\ntime.sleep(3)\n"
    subprocess.run([sys.executable,"-c",daemon,os.path.join(dir,"daemon-fds"),custody])
    write(os.path.join(state,"action_receipts",request["request_id"]+".json"),open(os.path.join(dir,"receipt"),"rb").read())
    record={{"name":name,"agent":"claude","run_id":"{resumed}","conversation_id":request["expected_conversation_id"],"expected_id":"","activity":"idle","phase":"idle","error":"","pid":{pid},"process_start":open(os.path.join(dir,"start")).read()}}
    write(os.path.join(state,hashlib.sha256(name.encode()).hexdigest()+".json"),json.dumps(record).encode())
elif {receipt}:
    write(os.path.join(state,"input_receipts",request["request_id"]+".json"),json.dumps({{"request_id":request["request_id"],"name":name,"run_id":request["expected_run_id"],"conversation_id":request["expected_conversation_id"],"digest":hashlib.sha256(data).hexdigest(),"status":"submitted","transport":"terminal"}}).encode())
else:
    open(os.path.join(dir,"sent"),"w").close()
"#,
                state = state.to_str().unwrap(),
                dir = dir.to_str().unwrap(),
                pid = pid,
                resumed = RESUMED,
                receipt = if receipt { "True" } else { "False" },
            ),
        )
        .unwrap();
        mode(path, 0o700);
        std::fs::write(dir.join("start"), started(pid)).unwrap();
    }
    /// Prepares the pinned image's exact action receipt for `original`.
    #[cfg(target_os = "linux")]
    fn prepared(f: &Fixture, dir: &Path, original: &Verified) {
        for folder in [receipt::RESUME_FOLDER, receipt::INPUT_FOLDER] {
            std::fs::create_dir(f.state.join(folder)).unwrap();
            mode(&f.state.join(folder), 0o700);
        }
        let value = crate::state::lifecycle_fixture_receipt(
            SESSION,
            original.intent.resume.as_bytes(),
            "completed",
            Some(RESUMED),
        );
        std::fs::write(dir.join("receipt"), value.to_string()).unwrap();
    }

    /// The pinned image runs the resume with its exact stdin, then the send
    /// with exactly the template bound to the resumed run, each holding its
    /// own custody. The resume's drain settles while the server it started
    /// still runs, and that server never holds the custody lock.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_pinned_image_runs_both_originals_and_a_detached_server_holds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hgs");
        let mut f = Fixture::unexecuted();
        image(&path, &f.state, dir.path(), true);
        f.runtime.fixture_image(&path);
        let original = f.original();
        prepared(&f, dir.path(), &original);
        assert!(f.admit(&original).accepted);
        let started_at = Instant::now();
        f.until("resumed", |f| {
            f.row(&original).receipt.resume.step == Step::Recorded
        });
        // The detached server sleeps 3 s: the resume drained without it.
        assert!(started_at.elapsed() < Duration::from_secs(3));
        f.until("settled", |f| f.row(&original).receipt.drained);
        let row = f.row(&original);
        assert_eq!(row.receipt.send.step, Step::Recorded);
        let (_, bound, _) = row.bound().unwrap();
        let seen: Vec<Value> = std::fs::read_to_string(dir.path().join("seen"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[0]["argv"],
            json!(["session-action", SESSION, "--json"])
        );
        assert_eq!(seen[0]["stdin"], original.intent.resume.as_str());
        assert_eq!(seen[1]["argv"], json!(["send", SESSION, "--json"]));
        assert_eq!(seen[1]["stdin"], bound.as_str());
        for child in &seen {
            assert!(child["custody"]
                .as_str()
                .is_some_and(|fd| fd.parse::<u32>().is_ok()));
        }
        let custody = seen[0]["custody"].as_str().unwrap();
        let fds = std::fs::read_to_string(dir.path().join("daemon-fds")).unwrap();
        assert!(
            !fds.split_whitespace().any(|fd| fd == custody),
            "the server holds the custody descriptor {custody}: {fds}"
        );
    }

    /// The send's final spawn turn re-proves the resumed run's record and
    /// its readiness from the retained roots, after every earlier read. A
    /// record that changed only there starts no send, which then never
    /// started; the resume stays recorded.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_pinned_send_reproves_its_target_and_readiness_in_the_final_turn() {
        for change in ["target", "busy"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("hgs");
            let mut f = Fixture::unexecuted();
            image(&path, &f.state, dir.path(), false);
            f.runtime.fixture_image(&path);
            let original = f.original();
            prepared(&f, dir.path(), &original);
            let turns = Arc::new(AtomicUsize::new(0));
            let count = turns.clone();
            let state = f.state.clone();
            f.runtime.fixture_spawn_hook(Arc::new(move || {
                // The second spawn turn is the send's.
                if count.fetch_add(1, Ordering::SeqCst) == 1 {
                    match change {
                        "target" => session_record(&state, SESSION, "claude", "run-3", json!({})),
                        _ => resumed_record(&state, false),
                    }
                }
            }));
            assert!(f.admit(&original).accepted);
            f.until("settled", |f| f.row(&original).receipt.drained);
            let row = f.row(&original);
            assert!(row.send.is_some(), "{change}: reserved before its turn");
            assert_eq!(turns.load(Ordering::SeqCst), 2, "{change}");
            assert_eq!(row.receipt.resume.step, Step::Recorded, "{change}");
            assert_eq!(
                (row.receipt.send.step, row.receipt.send.code.as_str()),
                (Step::NotStarted, wire::NOT_STARTED),
                "{change}"
            );
            assert_eq!(
                row.receipt.result(&original.plan),
                Some(Settled::NotSent {
                    resume: Step::Recorded
                }),
                "{change}"
            );
            assert!(!dir.path().join("sent").exists(), "{change}: never sent");
        }
    }

    /// A stop, a revoke, a changed source or a changed original record after
    /// the resume's reservation committed starts nothing: the target and
    /// consent vetoes run last in the spawn turn, and with no record of the
    /// request both children never started.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_stop_revoke_source_or_target_change_after_reservation_starts_nothing() {
        for action in ["stop", "revoke", "source", "target"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("hgs");
            let ran = dir.path().join("ran");
            std::fs::write(
                &path,
                format!(
                    "#!/bin/sh\ntouch {:?}\ncat >/dev/null\n",
                    ran.to_str().unwrap()
                ),
            )
            .unwrap();
            mode(&path, 0o700);
            let mut f = Fixture::unexecuted();
            f.runtime.fixture_image(&path);
            let (entered_tx, entered) = std::sync::mpsc::channel::<()>();
            let (release, released) = std::sync::mpsc::channel::<()>();
            let entered_tx = std::sync::Mutex::new(entered_tx);
            let released = std::sync::Mutex::new(released);
            f.runtime.fixture_spawn_hook(Arc::new(move || {
                let _ = entered_tx.lock().unwrap().send(());
                let _ = released
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10));
            }));
            let original = f.original();
            assert!(f.admit(&original).accepted);
            let until = Instant::now() + Duration::from_secs(8);
            while entered.try_recv().is_err() {
                assert!(
                    Instant::now() < until,
                    "{action}: never reached the spawn turn"
                );
                f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(
                f.row(&original).receipt.resume.step,
                Step::Running,
                "{action}"
            );
            match action {
                "stop" => f.runtime.request_stop(),
                "revoke" => f.revoke(),
                "source" => f
                    .runtime
                    .fixture_machine("33333333-3333-4333-8333-333333333333"),
                _ => session_record(&f.state, SESSION, "claude", "run-2", json!({})),
            }
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
            release.send(()).unwrap();
            f.until("settled", |f| f.row(&original).receipt.drained);
            let receipt = f.row(&original).receipt;
            assert_eq!(
                (receipt.resume.step, receipt.send.step),
                (Step::NotStarted, Step::NotStarted),
                "{action}"
            );
            assert!(!ran.exists(), "{action}: no child ever existed");
        }
    }

    /// A receipt that changes between its read and its recheck is Unknown,
    /// never the record it was: the resume may have launched, so nothing is
    /// sent.
    #[test]
    fn a_receipt_changed_around_its_read_is_unknown() {
        let ran = Arc::new(Ran::default());
        let mut f = Fixture::with(executor(ran.clone(), Resume::Completed { ready: true }));
        let state = f.state.clone();
        let original = f.original();
        let plan = original.plan.clone();
        let stdin = original.intent.resume.clone();
        f.runtime.fixture_read(
            Some(Arc::new(move || {
                action_receipt(&state, &plan, stdin.as_bytes(), "failed");
            })),
            runtime::READ_WAIT,
        );
        f.admit(&original);
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(
            (receipt.resume.step, receipt.resume.code.as_str()),
            (Step::Unknown, receipt::CHANGED)
        );
        assert_eq!(receipt.send.step, Step::NotStarted);
        assert_eq!(ran.counts(), (1, 0));
    }

    /// A stop that arrives while a readiness read runs ends the wait, vetoed,
    /// once that bounded read finishes: the send never starts, however ready
    /// the agent became meanwhile.
    #[test]
    fn a_stop_during_a_readiness_read_ends_the_wait_without_a_send() {
        let ran = Arc::new(Ran::default());
        let mut f = Fixture::with(executor(ran.clone(), Resume::Completed { ready: false }));
        f.runtime.fixture_wait(
            Duration::from_secs(5),
            Duration::from_millis(5),
            runtime::CHAIN,
        );
        let (entered_tx, entered) = std::sync::mpsc::channel::<()>();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let entered_tx = std::sync::Mutex::new(entered_tx);
        let released = std::sync::Mutex::new(released);
        let state = f.state.clone();
        f.runtime.fixture_ready_hook(Arc::new(move || {
            resumed_record(&state, true);
            let _ = entered_tx.lock().unwrap().send(());
            let _ = released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10));
        }));
        let original = f.original();
        f.admit(&original);
        let until = Instant::now() + Duration::from_secs(8);
        while entered.try_recv().is_err() {
            assert!(Instant::now() < until, "never read");
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
            std::thread::sleep(Duration::from_millis(2));
        }
        f.runtime.request_stop();
        release.send(()).unwrap();
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(receipt.resume.step, Step::Recorded);
        assert_eq!(
            (receipt.send.step, receipt.code.as_str()),
            (Step::NotStarted, ledger::VETOED)
        );
        assert_eq!(ran.counts(), (1, 0));
    }

    /// A preparation that is lost retires the operation: nothing started.
    #[test]
    fn a_lost_preparation_retires_without_any_child() {
        let ran = Arc::new(Ran::default());
        let mut f = Fixture::with(executor(ran.clone(), Resume::Completed { ready: true }));
        f.runtime
            .fixture_prepare_hook(Arc::new(|| panic!("preparation lost")));
        let original = f.original();
        f.admit(&original);
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(
            (
                receipt.resume.step,
                receipt.send.step,
                receipt.code.as_str()
            ),
            (Step::NotStarted, Step::NotStarted, ledger::INTERRUPTED)
        );
        assert_eq!(ran.counts(), (0, 0));
    }
}
