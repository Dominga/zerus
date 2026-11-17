//! Task form originals are saved before their one authority-bearing RPC.
//! Recovery only reads that immutable original; it never prepares a new plan.
use super::*;
use serde_json::Map;

#[derive(Debug)]
pub(crate) struct TaskError {
    pub(crate) uncertain: bool,
    pub(crate) message: String,
    pub(crate) closed_without_task: bool,
}
impl TaskError {
    fn not_sent(message: impl Into<String>) -> Self {
        Self {
            uncertain: false,
            closed_without_task: false,
            message: message.into(),
        }
    }
    fn unknown() -> Self {
        Self{uncertain:true,closed_without_task:false,message:"The original team intake is unconfirmed. Check its original result; it will not be sent again.".into()}
    }
}
fn matches(draft: &wire::TaskDraft, original: &wire::TaskOriginal) -> bool {
    original.valid() && draft.matches_plan(&original.plan)
}

fn outcome(original: &wire::TaskOriginal, bytes: &[u8]) -> Result<Map<String, Value>, TaskError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| TaskError::unknown())?;
    if value["intent"] != json!(original.intent)
        || value["reply"]["request_id"] != original.plan.request_id
    {
        return Err(TaskError::unknown());
    }
    let reply = value["reply"].as_object().ok_or_else(TaskError::unknown)?;
    if matches!(
        reply.get("code").and_then(Value::as_str),
        Some(
            "storage_failed"
                | "team_person_action_unknown"
                | "team_person_action_evidence_unavailable"
        )
    ) {
        return Err(TaskError::unknown());
    }
    if reply.get("task_exists") == Some(&Value::Bool(true)) {
        let task = match &original.plan.action {
            wire::Action::NewTask { .. } => original.plan.request_id.as_str(),
            wire::Action::Intake { task, .. } => task,
            _ => return Err(TaskError::unknown()),
        };
        let project = match &original.plan.action {
            wire::Action::NewTask { project, .. } | wire::Action::Intake { project, .. } => project,
            _ => return Err(TaskError::unknown()),
        };
        if reply.get("actor")
            != Some(&json!({"kind":"person","person":"local","origin":"desktop_ui"}))
            || value["reply"]["task"]["uuid"] != task
            || value["reply"]["task"]["project"] != *project
            || value["reply"]["team"]["id"] != original.plan.team
        {
            return Err(TaskError::unknown());
        }
        return Ok(reply.clone());
    }
    if reply.get("task_exists") == Some(&Value::Bool(false))
        && reply.get("actor")
            == Some(&json!({"kind":"person","person":"local","origin":"desktop_ui"}))
        && value["reply"]["team"]["id"] == original.plan.team
        && value["reply"]["team"]["queued"] == false
        && value["reply"]["task"].is_null()
    {
        return Err(TaskError {
            uncertain: false,
            closed_without_task: true,
            message: reply
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("The original intake was refused before its task was saved.")
                .into(),
        });
    }
    // An exact negative is shown as its refusal. Keeping the saved original
    // still prevents a fresh native confirmation during form recovery.
    Err(TaskError {
        uncertain: true,
        closed_without_task: false,
        message: reply
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("The original team intake was refused. Nothing was replayed.")
            .into(),
    })
}

