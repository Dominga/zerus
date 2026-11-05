//! A Send handed to the desktop service (zerus-5wmh.66.4.4, slice 3, the
//! deferred_send kind): the route of the one original and the read-only
//! checks of its result.
//!
//! Only the Send action that reserved and durably saved the checkpoint
//! offers its original ([`Composers::offer`]). A reload or a reopened session
//! never offers anything: it only looks the held identity up. While the
//! service still reports the original in progress, the draft stays uncertain
//! without Allow retry, shows [`STILL_SENDING`] as a normal status and is
//! checked again with a bounded back-off, only while its session is open and
//! the window is shown. A settled result ends it as the direct path would:
//! Recorded clears the draft, a child that never started releases its
//! identity, and Unknown stays uncertain for Allow retry. A service that
//! cannot be read leaves it uncertain with Allow retry.
use super::send::Delivery;
use super::*;
use crate::desktop_service::deferred_send::{self, Checked, SendAnswer, STILL_SENDING};
use zerus_client_core::actions::Outcome;
use zerus_client_core::desktop::deferred_send::Plan;
use zerus_client_core::operation_recovery::deferred_input::{Checkpoint, Kind};

/// The first check after a hand-over, and the longest wait between two.
const FIRST: f64 = 2.0;
const LONGEST: f64 = 30.0;

type SendFn = dyn Fn(&str, &Checkpoint, &[u8]) -> SendAnswer + Send + Sync;
type CheckFn = dyn Fn(&Plan) -> Checked + Send + Sync;

/// The selected desktop service's deferred send route.
pub struct DeferredRoute {
    /// Offers the original once and waits, bounded, for its result.
    pub send: Arc<SendFn>,
    /// One read-only lookup by the exact plan. It never sends anything.
    pub check: Arc<CheckFn>,
}

/// An original the service still reported in progress: what its checks need.
#[derive(Clone, Debug)]
pub(crate) struct Handed {
    pub(crate) plan: Plan,
    /// When the next check is due, on the hub clock.
    pub(crate) next: f64,
    pub(crate) delay: f64,
    /// A check is under way.
    pub(crate) checking: bool,
}

impl Handed {
    fn new(plan: Plan, now: f64) -> Self {
        Handed {
            plan,
            next: now + FIRST,
            delay: FIRST,
            checking: false,
        }
    }
}

/// What a check of a held identity started from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Basis {
    /// The draft's own hand-over: a missing answer is state 5.
    Handed,
    /// A reload's lookup of an identity of unknown path: a missing answer
    /// changes nothing.
    Reload,
}

/// The exact stdin of the one `hgs send` call, as the direct path builds it.
fn stdin(
    name: &str,
    checkpoint: &Checkpoint,
    text: &str,
    attachments: &[Attachment],
) -> Option<Vec<u8>> {
    let target = zerus_client_core::Target {
        computer: String::new(),
        name: name.into(),
        run_id: checkpoint.run_id.clone(),
        conversation_id: checkpoint.conversation_id.clone(),
    };
    zerus_client_core::actions::send_input(
        &target,
        text,
        attachments,
        None,
        None,
        &checkpoint.request_id,
    )
    .ok()
}

impl Composers {
    /// Sets the route (the app, at start): Linux and macOS in
    /// desktop-service mode. Without one every Send goes direct.
    #[cfg(any(target_os = "linux", target_os = "macos", test))]
    pub fn set_deferred(&self, route: DeferredRoute) {
        *lock(&self.0.deferred) = Some(Arc::new(route));
    }

    fn deferred(&self) -> Option<Arc<DeferredRoute>> {
        lock(&self.0.deferred).clone()
    }

