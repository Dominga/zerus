//! Explicit installer operation; the runtime never creates or repairs registry state.
use super::wire;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(windows)]
mod native;
#[cfg(any(windows, test))]
mod removal;
#[cfg(test)]
mod tests;

pub(super) const CLASS_ID: &str = "{0F0F70A2-1D40-47D8-AC5E-D6A9B789605F}";
const APPLICATION_KEY: &str = r"Software\Classes\AppUserModelId\com.hgdev.hgs-tray";
const ABI_VALUE: &str = "HGSDesktopNotificationABI";
const CONFIGURATION_VALUE: &str = "HGSDesktopConfiguration";
pub(in crate::desktop) const MARKER: &str = "desktop-notification-registration-v1";
/// The fixed Toast activation flag. Cold activation parses only this flag and
/// cannot acquire or promote the canonical service owner.
pub(in crate::desktop) const ACTIVATION_ENTRY: &str = "--desktop-notification-activation";

/// The one LocalServer32 command for an image and its configuration. Explicit
/// registration writes it; the service compares the registry with it exactly.
/// It matches the registered command path and arguments; it is not a content
/// or Grant proof. Each path is an absolute Windows path without quotes or
/// control characters, and the command stays below the registry bound.
pub(in crate::desktop) fn expected_command(
    image: &str,
    config: &str,
    state: &str,
) -> Result<String, &'static str> {
    if ![image, config, state]
        .iter()
        .all(|path| absolute(path) && !path.contains('"') && !path.chars().any(char::is_control))
    {
        return Err("registration_command_invalid");
    }
    let mut command = format!("\"{image}\"");
    for argument in [
        ACTIVATION_ENTRY,
        "--config-dir",
        config,
        "--state-dir",
        state,
    ] {
        command.push(' ');
        command.push_str(&crate::host::spawn::quote_argument(argument));
    }
    if command.encode_utf16().count() >= 32767 {
        return Err("registration_command_too_large");
    }
    Ok(command)
}

/// Diagnostic only: the registered command carries exactly the expected
/// arguments for ANOTHER quoted image that is itself a valid absolute path.
/// Anything malformed stays unavailable. Never a selection, permission or repair.
pub(in crate::desktop) fn names_other_image(registered: &str, expected: &str, image: &str) -> bool {
    let Some(arguments) = expected.strip_prefix(&format!("\"{image}\"")) else {
        return false;
    };
    registered
        .strip_prefix('"')
        .and_then(|rest| rest.split_once('"'))
        .is_some_and(|(other, rest)| {
            absolute(other)
                && !other.chars().any(char::is_control)
                && other != image
                && rest == arguments
        })
}

/// Exact provenance of an earlier image's registration of THIS configuration
/// and state (zerus-5wmh.66.3.30.8.5.1). The registered command must be,
/// byte for byte, what this build writes for one other absolute image with the
/// current configuration and state folders. Unlike `names_other_image`, this
/// is the only accepted parse. The image stays a string; it is never launched,
/// opened or trusted.
pub(in crate::desktop) fn previous_image(
    registered: &str,
    image: &str,
    config: &str,
    state: &str,
) -> Option<String> {
    let (other, _) = registered.strip_prefix('"')?.split_once('"')?;
    if other == image {
        return None;
    }
    (expected_command(other, config, state).ok()? == registered).then(|| other.to_owned())
}