pub(crate) fn run_task(
    app: &tauri::AppHandle,
    service: Option<Arc<Service>>,
    actions: &Actions,
    draft: &wire::TaskDraft,
    original: Option<wire::TaskOriginal>,
    recover: bool,
    checkpoint: impl FnMut(&wire::TaskOriginal) -> Result<(), String>,
) -> Result<Map<String, Value>, TaskError> {
    if crate::actions::blocked() {
        return Err(TaskError::not_sent(
            "Team intake requires the live Zerus window.",
        ));
    }
    if !draft.valid() {
        return Err(TaskError::not_sent("The exact task intake is invalid."));
    }
    let read = Read::new(service)
        .map_err(|_| TaskError::not_sent("Connect this window to its desktop service."))?;
    if !read.supports(wire::HELP_MARKER).unwrap_or(false) {
        return Err(TaskError::not_sent(
            "Update the selected desktop service to confirm team intake.",
        ));
    }
    if let Some(original) = original {
        if !matches(draft, &original) {
            return Err(TaskError::unknown());
        }
        return read
            .exchange(
                None,
                json!({"op":"team_person_action_result","intent":original.intent}),
                false,
                None,
            )
            .map_err(|_| TaskError::unknown())
            .and_then(|bytes| outcome(&original, &bytes));
    }
    if recover {
        return Err(TaskError::unknown());
    }
    if actions.reviewing.swap(true, Ordering::AcqRel) {
        return Err(TaskError::not_sent(
            "Finish the open team confirmation first.",
        ));
    }
    let _slot = ReviewSlot(&actions.reviewing);
    let bytes = read
        .exchange(
            None,
            json!({"op":"team_person_task_prepare","scope":read.scope,"draft":draft}),
            false,
            None,
        )
        .map_err(|_| {
            TaskError::not_sent("The exact team/task review is unavailable. Nothing was sent.")
        })?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| TaskError::not_sent("The exact team/task review is unavailable."))?;
    let plan: wire::Plan = serde_json::from_value(value["plan"].clone())
        .map_err(|_| TaskError::not_sent("Read the exact task and team again."))?;
    let review: wire::Review = serde_json::from_value(value["review"].clone())
        .map_err(|_| TaskError::not_sent("Read the exact task and team again."))?;
    let original = wire::TaskOriginal {
        plan,
        intent: review.intent.clone(),
    };
    if value["ok"] != true
        || !review.valid()
        || !matches(draft, &original)
        || original.intent.scope != read.scope
    {
        return Err(TaskError::not_sent("The exact team/task review changed."));
    }
    if !stage(&read, &original.plan) {
        return Err(TaskError::not_sent(
            "The complete task plan could not be prepared.",
        ));
    }
    let confirmed = confirm(app, &original.plan, review).map_err(TaskError::not_sent)?;
    if confirmed.0 != original.intent || !read.current() {
        return Err(TaskError::not_sent(
            "The selected service changed during confirmation.",
        ));
    }
    offer(confirmed, &original, checkpoint, |request| {
        read.exchange(Some(request), Value::Null, false, None)
    })
}

