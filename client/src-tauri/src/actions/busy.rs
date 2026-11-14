//! Live claim metadata only; removed with its last Claim, never a history/cache.
use super::*;
use std::time::{Instant, SystemTime};
use zerus_client_core::text::{Arg, Text};

pub(crate) const PREFIX: &str = "zerus-action-busy-v1:";
const MESSAGE: &str = zerus_client_core::source!(
    "This session is busy with “%1” (%2 elapsed, since %3 UTC). Wait for it to finish."
);
#[cfg(any(target_os = "linux", target_os = "macos", test))]
pub(crate) const REVIEW: &str = zerus_client_core::source!("Working folder review");
pub(super) const GENERIC: &str = zerus_client_core::source!("Session action");

pub(super) struct Holder {
    label: Mutex<&'static str>,
    began: Instant,
    started_utc: String,
}
impl Holder {
    pub(super) fn new(label: &'static str) -> Self {
        Self {
            label: Mutex::new(label),
            began: Instant::now(),
            started_utc: chrono::DateTime::<chrono::Utc>::from(SystemTime::now())
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        }
    }
    pub(super) fn label(&self, value: &'static str) {
        *self.label.lock().unwrap_or_else(PoisonError::into_inner) = value;
    }
    pub(super) fn refusal(&self) -> String {
        let seconds = self.began.elapsed().as_secs();
        let (source, count) = if seconds >= 3600 {
            (zerus_client_core::source!("%1 h"), seconds / 3600)
        } else if seconds >= 60 {
            (zerus_client_core::source!("%1 min"), seconds / 60)
        } else {
            (zerus_client_core::source!("%1 s"), seconds)
        };
        let label = *self.label.lock().unwrap_or_else(PoisonError::into_inner);
        let text = Text::with(
            MESSAGE,
            vec![
                Text::new(label).into(),
                Text::with(source, vec![Arg::Number(count.min(i64::MAX as u64) as i64)]).into(),
                self.started_utc.as_str().into(),
            ],
        );
        // Keep typed prose through String-only command errors. The bridge
        // renders it once, including nested action names and elapsed units.
        format!(
            "{PREFIX}{}",
            serde_json::json!({
                "action":label, "elapsed_seconds":seconds,
                "started_utc":self.started_utc, "message":text
            })
        )
    }
}

pub(super) fn action(action: &Action) -> &'static str {
    match action {
        Action::Interrupt { .. } => zerus_client_core::source!("Stop agent turn"),
        Action::MarkRead { .. } => zerus_client_core::source!("Mark session read"),
        Action::Settings { .. } => zerus_client_core::source!("Change model or effort"),
        // WhiteWh fork actions.
        Action::SettingsCheck { .. } => zerus_client_core::source!("Change model or effort"),
        Action::ResumeStale { .. } => zerus_client_core::source!("Resume session"),
        Action::ConfirmSend => zerus_client_core::source!("Send message"),
        Action::ClearContext { .. } => zerus_client_core::source!("Clear session"),
        Action::CompactContext => zerus_client_core::source!("Compact context"),
        Action::Pause | Action::PauseAll { .. } => zerus_client_core::source!("Pause session"),
        Action::Resume | Action::ResumeAll { .. } => zerus_client_core::source!("Resume session"),
        Action::StartFresh => zerus_client_core::source!("Start fresh session"),
        Action::Rename { .. } => zerus_client_core::source!("Rename session"),
        Action::Archive => zerus_client_core::source!("Archive session"),
        Action::Terminate | Action::Kill | Action::TerminateShell { .. } => {
            zerus_client_core::source!("Terminate session")
        }
        Action::Send { .. } => zerus_client_core::source!("Send message"),
        Action::StopProcess { .. } => zerus_client_core::source!("Stop process"),
        Action::Fork { .. } => zerus_client_core::source!("Fork session"),
        Action::StageFiles { .. } => zerus_client_core::source!("Transfer attachments"),
        Action::SendNow { .. } => zerus_client_core::source!("Send queued message"),
        Action::RemoveQueued { .. } => zerus_client_core::source!("Remove queued message"),
        Action::Answer { .. } => zerus_client_core::source!("Answer question"),
        Action::Recovery { .. } => zerus_client_core::source!("Change recovery retry"),
        Action::Forget | Action::ForgetArchive { .. } | Action::ForgetBinding => {
            zerus_client_core::source!("Forget session")
        }
        Action::Restore { .. } => zerus_client_core::source!("Restore session"),
    }
}

