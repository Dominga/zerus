//! Stored identity for logical project placement. Read only the binding fields;
//! no provider history reconciliation, RPC, discovery or process action belongs
//! to this metadata gate.
use super::*;

pub(crate) fn with_session_project_binding<T>(
    name: &str,
    check: impl FnOnce(&Value) -> std::result::Result<T, anyhow::Error>,
) -> anyhow::Result<T> {
    // Keep the launch metadata transaction's established lock order. The
    // provider bindings are immutable for this transaction, including rename.
    let _writer = lock(None).map_err(anyhow::Error::msg)?;
    let _dsh = lock(Some(&root().join("dsh/bindings.lock"))).map_err(anyhow::Error::msg)?;
    let _opencode = opencode::lock_bindings().map_err(anyhow::Error::msg)?;
    let (provider, path) = binding_location(name);
    let record = binding_metadata(&path, name, provider)?;
    check(&record)
}
/// What a pinned binding attempt found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Pinned<T> {
    /// `write` ran under the locks.
    Written(T),
    /// A lock was busy; nothing is held now, so the caller may retry.
    Busy,
    /// `name` is no longer that run and conversation.
    Changed,
}
/// Runs `write` while `name` is still the binding of run `run_id` with
/// conversation `conversation`: a fork's inherited project, written after its
/// launch released the state lock (zerus-wx3k). The locks and their order are
/// a project move's: the state writer lock (waited for until `deadline`),
/// then the provider's binding lock, tried once so that a busy provider never
/// holds the state lock (zerus-jx5o). The binding is checked under both.
/// `write` must not wait for another lock.
pub(crate) fn with_pinned_session_binding<T>(
    name: &str,
    run_id: &str,
    conversation: &Value,
    deadline: std::time::Instant,
    write: impl FnOnce() -> T,
) -> anyhow::Result<Pinned<T>> {
    let Some(_writer) = super::storage::writer_until(deadline).map_err(anyhow::Error::msg)? else {
        return Ok(Pinned::Busy);
    };
    let (provider, path) = binding_location(name);
    let _provider = match provider {
        Some(provider) => {
            let lock = root().join(provider).join("bindings.lock");
            let once = std::time::Instant::now();
            match super::storage::lock_until(&lock, once).map_err(anyhow::Error::msg)? {
                Some(guard) => Some(guard),
                None => return Ok(Pinned::Busy),
            }
        }
        None => None,
    };
    let Ok(record) = binding_metadata(&path, name, provider) else {
        return Ok(Pinned::Changed);
    };
    if string(&record, "run_id") != run_id || record["conversation_id"] != *conversation {
        return Ok(Pinned::Changed);
    }
    Ok(Pinned::Written(write()))
}
/// A binding's provider (none for a terminal session) and its record path.
fn binding_location(name: &str) -> (Option<&'static str>, PathBuf) {
    let provider = if name.starts_with("opencode/") {
        Some("opencode")
    } else if name.starts_with("dsh/") {
        Some("dsh")
    } else {
        None
    };
    let path = provider.map_or_else(
        || record_path(name),
        |provider| {
            root()
                .join(provider)
                .join("bindings")
                .join(legacy_record_path(name).file_name().unwrap())
        },
    );
    (provider, path)
}
fn binding_metadata(path: &Path, name: &str, provider: Option<&str>) -> anyhow::Result<Value> {
    use std::io::Read;
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.file_type().is_file(),
        "Session binding is not a regular file"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1_048_577)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1_048_576, "Session binding is too large");
    let record: Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        record.is_object() && string(&record, "name") == name,
        "Stored session identity changed"
    );
    let valid = match provider {
        Some("opencode") => {
            record["backend"] == "opencode"
                && opencode::native_id(string(&record, "conversation_id"), "ses_")
        }
        Some("dsh") => {
            record["backend"] == "dsh"
                && string(&record, "conversation_id").starts_with("session-")
                && string(&record, "conversation_id").len() <= 2048
        }
        None => record["version"] == 1 && AGENTS.contains(&string(&record, "agent")),
        _ => false,
    };
    anyhow::ensure!(
        valid && string(&record, "archive_id").is_empty() && record["state"] != "archived",
        "Choose the exact parent session; archived conversations are read-only"
    );
    Ok(record)
}
/// Rules are metadata on the saved session, so rename, resume and context
/// changes retain them. Pin native identities for the whole write.
pub(crate) fn update_iron_rules(
    name: &str,
    run: &str,
    conversation: &str,
    expected: u64,
    rules: &[String],
    request: &str,
    digest: &str,
) -> anyhow::Result<Value> {
    let _writer = lock(None).map_err(anyhow::Error::msg)?;
    let _dsh = lock(Some(&root().join("dsh/bindings.lock"))).map_err(anyhow::Error::msg)?;
    let _opencode = opencode::lock_bindings().map_err(anyhow::Error::msg)?;
    let provider = if name.starts_with("opencode/") {
        Some("opencode")
    } else if name.starts_with("dsh/") {
        Some("dsh")
    } else {
        None
    };
    let path = provider.map_or_else(
        || record_path(name),
        |provider| {
            root()
                .join(provider)
                .join("bindings")
                .join(legacy_record_path(name).file_name().unwrap())
        },
    );
    let mut record = binding_metadata(&path, name, provider)?;
    anyhow::ensure!(
        string(&record, "run_id") == run && string(&record, "conversation_id") == conversation,
        "Session identity changed"
    );
    let revision = record["iron_rules_revision"].as_u64().unwrap_or(0);
    if record["iron_rules_request"] == request {
        anyhow::ensure!(
            record["iron_rules_digest"] == digest,
            "Request identity was used for different rules"
        );
        return Ok(
            json!({"scope":"session","name":name,"rules":record["iron_rules"],"revision":revision,"run_id":run,"conversation_id":conversation}),
        );
    }
    anyhow::ensure!(
        revision == expected,
        "Session rules changed; read them again"
    );
    record["iron_rules"] = json!(rules);
    record["iron_rules_revision"] = json!(revision + 1);
    record["iron_rules_request"] = json!(request);
    record["iron_rules_digest"] = json!(digest);
    if provider.is_none() {
        write(&mut record).map_err(anyhow::Error::msg)?;
    } else {
        atomic(&path, &record.to_string()).map_err(anyhow::Error::msg)?;
    }
    Ok(
        json!({"scope":"session","name":name,"rules":rules,"revision":revision+1,"run_id":run,"conversation_id":conversation}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_bounded_literal_binding_identity_is_needed_for_paused_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binding.json");
        let name = "claude/example/task";
        let raw = json!({"version":1,"agent":"claude","name":name,"run_id":"run-one","conversation_id":"one","state":"paused","agent_home":"/unread/provider/home"});
        std::fs::write(&path, raw.to_string()).unwrap();
        assert_eq!(binding_metadata(&path, name, None).unwrap(), raw);
        assert!(binding_metadata(&path, "claude/foreign/task", None).is_err());
        assert!(binding_metadata(&path, name, Some("opencode")).is_err());
        std::fs::write(&path, "x".repeat(1_048_577)).unwrap();
        assert!(binding_metadata(&path, name, None).is_err());
        std::fs::write(
            &path,
            json!({"version":1,"agent":"claude","name":name,"archive_id":"archive"}).to_string(),
        )
        .unwrap();
        assert!(binding_metadata(&path, name, None).is_err());
    }
    /// zerus-wx3k: a fork's inherited project is written only while its
    /// binding is still the same run and conversation, and a busy state or
    /// provider lock never makes it wait past its deadline. The fixture runs
    /// in a child process with a private state directory.
    #[test]
    fn a_pinned_write_needs_the_same_binding_and_never_outwaits_its_deadline() {
        let state = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::session_project::tests::pinned_binding_fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", state.path())
            .env("ZERUS_PINNED_BINDING_FIXTURE", "1")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_RUN_ID")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    #[ignore = "run by a_pinned_write_needs_the_same_binding_and_never_outwaits_its_deadline"]
    fn pinned_binding_fixture() {
        use std::time::{Duration, Instant};
        if std::env::var_os("ZERUS_PINNED_BINDING_FIXTURE").is_none() {
            return;
        }
        let soon = || Instant::now() + Duration::from_millis(300);
        let pinned = |name: &str, run: &str, conversation: &Value| {
            with_pinned_session_binding(name, run, conversation, soon(), || "written").unwrap()
        };
        let free = |lock: &std::path::Path| {
            super::super::storage::lock_until(lock, Instant::now()).unwrap().is_some()
        };
        let terminal = "claude/example/fork";
        let deepseek = "dsh/example/fork";
        let save = |name: &str, record: Value| {
            let (_, path) = binding_location(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, record.to_string()).unwrap();
        };
        let own = json!({"version":1,"agent":"claude","name":terminal,"run_id":"run-1","conversation_id":null});
        save(terminal, own.clone());
        save(deepseek, json!({"backend":"dsh","name":deepseek,"run_id":"run-2","conversation_id":"session-2"}));
        let writer = root().join(".lock");
        let provider = root().join("dsh/bindings.lock");
        assert_eq!(pinned(terminal, "run-1", &Value::Null), Pinned::Written("written"));
        assert_eq!(pinned(deepseek, "run-2", &json!("session-2")), Pinned::Written("written"));
        // Both locks are held while `write` runs (Z6, Z7).
        let held = with_pinned_session_binding(deepseek, "run-2", &json!("session-2"), soon(), || {
            (free(&writer), free(&provider))
        });
        assert_eq!(held.unwrap(), Pinned::Written((false, false)));
        // A replaced run, another conversation or no binding: nothing is written.
        assert_eq!(pinned(terminal, "run-other", &Value::Null), Pinned::Changed);
        assert_eq!(pinned(terminal, "run-1", &json!("other")), Pinned::Changed);
        assert_eq!(pinned(deepseek, "run-2", &json!("session-other")), Pinned::Changed);
        assert_eq!(pinned("claude/example/missing", "run-1", &Value::Null), Pinned::Changed);
        // A held state writer lock: busy, never waiting past the deadline.
        let guard = super::super::storage::lock_until(&writer, soon()).unwrap().unwrap();
        let started = Instant::now();
        assert_eq!(pinned(deepseek, "run-2", &json!("session-2")), Pinned::Busy);
        assert!(started.elapsed() < Duration::from_secs(2));
        // Lock order: waiting for the writer, it holds no provider lock (X4).
        let waiting = std::thread::spawn(|| {
            with_pinned_session_binding(deepseek, "run-2", &json!("session-2"), Instant::now() + Duration::from_secs(5), || "written")
                .unwrap()
        });
        std::thread::sleep(Duration::from_millis(150));
        assert!(free(&provider), "the provider lock is taken only after the writer");
        drop(guard);
        assert_eq!(waiting.join().unwrap(), Pinned::Written("written"));
        // A busy provider lock: busy at once, and the writer is free again (PH).
        let guard = super::super::storage::lock_until(&provider, soon()).unwrap().unwrap();
        let started = Instant::now();
        assert_eq!(pinned(deepseek, "run-2", &json!("session-2")), Pinned::Busy);
        assert!(started.elapsed() < Duration::from_millis(250), "the provider lock is tried once");
        assert!(free(&writer));
        drop(guard);
        // The binding is checked under the lock: a run replaced while the
        // attempt waited for the writer writes nothing (PW, X6).
        let guard = super::super::storage::lock_until(&writer, soon()).unwrap().unwrap();
        let waiting = std::thread::spawn(|| {
            with_pinned_session_binding(terminal, "run-1", &Value::Null, Instant::now() + Duration::from_secs(5), || "written")
                .unwrap()
        });
        std::thread::sleep(Duration::from_millis(150));
        let mut replaced = own.clone();
        replaced["run_id"] = json!("run-replaced");
        save(terminal, replaced);
        drop(guard);
        assert_eq!(waiting.join().unwrap(), Pinned::Changed);
        save(terminal, own);
        assert_eq!(pinned(terminal, "run-1", &Value::Null), Pinned::Written("written"));
    }
}
