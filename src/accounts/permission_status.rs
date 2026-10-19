//! Read-only account launch defaults. Per-project, managed policies and explicit
//! session switches may still override these; never present them as live state.
use super::*;
use std::io::Read;

fn text(path: &Path) -> Option<String> {
    let mut value = String::new();
    match fs::File::open(path) {
        Ok(file) => { file.take(1024 * 1024 + 1).read_to_string(&mut value).ok()?; }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(String::new()),
        Err(_) => return None,
    }
    (value.len() <= 1024 * 1024).then_some(value)
}
fn native(provider: &str, home: &Path, claude_native: &Path) -> Option<(&'static str, String)> {
    match provider {
        "claude" => {
            let data = claude_settings::preview(claude_native, home).ok()?;
            let mode = data["permissions"]["defaultMode"].as_str().unwrap_or("default");
            Some((match mode { "bypassPermissions" => "bypass", "auto" => "auto", "plan" => "plan", "default" | "acceptEdits" | "dontAsk" => "default", _ => "unknown" }, mode.into()))
        }
        "codex" | "kimi" => {
            let data = text(&home.join("config.toml"))?.parse::<toml_edit::DocumentMut>().ok()?;
            if provider == "kimi" {
                let mode = data.get("default_permission_mode").and_then(|v|v.as_str()).unwrap_or("manual");
                return Some((match mode { "auto" | "yolo" => "bypass", "manual" => "default", _ => "unknown" }, mode.into()));
            }
            let profile = data.get("profile").and_then(|v|v.as_str());
            let get = |key: &str| profile.and_then(|p|data.get("profiles")?.get(p)?.get(key)?.as_str())
                .or_else(||data.get(key).and_then(|v|v.as_str()));
            let approval = get("approval_policy").unwrap_or("on-request");
            let sandbox = get("sandbox_mode").unwrap_or("read-only");
            Some((if approval == "never" && sandbox == "danger-full-access" { "bypass" } else { "default" },
                format!("Approvals: {approval}; sandbox: {sandbox}")))
        }
        _ => None,
    }
}
pub(super) fn describe(config: &Config, row: &mut Value, mode: &str) {
    row["permission_mode"] = json!(mode);
    let (effective, detail) = if let Some(set) = ["bypass", "auto"].into_iter().find(|set| *set == mode) {
        (set, "Set in Zerus".into())
    } else {
        native(string_field(row,"provider"), Path::new(string_field(row,"home")),
            &if row["native"].as_bool().unwrap_or(false) { PathBuf::from(string_field(row,"home")) } else { Path::new(&config.home).join(".claude") })
            .unwrap_or(("unknown", "Provider settings could not be verified".into()))
    };
    row["effective_permission_mode"] = json!(effective);
    row["permission_detail"] = json!(detail);
}
fn string_field<'a>(v: &'a Value, key: &str) -> &'a str { v[key].as_str().unwrap_or("") }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_provider_defaults_without_exposing_or_changing_credentials() {
        let tmp = tempfile::tempdir().unwrap(); let home = tmp.path();
        let path = home.join("config.toml");
        fs::write(&path,"approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\napi_key = 'secret'\n").unwrap();
        assert_eq!(native("codex",home,home).unwrap().0,"bypass");
        fs::write(&path,"approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\nprofile = 'safe'\n[profiles.safe]\nsandbox_mode = 'workspace-write'\n").unwrap();
        assert_eq!(native("codex",home,home).unwrap().0,"default");
        for mode in ["manual", "yolo", "auto"] {
            fs::write(&path,format!("default_permission_mode = '{mode}'")).unwrap();
            assert_eq!(native("kimi",home,home).unwrap().0,if mode=="manual" {"default"} else {"bypass"});
        }
        fs::write(&path,"broken = [").unwrap(); assert!(native("codex",home,home).is_none());
        let path = home.join("settings.json");
        fs::write(&path,r#"{"permissions":{"defaultMode":"bypassPermissions"},"apiKey":"secret"}"#).unwrap();
        assert_eq!(native("claude",home,home).unwrap(),("bypass","bypassPermissions".into()));
        let bytes = fs::read(&path).unwrap();
        assert!(native("dsh",home,home).is_none()); assert_eq!(fs::read(&path).unwrap(),bytes);
    }
}
