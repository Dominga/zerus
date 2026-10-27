//! Complete managed Claude account setup after a successful native sign-in.
//! Claude 2.1.289's browser `auth login` persists OAuth but leaves onboarding
//! unfinished (unlike its refresh-token login path), repeating login in the TUI.
use super::*;
use std::io::Read;

pub(super) fn keychain_service(home: &Path, native: bool) -> String {
    let storage = if native { env::var("CLAUDE_SECURESTORAGE_CONFIG_DIR").ok() } else { None };
    let implicit = storage.as_ref().map_or_else(
        || native && env::var("CLAUDE_CONFIG_DIR").ok().is_none_or(|s| s.is_empty()),
        String::is_empty,
    );
    if implicit { return "Claude Code-credentials".into(); }
    let directory = storage.unwrap_or_else(|| home.to_string_lossy().into_owned());
    format!("Claude Code-credentials-{}", &format!("{:x}", Sha256::digest(directory.as_bytes()))[..8])
}

pub(super) fn missing_login_status(home: &Path, native: bool) -> &'static str {
    if !cfg!(target_os = "macos") { return "signed_out"; }
    // Metadata only: neither the credential nor a password prompt is requested.
    let mut command = Command::new("security");
    command.args(["find-generic-password", "-s", &keychain_service(home, native)]);
    if inspection::capture(command, &[], 3).is_err() { return "signed_out"; }
    if crate::macos_session::keychain_locked() == Some(true) {
        if crate::macos_session::join() { "credentials_locked" } else { "desktop_session_unavailable" }
    } else { "credentials_unavailable" }
}

pub(super) fn saved_identity(home: &Path, native: bool) -> Value {
    let path = if native && env::var("CLAUDE_CONFIG_DIR").ok().is_none_or(|v|v.is_empty()) {
        PathBuf::from(crate::platform::home().unwrap_or_default()).join(".claude.json")
    } else { home.join(".claude.json") };
    let account = fs::File::open(path).ok()
        .and_then(|f| serde_json::from_reader::<_,Value>(f.take(1024*1024)).ok())
        .unwrap_or(Value::Null);
    let mut result = json!({});
    for (source, target) in [("emailAddress", "email"), ("accountUuid", "account_id"), ("displayName", "name")] {
        if let Some(value) = account["oauthAccount"][source].as_str().filter(|v| !v.is_empty()) {
            result[target] = json!(value.chars().filter(|c| !c.is_control()).take(256).collect::<String>());
        }
    }
    result
}

struct ConfigLock(PathBuf);
impl Drop for ConfigLock {
    fn drop(&mut self) { let _ = fs::remove_dir(&self.0); }
}

/// Takes Claude's own lock for `.claude.json` (a directory named
/// `<file>.lock`, the convention its native JSON writer uses) for a bounded
/// wait; a lock another running Claude holds is never broken.
fn config_lock(file: &Path, busy: &str) -> Result<ConfigLock> {
    let mut path = file.as_os_str().to_owned();
    path.push(".lock");
    let path = PathBuf::from(path);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match fs::create_dir(&path) {
            Ok(()) => return Ok(ConfigLock(path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && std::time::Instant::now() < deadline =>
                std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return Err(Error::new(1, busy)),
        }
    }
}

/// The account config file the next Claude launch reads (U16): the native
/// `~/.claude.json` unless `CLAUDE_CONFIG_DIR` names another account's folder.
pub(super) fn account_config() -> Result<PathBuf> {
    if let Some(directory) = env::var("CLAUDE_CONFIG_DIR").ok().filter(|value| !value.is_empty()) {
        return Ok(Path::new(&directory).join(".claude.json"));
    }
    crate::platform::home()
        .filter(|home| !home.is_empty())
        .map(|home| Path::new(&home).join(".claude.json"))
        .ok_or_else(|| Error::new(1, "The user home folder is unknown; the designer's folder was not trusted."))
}

/// Marks one folder trusted in Claude's account config — the project entry its
/// start-up prompt reads. Only that folder changes: another project's answer,
/// its tools, and the account's other settings stay as Claude saved them.
/// Zerus passes only its own private Team designer folder; start-up trust
/// stays manual for every other folder. `false` means it was already trusted.
pub(super) fn trust_folder(file: &Path, folder: &str) -> Result<bool> {
    if folder.is_empty() || folder.chars().any(char::is_control) {
        return Err(Error::new(1, "The Team designer's folder is invalid."));
    }
    let _lock = config_lock(file, "Claude account settings are busy; start the designer again.")?;
    let mut settings = match fs::File::open(file) {
        Ok(handle) => {
            let mut bytes = Vec::new();
            handle.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err(Error::new(1, "Claude account settings are too large; the designer's folder was not trusted."));
            }
            serde_json::from_slice::<Value>(&bytes)
                .map_err(|_| Error::new(1, "Claude account settings are invalid; the designer's folder was not trusted."))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error.into()),
    };
    if !settings.is_object() {
        return Err(Error::new(1, "Claude account settings are invalid; the designer's folder was not trusted."));
    }
    if !settings.get("projects").is_some_and(Value::is_object) {
        settings["projects"] = json!({});
    }
    let entry = &mut settings["projects"][folder];
    if !entry.is_object() {
        *entry = json!({});
    }
    if entry["hasTrustDialogAccepted"] == true {
        return Ok(false);
    }
    entry["hasTrustDialogAccepted"] = json!(true);
    super::claude_settings::write(file, &settings)?;
    Ok(true)
}

