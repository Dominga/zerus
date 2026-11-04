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
/// Runs `write` while `name` is still the binding of run `run_id` with
/// conversation `conversation`: a fork's inherited project, written after its
/// launch released the state lock (zerus-wx3k). The locks and their order are
/// a project move's: the state writer lock, then the provider's binding lock.
/// It waits for them no later than `deadline`. `None` when they stayed busy or
/// the binding changed. `write` must not wait for another lock.
pub(crate) fn with_pinned_session_binding<T>(
    name: &str,
    run_id: &str,
    conversation: &Value,
    deadline: std::time::Instant,
    write: impl FnOnce() -> T,
) -> anyhow::Result<Option<T>> {
    let Some(_writer) = super::storage::writer_until(deadline).map_err(anyhow::Error::msg)? else {
        return Ok(None);
    };
    let (provider, path) = binding_location(name);
    let _provider = match provider {
        Some(provider) => {
            let lock = root().join(provider).join("bindings.lock");
            match super::storage::lock_until(&lock, deadline).map_err(anyhow::Error::msg)? {
                Some(guard) => Some(guard),
                None => return Ok(None),
            }
        }
        None => None,
    };
    let Ok(record) = binding_metadata(&path, name, provider) else {
        return Ok(None);
    };
    if string(&record, "run_id") != run_id || record["conversation_id"] != *conversation {
        return Ok(None);
    }
    Ok(Some(write()))
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
        let terminal = "claude/example/fork";
        let deepseek = "dsh/example/fork";
        let (_, path) = binding_location(terminal);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            json!({"version":1,"agent":"claude","name":terminal,"run_id":"run-1","conversation_id":null}).to_string(),
        )
        .unwrap();
        let (_, path) = binding_location(deepseek);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            json!({"backend":"dsh","name":deepseek,"run_id":"run-2","conversation_id":"session-2"}).to_string(),
        )
        .unwrap();
        assert_eq!(pinned(terminal, "run-1", &Value::Null), Some("written"));
        assert_eq!(pinned(deepseek, "run-2", &json!("session-2")), Some("written"));
        // A replaced run, another conversation or no binding: nothing is written.
        assert_eq!(pinned(terminal, "run-other", &Value::Null), None);
        assert_eq!(pinned(terminal, "run-1", &json!("other")), None);
        assert_eq!(pinned(deepseek, "run-2", &json!("session-other")), None);
        assert_eq!(pinned("claude/example/missing", "run-1", &Value::Null), None);
        // A held state writer lock, or the provider's binding lock: no wait past the deadline.
        for (held, name, run, conversation) in [
            (root().join(".lock"), terminal, "run-1", Value::Null),
            (root().join("dsh/bindings.lock"), deepseek, "run-2", json!("session-2")),
        ] {
            let guard = super::super::storage::lock_until(&held, soon()).unwrap().unwrap();
            let started = Instant::now();
            assert_eq!(pinned(name, run, &conversation), None);
            assert!(started.elapsed() < Duration::from_secs(2));
            drop(guard);
            assert_eq!(pinned(name, run, &conversation), Some("written"));
        }
    }
}
