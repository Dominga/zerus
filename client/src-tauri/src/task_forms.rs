//! Two bounded private title forms, using the existing ordered draft writer.
//! A reserved new card is recovered only by an explicit submit of that same
//! identity; reads and autosaves never invoke a task mutation.
use crate::{
    composer::Composers,
    task_details::{TaskReads, TaskWrites},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use tauri::State;
use zerus_client_core::{
    drafts::{DraftStore, StoredDraft},
    task_details,
    tasks_write::{self, TaskCall, TaskTarget},
};

const CHANGED: &str = "The selected task changed. Your draft was kept.";
const TITLE_ERROR: &str =
    "Enter a single-line title, up to 512 bytes. Enter saves; Escape cancels.";
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Title,
    NewCard,
}
impl Kind {
    fn index(self) -> usize {
        if self == Self::Title {
            0
        } else {
            1
        }
    }
    fn key(self, project: &str, task: &str) -> String {
        if self == Self::Title {
            format!("tasks/titleDraft/{project}/{task}")
        } else {
            format!("tasks/newCard/{project}/title")
        }
    }
}
struct Slot {
    kind: Kind,
    project: String,
    task: String,
    handle: u64,
    rev: u64,
    stored: StoredDraft,
    durable: StoredDraft,
    blocked: bool,
    unsaved: bool,
    error: String,
}
#[derive(Clone, Default)]
struct Editor {
    slot: Arc<Mutex<Option<Slot>>>,
    lane: TaskReads,
}
#[derive(Default)]
struct Inner {
    editors: [Editor; 2],
    handles: AtomicU64,
}
#[derive(Clone, Default)]
pub struct TaskForms(Arc<Inner>);