pub(super) fn lane(lane: Lane) -> &'static str {
    match lane {
        Lane::Write => zerus_client_core::source!("Change parent session"),
        Lane::Notifications => zerus_client_core::source!("Change notifications"),
        Lane::KeepWarm => zerus_client_core::source!("Change cache warm settings"),
        Lane::QuestionRouting => zerus_client_core::source!("Change question routing"),
        Lane::Read => zerus_client_core::source!("Mark session read"),
        _ => GENERIC,
    }
}

pub(crate) fn is_busy(value: &str) -> bool {
    value == BUSY || value.starts_with(PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn name_and_run_collisions_keep_the_original_action_and_clock() {
        let directory = tempfile::tempdir().unwrap();
        let actions = Actions::new(directory.path().join("marks.json"));
        let target = Target {
            computer: String::new(),
            name: "codex/example/session".into(),
            run_id: "original-run".into(),
            conversation_id: "original-conversation".into(),
        };
        let action = Action::ClearContext {
            dismiss_prompt: false,
        };
        let running = actions.begin(Lane::key(&target, &action), &action).unwrap();
        let started = running._claim.holder.started_utc.clone();
        for other in [
            Target {
                name: "codex/example/renamed".into(),
                ..target.clone()
            },
            Target {
                run_id: "after-clear-run".into(),
                conversation_id: "after-clear-conversation".into(),
                ..target.clone()
            },
        ] {
            let error = actions
                .reserve_end_for(&other, &Action::Terminate)
                .err()
                .unwrap();
            let data: serde_json::Value =
                serde_json::from_str(error.strip_prefix(PREFIX).unwrap()).unwrap();
            assert_eq!(data["action"], "Clear session");
            assert_eq!(data["started_utc"], started);
        }
        drop(running);
        assert!(actions.reserve_end_for(&target, &Action::Terminate).is_ok());
        let keys = actions.flight.keys.lock().unwrap();
        assert!(keys.names.is_empty() && keys.runs.is_empty());
    }
    #[test]
    fn an_old_holder_reports_minutes_without_a_sleep_or_clock_reset() {
        let mut holder = Holder::new(REVIEW);
        holder.began = Instant::now() - Duration::from_secs(181);
        let data: serde_json::Value =
            serde_json::from_str(holder.refusal().strip_prefix(PREFIX).unwrap()).unwrap();
        let text: Text = serde_json::from_value(data["message"].clone()).unwrap();
        assert!(text
            .english()
            .contains("Working folder review” (3 min elapsed, since "));
        assert!(text.english().ends_with(" UTC). Wait for it to finish."));
    }
    #[test]
    fn a_foreign_claim_is_state_changed_instead_of_a_fabricated_busy_holder() {
        let directory = tempfile::tempdir().unwrap();
        let first = Actions::new(directory.path().join("first.json"));
        let second = Actions::new(directory.path().join("second.json"));
        let target = Target {
            computer: String::new(),
            name: "codex/example/session".into(),
            run_id: "run".into(),
            conversation_id: "conversation".into(),
        };
        let claim = first.reserve_end_for(&target, &Action::Terminate).unwrap();
        let error = second
            .activate_named(claim, &Action::Terminate, action(&Action::Terminate))
            .err()
            .unwrap();
        assert_eq!(error, zerus_client_core::end_action::STATE_CHANGED);
        assert!(!is_busy(&error));
    }
}
