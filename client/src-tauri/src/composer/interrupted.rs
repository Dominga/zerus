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
/// was sent to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub(crate) run: String,
    pub(crate) conversation: String,
    pub(crate) text: String,
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
    let clipped = recorded.chars().count() > CLIP && recorded.ends_with('\u{2026}');
    let known = if clipped {
        recorded.trim_end_matches('\u{2026}')
    } else {
        recorded
    };
    let sent = local.iter().rev().find(|message| {
        message.status != Status::Error
            && message.run_id == run
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
        conversation: details["conversation_id"].as_str().unwrap_or("").to_owned(),
        text,
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
                text: "Fix the build".into()
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
}
