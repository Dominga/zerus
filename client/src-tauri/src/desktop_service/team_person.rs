//! Native owner-card confirmation and receipt-only recovery. No CLI fallback.
use super::{creation_clone::Read, Service};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use zerus_client_core::{
    actions::Exit,
    desktop::{team_person as wire, ui_requests::Request},
    teams::action::{classify, TeamCall, TeamOutcome},
};

#[derive(Default)]
pub(crate) struct Decisions {
    originals: Mutex<BTreeMap<String, wire::Intent>>,
    reviewing: AtomicBool,
}
enum Path {
    Review,
    Read(Box<wire::Intent>),
    Unknown,
}
impl Decisions {
    fn path(&self, plan: &wire::Plan, check_only: bool) -> Path {
        if check_only {
            return self
                .original(plan)
                .map_or(Path::Unknown, |intent| Path::Read(Box::new(intent)));
        }
        Path::Review
    }
    fn original(&self, plan: &wire::Plan) -> Option<wire::Intent> {
        self.originals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&plan.request_id)
            .filter(|intent| &intent.plan == plan)
            .cloned()
    }
    fn reserve(&self, intent: &wire::Intent) -> bool {
        let mut originals = self
            .originals
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if originals.contains_key(&intent.plan.request_id)
            || originals.len() >= zerus_client_core::teams::action::intents::INTENTS_KEPT
        {
            return false;
        }
        originals.insert(intent.plan.request_id.clone(), intent.clone());
        true
    }
    pub(crate) fn dismiss(&self, request_id: &str) {
        self.originals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(request_id);
    }
}
struct Confirmed(wire::Intent);
impl Confirmed {
    /// Only the native dialog's result is passed here in production.
    fn after_dialog(review: wire::Review, accepted: bool) -> Option<Self> {
        (accepted && review.valid()).then_some(Self(review.intent))
    }
}
struct ReviewSlot<'a>(&'a AtomicBool);
impl Drop for ReviewSlot<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn failed(call: &TeamCall, code: &str, detail: &str) -> TeamOutcome {
    TeamOutcome::Failed {
        request_id: call.request_id.clone(),
        code: code.into(),
        detail: detail.into(),
    }
}
fn unknown(call: &TeamCall) -> TeamOutcome {
    TeamOutcome::Uncertain { request_id: call.request_id.clone(), detail: "The original owner-card decision could not be confirmed. Check the original receipt; the decision will not be sent again.".into() }
}
fn outcome(call: &TeamCall, intent: &wire::Intent, bytes: &[u8]) -> TeamOutcome {
    let value: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
    if value["intent"] != json!(intent) || value["reply"]["request_id"] != call.request_id {
        return unknown(call);
    }
    let reply = &value["reply"];
    if reply["ok"] == true
        && (reply["revision"].as_u64() != intent.plan.expected_revision.checked_add(1)
            || reply["actor"] != json!({"kind":"person","person":"local","origin":"desktop_ui"}))
    {
        return unknown(call);
    }
    classify(
        call,
        Exit::Code {
            code: if reply["ok"] == true { 0 } else { 1 },
            stdout: serde_json::to_vec(reply).unwrap_or_default(),
            stderr: String::new(),
        },
    )
}

