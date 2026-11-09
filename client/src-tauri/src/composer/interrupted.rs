//! Qt's interrupted prompt (`SessionsWindow.cpp:1192-1197,2279-2307`): after
//! Stop, the prompt of the interrupted turn returns to an empty message field,
//! as Terminal gives it back, once the session reports the turn interrupted.

use std::collections::VecDeque;

use serde_json::Value;
use zerus_client_core::local_messages::{LocalMessage, Status};

/// hgs records the prompt as its first 4000 characters and an ellipsis.
const CLIP: usize = 4000;
/// An automatic recovery's continuation is not the person's prompt.
const RECOVERY: &str = "[HGS automatic recovery]";
/// Pending prompts kept across sessions; the oldest goes first.
const KEPT: usize = 16;
/// The phases a stopped turn passes through before it reads `interrupted`.
const STOPPING: [&str; 4] = ["working", "tool", "compacting", "interrupted"];

/// The prompt a Stop will give back, bound to the run and conversation it
/// was sent to, and to that Stop (`stop`, numbered by the hub), so a late
/// refusal of an older Stop never drops a newer one's prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub(crate) run: String,
    pub(crate) conversation: String,
    pub(crate) text: String,
    pub(crate) stop: u64,
}

/// The prompts waiting for their sessions to report the turn interrupted, by
/// draft key.
#[derive(Default)]
pub(crate) struct Prompts(VecDeque<(String, Prompt)>);

impl Prompts {
    /// Records the prompt of a Stop; none clears the session's.
    pub(crate) fn set(&mut self, key: &str, prompt: Option<Prompt>) {
        self.remove(key);
        if let Some(prompt) = prompt {
            self.0.push_back((key.to_owned(), prompt));
            while self.0.len() > KEPT {
                self.0.pop_front();
            }
        }
    }

    pub(crate) fn remove(&mut self, key: &str) {
        self.0.retain(|(known, _)| known != key);
    }

    /// The interrupt of Stop `stop` was refused: its prompt alone goes,
    /// whichever session is open by then.
    pub(crate) fn refused(&mut self, stop: u64) {
        self.0.retain(|(_, prompt)| prompt.stop != stop);
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Prompt> {
        self.0
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, prompt)| prompt)
    }
}

/// Qt's `interruptedPrompt`: the exact text sent from Activity when it is
/// this turn's prompt, otherwise the recorded prompt. A clipped prompt
/// without its sent text, and an automatic continuation, give nothing back.
pub(crate) fn prompt_of(details: &Value, local: &[LocalMessage]) -> Option<Prompt> {
    let recorded = details["prompt"].as_str().unwrap_or("");
    if recorded.is_empty() || recorded.starts_with(RECOVERY) {
        return None;
    }
    let run = details["run_id"].as_str().unwrap_or("");
    let conversation = details["conversation_id"].as_str().unwrap_or("");
    let clipped = recorded.chars().count() > CLIP && recorded.ends_with('\u{2026}');
    // Only the one ellipsis hgs appends: the prompt's own 4000th character
    // may be an ellipsis too, as Qt's `chopped(1)` keeps it (zerus-34rs0).
    let known = if clipped {
        recorded.strip_suffix('\u{2026}').unwrap_or(recorded)
    } else {
        recorded
    };
    // The exact run and conversation: a row still sending in an earlier
    // conversation of the run is not this turn's text (zerus-gy7cj).
    let sent = local.iter().rev().find(|message| {
        message.status != Status::Error
            && message.run_id == run
            && message.conversation_id == conversation
            && if clipped {
                message.text.starts_with(known)
            } else {
                message.text.trim() == known.trim()
            }
    });
    let text = match sent {
        Some(message) => message.text.clone(),
        None if clipped => return None,
        None => recorded.to_owned(),
    };
    Some(Prompt {
        run: run.to_owned(),
        conversation: conversation.to_owned(),
        text,
        stop: 0,
    })
}

/// What the session's latest inspection means for a pending prompt.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The turn is still stopping, or the inspection is not this session's yet.
    Wait,
    /// The turn ended another way, or the session changed.
    Drop,
    /// The turn reads interrupted: the prompt goes back to the field.
    Restore,
}

