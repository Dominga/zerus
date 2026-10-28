//! The build's identity, shared by `build.rs` and the crate's tests: Qt's
//! rules in `tray/cmake/BuildVersion.cmake` and `tray/CMakeLists.txt:120-139`.

use std::path::Path;

/// `MAJOR.MINOR.PATCH` without leading zeros, as `VERSION` and `DEV_VERSION` hold it.
pub fn release_version(raw: &str) -> Option<String> {
    let value = raw.trim();
    let parts: Vec<&str> = value.split('.').collect();
    let valid = parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        });
    valid.then(|| value.to_owned())
}

fn numbers(version: &str) -> Vec<u64> {
    version
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect()
}

/// `DEV_VERSION` names a release newer than `VERSION` (`BuildVersion.cmake:4-7`).
pub fn check_dev_base(version: &str, dev_base: &str) -> Result<(), String> {
    if numbers(dev_base) > numbers(version) {
        Ok(())
    } else {
        Err(DEV_BASE_ERROR.to_owned())
    }
}

/// CMake's one message for a malformed or not newer `DEV_VERSION`.
pub const DEV_BASE_ERROR: &str =
    "DEV_VERSION must name a release newer than the stable product version";

/// `ZERUS_DEV_BUILD_NUMBER`: empty, or a positive integer of at most 15
/// digits (`BuildVersion.cmake:22-26`). CMake takes the variable as it is, so
/// surrounding blanks are refused.
pub fn dev_number(raw: &str) -> Result<Option<u64>, String> {
    if raw.is_empty() {
        return Ok(None);
    }
    let valid =
        raw.len() <= 15 && raw.bytes().all(|byte| byte.is_ascii_digit()) && !raw.starts_with('0');
    if !valid {
        return Err(
            "ZERUS_DEV_BUILD_NUMBER must be a positive integer of at most 15 digits".to_owned(),
        );
    }
    raw.parse()
        .map(Some)
        .map_err(|error| format!("ZERUS_DEV_BUILD_NUMBER: {error}"))
}

/// The source commit: a 40-character lowercase hex HEAD, only when Git's top
/// level is this repository (not a parent checkout).
pub fn source_commit(head: &str, toplevel: &str, root: &str) -> Option<String> {
    let head = head.trim();
    let valid = head.len() == 40
        && head
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    (valid && toplevel.trim() == root).then(|| head.to_owned())
}

/// A numbered Dev build needs a clean source tree (`BuildVersion.cmake:27-29`).
pub fn check_numbered(
    number: Option<u64>,
    commit: Option<&str>,
    dirty: Option<bool>,
) -> Result<(), String> {
    if number.is_some() && !(commit.is_some() && dirty == Some(false)) {
        Err("A numbered Dev build requires a clean Git source tree".to_owned())
    } else {
        Ok(())
    }
}

/// `ZERUS_LOCAL_PREVIEW_LABEL`: empty, or at most 32 lowercase letters and
/// digits in hyphen-separated words (`BuildVersion.cmake:10-14`).
pub fn preview_label(raw: &str) -> Result<Option<String>, String> {
    if raw.is_empty() {
        return Ok(None);
    }
    let valid = raw.len() <= 32
        && raw.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });
    if valid {
        Ok(Some(raw.to_owned()))
    } else {
        Err("A local preview label must contain at most 32 lowercase letters, digits or separated hyphens".to_owned())
    }
}

/// A local branch preview has no Dev number and a clean source tree (`BuildVersion.cmake:15-17`).
pub fn check_preview(
    label: Option<&str>,
    number: Option<u64>,
    commit: Option<&str>,
    dirty: Option<bool>,
) -> Result<(), String> {
    if label.is_some() && (number.is_some() || !(commit.is_some() && dirty == Some(false))) {
        Err("A local branch preview requires clean Git source and no Dev build number".to_owned())
    } else {
        Ok(())
    }
}

/// The path itself, or while it does not exist (a packed ref) the nearest
/// folder above it that does: cargo reruns on every build for a missing path,
/// and the folder changes when git writes the ref loose again.
pub fn watched(path: &Path) -> &Path {
    path.ancestors().find(|path| path.exists()).unwrap_or(path)
}
