//! WhiteWh fork (owner, 2026-11-13): an uncertain send that Claude recorded
//! after all.
//!
//! A send whose proof did not come within hgs's settle window stays
//! uncertain, and its draft keeps the send's identity (its checkpoint), even
//! across restarts, until Allow retry. Claude often records it moments later,
//! so while the open session's draft holds such an identity, the hub asks hgs
//! whether Claude's transcript records that exact send (`hgs send NAME
//! --confirm REQUEST --json`, which types and sends nothing): right away, then
//! with a growing pause, only while the window is shown. Only where no desktop
//! service takes sends (Windows, and Linux or macOS outside desktop-service
//! mode): the service checks the sends it holds itself. A submitted receipt of exactly that request,
//! session, run and conversation ends the uncertainty as a recorded send
//! does: the identity goes, the draft is cleared only while it still is that
//! message, and its Activity row shows it sent. Any other answer changes
//! nothing, and Allow retry stays.

use super::*;
use zerus_client_core::operation_recovery::deferred_input::Kind;

/// The pause after the first check, doubled after each answer up to
/// [`LONGEST`].
const FIRST: f64 = 15.0;
const LONGEST: f64 = 120.0;

impl Composers {
    /// Each tick: the open composer's due check.
    pub(crate) fn tick_late(&self, now: f64) {
        let job = {
            let mut hub = self.lock();
            if hub.hidden || hub.closed || self.deferred().is_some() {
                return;
            }
            let (Some(key), Some(open)) = (hub.open_key.clone(), hub.open.clone()) else {
                return;
            };
            if !open.computer.is_empty() || open.subagent.is_some() || open.archived() {
                return;
            }
            let Some(composer) = hub.composers.get_mut(&key) else {
                return;
            };
            if !composer.loaded
                || !composer.uncertain
                || composer.handed.is_some()
                || composer.busy()
                || composer.late.checking
                || now < composer.late.next
            {
                return;
            }
            let Some(checkpoint) = composer.checkpoint.as_ref().filter(|checkpoint| {
                checkpoint.kind == Kind::Send
                    && !checkpoint.run_id.is_empty()
                    && !checkpoint.conversation_id.is_empty()
            }) else {
                return;
            };
            composer.late.checking = true;
            let target = Target {
                computer: String::new(),
                name: open.name.clone(),
                run_id: checkpoint.run_id.clone(),
                conversation_id: checkpoint.conversation_id.clone(),
            };
            let held = (
                checkpoint.request_id.clone(),
                checkpoint.message_sha256.clone(),
            );
            (key, composer.handle, target, held)
        };
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let (key, handle, target, (request, message)) = job;
            let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (me.0.deps.dispatch)(target, Action::ConfirmSend, request.clone())
            }));
            let recorded = matches!(called, Ok(Ok(Outcome::Done { .. })));
            me.late_answer(&key, handle, &request, &message, recorded);
        }));
    }

    /// One check's answer, applied while the draft still holds that identity.
    fn late_answer(&self, key: &str, handle: u64, request: &str, message: &str, recorded: bool) {
        let now = self.now();
        let mut hub = self.lock();
        let Some(composer) = hub.composers.get_mut(key).filter(|c| c.handle == handle) else {
            return;
        };
        composer.late.checking = false;
        composer.late.delay = if composer.late.delay <= 0.0 {
            FIRST
        } else {
            (composer.late.delay * 2.0).min(LONGEST)
        };
        composer.late.next = now + composer.late.delay;
        let held = composer.uncertain
            && !composer.busy()
            && composer.handed.is_none()
            && composer
                .checkpoint
                .as_ref()
                .is_some_and(|checkpoint| checkpoint.request_id == request);
        if !recorded || !held {
            return;
        }
        composer.uncertain = false;
        composer.error = false;
        composer.checkpoint = None;
        composer.notice.clear();
        composer.late = Late::default();
        // Only the unchanged draft was the message.
        if send::message_digest(&composer.draft.text, &composer.draft.attachments) == message {
            composer.draft = Draft::default();
            composer.position = 0;
            composer.anchor = 0;
            composer.attachments_changed();
        }
        composer.text_epoch += 1;
        self.save(composer);
        let done = Outcome::Done {
            receipt: serde_json::json!({"request_id": request}),
            cleanup: None,
        };
        hub.local.finished_as_is(key, request, &done);
        hub.orphans.retain(|orphan| orphan.request != request);
        drop(hub);
        self.reconcile();
        self.changed();
    }
}

/// A draft's late checks: when the next is due, the pause before it, and
/// whether one runs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Late {
    pub(crate) next: f64,
    pub(crate) delay: f64,
    pub(crate) checking: bool,
}

#[cfg(test)]
mod tests {
    use super::super::send::tests::*;
    use super::super::tests::*;
    use super::*;

    const UNSURE: &str = "connection lost";