/// An absolute Windows path: a drive root, or a UNC or verbatim prefix.
fn absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || path.starts_with(r"\\")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Contents {
    values: BTreeMap<String, String>,
    children: BTreeSet<String>,
}
#[derive(Clone)]
struct Plan {
    configuration: String,
    sid: String,
    keys: Vec<(String, Contents)>,
}
fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}
impl Plan {
    fn new(configuration: &str, sid: &str, command: &str) -> Result<Self, &'static str> {
        if configuration.len() != 64
            || !configuration
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !sid.starts_with("S-1-")
            || sid.len() > 184
            || !sid
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'S' | b'-'))
            || matches!(sid, "S-1-5-18" | "S-1-5-19" | "S-1-5-20")
            || sid.starts_with("S-1-5-80-")
            || command.is_empty()
            || command.encode_utf16().count() >= 32767
            || command.chars().any(char::is_control)
        {
            return Err("notification_registration_plan_refused");
        }
        let class = format!(r"Software\Classes\CLSID\{CLASS_ID}");
        // Advertise the AUMID last; no worker or background process is started.
        Ok(Self {
            configuration: configuration.into(),
            sid: sid.into(),
            keys: vec![
                (
                    class.clone(),
                    Contents {
                        values: values(&[
                            (ABI_VALUE, wire::ABI),
                            (CONFIGURATION_VALUE, configuration),
                        ]),
                        children: ["LocalServer32".into()].into_iter().collect(),
                    },
                ),
                (
                    format!(r"{class}\LocalServer32"),
                    Contents {
                        values: values(&[("", command)]),
                        children: BTreeSet::new(),
                    },
                ),
                (
                    APPLICATION_KEY.into(),
                    Contents {
                        values: values(&[
                            (ABI_VALUE, wire::ABI),
                            (CONFIGURATION_VALUE, configuration),
                            ("CustomActivator", CLASS_ID),
                            ("DisplayName", "Zerus"),
                        ]),
                        children: BTreeSet::new(),
                    },
                ),
            ],
        })
    }
    /// The exact Plan of an earlier image's registration of this configuration
    /// and state, when every key matches what this build writes for that image:
    /// the same SID and configuration hash, this build's ABI marker, the fixed
    /// activator and display name, and no extra value or key. Anything else is
    /// None, and the current Plan's own refusal stands.
    fn previous(
        &self,
        registry: &mut impl Registry,
        image: &str,
        config: &str,
        state: &str,
    ) -> Result<Option<Plan>, &'static str> {
        let Some(server) =
            registry.read(&format!(r"Software\Classes\CLSID\{CLASS_ID}\LocalServer32"))?
        else {
            return Ok(None);
        };
        let Some(registered) = server.values.get("") else {
            return Ok(None);
        };
        if previous_image(registered, image, config, state).is_none() {
            return Ok(None);
        }
        let Ok(previous) = Plan::new(&self.configuration, &self.sid, registered) else {
            return Ok(None);
        };
        Ok(matches!(previous.inspect(registry), Ok(true)).then_some(previous))
    }
    fn inspect(&self, registry: &mut impl Registry) -> Result<bool, &'static str> {
        let mut present = 0;
        for (path, wanted) in &self.keys {
            if let Some(actual) = registry.read(path)? {
                if &actual != wanted {
                    return Err("foreign_or_incomplete_notification_registration_kept");
                }
                present += 1;
            }
        }
        if present != 0 && present != self.keys.len() {
            return Err("incomplete_notification_registration_kept");
        }
        Ok(present != 0)
    }
    fn apply(&self, registry: &mut impl Registry, operation: &str) -> Result<Reply, Failure> {
        if !matches!(operation, "status" | "install" | "remove") {
            return Err(Failure::before(
                "invalid_notification_registration_operation",
            ));
        }
        let registered = self.inspect(registry).map_err(Failure::before)?;
        if operation == "status" || (operation == "install") == registered {
            return Ok(Reply {
                registered,
                changed: false,
            });
        }
        // Native create is create-only. Each removal snapshots the opened key, then
        // deletes its fixed path. Failure never rolls back or deletes evidence.
        if operation == "install" {
            for (path, contents) in &self.keys {
                registry
                    .create_new(path, &contents.values)
                    .map_err(Failure::uncertain)?;
            }
        } else {
            for (path, contents) in self.keys.iter().rev() {
                let mut expected = contents.clone();
                expected.children.clear();
                registry
                    .remove_exact(path, &expected)
                    .map_err(Failure::uncertain)?;
            }
        }
        let after = self.inspect(registry).map_err(Failure::uncertain)?;
        if after != (operation == "install") {
            return Err(Failure::uncertain(
                "notification_registration_readback_refused",
            ));
        }
        Ok(Reply {
            registered: after,
            changed: true,
        })
    }
}
#[derive(Debug, PartialEq, Eq)]
struct Reply {
    registered: bool,
    changed: bool,
}
#[derive(Debug, PartialEq, Eq)]
struct Failure {
    reason: &'static str,
    uncertain: bool,
}
impl Failure {
    fn before(reason: &'static str) -> Self {
        Self {
            reason,
            uncertain: false,
        }
    }
    fn uncertain(reason: &'static str) -> Self {
        Self {
            reason,
            uncertain: true,
        }
    }
}
trait Registry {
    fn read(&mut self, path: &str) -> Result<Option<Contents>, &'static str>;
    fn create_new(
        &mut self,
        path: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<(), &'static str>;
    fn remove_exact(&mut self, path: &str, contents: &Contents) -> Result<(), &'static str>;
}

/// The removal guard as the execution sees it: on Windows the offline
/// exclusion with its current rollback proof; in tests a fake.
trait Gate {
    /// Right before the registry effect.
    fn admit(&self) -> anyhow::Result<()>;
    /// After the effect; a failure leaves its outcome unknown.
    fn confirm(&self) -> anyhow::Result<()>;
}

/// The Plan an operation acts on, fixed under the registration lock before
/// any guard or effect: the current image's, or, for `remove` only after a
/// fully read current-image mismatch, the exact earlier image's of the same
/// folders (zerus-5wmh.66.3.30.8.5.1). Install and status never fall back.
fn select(
    plan: &Plan,
    registry: &mut impl Registry,
    operation: &str,
    image: &str,
    config: &str,
    state: &str,
) -> Result<(Plan, bool), &'static str> {
    const MISMATCH: &str = "foreign_or_incomplete_notification_registration_kept";
    match plan.inspect(registry) {
        Ok(_) => Ok((plan.clone(), false)),
        Err(MISMATCH) if operation == "remove" => {
            match plan.previous(registry, image, config, state)? {
                Some(previous) => Ok((previous, true)),
                None => Err(MISMATCH),
            }
        }
        Err(reason) => Err(reason),
    }
}

