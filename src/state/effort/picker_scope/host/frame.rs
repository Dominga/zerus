//! Closed data frame, never a command, callback, permission or replay token.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};

pub(super) const FRAME_BYTES: usize = 48 << 10;
const REQUEST_BYTES: usize = 4096;
const MAX_BUDGET: u64 = 30_000_000_000;
type RootObject = (u64, u64, u32, u32, u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Operation {
    Effort,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Image {
    pub(super) file: (u64, u64, i64, i64),
}
impl Image {
    pub(super) fn own() -> Result<Self> {
        let image =
            crate::platform::image_identity().ok_or("Original picker host image unavailable")?;
        Ok(Self {
            file: (
                image.device,
                image.inode,
                image.modified,
                image.modified_nanos,
            ),
        })
    }
    pub(super) fn check(&self) -> Result<()> {
        if Self::own()? != *self {
            return Err("Original picker host image changed".into());
        }
        Ok(())
    }
}

pub(super) fn monotonic() -> Result<u64> {
    let mut clock: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut clock) } != 0
        || clock.tv_sec < 0
        || clock.tv_nsec < 0
    {
        return Err("Original picker monotonic clock unavailable".into());
    }
    (clock.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|v| v.checked_add(clock.tv_nsec as u64))
        .ok_or("Original picker clock overflow".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Deadline {
    pub(super) nanos: u64,
}
impl Deadline {
    pub(super) fn after(duration: Duration) -> Result<Self> {
        let budget = u64::try_from(duration.as_nanos()).map_err(|_| "Invalid picker budget")?;
        if budget == 0 || budget > MAX_BUDGET {
            return Err("Invalid original picker budget".into());
        }
        Ok(Self {
            nanos: monotonic()?
                .checked_add(budget)
                .ok_or("Picker deadline overflow")?,
        })
    }
    pub(super) fn remaining(&self) -> Result<Duration> {
        let now = monotonic()?;
        let remaining = self
            .nanos
            .checked_sub(now)
            .filter(|v| *v > 0 && *v <= MAX_BUDGET)
            .ok_or("Original picker deadline expired or invalid")?;
        Ok(Duration::from_nanos(remaining))
    }
    pub(super) fn instant(&self) -> Result<Instant> {
        // Snapshot Instant BEFORE the native clock query: conversion can only
        // shorten this original deadline, never renew it after self-exec.
        let instant = Instant::now();
        instant
            .checked_add(self.remaining()?)
            .ok_or("Invalid picker deadline".into())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Frame {
    pub(super) schema: u8,
    pub(super) nonce: String,
    pub(super) image: Image,
    pub(super) deadline: Deadline,
    pub(super) configuration: String,
    pub(super) root: String,
    pub(super) root_object: RootObject,
    pub(super) operation: Operation,
    /// Original stdin, byte-for-byte. Never deserialize and reserialize it for
    /// the existing receipt digest or an immutable original request identity.
    pub(super) raw: Vec<u8>,
    pub(super) request_id: String,
    /// Only original record binding fields, preserving absent versus null.
    pub(super) binding: Value,
}

fn canonical_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok_and(|u| !u.is_nil() && u.to_string() == s)
}
fn bounded(s: &str, max: usize, empty: bool) -> bool {
    (empty || !s.is_empty()) && s.len() <= max && !s.chars().any(char::is_control)
}
pub(super) fn namespace() -> Result<(String, RootObject)> {
    let path = crate::platform::canonical(root()).map_err(|e| e.to_string())?;
    let directory = Directory::open(&path)?;
    if !directory.private()? {
        return Err("Original picker state is not private".into());
    }
    let file = directory.descriptor();
    let mut m: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(file, &mut m) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok((
        path.to_str().ok_or("Picker root is not UTF-8")?.into(),
        (
            m.st_dev as u64,
            m.st_ino as u64,
            m.st_mode as u32,
            m.st_uid,
            m.st_gid,
        ),
    ))
}

pub(super) fn configuration() -> Result<String> {
    let home = crate::platform::home().ok_or("Picker configuration has no home")?;
    let dir = crate::config::config_dir(&home);
    let location =
        zerus_desktop_migration::configuration(&root(), &dir).map_err(|e| e.to_string())?;
    let mut env = std::collections::BTreeMap::new();
    let mut size = 0usize;
    for (key, value) in std::env::vars_os() {
        let Some(name) = key.to_str() else { continue };
        if matches!(
            name,
            "HGS_PICKER_HOST_FD" | "HGS_PICKER_HOST_SLOT" | "HGS_PICKER_HOST_CONTEXT"
        ) {
            continue;
        }
        if name.starts_with("HGS_")
            || name.starts_with("LC_")
            || matches!(
                name,
                "HOME"
                    | "PATH"
                    | "LANG"
                    | "TMPDIR"
                    | "TMUX"
                    | "TMUX_TMPDIR"
                    | "XDG_STATE_HOME"
                    | "XDG_CONFIG_HOME"
            )
        {
            size = size.saturating_add(name.len() + value.as_encoded_bytes().len());
            if size > 256 << 10 || env.len() >= 4096 {
                return Err("Picker configuration exceeds its bound".into());
            }
            env.insert(name.to_owned(), value.as_encoded_bytes().to_vec());
        }
    }
    let config_file = dir.join("config");
    let bytes = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&config_file)
    {
        Ok(mut file) => {
            let m = file.metadata().map_err(|e| e.to_string())?;
            if !m.is_file() || m.len() > 1 << 20 {
                return Err("Original picker configuration is unavailable".into());
            }
            let before = (
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
            );
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take((1 << 20) + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let m = file.metadata().map_err(|e| e.to_string())?;
            if bytes.len() > 1 << 20
                || before
                    != (
                        m.dev(),
                        m.ino(),
                        m.len(),
                        m.mtime(),
                        m.mtime_nsec(),
                        m.ctime(),
                        m.ctime_nsec(),
                    )
            {
                return Err("Original picker configuration changed".into());
            }
            Some((before, bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("Original picker configuration is unavailable".into()),
    };
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!({"location":location,"env":env,"config":bytes}))
                .map_err(|e| e.to_string())?
        )
    ))
}

impl Frame {
    pub(super) fn new(
        operation: Operation,
        raw: Vec<u8>,
        record: &Value,
        deadline: Deadline,
    ) -> Result<Self> {
        let binding = BINDING_FIELDS
            .iter()
            .filter_map(|k| record.get(*k).map(|v| ((*k).into(), v.clone())))
            .collect::<serde_json::Map<_, _>>();
        let id = serde_json::from_slice::<Value>(&raw).map_err(|e| e.to_string())?["request_id"]
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .ok_or("Invalid picker request UUID")?
            .to_string();
        let (root, root_object) = namespace()?;
        let frame = Self {
            schema: 1,
            nonce: uuid::Uuid::new_v4().to_string(),
            image: Image::own()?,
            deadline,
            configuration: configuration()?,
            root,
            root_object,
            operation,
            raw,
            request_id: id,
            binding: Value::Object(binding),
        };
        frame.validate()?;
        Ok(frame)
    }
    pub(super) fn validate(&self) -> Result<()> {
        if self.schema != 1
            || !canonical_uuid(&self.nonce)
            || !canonical_uuid(&self.request_id)
            || self.raw.len() > REQUEST_BYTES
            || self.raw.is_empty()
            || self.configuration.len() != 64
            || !self.configuration.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid original picker frame".into());
        }
        let map = self.binding.as_object().ok_or("Invalid picker binding")?;
        if map
            .keys()
            .any(|key| !BINDING_FIELDS.contains(&key.as_str()))
            || !canonical_uuid(string(&self.binding, "session_id"))
            || !is_id_layout_record(&self.binding, string(&self.binding, "session_id"))
            || !bounded(string(&self.binding, "name"), 2048, false)
            || !bounded(string(&self.binding, "run_id"), 128, false)
            || !bounded(string(&self.binding, "conversation_id"), 2048, true)
        {
            return Err("Invalid original picker selector".into());
        }
        let (id, run, conversation) = match self.operation {
            Operation::Effort => {
                let r: crate::state::effort::Request =
                    serde_json::from_slice(&self.raw).map_err(|e| e.to_string())?;
                if !valid(&r.effort) || r.expected_conversation_id.is_empty() {
                    return Err("Invalid original effort request".into());
                }
                (r.request_id, r.expected_run_id, r.expected_conversation_id)
            }
            Operation::Settings => {
                let r: crate::state::effort::SettingsRequest =
                    serde_json::from_slice(&self.raw).map_err(|e| e.to_string())?;
                if (r.model.is_none() && r.effort.is_none())
                    || r.model.as_deref().is_some_and(|v| !safe_model(v))
                    || r.effort
                        .as_deref()
                        .is_some_and(|v| !v.is_empty() && !valid(v))
                {
                    return Err("Invalid original settings request".into());
                }
                (r.request_id, r.expected_run_id, r.expected_conversation_id)
            }
        };
        if uuid::Uuid::parse_str(&id)
            .map_err(|e| e.to_string())?
            .to_string()
            != self.request_id
            || run != string(&self.binding, "run_id")
            || conversation != string(&self.binding, "conversation_id")
        {
            return Err("Original picker request and selector disagree".into());
        }
        self.deadline.remaining()?;
        Ok(())
    }
    pub(super) fn check_context(&self) -> Result<()> {
        self.validate()?;
        self.image.check()?;
        if namespace()? != (self.root.clone(), self.root_object)
            || configuration()? != self.configuration
        {
            return Err("Original picker configuration or namespace changed".into());
        }
        Ok(())
    }
}
