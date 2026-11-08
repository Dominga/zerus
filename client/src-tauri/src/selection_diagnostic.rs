//! Opt-in acceptance readback from the actual webview selection. Never an
//! input to navigation, ownership, notification admission, or a service route.
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Session {
    computer: String,
    name: String,
    run_id: Option<String>,
    conversation_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    sequence: u64,
    page: String,
    detail_tab: String,
    session: Option<Session>,
}

pub(crate) fn enabled() -> bool {
    cfg!(target_os = "macos")
        && std::env::var("ZERUS_CLIENT_ACCEPTANCE_SELECTION").as_deref() == Ok("1")
}

pub(crate) fn record(value: Observation) -> Result<(), String> {
    if !enabled() {
        return Err("selection readback disabled".into());
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or("selection readback requires private XDG_STATE_HOME")?;
    write(&base, value).map_err(|_| "selection readback unavailable".into())
}

fn private_directory(path: &Path) -> std::io::Result<()> {
    let m = std::fs::symlink_metadata(path)?;
    if !m.is_dir() || path.canonicalize()? != path {
        return Err(std::io::Error::other("private directory required"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no arguments or side effects.
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
            return Err(std::io::Error::other("private directory required"));
        }
    }
    Ok(())
}

fn write(base: &Path, value: Observation) -> std::io::Result<()> {
    let bounded = |s: &str| s.len() <= 256 && !s.chars().any(char::is_control);
    let valid = [
        "dashboard",
        "sessions",
        "projects",
        "machines",
        "accounts",
        "settings",
    ]
    .contains(&value.page.as_str())
        && ["activity", "terminal", "native", "processes", "taskview"]
            .contains(&value.detail_tab.as_str())
        && value.session.as_ref().is_none_or(|s| {
            bounded(&s.computer)
                && bounded(&s.name)
                && s.run_id.as_deref().is_none_or(bounded)
                && s.conversation_id.as_deref().is_none_or(bounded)
        });
    if !valid {
        return Err(std::io::Error::other("invalid selection observation"));
    }
    let body = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "recorded_at_unix_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?.as_millis(),
        "source_commit": option_env!("ZERUS_SOURCE_COMMIT"),
        "observation": value,
    }))?;
    if body.len() > 2048 {
        return Err(std::io::Error::other("selection observation limit"));
    }
    private_directory(base)?;
    let folder = base.join("hgs");
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut dir = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        dir.mode(0o700);
    }
    match dir.create(&folder) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    private_directory(&folder)?;
    let temporary = folder.join(format!(".selection-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&temporary)?;
        f.write_all(&body)?;
        f.sync_all()?;
        std::fs::rename(&temporary, folder.join("client-selection-diagnostic.json"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn observation() -> Observation {
        serde_json::from_value(serde_json::json!({"sequence":1,"page":"sessions","detail_tab":"activity",
            "session":{"computer":"","name":"codex/fixture","run_id":"run-fixture","conversation_id":"conversation-fixture"}})).unwrap()
    }
    #[test]
    fn actual_selection_is_private_bounded_and_replaced_without_following() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("selection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        write(&root, observation()).unwrap();
        let path = root.join("hgs/client-selection-diagnostic.json");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(std::fs::metadata(&path).unwrap().len() <= 2048);
        let data: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(data["observation"]["session"]["run_id"], "run-fixture");
        let kept = root.join("kept");
        std::fs::write(&kept, "unchanged").unwrap();
        std::fs::remove_file(&path).unwrap();
        symlink(&kept, &path).unwrap();
        let mut empty = observation();
        empty.session = None;
        write(&root, empty).unwrap();
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "unchanged");
        let mut huge = observation();
        huge.session.as_mut().unwrap().name = "x".repeat(257);
        assert!(write(&root, huge).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn foreign_directory_and_unknown_or_control_fields_refuse() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("selection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(write(&root, observation()).is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        symlink(&elsewhere, root.join("hgs")).unwrap();
        assert!(write(&root, observation()).is_err());
        let mut v = serde_json::to_value(observation()).unwrap();
        v["permission"] = true.into();
        assert!(serde_json::from_value::<Observation>(v).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