/// Runs off the GUI thread. The renderer can ask to review an exact card; only
/// this native dialog's acceptance reaches prepare_ui_request.
pub(crate) fn run(
    app: tauri::AppHandle,
    service: Option<Arc<Service>>,
    decisions: Arc<Decisions>,
    call: TeamCall,
    check_only: bool,
) -> TeamOutcome {
    if crate::actions::blocked() {
        return failed(
            &call,
            "person_gesture_required",
            "Owner-card decisions require the live Zerus window.",
        );
    }
    let Some(plan) = wire::Plan::from_call(&call) else {
        return failed(
            &call,
            "invalid_request",
            "Read the exact owner card before deciding it.",
        );
    };
    let Ok(read) = Read::new(service) else {
        return if check_only {
            unknown(&call)
        } else {
            failed(
                &call,
                "person_gesture_required",
                "Connect this window to its desktop service before deciding an owner card.",
            )
        };
    };
    if !read.supports(wire::HELP_MARKER).unwrap_or(false) {
        return if check_only {
            unknown(&call)
        } else {
            failed(
                &call,
                "person_gesture_required",
                "This desktop service does not support native owner-card decisions. Update Zerus.",
            )
        };
    }
    match decisions.path(&plan, check_only) {
        Path::Read(intent) => {
            return read
                .exchange(
                    None,
                    json!({"op":"team_person_result","intent":intent}),
                    false,
                    None,
                )
                .map_or_else(|_| unknown(&call), |bytes| outcome(&call, &intent, &bytes))
        }
        Path::Unknown => return unknown(&call),
        Path::Review => (),
    }
    if decisions.reviewing.swap(true, Ordering::AcqRel) {
        return failed(
            &call,
            "person_gesture_busy",
            "Finish the open owner-card confirmation first.",
        );
    }
    let _slot = ReviewSlot(&decisions.reviewing);
    {
        let originals = decisions
            .originals
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if originals.contains_key(&call.request_id) {
            return unknown(&call);
        }
        if originals.len() >= zerus_client_core::teams::action::intents::INTENTS_KEPT {
            return failed(
                &call,
                "person_gesture_busy",
                "Check the unresolved owner-card decisions first.",
            );
        }
    }
    let review = read
        .exchange(
            None,
            json!({"op":"team_person_prepare","scope":read.scope,"plan":plan}),
            false,
            None,
        )
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| serde_json::from_value::<wire::Review>(value["review"].clone()).ok());
    let Some(review) = review.filter(|review| {
        review.valid() && review.intent.plan == plan && review.intent.scope == read.scope
    }) else {
        return failed(
            &call,
            "owner_card_changed",
            "The exact owner card is unavailable or changed. Read the team again.",
        );
    };
    let Some(window) = crate::main_window(&app) else {
        return failed(
            &call,
            "person_gesture_required",
            "The main window is unavailable.",
        );
    };
    let title = crate::native_text::render(&zerus_client_core::text!("Decide owner card"));
    let parameters = review
        .card_parameters
        .iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>()
        .join("\n");
    let notes = review.card_notes.join("\n");
    let body = format!(
        "{}\n\n{}\n\n{}\n{}\n{}\n\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}",
        review.team_name,
        review.card_kind,
        review.card_text,
        parameters,
        notes,
        crate::native_text::render(&zerus_client_core::text!("Team")),
        plan.team,
        crate::native_text::render(&zerus_client_core::text!("Card")),
        plan.card,
        crate::native_text::render(&zerus_client_core::text!("Decision")),
        plan.decision,
        crate::native_text::render(&zerus_client_core::text!("Computer")),
        review.intent.machine,
        crate::native_text::render(&zerus_client_core::text!("Revision")),
        plan.expected_revision
    );
    let accepted = app
        .dialog()
        .message(body)
        .title(title.clone())
        .parent(&window)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            title,
            crate::native_text::render(&zerus_client_core::text!("Cancel")),
        ))
        .blocking_show();
    let Some(confirmed) = Confirmed::after_dialog(review, accepted) else {
        return failed(
            &call,
            "person_gesture_cancelled",
            "The owner-card decision was cancelled. Nothing was sent.",
        );
    };
    // Consent is minted only after acceptance, for the unchanged connection.
    if !read.current() {
        return failed(
            &call,
            "person_gesture_required",
            "The selected desktop service changed during confirmation.",
        );
    }
    let intent = confirmed.0;
    if !decisions.reserve(&intent) {
        return unknown(&call);
    }
    let result = read
        .exchange(
            Some(Request::TeamPersonDecide {
                intent: Box::new(intent.clone()),
            }),
            Value::Null,
            false,
            None,
        )
        .map_or_else(|_| unknown(&call), |bytes| outcome(&call, &intent, &bytes));
    if matches!(
        result,
        TeamOutcome::Done { .. } | TeamOutcome::Failed { .. }
    ) {
        decisions
            .originals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&call.request_id);
    }
    result
}

#[cfg(test)]
mod tests;
