//! Account-level OpenCode 2 configuration, never a running session's policy.
//! Upstream: packages/core/src/config/{discovery,normalize}.ts and
//! packages/util/src/global.ts at anomalyco/opencode 1a9751ad2e51.
use super::*;

const FILE_LIMIT: u64 = 1024 * 1024;
const TOTAL_LIMIT: u64 = 4 * FILE_LIMIT;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Policy {
    Default,
    Allow,
    Ask,
    Deny,
    Custom,
}

impl Policy {
    fn describe(self) -> (&'static str, String) {
        match self {
            Self::Default => ("allow", "OpenCode native default".into()),
            Self::Allow => ("allow", "OpenCode configured permissions".into()),
            Self::Ask => ("ask", "OpenCode configured permissions".into()),
            Self::Deny => ("deny", "OpenCode configured permissions".into()),
            Self::Custom => ("custom", "OpenCode configured permissions".into()),
        }
    }
    fn rule(&mut self, action: &str, resource: &str, effect: &str) -> Option<()> {
        if [action, resource, effect]
            .iter()
            .any(|s| s.contains("{env:") || s.contains("{file:"))
        {
            return None;
        }
        let effect = match effect {
            "allow" => Self::Allow,
            "ask" => Self::Ask,
            "deny" => Self::Deny,
            _ => return None,
        };
        if action == "*" && resource == "*" {
            *self = effect;
        } else if *self != effect && !(*self == Self::Default && effect == Self::Allow) {
            *self = Self::Custom;
        }
        Some(())
    }
}

/// Strip comments and trailing commas without treating quoted URLs or escaped
/// quotes as syntax. Parsing remains bounded by serde_json's depth limit.
fn jsonc(mut bytes: Vec<u8>) -> Option<Value> {
    let mut at = 0;
    let mut quoted = false;
    while at < bytes.len() {
        if bytes[at] == b'"' {
            quoted = !quoted;
            at += 1;
            continue;
        }
        if quoted && bytes[at] == b'\\' {
            at += 2;
            continue;
        }
        if !quoted && bytes.get(at..at + 2) == Some(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                bytes[at] = b' ';
                at += 1;
            }
        } else if !quoted && bytes.get(at..at + 2) == Some(b"/*") {
            bytes[at] = b' ';
            bytes[at + 1] = b' ';
            at += 2;
            while at + 1 < bytes.len() && bytes.get(at..at + 2) != Some(b"*/") {
                bytes[at] = b' ';
                at += 1;
            }
            if at + 1 >= bytes.len() {
                return None;
            }
            bytes[at] = b' ';
            bytes[at + 1] = b' ';
            at += 2;
        } else {
            at += 1;
        }
    }
    quoted = false;
    at = 0;
    while at < bytes.len() {
        if bytes[at] == b'"' {
            quoted = !quoted;
        } else if quoted && bytes[at] == b'\\' {
            at += 2;
            continue;
        } else if !quoted && bytes[at] == b',' {
            let next = bytes[at + 1..].iter().find(|b| !b.is_ascii_whitespace());
            if matches!(next, Some(b']' | b'}')) {
                bytes[at] = b' ';
            }
        }
        at += 1;
    }
    let data = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
    let value: Value = serde_json::from_slice(data).ok()?;
    value.is_object().then_some(value)
}

fn read(path: &Path, remaining: &mut u64) -> Option<Option<Value>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
        Ok(_) => {}
    }
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT.min(*remaining) + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > FILE_LIMIT.min(*remaining) {
        return None;
    }
    *remaining -= bytes.len() as u64;
    Some(Some(jsonc(bytes)?))
}

fn legacy(policy: &mut Policy, value: &Value) -> Option<()> {
    if let Some(effect) = value.as_str() {
        return policy.rule("*", "*", effect);
    }
    let map = value.as_object()?;
    // V1 object order is policy. serde_json's maps sort keys, so preserve
    // uncertainty for a multi-key object instead of inventing its last match.
    for (action, rule) in map {
        if let Some(effect) = rule.as_str() {
            policy.rule(action, "*", effect)?;
        } else {
            let resources = rule.as_object()?;
            for (resource, effect) in resources {
                policy.rule(action, resource, effect.as_str()?)?;
            }
            if resources.len() > 1 {
                *policy = Policy::Custom;
            }
        }
    }
    if map.len() > 1 {
        *policy = Policy::Custom;
    }
    Some(())
}

