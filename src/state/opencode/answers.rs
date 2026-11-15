//! Exact original answer receipts. A lookup never reads or delivers native input.
use super::*;

const ATTEMPTED: &str = "This answer was already attempted; refresh its native acknowledgement";
const RECEIPT_BYTES: u64 = 16 * 1024;

pub(super) fn digest(name: &str, request: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(json!([name, request]).to_string().as_bytes())
    )
}

pub(super) fn prior(name: &str, request: &Value, path: &Path) -> Result<Option<Value>> {
    // A special file, link, unreadable or malformed receipt is unavailable,
    // never absence and never permission to reserve or deliver again.
    let Some(bytes) = labels::read_file(path, RECEIPT_BYTES).map_err(|_| ATTEMPTED)? else {
        return Ok(None);
    };
    let prior: Value = serde_json::from_slice(&bytes).map_err(|_| ATTEMPTED)?;
    if prior["digest"] != digest(name, request)
        || prior["name"] != name
        || prior["request_id"] != request["request_id"]
        || prior["question_id"] != request["question_id"]
        || prior["question_hash"] != request["expected_question_hash"]
        || prior["run_id"] != request["expected_run_id"]
        || prior["conversation_id"] != request["expected_conversation_id"]
    {
        return Err(format!(
            "{ATTEMPTED}: request content or original target differs"
        ));
    }
    match prior["status"].as_str() {
        Some("answered") => Ok(Some(prior)),
        Some("refused") => Err(CHANGED.into()),
        _ => Err(ATTEMPTED.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_unknown_changed_and_unavailable_receipts_never_permit_another_attempt() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("answer.json");
        let request = json!({"request_id":"id","expected_run_id":"run","expected_conversation_id":"cid",
            "question_id":"frm_question","expected_question_hash":"hash","answers":[]});
        assert!(prior("name", &request, &path).unwrap().is_none());
        let receipt = json!({"request_id":"id","name":"name","run_id":"run","conversation_id":"cid",
            "question_id":"frm_question","question_hash":"hash","digest":digest("name",&request),"status":"answered"});
        fs::write(&path, receipt.to_string()).unwrap();
        assert_eq!(
            prior("name", &request, &path).unwrap(),
            Some(receipt.clone())
        );
        assert!(prior("different-name", &request, &path).is_err());
        let mut changed = request.clone();
        changed["answers"] = json!(["other"]);
        assert!(prior("name", &changed, &path).is_err());
        for status in ["pending", "unknown", "refused", ""] {
            let mut changed = receipt.clone();
            changed["status"] = json!(status);
            fs::write(&path, changed.to_string()).unwrap();
            assert!(prior("name", &request, &path).is_err(), "{status}");
        }
        let mut legacy = receipt;
        legacy.as_object_mut().unwrap().remove("digest");
        fs::write(&path, legacy.to_string()).unwrap();
        assert!(prior("name", &request, &path).is_err());
        fs::write(&path, " ".repeat(RECEIPT_BYTES as usize + 1)).unwrap();
        assert!(prior("name", &request, &path).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(prior("name", &request, &path).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn links_and_fifos_are_unavailable_without_waiting_for_a_writer() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("answer.json");
        symlink(temp.path().join("missing"), &path).unwrap();
        assert!(prior("name", &json!({}), &path).is_err());
        fs::remove_file(&path).unwrap();
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let start = Instant::now();
        assert!(prior("name", &json!({}), &path).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
