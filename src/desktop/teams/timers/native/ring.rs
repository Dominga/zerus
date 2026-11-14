//! The doorbell's original: `hgs send NAME --json` bound to the exact run
//! and conversation it was reserved for. Its admission is read inside one
//! immediate transaction on the team store, held across the spawn only.
//! Only the CLI's own `submitted` receipt is a submission; a missing receipt
//! after a refusing exit is certain only for agents that write receipts.
use super::*;
use crate::{
    desktop::original_store::{self, Failure, Roots},
    teams::store::timers::admit,
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use zerus_client_core::desktop::deferred_send as send;

const CODES: Codes = Codes {
    unobserved: "team_doorbell_unobserved",
    unreadable: "team_doorbell_unreadable",
    signalled: "team_doorbell_signalled",
    timed_out: "team_doorbell_timed_out",
    retained: "team_doorbell_retained",
};
/// The doorbell keeps this much of the send's stderr, its refusal.
const REFUSAL_BYTES: usize = 8 << 10;
/// Agents whose `hgs send` writes `input_receipts` before the first byte.
const RECEIPTED: [&str; 3] = ["claude", "codex", "kimi"];

pub(super) fn deliver(n: &Native<'_>, store: &Store, ring: &Ring) -> Rung {
    let Some(db) = store.db.as_ref() else {
        return Rung::NotStarted("The team store is unavailable".into());
    };
    let input = serde_json::json!({"request_id":ring.id,"expected_run_id":ring.run,
        "expected_conversation_id":ring.conversation,"text":ring.text})
    .to_string()
    .into_bytes();
    let root = if ring.agent == "dsh" { n.config.state.join("dsh") } else { n.config.state.clone() };
    let folder = if ring.agent == "dsh" { "receipts" } else { "input_receipts" };
    let Ok(roots) = Roots::open(&root, folder) else {
        return Rung::NotStarted("The receipt store is unavailable".into());
    };
    let busy = Cell::new(false);
    let args = ["send", ring.name.as_str(), "--json"];
    let ended = n.original(
        &ring.id,
        &args,
        &input,
        CODES,
        // `hgs send` refuses only on stderr; its stdout carries the receipt,
        // which is read from the receipt store instead (zerus-63rru.57).
        Output::StderrPrefix(REFUSAL_BYTES),
        Duration::from_secs(45),
        || {
            let tx = immediate(db)
                .inspect_err(|e| busy.set(e.kind() == std::io::ErrorKind::WouldBlock))?;
            #[cfg(test)]
            if let Some(hook) = super::tests::LOCKED.with(|slot| slot.borrow_mut().take()) {
                hook();
            }
            admit(&tx, &ring.gate)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
            Ok(tx)
        },
    );
    match ended {
        Ended::NotStarted if busy.get() => {
            Rung::NotStarted("The team store was busy; nothing started".into())
        }
        Ended::NotStarted => {
            Rung::NotStarted("The doorbell admission refused; nothing started".into())
        }
        Ended::Unknown(why) => Rung::Uncertain(why.into()),
        Ended::Exited { code, output } => receipt(&roots, ring, &input, code, &output),
    }
}
fn receipt(roots: &Roots, ring: &Ring, input: &[u8], code: i32, stderr: &[u8]) -> Rung {
    let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let plan = send::Plan {
        session: ring.name.clone(),
        request_id: ring.id.clone(),
        run_id: ring.run.clone(),
        conversation_id: ring.conversation.clone(),
        message_sha256: digest(ring.text.as_bytes()),
        input_sha256: digest(input),
    };
    let Ok(id) = uuid::Uuid::parse_str(&ring.id) else {
        return Rung::Uncertain("The doorbell request ID is invalid".into());
    };
    let (bytes, _evidence) = original_store::read(roots, &id, send::RECEIPT_BYTES, &|| {});
    match bytes {
        Ok(bytes) if ring.agent == "dsh" => hosted_receipt(ring, input, &bytes),
        Ok(bytes) => match send::Outcome::from_receipt(&plan, &bytes) {
            Ok(_) => Rung::Submitted,
            Err(why) => Rung::Uncertain(why.into()),
        },
        Err(Failure::Missing) if code != 0 && RECEIPTED.contains(&ring.agent.as_str()) => {
            Rung::Refused(refusal(stderr))
        }
        Err(failure) => {
            Rung::Uncertain(format!("The doorbell receipt is unavailable: {failure:?}"))
        }
    }
}
fn hosted_receipt(ring: &Ring, input: &[u8], bytes: &[u8]) -> Rung {
    let parsed = serde_json::from_slice::<serde_json::Value>(input);
    let receipt = serde_json::from_slice::<serde_json::Value>(bytes);
    if let (Ok(input), Ok(receipt)) = (parsed, receipt) {
        let digest = format!("{:x}", Sha256::digest(serde_json::json!([ring.name, input]).to_string().as_bytes()));
        if receipt["request_id"] == ring.id && receipt["name"] == ring.name
            && receipt["run_id"] == ring.run && receipt["conversation_id"] == ring.conversation
            && receipt["digest"] == digest && receipt["status"] == "submitted"
            && receipt["transport"] == "native_dsh" {
            return Rung::Submitted;
        }
    }
    Rung::Uncertain("The DeepSeek doorbell has no exact submitted native receipt; nothing is sent again".into())
}
/// The CLI's reason for a refused send: `hgs` writes a refused command as one
/// `hgs: <reason>` line on stderr, last, after any warnings that start the same
/// way (`cli::error_line`), and nothing on stdout (zerus-63rru.57).
fn refusal(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim_end();
    let last = text
        .rfind(&format!("\n{}", crate::cli::ERROR_PREFIX))
        .map_or(0, |at| at + 1);
    let line = &text[last..];
    let reason = line.strip_prefix(crate::cli::ERROR_PREFIX).unwrap_or(line);
    let reason = crate::desktop::teams::reason_text(reason);
    if reason.is_empty() {
        "The send was refused before any input".into()
    } else {
        reason
    }
}
/// A busy writer is a certain non-start, never an Unknown.
fn immediate(db: &Connection) -> std::io::Result<Transaction<'_>> {
    let prior: u64 = db
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .map_err(std::io::Error::other)?;
    db.busy_timeout(Duration::from_millis(100))
        .map_err(std::io::Error::other)?;
    let result = Transaction::new_unchecked(db, TransactionBehavior::Immediate);
    db.busy_timeout(Duration::from_millis(prior))
        .map_err(std::io::Error::other)?;
    result.map_err(|error| {
        let busy = matches!(&error, rusqlite::Error::SqliteFailure(e, _)
            if matches!(e.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked));
        let kind = if busy { std::io::ErrorKind::WouldBlock } else { std::io::ErrorKind::PermissionDenied };
        std::io::Error::new(kind, error)
    })
}
