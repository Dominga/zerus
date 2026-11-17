//! One explicit person decision, kept separately from native acceptance and clearing.
use super::*;
use zerus_client_core::input_acknowledgment::{canonical, Acknowledgment, PendingInput};

const FIELD: &str = "input_acknowledged_unknown";
const AUDIT_BYTES: u64 = 32 * 1024;

/// The acknowledgment is evidence for this pending input only, never a time-based bypass.
pub(crate) fn matches(record: &Value) -> bool {
    let ack = &record[FIELD];
    ack["status"] == "person-acknowledged-unknown"
        && canonical(string(ack, "request_id"))
        && canonical(string(ack, "original_request_id"))
        && !string(record, "conversation_id").is_empty()
        && ack["run_id"] == record["run_id"]
        && ack["conversation_id"] == record["conversation_id"]
        && ack["original_request_id"] == record["input_pending_request"]
        && ack["pending_at"].as_f64().is_some_and(|at| {
            at.is_finite() && at > 0.0 && Some(at) == record["input_pending_at"].as_f64()
        })
}

fn bounded(path: &Path, limit: u64) -> Result<Option<Value>> {
    // The shared reader opens no-follow/nonblocking and verifies the held
    // object is regular before reading. A byte limit alone cannot bound a FIFO.
    let Some(bytes) = labels::read_file(path, limit).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| e.to_string())
}

pub(crate) fn pending(record: &Value) -> Result<PendingInput> {
    if !cfg!(any(target_os = "linux", target_os = "macos"))
        || !matches!(string(record, "agent"), "claude" | "codex" | "kimi")
        || string(record, "conversation_id").is_empty()
        || string(record, "conversation_state") == "unknown"
        || !string(record, "expected_id").is_empty()
        || (!(string(record, "activity") == "idle" && string(record, "phase") == "idle")
            && !input::acceptance::native_idle(record))
        || !string(record, "error").is_empty()
        || pause_active(record)
        || input::input_typing(record).is_some()
        || input::send_in_flight(record)
        || !input::input_unconfirmed(record)
        || record["pending_settings"].is_object()
        || input::settings_unconfirmed_metadata(record)
        || clear_context::awaiting_start(record)
        || compact_context::pending(record)
    {
        return Err("This input cannot be acknowledged now; check Terminal".into());
    }
    let id = string(record, "input_pending_request");
    let at = record["input_pending_at"]
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or("The original input is unavailable")?;
    if !canonical(id) {
        return Err("The original input identity is unavailable".into());
    }
    let receipt = bounded(
        &absolute_root()?
            .join("input_receipts")
            .join(format!("{id}.json")),
        256 * 1024,
    )?
    .ok_or("The original input evidence is unavailable")?;
    if receipt["request_id"] != id
        || receipt["run_id"] != record["run_id"]
        || receipt["conversation_id"] != record["conversation_id"]
        || receipt["submitted_at"].as_f64() != Some(at)
        || receipt["name"] != record["name"]
    {
        return Err("The original input evidence changed".into());
    }
    Ok(PendingInput {
        original_request_id: id.into(),
        pending_at: at,
    })
}

fn terminal_request(ack: &Acknowledgment) -> Request {
    Request {
        request_id: ack.request_id.clone(),
        action: "acknowledge-input".into(),
        expected_run_id: ack.expected_run_id.clone(),
        expected_conversation_id: ack.expected_conversation_id.clone(),
        terminal_binding_id: Some(ack.terminal_binding_id.clone()),
        expires_at: Some(ack.expires_at),
        text: None,
        enter: None,
        key: None,
        draft_hash: None,
    }
}

fn path(id: &str) -> Result<PathBuf> {
    if !canonical(id) {
        return Err("Invalid input acknowledgment identity".into());
    }
    Ok(absolute_root()?
        .join("input_acknowledgments")
        .join(format!("{id}.json")))
}

fn valid_audit(receipt: &Value, ack: &Acknowledgment) -> bool {
    receipt["op"] == "input_acknowledge"
        && receipt["request_id"] == ack.request_id
        && receipt["original_request_id"] == ack.original_request_id
        && receipt["name"] == ack.name
        && receipt["run_id"] == ack.expected_run_id
        && receipt["conversation_id"] == ack.expected_conversation_id
        && receipt["acknowledgment"] == json!(ack)
        && matches!(
            receipt["source"].as_str(),
            Some("native_frontend" | "interactive_terminal")
        )
        && match receipt["status"].as_str() {
            Some("claimed") => {
                receipt.get("accepted").is_some_and(Value::is_null) && receipt["durable"] == false
            }
            Some("person-acknowledged-unknown") => {
                receipt["accepted"] == true
                    && receipt["durable"] == true
                    && receipt["settled_at"]
                        .as_f64()
                        .is_some_and(|at| at.is_finite() && at > 0.0)
            }
            _ => false,
        }
}

/// Point lookup only, with a fixed byte bound. It never retries a settlement.
pub(crate) fn lookup(ack: &Acknowledgment) -> Result<Option<Value>> {
    if !ack.valid() {
        return Err("Invalid input acknowledgment".into());
    }
    let Some(mut receipt) = bounded(&path(&ack.request_id)?, AUDIT_BYTES)? else {
        return Ok(None);
    };
    if !valid_audit(&receipt, ack) {
        return Err(
            "The recorded acknowledgment is not verified for this decision; check Terminal".into(),
        );
    }
    if receipt["status"] == "claimed" {
        // The audit reservation can survive a crash on either side of the record
        // write. An uncertain result never re-enters the mutation.
        receipt["status"] = json!("unknown");
        receipt["accepted"] = Value::Null;
        receipt["durable"] = json!(false);
        receipt["error"] =
            json!("The acknowledgment outcome is unknown. Check Terminal before a new decision.");
    }
    Ok(Some(receipt))
}