pub(super) fn finish_login(home: &Path) -> Result<()> {
    verify_login(home, false)?;
    finish_onboarding(home)
}

pub(super) fn verify_login(home: &Path, native: bool) -> Result<()> {
    // A native status probe is authoritative. Never infer login from a token file
    // or carry native-account state/permissions into an isolated account.
    let mut command = inspection::native_command("claude", home, native);
    command.args(["auth", "status", "--json"]);
    let bytes = inspection::capture_with_exit(command, &[], 5, Some(1))
        .map_err(|_| Error::new(1, "Claude login could not be verified."))?;
    let status: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new(1, "Claude returned an invalid login status."))?;
    if status["loggedIn"] != true || (!native && status["authMethod"] != "claude.ai") {
        let message = match missing_login_status(home, native) {
            "credentials_locked" => "The macOS login Keychain is locked. Unlock it in Keychain Access, then retry sign-in.",
            "desktop_session_unavailable" => "The macOS desktop sign-in session is unavailable. Log in to the Mac desktop and install the HGS desktop session service, then retry.",
            "credentials_unavailable" => "Claude could not use the saved Keychain sign-in. Check this account's Keychain access, then sign in again.",
            _ => "Claude did not confirm subscription sign-in.",
        };
        return Err(Error::new(1, message));
    }
    Ok(())
}

fn finish_onboarding(home: &Path) -> Result<()> {
    let path = home.join(".claude.json");
    let _lock = config_lock(&path, "Claude account setup is busy; finish setup in its terminal.")?;
    let file = platform::no_follow(fs::OpenOptions::new().read(true)).open(&path)?;
    if !file.metadata()?.is_file() { return Err(Error::new(1, "Claude account settings are not a regular file.")); }
    let mut bytes = Vec::new(); file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 { return Err(Error::new(1, "Claude account settings are too large.")); }
    let mut settings: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new(1, "Claude account settings are invalid; left unchanged."))?;
    if !settings.is_object() || !settings["oauthAccount"].is_object() {
        return Err(Error::new(1, "Claude has not saved this account's setup; finish setup in its terminal."));
    }
    if settings["hasCompletedOnboarding"] == true { return Ok(()); }
    settings["hasCompletedOnboarding"] = json!(true);
    let mut stage = tempfile::NamedTempFile::new_in(home)?;
    platform::make_private(stage.as_file())?;
    stage.write_all(&serde_json::to_vec_pretty(&settings).map_err(|e| Error::new(1,e.to_string()))?)?;
    stage.write_all(b"\n")?; stage.as_file().sync_all()?;
    platform::persist(stage, path).map_err(|e| Error::new(1, e.to_string()))?;
    Ok(())
}