/// One explicit operation under the held registration lock: select the Plan,
/// obtain the guard (`guard` refuses before any effect), admit, apply the
/// unchanged Plan (which inspects its exact contents again first), confirm.
/// Nothing is retried, and no other Plan is tried after an effect.
fn run<G: Gate>(
    plan: &Plan,
    registry: &mut impl Registry,
    operation: &str,
    (image, config, state): (&str, &str, &str),
    guard: impl FnOnce() -> anyhow::Result<Option<G>>,
) -> anyhow::Result<(Reply, bool)> {
    let (selected, previous) = select(plan, registry, operation, image, config, state)
        .map_err(|reason| anyhow::anyhow!(reason))?;
    let gate = guard()?;
    if let Some(gate) = &gate {
        gate.admit()?;
    }
    let reply = selected.apply(registry, operation).map_err(|failure| {
        anyhow::anyhow!(
            "{}{}",
            failure.reason,
            if failure.uncertain {
                "; outcome unknown, inspect status before retrying"
            } else {
                "; registration kept"
            }
        )
    })?;
    if let Some(gate) = &gate {
        gate.confirm()?;
    }
    Ok((reply, previous))
}

#[cfg(windows)]
pub(in crate::desktop) fn command(
    config: &crate::config::Config,
    operation: &str,
    request_id: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    native::command(config, operation, request_id)
}

// Called only for the response read from the selected authenticated service;
// caller JSON is never an admission source. Keep the original status epoch.
fn settled_revoke(value: &serde_json::Value, id: &str, configuration: &str, epoch: &str) -> bool {
    zerus_desktop_migration::canonical_uuid(id)
        && zerus_desktop_migration::canonical_uuid(epoch)
        && value["request_id"].as_str() == Some(id)
        && value["configuration"].as_str() == Some(configuration)
        && value["writer_epoch"].as_str() == Some(epoch)
        && ["accepted", "durable", "revoked", "checkpointed"]
            .iter()
            .all(|field| value[*field] == true)
}
