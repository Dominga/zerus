//! One retained, fixed startup producer. Only the original supervisor creates
//! it. The non-cloneable outer lifetime closes admission, not physical custody.
use super::*;
use crate::state::{claude_theme::AppearanceCall, storage};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub(in crate::state) struct Appearance(Arc<Producer>);
impl Appearance {
    pub(super) fn new(producer: Arc<Producer>) -> Self {
        Self(producer)
    }
    pub(in crate::state) fn producer(&self) -> Arc<Producer> {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests;
impl Drop for Appearance {
    fn drop(&mut self) {
        self.0.open.store(false, Ordering::Release);
    }
}

pub(in crate::state) struct Producer {
    source: Arc<native::Source>,
    domain: Arc<terminating::OriginalDomain>,
    binding: Value,
    record: Value,
    request: String,
    state: PathBuf,
    config: PathBuf,
    open: AtomicBool,
    failed: AtomicBool,
}
impl Producer {
    pub(super) fn new(original: &Original, record: &Value, request: &str) -> Result<Arc<Self>> {
        if !uuid::Uuid::parse_str(request)
            .is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == request)
        {
            return Err("Invalid original appearance request".into());
        }
        let value = Arc::new(Self {
            source: original.source.clone(),
            domain: original.domain.clone(),
            binding: original.binding.clone(),
            record: record.clone(),
            request: request.into(),
            state: storage::root(),
            config: crate::config::config_dir(&home().to_string_lossy()),
            open: AtomicBool::new(true),
            failed: AtomicBool::new(false),
        });
        value.check(record)?;
        Ok(value)
    }
    pub(in crate::state) fn check(&self, record: &Value) -> Result<()> {
        let valid = || -> Result<()> {
            let (state, config, _) = self.source.directories();
            if !self.open.load(Ordering::Acquire)
                || self.failed.load(Ordering::Acquire)
                || !native::profile()
                || !self.source.current()
                || !native::wait_policy()
                || storage::root() != self.state
                || crate::config::config_dir(&home().to_string_lossy()) != self.config
                || crate::platform::canonical(&self.state).map_err(|e| e.to_string())? != state
                || crate::platform::canonical(&self.config).map_err(|e| e.to_string())? != config
                || record["original_owner"] != self.binding
                || !claude_theme::eligible(record)
                || [
                    "session_id",
                    "run_id",
                    "pid",
                    "supervisor",
                    "pane",
                    "name",
                    "layout",
                    "launch_dir",
                    "agent_home",
                    "account_id",
                    "expected_id",
                ]
                .iter()
                .any(|key| record.get(*key) != self.record.get(*key))
                || ["layout_transition", "rename_shadow", "rename_target"]
                    .iter()
                    .any(|key| record.get(*key).is_some())
            {
                return Err("Original appearance source changed or closed".into());
            }
            let root = self.binding["root"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("Original appearance root unavailable")?;
            let identity = crate::platform::process_identity(root)
                .ok_or("Original appearance root unavailable")?;
            if identity.parent != std::process::id()
                || self.binding["root_birth"] != identity.birth
                || record["pid"] != root
            {
                return Err("Original appearance root changed".into());
            }
            self.domain.current()
        };
        let result = valid();
        if result.is_err() {
            self.failed.store(true, Ordering::Release);
        }
        result
    }
    pub(in crate::state) fn writer(&self, until: Instant) -> Result<storage::Guard> {
        let now = Instant::now();
        if until <= now || until.duration_since(now) > Duration::from_secs(30) {
            return Err("Original appearance admission deadline is invalid".into());
        }
        storage::existing_lock_until(&self.source.directories().0.join(".lock"), until)?
            .ok_or("Original appearance writer is busy".into())
    }
    pub(in crate::state) fn admit(
        &self,
        writer: &storage::Guard,
        record: &Value,
        call: &AppearanceCall,
        companion: Arc<dyn Send + Sync>,
        until: Instant,
    ) -> Result<terminating::StartupAdmission> {
        self.check(record)?;
        let allowed = match call {
            AppearanceCall::Capture(pane)
            | AppearanceCall::Pane(pane)
            | AppearanceCall::Enter(pane) => pane == string(record, "pane"),
            AppearanceCall::Layout(terminal) => terminal == terminal_name(record),
            AppearanceCall::Process(pid) => record["pid"].as_u64() == Some(u64::from(*pid)),
        };
        if !allowed {
            return Err("Unreviewed original appearance target".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let source = format!("{:x}", Sha256::digest(self.source.wire().to_string()));
        let intent = format!(
            "{:x}",
            Sha256::digest(
                json!({"producer":"claude-appearance-v1",
            "appearance_request":self.request,"call":call.wire(),"session":record["session_id"],
            "run":record["run_id"]})
                .to_string()
            )
        );
        let admitted =
            self.domain
                .appearance(writer, record, [&id, &source, &intent], companion, until)?;
        self.check(record)?;
        Ok(admitted)
    }
    pub(super) fn observation(&self) -> Value {
        json!({"family":"claude_appearance","open":self.open.load(Ordering::Acquire),
            "unresolved":self.failed.load(Ordering::Acquire),"effect_authority":false})
    }
}