    /// This computer's own session: only its sends are checked.
    fn open_local(c: &Composers) -> ComposerView {
        c.open(Some(SessionRef {
            computer: String::new(),
            ..session()
        }));
        let mut batch = state(ready());
        batch.activity[0].computer = zerus_client_core::ComputerId::from("");
        c.observe(&batch);
        until(c, |v| v.loaded && v.available)
    }

    /// A send that ended uncertain: its draft holds the identity.
    fn uncertain_send(c: &Composers, fake: &Fake, text: &str) -> (ComposerView, String) {
        *lock(&fake.outcome) = Box::new(|action, id| match action {
            Action::Send { .. } => Outcome::Uncertain {
                detail: UNSURE.into(),
            },
            _ => receipt(action, id),
        });
        let view = open_local(c);
        c.send_message(view.handle, 1, view.text_epoch, text.into(), 0, 0);
        let view = until(c, |v| !v.sending && v.uncertain);
        let id = sent_ids(fake).pop().unwrap();
        (view, id)
    }

    fn sent_ids(fake: &Fake) -> Vec<String> {
        lock(&fake.calls)
            .iter()
            .filter(|(_, action, _)| matches!(action, Action::Send { .. }))
            .map(|(_, _, id)| id.clone())
            .collect()
    }

    fn checks(fake: &Fake) -> Vec<(Target, String)> {
        lock(&fake.calls)
            .iter()
            .filter(|(_, action, _)| *action == Action::ConfirmSend)
            .map(|(target, _, id)| (target.clone(), id.clone()))
            .collect()
    }

    #[test]
    fn a_send_claude_recorded_later_ends_its_uncertainty_and_is_never_resent() {
        let root = dir("late-recorded");
        let (c, fake) = send_hub(&root);
        let (view, id) = uncertain_send(&c, &fake, "maybe sent");
        assert!(!view.can_send);
        // Not recorded yet: nothing changes, and the next check waits.
        *lock(&fake.outcome) = Box::new(|action, _| match action {
            Action::ConfirmSend => Outcome::Failed {
                detail: "Claude's transcript does not record this message yet.".into(),
            },
            _ => panic!("only the check runs: {action:?}"),
        });
        c.tick_late(c.now());
        eventually(|| checks(&fake).len() == 1);
        eventually(|| !c.lock().composers.values().any(|c| c.late.checking));
        let view = c.current().unwrap();
        assert!(view.uncertain && !view.can_send);
        c.tick_late(c.now());
        c.tick_late(c.now() + FIRST - 1.0);
        assert_eq!(checks(&fake).len(), 1, "the pause holds");
        // Recorded: the identity goes, and the field is free.
        *lock(&fake.outcome) = Box::new(|action, id| match action {
            Action::ConfirmSend => receipt(action, id),
            _ => panic!("only the check runs: {action:?}"),
        });
        *lock(&fake.now) += FIRST;
        c.tick_late(c.now());
        let view = until(&c, |v| !v.uncertain);
        assert!(view.text.is_empty() && view.status.is_empty());
        let asked = checks(&fake);
        assert_eq!(asked.len(), 2);
        assert!(asked.iter().all(|(target, asked)| asked == &id
            && target.computer.is_empty()
            && target.run_id == "run-1"
            && target.conversation_id == "conv-1"));
        assert_eq!(sent_ids(&fake), vec![id], "nothing is sent again");
        c.tick_late(c.now() + LONGEST);
        assert_eq!(checks(&fake).len(), 2, "no check without an identity");
    }

    #[test]
    fn a_new_text_typed_over_the_uncertain_send_stays_in_the_field() {
        let root = dir("late-kept-text");
        let (c, fake) = send_hub(&root);
        let (view, _) = uncertain_send(&c, &fake, "maybe sent");
        c.edit(
            view.handle,
            view.rev + 1,
            view.text_epoch,
            "a new message".into(),
            13,
            13,
        );
        until(&c, |v| v.text == "a new message");
        *lock(&fake.outcome) = Box::new(receipt);
        c.tick_late(c.now());
        let view = until(&c, |v| !v.uncertain);
        assert_eq!(view.text, "a new message");
        assert!(view.can_send);
    }

    #[test]
    fn another_computers_session_is_not_checked() {
        let root = dir("late-peer");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|action, id| match action {
            Action::Send { .. } => Outcome::Uncertain {
                detail: UNSURE.into(),
            },
            _ => receipt(action, id),
        });
        let view = opened(&c, ready());
        c.send_message(view.handle, 1, view.text_epoch, "maybe sent".into(), 0, 0);
        until(&c, |v| !v.sending && v.uncertain);
        c.tick_late(c.now());
        c.flush();
        assert!(checks(&fake).is_empty());
    }

    #[test]
    fn allow_retry_or_a_hidden_window_asks_nothing() {
        let root = dir("late-idle");
        let (c, fake) = send_hub(&root);
        let (view, _) = uncertain_send(&c, &fake, "maybe sent");
        c.set_hidden(true);
        c.tick_late(c.now());
        c.set_hidden(false);
        c.allow_retry(view.handle);
        c.tick_late(c.now());
        c.flush();
        assert!(checks(&fake).is_empty());
    }
}
