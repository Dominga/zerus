//! The native queue and Send and push now (`SessionsWindowShortcuts.cpp:248-341`,
//! `SessionsWindow.cpp:1859-1873, 3976-3980, 4908-4912`). A push never sends
//! message text twice: after the ordinary durable send it asks the agent to
//! send only the queue snapshot that holds that message.

use serde_json::Value;
use zerus_client_core::input_queue::{
    self as rules, Notice, PushFacts, PushStep, Pusher, QueueView,
};
use zerus_client_core::{Action, Outcome, Target};

use super::send::{outcome_of, SendReply};
use super::{ComposerEvent, Composers, Hub, SessionRef};

#[derive(Default)]
pub(crate) struct Queue {
    pub(crate) pusher: Pusher,
    /// One Send now across all sessions (`m_queueSendRequest`).
    pub(crate) sending: Option<String>,
    /// Hide the completed snapshot until another state is accepted.
    pub(crate) hidden: bool,
    session: Option<SessionRef>,
}

/// Captured under the hub lock, so a selection change cannot redirect a push
/// between its snapshot match and its Send now claim.
struct Delivery {
    key: String,
    target: Target,
    queue_id: String,
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn event(notice: Notice) -> ComposerEvent {
    ComposerEvent::Notice {
        text: notice.text,
        error: notice.error,
        transient: notice.transient,
    }
}

impl Composers {
    pub(crate) fn queue_part(&self, hub: &Hub) -> Option<QueueView> {
        if hub.queue.hidden {
            return None;
        }
        let ctx = self.context(hub)?;
        let details = ctx.details.as_deref()?;
        rules::queue_view(
            details,
            ctx.row.tracked,
            ctx.online,
            ctx.facts.inspect_error,
            hub.queue.sending.as_deref() == Some(ctx.key.as_str()),
        )
    }

    /// Qt's `sendQueuedNow` (248-260), while the selection and details are
    /// held. The native action is built only here, never from webview JSON.
    fn begin_queue(&self, hub: &mut Hub, queue_id: Option<String>) -> Option<Delivery> {
        if hub.closed || hub.queue.sending.is_some() {
            return None;
        }
        let ctx = self.context(hub);
        let details = ctx.as_ref().and_then(|ctx| ctx.details.as_deref());
        let id = queue_id.filter(|id| !id.is_empty()).unwrap_or_else(|| {
            details
                .map(|d| text(&d["input_queue"], "id").to_owned())
                .unwrap_or_default()
        });
        let ready = matches!((&ctx, details), (Some(ctx), Some(details)) if rules::can_send_now(details, ctx.online, &id));
        if !ready {
            self.event(
                hub,
                event(Notice {
                    text: rules::NOTHING_QUEUED.into(),
                    ..Notice::default()
                }),
            );
            return None;
        }
        let ctx = ctx.as_ref().expect("ready requires a selected session");
        let details = details.expect("ready requires details");
        let delivery = Delivery {
            key: ctx.key.clone(),
            target: Target {
                computer: ctx.session.computer.clone(),
                name: ctx.session.name.clone(),
                run_id: text(details, "run_id").to_owned(),
                conversation_id: text(details, "conversation_id").to_owned(),
            },
            queue_id: id,
        };
        hub.queue.sending = Some(delivery.key.clone());
        Some(delivery)
    }

    pub fn send_queue_now(&self, queue_id: Option<String>) -> bool {
        let delivery = self.begin_queue(&mut self.lock(), queue_id);
        self.changed();
        let Some(delivery) = delivery else {
            return false;
        };
        self.dispatch_queue(delivery);
        true
    }