fn apply(policy: &mut Policy, value: &Value) -> Option<()> {
    if let Some(tools) = value.get("tools") {
        for (action, enabled) in tools.as_object()? {
            policy.rule(
                action,
                "*",
                if enabled.as_bool()? { "allow" } else { "deny" },
            )?;
        }
        if tools.as_object()?.len() > 1 {
            *policy = Policy::Custom;
        }
    }
    if let Some(permission) = value.get("permission") {
        legacy(policy, permission)?;
    }
    if let Some(permissions) = value.get("permissions") {
        let rules = permissions.as_array()?;
        if rules.len() > 1024 {
            return None;
        }
        for rule in rules {
            policy.rule(
                rule["action"].as_str()?,
                rule["resource"].as_str()?,
                rule["effect"].as_str()?,
            )?;
        }
    }
    // These can add authority beyond the account's top-level tool rules. Do
    // not turn an unexpanded managed/agent policy into an apparent blanket allow.
    for rules in value
        .get("policies")
        .into_iter()
        .chain(value.get("experimental").and_then(|v| v.get("policies")))
    {
        if !rules.as_array()?.is_empty() {
            return None;
        }
    }
    for key in ["agents", "agent", "mode"] {
        if let Some(agents) = value.get(key) {
            for agent in agents.as_object()?.values() {
                let agent = agent.as_object()?;
                if agent.get("permission").is_some()
                    || agent.get("permissions").is_some()
                    || agent.get("tools").is_some()
                {
                    return None;
                }
            }
        }
    }
    if let Some(agent) = value.get("default_agent") {
        if agent.as_str()? != "build" {
            return None;
        }
    }
    Some(())
}

fn inspect(paths: &[PathBuf], managed: &[PathBuf]) -> Option<(&'static str, String)> {
    for path in managed {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return None, // A deployed plist needs OpenCode's managed resolver.
        }
    }
    let mut remaining = TOTAL_LIMIT;
    let mut policy = Policy::Default;
    for path in paths {
        if !path.is_absolute() {
            return None;
        }
        if let Some(value) = read(path, &mut remaining)? {
            apply(&mut policy, &value)?;
        }
    }
    Some(policy.describe())
}

pub(super) fn native(home: &Path) -> Option<(&'static str, String)> {
    let mut paths = vec![home.join("opencode.json"), home.join("opencode.jsonc")];
    if let Some(path) = env::var_os("OPENCODE_CONFIG").filter(|p| !p.is_empty()) {
        paths.push(path.into());
    }
    #[cfg(target_os = "macos")]
    let system = PathBuf::from("/Library/Application Support/opencode");
    #[cfg(windows)]
    let system = env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("opencode");
    #[cfg(not(any(target_os = "macos", windows)))]
    let system = PathBuf::from("/etc/opencode");
    paths.extend([system.join("opencode.json"), system.join("opencode.jsonc")]);
    #[cfg(target_os = "macos")]
    let managed = {
        use std::{
            ffi::{CStr, OsStr},
            os::unix::ffi::OsStrExt,
        };
        let mut buffer = vec![0_u8; 16384];
        let mut user = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        // Same effective user's username as OpenCode's os.userInfo(); no shell.
        let status = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                user.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status != 0 || result.is_null() {
            return None;
        }
        let user = unsafe { user.assume_init() };
        if user.pw_name.is_null() {
            return None;
        }
        let name = unsafe { CStr::from_ptr(user.pw_name) }.to_bytes();
        let root = Path::new("/Library/Managed Preferences");
        vec![
            root.join(OsStr::from_bytes(name))
                .join("ai.opencode.managed.plist"),
            root.join("ai.opencode.managed.plist"),
        ]
    };
    #[cfg(not(target_os = "macos"))]
    let managed = Vec::new();
    inspect(&paths, &managed)
}

#[cfg(test)]
mod tests;