    /// The window is hidden or shown again (the webview's visibility). A
    /// shown window checks a held send at once.
    pub fn set_hidden(&self, hidden: bool) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.hidden == hidden {
            return;
        }
        hub.hidden = hidden;
        if !hidden {
            Self::restart_checks(&mut hub, now);
        }
    }

    /// The open composer's held send, and its session's held answers, are
    /// checked at once, their back-off starting over: on opening the
    /// session, and on showing the window.
    pub(crate) fn restart_checks(hub: &mut Hub, now: f64) {
        let Some(key) = hub.open_key.clone() else {
            return;
        };
        super::questions::restart_answer_checks(hub, &key, now);
        if let Some(handed) = hub
            .composers
            .get_mut(&key)
            .and_then(|composer| composer.handed.as_mut())
        {
            handed.next = now;
            handed.delay = FIRST;
        }
    }

    /// Offers `delivery` to the desktop service when it takes it, with the
    /// exact stdin its direct call would get. `None`: the direct path sends
    /// it, decided before any offer. Called by `send_now` once its local row
    /// shows it sending, never on a reload.
    pub(crate) fn offer(&self, delivery: &Delivery) -> Option<SendAnswer> {
        let route = self.deferred()?;
        // v1: a main session on this computer, a plain send.
        if !delivery.session.computer.is_empty()
            || delivery.session.subagent.is_some()
            || delivery
                .compaction
                .as_ref()
                .is_some_and(|id| !id.is_empty())
        {
            return None;
        }
        let checkpoint = self
            .lock()
            .composers
            .get(&delivery.key)
            .and_then(|composer| composer.checkpoint.clone())
            .filter(|checkpoint| {
                checkpoint.kind == Kind::Send && checkpoint.request_id == delivery.request_id
            })?;
        let stdin = stdin(
            &delivery.session.name,
            &checkpoint,
            &delivery.text,
            &delivery.attachments,
        )?;
        match (route.send)(&delivery.session.name, &checkpoint, &stdin) {
            SendAnswer::Direct => None,
            answer => Some(answer),
        }
    }

    /// The end of an offered send, as `send_now` records it.
    pub(crate) fn offered(&self, delivery: &Delivery, answer: SendAnswer) -> Option<Outcome> {
        Some(match answer {
            SendAnswer::Recorded(outcome) => Outcome::Done {
                receipt: serde_json::json!({"request_id": outcome.request_id}),
                cleanup: None,
            },
            SendAnswer::Unsent(detail) => Outcome::Failed { detail },
            SendAnswer::Unconfirmed(detail) => Outcome::Uncertain { detail },
            SendAnswer::Pending(plan) => {
                self.handed_over(delivery, plan);
                return None;
            }
            SendAnswer::Direct => unreachable!("decided before the offer"),
        })
    }

    /// The service still sends it: the delivery ends uncertain, its row
    /// keeps sending, and the draft holds its identity without Allow retry.
    fn handed_over(&self, delivery: &Delivery, plan: Plan) {
        let now = self.now();
        let mut hub = self.lock();
        let key = delivery.key.clone();
        let current = hub
            .composers
            .get(&key)
            .is_some_and(|c| c.delivering == Some(delivery.revision));
        let end = send::End {
            ok: false,
            detail: STILL_SENDING,
            uncertain: true,
            preserve: delivery.preserve,
        };
        self.finish_in(&mut hub, &key, Some(delivery.revision), end);
        if current {
            if let Some(composer) = hub.composers.get_mut(&key) {
                composer.error = false;
                composer.notice = STILL_SENDING.into();
                composer.handed = Some(Handed::new(plan, now));
            }
        }
        drop(hub);
        self.changed();
    }

    /// Each tick: the open composer's due check, and a reloaded draft's one
    /// lookup. Nothing while the window is hidden.
    pub(crate) fn tick_handed(&self, now: f64) {
        let Some(route) = self.deferred() else {
            return;
        };
        let job = {
            let mut hub = self.lock();
            if hub.hidden || hub.closed {
                return;
            }
            let (Some(key), Some(open)) = (hub.open_key.clone(), hub.open.clone()) else {
                return;
            };
            let Some(composer) = hub.composers.get_mut(&key) else {
                return;
            };
            if let Some(handed) = composer.handed.as_mut() {
                if handed.checking || now < handed.next {
                    return;
                }
                handed.checking = true;
                Some((key, composer.handle, handed.plan.clone(), Basis::Handed))
            } else if composer.loaded
                && composer.uncertain
                && !composer.probed
                && !composer.busy()
                && open.computer.is_empty()
                && open.subagent.is_none()
            {
                // A reload's one lookup of the held identity, when the
                // unchanged draft still rebuilds its exact original.
                composer.probed = true;
                composer
                    .checkpoint
                    .as_ref()
                    .filter(|checkpoint| {
                        checkpoint.kind == Kind::Send
                            && send::message_digest(
                                &composer.draft.text,
                                &composer.draft.attachments,
                            ) == checkpoint.message_sha256
                    })
                    .and_then(|checkpoint| {
                        let input = stdin(
                            &open.name,
                            checkpoint,
                            &composer.draft.text,
                            &composer.draft.attachments,
                        )?;
                        deferred_send::plan(&open.name, checkpoint, &input)
                    })
                    .map(|plan| (key, composer.handle, plan, Basis::Reload))
            } else {
                None
            }
        };
        let Some((key, handle, plan, basis)) = job else {
            return;
        };
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            // A check that panicked knows nothing: never a held state 6.
            let checked =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (route.check)(&plan)))
                    .unwrap_or(Checked::Unavailable);
            me.checked(&key, handle, &plan, basis, checked);
        }));
    }

    /// Applies one check's answer while the draft still holds that identity.
    fn checked(&self, key: &str, handle: u64, plan: &Plan, basis: Basis, checked: Checked) {
        let now = self.now();
        let mut hub = self.lock();
        let held = hub.composers.get(key).is_some_and(|c| {
            c.handle == handle
                && !c.busy()
                && c.checkpoint
                    .as_ref()
                    .is_some_and(|checkpoint| checkpoint.request_id == plan.request_id)
                && (basis == Basis::Reload || c.handed.is_some())
        });
        if !held {
            if let Some(handed) = hub.composers.get_mut(key).and_then(|c| c.handed.as_mut()) {
                handed.checking = false;
            }
            return;
        }
        let outcome = match checked {
            Checked::InProgress => {
                if let Some(composer) = hub.composers.get_mut(key) {
                    let handed = composer
                        .handed
                        .get_or_insert_with(|| Handed::new(plan.clone(), now));
                    handed.checking = false;
                    handed.delay = if basis == Basis::Reload {
                        FIRST
                    } else {
                        (handed.delay * 2.0).min(LONGEST)
                    };
                    handed.next = now + handed.delay;
                    composer.error = false;
                    composer.notice = STILL_SENDING.into();
                }
                drop(hub);
                self.changed();
                return;
            }
            Checked::Unavailable if basis == Basis::Reload => return,
            Checked::Unavailable => Outcome::Uncertain {
                detail: deferred_send::UNCONFIRMED.into(),
            },
            Checked::Settled(SendAnswer::Recorded(outcome)) => Outcome::Done {
                receipt: serde_json::json!({"request_id": outcome.request_id}),
                cleanup: None,
            },
            Checked::Settled(SendAnswer::Unsent(detail)) => Outcome::Failed { detail },
            Checked::Settled(_) => Outcome::Uncertain {
                detail: deferred_send::UNCONFIRMED.into(),
            },
        };
        let detail = hub.local.finished_as_is(key, &plan.request_id, &outcome);
        if let Some(composer) = hub.composers.get_mut(key) {
            composer.handed = None;
            match outcome {
                Outcome::Done { .. } => {
                    composer.uncertain = false;
                    composer.error = false;
                    composer.checkpoint = None;
                    composer.notice = "Submitted to terminal".into();
                    // Only the unchanged draft was the message.
                    if send::message_digest(&composer.draft.text, &composer.draft.attachments)
                        == plan.message_sha256
                    {
                        composer.draft = attachments::Draft::default();
                        composer.position = 0;
                        composer.anchor = 0;
                        composer.attachments_changed();
                    }
                }
                // Nothing ran: the identity goes and the draft stays.
                Outcome::Failed { .. } => {
                    composer.uncertain = false;
                    composer.error = true;
                    composer.checkpoint = None;
                    composer.notice = detail;
                }
                // Unknown for good: uncertain until Allow retry.
                Outcome::Uncertain { .. } => {
                    composer.uncertain = true;
                    composer.error = true;
                    composer.notice = detail;
                }
            }
            composer.text_epoch += 1;
            self.save(composer);
        }
        drop(hub);
        self.reconcile();
        self.changed();
    }
}

#[cfg(test)]
mod tests;