fn view(slot: &Slot, saved: bool) -> Value {
    let pending = slot.stored.form_state.contains_key("request");
    let read_only =
        pending || (slot.kind == Kind::NewCard && (slot.stored.sending || slot.stored.uncertain));
    json!({"project":slot.project,"task":slot.task,"mode":slot.kind,"handle":slot.handle,"rev":slot.rev,
        "text":if slot.kind==Kind::NewCard && pending {slot.stored.form_state["request"]["title"].as_str().unwrap_or("")} else {&slot.stored.draft.text},"position":slot.stored.position,"anchor":slot.stored.anchor,
        "files":[],"error":slot.error,"blocked":slot.blocked,"unsaved":slot.unsaved,
        "recovery":pending,"read_only":read_only,"saved":saved})
}
fn checkpoint(slot: &mut Slot, store: &DraftStore) -> Result<(), String> {
    slot.unsaved = true;
    let (tx, rx) = mpsc::channel();
    store.replace_if_unchanged(
        &slot.kind.key(&slot.project, &slot.task),
        slot.durable.clone(),
        slot.stored.clone(),
        move |answer| {
            let _ = tx.send(answer);
        },
    );
    let saved = rx
        .recv()
        .map_err(|_| "The draft could not be saved.".to_owned())??;
    if !saved {
        slot.blocked = true;
        return Err(CHANGED.into());
    }
    slot.durable = slot.stored.clone();
    slot.unsaved = false;
    Ok(())
}
fn document(project: &str, task: &str) -> Result<Value, String> {
    let reply = tasks_write::run(
        &zerus_client_core::default_hgs(),
        &task_details::document_call(project, task)?,
    )?;
    task_details::checked(&reply, project, task)
}
fn title(value: &str) -> Result<&str, String> {
    let text = value.trim();
    if text.is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
        Err(TITLE_ERROR.into())
    } else {
        Ok(text)
    }
}
fn opened(
    state: &TaskForms,
    store: &DraftStore,
    kind: Kind,
    project: String,
    task: String,
    read: impl Fn(&str, &str) -> Result<Value, String>,
) -> Result<Slot, String> {
    let editor = &state.0.editors[kind.index()];
    if let Some(old) = crate::composer::lock(&editor.slot)
        .as_mut()
        .filter(|s| s.unsaved)
    {
        checkpoint(old, store)?;
    }
    let loaded = crate::task_editor::load(store, &kind.key(&project, &task))?;
    let durable = loaded.draft;
    let blocked = loaded.storage_error.is_some()
        || !durable.draft.attachments.is_empty()
        || (kind == Kind::Title && durable.form_state.contains_key("request"))
        || durable.form_state.get("schema").is_some_and(|v| v != 1);
    let mut slot = Slot {
        kind,
        project,
        task,
        handle: state.0.handles.fetch_add(1, Ordering::SeqCst) + 1,
        rev: 0,
        stored: durable.clone(),
        durable,
        blocked,
        unsaved: false,
        error: loaded.storage_error.unwrap_or_default(),
    };
    if blocked {
        slot.error = zerus_client_core::drafts::NOT_RESTORED.into();
        return Ok(slot);
    }
    if kind == Kind::Title
        && !slot.stored.form_state.contains_key("initialized")
        && slot.stored.draft.text.is_empty()
        && !slot.stored.sending
        && !slot.stored.uncertain
    {
        let detail = read(&slot.project, &slot.task)?;
        if detail["writable"] == false {
            return Err("Update Zerus to edit this task.".into());
        }
        slot.stored
            .form_state
            .insert("initialized".into(), json!(true));
        slot.stored.draft.text = detail["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| detail["text"].as_str().and_then(|s| s.lines().next()))
            .unwrap_or("")
            .chars()
            .take(512)
            .collect();
        checkpoint(&mut slot, store)?;
    }
    if kind == Kind::NewCard && slot.stored.form_state.contains_key("request") {
        request(&slot)?;
        slot.error = "Creation was interrupted. Press Enter to recover this same card.".into();
    } else if kind == Kind::NewCard && (slot.stored.sending || slot.stored.uncertain) {
        slot.error = "The reserved card is unavailable. Nothing was sent.".into();
    }
    Ok(slot)
}
#[tauri::command]
pub async fn task_form_open(
    state: State<'_, TaskForms>,
    composers: State<'_, Composers>,
    kind: Kind,
    project: String,
    task: String,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    if project.is_empty() || project.len() > 128 || project.chars().any(char::is_control) {
        return Err("Invalid task project.".into());
    }
    if kind == Kind::Title {
        task_details::document_call(&project, &task)?;
    } else if !task.is_empty() {
        return Err(CHANGED.into());
    }
    let permit = state.0.editors[kind.index()].lane.begin()?;
    let state = state.inner().clone();
    let composers = composers.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let slot = opened(&state, &composers.0.store, kind, project, task, document)?;
        let answer = view(&slot, false);
        *crate::composer::lock(&state.0.editors[kind.index()].slot) = Some(slot);
        Ok(answer)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[derive(Deserialize)]
pub struct Edit {
    handle: u64,
    rev: u64,
    text: String,
    position: i64,
    anchor: i64,
}
fn edited(slot: &mut Slot, input: Edit) -> Result<(), String> {
    if slot.handle != input.handle || input.rev < slot.rev || slot.blocked {
        return Err(CHANGED.into());
    }
    if slot.stored.form_state.contains_key("request") {
        let (_, intent) = request(slot)?;
        if input.text != intent.title {
            return Err("Recover the reserved card before creating another.".into());
        }
        // Retrying an unsaved checkpoint may repeat only this read-only view.
        // Keep the original text, request and delivery flags unchanged.
        let units = input.text.encode_utf16().count() as i64;
        slot.stored.position = input.position.clamp(0, units);
        slot.stored.anchor = input.anchor.clamp(0, units);
        slot.rev = input.rev;
        return Ok(());
    }
    if input.text.len() > crate::composer::MAX_TEXT_BYTES {
        return Err("Message exceeds 64 KiB.".into());
    }
    if slot.kind == Kind::Title && slot.stored.draft.text != input.text {
        slot.stored.sending = false;
        slot.stored.uncertain = false;
    }
    let units = input.text.encode_utf16().count() as i64;
    if slot.kind == Kind::Title {
        slot.stored
            .form_state
            .insert("initialized".into(), json!(true));
    }
    slot.stored.draft.text = input.text;
    slot.stored.position = input.position.clamp(0, units);
    slot.stored.anchor = input.anchor.clamp(0, units);
    slot.rev = input.rev;
    slot.error.clear();
    Ok(())
}
#[tauri::command]
pub async fn task_form_edit(
    state: State<'_, TaskForms>,
    composers: State<'_, Composers>,
    kind: Kind,
    input: Edit,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let editor = state.0.editors[kind.index()].clone();
    let permit = editor.lane.begin()?;
    let composers = composers.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let mut guard = crate::composer::lock(&editor.slot);
        let slot = guard.as_mut().ok_or(CHANGED)?;
        edited(slot, input)?;
        if let Err(e) = checkpoint(slot, &composers.0.store) {
            slot.error = e;
            return Err(slot.error.clone());
        }
        Ok(view(slot, false))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[derive(Deserialize)]
struct Request {
    project: String,
    title: String,
    request_id: String,
    stage: String,
    session: String,
    host: String,
    session_id: String,
}
fn request(slot: &Slot) -> Result<(Value, Request), String> {
    let raw = slot
        .stored
        .form_state
        .get("request")
        .cloned()
        .ok_or("The reserved card is unavailable. Nothing was sent.")?;
    let parsed: Request = serde_json::from_value(raw.clone())
        .map_err(|_| "The reserved card is unavailable. Nothing was sent.")?;
    if parsed.project != slot.project
        || !zerus_client_core::creation::canonical_uuid(&parsed.request_id)
        || !matches!(parsed.stage.as_str(), "todo" | "backlog")
        || title(&parsed.title).is_err()
        || parsed.session.is_empty()
        || [&parsed.session, &parsed.host, &parsed.session_id]
            .iter()
            .any(|s| s.len() > 8192 || s.chars().any(char::is_control))
        || (parsed.session == "any" && (!parsed.host.is_empty() || !parsed.session_id.is_empty()))
        || (parsed.session != "any" && parsed.session_id.is_empty())
    {
        return Err("The reserved card is invalid. Nothing was sent.".into());
    }
    Ok((raw, parsed))
}
fn submitted(
    slot: &mut Slot,
    store: &DraftStore,
    recover: bool,
    backlog: bool,
    target: TaskTarget,
    read: impl Fn(&str, &str) -> Result<Value, String>,
    run: impl Fn(&TaskCall) -> Result<serde_json::Map<String, Value>, String>,
) -> Result<(bool, Option<String>), String> {
    if slot.blocked {
        return Err(zerus_client_core::drafts::NOT_RESTORED.into());
    }
    let wanted = if slot.kind == Kind::NewCard && slot.stored.form_state.contains_key("request") {
        request(slot)?.1.title
    } else {
        if slot.kind == Kind::NewCard {
            let text = slot.stored.draft.text.trim();
            if text.is_empty() {
                return Err(String::new());
            }
            if text.chars().any(char::is_control) {
                return Err("Use a single-line task title without control characters.".into());
            }
            if text.len() > 512 {
                return Err("Use a shorter task title (up to 512 bytes).".into());
            }
            text.to_owned()
        } else {
            title(&slot.stored.draft.text)?.to_owned()
        }
    };
    checkpoint(slot, store)?;
    let result = if slot.kind == Kind::Title {
        if recover {
            return Err("Review the saved task title before saving another title.".into());
        }
        let detail = read(&slot.project, &slot.task)?;
        if detail["writable"] == false {
            return Err("Update Zerus to edit this task.".into());
        }
        if detail["title"].as_str() == Some(&wanted) {
            Ok(None)
        } else if slot.stored.uncertain || slot.stored.sending {
            Err("Title save unconfirmed. Review the task before saving another title. Your edit is kept.".into())
        } else {
            slot.stored.sending = true;
            checkpoint(slot, store)?;
            run(&TaskCall::rename(&slot.project, &slot.task, &wanted)).and_then(|r| {
                let t = r
                    .get("task")
                    .ok_or("Task title save unconfirmed. Your edit is kept.")?;
                if t["uuid"].as_str() != Some(&slot.task)
                    || t["project"].as_str() != Some(&slot.project)
                    || t["title"].as_str() != Some(&wanted)
                {
                    return Err("Task title save unconfirmed. Your edit is kept.".into());
                }
                Ok(None)
            })
        }
    } else {
        if !slot.stored.form_state.contains_key("request") {
            if recover || slot.stored.sending || slot.stored.uncertain {
                return Err("The reserved card is unavailable. Nothing was sent.".into());
            }
            slot.stored.form_state.insert("schema".into(), json!(1));
            slot.stored.form_state.insert("request".into(),json!({"project":slot.project,"title":wanted,"request_id":uuid::Uuid::new_v4().to_string(),"stage":if backlog {"backlog"} else {"todo"},"session":if target.session.is_empty(){"any"}else{&target.session},"host":target.host,"session_id":target.session_id}));
            checkpoint(slot, store)?;
        } else if !recover {
            return Err("Creation was interrupted. Press Enter to recover this same card.".into());
        }
        let (_, intent) = request(slot)?;
        let target = TaskTarget {
            host: intent.host,
            session: if intent.session == "any" {
                String::new()
            } else {
                intent.session
            },
            session_id: intent.session_id,
            mode: String::new(),
        };
        run(&TaskCall::add_named(
            &intent.project,
            &intent.title,
            &intent.request_id,
            &target,
            intent.stage == "backlog",
        ))
        .and_then(|reply| {
            let task = tasks_write::created(&reply, &intent.request_id)?;
            if task["project"].as_str() != Some(&slot.project) {
                return Err(
                    "Task creation needs review: the reply did not match the reserved card.".into(),
                );
            }
            Ok(Some(intent.request_id))
        })
    };
    let created = match result {
        Ok(v) => v,
        Err(error) => {
            slot.stored.sending = false;
            slot.stored.uncertain = true;
            checkpoint(slot, store)?;
            return Err(error);
        }
    };
    slot.stored.draft = Default::default();
    slot.stored.position = 0;
    slot.stored.anchor = 0;
    slot.stored.sending = false;
    slot.stored.uncertain = false;
    slot.stored.form_state.remove("request");
    slot.stored.form_state.remove("initialized");
    checkpoint(slot, store)?;
    slot.rev += 1;
    slot.error.clear();
    Ok((true, created))
}
#[derive(Deserialize)]
pub struct Submit {
    kind: Kind,
    handle: u64,
    rev: u64,
    recover: bool,
    backlog: bool,
    target: Option<zerus_client_core::actions::Target>,
    catalog_key: Option<String>,
    session_id: Option<String>,
}
#[tauri::command]
pub async fn task_form_submit(
    state: State<'_, TaskForms>,
    writes: State<'_, TaskWrites>,
    composers: State<'_, Composers>,
    app: State<'_, crate::commands::AppState>,
    input: Submit,
) -> Result<Value, String> {
    let Submit {
        kind,
        handle,
        rev,
        recover,
        backlog,
        target,
        catalog_key,
        session_id,
    } = input;
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let write = writes.begin()?;
    let editor = state.0.editors[kind.index()].clone();
    let permit = editor.lane.begin()?;
    let composers = composers.inner().clone();
    let pump = app.pump();
    tauri::async_runtime::spawn_blocking(move || {
        let (_write, _permit) = (write, permit);
        let mut guard = crate::composer::lock(&editor.slot);
        let slot = guard
            .as_mut()
            .filter(|s| s.handle == handle && s.rev == rev)
            .ok_or(CHANGED)?;
        // This is metadata creation, with no executor delivery. A known offline
        // target can receive a queued card; its stable identity still must match.
        let assignment = if kind == Kind::NewCard && !slot.stored.form_state.contains_key("request")
        {
            if let Some(target) = target {
                let row = pump
                    .row(&target.computer, catalog_key.as_deref().ok_or(CHANGED)?)
                    .ok_or(CHANGED)?;
                if !pump
                    .members_of(&slot.project)
                    .iter()
                    .any(|(key, _)| key == &format!("{}\n{}", row.machine, row.name))
                {
                    return Err(
                        "The selected session belongs to another project. Nothing was changed."
                            .into(),
                    );
                }
                crate::task_details::assignment_target(
                    &row,
                    &target,
                    session_id.as_deref().unwrap_or(""),
                )?
            } else {
                if catalog_key.is_some() || session_id.is_some() {
                    return Err(CHANGED.into());
                }
                TaskTarget::default()
            }
        } else {
            TaskTarget::default()
        };
        let result = submitted(
            slot,
            &composers.0.store,
            recover,
            backlog,
            assignment,
            document,
            |call| tasks_write::run(&zerus_client_core::default_hgs(), call),
        );
        let (saved, created) = match result {
            Ok(value) => value,
            Err(error) => {
                slot.error = if slot.kind == Kind::Title && error != TITLE_ERROR {
                    format!("Title was not saved. Your edit is kept. Try Enter again.\n{error}")
                } else if slot.kind == Kind::NewCard
                    && slot.stored.form_state.contains_key("request")
                {
                    format!("{error}\nPress Enter to retry with the same card identity.")
                } else {
                    error
                };
                (false, None)
            }
        };
        let mut answer = view(slot, saved);
        answer["created"] = json!(created);
        Ok(answer)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn task_form_cancel(
    state: State<'_, TaskForms>,
    composers: State<'_, Composers>,
    kind: Kind,
    handle: u64,
    rev: u64,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    if kind != Kind::Title {
        return Err("The reserved card was kept.".into());
    }
    let editor = state.0.editors[kind.index()].clone();
    let permit = editor.lane.begin()?;
    let composers = composers.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let mut guard = crate::composer::lock(&editor.slot);
        let slot = guard
            .as_mut()
            .filter(|s| s.handle == handle && s.rev == rev && !s.blocked)
            .ok_or(CHANGED)?;
        slot.stored.draft = Default::default();
        slot.stored.sending = false;
        slot.stored.uncertain = false;
        slot.stored.position = 0;
        slot.stored.anchor = 0;
        slot.stored.form_state.remove("initialized");
        checkpoint(slot, &composers.0.store)?;
        slot.rev += 1;
        slot.error.clear();
        Ok(view(slot, true))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    const ID: &str = "11111111-1111-4111-8111-111111111111";
    fn detail(title: &str) -> Value {
        json!({"uuid":ID,"project":"example","title":title,"writable":true})
    }
    fn edit(slot: &mut Slot, text: &str) {
        edited(
            slot,
            Edit {
                handle: slot.handle,
                rev: slot.rev + 1,
                text: text.into(),
                position: 5,
                anchor: 2,
            },
        )
        .unwrap();
    }
    #[test]
    fn title_restore_empty_draft_cursor_and_unknown_fields_survive_reopening() {
        let root = crate::composer::tests::dir("task-form-restore");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let mut slot = opened(
            &forms,
            &store,
            Kind::Title,
            "example".into(),
            ID.into(),
            |_, _| Ok(detail("Canonical")),
        )
        .unwrap();
        edit(&mut slot, "Imported title");
        slot.stored
            .form_state
            .insert("private".into(), json!({"keep":true}));
        checkpoint(&mut slot, &store).unwrap();
        let mut restored = opened(
            &forms,
            &store,
            Kind::Title,
            "example".into(),
            ID.into(),
            |_, _| panic!("must not replace a saved draft"),
        )
        .unwrap();
        assert_eq!(restored.stored.draft.text, "Imported title");
        assert_eq!(restored.stored.position, 5);
        assert_eq!(restored.stored.anchor, 2);
        assert_eq!(restored.stored.form_state["private"]["keep"], true);
        edit(&mut restored, "");
        checkpoint(&mut restored, &store).unwrap();
        let empty = opened(
            &forms,
            &store,
            Kind::Title,
            "example".into(),
            ID.into(),
            |_, _| panic!("empty edits stay empty"),
        )
        .unwrap();
        assert!(empty.stored.draft.text.is_empty());
        drop(store);
        drop(root);
    }
    #[test]
    fn uncertain_title_is_checked_by_read_and_never_replayed() {
        let root = crate::composer::tests::dir("task-form-title-unknown");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let mut slot = opened(
            &forms,
            &store,
            Kind::Title,
            "example".into(),
            ID.into(),
            |_, _| Ok(detail("Old")),
        )
        .unwrap();
        edit(&mut slot, "New title");
        let calls = RefCell::new(vec![]);
        assert!(submitted(
            &mut slot,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| Ok(detail("Old")),
            |call| {
                calls.borrow_mut().push(call.clone());
                Err("uncertain".into())
            }
        )
        .is_err());
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(
            calls.borrow()[0],
            TaskCall::rename("example", ID, "New title")
        );
        assert!(submitted(
            &mut slot,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| Ok(detail("Other title")),
            |_| panic!("never replay unknown title")
        )
        .is_err());
        assert!(
            submitted(
                &mut slot,
                &store,
                false,
                false,
                TaskTarget::default(),
                |_, _| Ok(detail("New title")),
                |_| panic!("already confirmed by read")
            )
            .unwrap()
            .0
        );
        assert!(slot.stored.draft.text.is_empty());
        drop(store);
        drop(root);
    }
    #[test]
    fn creation_reserves_before_call_and_explicitly_recovers_exact_original() {
        let root = crate::composer::tests::dir("task-form-create-unknown");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let mut slot = opened(
            &forms,
            &store,
            Kind::NewCard,
            "example".into(),
            String::new(),
            |_, _| panic!("opening does not submit or read tasks"),
        )
        .unwrap();
        edit(&mut slot, "Named card");
        let first = RefCell::new(None);
        assert!(submitted(
            &mut slot,
            &store,
            false,
            true,
            TaskTarget::default(),
            |_, _| panic!(),
            |call| {
                let persisted = crate::task_editor::load(&store, "tasks/newCard/example/title")
                    .unwrap()
                    .draft;
                let record = &persisted.form_state["request"];
                assert!(call
                    .args
                    .contains(&record["request_id"].as_str().unwrap().to_owned()));
                assert!(call.args.iter().any(|a| a == "--backlog"));
                *first.borrow_mut() = Some(call.clone());
                Err("unknown outcome".into())
            }
        )
        .is_err());
        slot.stored.form_state.get_mut("request").unwrap()["private_unknown"] =
            json!({"keep":true});
        checkpoint(&mut slot, &store).unwrap();
        let mut restored = opened(
            &forms,
            &store,
            Kind::NewCard,
            "example".into(),
            String::new(),
            |_, _| panic!(),
        )
        .unwrap();
        let (_, original) = request(&restored).unwrap();
        assert_eq!(
            restored.stored.form_state["request"]["private_unknown"]["keep"],
            true
        );
        assert!(view(&restored, false)["read_only"].as_bool().unwrap());
        assert!(submitted(
            &mut restored,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |_| panic!("recovery must be explicit")
        )
        .is_err());
        let result=submitted(&mut restored,&store,true,false,TaskTarget::default(),|_,_|panic!(),|call|{
            assert_eq!(call,first.borrow().as_ref().unwrap());
            Ok(json!({"task":{"uuid":original.request_id,"project":"example","stage":"done","title":"Later title"}}).as_object().unwrap().clone())
        }).unwrap();
        assert_eq!(result, (true, Some(original.request_id)));
        assert!(!restored.stored.form_state.contains_key("request"));
        drop(store);
        drop(root);
    }
    #[test]
    fn malformed_reserved_identity_and_foreign_receipts_keep_the_draft() {
        let root = crate::composer::tests::dir("task-form-refused");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let mut slot = opened(
            &forms,
            &store,
            Kind::NewCard,
            "example".into(),
            String::new(),
            |_, _| panic!(),
        )
        .unwrap();
        edit(&mut slot, "Named card");
        assert!(submitted(
            &mut slot,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |_| Ok(json!({"task":{"uuid":ID,"project":"elsewhere"}})
                .as_object()
                .unwrap()
                .clone())
        )
        .is_err());
        assert!(slot.stored.form_state.contains_key("request"));
        assert_eq!(slot.stored.draft.text, "Named card");
        slot.stored.form_state.get_mut("request").unwrap()["project"] = json!("other");
        assert!(submitted(
            &mut slot,
            &store,
            true,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |_| panic!("invalid identity cannot submit")
        )
        .is_err());
        drop(store);
        drop(root);
    }
    #[test]
    fn unknown_creation_without_its_record_never_gets_a_new_identity() {
        let root = crate::composer::tests::dir("task-form-unknown-no-record");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let mut slot = opened(
            &forms,
            &store,
            Kind::NewCard,
            "example".into(),
            String::new(),
            |_, _| panic!(),
        )
        .unwrap();
        slot.stored.uncertain = true;
        edit(&mut slot, "Changed text cannot replace the lost identity");
        assert!(slot.stored.uncertain);
        assert!(view(&slot, false)["read_only"].as_bool().unwrap());
        assert!(submitted(
            &mut slot,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |_| panic!("unknown creation cannot reserve again")
        )
        .is_err());
        assert!(!slot.stored.form_state.contains_key("request"));
    }
    #[test]
    fn reserved_card_checkpoint_retry_keeps_its_identity_and_readonly_text() {
        let root = crate::composer::tests::dir("task-form-checkpoint-retry");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let mut slot = opened(
            &forms,
            &store,
            Kind::NewCard,
            "example".into(),
            String::new(),
            |_, _| panic!(),
        )
        .unwrap();
        edit(&mut slot, "  Named card  ");
        slot.stored.form_state.insert("request".into(), json!({"project":"example","title":"Named card","request_id":ID,"stage":"todo","session":"any","host":"","session_id":""}));
        slot.unsaved = true;
        let original = slot.stored.clone();
        let input = Edit {
            handle: slot.handle,
            rev: slot.rev,
            text: "Named card".into(),
            position: 2,
            anchor: 2,
        };
        edited(&mut slot, input).unwrap();
        checkpoint(&mut slot, &store).unwrap();
        assert_eq!(slot.stored.form_state, original.form_state);
        assert_eq!(slot.stored.draft, original.draft);
        assert!(view(&slot, false)["read_only"].as_bool().unwrap());
        let input = Edit {
            handle: slot.handle,
            rev: slot.rev + 1,
            text: "Other card".into(),
            position: 2,
            anchor: 2,
        };
        assert!(edited(&mut slot, input).is_err());
        assert_eq!(request(&slot).unwrap().1.request_id, ID);
    }
    #[test]
    fn replaced_form_checkpoint_cannot_reserve_or_submit_another_card() {
        let root = crate::composer::tests::dir("task-form-replaced");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let mut slot = opened(
            &forms,
            &store,
            Kind::NewCard,
            "example".into(),
            String::new(),
            |_, _| panic!(),
        )
        .unwrap();
        edit(&mut slot, "This editor's title");
        let mut newer = slot.stored.clone();
        newer
            .form_state
            .insert("request".into(), json!({"request_id":ID,"private":"kept"}));
        let (tx, rx) = mpsc::channel();
        store.save_durable(
            "tasks/newCard/example/title",
            newer.clone(),
            move |answer| {
                let _ = tx.send(answer);
            },
        );
        rx.recv().unwrap().unwrap();
        assert!(submitted(
            &mut slot,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |_| panic!("no task call after a checkpoint conflict")
        )
        .is_err());
        assert!(slot.blocked);
        assert_eq!(
            crate::task_editor::load(&store, "tasks/newCard/example/title")
                .unwrap()
                .draft,
            newer
        );
    }
    #[test]
    fn stale_and_invalid_title_never_write_or_erase_text() {
        let root = crate::composer::tests::dir("task-form-stale");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let mut slot = opened(
            &forms,
            &store,
            Kind::Title,
            "example".into(),
            ID.into(),
            |_, _| Ok(detail("Original")),
        )
        .unwrap();
        let bad = Edit {
            handle: slot.handle + 1,
            rev: 1,
            text: "Wrong".into(),
            position: 0,
            anchor: 0,
        };
        assert!(edited(&mut slot, bad).is_err());
        assert_eq!(slot.stored.draft.text, "Original");
        edit(&mut slot, "Two\nlines");
        assert!(submitted(
            &mut slot,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |_| panic!()
        )
        .is_err());
        assert_eq!(slot.stored.draft.text, "Two\nlines");
        drop(store);
        drop(root);
    }
}