    fn dispatch_queue(&self, delivery: Delivery) {
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let closed = me.lock().closed;
            if closed {
                me.queue_sent(&delivery.key, None);
                return;
            }
            let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (me.0.deps.dispatch)(
                    delivery.target,
                    Action::SendNow {
                        queue_id: delivery.queue_id,
                    },
                    uuid::Uuid::new_v4().to_string(),
                )
            }));
            // Lane::Queue busy means no call ran. Qt stays quiet (251).
            let outcome = match called {
                Ok(Err(error)) if error == crate::actions::BUSY => None,
                other => Some(outcome_of(other, rules::NOTHING_QUEUED)),
            };
            me.queue_sent(&delivery.key, outcome.as_ref());
        }));
    }

    fn queue_sent(&self, key: &str, outcome: Option<&Outcome>) {
        let mut hub = self.lock();
        hub.queue.sending = None;
        let current = hub.open_key.as_deref() == Some(key) && outcome.is_some();
        if current {
            // Remove the actual held queue too: the next chord must not use
            // the snapshot we just acted on (Qt 1863), even while hidden.
            if let Some(details) = hub.followed.details.as_mut() {
                if let Some(fields) = std::sync::Arc::make_mut(details).as_object_mut() {
                    fields.remove("input_queue");
                }
            }
            hub.queue.hidden = true;
            self.event(
                &mut hub,
                event(rules::send_now_notice(
                    outcome.expect("a finished action has an outcome"),
                )),
            );
        }
        drop(hub);
        self.changed();
        if current {
            self.refresh_details();
        }
    }

    /// The composer chord only (`MessageComposer::push`, 834-840). An empty
    /// composer or a message in flight asks to send the existing queue now.
    pub fn send_push(
        &self,
        handle: u64,
        rev: u64,
        epoch: u64,
        text: String,
        position: i64,
        anchor: i64,
    ) -> SendReply {
        self.edit(handle, rev, epoch, text.clone(), position, anchor);
        let mut hub = self.lock();
        let Some(key) = hub.open_key.clone() else {
            return SendReply::Busy;
        };
        let Some(composer) = hub.composers.get(&key).filter(|c| c.handle == handle) else {
            return SendReply::Busy;
        };
        if composer.sending || !composer.has_content() {
            drop(hub);
            self.send_queue_now(None);
            return SendReply::Busy;
        }
        if composer.text_epoch != epoch || composer.draft.text != text {
            self.hand_back(hub, handle);
            return SendReply::Busy;
        }
        if !self.view_of(&hub, &key).is_some_and(|v| v.can_send) {
            return SendReply::Busy;
        }
        let message = composer.draft.text.clone();
        if let Some(notice) = hub.queue.pusher.arm(&key, &message, self.now()) {
            self.event(&mut hub, event(notice));
        }
        hub.queue.session = hub.open.clone();
        // Keep the selected-session arm and the ordinary send's checks under
        // one lock, exactly as send_message holds them. Its save is unchanged.
        let reply = self.send_held(&mut hub, handle, epoch, &text);
        if matches!(reply, SendReply::Busy | SendReply::Blocked { .. }) {
            self.hand_back(hub, handle);
        } else {
            drop(hub);
        }
        self.changed();
        reply
    }

    pub(crate) fn push_finished(&self, request: &str, ok: bool, uncertain: bool) {
        let mut hub = self.lock();
        if let Some(notice) = hub.queue.pusher.send_finished(request, ok, uncertain) {
            self.event(&mut hub, event(notice));
            drop(hub);
            self.changed();
        }
    }

    fn push_facts(&self, hub: &Hub) -> PushFacts {
        let Some(push) = hub.queue.pusher.active() else {
            return PushFacts::default();
        };
        let composer = hub.composers.get(&push.key);
        PushFacts {
            dialog: hub.prompts.values().any(|p| p.key == push.key) || hub.answers.cold.is_some(),
            sending: composer.is_some_and(|c| c.sending),
            waiting: composer.is_some_and(|c| c.waiting.is_some()),
        }
    }

    pub(crate) fn queue_observed(&self) {
        let mut hub = self.lock();
        let (Some(key), Some(details)) = (hub.open_key.clone(), hub.followed.details.clone())
        else {
            return;
        };
        let facts = self.push_facts(&hub);
        let busy = hub.queue.sending.is_some();
        let step = hub
            .queue
            .pusher
            .inspected(&key, &details, self.now(), facts, busy);
        let delivery = match step {
            PushStep::Wait => return,
            PushStep::End(notice) => {
                if let Some(notice) = notice {
                    self.event(&mut hub, event(notice));
                }
                None
            }
            PushStep::SendNow(id) => self.begin_queue(&mut hub, Some(id)),
        };
        drop(hub);
        self.changed();
        if let Some(delivery) = delivery {
            self.dispatch_queue(delivery);
        }
    }

    pub(crate) fn push_selection(&self, hub: &mut Hub, key: Option<&str>) {
        let Some(push) = hub.queue.pusher.active() else {
            return;
        };
        if key == Some(push.key.as_str()) {
            return;
        }
        let fallback = push.key.split('\n').nth(1).unwrap_or_default().to_owned();
        let title = hub
            .queue
            .session
            .as_ref()
            .and_then(|s| (self.0.deps.rows)(&s.computer, &s.key, &s.name))
            .map(|(row, _)| row.title)
            .filter(|title| !title.is_empty())
            .unwrap_or(fallback);
        if let Some(notice) = hub.queue.pusher.selection_changed(key, &title) {
            self.event(hub, event(notice));
        }
    }

    pub(crate) fn push_check(&self) {
        let mut hub = self.lock();
        let facts = self.push_facts(&hub);
        if let Some(notice) = hub.queue.pusher.check(self.now(), facts) {
            self.event(&mut hub, event(notice));
            drop(hub);
            self.changed();
        }
    }
}