pub(crate) fn settle(
    ack: &Acknowledgment,
    authority: &mut crate::desktop::input_acknowledgment::Person,
) -> Result<Value> {
    if !ack.valid() {
        return Err("Invalid input acknowledgment".into());
    }
    let _guard = lock(None)?;
    authority.check(ack)?;
    if let Some(receipt) = lookup(ack)? {
        return Ok(receipt);
    }
    let request = terminal_request(ack);
    let mut record = check_handoff(&ack.name, &request)?;
    let original = pending(&record)?;
    if original.original_request_id != ack.original_request_id
        || original.pending_at != ack.pending_at
    {
        return Err("The pending input changed; review it again".into());
    }
    if composer_now(backend::current(), &record) != Some(input::composer::Composer::Empty) {
        return Err("The native input is not verified empty; check Terminal".into());
    }
    // Reservation is durable before any gate can open. No original input receipt
    // is changed. Each audit is a bounded, exact-ID read, never a history scan.
    let path = path(&ack.request_id)?;
    let mut receipt = json!({"op":"input_acknowledge", "request_id":ack.request_id,
        "original_request_id":ack.original_request_id,"name":ack.name,
        "run_id":ack.expected_run_id,"conversation_id":ack.expected_conversation_id,
        "status":"claimed","accepted":null,"durable":false,"source":authority.source(),"acknowledgment":ack});
    atomic(&path, &receipt.to_string())?;
    input::acceptance::remember_unknown(&record)?;
    // Revalidate the original native caller and session immediately before the
    // record write. A failed reserved decision remains unknown and is not replayed.
    authority.check(ack)?;
    let fresh = check_handoff(&ack.name, &request)?;
    if pending(&fresh)? != original {
        return Err("The pending input changed; review it again".into());
    }
    if composer_now(backend::current(), &record) != Some(input::composer::Composer::Empty) {
        return Err("The native input changed before acknowledgment".into());
    }
    authority.check(ack)?;
    check_handoff(&ack.name, &request)?;
    let settled = json!({"status":"person-acknowledged-unknown", "request_id":ack.request_id,
        "original_request_id":ack.original_request_id, "pending_at":ack.pending_at,
        "run_id":ack.expected_run_id, "conversation_id":ack.expected_conversation_id,
        "terminal_binding_id":ack.terminal_binding_id, "at":now(), "source":authority.source()});
    record[FIELD] = settled;
    write(&mut record)?;
    receipt["status"] = json!("person-acknowledged-unknown");
    receipt["accepted"] = json!(true);
    receipt["durable"] = json!(true);
    receipt["settled_at"] = record[FIELD]["at"].clone();
    atomic(&path, &receipt.to_string())?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn evidence_reads_require_bounded_regular_files_and_never_follow_links() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("evidence.json");
        assert!(bounded(&path, 4).unwrap().is_none());
        fs::write(&path, b"null").unwrap();
        assert_eq!(bounded(&path, 4).unwrap(), Some(Value::Null));
        assert!(bounded(&path, 3).is_err());
        assert!(bounded(dir.path(), 4).is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("link.json");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(bounded(&link, 4).is_err());
            fs::remove_file(&path).unwrap();
            assert!(bounded(&link, 4).is_err(), "a dangling link is not absence");
            assert!(bounded(Path::new("/dev/null"), 4).is_err());
        }
    }

    #[test]
    #[cfg(unix)]
    fn fifo_evidence_refuses_without_waiting_for_a_writer() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt, sync::mpsc, time::Duration};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("evidence.json");
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (tx, rx) = mpsc::channel();
        let target = path.clone();
        let reader = std::thread::spawn(move || tx.send(bounded(&target, 4)).unwrap());
        let result = rx.recv_timeout(Duration::from_secs(1));
        if result.is_err() {
            // Settle this fixture's reader before failing a blocking-open mutant.
            use std::{io::Write, os::unix::fs::OpenOptionsExt};
            if let Ok(mut writer) = fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
            {
                let _ = writer.write_all(b"null");
            }
        }
        reader.join().unwrap();
        assert!(result.expect("FIFO evidence read blocked").is_err());
    }

    #[test]
    fn acknowledgment_settles_only_its_exact_original_pending_input() {
        let mut record = json!({"run_id":"run","conversation_id":"conversation",
            "input_pending_request":"11111111-1111-4111-8111-111111111111","input_pending_at":10.0,"input_seen_at":0.0});
        assert!(input::input_unconfirmed(&record));
        record[FIELD] = json!({"request_id":"22222222-2222-4222-8222-222222222222",
            "status":"person-acknowledged-unknown","original_request_id":record["input_pending_request"],
            "pending_at":10.0,"run_id":"run","conversation_id":"conversation"});
        assert!(!input::input_unconfirmed(&record));
        for (field, value) in [
            ("run_id", json!("new-run")),
            ("conversation_id", json!("new-conversation")),
            (
                "input_pending_request",
                json!("33333333-3333-4333-8333-333333333333"),
            ),
            ("input_pending_at", json!(11.0)),
        ] {
            let mut changed = record.clone();
            changed[field] = value;
            assert!(
                input::input_unconfirmed(&changed),
                "old acknowledgment must not settle {field}"
            );
        }
        let mut unknown = record;
        unknown[FIELD]["status"] = json!("claimed");
        assert!(input::input_unconfirmed(&unknown));
    }
}