/// This is the only task execution offer. Its input is the opaque native
/// confirmation, and persistence must complete before the native request.
fn offer(
    confirmed: Confirmed,
    original: &wire::TaskOriginal,
    checkpoint: impl FnOnce(&wire::TaskOriginal) -> Result<(), String>,
    send: impl FnOnce(Request) -> Result<Vec<u8>, String>,
) -> Result<Map<String, Value>, TaskError> {
    if confirmed.0 != original.intent || !original.valid() {
        return Err(TaskError::unknown());
    }
    checkpoint(original).map_err(|message| TaskError {
        uncertain: true,
        closed_without_task: false,
        message,
    })?;
    let bytes = send(Request::TeamPersonAction {
        intent: Box::new(confirmed.0),
    })
    .map_err(|_| TaskError::unknown())?;
    outcome(original, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_task_offer_requires_the_exact_durable_original_before_its_only_request() {
        use std::cell::Cell;
        let root = crate::composer::tests::dir("native-task-offer-checkpoint");
        let drafts = crate::composer::tests::drafts(&root);
        let call = super::super::tests::call();
        let mut review = super::super::tests::review(&call);
        let plan = wire::Plan {
            request_id: call.request_id.clone(),
            team: call.team.clone(),
            expected_revision: 2,
            action: wire::Action::NewTask {
                project: "example".into(),
                title: "Bound task".into(),
                backlog: false,
            },
        };
        review.intent.plan = plan.reference().unwrap();
        let original = wire::TaskOriginal {
            plan,
            intent: review.intent.clone(),
        };
        let sent = Cell::new(0);
        let confirmed = Confirmed::after_dialog(review.clone(), true).unwrap();
        assert!(offer(
            confirmed,
            &original,
            |_| Err("private checkpoint failure".into()),
            |_| {
                sent.set(sent.get() + 1);
                panic!("a failed checkpoint cannot submit")
            }
        )
        .is_err());
        assert_eq!(sent.get(), 0);
        let confirmed = Confirmed::after_dialog(review, true).unwrap();
        let key = "tasks/native-model-original";
        let answer=offer(confirmed,&original,|original|{
            let (tx,rx)=std::sync::mpsc::channel();
            let mut next=zerus_client_core::drafts::StoredDraft::default();next.form_state.insert("native_original".into(),json!(original));
            drafts.replace_if_unchanged(key,Default::default(),next,move |result|{let _=tx.send(result);});
            if rx.recv().map_err(|e|e.to_string())?? {Ok(())}else{Err("changed".into())}
        },|request|{
            sent.set(sent.get()+1);
            assert!(matches!(request,Request::TeamPersonAction{intent} if *intent==original.intent));
            let saved=crate::task_editor::load(&drafts,key)?;
            assert_eq!(saved.draft.form_state["native_original"],json!(original),"durable data precedes the actual native request callback");
            Ok(serde_json::to_vec(&json!({"intent":original.intent,"reply":{"ok":true,"request_id":original.plan.request_id,"actor":{"kind":"person","person":"local","origin":"desktop_ui"},"task_exists":true,"task":{"uuid":original.plan.request_id,"project":"example"},"team":{"id":original.plan.team,"queued":true}}})).unwrap())
        }).unwrap();
        assert_eq!(sent.get(), 1);
        assert_eq!(answer["team"]["queued"], true);
    }
    #[test]
    fn task_recovery_matches_immutable_original_and_never_adopts_revision_or_action() {
        let draft = wire::TaskDraft {
            request_id: uuid::Uuid::new_v4().to_string(),
            team: uuid::Uuid::new_v4().to_string(),
            action: wire::Action::NewTask {
                project: "example".into(),
                title: "Original title".into(),
                backlog: false,
            },
        };
        let plan = draft.plan(8).unwrap();
        let original = wire::TaskOriginal {
            intent: wire::Intent {
                scope: zerus_client_core::desktop::creation_clone::Scope {
                    configuration: "a".repeat(64),
                    service_epoch: uuid::Uuid::new_v4().to_string(),
                },
                machine: uuid::Uuid::new_v4().to_string(),
                store: wire::StoreBinding::Existing {
                    fingerprint: "b".repeat(64),
                },
                facts: "c".repeat(64),
                request: "d".repeat(64),
                plan: plan.reference().unwrap(),
            },
            plan,
        };
        assert!(matches(&draft, &original));
        let mut changed = draft.clone();
        changed.action = wire::Action::NewTask {
            project: "example".into(),
            title: "Other title".into(),
            backlog: false,
        };
        assert!(!matches(&changed, &original));
        let value = json!({"intent":original.intent,"reply":{"ok":true,"request_id":draft.request_id,"actor":{"kind":"person","person":"local","origin":"desktop_ui"},"task_exists":true,"task":{"uuid":draft.request_id,"project":"example"},"team":{"id":draft.team,"queued":true}}});
        assert!(outcome(&original, &serde_json::to_vec(&value).unwrap()).is_ok());
        let mut foreign = value.clone();
        foreign["reply"]["actor"]["origin"] = json!("cli_interim");
        assert!(outcome(&original, &serde_json::to_vec(&foreign).unwrap()).is_err());
        let negative = json!({"intent":original.intent,"reply":{"ok":false,"request_id":draft.request_id,"actor":{"kind":"person","person":"local","origin":"desktop_ui"},"task_exists":false,"team":{"id":draft.team,"queued":false},"message":"Consent expired before save"}});
        let closed = outcome(&original, &serde_json::to_vec(&negative).unwrap()).unwrap_err();
        assert!(closed.closed_without_task);
        assert!(!closed.uncertain);
        let mut missing = negative;
        missing["reply"]
            .as_object_mut()
            .unwrap()
            .remove("task_exists");
        assert!(
            !outcome(&original, &serde_json::to_vec(&missing).unwrap())
                .unwrap_err()
                .closed_without_task
        );
        let mut unknown = value;
        unknown["reply"]["code"] = json!("team_person_action_unknown");
        assert!(
            outcome(&original, &serde_json::to_vec(&unknown).unwrap())
                .unwrap_err()
                .uncertain
        );
    }
}