fn main_origin(label: &str) -> Result<(), String> {
    if label == crate::MAIN_WINDOW {
        Ok(())
    } else {
        Err("Composer actions are available only in the main view.".into())
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn queue_send_now(
    webview: tauri::Webview,
    state: tauri::State<'_, Composers>,
    queue_id: Option<String>,
) -> Result<bool, String> {
    main_origin(webview.label())?;
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.send_queue_now(queue_id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)] // Webview and managed state accompany editor coordinates.
pub async fn send_push(
    webview: tauri::Webview,
    state: tauri::State<'_, Composers>,
    handle: u64,
    rev: u64,
    epoch: u64,
    text: String,
    position: i64,
    anchor: i64,
) -> Result<SendReply, String> {
    main_origin(webview.label())?;
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        composers.send_push(handle, rev, epoch, text, position, anchor)
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::send::tests::{opened, ready, receipt, send_hub, sent, state};
    use crate::composer::tests::{dir, until};
    use crate::composer::{lock, ComposerEvent};
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn working() -> Value {
        let mut details = ready();
        details["activity"] = json!("busy");
        details["phase"] = json!("working");
        details
    }

    fn queued(text: &str, can: bool) -> Value {
        let mut details = working();
        details["input_queue"] = json!({"id": "queue-1", "text": text, "can_send_now": can, "hint": "Send the queued messages now"});
        details
    }

    fn submitted(action: &Action, id: &str) -> Outcome {
        match action {
            Action::SendNow { queue_id } => Outcome::Done {
                receipt: json!({"status": "submitted", "request_id": id, "queue_id": queue_id}),
                cleanup: None,
            },
            _ => receipt(action, id),
        }
    }

    fn send_nows(fake: &crate::composer::send::tests::Fake) -> usize {
        sent(fake)
            .iter()
            .filter(|(_, action, _)| matches!(action, Action::SendNow { .. }))
            .count()
    }

    fn wait_for(done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn notice(text: &str) -> ComposerEvent {
        ComposerEvent::Notice {
            text: text.into(),
            error: false,
            transient: false,
        }
    }

    #[test]
    fn send_now_runs_once_and_hides_the_panel_until_the_next_state() {
        let root = dir("send-now");
        let (c, fake) = send_hub(&root);
        let (gate, wait) = std::sync::mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        *lock(&fake.outcome) = Box::new(move |action, id| {
            if matches!(action, Action::SendNow { .. }) {
                let _ = lock(&wait).recv();
            }
            submitted(action, id)
        });
        let view = opened(&c, queued("❯ first", true));
        let queue = view.queue.clone().unwrap();
        assert_eq!(
            (queue.text.as_str(), queue.label.as_str(), queue.enabled),
            ("❯ first", "Send now", true)
        );
        assert!(c.send_queue_now(Some("queue-1".into())));
        until(&c, |v| {
            v.queue
                .as_ref()
                .is_some_and(|q| q.label == "Sending…" && !q.enabled)
        });
        assert!(!c.send_queue_now(None), "one send-now at a time");
        gate.send(()).unwrap();
        until(&c, |v| v.queue.is_none());
        assert_eq!(
            c.take_events(),
            vec![notice(
                "Send now requested. Waiting for the agent to accept the queue."
            )]
        );
        c.observe(&state(queued("❯ first", true)));
        assert!(
            c.current().unwrap().queue.is_some(),
            "the next state shows the queue again"
        );
        assert_eq!(send_nows(&fake), 1);
    }

    #[test]
    fn nothing_queued_says_so() {
        let root = dir("nothing-queued");
        let (c, fake) = send_hub(&root);
        opened(&c, working());
        assert!(!c.send_queue_now(None));
        assert_eq!(
            c.take_events(),
            vec![notice("Nothing queued can be sent now")]
        );
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_push_sends_the_queue_once_the_agent_queues_it() {
        let root = dir("push");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, working());
        assert_eq!(
            c.send_push(
                view.handle,
                1,
                view.text_epoch,
                "please also run lint".into(),
                20,
                20
            ),
            SendReply::Sending
        );
        until(&c, |v| !v.sending && v.text.is_empty());
        assert_eq!(send_nows(&fake), 0, "nothing is queued yet");
        c.observe(&state(queued("❯ please also run lint", true)));
        wait_for(|| send_nows(&fake) == 1);
        let calls = sent(&fake);
        assert_eq!(
            calls
                .iter()
                .filter(|(_, action, _)| matches!(action, Action::Send { .. }))
                .count(),
            1,
            "a push never sends text again"
        );
        let Some((_, Action::SendNow { queue_id }, _)) = calls.last() else {
            panic!("no send-now")
        };
        assert_eq!(queue_id, "queue-1");
        c.observe(&state(queued("❯ please also run lint", true)));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(send_nows(&fake), 1, "the push ended with its one send-now");
    }

    #[test]
    fn a_push_with_an_empty_field_is_send_now() {
        let root = dir("push-empty");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, queued("❯ earlier", true));
        assert_eq!(
            c.send_push(view.handle, 1, view.text_epoch, String::new(), 0, 0),
            SendReply::Busy
        );
        wait_for(|| send_nows(&fake) == 1);
    }

    #[test]
    fn selecting_another_session_ends_the_push() {
        let root = dir("push-switch");
        let (c, fake) = send_hub(&root);
        lock(&fake.row).title = "review".into();
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, working());
        c.send_push(view.handle, 1, view.text_epoch, "later".into(), 5, 5);
        until(&c, |v| !v.sending && v.text.is_empty());
        c.take_events();
        c.open(Some(SessionRef {
            computer: "studio".into(),
            key: "run:run-2".into(),
            name: "codex/sample-project/other".into(),
            archive_id: None,
        }));
        assert_eq!(
            c.take_events(),
            vec![notice(
                "The message to review was not pushed: another session was selected"
            )]
        );
    }

    #[test]
    fn a_push_ends_after_twenty_seconds() {
        let root = dir("push-late");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, working());
        c.send_push(view.handle, 1, view.text_epoch, "later".into(), 5, 5);
        until(&c, |v| !v.sending && v.text.is_empty());
        c.take_events();
        *lock(&fake.now) += 21.0;
        c.tick();
        assert_eq!(c.take_events(), vec![notice("The message was sent without push: the agent did not offer to send it now within 20 seconds")]);
        c.observe(&state(queued("❯ later", true)));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            send_nows(&fake),
            0,
            "a snapshot after the deadline never sends the queue"
        );
    }

    #[test]
    fn a_busy_queue_lane_is_quiet_and_keeps_the_queue_available() {
        let root = dir("queue-busy");
        let (c, fake) = send_hub(&root);
        opened(&c, queued("❯ first", true));
        *lock(&fake.taken) = Box::new(|action| matches!(action, Action::SendNow { .. }));
        assert!(c.send_queue_now(None));
        wait_for(|| c.lock().queue.sending.is_none());
        assert!(c.take_events().is_empty());
        assert!(c.current().unwrap().queue.unwrap().enabled);
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn an_uncertain_queue_action_waits_for_an_explicit_new_action() {
        let root = dir("queue-uncertain");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Uncertain {
            detail: "Check Terminal".into(),
        });
        opened(&c, queued("❯ first", true));
        assert!(c.send_queue_now(None));
        wait_for(|| c.lock().queue.sending.is_none());
        assert!(c.current().unwrap().queue.is_none());
        assert!(c
            .lock()
            .followed
            .details
            .as_ref()
            .unwrap()
            .get("input_queue")
            .is_none());
        assert_eq!(
            c.take_events(),
            vec![ComposerEvent::Notice {
                text: "Check Terminal".into(),
                error: true,
                transient: false
            }]
        );
        c.resend();
        c.observe(&state(queued("❯ first", true)));
        c.tick();
        assert_eq!(send_nows(&fake), 1, "fresh state and reload never retry");
        *lock(&fake.outcome) = Box::new(submitted);
        assert!(c.send_queue_now(None));
        wait_for(|| c.lock().queue.sending.is_none());
        let calls = sent(&fake);
        assert_eq!(calls.len(), 2);
        assert_ne!(
            calls[0].2, calls[1].2,
            "an explicit action makes a new request"
        );
        for (_, _, id) in calls {
            assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 4);
        }
    }

    #[test]
    fn a_stale_queue_uses_qts_full_transient_notice() {
        let root = dir("queue-stale");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed {
            detail: "hgs: The native queue is no longer visible; nothing was sent".into(),
        });
        opened(&c, queued("❯ first", true));
        assert!(c.send_queue_now(None));
        wait_for(|| c.lock().queue.sending.is_none());
        assert_eq!(c.take_events(), vec![ComposerEvent::Notice { text: "The native queue is no longer visible; nothing was sent. Activity is being refreshed; check Terminal or the current queue before retrying.".into(), error: true, transient: true }]);
        assert!(c
            .lock()
            .followed
            .details
            .as_ref()
            .unwrap()
            .get("input_queue")
            .is_none());
        assert!(!c.send_queue_now(Some("queue-1".into())));
        assert_eq!(
            c.take_events(),
            vec![notice("Nothing queued can be sent now")]
        );
        assert_eq!(send_nows(&fake), 1);
    }

    #[test]
    fn a_queue_dispatch_panic_releases_the_window_wide_request() {
        let root = dir("queue-panic");
        let (c, fake) = send_hub(&root);
        *lock(&fake.taken) = Box::new(|_| panic!("synthetic queue dispatch panic"));
        opened(&c, queued("❯ first", true));
        assert!(c.send_queue_now(None));
        wait_for(|| c.lock().queue.sending.is_none());
        assert_eq!(
            c.take_events(),
            vec![ComposerEvent::Notice {
                text: "The request ended abnormally.".into(),
                error: true,
                transient: false
            }]
        );
        assert!(c.current().unwrap().queue.is_none());
    }

    #[test]
    fn send_now_keeps_its_target_and_is_quiet_after_selection_changes() {
        let root = dir("queue-switch");
        let (c, fake) = send_hub(&root);
        let (release, held) = std::sync::mpsc::channel();
        let held = Mutex::new(held);
        *lock(&fake.outcome) = Box::new(move |action, id| {
            if matches!(action, Action::SendNow { .. }) {
                lock(&held).recv_timeout(Duration::from_secs(5)).unwrap();
            }
            submitted(action, id)
        });
        opened(&c, queued("❯ first", true));
        assert!(c.send_queue_now(None));
        c.open(None);
        assert!(
            !c.send_queue_now(None),
            "the one in-flight action blocks every selection quietly"
        );
        release.send(()).unwrap();
        wait_for(|| c.lock().queue.sending.is_none());
        assert!(c.take_events().is_empty());
        let calls = sent(&fake);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].0,
            Target {
                computer: "studio".into(),
                name: "claude/sample-project/review".into(),
                run_id: "run-1".into(),
                conversation_id: "conv-1".into()
            }
        );
    }

    #[test]
    fn a_push_whose_send_is_uncertain_never_pushes_a_later_queue() {
        let root = dir("push-uncertain");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Uncertain {
            detail: "unknown delivery".into(),
        });
        let view = opened(&c, working());
        assert_eq!(
            c.send_push(view.handle, 1, view.text_epoch, "later".into(), 5, 5),
            SendReply::Sending
        );
        until(&c, |v| v.uncertain);
        assert_eq!(
            c.take_events(),
            vec![ComposerEvent::Notice {
                text: "The message was not pushed: delivery is not confirmed".into(),
                error: true,
                transient: false
            }]
        );
        c.resend();
        c.observe(&state(queued("❯ later", true)));
        c.tick();
        assert_eq!(send_nows(&fake), 0);
        assert_eq!(sent(&fake).len(), 1, "the message is never sent again");
        assert!(c.lock().queue.pusher.active().is_none());
    }

    #[test]
    fn a_push_cannot_arm_for_another_handle_or_a_stale_editor() {
        let root = dir("push-stale-editor");
        let (c, fake) = send_hub(&root);
        let view = opened(&c, working());
        c.edit(view.handle, 1, view.text_epoch, "kept".into(), 4, 4);
        assert_eq!(
            c.send_push(view.handle + 1, 2, view.text_epoch, "foreign".into(), 7, 7),
            SendReply::Busy
        );
        assert!(c.lock().queue.pusher.active().is_none());
        assert_eq!(
            c.send_push(view.handle, 2, view.text_epoch + 1, "kept".into(), 4, 4),
            SendReply::Busy
        );
        assert!(c.lock().queue.pusher.active().is_none());
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_push_waits_for_its_durable_message_save() {
        let root = dir("push-durable");
        let store = crate::composer::tests::drafts(&root);
        let (c, fake) = crate::composer::send::tests::send_hub_on(&store);
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, working());
        let release = crate::composer::tests::hold(&store);
        assert_eq!(
            c.send_push(view.handle, 1, view.text_epoch, "later".into(), 5, 5),
            SendReply::Sending
        );
        assert!(sent(&fake).is_empty());
        c.observe(&state(queued("❯ later", true)));
        assert_eq!(
            send_nows(&fake),
            0,
            "a matching queue never bypasses the save or its send acknowledgement"
        );
        release.send(()).unwrap();
        until(&c, |v| !v.sending);
        c.observe(&state(queued("❯ later", true)));
        wait_for(|| send_nows(&fake) == 1);
        assert_eq!(
            sent(&fake)
                .iter()
                .filter(|(_, a, _)| matches!(a, Action::Send { .. }))
                .count(),
            1
        );
    }
}
