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
    /// The queue chosen in this submit, when the New task form names one.
    team: Option<String>,
    /// A card created for a team whose queue did not take it: the task, the
    /// team and the team's reason, for this submit's answer only.
    not_queued: Option<Value>,
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
    // A reserved card shows the team it was reserved for: recovery keeps it.
    let team = if slot.kind == Kind::NewCard && pending {
        slot.stored.form_state["request"]["team"].as_str()
    } else {
        None
    };
    json!({"project":slot.project,"task":slot.task,"mode":slot.kind,"handle":slot.handle,"rev":slot.rev,
        "text":if slot.kind==Kind::NewCard && pending {slot.stored.form_state["request"]["title"].as_str().unwrap_or("")} else {&slot.stored.draft.text},"position":slot.stored.position,"anchor":slot.stored.anchor,
        "files":[],"error":slot.error,"blocked":slot.blocked,"unsaved":slot.unsaved,
        "recovery":pending,"read_only":read_only,"saved":saved,"team":team})
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
        team: None,
        not_queued: None,
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
    /// The queue the card is reserved for (`task add --team`), when chosen.
    #[serde(default)]
    team: Option<String>,
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
        || parsed
            .team
            .as_deref()
            .is_some_and(|team| !uuid::Uuid::parse_str(team).is_ok())
        || (parsed.team.is_some() && parsed.session != "any")
    {
        return Err("The reserved card is invalid. Nothing was sent.".into());
    }
    Ok((raw, parsed))
}
/// The queue a New task submit names, set on the slot before anything is
/// reserved: a new card takes the chosen team (a canonical UUID), and a card
/// already reserved keeps the destination it was reserved with, so a submit
/// naming another team is refused. A title names no team.
fn destination(slot: &mut Slot, team: Option<String>) -> Result<(), String> {
    if team
        .as_deref()
        .is_some_and(|team| !zerus_client_core::creation::canonical_uuid(team))
    {
        return Err("Not a team id. Nothing was sent.".into());
    }
    if slot.kind == Kind::Title {
        return if team.is_some() {
            Err("A task title names no team. Nothing was sent.".into())
        } else {
            Ok(())
        };
    }
    if slot.stored.form_state.contains_key("request") {
        let reserved = request(slot)?.1.team;
        if team.is_some() && team != reserved {
            return Err("The reserved card keeps its team. Nothing was sent.".into());
        }
        slot.team = reserved;
    } else {
        slot.team = team;
    }
    Ok(())
}
/// A task created for a team whose queue did not confirm it: the command
/// line answers `task_exists: true` with `team.queued: false` and the team's
/// structured error (package 6's late refusal). The task exists, so the card
/// is done and its identity kept; the answer carries the team's reason so the
/// person can send that task to the team instead of creating another. Only
/// `queued: true` together with an `id` string equal to the chosen team
/// confirms the queue; any other team object, or none, keeps the notice, with
/// the command line's reason when it gave one.
fn not_queued(reply: &serde_json::Map<String, Value>, task: &str, team: &str) -> Option<Value> {
    let answer = reply.get("team").filter(|answer| answer.is_object());
    let queued = answer.is_some_and(|answer| {
        answer.get("queued").and_then(Value::as_bool) == Some(true)
            && answer.get("id").and_then(Value::as_str) == Some(team)
    });
    if queued {
        return None;
    }
    fn given(reason: Option<&str>) -> Option<&str> {
        reason.filter(|reason| !reason.trim().is_empty())
    }
    let reason = given(answer.and_then(|answer| answer["error"]["message"].as_str()))
        .or_else(|| given(reply.get("message").and_then(Value::as_str)))
        .unwrap_or("The team did not confirm that it queued the task.");
    let code = answer
        .and_then(|answer| answer["error"]["code"].as_str())
        .unwrap_or_default();
    Some(json!({"task": task, "team": team, "code": code, "reason": reason}))
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
    slot.not_queued = None;
    let team = slot.team.clone();
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
            slot.stored.form_state.insert("request".into(),json!({"project":slot.project,"title":wanted,"request_id":uuid::Uuid::new_v4().to_string(),"stage":if backlog {"backlog"} else {"todo"},"session":if target.session.is_empty(){"any"}else{&target.session},"host":target.host,"session_id":target.session_id,"team":team}));
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
        let add = |target: TaskTarget| {
            if let Some(team) = &intent.team {
                TaskCall::add_team_named(&intent.project, &intent.title, &intent.request_id, team)
            } else {
                TaskCall::add_named(
                    &intent.project,
                    &intent.title,
                    &intent.request_id,
                    &target,
                    intent.stage == "backlog",
                )
            }
        };
        run(&add(target)).and_then(|reply| {
            let task = tasks_write::created(&reply, &intent.request_id)?;
            if task["project"].as_str() != Some(&slot.project) {
                return Err(
                    "Task creation needs review: the reply did not match the reserved card.".into(),
                );
            }
            if let Some(team) = &intent.team {
                slot.not_queued = not_queued(&reply, &intent.request_id, team);
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
/// The New task form's submit, as the webview sends it: strict, so a field
/// this version does not know is refused, never left out of what is sent.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submit {
    kind: Kind,
    handle: u64,
    rev: u64,
    recover: bool,
    backlog: bool,
    target: Option<zerus_client_core::actions::Target>,
    catalog_key: Option<String>,
    session_id: Option<String>,
    /// A queue chosen in the New task form (`task add --team`).
    team: Option<String>,
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
        team,
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
        if kind == Kind::NewCard
            && team.is_some()
            && (target.is_some() || catalog_key.is_some() || session_id.is_some())
        {
            return Err("A queued card names no session. Nothing was sent.".into());
        }
        destination(slot, team.clone())?;
        let assignment = if kind == Kind::NewCard
            && !slot.stored.form_state.contains_key("request")
            && team.is_none()
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
        answer["not_queued"] = slot.not_queued.take().unwrap_or(Value::Null);
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
    /// Second review, finding 8: the New task form's chosen team reaches the
    /// reservation and the command (`task add --team`), a reserved card keeps
    /// the destination it was reserved with, and an older draft without a
    /// team still recovers to its original destination.
    #[test]
    fn the_chosen_team_is_reserved_and_sent_and_recovery_keeps_it() {
        const TEAM: &str = "33333333-3333-4333-8333-333333333333";
        const OTHER: &str = "44444444-4444-4444-8444-444444444444";
        let root = crate::composer::tests::dir("task-form-team");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let open = || {
            opened(
                &forms,
                &store,
                Kind::NewCard,
                "example".into(),
                String::new(),
                |_, _| panic!("a new card reads no task"),
            )
            .unwrap()
        };
        let mut slot = open();
        edit(&mut slot, "Queued card");
        assert!(destination(&mut slot, Some("not-a-team".into())).is_err());
        destination(&mut slot, Some(TEAM.into())).unwrap();
        let sent = RefCell::new(None);
        assert!(submitted(
            &mut slot,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |call| {
                let persisted = crate::task_editor::load(&store, "tasks/newCard/example/title")
                    .unwrap()
                    .draft;
                assert_eq!(
                    persisted.form_state["request"]["team"], TEAM,
                    "reserved before the call"
                );
                *sent.borrow_mut() = Some(call.clone());
                Err("unknown outcome".into())
            }
        )
        .is_err());
        let call = sent.borrow().clone().unwrap();
        let at = call
            .args
            .iter()
            .position(|arg| arg == "--team")
            .expect("the command names the team");
        assert_eq!(call.args[at + 1], TEAM);
        assert!(!call.args.iter().any(|arg| arg == "--target"));
        // Reopened, the reserved card shows its team and keeps it.
        let mut restored = open();
        assert_eq!(view(&restored, false)["team"], TEAM);
        assert!(
            destination(&mut restored, Some(OTHER.into())).is_err(),
            "another team is refused"
        );
        destination(&mut restored, None).unwrap();
        let result = submitted(
            &mut restored,
            &store,
            true,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |again| {
                assert_eq!(again, &call, "recovery sends the same reserved card");
                let id = again.args[again
                    .args
                    .iter()
                    .position(|arg| arg == "--request-id")
                    .unwrap()
                    + 1]
                .clone();
                Ok(json!({"task":{"uuid":id,"project":"example","stage":"todo","title":"Queued card"}}).as_object().unwrap().clone())
            },
        );
        assert!(result.unwrap().0);
        // An older reserved draft without a team recovers to any session.
        let mut older = open();
        edit(&mut older, "Older card");
        older.stored.form_state.insert("schema".into(), json!(1));
        older.stored.form_state.insert("request".into(), json!({"project":"example","title":"Older card","request_id":"55555555-5555-4555-8555-555555555555","stage":"todo","session":"any","host":"","session_id":""}));
        checkpoint(&mut older, &store).unwrap();
        let mut older = open();
        assert_eq!(view(&older, false)["team"], Value::Null);
        destination(&mut older, None).unwrap();
        assert!(
            destination(&mut older, Some(TEAM.into())).is_err(),
            "an older draft keeps its destination"
        );
        destination(&mut older, None).unwrap();
        let older_call = RefCell::new(None);
        let _ = submitted(
            &mut older,
            &store,
            true,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |call| {
                *older_call.borrow_mut() = Some(call.clone());
                Err("unknown outcome".into())
            },
        );
        let older_call = older_call.borrow().clone().unwrap();
        assert!(!older_call.args.iter().any(|arg| arg == "--team"));
        assert!(older_call.args.iter().any(|arg| arg == "--target"));
        drop(store);
        drop(root);
    }
    /// Second confirmation, finding 6: the command line created the task and
    /// the team's queue refused it (`task_exists`, `team.queued: false`, the
    /// team's error and a message, package 6's late refusal). The card is
    /// done with the task's identity kept, and the answer says it was not
    /// queued with the team's reason; nothing invites a second card.
    #[test]
    fn a_task_created_but_not_queued_keeps_its_identity_and_the_teams_reason() {
        const TEAM: &str = "33333333-3333-4333-8333-333333333333";
        let root = crate::composer::tests::dir("task-form-not-queued");
        let store = crate::composer::tests::drafts(&root);
        let forms = TaskForms::default();
        let open = || {
            opened(
                &forms,
                &store,
                Kind::NewCard,
                "example".into(),
                String::new(),
                |_, _| panic!("a new card reads no task"),
            )
            .unwrap()
        };
        let request_of = |call: &TaskCall| {
            call.args[call
                .args
                .iter()
                .position(|arg| arg == "--request-id")
                .unwrap()
                + 1]
            .clone()
        };
        let mut slot = open();
        edit(&mut slot, "Queued card");
        destination(&mut slot, Some(TEAM.into())).unwrap();
        let created = submitted(&mut slot, &store, false, false, TaskTarget::default(), |_, _| panic!(), |call| {
            let id = request_of(call);
            Ok(json!({
                "task": {"uuid": id, "project": "example", "ref": "example/7"},
                "task_exists": true,
                "team": {"id": TEAM, "queued": false, "error": {"code": "invalid_state", "message": "The team is stopped."}},
                "message": "Task example/7 exists and was not queued (invalid_state): The team is stopped.",
            }).as_object().unwrap().clone())
        })
        .unwrap();
        let id = created
            .1
            .clone()
            .expect("the created task's identity is kept");
        assert!(created.0);
        assert!(
            !slot.stored.form_state.contains_key("request"),
            "the card is done: no recovery creates it again"
        );
        assert_eq!(
            slot.not_queued,
            Some(
                json!({"task": id, "team": TEAM, "code": "invalid_state", "reason": "The team is stopped."})
            )
        );
        // A reply that queued it carries nothing; one that says nothing about
        // the team is no confirmation.
        let mut queued = open();
        edit(&mut queued, "Queued card");
        destination(&mut queued, Some(TEAM.into())).unwrap();
        submitted(&mut queued, &store, false, false, TaskTarget::default(), |_, _| panic!(), |call| {
            Ok(json!({"task": {"uuid": request_of(call), "project": "example"}, "task_exists": true, "team": {"id": TEAM, "queued": true}}).as_object().unwrap().clone())
        })
        .unwrap();
        assert_eq!(queued.not_queued, None);
        let mut silent = open();
        edit(&mut silent, "Queued card");
        destination(&mut silent, Some(TEAM.into())).unwrap();
        submitted(
            &mut silent,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |call| {
                Ok(
                    json!({"task": {"uuid": request_of(call), "project": "example"}})
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            },
        )
        .unwrap();
        assert_eq!(
            silent.not_queued.as_ref().unwrap()["reason"],
            "The team did not confirm that it queued the task."
        );
        // A card without a team never reports a queue.
        let mut plain = open();
        edit(&mut plain, "Plain card");
        destination(&mut plain, None).unwrap();
        submitted(
            &mut plain,
            &store,
            false,
            false,
            TaskTarget::default(),
            |_, _| panic!(),
            |call| {
                Ok(
                    json!({"task": {"uuid": request_of(call), "project": "example"}})
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            },
        )
        .unwrap();
        assert_eq!(plain.not_queued, None);
        drop(store);
        drop(root);
    }
    /// Third confirmation, finding 1: only `queued: true` with an `id` string
    /// equal to the chosen team confirms the queue. Every other shape keeps
    /// the notice, with the command line's reason when it gave one.
    #[test]
    fn only_the_chosen_teams_queued_receipt_confirms_the_queue() {
        const TEAM: &str = "33333333-3333-4333-8333-333333333333";
        const OTHER: &str = "44444444-4444-4444-8444-444444444444";
        const TASK: &str = "55555555-5555-4555-8555-555555555555";
        const GENERIC: &str = "The team did not confirm that it queued the task.";
        let task = json!({"uuid": TASK, "project": "example"});
        let cases: Vec<(&str, Value, Option<&str>)> = vec![
            (
                "confirmed",
                json!({"task": task, "team": {"id": TEAM, "queued": true}}),
                None,
            ),
            (
                "confirmed beside a message",
                json!({"task": task, "team": {"id": TEAM, "queued": true, "observation_pending": true}, "message": "Task was queued; its first observation is pending."}),
                None,
            ),
            (
                "no id",
                json!({"task": task, "team": {"queued": true}}),
                Some(GENERIC),
            ),
            (
                "a numeric id",
                json!({"task": task, "team": {"queued": true, "id": 27}}),
                Some(GENERIC),
            ),
            (
                "a null id",
                json!({"task": task, "team": {"queued": true, "id": null}}),
                Some(GENERIC),
            ),
            (
                "another team",
                json!({"task": task, "team": {"queued": true, "id": OTHER}}),
                Some(GENERIC),
            ),
            (
                "queued as text",
                json!({"task": task, "team": {"queued": "true", "id": TEAM}}),
                Some(GENERIC),
            ),
            (
                "queued missing",
                json!({"task": task, "team": {"id": TEAM}}),
                Some(GENERIC),
            ),
            (
                "refused with a reason",
                json!({"task": task, "team": {"id": TEAM, "queued": false, "error": {"code": "invalid_state", "message": "The team is stopped."}}, "message": "Task example/7 exists and was not queued."}),
                Some("The team is stopped."),
            ),
            (
                "a blank error falls back to the message",
                json!({"task": task, "team": {"id": TEAM, "queued": false, "error": {"message": "  "}}, "message": "Task example/7 exists and was not queued."}),
                Some("Task example/7 exists and was not queued."),
            ),
            (
                "another team with the command line's message",
                json!({"task": task, "team": {"queued": true, "id": OTHER}, "message": "Queued elsewhere."}),
                Some("Queued elsewhere."),
            ),
            (
                "a team that is not an object",
                json!({"task": task, "team": "queued", "message": "Odd reply."}),
                Some("Odd reply."),
            ),
            (
                "a storage failure of the team store",
                json!({"task": task, "task_exists": true, "team": {"id": TEAM, "queued": false, "error": {"code": "storage_failed", "message": "The result is not known; repeat the same request with the same request ID."}}, "message": "Task example/7 exists and was not queued (storage_failed): The result is not known; repeat the same request with the same request ID."}),
                Some("The result is not known; repeat the same request with the same request ID."),
            ),
            (
                "a null team",
                json!({"task": task, "team": null}),
                Some(GENERIC),
            ),
            ("no team at all", json!({"task": task}), Some(GENERIC)),
        ];
        for (label, reply, reason) in cases {
            let notice = not_queued(reply.as_object().unwrap(), TASK, TEAM);
            match reason {
                None => assert_eq!(notice, None, "{label}"),
                Some(reason) => {
                    let notice = notice
                        .unwrap_or_else(|| panic!("{label}: the queue was taken as confirmed"));
                    assert_eq!(
                        (
                            notice["task"].as_str(),
                            notice["team"].as_str(),
                            notice["reason"].as_str()
                        ),
                        (Some(TASK), Some(TEAM), Some(reason)),
                        "{label}"
                    );
                }
            }
        }
    }
    /// The review's B2 on the sending side: the New task form's submit (which
    /// carries the team for `task add --team`) refuses a field this version
    /// does not know; the webview's own shape is read.
    #[test]
    fn a_submit_with_an_unknown_field_is_refused() {
        let sent = json!({"kind": "new_card", "handle": 1, "rev": 2, "recover": false, "backlog": false,
            "target": null, "catalog_key": null, "session_id": null, "team": "33333333-3333-4333-8333-333333333333"});
        assert!(serde_json::from_value::<Submit>(sent.clone()).is_ok());
        let mut later = sent;
        later["queue_position"] = json!("top");
        assert!(serde_json::from_value::<Submit>(later).is_err());
    }
    /// zerus-63rru.19: a storage failure of the team store keeps its code on
    /// the notice, so the form does not call the queue's answer a refusal.
    #[test]
    fn a_storage_failure_keeps_its_code_on_the_notice() {
        const TEAM: &str = "33333333-3333-4333-8333-333333333333";
        const TASK: &str = "55555555-5555-4555-8555-555555555555";
        let problem: Value = serde_json::from_str(include_str!(
            "../../../crates/team-plan/fixtures/problem-storage-failed.json"
        ))
        .unwrap();
        let reply = json!({"task": {"uuid": TASK, "project": "example"}, "task_exists": true, "team": {"id": TEAM, "queued": false, "error": problem}});
        let notice = not_queued(reply.as_object().unwrap(), TASK, TEAM).unwrap();
        assert_eq!(notice["code"], "storage_failed");
        assert_eq!(
            notice["reason"],
            "The result is not known; repeat the same request with the same request ID."
        );
    }
    /// The store's busy refusal on the queue: nothing was written, so the
    /// notice keeps the refusal's code (the form calls it not queued and offers
    /// Send to team again); a problem's detail is not the notice's reason.
    #[test]
    fn a_busy_store_keeps_its_code_and_a_detail_stays_out_of_the_reason() {
        const TEAM: &str = "33333333-3333-4333-8333-333333333333";
        const TASK: &str = "55555555-5555-4555-8555-555555555555";
        let mut busy: Value = serde_json::from_str(include_str!(
            "../../../crates/team-plan/fixtures/problem-store-busy.json"
        ))
        .unwrap();
        busy["detail"] = json!("lock held by another writer");
        let reply = json!({"task": {"uuid": TASK, "project": "example"}, "task_exists": true, "team": {"id": TEAM, "queued": false, "error": busy}});
        let notice = not_queued(reply.as_object().unwrap(), TASK, TEAM).unwrap();
        assert_eq!(notice["code"], "store_busy");
        assert_eq!(
            notice["reason"],
            "The team store is busy. Nothing was written; try again."
        );
        let printed: Value = serde_json::from_str(include_str!(
            "../../../crates/team-plan/fixtures/cli-storage-failed.json"
        ))
        .unwrap();
        let reply = json!({"task": {"uuid": TASK, "project": "example"}, "task_exists": true, "team": {"id": TEAM, "queued": false, "error": printed["error"]}});
        let notice = not_queued(reply.as_object().unwrap(), TASK, TEAM).unwrap();
        assert_eq!(notice["code"], "storage_failed");
        assert_eq!(
            notice["reason"],
            "The result is not known; repeat the same request with the same request ID."
        );
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