/// Qt's `restoreInterruptedPrompt` checks.
pub(crate) fn verdict(prompt: &Prompt, details: &Value) -> Verdict {
    let run = details["run_id"].as_str().unwrap_or("");
    if run.is_empty() {
        return Verdict::Wait;
    }
    let phase = details["phase"].as_str().unwrap_or("");
    if run != prompt.run
        || details["conversation_id"].as_str().unwrap_or("") != prompt.conversation
        || !STOPPING.contains(&phase)
    {
        return Verdict::Drop;
    }
    if phase == "interrupted" {
        Verdict::Restore
    } else {
        Verdict::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn local(text: &str, run: &str, status: Status) -> LocalMessage {
        LocalMessage {
            id: "request".into(),
            text: text.into(),
            status,
            attachments: Vec::new(),
            run_id: run.into(),
            conversation_id: "conversation".into(),
            submitted_at: 1.0,
            error: String::new(),
            uncertain: false,
            message_id: None,
            submitted_text: None,
            source: None,
            voice: None,
            resumed_from: None,
        }
    }

    fn details(prompt: &str, phase: &str) -> Value {
        json!({"prompt": prompt, "phase": phase, "run_id": "run", "conversation_id": "conversation"})
    }

    #[test]
    fn gives_back_the_sent_text_of_this_turn_else_the_recorded_prompt() {
        let sent = [local("  Fix the build\n", "run", Status::Sent)];
        assert_eq!(
            prompt_of(&details("Fix the build", "working"), &sent).map(|p| p.text),
            Some("  Fix the build\n".into())
        );
        // Another run's or a failed send's text is not this turn's.
        let other = [
            local("Fix the build!", "older", Status::Sent),
            local("Fix the build", "run", Status::Error),
        ];
        let recorded = prompt_of(&details("Fix the build", "working"), &other).unwrap();
        assert_eq!(
            recorded,
            Prompt {
                run: "run".into(),
                conversation: "conversation".into(),
                text: "Fix the build".into(),
                stop: 0
            }
        );
    }

    #[test]
    fn a_clipped_prompt_needs_its_sent_text_and_a_recovery_gives_nothing() {
        let full = "x".repeat(4100);
        let clipped = format!("{}\u{2026}", &full[..CLIP]);
        assert_eq!(
            prompt_of(
                &details(&clipped, "working"),
                &[local(&full, "run", Status::Sending)]
            )
            .map(|p| p.text),
            Some(full.clone())
        );
        assert_eq!(prompt_of(&details(&clipped, "working"), &[]), None);
        assert_eq!(
            prompt_of(
                &details("[HGS automatic recovery] continue", "working"),
                &[]
            ),
            None
        );
        assert_eq!(prompt_of(&details("", "working"), &[]), None);
    }

    /// review-win-d's control for PR #1778 (zerus-gy7cj).
    #[test]
    fn a_pending_message_from_another_conversation_is_not_this_turns_exact_prompt() {
        let prefix = "x".repeat(CLIP);
        let mut old = local(&format!("{prefix}old turn suffix"), "run", Status::Sending);
        old.conversation_id = "old-conversation".into();
        old.uncertain = true;
        let mut clipped = details(&format!("{prefix}\u{2026}"), "working");
        clipped["conversation_id"] = json!("new-conversation");
        assert_eq!(prompt_of(&clipped, &[old.clone()]), None);
        // Unclipped, the recorded prompt itself is this turn's.
        let mut whole = details("Fix the build", "working");
        whole["conversation_id"] = json!("new-conversation");
        old.text = "Fix the build".into();
        assert_eq!(
            prompt_of(&whole, &[old]).map(|p| (p.text, p.conversation)),
            Some(("Fix the build".into(), "new-conversation".into()))
        );
    }

    /// review-win-d's control for PR #1778 (zerus-34rs0).
    #[test]
    fn clipping_removes_only_the_ellipsis_hgs_appends() {
        let prefix = "x".repeat(CLIP - 1);
        // The prompt's 4000th character is an ellipsis; hgs appends one more.
        let clipped = details(&format!("{prefix}\u{2026}\u{2026}"), "working");
        let wrong = local(
            &format!("{prefix}y is a different 4000th character"),
            "run",
            Status::Sending,
        );
        assert_eq!(prompt_of(&clipped, &[wrong]), None);
        let right = local(
            &format!("{prefix}\u{2026} and the rest of it"),
            "run",
            Status::Sending,
        );
        assert_eq!(
            prompt_of(&clipped, &[right]).map(|p| p.text),
            Some(format!("{prefix}\u{2026} and the rest of it"))
        );
    }

    #[test]
    fn restores_only_once_the_same_turn_reads_interrupted() {
        let prompt = prompt_of(&details("Fix the build", "working"), &[]).unwrap();
        assert_eq!(
            verdict(&prompt, &details("Fix the build", "tool")),
            Verdict::Wait
        );
        assert_eq!(
            verdict(&prompt, &json!({"phase": "interrupted"})),
            Verdict::Wait
        );
        assert_eq!(
            verdict(&prompt, &details("Fix the build", "interrupted")),
            Verdict::Restore
        );
        assert_eq!(
            verdict(&prompt, &details("Fix the build", "idle")),
            Verdict::Drop
        );
        let mut moved = details("Fix the build", "interrupted");
        moved["run_id"] = json!("another");
        assert_eq!(verdict(&prompt, &moved), Verdict::Drop);
        moved = details("Fix the build", "interrupted");
        moved["conversation_id"] = json!("another");
        assert_eq!(verdict(&prompt, &moved), Verdict::Drop);
    }

    #[test]
    fn keeps_one_prompt_per_session_and_at_most_sixteen() {
        let mut prompts = Prompts::default();
        let prompt = |text: &str| Prompt {
            run: "run".into(),
            conversation: "c".into(),
            text: text.into(),
            stop: 0,
        };
        prompts.set("a", Some(prompt("first")));
        prompts.set("a", Some(prompt("second")));
        assert_eq!(prompts.get("a").map(|p| p.text.as_str()), Some("second"));
        prompts.set("a", None);
        assert!(prompts.get("a").is_none());
        for i in 0..20 {
            prompts.set(&format!("k{i}"), Some(prompt("p")));
        }
        assert!(prompts.get("k3").is_none());
        assert!(prompts.get("k4").is_some());
        assert!(prompts.get("k19").is_some());
    }

    #[test]
    fn a_refusal_drops_only_its_own_stops_prompt() {
        let mut prompts = Prompts::default();
        let stopped = |stop: u64| Prompt {
            run: "run".into(),
            conversation: "c".into(),
            text: "p".into(),
            stop,
        };
        prompts.set("alpha", Some(stopped(1)));
        prompts.set("beta", Some(stopped(2)));
        prompts.refused(1);
        assert!(prompts.get("alpha").is_none());
        assert_eq!(prompts.get("beta").map(|p| p.stop), Some(2));
        // An older Stop of the same session refused after a newer Stop.
        prompts.set("beta", Some(stopped(3)));
        prompts.refused(2);
        assert_eq!(prompts.get("beta").map(|p| p.stop), Some(3));
    }
}
